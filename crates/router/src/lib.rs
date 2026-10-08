//! Where the app is: the pages toyou can show.

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Login,
    Home,
    Explore,
    Library,
    Search(String),
    Browse { id: String, params: Option<String> },
    /// Large artwork plus Up next / Lyrics / Related for the current song.
    NowPlaying,
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
            Route::NowPlaying => "now-playing".into(),
        }
    }
}
