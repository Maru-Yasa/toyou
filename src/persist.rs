//! Remembers the playback session (up-next queue, current song, position, volume) across
//! restarts, in `~/.config/toyou/state.json`.

use serde::{Deserialize, Serialize};

use crate::api::Track;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SavedState {
    pub queue: Vec<Track>,
    pub current: Option<usize>,
    /// Seconds into the current song.
    pub position: f64,
    pub volume: f64,
}

fn path() -> Option<std::path::PathBuf> {
    crate::auth::config_dir().map(|dir| dir.join("state.json"))
}

pub fn load() -> Option<SavedState> {
    let state: SavedState = serde_json::from_slice(&std::fs::read(path()?).ok()?).ok()?;
    // Drop a current index that no longer points into the queue.
    let current = state.current.filter(|ix| *ix < state.queue.len());
    Some(SavedState { current, ..state })
}

/// Writes atomically (temp file + rename), so a crash mid-save never corrupts the file.
pub fn save(state: &SavedState) {
    let Some(path) = path() else { return };
    let Ok(json) = serde_json::to_vec(state) else { return };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, duration: &str) -> Track {
        Track {
            video_id: id.into(),
            title: format!("Song {id}"),
            artists: "Artist".into(),
            album: String::new(),
            duration: duration.into(),
            thumbnail: None,
        }
    }

    #[test]
    fn parses_listed_durations() {
        assert_eq!(track("a", "3:45").duration_secs(), Some(225.0));
        assert_eq!(track("a", "1:02:03").duration_secs(), Some(3723.0));
        assert_eq!(track("a", "").duration_secs(), None);
    }

    #[test]
    fn saves_and_restores_the_session() {
        // Use a throwaway config dir, never the user's real one.
        let dir = std::env::temp_dir().join(format!("toyou-persist-test-{}", std::process::id()));
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };

        save(&SavedState { queue: vec![track("a", "3:00"), track("b", "4:00")], current: Some(1), position: 42.5, volume: 55.0 });
        let restored = load().expect("state file");
        assert_eq!(restored.queue.len(), 2);
        assert_eq!(restored.queue[1].video_id, "b");
        assert_eq!(restored.current, Some(1));
        assert_eq!(restored.position, 42.5);
        assert_eq!(restored.volume, 55.0);

        // A stale index past the end of the queue is dropped.
        save(&SavedState { queue: vec![track("a", "3:00")], current: Some(5), ..Default::default() });
        assert_eq!(load().unwrap().current, None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
