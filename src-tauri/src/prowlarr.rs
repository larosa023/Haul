//! Built-in Prowlarr: Haul finds Prowlarr on this PC, reads its address and API key from
//! Prowlarr's own config.xml, starts it hidden if it isn't running, and searches every site
//! added in Prowlarr with one call. Nothing to copy or paste, and no window to keep open.
//! Indexers Prowlarr has paused after failures are re-tested automatically.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::search::Hit;

#[derive(Clone, Debug, PartialEq)]
pub struct Conn {
    pub base: String,
    pub key: String,
}

/// Prowlarr's data folder (where config.xml lives).
fn data_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    [env("ProgramData").map(|p| p.join("Prowlarr")), env("LOCALAPPDATA").map(|p| p.join("Prowlarr")), env("APPDATA").map(|p| p.join("Prowlarr"))]
        .into_iter()
        .flatten()
        .find(|d| d.join("config.xml").is_file())
}

/// Where Prowlarr is installed, if Haul can find it. Prefers the console build (no tray icon),
/// which Haul runs without a window.
pub fn find() -> Option<PathBuf> {
    let set = crate::settings::with(|s| s.prowlarr_path.clone());
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = Vec::new();
    if !set.is_empty() {
        if let Some(d) = PathBuf::from(&set).parent() {
            dirs.push(d.to_path_buf());
        }
    }
    for k in ["ProgramData", "ProgramFiles", "LOCALAPPDATA"] {
        if let Some(b) = env(k) {
            dirs.push(b.join("Prowlarr").join("bin"));
            dirs.push(b.join("Prowlarr"));
        }
    }
    for d in dirs {
        for exe in ["Prowlarr.Console.exe", "Prowlarr.exe"] {
            let p = d.join(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Address and API key, read from Prowlarr's config.xml.
pub fn conn() -> Option<Conn> {
    let xml = std::fs::read_to_string(data_dir()?.join("config.xml")).ok()?;
    parse_config(&xml)
}

pub fn parse_config(xml: &str) -> Option<Conn> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let get = |tag: &str| doc.descendants().find(|n| n.tag_name().name().eq_ignore_ascii_case(tag)).and_then(|n| n.text()).map(|t| t.trim().to_string()).unwrap_or_default();
    let key = get("ApiKey");
    if key.is_empty() {
        return None;
    }
    let port = get("Port").parse::<u16>().unwrap_or(9696);
    let base_path = get("UrlBase");
    let base_path = base_path.trim_matches('/');
    let base = if base_path.is_empty() { format!("http://127.0.0.1:{port}") } else { format!("http://127.0.0.1:{port}/{base_path}") };
    Some(Conn { base, key })
}

async fn running(c: &Conn) -> bool {
    crate::http::client().get(format!("{}/ping", c.base)).timeout(Duration::from_secs(2)).send().await.map(|r| r.status().is_success()).unwrap_or(false)
}

/// The connection to use, starting Prowlarr hidden if needed (waits up to a minute).
/// None when it's turned off in Settings or not installed.
pub async fn ensure() -> Option<Conn> {
    if !crate::settings::with(|s| s.prowlarr_auto) {
        return None;
    }
    let c = conn()?;
    if running(&c).await {
        return Some(c);
    }
    let exe = find()?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("-nobrowser");
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No window, and not tied to Haul (it keeps running if Haul closes).
        cmd.creation_flags(0x0800_0000 | 0x0000_0008);
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.spawn().ok()?;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if running(&c).await {
            return Some(c);
        }
    }
    None
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Release {
    title: String,
    size: u64,
    seeders: Option<i64>,
    leechers: Option<i64>,
    indexer: String,
    magnet_url: Option<String>,
    download_url: Option<String>,
    info_url: Option<String>,
    info_hash: Option<String>,
    publish_date: Option<String>,
    categories: Vec<Category>,
    grabs: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Category {
    id: u64,
}

fn categories(cat: &str) -> &'static [u32] {
    match cat {
        "movies" => &[2000],
        "tv" => &[5000],
        "music" => &[3000],
        "software" => &[4000],
        "games" => &[1000, 4050],
        "books" => &[7000],
        _ => &[],
    }
}

pub fn parse_results(json: &str) -> Result<Vec<Hit>, String> {
    let list: Vec<Release> = serde_json::from_str(json).map_err(|_| "sent something Haul couldn't read".to_string())?;
    Ok(list
        .into_iter()
        .filter_map(|r| {
            let magnet = r.magnet_url.filter(|m| m.starts_with("magnet:"));
            let magnet = magnet.or_else(|| r.info_hash.as_deref().filter(|h| h.len() >= 32).map(|h| format!("magnet:?xt=urn:btih:{h}&dn={}", urlencoding(&r.title))));
            let link = magnet.or(r.download_url).filter(|l| !l.is_empty())?;
            let ids: Vec<u64> = r.categories.iter().map(|c| c.id).collect();
            let cat = if ids.contains(&4050) || ids.iter().any(|c| (1000..2000).contains(c)) {
                "Games".to_string()
            } else {
                ids.iter().filter(|c| **c < 100_000).max_by_key(|c| *c % 1000 != 0).copied().map(crate::search::cat_name).unwrap_or_default()
            };
            Some(Hit {
                title: r.title,
                size: r.size,
                seeders: r.seeders.unwrap_or(-1),
                leechers: r.leechers.unwrap_or(-1),
                source: r.indexer,
                link,
                info_url: r.info_url.unwrap_or_default(),
                published: r.publish_date.and_then(|d| chrono::DateTime::parse_from_rfc3339(&d).ok()).map(|d| d.timestamp()).unwrap_or(0),
                category: cat,
                downloads: r.grabs.unwrap_or(0),
            })
        })
        .collect())
}

fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

async fn search_once(c: &Conn, q: &str, cat: &str) -> Result<Vec<Hit>, String> {
    let mut query: Vec<(&str, String)> = vec![("query", q.to_string()), ("type", "search".into()), ("limit", "100".into())];
    for id in categories(cat) {
        query.push(("categories", id.to_string()));
    }
    let r = crate::http::client()
        .get(format!("{}/api/v1/search", c.base))
        .header("X-Api-Key", &c.key)
        .query(&query)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| if e.is_timeout() { "took too long to answer".to_string() } else { "couldn't be reached".to_string() })?;
    match r.status().as_u16() {
        200..=299 => {}
        401 | 403 => return Err("didn't accept its API key (restart Prowlarr, then search again)".into()),
        s => return Err(format!("answered with an error ({s})")),
    }
    parse_results(&r.text().await.map_err(|e| e.to_string())?)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct IndexerStatus {
    disabled_till: Option<String>,
}

/// True if Prowlarr has paused ("benched") any site after earlier failures.
async fn any_paused(c: &Conn) -> bool {
    let Ok(r) = crate::http::client().get(format!("{}/api/v1/indexerstatus", c.base)).header("X-Api-Key", &c.key).timeout(Duration::from_secs(10)).send().await else { return false };
    let list: Vec<IndexerStatus> = r.json().await.unwrap_or_default();
    list.iter().any(|s| s.disabled_till.is_some())
}

/// Searches every site in Prowlarr. If some are paused, asks Prowlarr to re-test them
/// (which un-pauses the ones that work now) and searches again.
pub async fn search(c: &Conn, q: &str, cat: &str) -> Result<Vec<Hit>, String> {
    if any_paused(c).await {
        let _ = crate::http::client().post(format!("{}/api/v1/indexer/testall", c.base)).header("X-Api-Key", &c.key).timeout(Duration::from_secs(90)).send().await;
    }
    search_once(c, q, cat).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct IndexerInfo {
    enable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Prowlarr is installed and its config was found.
    pub found: bool,
    pub running: bool,
    /// Sites turned on in Prowlarr.
    pub sites: usize,
    pub address: String,
}

pub async fn status() -> Status {
    let Some(c) = conn() else { return Status { found: false, running: false, sites: 0, address: String::new() } };
    let up = running(&c).await;
    let mut sites = 0;
    if up {
        if let Ok(r) = crate::http::client().get(format!("{}/api/v1/indexer", c.base)).header("X-Api-Key", &c.key).timeout(Duration::from_secs(10)).send().await {
            let list: Vec<IndexerInfo> = r.json().await.unwrap_or_default();
            sites = list.iter().filter(|i| i.enable).count();
        }
    }
    Status { found: true, running: up, sites, address: c.base.replace("127.0.0.1", "localhost") }
}

/// Starts it (if needed) and reports.
pub async fn start() -> Status {
    let _ = ensure().await;
    status().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_config() {
        let c = parse_config("<Config><Port>9797</Port><UrlBase>/prowlarr/</UrlBase><ApiKey>abc123</ApiKey></Config>").unwrap();
        assert_eq!(c.base, "http://127.0.0.1:9797/prowlarr");
        assert_eq!(c.key, "abc123");
        let c = parse_config("<Config><ApiKey>k</ApiKey><UrlBase></UrlBase></Config>").unwrap();
        assert_eq!(c.base, "http://127.0.0.1:9696");
        assert!(parse_config("<Config><Port>1</Port></Config>").is_none());
    }

    #[test]
    fn reads_results() {
        let json = r#"[
          {"title":"Ubuntu 24.04","size":6000000000,"seeders":120,"leechers":4,"indexer":"The Pirate Bay",
           "magnetUrl":"magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01","infoUrl":"https://x/1",
           "publishDate":"2026-04-25T10:00:00Z","categories":[{"id":4000,"name":"PC"},{"id":100001}]},
          {"title":"Game","size":10,"indexer":"BlueRoms","downloadUrl":"http://localhost:9696/2/download?link=x",
           "categories":[{"id":1000}]},
          {"title":"Hash only","size":1,"indexer":"X","infoHash":"0123456789abcdef0123456789abcdef01234567","categories":[]},
          {"title":"No link","size":1,"indexer":"X"}
        ]"#;
        let h = parse_results(json).unwrap();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].seeders, 120);
        assert_eq!(h[0].category, "Software");
        assert!(h[0].link.starts_with("magnet:"));
        assert!(h[0].published > 0);
        assert_eq!(h[1].category, "Games");
        assert_eq!(h[1].seeders, -1);
        assert!(h[2].link.starts_with("magnet:?xt=urn:btih:0123"));
    }
}
