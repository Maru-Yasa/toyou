//! toyou's window contents: the app state (`MusicApp`) and every screen, one module each.
//!
//! `MusicApp` and its render methods share this crate because Rust keeps a type's `impl`
//! blocks in the crate that defines it; plain state types live in `state` and `router`.

mod app;
mod login;
mod now_playing;
mod pages;
mod palette;
mod player_bar;
mod root;
mod sidebar;

pub use app::MusicApp;
