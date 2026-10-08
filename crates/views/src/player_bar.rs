//! The player bar at the bottom, the volume popover, and the up-next panel.


use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonRounded, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use router::Route;
use ui::{Palette, format_time};

use crate::app::MusicApp;

impl MusicApp {
    pub(crate) fn render_player_bar(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let track = self.current_track().cloned();
        let state = &self.playback;
        let paused = state.paused || state.idle;

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
                    .id("now-playing-info")
                    .w(px(280.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(track.is_some(), |el| {
                        el.cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_now_playing(cx)))
                    })
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
                            .child(div().text_xs().text_color(p.muted_fg)
                                // Errors may wrap onto a second line so more of the reason shows.
                                .map(|el| if self.player_error.is_some() { el.line_clamp(2) } else { el.truncate() })
                                .child(
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
                    .w(px(120.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_1()
                    .text_color(p.muted_fg)
                    .child(self.render_volume_popover(p, cx))
                    .child(
                        Button::new("now-playing-toggle")
                            .ghost()
                            .small()
                            .icon(if self.route == Route::NowPlaying { IconName::ChevronDown } else { IconName::ChevronUp })
                            .tooltip("Now playing")
                            .disabled(track.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_now_playing(cx))),
                    ),
            )
    }

    /// A speaker button that opens a vertical volume slider above it.
    pub(crate) fn render_volume_popover(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let volume = self.playback.volume.round() as i64;
        let muted = volume <= 0;
        let icon = if muted { IconName::VolumeX } else { IconName::Volume2 };
        Popover::new("volume-popover")
            .anchor(Anchor::BottomCenter)
            .offset(px(8.0))
            .trigger(Button::new("volume").ghost().small().icon(icon).tooltip(format!("Volume {volume}%")))
            .child(
                div()
                    .w(px(48.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .py_1()
                    .child(div().text_xs().text_color(p.muted_fg).child(format!("{volume}%")))
                    .child(div().h(px(140.0)).child(Slider::new(&self.volume).vertical()))
                    .child(
                        Button::new("mute")
                            .ghost()
                            .small()
                            .icon(icon)
                            .tooltip(if muted { "Unmute" } else { "Mute" })
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_mute(window, cx))),
                    ),
            )
    }

    pub(crate) fn render_queue(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
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
