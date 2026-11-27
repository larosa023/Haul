//! Debrid services: premium-speed downloads from file hosters, and torrents downloaded on
//! the service's servers and then fetched over plain HTTPS.
//!
//! Real-Debrid, AllDebrid, Premiumize and TorBox. One is active at a time; each keeps its own
//! API key. The rest of Haul talks to this module only, in Real-Debrid's terms (statuses,
//! file lists, links that `unrestrict` turns into direct links), and the other services are
//! translated into those terms here.

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Service {
    RealDebrid,
    AllDebrid,
    Premiumize,
    TorBox,
}

impl Service {
    pub fn from_id(id: &str) -> Service {
        match id {
            "alldebrid" => Service::AllDebrid,
            "premiumize" => Service::Premiumize,
            "torbox" => Service::TorBox,
            _ => Service::RealDebrid,
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Service::RealDebrid => "realdebrid",
            Service::AllDebrid => "alldebrid",
            Service::Premiumize => "premiumize",
            Service::TorBox => "torbox",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Service::RealDebrid => "Real-Debrid",
            Service::AllDebrid => "AllDebrid",
            Service::Premiumize => "Premiumize",
            Service::TorBox => "TorBox",
        }
    }
}

pub const ALL: [Service; 4] = [Service::RealDebrid, Service::AllDebrid, Service::Premiumize, Service::TorBox];

/// The service in use.
pub fn service() -> Service {
    Service::from_id(&settings::with(|s| s.debrid_service.clone()))
}

pub fn name() -> &'static str {
    service().name()
}

/// Does an error message come from a debrid service (rather than the file host)?
pub fn is_service_error(text: &str) -> bool {
    ALL.iter().any(|s| text.starts_with(s.name())) || text.starts_with("Add your")
}

pub fn key_for(svc: Service) -> String {
    settings::with(|s| s.debrid_keys.get(svc.id()).cloned()).map(|k| secret::open(&k)).unwrap_or_default()
}

pub fn key() -> String {
    key_for(service())
}

