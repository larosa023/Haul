//! Real-Debrid (https://real-debrid.com): premium-speed downloads from file hosters, and
//! torrents downloaded on Real-Debrid's servers and then fetched over plain HTTPS.
//!
//! Uses the personal API token from https://real-debrid.com/apitoken (Bearer auth).
//! API: https://api.real-debrid.com/rest/1.0/

use std::collections::HashSet;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{secret, settings};

const BASE: &str = "https://api.real-debrid.com/rest/1.0";

#[cfg(test)]
pub static TEST_BASE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn base() -> &'static str {
    #[cfg(test)]
    if let Some(b) = TEST_BASE.get() {
        return b;
    }
    BASE
}

pub fn key() -> String {
    secret::open(&settings::with(|s| s.debrid_key.clone()))
}

/// Connected and switched on.
pub fn active() -> bool {
    settings::with(|s| s.debrid_enabled && !s.debrid_key.is_empty())
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct Account {
    pub username: String,
    pub email: String,
    /// "premium" or "free"
    #[serde(rename(deserialize = "type"))]
    pub kind: String,
    /// Seconds of premium left.
    pub premium: i64,
    /// ISO date premium ends.
    pub expiration: String,
    pub points: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Unrestricted {
    pub filename: String,
    pub filesize: u64,
    /// The direct download link.
    pub download: String,
    pub host: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RdFile {
    pub id: u64,
    /// Starts with "/".
    pub path: String,
    pub bytes: u64,
    pub selected: u8,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TorrentInfo {
    pub id: String,
    pub filename: String,
    pub hash: String,
    pub bytes: u64,
    pub progress: f64,
    pub status: String,
    pub files: Vec<RdFile>,
    pub links: Vec<String>,
    /// Bytes per second (while Real-Debrid downloads it).
    pub speed: u64,
    pub seeders: u32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ApiError {
    error: String,
    error_code: i64,
}

/// Real-Debrid's error codes, in plain words.
fn explain(code: i64, raw: &str) -> String {
    match code {
        8 | 9 => "Real-Debrid didn't accept the API key (check it in Settings → Debrid)".into(),
        16 => "Real-Debrid doesn't support this hoster".into(),
        19 | 21 | 24 => "The file isn't available on the hoster any more".into(),
        20 => "Real-Debrid premium is needed for this".into(),
        23 => "Real-Debrid's traffic limit for this hoster is used up for today".into(),
        34 => "Real-Debrid is busy (too many requests); trying again shortly".into(),
        35 => "Real-Debrid blocked this file (infringing)".into(),
        _ if raw.is_empty() => format!("Real-Debrid error {code}"),
        _ => format!("Real-Debrid: {}", raw.replace('_', " ")),
    }
}

async fn request(method: reqwest::Method, path: &str, form: Option<&[(&str, &str)]>, body: Option<Vec<u8>>, key: &str) -> Result<String, String> {
    if key.is_empty() {
        return Err("Add your Real-Debrid API key in Settings → Debrid".into());
    }
    let mut tries = 0;
    loop {
        let mut rq = crate::http::client()
            .request(method.clone(), format!("{}{path}", base()))
            .bearer_auth(key)
            .timeout(Duration::from_secs(30));
        if let Some(f) = form {
            rq = rq.form(f);
        }
        if let Some(b) = &body {
            rq = rq.body(b.clone());
        }
        let r = rq.send().await.map_err(|e| format!("Couldn't reach Real-Debrid: {e}"))?;
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        if status.is_success() {
            return Ok(text);
        }
        let err: ApiError = serde_json::from_str(&text).unwrap_or_default();
        // Rate limit (250 requests a minute): wait and try again a couple of times.
        if (status.as_u16() == 429 || err.error_code == 34) && tries < 3 {
            tries += 1;
            tokio::time::sleep(Duration::from_secs(2 * tries)).await;
            continue;
        }
        if status.as_u16() == 401 {
            return Err(explain(8, ""));
        }
        return Err(explain(err.error_code, &err.error));
    }
}

async fn get<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T, String> {
    let t = request(reqwest::Method::GET, path, None, None, &key()).await?;
    serde_json::from_str(&t).map_err(|e| format!("Unexpected answer from Real-Debrid: {e}"))
}

/// Checks a key and returns the account it belongs to.
pub async fn account_for(key: &str) -> Result<Account, String> {
    let t = request(reqwest::Method::GET, "/user", None, None, key).await?;
    serde_json::from_str(&t).map_err(|e| format!("Unexpected answer from Real-Debrid: {e}"))
}

pub async fn account() -> Result<Account, String> {
    get("/user").await
}

/// Turns a hoster link into a direct download link.
pub async fn unrestrict(link: &str, password: &str) -> Result<Unrestricted, String> {
    let mut form = vec![("link", link)];
    if !password.is_empty() {
        form.push(("password", password));
    }
    let t = request(reqwest::Method::POST, "/unrestrict/link", Some(&form), None, &key()).await?;
    let u: Unrestricted = serde_json::from_str(&t).map_err(|e| format!("Unexpected answer from Real-Debrid: {e}"))?;
    if u.download.is_empty() {
        return Err("Real-Debrid didn't return a download link".into());
    }
    Ok(u)
}

/// Domains Real-Debrid can unrestrict (cached for an hour).
pub async fn hosts() -> HashSet<String> {
    static CACHE: Mutex<Option<(Instant, HashSet<String>)>> = Mutex::new(None);
    if let Some((at, h)) = CACHE.lock().as_ref() {
        if at.elapsed() < Duration::from_secs(3600) {
            return h.clone();
        }
    }
    // This list is public; no key needed.
    let list: Vec<String> = match crate::http::client().get(format!("{}/hosts/domains", base())).timeout(Duration::from_secs(20)).send().await {
        Ok(r) => r.json().await.unwrap_or_default(),
        Err(_) => vec![],
    };
    let set: HashSet<String> = list.into_iter().map(|d| d.to_ascii_lowercase()).collect();
    if !set.is_empty() {
        *CACHE.lock() = Some((Instant::now(), set.clone()));
    }
    set
}

/// Is this link on a hoster Real-Debrid supports?
pub fn host_supported(url: &str, hosts: &HashSet<String>) -> bool {
    let Some(host) = url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())) else { return false };
    let host = host.trim_start_matches("www.");
    hosts.iter().any(|d| host == d || host.ends_with(&format!(".{d}")))
}

/// Should this link go through Real-Debrid?
pub async fn should_unrestrict(url: &str) -> bool {
    if !active() || !settings::with(|s| s.debrid_hosters) {
        return false;
    }
    host_supported(url, &hosts().await)
}

pub async fn add_magnet(magnet: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Added {
        id: String,
    }
    let t = request(reqwest::Method::POST, "/torrents/addMagnet", Some(&[("magnet", magnet)]), None, &key()).await?;
    serde_json::from_str::<Added>(&t).map(|a| a.id).map_err(|e| format!("Unexpected answer from Real-Debrid: {e}"))
}

pub async fn add_torrent_file(bytes: Vec<u8>) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Added {
        id: String,
    }
    let t = request(reqwest::Method::PUT, "/torrents/addTorrent", None, Some(bytes), &key()).await?;
    serde_json::from_str::<Added>(&t).map(|a| a.id).map_err(|e| format!("Unexpected answer from Real-Debrid: {e}"))
}

pub async fn info(id: &str) -> Result<TorrentInfo, String> {
    get(&format!("/torrents/info/{id}")).await
}

/// `files`: Real-Debrid file ids, or empty for all.
pub async fn select_files(id: &str, files: &[u64]) -> Result<(), String> {
    let list = if files.is_empty() { "all".to_string() } else { files.iter().map(u64::to_string).collect::<Vec<_>>().join(",") };
    request(reqwest::Method::POST, &format!("/torrents/selectFiles/{id}"), Some(&[("files", &list)]), None, &key()).await.map(|_| ())
}

pub async fn delete(id: &str) {
    let _ = request(reqwest::Method::DELETE, &format!("/torrents/delete/{id}"), None, None, &key()).await;
}

/// What a Real-Debrid torrent status means for Haul.
pub enum Phase {
    /// Still turning the magnet into a file list.
    Converting,
    /// Waiting for us to pick files.
    PickFiles,
    /// Real-Debrid is downloading (or packing) it.
    Working,
    /// Ready: the links can be downloaded.
    Ready,
    Failed(String),
}

pub fn phase(status: &str) -> Phase {
    match status {
        "magnet_conversion" => Phase::Converting,
        "waiting_files_selection" => Phase::PickFiles,
        "queued" | "downloading" | "compressing" | "uploading" => Phase::Working,
        "downloaded" => Phase::Ready,
        "magnet_error" => Phase::Failed("Real-Debrid couldn't read this magnet link".into()),
        "virus" => Phase::Failed("Real-Debrid found a virus in this torrent".into()),
        "dead" => Phase::Failed("This torrent has no seeders on Real-Debrid".into()),
        "error" => Phase::Failed("Real-Debrid couldn't download this torrent".into()),
        other => Phase::Failed(format!("Real-Debrid status: {other}")),
    }
}

/// Short text for the list while Real-Debrid works on a torrent.
pub fn status_note(i: &TorrentInfo) -> String {
    match i.status.as_str() {
        "queued" => "Real-Debrid: queued".into(),
        "compressing" => "Real-Debrid: packing".into(),
        "uploading" => "Real-Debrid: almost ready".into(),
        "magnet_conversion" => "Real-Debrid: reading the magnet".into(),
        _ => format!("Real-Debrid: downloading · {} seeders", i.seeders),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_match() {
        let h: HashSet<String> = ["1fichier.com", "rapidgator.net"].iter().map(|s| s.to_string()).collect();
        assert!(host_supported("https://1fichier.com/?abc", &h));
        assert!(host_supported("https://www.rapidgator.net/file/x", &h));
        assert!(host_supported("https://dl.rapidgator.net/file/x", &h));
        assert!(!host_supported("https://notrapidgator.net/x", &h));
        assert!(!host_supported("nope", &h));
    }

    #[test]
    fn parses_api_shapes() {
        let i: TorrentInfo = serde_json::from_str(
            r#"{"id":"ABC","filename":"Show","hash":"dd82","bytes":1000,"original_bytes":1000,"host":"real-debrid.com","split":2000,
                "progress":42.5,"status":"downloading","added":"2026-01-01T00:00:00.000Z",
                "files":[{"id":1,"path":"/Show/e1.mkv","bytes":600,"selected":1},{"id":2,"path":"/Show/info.nfo","bytes":4,"selected":0}],
                "links":[],"speed":1048576,"seeders":12}"#,
        )
        .unwrap();
        assert_eq!(i.files.len(), 2);
        assert!(matches!(phase(&i.status), Phase::Working));
        let a: Account = serde_json::from_str(r#"{"id":1,"username":"anthony","email":"a@b.c","points":10,"type":"premium","premium":864000,"expiration":"2027-01-01T00:00:00.000Z"}"#).unwrap();
        assert_eq!(a.kind, "premium");
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["kind"], "premium");
        assert_eq!(explain(23, "x"), "Real-Debrid's traffic limit for this hoster is used up for today");
    }
}
