//! The window: title bar, search bar, layout, shortcuts and focus handling.


use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, TitleBar};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use input::{TogglePlay, NextTrack, PrevTrack, GoBack, FocusSearch, ToggleFps, OpenSongPalette, OpenCommandPalette, GoHome, GoExplore, GoLibrary, ToggleQueue, ShuffleUpNext, RefreshPage, SignIn, SignOut, ToggleNowPlaying};
use router::Route;
use state::FpsMeter;
use ui::{Palette, resize_handles};

use crate::app::MusicApp;
use crate::palette::PaletteKind;

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
            .on_action(cx.listener(|this, _: &ToggleNowPlaying, _, cx| this.toggle_now_playing(cx)))
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
    pub fn render_main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        match self.route {
            Route::Login => self.render_login(p, cx).into_any_element(),
            Route::NowPlaying => self.render_now_playing(p, window, cx),
            _ => self.render_page(p, cx),
        }
    }

    /// The sidebar, rendered on behalf of the cached [`crate::app::SidebarView`].
    pub fn render_sidebar_element(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        self.render_sidebar(p, cx).into_any_element()
    }

    pub(crate) fn render_top_bar(&self, p: Palette, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
}
