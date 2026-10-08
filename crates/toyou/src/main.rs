//! toyou: a lightweight YouTube Music desktop client built on GPUI Kit.

use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;
use views::MusicApp;

fn main() {
    tune_allocator();
    gpui_kit::application().with_assets(icons::AppAssets).run(|cx: &mut App| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        input::bind_keys(cx);

        let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(900.0), px(560.0))),
            app_id: Some("toyou".into()),
            window_decorations: Some(WindowDecorations::Client),
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| MusicApp::new(window, cx)))
            .expect("failed to open window");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}

/// glibc's malloc keeps freed memory in a pool per thread and rarely hands it back. toyou has
/// ~40 threads and frees a lot of short-lived memory while drawing (layout, text shaping), so
/// the process grew long after its live data stopped growing. Cap the pools at two and give
/// freed memory back to the system every half minute.
#[cfg(target_os = "linux")]
fn tune_allocator() {
    // SAFETY: plain configuration calls into glibc; M_ARENA_MAX is set before other threads start.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
    std::thread::Builder::new()
        .name("toyou-trim".into())
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            // SAFETY: malloc_trim is thread-safe and only releases free memory.
            unsafe {
                libc::malloc_trim(0);
            }
        })
        .ok();
}

#[cfg(not(target_os = "linux"))]
fn tune_allocator() {}
