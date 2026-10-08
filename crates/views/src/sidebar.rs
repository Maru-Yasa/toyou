//! The sidebar: navigation, your playlists, and your account.


use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use music::Target;
use router::Route;
use ui::Palette;

use crate::app::MusicApp;

impl MusicApp {
    pub(crate) fn nav_item(&self, id: &'static str, icon: IconName, label: &'static str, route: Route, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
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

    pub(crate) fn render_sidebar(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(crate) fn render_sidebar_playlists(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
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
}
