//! toyou: a lightweight YouTube Music desktop client built on GPUI Kit.

use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;
use views::MusicApp;

fn main() {
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
