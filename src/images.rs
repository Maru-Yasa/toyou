//! Album-art cache. GPUI on Linux has no HTTP client for `img(url)`, so thumbnails are
//! downloaded here and handed to `img` as encoded bytes (GPUI decodes them off-thread).
//!
//! Downloads run on a few dedicated threads rather than GPUI's background executor, so a
//! page full of artwork can't starve decoding or API requests. Thumbnails are requested
//! lazily, only once they scroll near the viewport (see `ui::MusicApp::thumb`).

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::Arc;

use gpui_kit::{Context, Image, ImageFormat, Task};

const FETCH_THREADS: usize = 6;
/// Loaded images kept in memory; the oldest are evicted beyond this. Covers are requested at
/// display size (see [`sized_url`]), so 300 of them is only a few tens of MB decoded.
const MAX_LOADED: usize = 300;

enum Slot {
    Loading,
    Ready(Arc<Image>),
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
        let (done_tx, done_rx) = flume::unbounded::<(String, Option<Image>)>();
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
                                Slot::Ready(Arc::new(image))
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
            if let Some(Slot::Ready(image)) = self.slots.remove(&url) {
                image.remove_asset(cx);
            }
        }
    }

    /// The image, if it has been downloaded. Never starts a download.
    pub fn peek(&self, url: &str) -> Option<Arc<Image>> {
        match self.slots.get(url) {
            Some(Slot::Ready(image)) => Some(image.clone()),
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

fn fetch(url: &str) -> Option<Image> {
    let mut bytes = Vec::new();
    ureq::get(url).call().ok()?.into_reader().take(4 << 20).read_to_end(&mut bytes).ok()?;
    let format = match bytes.as_slice() {
        [0xFF, 0xD8, ..] => ImageFormat::Jpeg,
        [0x89, b'P', b'N', b'G', ..] => ImageFormat::Png,
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => ImageFormat::Webp,
        [b'G', b'I', b'F', ..] => ImageFormat::Gif,
        _ => return None,
    };
    Some(Image::from_bytes(format, bytes))
}
