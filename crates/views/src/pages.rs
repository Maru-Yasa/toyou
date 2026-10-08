//! Pages built from YouTube Music data: Home, Explore, Library, search, albums and playlists.

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use music::{Card, Item, Page, Track};
use router::Route;
use state::Load;
use ui::{Palette, thumb_element, when_visible};

use crate::app::MusicApp;

const TRACK_GRID_COLUMNS: usize = 4;
const TRACK_GRID_COLLAPSED: usize = TRACK_GRID_COLUMNS * 4;
const CARDS_COLLAPSED: usize = 10;
const CARD_WIDTH: f32 = 168.0;

impl MusicApp {
    /// Album art that downloads lazily: only once it is on screen.
    pub(crate) fn thumb(&self, url: Option<&String>, size: Pixels, round: bool, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        thumb_element(&self.images, self.scale_factor.get(), url, size, round, p, cx)
    }

    pub(crate) fn render_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
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

    pub(crate) fn page_title(&self) -> Option<(String, Option<String>)> {
        let greeting = self.account.as_ref().map(|a| format!("Picked for {}", a.name));
        match &self.route {
            Route::Home => Some(("Home".into(), greeting.or(Some("Popular right now".into())))),
            Route::Explore => Some(("Explore".into(), Some("New releases, charts, moods & genres".into()))),
            Route::Library => Some(("Library".into(), Some("Your playlists and liked music".into()))),
            Route::Search(q) => Some((format!("Results for “{q}”"), None)),
            Route::Browse { .. } | Route::Login | Route::NowPlaying => None,
        }
    }

    pub(crate) fn render_page_content(&self, page: Arc<Page>, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
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
            let limit = if tracks.is_empty() { CARDS_COLLAPSED } else { TRACK_GRID_COLLAPSED };
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
                let shown = if expanded { tracks.len() } else { tracks.len().min(TRACK_GRID_COLLAPSED) };
                // Album/playlist pages keep a numbered single-column list; elsewhere songs
                // flow into columns.
                let mut list = if is_detail { div().flex().flex_col() } else { div().flex().flex_wrap() };
                for (ix, track) in tracks.iter().take(shown).enumerate() {
                    let row_id = section_ix * 10_000 + ix;
                    let queue_ix = page_track_ix + ix;
                    let track = (*track).clone();
                    let row = self.render_track_row(row_id, &track, is_detail.then_some(queue_ix + 1), p, cx).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if is_detail {
                                this.play_from_page(queue_ix, false, cx);
                            } else {
                                this.play_radio(track.clone(), cx);
                            }
                        }),
                    );
                    list = if is_detail {
                        list.child(row)
                    } else {
                        list.child(div().w(relative(1.0 / TRACK_GRID_COLUMNS as f32)).min_w_0().pr_3().child(row))
                    };
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

    pub(crate) fn render_track_row(&self, id: usize, track: &Track, number: Option<usize>, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
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

    pub(crate) fn render_card(&self, id: usize, card: &Card, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
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

    pub(crate) fn render_chip(&self, id: usize, card: &Card, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
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
}
