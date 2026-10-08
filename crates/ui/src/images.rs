//! Album-art cache. GPUI on Linux has no HTTP client for `img(url)`, so thumbnails are
//! downloaded here and handed to `img` as encoded bytes (GPUI decodes them off-thread).
//!
//! Downloads run on a few dedicated threads rather than GPUI's background executor, so a
//! page full of artwork can't starve decoding or API requests. Thumbnails are requested
//! lazily, only once they scroll near the viewport (see `ui::MusicApp::thumb`).

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::Arc;

use gpui_kit::{Context, Hsla, Image, ImageFormat, Task, hsla};

const FETCH_THREADS: usize = 6;
/// Loaded images kept in memory; the oldest are evicted beyond this. Covers are requested at
/// display size (see [`sized_url`]), so 150 of them is only a few MB decoded.
const MAX_LOADED: usize = 150;

enum Slot {
    Loading,
    /// The image and its color (see [`tint_of`]).
    Ready(Arc<Image>, Option<Hsla>),
    Failed,
}

pub struct ImageCache {
    slots: HashMap<String, Slot>,
    /// Loaded URLs, oldest first, for eviction.
    loaded: VecDeque<String>,
    jobs: flume::Sender<String>,
    _deliver: Task<()>,
}

impl ImageCache {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (jobs, job_rx) = flume::unbounded::<String>();
        let (done_tx, done_rx) = flume::unbounded::<(String, Option<(Image, Option<Hsla>)>)>();
        for _ in 0..FETCH_THREADS {
            let (job_rx, done_tx) = (job_rx.clone(), done_tx.clone());
            // Workers exit once the cache (and so the job sender) is dropped.
            std::thread::spawn(move || {
                for url in job_rx.iter() {
                    let image = fetch(&url);
                    if done_tx.send((url, image)).is_err() {
                        break;
                    }
                }
            });
        }

        let deliver = cx.spawn(async move |this, cx| {
            while let Ok(first) = done_rx.recv_async().await {
                // Apply everything that has arrived so far in one update: one re-render.
                let batch: Vec<_> = std::iter::once(first).chain(done_rx.try_iter()).collect();
                let applied = this.update(cx, |this, cx| {
                    for (url, image) in batch {
                        let slot = match image {
                            Some(image) => {
                                this.loaded.push_back(url.clone());
                                Slot::Ready(Arc::new(image.0), image.1)
                            }
                            None => Slot::Failed,
                        };
                        this.slots.insert(url, slot);
                    }
                    this.evict(cx);
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        });

        Self { slots: HashMap::new(), loaded: VecDeque::new(), jobs, _deliver: deliver }
    }

    /// Drops the oldest images beyond [`MAX_LOADED`], including GPUI's decoded copy.
    /// An evicted image that is still on screen is simply downloaded again.
    fn evict(&mut self, cx: &mut Context<Self>) {
        while self.loaded.len() > MAX_LOADED {
            let Some(url) = self.loaded.pop_front() else { break };
            if let Some(Slot::Ready(image, _)) = self.slots.remove(&url) {
                image.remove_asset(cx);
            }
        }
    }

    /// The image, if it has been downloaded. Never starts a download.
    pub fn peek(&self, url: &str) -> Option<Arc<Image>> {
        match self.slots.get(url) {
            Some(Slot::Ready(image, _)) => Some(image.clone()),
            _ => None,
        }
    }

    /// The dominant color of a downloaded image, for tinting what surrounds it.
    pub fn tint(&self, url: &str) -> Option<Hsla> {
        match self.slots.get(url) {
            Some(Slot::Ready(_, tint)) => *tint,
            _ => None,
        }
    }

    /// Queues a download unless the image is already loaded, loading, or failed.
    pub fn request(&mut self, url: &str) {
        if !self.slots.contains_key(url) {
            self.slots.insert(url.to_string(), Slot::Loading);
            let _ = self.jobs.send(url.to_string());
        }
    }
}

/// Rewrites a YouTube artwork URL to ask for an image `size` pixels wide, instead of the
/// 226–544px versions the API hands out for covers we show at 36–180px.
pub fn sized_url(url: &str, size: u32) -> String {
    let resizable = ["googleusercontent.com/", "ggpht.com/"].iter().any(|host| url.contains(host));
    if resizable {
        // These URLs end in "=<options>", e.g. "=w226-h226-l90-rj" or "=s192".
        let base = match url.rfind('=') {
            Some(ix) if !url[ix..].contains('/') => &url[..ix],
            _ => url,
        };
        return format!("{base}=w{size}-h{size}-l90-rj");
    }
    // Video thumbnails (i.ytimg.com/vi/<id>/<name>.jpg): the 320px variant is plenty.
    if url.contains("i.ytimg.com/vi/") && size <= 320 {
        if let Some(ix) = url.rfind('/') {
            return format!("{}/mqdefault.jpg", &url[..ix]);
        }
    }
    url.to_string()
}

fn fetch(url: &str) -> Option<(Image, Option<Hsla>)> {
    let mut bytes = Vec::new();
    ureq::get(url).call().ok()?.into_reader().take(4 << 20).read_to_end(&mut bytes).ok()?;
    let format = match bytes.as_slice() {
        [0xFF, 0xD8, ..] => ImageFormat::Jpeg,
        [0x89, b'P', b'N', b'G', ..] => ImageFormat::Png,
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => ImageFormat::Webp,
        [b'G', b'I', b'F', ..] => ImageFormat::Gif,
        _ => return None,
    };
    let tint = tint_of(&bytes);
    Some((Image::from_bytes(format, bytes), tint))
}

/// An image's color: the average of a tiny copy, weighted toward saturated pixels so a
/// white or grey background doesn't wash it out, then nudged to a usable glow color.
fn tint_of(bytes: &[u8]) -> Option<Hsla> {
    let small = image::load_from_memory(bytes).ok()?.thumbnail(24, 24).to_rgb8();
    let (mut sum, mut weight) = ([0.0f32; 3], 0.0f32);
    for px in small.pixels() {
        let [r, g, b] = px.0.map(|c| c as f32 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let w = 0.15 + (max - min);
        sum = [sum[0] + r * w, sum[1] + g * w, sum[2] + b * w];
        weight += w;
    }
    if weight == 0.0 {
        return None;
    }
    let color: Hsla = gpui_kit::Rgba { r: sum[0] / weight, g: sum[1] / weight, b: sum[2] / weight, a: 1.0 }.into();
    Some(hsla(color.h, color.s.max(0.35), color.l.clamp(0.35, 0.6), 1.0))
}
