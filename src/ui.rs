//! Rendering for `MusicApp`: sidebar, pages, login, player bar and queue panel.

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonRounded, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, TitleBar,
};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

use crate::api::{Card, Item, Page, Target, Track};
use crate::app::{FpsMeter, Load, MusicApp, Route};
use crate::palette::PaletteKind;
use crate::auth::BROWSERS;
use crate::images::ImageCache;
use crate::{
    FocusSearch, GoBack, GoExplore, GoHome, GoLibrary, NextTrack, OpenCommandPalette, OpenSongPalette, PrevTrack,
    RefreshPage, ShuffleUpNext, SignIn, SignOut, ToggleFps, TogglePlay, ToggleQueue,
};

const TRACKS_COLLAPSED: usize = 6;
const CARDS_COLLAPSED: usize = 10;
const CARD_WIDTH: f32 = 168.0;

/// Theme colors copied out of the global so rendering can keep using `cx` mutably.
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub bg: Hsla,
    pub fg: Hsla,
    pub muted: Hsla,
    pub muted_fg: Hsla,
    pub border: Hsla,
    pub sidebar: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub primary: Hsla,
    pub secondary: Hsla,
    pub danger: Hsla,
}

impl Palette {
    pub fn new(cx: &App) -> Self {
        let t = cx.theme();
        Self {
            bg: t.background,
            fg: t.foreground,
            muted: t.muted,
            muted_fg: t.muted_foreground,
            border: t.border,
            sidebar: t.sidebar,
            hover: t.list_hover,
            active: t.list_active,
            primary: t.primary,
            secondary: t.secondary,
            danger: t.danger,
        }
    }
}

impl Render for MusicApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let build_started = std::time::Instant::now();
        self.scale_factor.set(window.scale_factor());
        let p = Palette::new(cx);
        let main = AnyView::from(self.page_view.clone())
            .cached(StyleRefinement::default().flex_1().min_h_0().w_full());
        let sidebar = AnyView::from(self.sidebar_view.clone())
            .cached(StyleRefinement::default().w(px(220.0)).flex_none().h_full());

        self.ensure_focus(window, cx);
        let app = div()
            .track_focus(&self.focus)
            // After any click, make sure something still holds focus for shortcuts.
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.ensure_focus(window, cx)))
            .on_action(cx.listener(|this, _: &TogglePlay, _, _| this.toggle_pause()))
            .on_action(cx.listener(|this, _: &NextTrack, _, cx| this.next(cx)))
            .on_action(cx.listener(|this, _: &PrevTrack, _, cx| this.previous(cx)))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _: &ToggleFps, _, cx| this.toggle_fps(cx)))
            .on_action(cx.listener(|this, _: &OpenSongPalette, window, cx| this.open_palette(PaletteKind::Songs, window, cx)))
            .on_action(cx.listener(|this, _: &OpenCommandPalette, window, cx| {
                this.open_palette(PaletteKind::Commands, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GoHome, _, cx| this.navigate(Route::Home, cx)))
            .on_action(cx.listener(|this, _: &GoExplore, _, cx| this.navigate(Route::Explore, cx)))
            .on_action(cx.listener(|this, _: &GoLibrary, _, cx| this.navigate(Route::Library, cx)))
            .on_action(cx.listener(|this, _: &ToggleQueue, _, cx| {
                this.show_queue = !this.show_queue;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShuffleUpNext, _, cx| this.shuffle_upcoming(cx)))
            .on_action(cx.listener(|this, _: &RefreshPage, _, cx| this.reload(cx)))
            .on_action(cx.listener(|this, _: &SignOut, _, cx| this.logout(cx)))
            .on_action(cx.listener(|this, _: &SignIn, _, cx| this.navigate(Route::Login, cx)))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.search_input.update(cx, |input, cx| input.focus(window, cx));
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .text_color(p.fg)
            .child(
                TitleBar::new().child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .child(div().text_color(p.primary).child(Icon::new(IconName::Disc3).small()))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("toyou"))
                        .when_some(self.current_track(), |el, track| {
                            el.child(div().text_color(p.muted_fg).truncate().child(format!("— {} · {}", track.title, track.artists)))
                        }),
                )
                .when_some(self.fps.as_ref().map(FpsMeter::stats), |el, (fps, frame_ms, build_ms)| {
                    el.child(
                        div()
                            .mr_3()
                            .px_2()
                            .rounded_sm()
                            .bg(p.muted)
                            .text_xs()
                            .font_family("monospace")
                            .text_color(if fps >= 55 { p.muted_fg } else { p.danger })
                            .child(format!("{fps:>3} fps · {frame_ms:5.1} ms/frame · build {build_ms:4.1} ms")),
                    )
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(self.render_top_bar(p, window, cx))
                            .child(main),
                    )
                    .when(self.show_queue, |el| el.child(self.render_queue(p, cx))),
            )
            .child(self.render_player_bar(p, cx));

        // On Linux (e.g. GNOME Wayland) the app draws its own frame. We skip gpui-kit's
        // `window_border`: its shadow margin isn't transparent on every compositor and shows up
        // as a box around the window. Edges for resizing are added here instead.
        let app = app
            .relative()
            .when(self.palette.is_some(), |el| el.child(self.render_palette(cx)))
            .children(resize_handles(window));

        if let Some(meter) = &mut self.fps {
            meter.record_frame(build_started.elapsed());
            // Keep drawing so the meter reflects the cost of a frame, not idle time.
            window.request_animation_frame();
        }
        app
    }
}

