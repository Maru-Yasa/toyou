//! The icons toyou draws, served to GPUI as an asset source.

use std::borrow::Cow;

use gpui_kit::assets::Assets;
use gpui_kit::*;

// Embed only the extra Lucide icons we use, on top of the default component set.
gpui_kit::assets::icon_assets!(
    AppIcons,
    [
        ChevronDown, ChevronUp, Compass, Disc3, ExternalLink, Gauge, House, Library, ListMusic, LogIn, LogOut, Music, Pause, Play, RefreshCw,
        Shuffle, SkipBack, SkipForward, Volume2, VolumeX
    ]
);

/// gpui-kit's default component icons plus the extra ones above.
pub struct AppAssets;

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
