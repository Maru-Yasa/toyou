//! Zed-style palettes on top of gpui-kit's `Command` component:
//! - Ctrl+P searches YouTube Music (playlists when the query is empty) and plays/opens a result;
//! - Ctrl+Shift+P lists toyou's commands (navigation, playback, FPS counter, sign out, …).

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::IndexPath;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use music::{Item, Section};
use crate::app::MusicApp;
use ui::{Palette, thumb_element};
use input::{
    FocusSearch, GoBack, GoExplore, GoHome, GoLibrary, NextTrack, PrevTrack, RefreshPage, ShuffleUpNext, SignIn,
    SignOut, ToggleFps, ToggleNowPlaying, TogglePlay, ToggleQueue,
};

/// How long typing must pause before a search is sent.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq)]
pub enum PaletteKind {
    Songs,
    Commands,
}

pub struct PaletteState {
    kind: PaletteKind,
    command: Entity<CommandState>,
    /// Song palette results, grouped like the search page.
    sections: Vec<Section>,
    query: String,
    _search: Option<Task<()>>,
}

impl MusicApp {
    pub fn open_palette(&mut self, kind: PaletteKind, window: &mut Window, cx: &mut Context<Self>) {
        // Pressing the same shortcut again closes it, like Zed.
        if self.palette.as_ref().is_some_and(|palette| palette.kind == kind) {
            self.close_palette(window, cx);
            return;
        }
        let command = cx.new(|cx| CommandState::new(window, cx));
        command.update(cx, |state, cx| state.focus(window, cx));
        self.palette = Some(PaletteState {
            kind,
            command,
            sections: self.playlist_sections(),
            query: String::new(),
            _search: None,
        });
        cx.notify();
    }

    /// Closes the palette and gives keyboard focus back to the app. Otherwise focus would
    /// stay on the palette's now-removed search field, and shortcuts would go nowhere.
    pub fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// With an empty query, the song palette offers your playlists for quick jumping.
    pub(crate) fn playlist_sections(&self) -> Vec<Section> {
        if self.playlists.is_empty() {
            return Vec::new();
        }
        let items = self.playlists.iter().cloned().map(Item::Card).collect();
        vec![Section { title: "Your playlists".into(), items }]
    }

