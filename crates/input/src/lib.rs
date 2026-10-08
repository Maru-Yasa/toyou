//! Actions toyou responds to, and their default key bindings.

use gpui_kit::*;

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
        ToggleNowPlaying,
    ]
);

/// Registers the default shortcuts.
pub fn bind_keys(cx: &mut App) {
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
}
