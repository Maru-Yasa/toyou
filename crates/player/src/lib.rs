//! toyou's own audio player: a song is resolved to YouTube's AAC stream straight from the
//! InnerTube `player` endpoint, downloaded in ranged chunks, decoded by symphonia and played
//! through rodio/cpal. No external programs.
//!
//! Threads:
//! - the *audio thread* owns the output device and the rodio `Player`, and handles commands;
//! - a *loader thread* per song resolves its stream and builds the decoder, so a slow
//!   network never blocks playback controls;
//! - a *download thread* per song fills an in-memory buffer the decoder reads from.
//!   rodio decodes inside the sound card's callback, so reads must not hit the network.

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use rodio::{Decoder, DeviceSinkBuilder, Source};
use serde_json::Value;

/// Download granularity for the stream buffer.
const CHUNK: u64 = 1 << 20;
/// How many times a chunk is requested before the download fails.
const DOWNLOAD_ATTEMPTS: u32 = 3;
/// Resolved stream URLs expire after ~6 hours; re-resolve well before that.
const RESOLVE_TTL: Duration = Duration::from_secs(60 * 60);
const TICK: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Default)]
pub struct PlaybackState {
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub idle: bool,
    pub volume: f64,
    /// Incremented each time a track plays to its natural end.
    pub finished_count: u64,
    /// Incremented each time a track fails to load; `last_error` says why.
    pub failed_count: u64,
    pub last_error: Option<String>,
}

enum PlayerCommand {
    Load { url: String, start_at: f64, paused: bool },
    Loaded { generation: u64, decoder: Decoder<HttpStream>, duration: f64, start_at: f64, paused: bool },
    LoadFailed { generation: u64, error: String },
    TogglePause,
    SeekTo(f64),
    SetVolume(f64),
    Stop,
}

pub struct Player {
    commands: flume::Sender<PlayerCommand>,
    state: Arc<Mutex<PlaybackState>>,
}

impl Player {
    pub fn spawn() -> Result<Self, String> {
        let state = Arc::new(Mutex::new(PlaybackState { idle: true, volume: 70.0, ..Default::default() }));
        let (commands, receiver) = flume::unbounded();
        let (ready_tx, ready_rx) = flume::bounded(1);

        let thread_state = state.clone();
        let loopback = commands.clone();
        thread::Builder::new()
            .name("toyou-audio".into())
            .spawn(move || audio_thread(receiver, loopback, thread_state, ready_tx))
            .map_err(|e| e.to_string())?;

        // Report a missing/broken sound device right away rather than on first play.
        ready_rx.recv().map_err(|_| "audio thread exited".to_string())??;
        Ok(Self { commands, state })
    }

    pub fn state(&self) -> PlaybackState {
        self.state.lock().unwrap().clone()
    }

    pub fn load(&mut self, url: &str) {
        self.load_at(url, 0.0, false);
    }

    /// Loads a song and starts it `start_at` seconds in, optionally paused.
    pub fn load_at(&mut self, url: &str, start_at: f64, paused: bool) {
        let _ = self.commands.send(PlayerCommand::Load { url: url.to_string(), start_at, paused });
    }

    pub fn toggle_pause(&mut self) {
        let _ = self.commands.send(PlayerCommand::TogglePause);
    }

    pub fn seek_to(&mut self, seconds: f64) {
        let _ = self.commands.send(PlayerCommand::SeekTo(seconds));
    }

    pub fn set_volume(&mut self, volume: f64) {
        let _ = self.commands.send(PlayerCommand::SetVolume(volume));
    }

    pub fn stop(&mut self) {
        let _ = self.commands.send(PlayerCommand::Stop);
    }

    /// Resolves a song's stream in the background so playing it later starts faster.
    pub fn prefetch(&self, url: &str) {
        let url = url.to_string();
        thread::spawn(move || {
            let _ = resolve(&url);
        });
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Dropping the last sender ends the audio thread, which releases the device.
        let _ = self.commands.send(PlayerCommand::Stop);
    }
}

