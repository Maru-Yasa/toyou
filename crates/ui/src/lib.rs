//! Building blocks shared by toyou's views: the theme palette, lazily loaded album art,
//! visibility checks, window resize edges, and small helpers.

mod images;

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub use images::{ImageCache, sized_url};

/// Theme colors copied out of the global so rendering can keep using `cx` mutably.
#[derive(Clone, Copy)]
pub struct Palette {
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


/// Album art at `size`, downloaded lazily (only once it is on screen) at the size it's drawn.
pub fn thumb_element(
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

/// The color of the artwork `thumb_element` shows for the same url and size, once loaded.
pub fn art_tint(images: &Entity<ImageCache>, scale_factor: f32, url: Option<&String>, size: Pixels, cx: &App) -> Option<Hsla> {
    let pixels = (f32::from(size) * scale_factor).ceil() as u32;
    let url = crate::images::sized_url(url?, pixels);
    images.read(cx).tint(&url)
}

/// An invisible element that calls `on_visible` each frame any part of it is on screen.
/// GPUI has no visibility API, but during prepaint the window's content mask is the
/// intersection of every enclosing clip (scroll views included); off-screen it is empty.
pub fn when_visible(on_visible: impl Fn(&mut App) + 'static) -> Canvas<()> {
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
pub fn resize_handles(window: &Window) -> Vec<AnyElement> {
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

/// Linear blend between two colors (`t` from 0 to 1), for animated transitions.
pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    Hsla { h: lerp(from.h, to.h), s: lerp(from.s, to.s), l: lerp(from.l, to.l), a: lerp(from.a, to.a) }
}

pub fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}
