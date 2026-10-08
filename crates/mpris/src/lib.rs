//! MPRIS, the D-Bus interface Linux desktops use to talk to media players. With it, media
//! keys, Bluetooth earbuds (their taps arrive as media keys) and GNOME's media controls can
//! play, pause and skip, and show the current song.
//!
//! The D-Bus side runs on zbus's own thread. Commands are queued for the app to pick up on its
//! next tick ([`Mpris::commands`]); the app reports its state back with [`Mpris::update`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use zbus::blocking::Connection;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::{fdo, interface};

const BUS_NAME: &str = "org.mpris.MediaPlayer2.toyou";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
/// A jump bigger than this (seconds) is announced as a seek.
const SEEK_THRESHOLD: f64 = 2.0;

/// Something the desktop asked toyou to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    /// Jump to this many seconds into the song.
    SetPosition(f64),
    /// Move by this many seconds (negative is backwards).
    Seek(f64),
    /// 0.0 to 1.0.
    SetVolume(f64),
    /// Bring the window to the front.
    Raise,
}

/// The song toyou is on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub art_url: Option<String>,
    /// Seconds.
    pub length: f64,
}

/// What toyou is doing, as reported to the desktop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub playing: bool,
    pub track: Option<Track>,
    /// 0.0 to 1.0.
    pub volume: f64,
}

#[derive(Default)]
struct Shared {
    status: Status,
    /// Seconds into the song.
    position: f64,
}

pub struct Mpris {
    connection: Connection,
    shared: Arc<Mutex<Shared>>,
    commands: flume::Receiver<Command>,
    /// The last position reported, and when, to spot seeks.
    last_position: Option<(f64, Instant)>,
}

impl Mpris {
    /// Publishes toyou on the session bus.
    pub fn start() -> Result<Self, String> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (sender, commands) = flume::unbounded();
        let connection = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(BUS_NAME))
            .and_then(|b| b.serve_at(OBJECT_PATH, Root { commands: sender.clone() }))
            .and_then(|b| b.serve_at(OBJECT_PATH, Player { shared: shared.clone(), commands: sender }))
            .and_then(|b| b.build())
            .map_err(|e| format!("couldn't register media controls: {e}"))?;
        Ok(Self { connection, shared, commands, last_position: None })
    }

    /// Commands received since the last call.
    pub fn commands(&self) -> Vec<Command> {
        self.commands.try_iter().collect()
    }

    /// Reports the current state. Cheap to call often: the desktop is only told what changed.
    pub fn update(&mut self, status: Status, position: f64) {
        let changed = {
            let mut shared = self.shared.lock().unwrap();
            shared.position = position;
            if shared.status == status {
                None
            } else {
                let old = std::mem::replace(&mut shared.status, status.clone());
                Some(old)
            }
        };

        // A position that doesn't follow from the last one (plus elapsed time) was a seek.
        let expected = self.last_position.map(|(pos, at)| {
            if status.playing { pos + at.elapsed().as_secs_f64() } else { pos }
        });
        let seeked = expected.is_some_and(|expected| (position - expected).abs() > SEEK_THRESHOLD);
        let new_track = changed.as_ref().is_some_and(|old| old.track.as_ref().map(|t| &t.id) != status.track.as_ref().map(|t| &t.id));
        self.last_position = Some((position, Instant::now()));

        let Ok(iface) = self.connection.object_server().interface::<_, Player>(OBJECT_PATH) else { return };
        let emitter = iface.signal_emitter();
        zbus::block_on(async {
            if let Some(old) = &changed {
                let player = iface.get();
                if old.playing != status.playing || old.track.is_some() != status.track.is_some() {
                    let _ = player.playback_status_changed(emitter).await;
                }
                if old.track != status.track {
                    let _ = player.metadata_changed(emitter).await;
                }
                if old.volume != status.volume {
                    let _ = player.volume_changed(emitter).await;
                }
            }
            if seeked && !new_track {
                let _ = Player::seeked(emitter, micros(position)).await;
            }
        });
    }
}

fn micros(seconds: f64) -> i64 {
    (seconds.max(0.0) * 1_000_000.0) as i64
}

/// `org.mpris.MediaPlayer2`: who the player is.
struct Root {
    commands: flume::Sender<Command>,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        let _ = self.commands.send(Command::Raise);
    }

    fn quit(&self) {}

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        "toyou"
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "toyou"
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

/// `org.mpris.MediaPlayer2.Player`: playback state and controls.
struct Player {
    shared: Arc<Mutex<Shared>>,
    commands: flume::Sender<Command>,
}

impl Player {
    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.send(Command::Next);
    }

    fn previous(&self) {
        self.send(Command::Previous);
    }

    fn pause(&self) {
        self.send(Command::Pause);
    }

    fn play_pause(&self) {
        self.send(Command::PlayPause);
    }

    fn stop(&self) {
        self.send(Command::Stop);
    }

    fn play(&self) {
        self.send(Command::Play);
    }

    /// `offset` is in microseconds.
    fn seek(&self, offset: i64) {
        self.send(Command::Seek(offset as f64 / 1_000_000.0));
    }

    fn set_position(&self, _track_id: ObjectPath<'_>, position: i64) {
        self.send(Command::SetPosition(position as f64 / 1_000_000.0));
    }

    fn open_uri(&self, _uri: &str) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("toyou can't open URIs".into()))
    }

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> &str {
        let shared = self.shared.lock().unwrap();
        match (&shared.status.track, shared.status.playing) {
            (None, _) => "Stopped",
            (Some(_), true) => "Playing",
            (Some(_), false) => "Paused",
        }
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let shared = self.shared.lock().unwrap();
        let Some(track) = &shared.status.track else { return HashMap::new() };
        let mut map = HashMap::new();
        let mut put = |key: &str, value: Value<'_>| {
            if let Ok(value) = OwnedValue::try_from(value) {
                map.insert(key.to_string(), value);
            }
        };
        // Track ids must be valid D-Bus object paths.
        let id: String = track.id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        if let Ok(path) = ObjectPath::try_from(format!("/org/toyou/track/t{id}")) {
            put("mpris:trackid", Value::from(path));
        }
        put("mpris:length", Value::from(micros(track.length)));
        put("xesam:title", Value::from(track.title.as_str()));
        put("xesam:artist", Value::from(track.artists.clone()));
        if !track.album.is_empty() {
            put("xesam:album", Value::from(track.album.as_str()));
        }
        if let Some(art) = &track.art_url {
            put("mpris:artUrl", Value::from(art.as_str()));
        }
        map
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.shared.lock().unwrap().status.volume
    }

    #[zbus(property)]
    fn set_volume(&self, volume: f64) {
        self.send(Command::SetVolume(volume.clamp(0.0, 1.0)));
    }

    /// Microseconds into the song. Not announced on change, per the MPRIS spec.
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        micros(self.shared.lock().unwrap().position)
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_control(&self) -> bool {
        true
    }
}
