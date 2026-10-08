//! The Now playing view: large artwork plus Up next, synced Lyrics and Related.


use gpui_kit::component::spinner::Spinner;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use music::Item;
use state::{Fetch, NowTab};
use ui::{Palette, art_tint, mix};

use crate::app::MusicApp;

/// Synced lyrics: normal line size (rem), how much the active line grows, and how long it takes.
const LYRIC_SIZE: f32 = 1.5;
const LYRIC_GROWTH: f32 = 0.15;
/// Height of the window chrome around this view: title bar, search bar and player bar.
const CHROME_HEIGHT: f32 = 34.0 + 60.0 + 88.0;
const LYRIC_ANIMATION: std::time::Duration = std::time::Duration::from_millis(280);

impl MusicApp {
    pub(crate) fn render_now_playing(&self, p: Palette, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(track) = self.current_track().cloned() else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(p.muted_fg)
                .child("Nothing playing yet. Pick a song from Home, Explore or search.")
                .into_any_element();
        };
        // Layout A: the artwork fills the left column (title and artist stay in the player bar),
        // and the tabs take about 44% of the width on the right.
        let viewport = window.viewport_size();
        let content_w = f32::from(viewport.width) - 220.0 - if self.show_queue { 320.0 } else { 0.0 };
        let content_h = f32::from(viewport.height) - CHROME_HEIGHT;
        let right_w = (content_w * 0.44).max(340.0);
        let art = px((content_h - 64.0).min(content_w - right_w - 120.0).clamp(220.0, 560.0));

        // The song's own color tints the view and the shadow under the cover.
        let tint = art_tint(&self.images, self.scale_factor.get(), track.thumbnail.as_ref(), art, cx).unwrap_or(p.muted_fg);

        let pill = |id: &'static str, label: &'static str, tab: NowTab, cx: &mut Context<Self>| {
            let active = self.now_tab == tab;
            div()
                .id(id)
                .px_4()
                .py(px(7.0))
                .rounded_full()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .map(|el| {
                    if active {
                        el.bg(p.fg).text_color(p.bg)
                    } else {
                        el.text_color(p.muted_fg).hover(move |s| s.text_color(p.fg))
                    }
                })
                .on_click(cx.listener(move |this, _, _, cx| this.set_now_tab(tab, cx)))
                .child(label)
        };
        let tabs = div().flex().child(
            div()
                .flex()
                .gap_1()
                .p_1()
                .rounded_full()
                .bg(hsla(0.0, 0.0, 1.0, 0.06))
                .border_1()
                .border_color(hsla(0.0, 0.0, 1.0, 0.07))
                .child(pill("tab-up-next", "UP NEXT", NowTab::UpNext, cx))
                .child(pill("tab-lyrics", "LYRICS", NowTab::Lyrics, cx))
                .child(pill("tab-related", "RELATED", NowTab::Related, cx)),
        );

