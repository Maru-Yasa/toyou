//! Plain state types shared by the views: page loading, on-demand fetches, the Now playing
//! tab, the FPS meter, and queue shuffling.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::SharedString;
use music::{Page, Track};

/// Tabs of the Now playing view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NowTab {
    UpNext,
    Lyrics,
    Related,
}

/// Something fetched on demand for the Now playing view.
pub enum Fetch<T> {
    Loading,
    Ready(T),
    Failed(SharedString),
}

/// Debug FPS counter (F12, or `TOYOU_FPS=1`). While shown, the window redraws continuously,
/// so it reports how fast the current screen *can* be drawn, not only how often it changes.
#[derive(Default)]
pub struct FpsMeter {
    frames: VecDeque<Instant>,
    build_time: Duration,
}

impl FpsMeter {
    pub fn record_frame(&mut self, build_time: Duration) {
        let now = Instant::now();
        self.frames.push_back(now);
        while self.frames.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1)) {
            self.frames.pop_front();
        }
        self.build_time = build_time;
    }

    /// Frames in the last second, average frame time, and time spent building the UI tree.
    pub fn stats(&self) -> (usize, f32, f32) {
        let frame_ms = match (self.frames.front(), self.frames.back()) {
            (Some(first), Some(last)) if self.frames.len() > 1 => {
                last.duration_since(*first).as_secs_f32() * 1000.0 / (self.frames.len() - 1) as f32
            }
            _ => 0.0,
        };
        (self.frames.len(), frame_ms, self.build_time.as_secs_f32() * 1000.0)
    }
}

pub enum Load {
    Loading,
    Ready(Arc<Page>),
    Failed(SharedString),
}

/// Fisher–Yates with a tiny xorshift seeded from the clock; good enough for a playlist.
pub fn shuffle_tracks(tracks: &mut [Track]) {
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545F4914F6CDD1D)
        | 1;
    for i in (1..tracks.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        tracks.swap(i, (seed % (i as u64 + 1)) as usize);
    }
}