/// Connected and switched on.
pub fn active() -> bool {
    settings::with(|s| s.debrid_enabled && s.debrid_keys.get(&s.debrid_service).is_some_and(|k| !k.is_empty()))
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
    #[serde(skip_deserializing)]
    pub service: String,
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
    /// Why it failed, from services that say (others use the status).
    #[serde(skip)]
    pub message: String,
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

async fn rd_request(method: reqwest::Method, path: &str, form: Option<&[(&str, &str)]>, body: Option<Vec<u8>>, key: &str) -> Result<String, String> {
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
    let t = rd_request(reqwest::Method::GET, path, None, None, &key()).await?;
    serde_json::from_str(&t).map_err(|e| format!("Unexpected answer from Real-Debrid: {e}"))
}

/// Checks a key and returns the account it belongs to.
async fn rd_account_for(key: &str) -> Result<Account, String> {
    let t = rd_request(reqwest::Method::GET, "/user", None, None, key).await?;
    let mut a: Account = serde_json::from_str(&t).map_err(|e| format!("Unexpected answer from Real-Debrid: {e}"))?;
    a.service = "realdebrid".into();
    Ok(a)
}

async fn rd_account() -> Result<Account, String> {
    rd_account_for(&key()).await
}

/// Turns a hoster link into a direct download link.
async fn rd_unrestrict(link: &str, password: &str) -> Result<Unrestricted, String> {
    let mut form = vec![("link", link)];
    if !password.is_empty() {
        form.push(("password", password));
    }
    let t = rd_request(reqwest::Method::POST, "/unrestrict/link", Some(&form), None, &key()).await?;
    let u: Unrestricted = serde_json::from_str(&t).map_err(|e| format!("Unexpected answer from Real-Debrid: {e}"))?;
    if u.download.is_empty() {
        return Err("Real-Debrid didn't return a download link".into());
    }
    Ok(u)
}

/// Domains Real-Debrid can unrestrict (cached for an hour).
async fn rd_hosts() -> HashSet<String> {
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

/// Should this link go through the debrid service?
pub async fn should_unrestrict(url: &str) -> bool {
    if !active() || !settings::with(|s| s.debrid_hosters) {
        return false;
    }
    host_supported(url, &hosts().await)
}

async fn rd_add_magnet(magnet: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Added {
        id: String,
    }
    let t = rd_request(reqwest::Method::POST, "/torrents/addMagnet", Some(&[("magnet", magnet)]), None, &key()).await?;
    serde_json::from_str::<Added>(&t).map(|a| a.id).map_err(|e| format!("Unexpected answer from Real-Debrid: {e}"))
}

async fn rd_add_torrent_file(bytes: Vec<u8>) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Added {
        id: String,
    }
    let t = rd_request(reqwest::Method::PUT, "/torrents/addTorrent", None, Some(bytes), &key()).await?;
    serde_json::from_str::<Added>(&t).map(|a| a.id).map_err(|e| format!("Unexpected answer from Real-Debrid: {e}"))
}

async fn rd_info(id: &str) -> Result<TorrentInfo, String> {
    get(&format!("/torrents/info/{id}")).await
}

/// `files`: Real-Debrid file ids, or empty for all.
async fn rd_select_files(id: &str, files: &[u64]) -> Result<(), String> {
    let list = if files.is_empty() { "all".to_string() } else { files.iter().map(u64::to_string).collect::<Vec<_>>().join(",") };
    rd_request(reqwest::Method::POST, &format!("/torrents/selectFiles/{id}"), Some(&[("files", &list)]), None, &key()).await.map(|_| ())
}

async fn rd_delete(id: &str) {
    let _ = rd_request(reqwest::Method::DELETE, &format!("/torrents/delete/{id}"), None, None, &key()).await;
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

pub fn phase_of(i: &TorrentInfo) -> Phase {
    if i.status == "error" && !i.message.is_empty() {
        return Phase::Failed(i.message.clone());
    }
    phase(&i.status)
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
        "error" => Phase::Failed(format!("{} couldn't download this torrent", name())),
        other => Phase::Failed(format!("Real-Debrid status: {other}")),
    }
}

/// Short text for the list while Real-Debrid works on a torrent.
pub fn status_note(i: &TorrentInfo) -> String {
    let n = name();
    match i.status.as_str() {
        "queued" => format!("{n}: queued"),
        "compressing" => format!("{n}: packing"),
        "uploading" => format!("{n}: almost ready"),
        "magnet_conversion" => format!("{n}: reading the magnet"),
        _ if i.seeders > 0 => format!("{n}: downloading · {} seeders", i.seeders),
        _ => format!("{n}: downloading"),
    }
}


// ================================================================ the facade

fn bad_answer(svc: Service, e: impl std::fmt::Display) -> String {
    format!("Unexpected answer from {}: {e}", svc.name())
}

/// Checks a key and returns the account it belongs to.
pub async fn account_for(svc: Service, key: &str) -> Result<Account, String> {
    let mut a = match svc {
        Service::RealDebrid => rd_account_for(key).await?,
        Service::AllDebrid => ad::account(key).await?,
        Service::Premiumize => pm::account(key).await?,
        Service::TorBox => tb::account(key).await?,
    };
    a.service = svc.id().into();
    Ok(a)
}

pub async fn account() -> Result<Account, String> {
    match service() {
        Service::RealDebrid => rd_account().await,
        svc => account_for(svc, &key()).await,
    }
}

/// Turns a hoster link (or a link from a finished torrent) into a direct download link.
pub async fn unrestrict(link: &str, password: &str) -> Result<Unrestricted, String> {
    if let Some(direct) = link.strip_prefix("haul-direct:") {
        let filename = url::Url::parse(direct)
            .ok()
            .and_then(|u| u.path_segments().and_then(|mut p| p.next_back().map(|s| s.to_string())))
            .map(|s| percent_encoding::percent_decode_str(&s).decode_utf8_lossy().to_string())
            .unwrap_or_default();
        return Ok(Unrestricted { filename, filesize: 0, download: direct.to_string(), host: String::new() });
    }
    if let Some(rest) = link.strip_prefix("haul-torbox:") {
        return tb::request_dl(rest).await;
    }
    match service() {
        Service::RealDebrid => rd_unrestrict(link, password).await,
        Service::AllDebrid => ad::unlock(link, password).await,
        Service::Premiumize => pm::direct(link).await,
        Service::TorBox => tb::web(link).await,
    }
}

/// Domains the service can unrestrict (cached for an hour per service).
pub async fn hosts() -> HashSet<String> {
    match service() {
        Service::RealDebrid => rd_hosts().await,
        svc => {
            static CACHE: Mutex<Option<(Service, Instant, HashSet<String>)>> = Mutex::new(None);
            if let Some((s, at, h)) = CACHE.lock().as_ref() {
                if *s == svc && at.elapsed() < Duration::from_secs(3600) {
                    return h.clone();
                }
            }
            let list = match svc {
                Service::AllDebrid => ad::hosts().await,
                Service::Premiumize => pm::hosts().await,
                _ => tb::hosts().await,
            };
            let set: HashSet<String> = list.into_iter().map(|d| d.trim().trim_start_matches("www.").to_ascii_lowercase()).filter(|d| d.contains('.')).collect();
            if !set.is_empty() {
                *CACHE.lock() = Some((svc, Instant::now(), set.clone()));
            }
            set
        }
    }
}

pub async fn add_magnet(magnet: &str) -> Result<String, String> {
    match service() {
        Service::RealDebrid => rd_add_magnet(magnet).await,
        Service::AllDebrid => ad::add(magnet.as_bytes().to_vec(), true).await,
        Service::Premiumize => pm::add(magnet.as_bytes().to_vec(), true).await,
        Service::TorBox => tb::add(magnet.as_bytes().to_vec(), true).await,
    }
}

pub async fn add_torrent_file(bytes: Vec<u8>) -> Result<String, String> {
    match service() {
        Service::RealDebrid => rd_add_torrent_file(bytes).await,
        Service::AllDebrid => ad::add(bytes, false).await,
        Service::Premiumize => pm::add(bytes, false).await,
        Service::TorBox => tb::add(bytes, false).await,
    }
}

/// Where a torrent is up to. Other services are described in Real-Debrid's terms: their
/// files are all "selected" and their `links` line up with `files`.
pub async fn info(id: &str) -> Result<TorrentInfo, String> {
    match service() {
        Service::RealDebrid => rd_info(id).await,
        Service::AllDebrid => ad::info(id).await,
        Service::Premiumize => pm::info(id).await,
        Service::TorBox => tb::info(id).await,
    }
}

/// Only Real-Debrid lets you choose files up front; the others fetch the whole torrent.
pub fn picks_files() -> bool {
    service() == Service::RealDebrid
}

/// `files`: file ids, or empty for all.
pub async fn select_files(id: &str, files: &[u64]) -> Result<(), String> {
    match service() {
        Service::RealDebrid => rd_select_files(id, files).await,
        _ => Ok(()),
    }
}

pub async fn delete(id: &str) {
    match service() {
        Service::RealDebrid => rd_delete(id).await,
        Service::AllDebrid => ad::delete(id).await,
        Service::Premiumize => pm::delete(id).await,
        Service::TorBox => tb::delete(id).await,
    }
}

/// One call to a non-Real-Debrid API, as JSON, with a retry when rate limited.
async fn call(svc: Service, build: impl Fn() -> reqwest::RequestBuilder) -> Result<serde_json::Value, String> {
    let mut tries = 0;
    loop {
        let r = build().timeout(Duration::from_secs(30)).send().await.map_err(|e| format!("Couldn't reach {}: {e}", svc.name()))?;
        let status = r.status().as_u16();
        if status == 429 && tries < 3 {
            tries += 1;
            tokio::time::sleep(Duration::from_secs(2 * tries)).await;
            continue;
        }
        if status == 401 || status == 403 {
            return Err(format!("{} didn't accept the API key (check it in Settings → Debrid)", svc.name()));
        }
        let text = r.text().await.unwrap_or_default();
        return serde_json::from_str(&text).map_err(|_| if status >= 500 { format!("{} is having problems ({status}); trying again shortly", svc.name()) } else { bad_answer(svc, format!("status {status}")) });
    }
}

fn s_of(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn u_of(v: &serde_json::Value) -> u64 {
    v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)).or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

fn days_left(until_unix: i64) -> (i64, String) {
    let left = until_unix - chrono::Utc::now().timestamp();
    let iso = chrono::DateTime::from_timestamp(until_unix, 0).map(|d| d.to_rfc3339()).unwrap_or_default();
    (left.max(0), iso)
}

/// AllDebrid (https://alldebrid.com), API v4 / v4.1.
mod ad {
    use super::*;
    const BASE: &str = "https://api.alldebrid.com";
    const SVC: Service = Service::AllDebrid;

    async fn api(path: &str, key: &str, form: Vec<(String, String)>) -> Result<serde_json::Value, String> {
        if key.is_empty() {
            return Err("Add your AllDebrid API key in Settings → Debrid".into());
        }
        let url = format!("{BASE}{path}?agent=Haul");
        let v = call(SVC, || crate::http::client().post(&url).bearer_auth(key).form(&form)).await?;
        if v["status"] == "success" {
            return Ok(v["data"].clone());
        }
        let code = s_of(&v["error"]["code"]);
        let msg = s_of(&v["error"]["message"]);
        Err(match code.as_str() {
            "AUTH_BAD_APIKEY" | "AUTH_MISSING_APIKEY" | "AUTH_BLOCKED" => "AllDebrid didn't accept the API key (check it in Settings → Debrid)".into(),
            "MUST_BE_PREMIUM" => "AllDebrid premium is needed for this".into(),
            "LINK_HOST_NOT_SUPPORTED" => "AllDebrid doesn't support this hoster".into(),
            "LINK_DOWN" | "LINK_IS_MISSING" => "The file isn't available on the hoster any more".into(),
            "LINK_HOST_LIMIT_REACHED" => "AllDebrid's limit for this hoster is used up for today".into(),
            _ if !msg.is_empty() => format!("AllDebrid: {msg}"),
            _ => format!("AllDebrid error {code}"),
        })
    }

    pub async fn account(key: &str) -> Result<Account, String> {
        let d = api("/v4/user", key, vec![]).await?;
        let u = &d["user"];
        let (premium, expiration) = days_left(u["premiumUntil"].as_i64().unwrap_or(0));
        Ok(Account {
            username: s_of(&u["username"]),
            email: s_of(&u["email"]),
            kind: if u["isPremium"].as_bool().unwrap_or(false) { "premium".into() } else { "free".into() },
            premium,
            expiration,
            points: u["fidelityPoints"].as_i64().unwrap_or(0),
            service: String::new(),
        })
    }

    pub async fn unlock(link: &str, password: &str) -> Result<Unrestricted, String> {
        let mut form = vec![("link".to_string(), link.to_string())];
        if !password.is_empty() {
            form.push(("password".into(), password.into()));
        }
        let d = api("/v4/link/unlock", &key(), form).await?;
        let mut direct = s_of(&d["link"]);
        // Some hosters make the link over time: ask every 5 seconds (up to 5 minutes).
        if direct.is_empty() && d["delayed"].as_u64().is_some() {
            let id = s_of(&d["delayed"]);
            for _ in 0..60 {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let w = api("/v4/link/delayed", &key(), vec![("id".into(), id.clone())]).await?;
                match w["status"].as_u64() {
                    Some(2) => {
                        direct = s_of(&w["link"]);
                        break;
                    }
                    Some(3) => return Err("AllDebrid couldn't generate the link".into()),
                    _ => {}
                }
            }
        }
        if direct.is_empty() {
            return Err("AllDebrid didn't return a download link".into());
        }
        Ok(Unrestricted { filename: s_of(&d["filename"]), filesize: u_of(&d["filesize"]), download: direct, host: s_of(&d["host"]) })
    }

    pub async fn hosts() -> Vec<String> {
        let url = format!("{BASE}/v4/hosts/domains?agent=Haul");
        let v = call(SVC, || crate::http::client().get(&url).bearer_auth(key())).await.unwrap_or_default();
        v["data"]["hosts"].as_array().map(|a| a.iter().map(s_of).collect()).unwrap_or_default()
    }

    pub async fn add(src: Vec<u8>, magnet: bool) -> Result<String, String> {
        let key = key();
        if key.is_empty() {
            return Err("Add your AllDebrid API key in Settings → Debrid".into());
        }
        let d = if magnet {
            api("/v4/magnet/upload", &key, vec![("magnets[]".into(), String::from_utf8_lossy(&src).to_string())]).await?
        } else {
            let url = format!("{BASE}/v4/magnet/upload/file?agent=Haul");
            let v = call(SVC, || {
                let part = reqwest::multipart::Part::bytes(src.clone()).file_name("file.torrent").mime_str("application/x-bittorrent").unwrap();
                crate::http::client().post(&url).bearer_auth(&key).multipart(reqwest::multipart::Form::new().part("files[]", part))
            })
            .await?;
            if v["status"] != "success" {
                return Err(format!("AllDebrid: {}", s_of(&v["error"]["message"])));
            }
            v["data"].clone()
        };
        let m = d["magnets"].get(0).or_else(|| d["files"].get(0)).cloned().unwrap_or_default();
        if !m["error"].is_null() {
            return Err(format!("AllDebrid: {}", s_of(&m["error"]["message"])));
        }
        let id = s_of(&m["id"]);
        if id.is_empty() {
            return Err("AllDebrid didn't accept the torrent".into());
        }
        Ok(id)
    }

    /// Flattens AllDebrid's file tree ({n, e} folders, {n, s, l} files) into paths.
    pub fn flatten(nodes: &serde_json::Value, prefix: &str, out: &mut Vec<(String, u64, String)>) {
        for n in nodes.as_array().into_iter().flatten() {
            let name = s_of(&n["n"]);
            let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if n["e"].is_array() {
                flatten(&n["e"], &path, out);
            } else {
                out.push((path, u_of(&n["s"]), s_of(&n["l"])));
            }
        }
    }

    pub async fn info(id: &str) -> Result<TorrentInfo, String> {
        let key = key();
        let d = api("/v4.1/magnet/status", &key, vec![("id".into(), id.into())]).await?;
        let m = if d["magnets"].is_array() { d["magnets"][0].clone() } else { d["magnets"].clone() };
        let code = m["statusCode"].as_u64().unwrap_or(0);
        let size = u_of(&m["size"]);
        let mut info = TorrentInfo {
            id: id.into(),
            filename: s_of(&m["filename"]),
            bytes: size,
            progress: if size > 0 { u_of(&m["downloaded"]) as f64 * 100.0 / size as f64 } else { 0.0 },
            speed: u_of(&m["downloadSpeed"]),
            seeders: u_of(&m["seeders"]) as u32,
            status: match code {
                0 => "queued",
                1 => "downloading",
                2 => "compressing",
                3 => "uploading",
                4 => "downloaded",
                _ => "error",
            }
            .into(),
            message: if code >= 5 { format!("AllDebrid: {}", s_of(&m["status"])) } else { String::new() },
            ..Default::default()
        };
        if code == 4 {
            info.progress = 100.0;
            let f = api("/v4/magnet/files", &key, vec![("id[]".into(), id.into())]).await?;
            let mut files = Vec::new();
            flatten(&f["magnets"][0]["files"], "", &mut files);
            for (n, (path, bytes, link)) in files.into_iter().enumerate() {
                info.files.push(RdFile { id: n as u64 + 1, path: format!("/{path}"), bytes, selected: 1 });
                info.links.push(link);
            }
        }
        Ok(info)
    }

    pub async fn delete(id: &str) {
        let _ = api("/v4/magnet/delete", &key(), vec![("id".into(), id.into())]).await;
    }
}

/// Premiumize (https://www.premiumize.me).
mod pm {
    use super::*;
    const BASE: &str = "https://www.premiumize.me/api";
    const SVC: Service = Service::Premiumize;

    fn check(v: serde_json::Value) -> Result<serde_json::Value, String> {
        if v["status"] == "success" {
            return Ok(v);
        }
        let msg = s_of(&v["message"]);
        Err(if msg.to_ascii_lowercase().contains("apikey") || msg.to_ascii_lowercase().contains("not logged") {
            "Premiumize didn't accept the API key (check it in Settings → Debrid)".into()
        } else if msg.is_empty() {
            "Premiumize returned an error".into()
        } else {
            format!("Premiumize: {msg}")
        })
    }

    async fn get(path: &str, key: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, String> {
        if key.is_empty() {
            return Err("Add your Premiumize API key in Settings → Debrid".into());
        }
        let url = format!("{BASE}{path}");
        check(call(SVC, || crate::http::client().get(&url).query(&[("apikey", key)]).query(query)).await?)
    }

    async fn post(path: &str, key: &str, form: &[(&str, &str)]) -> Result<serde_json::Value, String> {
        if key.is_empty() {
            return Err("Add your Premiumize API key in Settings → Debrid".into());
        }
        let url = format!("{BASE}{path}");
        check(call(SVC, || crate::http::client().post(&url).query(&[("apikey", key)]).form(form)).await?)
    }

    pub async fn account(key: &str) -> Result<Account, String> {
        let v = get("/account/info", key, &[]).await?;
        let until = v["premium_until"].as_i64().unwrap_or(0);
        let (premium, expiration) = days_left(until);
        Ok(Account { username: s_of(&v["customer_id"]), kind: if premium > 0 { "premium".into() } else { "free".into() }, premium, expiration, ..Default::default() })
    }

    /// Direct links for a hoster link, or for a magnet that's cached / finished on Premiumize.
    async fn directdl(src: &str) -> Result<Vec<(String, u64, String)>, String> {
        let v = post("/transfer/directdl", &key(), &[("src", src)]).await?;
        Ok(v["content"].as_array().into_iter().flatten().map(|c| (s_of(&c["path"]), u_of(&c["size"]), s_of(&c["link"]))).filter(|c| !c.2.is_empty()).collect())
    }

    pub async fn direct(link: &str) -> Result<Unrestricted, String> {
        let files = directdl(link).await?;
        let (path, size, url) = files.into_iter().next().ok_or("Premiumize didn't return a download link")?;
        Ok(Unrestricted { filename: path.rsplit('/').next().unwrap_or(&path).to_string(), filesize: size, download: url, host: String::new() })
    }

    pub async fn hosts() -> Vec<String> {
        get("/services/list", &key(), &[]).await.map(|v| v["directdl"].as_array().map(|a| a.iter().map(s_of).collect()).unwrap_or_default()).unwrap_or_default()
    }

    /// Ids are "transfer id|info hash" (the hash fetches the files once it's done).
    pub async fn add(src: Vec<u8>, magnet: bool) -> Result<String, String> {
        let key = key();
        let hash = crate::torrent::quick_hash(&src).unwrap_or_default();
        let v = if magnet {
            post("/transfer/create", &key, &[("src", &String::from_utf8_lossy(&src))]).await?
        } else {
            if key.is_empty() {
                return Err("Add your Premiumize API key in Settings → Debrid".into());
            }
            let url = format!("{BASE}/transfer/create");
            check(
                call(SVC, || {
                    let part = reqwest::multipart::Part::bytes(src.clone()).file_name("file.torrent").mime_str("application/x-bittorrent").unwrap();
                    crate::http::client().post(&url).query(&[("apikey", &key)]).multipart(reqwest::multipart::Form::new().part("file", part))
                })
                .await?,
            )?
        };
        Ok(format!("{}|{hash}", s_of(&v["id"])))
    }

    pub async fn info(id: &str) -> Result<TorrentInfo, String> {
        let (tid, hash) = id.split_once('|').unwrap_or((id, ""));
        let v = get("/transfer/list", &key(), &[]).await?;
        let t = v["transfers"].as_array().into_iter().flatten().find(|t| s_of(&t["id"]) == tid).cloned();
        let Some(t) = t else {
            return Ok(TorrentInfo { id: id.into(), status: "error".into(), message: "Premiumize removed this transfer".into(), ..Default::default() });
        };
        let st = s_of(&t["status"]);
        let progress = t["progress"].as_f64().unwrap_or(0.0) * 100.0;
        let mut info = TorrentInfo {
            id: id.into(),
            filename: s_of(&t["name"]),
            hash: hash.into(),
            progress,
            status: match st.as_str() {
                "waiting" | "queued" => "queued",
                "finished" | "seeding" => "downloaded",
                "error" | "timeout" | "deleted" | "banned" => "error",
                _ => "downloading",
            }
            .into(),
            message: if matches!(st.as_str(), "error" | "timeout" | "deleted" | "banned") { format!("Premiumize: {}", if s_of(&t["message"]).is_empty() { st.clone() } else { s_of(&t["message"]) }) } else { String::new() },
            ..Default::default()
        };
        if info.status == "downloaded" {
            info.progress = 100.0;
            let src = if hash.is_empty() { s_of(&t["src"]) } else { format!("magnet:?xt=urn:btih:{hash}") };
            for (n, (path, bytes, link)) in directdl(&src).await?.into_iter().enumerate() {
                info.bytes += bytes;
                info.files.push(RdFile { id: n as u64 + 1, path: format!("/{path}"), bytes, selected: 1 });
                info.links.push(format!("haul-direct:{link}"));
            }
            if info.files.is_empty() {
                return Err("Premiumize finished but returned no files".into());
            }
        }
        Ok(info)
    }

    pub async fn delete(id: &str) {
        let tid = id.split('|').next().unwrap_or(id);
        let _ = post("/transfer/delete", &key(), &[("id", tid)]).await;
    }
}

/// TorBox (https://torbox.app).
mod tb {
    use super::*;
    const BASE: &str = "https://api.torbox.app/v1/api";
    const SVC: Service = Service::TorBox;

    fn check(v: serde_json::Value) -> Result<serde_json::Value, String> {
        if v["success"].as_bool().unwrap_or(false) {
            return Ok(v["data"].clone());
        }
        let code = s_of(&v["error"]);
        let detail = s_of(&v["detail"]);
        Err(match code.as_str() {
            "AUTH_ERROR" | "NO_AUTH" | "BAD_TOKEN" => "TorBox didn't accept the API key (check it in Settings → Debrid)".into(),
            "PLAN_RESTRICTED_FEATURE" => "Your TorBox plan doesn't include this".into(),
            "ACTIVE_LIMIT" | "DOWNLOAD_LIMIT" | "MONTHLY_LIMIT" | "COOLDOWN_LIMIT" => format!("TorBox: {}", if detail.is_empty() { "limit reached".into() } else { detail }),
            _ if !detail.is_empty() => format!("TorBox: {detail}"),
            _ => format!("TorBox error {code}"),
        })
    }

    async fn get(path: &str, key: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, String> {
        if key.is_empty() {
            return Err("Add your TorBox API key in Settings → Debrid".into());
        }
        let url = format!("{BASE}{path}");
        check(call(SVC, || crate::http::client().get(&url).bearer_auth(key).query(query)).await?)
    }

    pub async fn account(key: &str) -> Result<Account, String> {
        let d = get("/user/me", key, &[]).await?;
        let until = chrono::DateTime::parse_from_rfc3339(&s_of(&d["premium_expires_at"])).map(|d| d.timestamp()).unwrap_or(0);
        let (premium, expiration) = days_left(until);
        let plan = d["plan"].as_i64().unwrap_or(0);
        Ok(Account { username: s_of(&d["email"]), email: s_of(&d["email"]), kind: if plan > 0 || premium > 0 { "premium".into() } else { "free".into() }, premium, expiration, ..Default::default() })
    }

    async fn create(path: &str, form: reqwest::multipart::Form) -> Result<serde_json::Value, String> {
        let key = key();
        if key.is_empty() {
            return Err("Add your TorBox API key in Settings → Debrid".into());
        }
        let url = format!("{BASE}{path}");
        // Multipart forms can't be cloned, so build it once (no automatic retry here).
        let r = crate::http::client().post(&url).bearer_auth(&key).multipart(form).timeout(Duration::from_secs(60)).send().await.map_err(|e| format!("Couldn't reach TorBox: {e}"))?;
        if r.status().as_u16() == 401 || r.status().as_u16() == 403 {
            return Err("TorBox didn't accept the API key (check it in Settings → Debrid)".into());
        }
        check(r.json().await.map_err(|e| bad_answer(SVC, e))?)
    }

    pub async fn add(src: Vec<u8>, magnet: bool) -> Result<String, String> {
        let form = if magnet {
            reqwest::multipart::Form::new().text("magnet", String::from_utf8_lossy(&src).to_string())
        } else {
            let part = reqwest::multipart::Part::bytes(src).file_name("file.torrent").mime_str("application/x-bittorrent").unwrap();
            reqwest::multipart::Form::new().part("file", part)
        };
        let d = create("/torrents/createtorrent", form).await?;
        let id = s_of(&d["torrent_id"]);
        if id.is_empty() {
            return Err("TorBox didn't accept the torrent".into());
        }
        Ok(format!("t:{id}"))
    }

    fn files_of(d: &serde_json::Value) -> Vec<(u64, String, u64)> {
        d["files"].as_array().into_iter().flatten().map(|f| (u_of(&f["id"]), s_of(&f["name"]), u_of(&f["size"]))).collect()
    }

    /// Ids are "t:<torrent id>" or "w:<web download id>".
    pub async fn info(id: &str) -> Result<TorrentInfo, String> {
        let (kind, num) = id.split_once(':').unwrap_or(("t", id));
        let path = if kind == "w" { "/webdl/mylist" } else { "/torrents/mylist" };
        let d = get(path, &key(), &[("id", num), ("bypass_cache", "true")]).await?;
        let d = if d.is_array() { d[0].clone() } else { d };
        let state = s_of(&d["download_state"]).to_ascii_lowercase();
        let finished = d["download_finished"].as_bool().unwrap_or(false) && d["download_present"].as_bool().unwrap_or(true);
        let failed = state.contains("error") || state.contains("failed") || state == "stalled (no seeds)";
        let mut info = TorrentInfo {
            id: id.into(),
            filename: s_of(&d["name"]),
            hash: s_of(&d["hash"]),
            bytes: u_of(&d["size"]),
            progress: d["progress"].as_f64().unwrap_or(0.0) * 100.0,
            speed: u_of(&d["download_speed"]),
            seeders: u_of(&d["seeds"]) as u32,
            status: if finished {
                "downloaded"
            } else if failed {
                "error"
            } else if state.contains("meta") {
                "magnet_conversion"
            } else if state.contains("queue") {
                "queued"
            } else {
                "downloading"
            }
            .into(),
            message: if failed { format!("TorBox: {state}") } else { String::new() },
            ..Default::default()
        };
        if finished {
            info.progress = 100.0;
            for (fid, name, size) in files_of(&d) {
                info.files.push(RdFile { id: fid, path: format!("/{name}"), bytes: size, selected: 1 });
                info.links.push(format!("haul-torbox:{kind}:{num}:{fid}"));
            }
        }
        Ok(info)
    }

    /// A fresh direct link for a file ("t:<id>:<file>" or "w:<id>:<file>").
    pub async fn request_dl(spec: &str) -> Result<Unrestricted, String> {
        let mut it = spec.splitn(3, ':');
        let (kind, id, file) = (it.next().unwrap_or("t"), it.next().unwrap_or(""), it.next().unwrap_or("0"));
        let key = key();
        let (path, idk) = if kind == "w" { ("/webdl/requestdl", "web_id") } else { ("/torrents/requestdl", "torrent_id") };
        let d = get(path, &key, &[("token", &key), (idk, id), ("file_id", file)]).await?;
        let url = s_of(&d);
        if url.is_empty() {
            return Err("TorBox didn't return a download link".into());
        }
        Ok(Unrestricted { download: url, ..Default::default() })
    }

    /// A hoster link: TorBox fetches it to its servers first, then it's a normal download.
    pub async fn web(link: &str) -> Result<Unrestricted, String> {
        let d = create("/webdl/createwebdownload", reqwest::multipart::Form::new().text("link", link.to_string())).await?;
        let id = s_of(&d["webdownload_id"]);
        if id.is_empty() {
            return Err("TorBox didn't accept the link".into());
        }
        let spec = format!("w:{id}");
        for _ in 0..180 {
            let i = info(&spec).await?;
            match phase_of(&i) {
                Phase::Ready => {
                    let f = i.files.first().ok_or("TorBox finished but returned no files")?;
                    let mut u = request_dl(&format!("w:{id}:{}", f.id)).await?;
                    u.filename = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
                    u.filesize = f.bytes;
                    return Ok(u);
                }
                Phase::Failed(e) => return Err(e),
                _ => tokio::time::sleep(Duration::from_secs(5)).await,
            }
        }
        Err("TorBox took too long to fetch the file; try again later".into())
    }

    pub async fn hosts() -> Vec<String> {
        let d = get("/webdl/hosters", &key(), &[]).await.unwrap_or_default();
        let mut out = Vec::new();
        for h in d.as_array().into_iter().flatten() {
            if h["status"].as_bool() == Some(false) {
                continue;
            }
            match &h["domains"] {
                serde_json::Value::Array(a) => out.extend(a.iter().map(s_of)),
                v => out.push(s_of(v)),
            }
            out.push(s_of(&h["domain"]));
        }
        out
    }

    pub async fn delete(id: &str) {
        let (kind, num) = id.split_once(':').unwrap_or(("t", id));
        let key = key();
        let (path, idk) = if kind == "w" { ("/webdl/controlwebdownload", "webdl_id") } else { ("/torrents/controltorrent", "torrent_id") };
        let url = format!("{BASE}{path}");
        let body = serde_json::json!({ idk: num.parse::<u64>().unwrap_or(0), "operation": "delete" });
        let _ = call(SVC, || crate::http::client().post(&url).bearer_auth(&key).json(&body)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real network, on CI (HAUL_TEST_NET=1): each service's account endpoint answers a wrong
    /// key with a plain "didn't accept the API key", which shows the URLs and parsing are right.
    #[test]
    fn real_debrid_services_reject_bad_key() {
        if std::env::var("HAUL_TEST_NET").is_err() {
            return;
        }
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async {
            for svc in ALL {
                let e = account_for(svc, "haul-ci-not-a-real-key").await.unwrap_err();
                println!("{}: {e}", svc.name());
                assert!(e.contains("API key") || e.contains("didn't accept"), "{}: {e}", svc.name());
            }
        });
    }

    #[test]
    fn other_services_parse() {
        let tree: serde_json::Value = serde_json::from_str(r#"[{"n":"Show","e":[{"n":"e1.mkv","s":600,"l":"https://alldebrid.com/f/a"},{"n":"Subs","e":[{"n":"e1.srt","s":4,"l":"https://alldebrid.com/f/b"}]}]}]"#).unwrap();
        let mut out = Vec::new();
        ad::flatten(&tree, "", &mut out);
        assert_eq!(out, vec![("Show/e1.mkv".into(), 600, "https://alldebrid.com/f/a".into()), ("Show/Subs/e1.srt".into(), 4, "https://alldebrid.com/f/b".into())]);
        assert_eq!(Service::from_id("torbox").name(), "TorBox");
        assert!(is_service_error("AllDebrid: LINK_DOWN"));
        assert!(!is_service_error("The server refused access"));
    }

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