        let scroll = |body: AnyElement| {
            div()
                .id(SharedString::from(format!("now-tab-{:?}", self.now_tab)))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(body)
                .into_any_element()
        };
        let body = match self.now_tab {
            NowTab::UpNext => self.render_up_next_tab(p, cx),
            NowTab::Lyrics => self.render_lyrics_tab(&track.video_id, p, window, cx),
            NowTab::Related => scroll(self.render_related_tab(&track.video_id, p, cx)),
        };

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(48.0))
            .px(px(40.0))
            .py(px(28.0))
            .bg(linear_gradient(
                115.0,
                linear_color_stop(tint.opacity(0.22), 0.0),
                linear_color_stop(tint.opacity(0.0), 0.7),
            ))
            .child(
                div().flex_1().min_w_0().flex().items_center().justify_center().child(
                    div()
                        .size(art)
                        .rounded_xl()
                        .overflow_hidden()
                        .shadow(vec![BoxShadow {
                            color: tint.opacity(0.5),
                            offset: point(px(0.0), px(24.0)),
                            blur_radius: px(48.0),
                            spread_radius: px(-16.0),
                            inset: false,
                        }])
                        .child(self.thumb(track.thumbnail.as_ref(), art, false, p, cx)),
                ),
            )
            .child(
                div()
                    .w(px(right_w))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(18.0))
                    .min_h_0()
                    .child(tabs)
                    .child(body),
            )
            .into_any_element()
    }

    /// The queue as a virtual list: only rows on screen are built and drawn, however long the
    /// queue grows. It scrolls to the playing song (see `MusicApp::reveal_current_song`).
    pub(crate) fn render_up_next_tab(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let list = uniform_list(
            "up-next",
            self.queue.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .map(|ix| {
                        let track = this.queue[ix].clone();
                        this.render_track_row(50_000 + ix, &track, None, p, cx)
                            // Virtual-list rows don't stretch on their own.
                            .w_full()
                            .when(this.current == Some(ix), |row| row.bg(hsla(0.0, 0.0, 1.0, 0.08)).child(equalizer(p.fg)))
                            .on_click(cx.listener(move |this, _, _, cx| this.play_index(ix, cx)))
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.up_next_scroll)
        .w_full()
        .flex_1();

        div()
            .flex()
            .flex_col()
            .gap_3()
            .flex_1()
            .min_h_0()
            .when_some(self.queue_source.clone(), |el, source| {
                el.child(
                    div()
                        .px_2()
                        .flex()
                        .flex_col()
                        .child(div().text_xs().text_color(p.muted_fg).child("PLAYING FROM"))
                        .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).truncate().child(source)),
                )
            })
            .child(list)
            .into_any_element()
    }

    pub(crate) fn render_lyrics_tab(&self, video_id: &str, p: Palette, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let message = |text: String| div().p_4().text_sm().text_color(p.muted_fg).child(text).into_any_element();
        let lyrics = match self.lyrics.get(video_id) {
            None | Some(Fetch::Loading) => {
                return div().p_8().flex().justify_center().child(Spinner::new()).into_any_element();
            }
            Some(Fetch::Failed(err)) => return message(format!("Couldn't load lyrics: {err}")),
            Some(Fetch::Ready(None)) => return message("Lyrics aren't available for this song.".into()),
            Some(Fetch::Ready(Some(lyrics))) => lyrics.clone(),
        };
        let synced = lyrics.synced();
        let current = if synced { self.lyrics_line } else { None };
        let previous = if synced { self.lyrics_prev_line } else { None };
        // Ease the scroll toward the active line; keep frames coming until it settles.
        if synced && self.step_lyrics_scroll() {
            window.request_animation_frame();
        }
        let upcoming = p.fg.opacity(0.32);
        let sung = p.fg.opacity(0.18);

        // Lines are direct children of the scroll view so the current one can be centered;
        // the padding lets the first and last lines reach the middle too.
        let list = div()
            .id("lyrics-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.lyrics_scroll)
            .px_2()
            .when(synced, |el| el.pt(px(160.0)).pb(px(240.0)))
            .when(!synced, |el| el.pt_3().pb_6())
            .children(lyrics.lines.iter().enumerate().map(|(ix, line)| {
                if line.text.trim().is_empty() {
                    return div().h_4().into_any_element();
                }
                let color = match current {
                    Some(now) if ix < now => sung,
                    _ if synced => upcoming,
                    _ => p.fg,
                };
                let line_el = div()
                    .id(("lyric", ix))
                    .py_1()
                    .font_weight(FontWeight::BOLD)
                    .text_color(color)
                    .child(line.text.clone())
                    .when_some(line.start_ms.filter(|_| synced), |el, start_ms| {
                        el.cursor_pointer()
                            .hover(move |s| s.text_color(p.fg))
                            .on_click(cx.listener(move |this, _, _, cx| this.seek_to_lyric(start_ms, cx)))
                    });
                // The line being sung grows and brightens; the one before it shrinks and dims.
                // Element ids include the line index, so each change starts a fresh animation.
                let ease = || Animation::new(LYRIC_ANIMATION).with_easing(ease_out_quint());
                if current == Some(ix) {
                    line_el
                        .with_animation(("lyric-grow", ix), ease(), move |el, t| {
                            el.text_size(rems(LYRIC_SIZE + LYRIC_GROWTH * t)).text_color(mix(upcoming, p.fg, t))
                        })
                        .into_any_element()
                } else if previous == Some(ix) {
                    line_el
                        .with_animation(("lyric-shrink", ix), ease(), move |el, t| {
                            el.text_size(rems(LYRIC_SIZE + LYRIC_GROWTH * (1.0 - t))).text_color(mix(p.fg, sung, t))
                        })
                        .into_any_element()
                } else {
                    line_el.text_size(rems(LYRIC_SIZE)).into_any_element()
                }
            }))
            .child(div().pt_4().text_xs().text_color(p.muted_fg).child(lyrics.source.clone()));

        // Lines fade out toward the top and bottom edges.
        let fade = |angle: f32| linear_gradient(angle, linear_color_stop(p.bg, 0.0), linear_color_stop(p.bg.opacity(0.0), 1.0));
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(list)
            .when(synced, |el| {
                el.child(div().absolute().top_0().left_0().right_0().h(px(72.0)).bg(fade(180.0)))
                    .child(div().absolute().bottom_0().left_0().right_0().h(px(96.0)).bg(fade(0.0)))
            })
            .into_any_element()
    }

    pub(crate) fn render_related_tab(&self, video_id: &str, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let page = match self.related.get(video_id) {
            None | Some(Fetch::Loading) => {
                return div().p_8().flex().justify_center().child(Spinner::new()).into_any_element();
            }
            Some(Fetch::Failed(err)) => {
                return div().p_4().text_sm().text_color(p.muted_fg).child(format!("Couldn't load related: {err}")).into_any_element();
            }
            Some(Fetch::Ready(page)) => page.clone(),
        };
        let mut content = div().flex().flex_col().gap_6().pb_6();
        for (section_ix, section) in page.sections.iter().enumerate() {
            let mut block = div().flex().flex_col().gap_2();
            if !section.title.is_empty() {
                block = block.child(div().px_2().font_weight(FontWeight::SEMIBOLD).child(section.title.clone()));
            }
            let mut cards = div().flex().flex_wrap().gap_1();
            let mut has_cards = false;
            for (ix, item) in section.items.iter().take(8).enumerate() {
                let id = 60_000 + section_ix * 100 + ix;
                match item {
                    Item::Track(track) => {
                        let track_for_click = track.clone();
                        block = block.child(
                            self.render_track_row(id, track, None, p, cx)
                                .on_click(cx.listener(move |this, _, _, cx| this.play_radio(track_for_click.clone(), cx))),
                        );
                    }
                    Item::Card(card) if !card.chip => {
                        has_cards = true;
                        cards = cards.child(self.render_card(id, card, p, cx));
                    }
                    Item::Card(_) => {}
                }
            }
            if has_cards {
                block = block.child(cards);
            }
            content = content.child(block);
        }
        if page.sections.is_empty() {
            content = content.child(div().p_4().text_sm().text_color(p.muted_fg).child("Nothing related found."));
        }
        content.into_any_element()
    }
}

/// Three bars marking the song that's playing. Static on purpose: an endless animation here
/// would redraw the whole Now playing view every frame while music plays.
fn equalizer(color: Hsla) -> impl IntoElement {
    const HEIGHT: f32 = 14.0;
    div()
        .flex()
        .items_end()
        .gap(px(2.0))
        .h(px(HEIGHT))
        .flex_none()
        .children([0.55f32, 1.0, 0.7].map(|level| div().w(px(3.0)).rounded_sm().bg(color).h(px(HEIGHT * level))))
}
