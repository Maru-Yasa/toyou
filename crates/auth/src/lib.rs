//! YouTube Music sign-in: the session's cookies, stored as a Netscape cookie file. They come
//! from the sign-in window (the `webview` crate) or from a pasted `Cookie` header.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const ORIGIN: &str = "https://music.youtube.com";

/// Exit codes of the `toyou-login` sign-in window, read by the main app.
pub const LOGIN_EXIT_SIGNED_IN: i32 = 0;
pub const LOGIN_EXIT_CANCELLED: i32 = 2;

/// One cookie, as stored in a Netscape cookie file.
pub struct CookieRecord {
    pub domain: String,
    pub path: String,
    pub secure: bool,
    /// Unix time; 0 for a session cookie.
    pub expires: u64,
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub cookie_file: PathBuf,
    cookie_header: String,
    sapisid: String,
}

impl Session {
    /// The previously saved session, if any.
    pub fn load() -> Option<Self> {
        Self::from_cookie_file(&cookie_path()).ok()
    }

    /// Accepts the value of a `Cookie:` request header copied from the browser dev tools.
    pub fn from_cookie_header(header: &str) -> Result<Self, String> {
        let header = header.trim().trim_start_matches("Cookie:").trim_start_matches("cookie:").trim();
        let records: Vec<CookieRecord> = header
            .split(';')
            .filter_map(|pair| pair.trim().split_once('='))
            .map(|(name, value)| CookieRecord {
                domain: ".youtube.com".into(),
                path: "/".into(),
                secure: true,
                expires: 2147483647,
                name: name.trim().into(),
                value: value.trim().into(),
            })
            .collect();
        Self::save_cookies(&records)
    }

    /// Validates the cookies contain a YouTube login, then stores them as the session.
    pub fn save_cookies(records: &[CookieRecord]) -> Result<Self, String> {
        let mut file = String::from("# Netscape HTTP Cookie File\n");
        for c in records {
            let subdomains = if c.domain.starts_with('.') { "TRUE" } else { "FALSE" };
            let secure = if c.secure { "TRUE" } else { "FALSE" };
            file.push_str(&format!("{}\t{subdomains}\t{}\t{secure}\t{}\t{}\t{}\n", c.domain, c.path, c.expires, c.name, c.value));
        }
        let target = cookie_path();
        let tmp = target.with_extension("tmp");
        std::fs::write(&tmp, file).map_err(|e| e.to_string())?;
        restrict_permissions(&tmp);
        let session = Self::from_cookie_file(&tmp).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
        Ok(Session { cookie_file: target, ..session })
    }

    pub fn from_cookie_file(path: &Path) -> Result<Self, String> {
        let mut cookies: Vec<(String, String)> = Vec::new();
        for record in read_cookie_file(path) {
            match cookies.iter_mut().find(|(n, _)| *n == record.name) {
                Some(existing) => existing.1 = record.value,
                None => cookies.push((record.name, record.value)),
            }
        }
        let has = |names: &[&str]| names.iter().find_map(|key| cookies.iter().find(|(n, _)| n == key).map(|(_, v)| v.clone()));
        let sapisid = has(&["SAPISID", "__Secure-3PAPISID"]);
        let signed_in = has(&["SID", "__Secure-1PSID", "__Secure-3PSID"]).is_some();
        let (Some(sapisid), true) = (sapisid, signed_in) else {
            return Err("No YouTube login found. Make sure you're signed in to music.youtube.com there.".into());
        };
        let cookie_header = cookies.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; ");
        Ok(Session { cookie_file: path.to_path_buf(), cookie_header, sapisid })
    }

    pub fn cookie_header(&self) -> &str {
        &self.cookie_header
    }

    /// The `SAPISIDHASH` authorization header YouTube expects alongside cookies.
    pub fn authorization(&self) -> String {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let hash = sha1_smol::Sha1::from(format!("{ts} {} {ORIGIN}", self.sapisid)).digest().to_string();
        format!("SAPISIDHASH {ts}_{hash}")
    }

    pub fn logout(&self) {
        let _ = std::fs::remove_file(&self.cookie_file);
    }
}

/// The youtube.com cookies in a Netscape cookie file.
fn read_cookie_file(path: &Path) -> Vec<CookieRecord> {
    let Ok(contents) = std::fs::read_to_string(path) else { return Vec::new() };
    contents
        .lines()
        // Cookie files mark HttpOnly cookies with this prefix; they are still cookies.
        .map(|line| line.strip_prefix("#HttpOnly_").unwrap_or(line))
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let domain = f.first()?.trim_start_matches('.');
            (f.len() >= 7 && (domain == "youtube.com" || domain.ends_with(".youtube.com"))).then(|| CookieRecord {
                domain: f[0].to_string(),
                path: f[2].to_string(),
                secure: f[3] == "TRUE",
                expires: f[4].parse().unwrap_or(0),
                name: f[5].to_string(),
                value: f[6].to_string(),
            })
        })
        .collect()
}

/// `~/.config/toyou`, where the session and rustypipe's cache live.
pub fn config_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    let dir = config.join("toyou");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn cookie_path() -> PathBuf {
    config_dir().unwrap_or_else(std::env::temp_dir).join("cookies.txt")
}

fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}