impl MusicApp {
    /// The page area's content, rendered on behalf of the cached [`crate::app::PageView`].
    pub fn render_main(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        if self.route == Route::Login {
            self.render_login(p, cx).into_any_element()
        } else {
            self.render_page(p, cx)
        }
    }

    /// The sidebar, rendered on behalf of the cached [`crate::app::SidebarView`].
    pub fn render_sidebar_element(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        self.render_sidebar(p, cx).into_any_element()
    }

    // ---- Shared pieces ----------------------------------------------------------------

    /// Album art that downloads lazily: only once it is on screen.
    fn thumb(&self, url: Option<&String>, size: Pixels, round: bool, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        thumb_element(&self.images, self.scale_factor.get(), url, size, round, p, cx)
    }

    fn nav_item(&self, id: &'static str, icon: IconName, label: &'static str, route: Route, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.route == route;
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .text_sm()
            .text_color(if active { p.fg } else { p.muted_fg })
            .when(active, |el| el.bg(p.active).font_weight(FontWeight::MEDIUM))
            .hover(move |s| s.bg(p.hover).text_color(p.fg))
            .on_click(cx.listener(move |this, _, _, cx| this.navigate(route.clone(), cx)))
            .child(Icon::new(icon))
            .child(label)
    }

    fn render_sidebar(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let account = match (&self.session, &self.account) {
            (Some(_), None) if self.session_expired => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_xs().text_color(p.danger).child("Your session expired."))
                .child(
                    Button::new("sign-in-again")
                        .primary()
                        .icon(IconName::LogIn)
                        .label("Sign in again")
                        .w_full()
                        .on_click(cx.listener(|this, _, _, cx| this.navigate(Route::Login, cx))),
                )
                .into_any_element(),
            (Some(_), account) => {
                let name = account.as_ref().map_or("Signed in".to_string(), |a| a.name.clone());
                let photo = account.as_ref().and_then(|a| a.photo.clone());
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.thumb(photo.as_ref(), px(32.0), true, p, cx))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(name))
                    .child(
                        Button::new("logout")
                            .ghost()
                            .small()
                            .icon(IconName::LogOut)
                            .tooltip("Sign out")
                            .on_click(cx.listener(|this, _, _, cx| this.logout(cx))),
                    )
                    .into_any_element()
            }
            (None, _) if self.route == Route::Login => div().into_any_element(),
            (None, _) => Button::new("sign-in")
                .primary()
                .icon(IconName::LogIn)
                .label("Sign in")
                .w_full()
                .on_click(cx.listener(|this, _, _, cx| this.navigate(Route::Login, cx)))
                .into_any_element(),
        };

        div()
            .w(px(220.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .pt_4()
            .bg(p.sidebar)
            .border_r_1()
            .border_color(p.border)
            .child(self.nav_item("nav-home", IconName::House, "Home", Route::Home, p, cx))
            .child(self.nav_item("nav-explore", IconName::Compass, "Explore", Route::Explore, p, cx))
            .when(self.session.is_some(), |el| {
                el.child(self.nav_item("nav-library", IconName::Library, "Library", Route::Library, p, cx))
            })
            .child(self.render_sidebar_playlists(p, cx))
            .child(account)
    }

    /// The user's saved and created playlists; fills the space between nav and account.
    fn render_sidebar_playlists(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().flex().flex_col().gap_0p5();
        for (ix, card) in self.playlists.iter().enumerate() {
            let active = matches!((&card.target, &self.route), (
                Target::Browse { id, .. },
                Route::Browse { id: open, .. },
            ) if id == open);
            let target = card.target.clone();
            list = list.child(
                div()
                    .id(("sidebar-playlist", ix))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .when(active, |el| el.bg(p.active))
                    .hover(move |s| s.bg(p.hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_target(&target, cx)))
                    .child(self.thumb(card.thumbnail.as_ref(), px(36.0), false, p, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .text_color(if active { p.fg } else { p.fg.opacity(0.9) })
                                    .child(card.title.clone()),
                            )
                            .child(div().truncate().text_xs().text_color(p.muted_fg).child(card.subtitle.clone())),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .when(!self.playlists.is_empty(), |el| {
                el.child(div().h(px(1.0)).mx_2().my_3().bg(p.border))
                    .child(
                        div()
                            .px_3()
                            .pb_2()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.muted_fg)
                            .child("PLAYLISTS"),
                    )
                    .child(div().id("sidebar-playlists").flex_1().min_h_0().overflow_y_scroll().pb_2().child(list))
            })
    }

    fn render_top_bar(&self, p: Palette, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(60.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_6()
            .border_b_1()
            .border_color(p.border)
            .child(
                Button::new("back")
                    .ghost()
                    .icon(IconName::ChevronLeft)
                    .tooltip("Back (Alt+Left)")
                    .disabled(self.back.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
            )
            .child(
                div().w(px(440.0)).child(
                    Input::new(&self.search_input)
                        .cleanable(true)
                        .prefix(Icon::new(IconName::Search).small().text_color(p.muted_fg)),
                ),
            )
            .child(div().flex_1())
            .when(self.route != Route::Login, |el| {
                el.child(
                    Button::new("reload")
                        .ghost()
                        .icon(IconName::RefreshCw)
                        .tooltip("Refresh")
                        .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                )
            })
    }

    // ---- Pages ------------------------------------------------------------------------

    fn render_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let key = self.route.key();
        let body = match self.pages.get(&key) {
            None | Some(Load::Loading) => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .py_24()
                .child(Spinner::new().large())
                .into_any_element(),
            Some(Load::Failed(err)) => div()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .py_24()
                .text_color(p.muted_fg)
                .child(err.clone())
                .child(Button::new("retry").outline().label("Try again").on_click(cx.listener(|this, _, _, cx| this.reload(cx))))
                .into_any_element(),
            Some(Load::Ready(page)) => self.render_page_content(page.clone(), p, cx).into_any_element(),
        };
        div()
            .id(SharedString::from(key))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_6()
            .py_5()
            .child(body)
            .into_any_element()
    }

    fn page_title(&self) -> Option<(String, Option<String>)> {
        let greeting = self.account.as_ref().map(|a| format!("Picked for {}", a.name));
        match &self.route {
            Route::Home => Some(("Home".into(), greeting.or(Some("Popular right now".into())))),
            Route::Explore => Some(("Explore".into(), Some("New releases, charts, moods & genres".into()))),
            Route::Library => Some(("Library".into(), Some("Your playlists and liked music".into()))),
            Route::Search(q) => Some((format!("Results for “{q}”"), None)),
            Route::Browse { .. } | Route::Login => None,
        }
    }

    fn render_page_content(&self, page: Arc<Page>, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let is_detail = matches!(self.route, Route::Browse { .. }) && !page.tracks().is_empty();
        let mut content = div().flex().flex_col().gap_8();

        if let Some((title, subtitle)) = self.page_title() {
            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_2xl().font_weight(FontWeight::BOLD).child(title))
                    .when_some(subtitle, |el, s| el.child(div().text_sm().text_color(p.muted_fg).child(s))),
            );
        }
        if let Some(header) = &page.header {
            let track_count = page.tracks().len();
            content = content.child(
                div()
                    .flex()
                    .items_end()
                    .gap_6()
                    .child(self.thumb(header.thumbnail.as_ref(), px(180.0), false, p, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .min_w_0()
                            .child(div().text_3xl().font_weight(FontWeight::BOLD).line_clamp(2).child(header.title.clone()))
                            .child(div().text_color(p.muted_fg).child(header.subtitle.clone()))
                            .when(track_count > 0, |el| {
                                el.child(div().text_sm().text_color(p.muted_fg).child(format!("{track_count} songs")))
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .pt_2()
                                            .child(
                                                Button::new("play-all")
                                                    .primary()
                                                    .icon(IconName::Play)
                                                    .label("Play")
                                                    .on_click(cx.listener(|this, _, _, cx| this.play_from_page(0, false, cx))),
                                            )
                                            .child(
                                                Button::new("shuffle-all")
                                                    .outline()
                                                    .icon(IconName::Shuffle)
                                                    .label("Shuffle")
                                                    .on_click(cx.listener(|this, _, _, cx| this.play_from_page(0, true, cx))),
                                            ),
                                    )
                            }),
                    ),
            );
        }
        if page.sections.is_empty() {
            content = content.child(div().text_color(p.muted_fg).child("Nothing here yet."));
        }

        // Running index into `page.tracks()` so clicks on a detail page queue the whole page.
        let mut page_track_ix = 0;
        for (section_ix, section) in page.sections.iter().enumerate() {
            let expanded = is_detail || self.expanded.contains(&(self.route.key(), section_ix));
            let tracks: Vec<&Track> = section.items.iter().filter_map(|i| if let Item::Track(t) = i { Some(t) } else { None }).collect();
            let cards: Vec<&Card> = section.items.iter().filter_map(|i| if let Item::Card(c) = i { Some(c) } else { None }).collect();
            let limit = if tracks.is_empty() { CARDS_COLLAPSED } else { TRACKS_COLLAPSED };
            let collapsible = !is_detail && section.items.len() > limit;

            let mut block = div().flex().flex_col().gap_3();
            if !section.title.is_empty() || collapsible {
                block = block.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(section.title.clone()))
                        .when(collapsible, |el| {
                            el.child(
                                Button::new(("more", section_ix))
                                    .ghost()
                                    .small()
                                    .label(if expanded { "Show less" } else { "Show all" })
                                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_expanded(section_ix, cx))),
                            )
                        }),
                );
            }

            if !tracks.is_empty() {
                let shown = if expanded { tracks.len() } else { tracks.len().min(TRACKS_COLLAPSED) };
                let mut list = div().flex().flex_col();
                for (ix, track) in tracks.iter().take(shown).enumerate() {
                    let row_id = section_ix * 10_000 + ix;
                    let queue_ix = page_track_ix + ix;
                    let track = (*track).clone();
                    list = list.child(self.render_track_row(row_id, &track, is_detail.then_some(queue_ix + 1), p, cx).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if is_detail {
                                this.play_from_page(queue_ix, false, cx);
                            } else {
                                this.play_radio(track.clone(), cx);
                            }
                        }),
                    ));
                }
                block = block.child(list);
            }
            page_track_ix += tracks.len();

            if !cards.is_empty() {
                let shown = if expanded { cards.len() } else { cards.len().min(CARDS_COLLAPSED) };
                let mut grid = div().flex().flex_wrap().gap_2();
                for (ix, card) in cards.iter().take(shown).enumerate() {
                    let id = section_ix * 10_000 + ix;
                    grid = grid.child(if card.chip { self.render_chip(id, card, p, cx).into_any_element() } else { self.render_card(id, card, p, cx).into_any_element() });
                }
                block = block.child(grid);
            }
            content = content.child(block);
        }

        // Reaching the end of the page loads the next batch of sections (Home).
        if page.continuation.is_some() {
            let this = cx.entity().downgrade();
            content = content.child(
                div()
                    .h(px(64.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(self.is_loading_more(), |el| el.child(Spinner::new()))
                    .child(
                        when_visible(move |cx| {
                            let this = this.clone();
                            // Defer: we're mid-frame, and loading more re-renders the page.
                            cx.defer(move |cx| {
                                this.update(cx, |this, cx| this.load_more(cx)).ok();
                            });
                        })
                        .size(px(1.0)),
                    ),
            );
        }
        content
    }

    fn render_track_row(&self, id: usize, track: &Track, number: Option<usize>, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let playing = self.current_track().is_some_and(|t| t.video_id == track.video_id);
        let subtitle = [track.artists.as_str(), track.album.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ");
        div()
            .id(("track", id))
            .flex()
            .items_center()
            .gap_3()
            .h(px(56.0))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(p.hover))
            .when_some(number, |el, n| {
                el.child(
                    div()
                        .w(px(28.0))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .text_sm()
                        .text_color(if playing { p.primary } else { p.muted_fg })
                        .child(if playing { Icon::new(IconName::Volume2).small().into_any_element() } else { n.to_string().into_any_element() }),
                )
            })
            .child(self.thumb(track.thumbnail.as_ref(), px(40.0), false, p, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if playing { p.primary } else { p.fg })
                            .child(track.title.clone()),
                    )
                    .child(div().truncate().text_xs().text_color(p.muted_fg).child(subtitle)),
            )
            .child(div().flex_none().text_sm().text_color(p.muted_fg).child(track.duration.clone()))
    }

    fn render_card(&self, id: usize, card: &Card, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let target = card.target.clone();
        div()
            .id(("card", id))
            .w(px(CARD_WIDTH))
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_lg()
            .cursor_pointer()
            .hover(move |s| s.bg(p.hover))
            .on_click(cx.listener(move |this, _, _, cx| this.open_target(&target, cx)))
            .child(self.thumb(card.thumbnail.as_ref(), px(CARD_WIDTH - 16.0), card.round, p, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .when(card.round, |el| el.items_center())
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).line_clamp(2).child(card.title.clone()))
                    .child(div().text_xs().text_color(p.muted_fg).truncate().child(card.subtitle.clone())),
            )
    }

    fn render_chip(&self, id: usize, card: &Card, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let target = card.target.clone();
        div()
            .id(("chip", id))
            .w(px(CARD_WIDTH))
            .px_4()
            .py_3()
            .rounded_md()
            .bg(p.secondary)
            .border_l_4()
            .border_color(p.primary)
            .cursor_pointer()
            .truncate()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .hover(move |s| s.bg(p.hover))
            .on_click(cx.listener(move |this, _, _, cx| this.open_target(&target, cx)))
            .child(card.title.clone())
    }

    // ---- Login ------------------------------------------------------------------------

    fn render_login(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.login_busy.is_some();
        let mut browsers = div().flex().flex_wrap().gap_2();
        for (ix, (label, name)) in BROWSERS.iter().enumerate() {
            browsers = browsers.child(
                Button::new(("browser", ix))
                    .outline()
                    .small()
                    .label(*label)
                    .loading(self.login_busy == Some(*name))
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| this.login_with_browser(name, cx))),
            );
        }

        let other_logins = div()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(p.border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Import from a browser you're signed into"))
                    .child(div().text_xs().text_color(p.muted_fg).child("Uses yt-dlp to read that browser's YouTube session."))
                    .child(browsers),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Paste a Cookie header"))
                    .child(div().text_xs().text_color(p.muted_fg).child(
                        "In the browser's dev tools, open any music.youtube.com request and copy the value of its Cookie header.",
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.cookie_input)))
                            .child(
                                Button::new("use-cookie")
                                    .outline()
                                    .label("Sign in")
                                    .loading(self.login_busy == Some("cookie"))
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.login_with_cookie_header(cx))),
                            ),
                    ),
            );

        div()
            .id("login")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .justify_center()
            .py_10()
            .px_6()
            .child(
                div()
                    .w(px(520.0))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_color(p.primary).text_3xl().child(Icon::new(IconName::Disc3)))
                            .child(div().text_2xl().font_weight(FontWeight::BOLD).child("Sign in to YouTube Music"))
                            .child(div().text_sm().text_color(p.muted_fg).child(
                                "Get your personalized Home, your library and playlists. Your session is stored only on this computer.",
                            )),
                    )
                    .when_some(self.login_status.clone(), |el, (message, is_error)| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .border_1()
                                .border_color(if is_error { p.danger } else { p.border })
                                .text_sm()
                                .text_color(if is_error { p.danger } else { p.muted_fg })
                                .when(busy, |el| el.child(Spinner::new().small()))
                                .child(div().flex_1().child(message)),
                        )
                    })
                    .child(
                        Button::new("google")
                            .primary()
                            .large()
                            .w_full()
                            .icon(IconName::LogIn)
                            .label("Sign in with Google")
                            .loading(self.login_busy == Some("google"))
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.login_with_google(cx))),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(
                                Button::new("other-logins")
                                    .ghost()
                                    .small()
                                    .label(if self.show_other_logins { "Hide other ways to sign in" } else { "Other ways to sign in" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_other_logins = !this.show_other_logins;
                                        this.changed(cx);
                                    })),
                            )
                            .child(
                                Button::new("guest")
                                    .ghost()
                                    .small()
                                    .label("Continue without signing in")
                                    .on_click(cx.listener(|this, _, _, cx| this.continue_as_guest(cx))),
                            ),
                    )
                    .when(self.show_other_logins, |el| el.child(other_logins)),
            )
    }

    // ---- Player -----------------------------------------------------------------------

    fn render_player_bar(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let track = self.current_track().cloned();
        let state = &self.playback;
        let paused = state.paused || state.idle;
        let muted = state.volume <= 0.0;

        div()
            .h(px(88.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_6()
            .px_4()
            .bg(p.sidebar)
            .border_t_1()
            .border_color(p.border)
            .child(
                div()
                    .w(px(280.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(self.thumb(track.as_ref().and_then(|t| t.thumbnail.as_ref()), px(56.0), false, p, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(track.as_ref().map_or("Nothing playing".into(), |t| t.title.clone())),
                            )
                            .child(div().truncate().text_xs().text_color(p.muted_fg).child(
                                match (&self.player_error, &track) {
                                    (Some(err), _) => err.to_string(),
                                    (None, Some(t)) => t.artists.clone(),
                                    (None, None) => "Pick something from Home or Explore".into(),
                                },
                            )),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("shuffle")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Shuffle)
                                    .tooltip("Shuffle up next")
                                    .on_click(cx.listener(|this, _, _, cx| this.shuffle_upcoming(cx))),
                            )
                            .child(
                                Button::new("prev")
                                    .ghost()
                                    .icon(IconName::SkipBack)
                                    .tooltip("Previous (Ctrl+,)")
                                    .on_click(cx.listener(|this, _, _, cx| this.previous(cx))),
                            )
                            .child(
                                Button::new("play")
                                    .primary()
                                    .rounded(ButtonRounded::Large)
                                    .icon(if paused { IconName::Play } else { IconName::Pause })
                                    .tooltip("Play / Pause (Ctrl+Space)")
                                    .on_click(cx.listener(|this, _, _, _| this.toggle_pause())),
                            )
                            .child(
                                Button::new("next")
                                    .ghost()
                                    .icon(IconName::SkipForward)
                                    .tooltip("Next (Ctrl+.)")
                                    .on_click(cx.listener(|this, _, _, cx| this.next(cx))),
                            )
                            .child(
                                Button::new("queue-toggle")
                                    .ghost()
                                    .small()
                                    .icon(IconName::ListMusic)
                                    .tooltip("Up next")
                                    .selected(self.show_queue)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_queue = !this.show_queue;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .max_w(px(560.0))
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_xs()
                            .text_color(p.muted_fg)
                            .child(div().w(px(40.0)).text_right().child(format_time(state.position)))
                            .child(div().flex_1().child(Slider::new(&self.progress)))
                            .child(div().w(px(40.0)).child(format_time(state.duration))),
                    ),
            )
            .child(
                div()
                    .w(px(200.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(p.muted_fg)
                    .child(Icon::new(if muted { IconName::VolumeX } else { IconName::Volume2 }).small())
                    .child(div().flex_1().child(Slider::new(&self.volume))),
            )
    }

    fn render_queue(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().flex().flex_col();
        for (ix, track) in self.queue.iter().enumerate() {
            let row = self.render_track_row(ix, track, None, p, cx);
            list = list.child(row.on_click(cx.listener(move |this, _, _, cx| this.play_index(ix, cx))));
        }
        div()
            .w(px(320.0))
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(p.border)
            .bg(p.sidebar)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .h(px(60.0))
                    .border_b_1()
                    .border_color(p.border)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Up next"))
                    .child(div().text_xs().text_color(p.muted_fg).child(format!("{} songs", self.queue.len()))),
            )
            .child(
                div()
                    .id("queue")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .when(self.queue.is_empty(), |el| {
                        el.child(div().p_4().text_sm().text_color(p.muted_fg).child("Play a song and suggestions will show up here."))
                    })
                    .child(list),
            )
    }
}

/// Album art at `size`, downloaded lazily (only once it is on screen) at the size it's drawn.
pub(crate) fn thumb_element(
    images: &Entity<ImageCache>,
    scale_factor: f32,
    url: Option<&String>,
    size: Pixels,
    round: bool,
    p: Palette,
    cx: &App,
) -> AnyElement {
    let pixels = (f32::from(size) * scale_factor).ceil() as u32;
    let url = url.map(|url| crate::images::sized_url(url, pixels));
    let image = url.as_ref().and_then(|url| images.read(cx).peek(url));
    let frame = div()
        .size(size)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .bg(p.muted)
        .text_color(p.muted_fg)
        .map(|el| if round { el.rounded_full() } else { el.rounded_md() });
    match (image, url) {
        (Some(image), _) => frame.child(img(image).size_full().object_fit(ObjectFit::Cover)).into_any_element(),
        (None, Some(url)) => {
            let images = images.clone();
            frame
                .relative()
                .child(Icon::new(IconName::Music))
                .child(when_visible(move |cx| images.update(cx, |cache, _| cache.request(&url))).absolute().size_full())
                .into_any_element()
        }
        (None, None) => frame.child(Icon::new(IconName::Music)).into_any_element(),
    }
}

/// An invisible element that calls `on_visible` each frame any part of it is on screen.
/// GPUI has no visibility API, but during prepaint the window's content mask is the
/// intersection of every enclosing clip (scroll views included); off-screen it is empty.
fn when_visible(on_visible: impl Fn(&mut App) + 'static) -> Canvas<()> {
    canvas(
        move |bounds, window, cx| {
            let visible = window.content_mask().bounds.intersect(&bounds);
            if visible.size.width > px(0.0) && visible.size.height > px(0.0) {
                on_visible(cx);
            }
        },
        |_, _, _, _| {},
    )
}

/// Invisible strips along the window edges and corners that start an interactive resize.
fn resize_handles(window: &Window) -> Vec<AnyElement> {
    const HIT: f32 = 6.0;
    let Decorations::Client { tiling } = window.window_decorations() else { return Vec::new() };
    if window.is_maximized() || window.is_fullscreen() {
        return Vec::new();
    }
    let (top, bottom, left, right) = (!tiling.top, !tiling.bottom, !tiling.left, !tiling.right);
    let handles = [
        (top, ResizeEdge::Top, CursorStyle::ResizeUpDown),
        (bottom, ResizeEdge::Bottom, CursorStyle::ResizeUpDown),
        (left, ResizeEdge::Left, CursorStyle::ResizeLeftRight),
        (right, ResizeEdge::Right, CursorStyle::ResizeLeftRight),
        (top && left, ResizeEdge::TopLeft, CursorStyle::ResizeUpLeftDownRight),
        (bottom && right, ResizeEdge::BottomRight, CursorStyle::ResizeUpLeftDownRight),
        (top && right, ResizeEdge::TopRight, CursorStyle::ResizeUpRightDownLeft),
        (bottom && left, ResizeEdge::BottomLeft, CursorStyle::ResizeUpRightDownLeft),
    ];
    handles
        .into_iter()
        .filter(|(enabled, _, _)| *enabled)
        .map(|(_, edge, cursor)| {
            let hit = px(HIT);
            let corner = px(HIT * 2.0);
            let strip = div().absolute().cursor(cursor).occlude();
            let strip = match edge {
                ResizeEdge::Top => strip.top_0().left(corner).right(corner).h(hit),
                ResizeEdge::Bottom => strip.bottom_0().left(corner).right(corner).h(hit),
                ResizeEdge::Left => strip.left_0().top(corner).bottom(corner).w(hit),
                ResizeEdge::Right => strip.right_0().top(corner).bottom(corner).w(hit),
                ResizeEdge::TopLeft => strip.top_0().left_0().size(corner),
                ResizeEdge::TopRight => strip.top_0().right_0().size(corner),
                ResizeEdge::BottomLeft => strip.bottom_0().left_0().size(corner),
                ResizeEdge::BottomRight => strip.bottom_0().right_0().size(corner),
            };
            strip
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    window.start_window_resize(edge);
                })
                .into_any_element()
        })
        .collect()
}

fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}
