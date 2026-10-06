mod api;
mod app;
mod auth;
mod images;
mod palette;
mod persist;
mod player;
mod ui;

use std::borrow::Cow;

use gpui_kit::assets::Assets;
use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;

use app::MusicApp;

// Embed only the extra Lucide icons we use, on top of the default component set.
gpui_kit::assets::icon_assets!(
    AppIcons,
    [
        Compass, Disc3, ExternalLink, Gauge, House, Library, ListMusic, LogIn, LogOut, Music, Pause, Play, RefreshCw,
        Shuffle, SkipBack, SkipForward, Volume2, VolumeX
    ]
);

struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match AppIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(AppIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

actions!(
    toyou,
    [
        TogglePlay,
        NextTrack,
        PrevTrack,
        GoBack,
        FocusSearch,
        ToggleFps,
        OpenSongPalette,
        OpenCommandPalette,
        GoHome,
        GoExplore,
        GoLibrary,
        ToggleQueue,
        ShuffleUpNext,
        RefreshPage,
        SignIn,
        SignOut,
    ]
);

fn main() {
    gpui_kit::application().with_assets(AppAssets).run(|cx: &mut App| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-space", TogglePlay, None),
            KeyBinding::new("ctrl-.", NextTrack, None),
            KeyBinding::new("ctrl-,", PrevTrack, None),
            KeyBinding::new("alt-left", GoBack, None),
            KeyBinding::new("ctrl-k", FocusSearch, None),
            KeyBinding::new("f12", ToggleFps, None),
            KeyBinding::new("ctrl-p", OpenSongPalette, None),
            KeyBinding::new("ctrl-shift-p", OpenCommandPalette, None),
        ]);

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
