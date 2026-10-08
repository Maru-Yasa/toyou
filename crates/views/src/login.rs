//! The sign-in page.


use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ui::Palette;

use crate::app::MusicApp;

impl MusicApp {
    pub(crate) fn render_login(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.login_busy.is_some();
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
}