fn audio_thread(
    receiver: flume::Receiver<PlayerCommand>,
    loopback: flume::Sender<PlayerCommand>,
    state: Arc<Mutex<PlaybackState>>,
    ready: flume::Sender<Result<(), String>>,
) {
    let mut sink = match DeviceSinkBuilder::open_default_sink() {
        Ok(sink) => sink,
        Err(e) => {
            let _ = ready.send(Err(format!("couldn't open the sound device: {e}")));
            return;
        }
    };
    sink.log_on_drop(false);
    let player = rodio::Player::connect_new(sink.mixer());
    player.set_volume(0.7);
    let _ = ready.send(Ok(()));

    // Bumped on every load, so results of a superseded load are ignored.
    let mut generation = 0u64;
    let mut loading = false;
    let mut has_track = false;

    loop {
        let command = match receiver.recv_timeout(TICK) {
            Ok(command) => Some(command),
            Err(flume::RecvTimeoutError::Timeout) => None,
            // Only the loopback sender is left once the `Player` is dropped.
            Err(flume::RecvTimeoutError::Disconnected) => break,
        };
        if receiver.sender_count() <= 1 {
            break;
        }

        match command {
            Some(PlayerCommand::Load { url, start_at, paused }) => {
                generation += 1;
                player.clear();
                player.play();
                loading = true;
                has_track = false;
                {
                    let mut s = state.lock().unwrap();
                    s.idle = false;
                    s.paused = false;
                    s.position = 0.0;
                    s.duration = 0.0;
                    s.last_error = None;
                }
                let (loopback, load_generation) = (loopback.clone(), generation);
                thread::spawn(move || {
                    let message = match open_with_retry(&url) {
                        Ok((decoder, duration)) => {
                            PlayerCommand::Loaded { generation: load_generation, decoder, duration, start_at, paused }
                        }
                        Err(error) => {
                            // The player bar shortens messages; keep the full one on the terminal.
                            eprintln!("toyou: couldn't play {url}: {error}");
                            PlayerCommand::LoadFailed { generation: load_generation, error }
                        }
                    };
                    let _ = loopback.send(message);
                });
            }
            Some(PlayerCommand::Loaded { generation: g, decoder, duration, start_at, paused }) if g == generation => {
                player.append(decoder);
                if start_at > 0.0 {
                    let _ = player.try_seek(Duration::from_secs_f64(start_at));
                }
                if paused {
                    player.pause();
                }
                loading = false;
                has_track = true;
                state.lock().unwrap().duration = duration;
            }
            Some(PlayerCommand::LoadFailed { generation: g, error }) if g == generation => {
                loading = false;
                let mut s = state.lock().unwrap();
                s.idle = true;
                s.failed_count += 1;
                s.last_error = Some(error);
            }
            Some(PlayerCommand::Loaded { .. } | PlayerCommand::LoadFailed { .. }) => {} // superseded
            Some(PlayerCommand::TogglePause) => {
                if player.is_paused() {
                    player.play();
                } else {
                    player.pause();
                }
            }
            Some(PlayerCommand::SeekTo(seconds)) => {
                let _ = player.try_seek(Duration::from_secs_f64(seconds.max(0.0)));
            }
            Some(PlayerCommand::SetVolume(volume)) => {
                player.set_volume((volume / 100.0).clamp(0.0, 1.5) as rodio::Float);
                state.lock().unwrap().volume = volume;
            }
            Some(PlayerCommand::Stop) => {
                generation += 1;
                player.clear();
                loading = false;
                has_track = false;
            }
            None => {}
        }

        let mut s = state.lock().unwrap();
        s.paused = player.is_paused();
        if has_track {
            s.position = player.get_pos().as_secs_f64();
            if player.empty() {
                has_track = false;
                s.finished_count += 1;
            }
        }
        s.idle = !has_track && !loading;
    }
}

// ---- Resolving and opening a stream ----------------------------------------------------

#[derive(Clone)]
struct Resolved {
    url: String,
    headers: Vec<(String, String)>,
    size: Option<u64>,
    duration: f64,
}

fn resolve_cache() -> &'static Mutex<HashMap<String, (Resolved, Instant)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Resolved, Instant)>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The YouTube client asked for streams. Its URLs work as they are: no signature to decode, no
/// proof-of-origin token, and every byte range is served (other clients cut off after ~1 MB).
const STREAM_CLIENT: &str = "VISIONOS";
const STREAM_CLIENT_ID: &str = "101";
const STREAM_CLIENT_VERSION: &str = "1.02";
const STREAM_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const INNERTUBE: &str = "https://www.youtube.com/youtubei/v1";
/// AAC in MP4, which symphonia decodes in pure Rust.
const AAC_ITAG: u64 = 140;

/// Finds the AAC stream of a song (`watch_url` is a `…/watch?v=<id>` link). No download
/// happens here.
fn resolve(watch_url: &str) -> Result<Resolved, String> {
    if let Some((resolved, at)) = resolve_cache().lock().unwrap().get(watch_url) {
        if at.elapsed() < RESOLVE_TTL {
            return Ok(resolved.clone());
        }
    }
    let video_id = watch_url
        .split(['?', '&'])
        .find_map(|part| part.strip_prefix("v="))
        .ok_or("not a song link")?;
    // YouTube refuses requests without a visitor id ("confirm you're not a bot"); if the
    // cached one stopped working, try once more with a fresh one.
    let resolved = player_request(video_id, &visitor_id(false)?)
        .or_else(|_| player_request(video_id, &visitor_id(true)?))?;
    resolve_cache().lock().unwrap().insert(watch_url.to_string(), (resolved.clone(), Instant::now()));
    Ok(resolved)
}

