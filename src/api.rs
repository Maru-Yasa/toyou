//! A small YouTube Music (InnerTube) client: home, explore, library, browse pages,
//! search and radio. Responses are deeply nested JSON; we pull out only what we render.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::{ORIGIN, Session};

const API: &str = "https://music.youtube.com/youtubei/v1";
const CLIENT_VERSION: &str = "1.20250101.01.00";
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Browse { id: String, params: Option<String> },
    Watch { video_id: Option<String>, playlist_id: Option<String> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artists: String,
    pub album: String,
    pub duration: String,
    pub thumbnail: Option<String>,
}

impl Track {
    pub fn url(&self) -> String {
        format!("{ORIGIN}/watch?v={}", self.video_id)
    }

    /// The listed duration ("3:45" or "1:02:03") in seconds, if it parses.
    pub fn duration_secs(&self) -> Option<f64> {
        self.duration.split(':').try_fold(0.0, |total, part| Some(total * 60.0 + part.trim().parse::<f64>().ok()?))
    }
}

#[derive(Clone, Debug)]
pub struct Card {
    pub title: String,
    pub subtitle: String,
    pub thumbnail: Option<String>,
    pub target: Target,
    /// Artists are shown with round artwork.
    pub round: bool,
    /// Text-only navigation buttons such as moods and genres.
    pub chip: bool,
}

#[derive(Clone, Debug)]
pub enum Item {
    Track(Track),
    Card(Card),
}

#[derive(Clone, Debug)]
pub struct Section {
    pub title: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Default)]
