//! The `toyou-login` program: a small native browser window (WebKitGTK via wry) for
//! signing in to Google. It is a separate program because wry/tao drive their own GTK
//! event loop, which can't share a window with GPUI, and so the main app never loads
//! WebKit. Once YouTube's login cookies appear, they are saved for the main app and
//! the window closes itself.

use std::time::{Duration, Instant};

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{WebContext, WebViewBuilder};

use crate::auth::{self, CookieRecord, LOGIN_EXIT_CANCELLED, LOGIN_EXIT_SIGNED_IN, ORIGIN};

const SIGN_IN_URL: &str =
    "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube&passive=true&continue=https%3A%2F%2Fmusic.youtube.com%2F";
/// Google refuses sign-in from browsers it considers "embedded", so present as Firefox.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";
const POLL_INTERVAL: Duration = Duration::from_millis(800);

pub fn run() -> ! {
    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("Sign in to YouTube Music — toyou")
        .with_inner_size(tao::dpi::LogicalSize::new(520.0, 720.0))
        .build(&event_loop)
        .expect("failed to open the sign-in window");

    // A persistent profile, so Google recognizes this "browser" on later sign-ins.
    let mut context = WebContext::new(auth::config_dir().map(|dir| dir.join("webview")));
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_user_agent(USER_AGENT)
        .with_url(SIGN_IN_URL);
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("no GTK container")).expect("failed to create the web view")
    };

    let mut next_poll = Instant::now() + POLL_INTERVAL;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(next_poll);
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => std::process::exit(LOGIN_EXIT_CANCELLED),
            Event::NewEvents(_) if Instant::now() >= next_poll => {
                next_poll = Instant::now() + POLL_INTERVAL;
                // Only look once Google has sent us back to YouTube Music.
                let on_youtube = webview.url().is_ok_and(|url| url.starts_with(ORIGIN));
                if !on_youtube {
                    return;
                }
                let Ok(cookies) = webview.cookies() else { return };
                let records: Vec<CookieRecord> = cookies
                    .iter()
                    .filter(|c| c.domain().is_some_and(|d| d.trim_start_matches('.').ends_with("youtube.com")))
                    .map(|c| CookieRecord {
                        domain: c.domain().unwrap_or(".youtube.com").to_string(),
                        path: c.path().unwrap_or("/").to_string(),
                        secure: c.secure().unwrap_or(false),
                        expires: c.expires_datetime().map_or(0, |t| t.unix_timestamp().max(0) as u64),
                        name: c.name().to_string(),
                        value: c.value().to_string(),
                    })
                    .collect();
                if auth::Session::save_cookies(&records).is_ok() {
                    std::process::exit(LOGIN_EXIT_SIGNED_IN);
                }
            }
            _ => {}
        }
    })
}