/// A guest visitor id: the session identity YouTube expects with every request.
/// Fetched once and reused; `fresh` replaces it.
fn visitor_id(fresh: bool) -> Result<String, String> {
    static VISITOR: Mutex<Option<String>> = Mutex::new(None);
    let mut cached = VISITOR.lock().unwrap();
    if let (false, Some(id)) = (fresh, cached.as_ref()) {
        return Ok(id.clone());
    }
    let response: Value = ureq::post(&format!("{INNERTUBE}/visitor_id?prettyPrint=false"))
        .timeout(Duration::from_secs(15))
        .send_json(serde_json::json!({
            "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20250101.01.00", "hl": "en" } }
        }))
        .map_err(|e| format!("couldn't reach YouTube: {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let id = response
        .pointer("/responseContext/visitorData")
        .and_then(Value::as_str)
        .ok_or("YouTube didn't return a visitor id")?
        .to_string();
    *cached = Some(id.clone());
    Ok(id)
}

fn player_request(video_id: &str, visitor: &str) -> Result<Resolved, String> {
    let body = serde_json::json!({
        "context": {
            "client": {
                "clientName": STREAM_CLIENT,
                "clientVersion": STREAM_CLIENT_VERSION,
                "deviceMake": "Apple",
                "deviceModel": "RealityDevice17,1",
                "osName": "visionOS",
                "osVersion": "26.5.23O471",
                "userAgent": STREAM_USER_AGENT,
                "visitorData": visitor,
                "hl": "en",
            }
        },
        "videoId": video_id,
        "contentCheckOk": true,
        "racyCheckOk": true,
    });
    let response: Value = ureq::post(&format!("{INNERTUBE}/player?prettyPrint=false"))
        .timeout(Duration::from_secs(20))
        .set("User-Agent", STREAM_USER_AGENT)
        .set("X-YouTube-Client-Name", STREAM_CLIENT_ID)
        .set("X-YouTube-Client-Version", STREAM_CLIENT_VERSION)
        .set("X-Goog-Visitor-Id", visitor)
        .set("Origin", "https://www.youtube.com")
        .send_json(body)
        .map_err(|e| format!("couldn't reach YouTube: {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;

    let status = response.pointer("/playabilityStatus/status").and_then(Value::as_str).unwrap_or("");
    if status != "OK" {
        let reason = response.pointer("/playabilityStatus/reason").and_then(Value::as_str);
        return Err(reason.unwrap_or("YouTube won't play this song").to_string());
    }
    let formats = response
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .ok_or("no audio streams for this song")?;
    let is_aac = |f: &&Value| {
        f.get("url").is_some() && f.get("mimeType").and_then(Value::as_str).is_some_and(|m| m.starts_with("audio/mp4"))
    };
    let format = formats
        .iter()
        .filter(is_aac)
        .find(|f| f.get("itag").and_then(Value::as_u64) == Some(AAC_ITAG))
        .or_else(|| formats.iter().filter(is_aac).max_by_key(|f| f.get("bitrate").and_then(Value::as_u64)))
        .ok_or("no AAC audio stream for this song")?;

    let number = |v: Option<&Value>| v.and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
    Ok(Resolved {
        url: format["url"].as_str().unwrap_or_default().to_string(),
        headers: vec![("User-Agent".into(), STREAM_USER_AGENT.into())],
        size: number(format.get("contentLength")),
        duration: number(format.get("approxDurationMs")).map_or(0.0, |ms| ms as f64 / 1000.0),
    })
}

/// Opens a song, trying once more with a fresh stream link and visitor id if the first attempt
/// fails: YouTube can refuse a link, or a download can drop while the decoder reads the header.
fn open_with_retry(watch_url: &str) -> Result<(Decoder<HttpStream>, f64), String> {
    open(watch_url).or_else(|first| {
        resolve_cache().lock().unwrap().remove(watch_url);
        let _ = visitor_id(true);
        thread::sleep(Duration::from_millis(300));
        open(watch_url).map_err(|second| if second == first { second } else { format!("{second} (first try: {first})") })
    })
}

fn open(watch_url: &str) -> Result<(Decoder<HttpStream>, f64), String> {
    let resolved = resolve(watch_url)?;
    let stream = HttpStream::open(&resolved)?;
    let byte_len = stream.len;
    let decoder = Decoder::builder()
        .with_data(stream)
        .with_byte_len(byte_len)
        .with_seekable(true)
        .with_hint("m4a")
        .with_mime_type("audio/mp4")
        .build()
        .map_err(|e| format!("couldn't decode the stream: {e}"))?;
    let duration = decoder.total_duration().map_or(resolved.duration, |d| d.as_secs_f64());
    Ok((decoder, duration))
}

// ---- HTTP stream buffer ----------------------------------------------------------------

#[derive(Default)]
struct Buffer {
    data: Vec<u8>,
    done: bool,
    error: Option<String>,
}

/// A seekable reader over a remote file that a background thread downloads into memory.
/// Reads wait only when they get ahead of the download.
struct HttpStream {
    shared: Arc<(Mutex<Buffer>, Condvar)>,
    len: u64,
    pos: u64,
}

impl HttpStream {
    fn open(resolved: &Resolved) -> Result<Self, String> {
        // The first chunk also tells us the total size when YouTube didn't list it.
        let (first, total) = fetch_range(resolved, 0, CHUNK - 1)?;
        let len = resolved.size.or(total).ok_or("unknown stream size")?;
        let mut buffer = Buffer { data: Vec::with_capacity(len as usize), ..Default::default() };
        buffer.data.extend_from_slice(&first);
        buffer.done = buffer.data.len() as u64 >= len;
        let shared = Arc::new((Mutex::new(buffer), Condvar::new()));

        let (download, resolved) = (Arc::downgrade(&shared), resolved.clone());
        thread::spawn(move || {
            let mut offset = first.len() as u64;
            while offset < len {
                // Stop if the song was skipped and the reader dropped.
                let Some(shared) = download.upgrade() else { return };
                let end = (offset + CHUNK).min(len) - 1;
                // A chunk gets a few tries before the song gives up: connections do drop.
                let mut result = fetch_range(&resolved, offset, end);
                for attempt in 1..DOWNLOAD_ATTEMPTS {
                    if result.is_ok() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(400 * attempt as u64));
                    result = fetch_range(&resolved, offset, end);
                }
                let (lock, ready) = &*shared;
                let mut buffer = lock.lock().unwrap();
                match result {
                    Ok((bytes, _)) if !bytes.is_empty() => {
                        offset += bytes.len() as u64;
                        buffer.data.extend_from_slice(&bytes);
                    }
                    Ok(_) => buffer.error = Some("the stream ended early".into()),
                    Err(e) => buffer.error = Some(e),
                }
                let failed = buffer.error.is_some();
                buffer.done = failed || offset >= len;
                ready.notify_all();
                if failed {
                    return;
                }
            }
        });
        Ok(Self { shared, len, pos: 0 })
    }
}

impl Read for HttpStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len || out.is_empty() {
            return Ok(0);
        }
        let (lock, ready) = &*self.shared;
        let mut buffer = lock.lock().unwrap();
        while (buffer.data.len() as u64) <= self.pos && !buffer.done {
            buffer = ready.wait(buffer).unwrap();
        }
        let available = buffer.data.len() as u64;
        if self.pos >= available {
            let message = buffer.error.clone().unwrap_or_else(|| "stream ended".into());
            return Err(io::Error::other(message));
        }
        let start = self.pos as usize;
        let n = out.len().min(available as usize - start);
        out[..n].copy_from_slice(&buffer.data[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for HttpStream {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(n) => self.pos as i64 + n,
            SeekFrom::End(n) => self.len as i64 + n,
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

/// GETs `start..=end` of the stream. Returns the bytes and, if reported, the total size.
fn fetch_range(resolved: &Resolved, start: u64, end: u64) -> Result<(Vec<u8>, Option<u64>), String> {
    let mut request = ureq::get(&resolved.url).timeout(Duration::from_secs(20));
    for (name, value) in &resolved.headers {
        request = request.set(name, value);
    }
    let response = request
        .set("Range", &format!("bytes={start}-{end}"))
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let total = response
        .header("Content-Range")
        .and_then(|range| range.rsplit('/').next())
        .and_then(|total| total.parse().ok());
    let mut bytes = Vec::with_capacity((end - start + 1) as usize);
    response.into_reader().read_to_end(&mut bytes).map_err(|e| format!("download failed: {e}"))?;
    Ok((bytes, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hits YouTube; run with `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_decodes_and_seeks_youtube_aac() {
        let url = "https://music.youtube.com/watch?v=ZFZM6jDTWd4";
        let started = Instant::now();
        let (mut decoder, duration) = open(url).unwrap();
        println!("opened in {:?}; duration {duration:.1}s, {} Hz, {} ch", started.elapsed(), decoder.sample_rate(), decoder.channels());
        assert!(duration > 500.0);
        let one_second = decoder.sample_rate().get() as usize * decoder.channels().get() as usize;
        let samples = decoder.by_ref().take(one_second).count();
        assert_eq!(samples, one_second, "decoded the first second");
        decoder.try_seek(Duration::from_secs(300)).unwrap();
        let after_seek = decoder.by_ref().take(one_second).count();
        assert_eq!(after_seek, one_second, "decodes after seeking to 5:00");
        println!("seek ok; total {:?}", started.elapsed());
    }
}