pub struct Header {
    pub title: String,
    pub subtitle: String,
    pub thumbnail: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Page {
    pub header: Option<Header>,
    pub sections: Vec<Section>,
    /// Token for loading more sections (Home), fetched lazily as the user scrolls.
    pub continuation: Option<String>,
}

impl Page {
    /// All playable tracks on the page, in order (used by detail pages).
    pub fn tracks(&self) -> Vec<Track> {
        self.sections
            .iter()
            .flat_map(|s| &s.items)
            .filter_map(|item| match item {
                Item::Track(t) => Some(t.clone()),
                Item::Card(_) => None,
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct Account {
    pub name: String,
    pub photo: Option<String>,
}

#[derive(Clone, Default)]
pub struct Client {
    session: Option<Arc<Session>>,
}

impl Client {
    pub fn new(session: Option<Session>) -> Self {
        Self { session: session.map(Arc::new) }
    }

    pub fn is_logged_in(&self) -> bool {
        self.session.is_some()
    }

    fn post(&self, endpoint: &str, mut body: Value) -> Result<Value, String> {
        body["context"] = json!({
            "client": { "clientName": "WEB_REMIX", "clientVersion": CLIENT_VERSION, "hl": "en" }
        });
        let mut request = ureq::post(&format!("{API}/{endpoint}?prettyPrint=false"))
            .set("Origin", ORIGIN)
            .set("X-Origin", ORIGIN)
            .set("User-Agent", USER_AGENT);
        if let Some(session) = &self.session {
            request = request
                .set("Cookie", session.cookie_header())
                .set("Authorization", &session.authorization())
                .set("X-Goog-AuthUser", "0");
        }
        request
            .send_json(body)
            .map_err(|e| match e {
                ureq::Error::Status(401 | 403, _) => "YouTube rejected the session; try signing in again".into(),
                e => e.to_string(),
            })?
            .into_json()
            .map_err(|e| e.to_string())
    }

    pub fn browse(&self, id: &str, params: Option<&str>) -> Result<Page, String> {
        let mut body = json!({ "browseId": id });
        if let Some(params) = params {
            body["params"] = json!(params);
        }
        Ok(parse_page(&self.post("browse", body)?))
    }

    /// Personalized when signed in: "Quick picks", mixes, recommendations, etc.
    /// Only the first sections; the rest come from [`Client::home_more`].
    pub fn home(&self) -> Result<Page, String> {
        let response = self.post("browse", json!({ "browseId": "FEmusic_home" }))?;
        let mut page = parse_page(&response);
        // Guests get the first page again instead of more sections, so only page when signed in.
        if self.is_logged_in() {
            page.continuation = continuation_token(&response);
        }
        Ok(page)
    }

    /// The next batch of Home sections and the token for the batch after it.
    pub fn home_more(&self, token: &str) -> Result<(Vec<Section>, Option<String>), String> {
        let response = self.post("browse", json!({ "continuation": token }))?;
        let continuation = response.pointer("/continuationContents/sectionListContinuation");
        let sections = continuation
            .and_then(|c| c.get("contents"))
            .and_then(Value::as_array)
            .map(|c| parse_sections(c))
            .unwrap_or_default();
        let next = continuation
            .and_then(|c| c.pointer("/continuations/0/nextContinuationData/continuation"))
            .and_then(Value::as_str)
            .map(String::from);
        Ok((sections, next))
    }

    pub fn explore(&self) -> Result<Page, String> {
        self.browse("FEmusic_explore", None)
    }

    pub fn library(&self) -> Result<Page, String> {
        let mut page = self.browse("FEmusic_liked_playlists", None)?;
        let liked = Card {
            title: "Liked music".into(),
            subtitle: "Auto playlist".into(),
            thumbnail: None,
            target: Target::Browse { id: "VLLM".into(), params: None },
            round: false,
            chip: false,
        };
        // The liked-songs playlist is sometimes already listed; avoid showing it twice.
        let has_liked = page.sections.iter().flat_map(|s| &s.items).any(|item| {
            matches!(item, Item::Card(c) if c.target == liked.target)
        });
        match page.sections.first_mut() {
            Some(first) if !has_liked => first.items.insert(0, Item::Card(liked)),
            None => page.sections.push(Section { title: String::new(), items: vec![Item::Card(liked)] }),
            _ => {}
        }
        for section in page.sections.iter_mut().filter(|s| s.title.is_empty()) {
            section.title = "Playlists".into();
        }
        // Drop the "New playlist" button tile.
        for section in &mut page.sections {
            section.items.retain(|item| !matches!(item, Item::Card(c) if c.subtitle.is_empty() && c.thumbnail.is_none() && c.title == "New playlist"));
        }
        Ok(page)
    }

    pub fn search(&self, query: &str) -> Result<Page, String> {
        let response = self.post("search", json!({ "query": query }))?;
        let contents = response
            .pointer("/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // Results arrive as a "top result" card plus a flat run of single-item sections;
        // regroup them by kind.
        let (mut top, mut songs, mut artists, mut collections) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for section in &contents {
            if let Some(card) = section.get("musicCardShelfRenderer") {
                // Songs under the top result omit the artist; it's the card's title.
                let artist = runs_text(card.get("title"));
                if let Some(target) = card.pointer("/title/runs/0/navigationEndpoint").and_then(parse_target) {
                    top.push(Item::Card(Card {
                        title: runs_text(card.get("title")),
                        subtitle: runs_text(card.get("subtitle")),
                        thumbnail: best_thumbnail(card.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"), 226),
                        round: page_type(card.pointer("/title/runs/0/navigationEndpoint")) == Some("MUSIC_PAGE_TYPE_ARTIST"),
                        target,
                        chip: false,
                    }));
                }
                songs.extend(card.get("contents").and_then(Value::as_array).into_iter().flatten().filter_map(parse_item).map(|item| match item {
                    Item::Track(track) if track.artists.is_empty() => Item::Track(Track { artists: artist.clone(), ..track }),
                    item => item,
                }));
                continue;
            }
            let items = ["itemSectionRenderer", "musicShelfRenderer"]
                .iter()
                .find_map(|kind| section.pointer(&format!("/{kind}/contents"))?.as_array());
            for item in items.into_iter().flatten().filter_map(parse_item) {
                match &item {
                    Item::Track(_) => songs.push(item),
                    Item::Card(card) if card.round => artists.push(item),
                    Item::Card(_) => collections.push(item),
                }
            }
        }
        let sections = [("Top result", top), ("Songs", songs), ("Artists", artists), ("Albums & playlists", collections)]
            .into_iter()
            .filter(|(_, items)| !items.is_empty())
            .map(|(title, items)| Section { title: title.into(), items })
            .collect();
        Ok(Page { header: None, sections, continuation: None })
    }

    /// "Up next" for a song or playlist: a radio of suggestions, or the playlist's queue.
    pub fn up_next(&self, video_id: Option<&str>, playlist_id: Option<&str>) -> Result<Vec<Track>, String> {
        let mut body = json!({ "isAudioOnly": true, "enablePersistentPlaylistPanel": true });
        if let Some(video_id) = video_id {
            body["videoId"] = json!(video_id);
            if playlist_id.is_none() {
                body["playlistId"] = json!(format!("RDAMVM{video_id}"));
            }
        }
        if let Some(playlist_id) = playlist_id {
            body["playlistId"] = json!(playlist_id);
        }
        let response = self.post("next", body)?;
        let items = response
            .pointer("/contents/singleColumnMusicWatchNextResultsRenderer/tabbedRenderer/watchNextTabbedResultsRenderer/tabs/0/tabRenderer/content/musicQueueRenderer/content/playlistPanelRenderer/contents")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(items
            .iter()
            .filter_map(|item| {
                item.get("playlistPanelVideoRenderer")
                    .or_else(|| item.pointer("/playlistPanelVideoWrapperRenderer/primaryRenderer/playlistPanelVideoRenderer"))
            })
            .filter_map(parse_panel_video)
            .collect())
    }

    pub fn account(&self) -> Result<Account, String> {
        let response = self.post("account/account_menu", json!({}))?;
        let header = find_key(&response, "activeAccountHeaderRenderer").ok_or("not signed in")?;
        Ok(Account {
            name: runs_text(header.get("accountName")),
            photo: best_thumbnail(header.pointer("/accountPhoto/thumbnails"), 88),
        })
    }
}

fn continuation_token(response: &Value) -> Option<String> {
    find_key(response, "nextContinuationData")?
        .get("continuation")?
        .as_str()
        .map(String::from)
}

fn parse_page(response: &Value) -> Page {
    let mut sections = Vec::new();
    let mut header = response.get("header").and_then(parse_header);

    let tab_contents = response
        .pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents")
        .or_else(|| response.pointer("/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents"))
        .and_then(Value::as_array);
    if let Some(contents) = tab_contents {
        for section in contents {
            if header.is_none() {
                header = parse_header(section);
            }
        }
        sections.extend(parse_sections(contents));
    }
    if let Some(secondary) = response
        .pointer("/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents")
        .and_then(Value::as_array)
    {
        sections.extend(parse_sections(secondary));
    }

    // Album tracks carry no artwork or artist of their own; inherit them from the header.
    if let Some(header) = &header {
        for item in sections.iter_mut().flat_map(|s| s.items.iter_mut()) {
            if let Item::Track(track) = item {
                if track.thumbnail.is_none() {
                    track.thumbnail = header.thumbnail.clone();
                }
                if track.artists.is_empty() {
                    track.artists = header.subtitle.clone();
                }
            }
        }
    }
    Page { header, sections, continuation: None }
}

fn parse_header(value: &Value) -> Option<Header> {
    const KINDS: &[&str] = &[
        "musicResponsiveHeaderRenderer",
        "musicImmersiveHeaderRenderer",
        "musicVisualHeaderRenderer",
        "musicDetailHeaderRenderer",
    ];
    let value = value
        .pointer("/musicEditablePlaylistDetailHeaderRenderer/header")
        .unwrap_or(value);
    let renderer = KINDS.iter().find_map(|k| value.get(*k))?;
    let subtitle = [renderer.get("straplineTextOne"), renderer.get("subtitle")]
        .into_iter()
        .map(runs_text)
        .find(|s| !s.is_empty())
        .unwrap_or_default();
    let thumbnail = find_key(renderer.get("thumbnail").unwrap_or(&Value::Null), "thumbnails")
        .and_then(|t| best_thumbnail(Some(t), 400));
    Some(Header { title: runs_text(renderer.get("title")), subtitle, thumbnail })
}

fn parse_sections(contents: &[Value]) -> Vec<Section> {
    contents
        .iter()
        .filter_map(|section| {
            let (renderer, items_key) = [
                ("musicCarouselShelfRenderer", "contents"),
                ("musicImmersiveCarouselShelfRenderer", "contents"),
                ("musicShelfRenderer", "contents"),
                ("musicPlaylistShelfRenderer", "contents"),
                ("gridRenderer", "items"),
            ]
            .iter()
            .find_map(|(kind, key)| Some((section.get(*kind)?, *key)))?;

            let title = [
                renderer.pointer("/header/musicCarouselShelfBasicHeaderRenderer/title"),
                renderer.pointer("/header/gridHeaderRenderer/title"),
                renderer.get("title"),
            ]
            .into_iter()
            .map(runs_text)
            .find(|t| !t.is_empty())
            .unwrap_or_default();
            let items: Vec<Item> = renderer.get(items_key)?.as_array()?.iter().filter_map(parse_item).collect();
            (!items.is_empty()).then_some(Section { title, items })
        })
        .collect()
}

fn parse_item(value: &Value) -> Option<Item> {
    if let Some(r) = value.get("musicTwoRowItemRenderer") {
        let target = parse_target(r.get("navigationEndpoint")?)?;
        let thumbnail = best_thumbnail(r.pointer("/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails"), 226);
        let round = page_type(r.get("navigationEndpoint")) == Some("MUSIC_PAGE_TYPE_ARTIST");
        return Some(Item::Card(Card {
            title: runs_text(r.get("title")),
            subtitle: runs_text(r.get("subtitle")),
            thumbnail,
            target,
            round,
            chip: false,
        }));
    }
    if let Some(r) = value.get("musicNavigationButtonRenderer") {
        return Some(Item::Card(Card {
            title: runs_text(r.get("buttonText")),
            subtitle: String::new(),
            thumbnail: None,
            target: parse_target(r.get("clickCommand")?)?,
            round: false,
            chip: true,
        }));
    }
    let r = value.get("musicResponsiveListItemRenderer")?;
    let column = |ix: usize| {
        runs_text(r.pointer(&format!("/flexColumns/{ix}/musicResponsiveListItemFlexColumnRenderer/text")))
    };
    let thumbnail = best_thumbnail(r.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"), 60);

    let video_id = r
        .pointer("/playlistItemData/videoId")
        .or_else(|| r.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/watchEndpoint/videoId"))
        .or_else(|| r.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId"))
        .and_then(Value::as_str);

    let Some(video_id) = video_id else {
        // Not a song: an artist, album or playlist row (e.g. in search results).
        let endpoint = r.get("navigationEndpoint")?;
        return Some(Item::Card(Card {
            title: column(0),
            subtitle: column(1),
            thumbnail,
            target: parse_target(endpoint)?,
            round: page_type(Some(endpoint)) == Some("MUSIC_PAGE_TYPE_ARTIST"),
            chip: false,
        }));
    };

    let details = column(1);
    let mut parts: Vec<&str> = details.split(" • ").map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.len() > 1 && matches!(parts[0], "Song" | "Video" | "Episode") {
        parts.remove(0);
    }
    let mut duration = runs_text(r.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text"));
    if parts.last().is_some_and(|p| is_duration(p)) {
        duration = parts.pop().unwrap_or_default().to_string();
    }
    parts.retain(|p| !is_play_count(p));
    let artists = parts.first().copied().unwrap_or_default().to_string();
    let album = match parts.get(1) {
        Some(album) => album.to_string(),
        None => Some(column(2)).filter(|c| !is_play_count(c)).unwrap_or_default(),
    };

    Some(Item::Track(Track { video_id: video_id.to_string(), title: column(0), artists, album, duration, thumbnail }))
}

fn parse_panel_video(r: &Value) -> Option<Track> {
    let byline = runs_text(r.get("longBylineText"));
    let mut parts = byline.split(" • ").map(str::trim);
    Some(Track {
        video_id: r.get("videoId")?.as_str()?.to_string(),
        title: runs_text(r.get("title")),
        artists: parts.next().unwrap_or_default().to_string(),
        album: parts.next().filter(|p| !is_play_count(p)).unwrap_or_default().to_string(),
        duration: runs_text(r.get("lengthText")),
        thumbnail: best_thumbnail(r.pointer("/thumbnail/thumbnails"), 60),
    })
}

fn parse_target(endpoint: &Value) -> Option<Target> {
    if let Some(browse) = endpoint.get("browseEndpoint") {
        return Some(Target::Browse {
            id: browse.get("browseId")?.as_str()?.to_string(),
            params: browse.get("params").and_then(Value::as_str).map(String::from),
        });
    }
    let watch = endpoint.get("watchEndpoint").or_else(|| endpoint.get("watchPlaylistEndpoint"))?;
    Some(Target::Watch {
        video_id: watch.get("videoId").and_then(Value::as_str).map(String::from),
        playlist_id: watch.get("playlistId").and_then(Value::as_str).map(String::from),
    })
}

fn page_type(endpoint: Option<&Value>) -> Option<&str> {
    endpoint?
        .pointer("/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")?
        .as_str()
}

fn runs_text(value: Option<&Value>) -> String {
    let Some(value) = value else { return String::new() };
    if let Some(text) = value.get("simpleText").and_then(Value::as_str) {
        return text.to_string();
    }
    value
        .get("runs")
        .and_then(Value::as_array)
        .map(|runs| runs.iter().filter_map(|r| r.get("text")?.as_str()).collect())
        .unwrap_or_default()
}

/// The smallest thumbnail at least `min_width` wide, or the largest available.
fn best_thumbnail(thumbnails: Option<&Value>, min_width: u64) -> Option<String> {
    let list = thumbnails?.as_array()?;
    let width = |t: &Value| t.get("width").and_then(Value::as_u64).unwrap_or(0);
    list.iter()
        .filter(|t| width(t) >= min_width)
        .min_by_key(|t| width(t))
        .or_else(|| list.iter().max_by_key(|t| width(t)))
        .and_then(|t| t.get("url")?.as_str())
        .map(|url| if url.starts_with("//") { format!("https:{url}") } else { url.to_string() })
}

fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map.get(key).or_else(|| map.values().find_map(|v| find_key(v, key))),
        Value::Array(list) => list.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}

fn is_duration(text: &str) -> bool {
    text.contains(':') && text.chars().all(|c| c.is_ascii_digit() || c == ':')
}

fn is_play_count(text: &str) -> bool {
    text.ends_with(" plays") || text.ends_with(" views") || text.ends_with(" play") || text.ends_with(" view")
}

#[cfg(test)]
mod tests {
    use super::*;

    // These hit the live API as a guest; run with `cargo test -- --ignored --nocapture`.

    fn summarize(page: &Page) {
        for section in &page.sections {
            let tracks = section.items.iter().filter(|i| matches!(i, Item::Track(_))).count();
            println!("{:40} {} items ({tracks} tracks)", section.title, section.items.len());
        }
    }

    #[test]
    #[ignore]
    fn live_home_and_explore() {
        let client = Client::default();
        let home = client.home().unwrap();
        summarize(&home);
        assert!(!home.sections.is_empty());
        let explore = client.explore().unwrap();
        summarize(&explore);
        assert!(explore.sections.iter().any(|s| s.items.iter().any(|i| matches!(i, Item::Card(c) if c.chip))));
    }

    #[test]
    #[ignore]
    fn live_search_album_and_radio() {
        let client = Client::default();
        let results = client.search("daft punk").unwrap();
        summarize(&results);
        let album = results
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .find_map(|i| match i {
                Item::Card(Card { target: Target::Browse { id, .. }, .. }) if id.starts_with("MPRE") => Some(id.clone()),
                _ => None,
            })
            .expect("an album in search results");
        let page = client.browse(&album, None).unwrap();
        let header = page.header.clone().expect("album header");
        println!("album: {} — {}", header.title, header.subtitle);
        let tracks = page.tracks();
        assert!(!tracks.is_empty());
        for t in tracks.iter().take(3) {
            println!("  {} | {} | {}", t.title, t.artists, t.duration);
        }
        let radio = client.up_next(Some(&tracks[0].video_id), None).unwrap();
        println!("radio: {} tracks, e.g. {} — {}", radio.len(), radio[1].title, radio[1].artists);
        assert!(radio.len() > 5);
    }
}
