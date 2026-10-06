//! Application state and behavior: navigation, sign-in, and the playback queue.
//! Rendering lives in `ui.rs`.

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::*;

use crate::api::{Account, Card, Client, Item, Page, Target, Track};
use crate::auth::Session;
use crate::images::ImageCache;
use crate::player::{PlaybackState, Player};

/// Resolution of the seek slider, which tracks playback as a ratio of the duration.
pub const PROGRESS_STEPS: f32 = 1000.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Login,
    Home,
    Explore,
    Library,
    Search(String),
    Browse { id: String, params: Option<String> },
}

impl Route {
    pub fn key(&self) -> String {
        match self {
            Route::Login => "login".into(),
            Route::Home => "home".into(),
            Route::Explore => "explore".into(),
            Route::Library => "library".into(),
            Route::Search(q) => format!("search:{q}"),
            Route::Browse { id, params } => format!("browse:{id}:{}", params.as_deref().unwrap_or("")),
        }
    }
}

/// Debug FPS counter (F12, or `TOYOU_FPS=1`). While shown, the window redraws continuously,
/// so it reports how fast the current screen *can* be drawn, not only how often it changes.
#[derive(Default)]
pub struct FpsMeter {
    frames: VecDeque<Instant>,
    build_time: Duration,
}

impl FpsMeter {
    pub fn record_frame(&mut self, build_time: Duration) {
        let now = Instant::now();
        self.frames.push_back(now);
        while self.frames.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1)) {
            self.frames.pop_front();
        }
        self.build_time = build_time;
    }

    /// Frames in the last second, average frame time, and time spent building the UI tree.
    pub fn stats(&self) -> (usize, f32, f32) {
        let frame_ms = match (self.frames.front(), self.frames.back()) {
            (Some(first), Some(last)) if self.frames.len() > 1 => {
                last.duration_since(*first).as_secs_f32() * 1000.0 / (self.frames.len() - 1) as f32
            }
            _ => 0.0,
        };
        (self.frames.len(), frame_ms, self.build_time.as_secs_f32() * 1000.0)
    }
}

/// The cached page area (current page or sign-in). Renders through `MusicApp`.
pub struct PageView {
    app: WeakEntity<MusicApp>,
}

/// The cached sidebar. Renders through `MusicApp`.
pub struct SidebarView {
    app: WeakEntity<MusicApp>,
}

impl Render for PageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.app.update(cx, |app, cx| app.render_main(cx)).ok();
        div().size_full().flex().flex_col().children(content)
    }
}

impl Render for SidebarView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.app.update(cx, |app, cx| app.render_sidebar_element(cx)).ok();
        div().size_full().children(content)
    }
}

pub enum Load {
    Loading,
    Ready(Arc<Page>),
    Failed(SharedString),
}

pub struct MusicApp {
    pub client: Client,
    pub session: Option<Session>,
    pub account: Option<Account>,
    /// Set when YouTube rejected the saved session (e.g. it expired).
    pub session_expired: bool,
    /// The signed-in user's saved and created playlists, shown in the sidebar.
    pub playlists: Vec<Card>,

    pub route: Route,
    pub back: Vec<Route>,
    pub pages: HashMap<String, Load>,
    /// Whether the alternative sign-in methods are shown on the login page.
    pub show_other_logins: bool,
    /// Sections the user expanded with "Show all", keyed by route key and section index.
    pub expanded: HashSet<(String, usize)>,

    /// Keyboard focus for the app itself, so shortcuts work when no input is focused.
    pub focus: FocusHandle,
    pub search_input: Entity<InputState>,
    pub cookie_input: Entity<InputState>,
    pub login_status: Option<(SharedString, bool)>,
    /// The login method in progress: a browser name, or "cookie".
    pub login_busy: Option<&'static str>,

    pub images: Entity<ImageCache>,
    /// The window's scale factor, so covers are downloaded at the size they're drawn.
    pub scale_factor: Cell<f32>,
    pub progress: Entity<SliderState>,
    pub volume: Entity<SliderState>,

    pub player: Option<Player>,
    pub player_error: Option<SharedString>,
    pub playback: PlaybackState,
    pub queue: Vec<Track>,
    pub current: Option<usize>,
    pub show_queue: bool,
    seen_finished: u64,
    seen_failed: u64,
    /// A song restored from the last session, not yet loaded: pressing play resumes it here.
    resume_at: Option<f64>,
    /// What was last written to disk (queue length, current song), and when.
    last_saved: (usize, Option<String>, Instant),
    loading_more: bool,
    /// Songs that failed in a row; stops auto-skipping after a few.
    consecutive_failures: u32,
    seeking: bool,
    extending_queue: bool,