    pub(crate) fn palette_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let playlists = self.playlist_sections();
        let Some(palette) = &mut self.palette else { return };
        palette.query = query.trim().to_string();
        if palette.query.is_empty() {
            palette.sections = playlists;
            palette._search = None;
            cx.notify();
            return;
        }
        let query = palette.query.clone();
        let client = self.client.clone();
        palette._search = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let set_loading = |this: &WeakEntity<MusicApp>, loading: bool, cx: &mut AsyncWindowContext| {
                this.update_in(cx, |this, window, cx| {
                    if let Some(palette) = &this.palette {
                        palette.command.update(cx, |state, cx| state.set_loading(loading, window, cx));
                    }
                })
            };
            if set_loading(&this, true, cx).is_err() {
                return;
            }
            let request = cx.background_executor().spawn({
                let query = query.clone();
                async move { client.search(&query) }
            });
            let result = request.await;
            let _ = set_loading(&this, false, cx);
            this.update(cx, |this, cx| {
                let Some(palette) = &mut this.palette else { return };
                if palette.query != query {
                    return; // The user kept typing; a newer search is on its way.
                }
                palette.sections = result.map(|page| page.sections).unwrap_or_default();
                cx.notify();
            })
            .ok();
        }));
    }

    /// Plays or opens the chosen song-palette result.
    pub(crate) fn palette_confirm(&mut self, index: IndexPath, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.take() else { return };
        self.focus.focus(window, cx);
        cx.notify();
        if palette.kind != PaletteKind::Songs {
            return; // Commands are actions; the palette already dispatched it.
        }
        match palette.sections.get(index.section).and_then(|s| s.items.get(index.row)) {
            Some(Item::Track(track)) => self.play_radio(track.clone(), cx),
            Some(Item::Card(card)) => self.open_target(&card.target.clone(), cx),
            None => {}
        }
    }

    pub fn render_palette(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(palette) = &self.palette else { return div().into_any_element() };
        let p = Palette::new(cx);
        let this = cx.weak_entity();

        let mut command = Command::new(&palette.command)
            .on_query({
                let this = this.clone();
                move |query, window, cx| {
                    this.update(cx, |this, cx| this.palette_query(query, window, cx)).ok();
                }
            })
            .on_confirm({
                let this = this.clone();
                move |index, window, cx| {
                    this.update(cx, |this, cx| this.palette_confirm(index, window, cx)).ok();
                }
            })
            .on_cancel({
                let this = this.clone();
                move |window, cx| {
                    this.update(cx, |this, cx| this.close_palette(window, cx)).ok();
                }
            })
            .max_h(px(420.0));

        command = match palette.kind {
            PaletteKind::Songs => {
                let mut command = command
                    .filterable(false)
                    .placeholder("Search songs, albums, artists, playlists…")
                    .empty(move |state, _, cx| {
                        let message = if state.query(cx).is_empty() { "Type to search YouTube Music" } else { "No results" };
                        div().p_4().text_sm().text_color(p.muted_fg).child(message)
                    });
                for section in &palette.sections {
                    let mut group = CommandGroup::new().label(section.title.clone());
                    for item in &section.items {
                        group = group.item(self.song_row(item, p));
                    }
                    command = command.group(group);
                }
                command
            }
            PaletteKind::Commands => command.placeholder("Run a command…").items(self.commands()),
        };

        // A full-window backdrop: clicking outside the palette closes it.
        div()
            .id("palette-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .pt(px(72.0))
            .bg(p.bg.opacity(0.35))
            .occlude()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                this.update(cx, |this, cx| this.close_palette(window, cx)).ok();
            })
            .child(
                div()
                    .id("palette")
                    .w(px(600.0))
                    .h_full()
                    .max_h(px(520.0))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(command.bg(p.sidebar).shadow_lg()),
            )
            .into_any_element()
    }

    pub(crate) fn song_row(&self, item: &Item, p: Palette) -> CommandItem {
        let (images, scale) = (self.images.clone(), self.scale_factor.get());
        let (title, subtitle, thumbnail, kind, round) = match item {
            Item::Track(t) => {
                let subtitle = [t.artists.as_str(), t.album.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ");
                (t.title.clone(), subtitle, t.thumbnail.clone(), t.duration.clone(), false)
            }
            Item::Card(c) => (c.title.clone(), c.subtitle.clone(), c.thumbnail.clone(), String::new(), c.round),
        };
        CommandItem::new().label(title.clone()).keywords([subtitle.clone()]).child(move |_, cx| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .w_full()
                .child(thumb_element(&images, scale, thumbnail.as_ref(), px(32.0), round, p, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().text_sm().child(title.clone()))
                        .child(div().truncate().text_xs().text_color(p.muted_fg).child(subtitle.clone())),
                )
                .when(!kind.is_empty(), |el| el.child(div().text_xs().text_color(p.muted_fg).child(kind.clone())))
        })
    }

    /// Everything toyou can do, as palette entries. Each is an action, so the palette
    /// dispatches it on Enter and shows its keyboard shortcut.
    pub(crate) fn commands(&self) -> Vec<CommandItem> {
        let signed_in = self.session.is_some();
        let mut items = vec![
            entry("Go to Home", IconName::House, GoHome, &["navigate"]),
            entry("Go to Explore", IconName::Compass, GoExplore, &["navigate", "new releases", "moods", "charts"]),
        ];
        if signed_in {
            items.push(entry("Go to Library", IconName::Library, GoLibrary, &["navigate", "playlists"]));
        }
        items.extend([
            entry("Go back", IconName::ChevronLeft, GoBack, &["navigate", "previous page"]),
            entry("Show now playing", IconName::Disc3, ToggleNowPlaying, &["lyrics", "related", "up next", "artwork"]),
            entry("Play / pause", IconName::Play, TogglePlay, &["playback", "resume", "stop"]),
            entry("Next song", IconName::SkipForward, NextTrack, &["playback", "skip"]),
            entry("Previous song", IconName::SkipBack, PrevTrack, &["playback", "restart"]),
            entry("Shuffle up next", IconName::Shuffle, ShuffleUpNext, &["queue", "random"]),
            entry("Toggle up next panel", IconName::ListMusic, ToggleQueue, &["queue", "show", "hide"]),
            entry("Focus search", IconName::Search, FocusSearch, &["find"]),
            entry("Refresh page", IconName::RefreshCw, RefreshPage, &["reload"]),
            entry("Toggle FPS counter", IconName::Gauge, ToggleFps, &["debug", "performance", "frames"]),
        ]);
        items.push(if signed_in {
            entry("Sign out", IconName::LogOut, SignOut, &["log out", "logout", "account"])
        } else {
            entry("Sign in", IconName::LogIn, SignIn, &["log in", "login", "account", "google"])
        });
        items
    }
}

fn entry(label: &'static str, icon: IconName, action: impl Action, keywords: &[&'static str]) -> CommandItem {
    CommandItem::new()
        .label(label)
        .icon(icon)
        .action(Box::new(action))
        .keywords(keywords.iter().copied())
}