    pub fps: Option<FpsMeter>,
    /// The open Ctrl+P / Ctrl+Shift+P palette, if any.
    pub palette: Option<crate::palette::PaletteState>,

    /// The page area and the sidebar are cached views: GPUI reuses their last frame unless
    /// they are notified, so the once-a-second player tick doesn't re-layout the page.
    pub page_view: Entity<PageView>,
    pub sidebar_view: Entity<SidebarView>,

    tasks: HashMap<&'static str, Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl MusicApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search songs, albums, artists, playlists"));
        let cookie_input = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("SID=…; SAPISID=…; …"));
        let progress = cx.new(|_| SliderState::new().min(0.0).max(PROGRESS_STEPS).step(1.0));
        let volume = cx.new(|_| SliderState::new().min(0.0).max(100.0).step(1.0).default_value(70.0));
        let images = cx.new(ImageCache::new);

        let subscriptions = vec![
            cx.observe(&images, |this, _, cx| this.changed(cx)),
            cx.subscribe(&search_input, |this, input, event: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let query = input.read(cx).value().trim().to_string();
                    if !query.is_empty() {
                        this.navigate(Route::Search(query), cx);
                    }
                }
            }),
            cx.subscribe(&cookie_input, |this, _, event: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.login_with_cookie_header(cx);
                }
            }),
            cx.subscribe(&progress, |this, _, event: &SliderEvent, _| match event {
                SliderEvent::Change(_) => this.seeking = true,
                SliderEvent::Release(value) => {
                    this.seeking = false;
                    let target = value.end() as f64 / PROGRESS_STEPS as f64 * this.playback.duration;
                    match &mut this.resume_at {
                        Some(at) => *at = target,
                        None => this.with_player(|p| p.seek_to(target)),
                    }
                }
            }),
            cx.subscribe(&volume, |this, _, event: &SliderEvent, _| {
                let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
                let target = value.end() as f64;
                this.with_player(|p| p.set_volume(target));
            }),
        ];

        let app = cx.weak_entity();
        let page_view = cx.new(|_| PageView { app: app.clone() });
        let sidebar_view = cx.new(|_| SidebarView { app });

        let session = Session::load();
        let mut this = Self {
            client: Client::new(session.clone()),
            session: None,
            account: None,
            session_expired: false,
            playlists: Vec::new(),
            route: Route::Login,
            back: Vec::new(),
            pages: HashMap::new(),
            expanded: HashSet::new(),
            show_other_logins: false,
            focus: cx.focus_handle(),
            search_input,
            cookie_input,
            login_status: None,
            login_busy: None,
            images,
            scale_factor: Cell::new(window.scale_factor()),
            progress,
            volume,
            player: None,
            player_error: None,
            playback: PlaybackState { idle: true, ..Default::default() },
            queue: Vec::new(),
            current: None,
            show_queue: false,
            seen_finished: 0,
            seen_failed: 0,
            resume_at: None,
            last_saved: (0, None, Instant::now()),
            loading_more: false,
            consecutive_failures: 0,
            seeking: false,
            extending_queue: false,
            fps: std::env::var_os("TOYOU_FPS").map(|_| FpsMeter::default()),
            palette: None,
            page_view,
            sidebar_view,
            tasks: HashMap::new(),
            _subscriptions: subscriptions,
        };
        match session {
            Some(session) => this.set_session(Some(session), cx),
            None => this.spawn_player(),
        }

        // Poll the player a few times per second; cheap, and keeps the UI in sync.
        let poll = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(250)).await;
                if this.update_in(cx, |this, window, cx| this.sync_playback(window, cx)).is_err() {
                    break;
                }
            }
        });
        this.tasks.insert("poll", poll);
        this.restore_state(window, cx);
        this.focus.focus(window, cx);
        this._subscriptions.push(cx.on_app_quit(|this, _| {
            this.save_state();
            async {}
        }));
        this
    }

    /// gpui routes shortcuts through the focused element and its parents. If focus is
    /// nowhere, or on an element that's no longer on screen (e.g. a closed palette's search
    /// field), shortcuts reach nothing, so focus falls back to the app's root.
    /// Deferred: focus can't change mid-frame.
    pub fn ensure_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.contains_focused(window, cx) {
            cx.defer_in(window, |this, window, cx| {
                if !this.focus.contains_focused(window, cx) {
                    this.focus.focus(window, cx);
                }
            });
        }
    }

    /// Re-renders after a change that may affect the page or sidebar. (A change that only
    /// affects the player bar or title bar can use `cx.notify()` and keep them cached.)
    pub fn changed(&mut self, cx: &mut Context<Self>) {
        self.page_view.update(cx, |_, cx| cx.notify());
        self.sidebar_view.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    // ---- Navigation -------------------------------------------------------------------

    pub fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        if route != self.route {
            let previous = std::mem::replace(&mut self.route, route);
            if previous != Route::Login {
                self.back.push(previous);
            }
        }
        self.ensure_loaded(cx);
        self.changed(cx);
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(route) = self.back.pop() {
            self.route = route;
            self.ensure_loaded(cx);
            self.changed(cx);
        }
    }

    pub fn open_target(&mut self, target: &Target, cx: &mut Context<Self>) {
        match target {
            Target::Browse { id, params } => self.navigate(Route::Browse { id: id.clone(), params: params.clone() }, cx),
            Target::Watch { video_id, playlist_id } => self.play_watch(video_id.clone(), playlist_id.clone(), cx),
        }
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.route == Route::Library {
            self.load_playlists(cx);
        }
        self.pages.remove(&self.route.key());
        self.ensure_loaded(cx);
        self.changed(cx);
    }

    fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        let route = self.route.clone();
        let key = route.key();
        if route == Route::Login || matches!(self.pages.get(&key), Some(Load::Ready(_) | Load::Loading)) {
            return;
        }
        self.pages.insert(key.clone(), Load::Loading);
        let client = self.client.clone();
        let request = cx.background_executor().spawn(async move {
            match route {
                Route::Login => unreachable!(),
                Route::Home => client.home(),
                Route::Explore => client.explore(),
                Route::Library => client.library(),
                Route::Search(query) => client.search(&query),
                Route::Browse { id, params } => client.browse(&id, params.as_deref()),
            }
        });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                let load = match result {
                    Ok(page) => Load::Ready(Arc::new(page)),
                    Err(err) => Load::Failed(err.into()),
                };
                this.pages.insert(key, load);
                this.changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Whether the current page can load more sections, and isn't already doing so.
    pub fn can_load_more(&self) -> bool {
        !self.loading_more
            && matches!(self.pages.get(&self.route.key()), Some(Load::Ready(page)) if page.continuation.is_some())
    }

    pub fn is_loading_more(&self) -> bool {
        self.loading_more
    }

    /// Appends the next batch of sections to the current page (Home), as the user scrolls.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if !self.can_load_more() {
            return;
        }
        let key = self.route.key();
        let Some(Load::Ready(page)) = self.pages.get(&key) else { return };
        let Some(token) = page.continuation.clone() else { return };
        self.loading_more = true;
        let client = self.client.clone();
        let request = cx.background_executor().spawn(async move { client.home_more(&token) });
        let task = cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.loading_more = false;
                if let Some(Load::Ready(page)) = this.pages.get_mut(&key) {
                    let page = Arc::make_mut(page);
                    match result {
                        Ok((sections, next)) => {
                            page.sections.extend(sections);
                            page.continuation = next;
                        }
                        // Stop paging on errors rather than retrying on every scroll.
                        Err(_) => page.continuation = None,
                    }
                }
                this.changed(cx);
            })
            .ok();
        });
        self.tasks.insert("more", task);
        self.changed(cx);
    }

    /// Fetches the user's playlists (and Liked music) for the sidebar.
    pub fn load_playlists(&mut self, cx: &mut Context<Self>) {
        if !self.client.is_logged_in() {
            return;
        }
        let client = self.client.clone();
        let request = cx.background_executor().spawn(async move { client.library() });
        let task = cx.spawn(async move |this, cx| {
            let Ok(library) = request.await else { return };
            this.update(cx, |this, cx| {
                this.playlists = library
                    .sections
                    .into_iter()
                    .flat_map(|section| section.items)
                    .filter_map(|item| match item {
                        Item::Card(card) if !card.chip => Some(card),
                        _ => None,
                    })
                    .collect();
                this.changed(cx);
            })
            .ok();
        });
        self.tasks.insert("playlists", task);
    }

    pub fn toggle_expanded(&mut self, section: usize, cx: &mut Context<Self>) {
        let key = (self.route.key(), section);
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        self.changed(cx);
    }

    // ---- Sign-in ----------------------------------------------------------------------

    /// Opens the sign-in window and waits for it to finish. It's a separate program,
    /// `toyou-login`, so the main app never loads WebKit/GTK.
    pub fn login_with_google(&mut self, cx: &mut Context<Self>) {
        self.run_login("google", cx, || {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("toyou-login");
            let status = std::process::Command::new(&exe)
                .status()
                .map_err(|e| format!("couldn't open the sign-in window ({}): {e}", exe.display()))?;
            match status.code() {
                Some(crate::auth::LOGIN_EXIT_SIGNED_IN) => {
                    Session::load().ok_or_else(|| "Signed in, but no YouTube session was saved".to_string())
                }
                Some(crate::auth::LOGIN_EXIT_CANCELLED) => Err("Sign-in window closed before finishing.".into()),
                _ => Err("The sign-in window crashed. Try another sign-in method below.".into()),
            }
        });
    }

    pub fn login_with_browser(&mut self, browser: &'static str, cx: &mut Context<Self>) {
        self.run_login(browser, cx, move || Session::import_from_browser(browser));
    }

    pub fn login_with_cookie_header(&mut self, cx: &mut Context<Self>) {
        let header = self.cookie_input.read(cx).value().to_string();
        if header.trim().is_empty() {
            self.login_status = Some(("Paste the Cookie header first, then press Sign in.".into(), true));
            self.changed(cx);
            return;
        }
        self.run_login("cookie", cx, move || Session::from_cookie_header(&header));
    }

    fn run_login(
        &mut self,
        method: &'static str,
        cx: &mut Context<Self>,
        login: impl FnOnce() -> Result<Session, String> + Send + 'static,
    ) {
        if self.login_busy.is_some() {
            return;
        }
        self.login_busy = Some(method);
        let message = match method {
            "google" => "Finish signing in in the window that just opened…",
            "cookie" => "Checking the session…",
            _ => "Reading the session from your browser…",
        };
        self.login_status = Some((message.into(), false));
        let request = cx.background_executor().spawn(async move {
            let session = login()?;
            // Confirm YouTube accepts the session before switching to it.
            let account = Client::new(Some(session.clone())).account()?;
            Ok::<_, String>((session, account))
        });
        let task = cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.login_busy = None;
                match result {
                    Ok((session, account)) => {
                        this.login_status = None;
                        this.set_session(Some(session), cx);
                        this.account = Some(account);
                    }
                    Err(err) => this.login_status = Some((err.into(), true)),
                }
                this.changed(cx);
            })
            .ok();
        });
        self.tasks.insert("login", task);
        self.changed(cx);
    }

    pub fn continue_as_guest(&mut self, cx: &mut Context<Self>) {
        self.back.clear();
        self.navigate(Route::Home, cx);
    }

    pub fn logout(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            session.logout();
        }
        self.set_session(None, cx);
        self.route = Route::Login;
        self.changed(cx);
    }

    fn set_session(&mut self, session: Option<Session>, cx: &mut Context<Self>) {
        self.client = Client::new(session.clone());
        self.session = session;
        self.account = None;
        self.session_expired = false;
        self.playlists.clear();
        self.pages.clear();
        self.expanded.clear();
        self.back.clear();
        self.spawn_player();

        if self.session.is_some() {
            let client = self.client.clone();
            let request = cx.background_executor().spawn(async move { client.account() });
            let task = cx.spawn(async move |this, cx| {
                let result = request.await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(account) => this.account = Some(account),
                        Err(_) => this.session_expired = true,
                    }
                    this.changed(cx);
                })
                .ok();
            });
            self.tasks.insert("account", task);
            self.load_playlists(cx);
            self.route = Route::Home;
            self.ensure_loaded(cx);
        }
        self.changed(cx);
    }

    // ---- Playback ---------------------------------------------------------------------

    fn spawn_player(&mut self) {
        self.player = None; // Drop the old player first so it releases the sound device.
        self.queue.clear();
        self.current = None;
        self.playback = PlaybackState { idle: true, ..Default::default() };
        self.seen_finished = 0;
        self.seen_failed = 0;
        match Player::spawn() {
            Ok(player) => {
                self.player = Some(player);
                self.player_error = None;
            }
            Err(err) => self.player_error = Some(err.into()),
        }
    }

    pub fn with_player(&mut self, f: impl FnOnce(&mut Player)) {
        if let Some(player) = &mut self.player {
            f(player);
        }
    }

    /// Brings back the last session's queue and song, paused where it was left.
    fn restore_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(saved) = crate::persist::load() else { return };
        if saved.volume > 0.0 {
            self.volume.update(cx, |slider, cx| slider.set_value(saved.volume as f32, window, cx));
            self.with_player(|p| p.set_volume(saved.volume));
            self.playback.volume = saved.volume;
        }
        self.queue = saved.queue;
        self.current = saved.current;
        if let Some(track) = self.current_track() {
            let duration = track.duration_secs().unwrap_or(0.0);
            self.resume_at = Some(saved.position.min(duration.max(saved.position)));
            self.playback.position = saved.position;
            self.playback.duration = duration;
        }
        self.last_saved = (self.queue.len(), self.current_track().map(|t| t.video_id.clone()), Instant::now());
        self.changed(cx);
    }

    pub fn save_state(&self) {
        crate::persist::save(&crate::persist::SavedState {
            queue: self.queue.clone(),
            current: self.current,
            position: self.resume_at.unwrap_or(self.playback.position),
            volume: self.playback.volume,
        });
    }

    /// Saves when the queue or current song changed, or every 10 s while playing.
    fn save_if_needed(&mut self) {
        let fingerprint = (self.queue.len(), self.current_track().map(|t| t.video_id.clone()));
        let playing = !self.playback.paused && !self.playback.idle;
        let (len, current, at) = &self.last_saved;
        let changed = (*len, current.clone()) != fingerprint;
        if changed || (playing && at.elapsed() >= Duration::from_secs(10)) {
            self.save_state();
            self.last_saved = (fingerprint.0, fingerprint.1, Instant::now());
        }
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.current.and_then(|ix| self.queue.get(ix))
    }

    fn sync_playback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        let mut state = player.state();
        if let Some(at) = self.resume_at {
            state.position = at;
            state.duration = self.playback.duration;
            state.paused = true;
        }
        if state.finished_count != self.seen_finished {
            self.seen_finished = state.finished_count;
            self.consecutive_failures = 0;
            self.next(cx);
        }
        if state.failed_count != self.seen_failed {
            self.seen_failed = state.failed_count;
            self.consecutive_failures += 1;
            let reason = state.last_error.clone().unwrap_or_else(|| "unknown error".into());
            self.player_error = Some(format!("Couldn't play this song: {reason}").into());
            if self.consecutive_failures < 3 {
                self.next(cx);
            }
            self.changed(cx);
        }
        if state.position > 0.0 && self.player_error.is_some() && state.last_error.is_none() {
            // Something is playing again; clear the old error.
            self.player_error = None;
            self.consecutive_failures = 0;
        }
        if !self.seeking {
            let ratio = if state.duration > 0.0 { (state.position / state.duration).clamp(0.0, 1.0) } else { 0.0 };
            self.progress.update(cx, |slider, cx| slider.set_value(ratio as f32 * PROGRESS_STEPS, window, cx));
        }
        // Re-render only when something visible changes: the shown time moves once a second.
        let shown_second_changed = state.position.floor() != self.playback.position.floor()
            || state.duration.floor() != self.playback.duration.floor();
        if shown_second_changed || state.paused != self.playback.paused || state.idle != self.playback.idle {
            cx.notify();
        }
        self.playback = state;
        self.save_if_needed();
    }

    pub fn play_queue(&mut self, tracks: Vec<Track>, start: usize, cx: &mut Context<Self>) {
        self.queue = tracks;
        self.play_index(start, cx);
    }

    pub fn play_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        let (Some(player), Some(track)) = (&mut self.player, self.queue.get(ix)) else { return };
        self.resume_at = None;
        player.load(&track.url());
        // Look up the next song's stream now, so skipping to it starts quickly.
        if let Some(next) = self.queue.get(ix + 1) {
            player.prefetch(&next.url());
        }
        self.current = Some(ix);
        self.playback.position = 0.0;
        self.playback.duration = 0.0;
        self.changed(cx);
    }

    /// Plays a single song followed by YouTube Music's suggestions for it.
    pub fn play_radio(&mut self, track: Track, cx: &mut Context<Self>) {
        let video_id = track.video_id.clone();
        self.play_queue(vec![track], 0, cx);
        let client = self.client.clone();
        let request = cx.background_executor().spawn({
            let video_id = video_id.clone();
            async move { client.up_next(Some(&video_id), None) }
        });
        let task = cx.spawn(async move |this, cx| {
            let Ok(suggestions) = request.await else { return };
            this.update(cx, |this, cx| {
                // Only extend if the user is still on the song this radio is for.
                if this.current_track().map(|t| &t.video_id) == Some(&video_id) {
                    let ix = this.current.unwrap_or(0);
                    this.queue.truncate(ix + 1);
                    this.queue.extend(suggestions.into_iter().filter(|t| t.video_id != video_id));
                    this.changed(cx);
                }
            })
            .ok();
        });
        self.tasks.insert("radio", task);
    }

    /// Plays a watch target (a playlist, album, or video) using its "up next" queue.
    pub fn play_watch(&mut self, video_id: Option<String>, playlist_id: Option<String>, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let request = cx
            .background_executor()
            .spawn(async move { client.up_next(video_id.as_deref(), playlist_id.as_deref()) });
        let task = cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| match result {
                Ok(tracks) if !tracks.is_empty() => this.play_queue(tracks, 0, cx),
                _ => {
                    this.player_error = Some("Couldn't load that queue".into());
                    this.changed(cx);
                }
            })
            .ok();
        });
        self.tasks.insert("radio", task);
    }

    /// Plays a track from the open album/playlist page, queueing the rest of the page.
    pub fn play_from_page(&mut self, index: usize, shuffle: bool, cx: &mut Context<Self>) {
        let Some(Load::Ready(page)) = self.pages.get(&self.route.key()) else { return };
        let mut tracks = page.tracks();
        if shuffle {
            shuffle_tracks(&mut tracks);
        }
        self.play_queue(tracks, index, cx);
    }

    pub fn toggle_fps(&mut self, cx: &mut Context<Self>) {
        self.fps = match self.fps {
            Some(_) => None,
            None => Some(FpsMeter::default()),
        };
        cx.notify();
    }

    pub fn toggle_pause(&mut self) {
        // A song restored from the last session isn't loaded yet: start it where it left off.
        if let Some(at) = self.resume_at.take() {
            let url = self.current_track().map(Track::url);
            let next = self.current.and_then(|ix| self.queue.get(ix + 1)).map(Track::url);
            if let (Some(player), Some(url)) = (&mut self.player, url) {
                player.load_at(&url, at, false);
                if let Some(next) = next {
                    player.prefetch(&next);
                }
            }
            return;
        }
        self.with_player(Player::toggle_pause);
    }

    pub fn previous(&mut self, cx: &mut Context<Self>) {
        // Like most players: restart the song unless we're near its start.
        if self.playback.position > 5.0 {
            match &mut self.resume_at {
                Some(at) => *at = 0.0, // Restored but not loaded yet: resume from the start.
                None => self.with_player(|p| p.seek_to(0.0)),
            }
        } else if let Some(ix) = self.current.filter(|ix| *ix > 0) {
            self.play_index(ix - 1, cx);
        }
    }

    pub fn next(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.current else { return };
        if ix + 1 < self.queue.len() {
            self.play_index(ix + 1, cx);
        } else {
            self.extend_with_suggestions(cx);
        }
    }

    /// When the queue runs out, keep the music going with suggestions for the last song.
    fn extend_with_suggestions(&mut self, cx: &mut Context<Self>) {
        let Some(last) = self.queue.last().map(|t| t.video_id.clone()) else { return };
        if self.extending_queue {
            return;
        }
        self.extending_queue = true;
        let client = self.client.clone();
        let request = cx.background_executor().spawn(async move { client.up_next(Some(&last), None) });
        let task = cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.extending_queue = false;
                let known: HashSet<String> = this.queue.iter().map(|t| t.video_id.clone()).collect();
                let fresh: Vec<Track> = result.unwrap_or_default().into_iter().filter(|t| !known.contains(&t.video_id)).collect();
                if fresh.is_empty() {
                    this.with_player(Player::stop);
                    this.current = None;
                    this.changed(cx);
                    return;
                }
                let next = this.queue.len();
                this.queue.extend(fresh);
                this.play_index(next, cx);
            })
            .ok();
        });
        self.tasks.insert("extend", task);
    }

    pub fn shuffle_upcoming(&mut self, cx: &mut Context<Self>) {
        let start = self.current.map_or(0, |ix| ix + 1);
        if start < self.queue.len() {
            shuffle_tracks(&mut self.queue[start..]);
            self.changed(cx);
        }
    }
}

/// Fisher–Yates with a tiny xorshift seeded from the clock; good enough for a playlist.
fn shuffle_tracks(tracks: &mut [Track]) {
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545F4914F6CDD1D)
        | 1;
    for i in (1..tracks.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        tracks.swap(i, (seed % (i as u64 + 1)) as usize);
    }
}
