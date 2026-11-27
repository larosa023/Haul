//! Direct (HTTP/HTTPS) downloads.
//!
//! Large files are split into parallel ranges. When a connection finishes its range it takes
//! over half of the biggest range still left (like IDM), so all connections stay busy until the
//! end. Progress is saved per range, so downloads resume after a pause, a crash or a restart.
//! The file is written in place (sparse on NTFS, so no zero-filling) as `name.ext.haul` and
//! renamed when complete.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures_util::StreamExt;
use parking_lot::Mutex;
use reqwest::header::{self, HeaderMap, HeaderValue};

use crate::limiter::Limiter;
use crate::model::{self, Segment};
use crate::{paths, settings, sorter};

/// Smallest range worth giving its own connection.
const MIN_SPLIT: u64 = 2 * 1024 * 1024;
const CHUNK_TIMEOUT: Duration = Duration::from_secs(30);

pub fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(default_user_agent())
            .connect_timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(15))
            .build()
            .expect("http client")
    })
}

pub fn default_user_agent() -> String {
    format!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Haul/{}", env!("CARGO_PKG_VERSION"))
}

/// Shared between a running download and the engine.
pub struct Control {
    pub stop: AtomicBool,
    pub downloaded: AtomicU64,
    pub segments: Mutex<Vec<Segment>>,
    pub limiter: Limiter,
    pub connections: AtomicU64,
}

impl Control {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            stop: AtomicBool::new(false),
            downloaded: AtomicU64::new(0),
            segments: Mutex::new(Vec::new()),
            limiter: Limiter::new(0),
            connections: AtomicU64::new(0),
        })
    }
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// The global limiter shared by every direct download.
pub fn global_limiter() -> &'static Limiter {
    static L: OnceLock<Limiter> = OnceLock::new();
    L.get_or_init(|| Limiter::new(0))
}

#[derive(Clone, Default)]
pub struct Request {
    pub url: String,
    pub referrer: String,
    pub cookies: String,
    pub user_agent: String,
    /// Bearer token, sent only to huggingface.co (gated Hugging Face models).
    pub bearer: String,
    /// ETag or Last-Modified of the file we started: a range request then only returns part of
    /// that exact file, and the server sends the whole (new) file if it changed.
    pub if_range: String,
}

/// The bytes of one part of a download (HTTP response body or FTP transfer).
pub type ByteStream = std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<bytes::Bytes, Fail>> + Send>>;

fn headers(req: &Request) -> HeaderMap {
    let mut h = HeaderMap::new();
    if !req.referrer.is_empty() {
        if let Ok(v) = HeaderValue::from_str(&req.referrer) {
            h.insert(header::REFERER, v);
        }
    }
    if !req.cookies.is_empty() {
        if let Ok(v) = HeaderValue::from_str(&req.cookies) {
            h.insert(header::COOKIE, v);
        }
    }
    let ua = if !req.user_agent.is_empty() {
        req.user_agent.clone()
    } else {
        settings::with(|s| s.user_agent.clone())
    };
    if !ua.is_empty() {
        if let Ok(v) = HeaderValue::from_str(&ua) {
            h.insert(header::USER_AGENT, v);
        }
    }
    h.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    if !req.bearer.is_empty() {
        let host = url::Url::parse(&req.url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        if host == "huggingface.co" || host.ends_with(".huggingface.co") {
            if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", req.bearer)) {
                h.insert(header::AUTHORIZATION, v);
            }
        }
    }
    h
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeInfo {
    pub name: String,
    pub size: u64,
    pub resumable: bool,
    pub final_url: String,
    pub etag: String,
    pub last_modified: String,
    pub category: String,
    pub folder: String,
    /// The file comes through Real-Debrid.
    pub debrid: bool,
    /// The link is a web page (HTML), not a file: offer to find videos or grab its links.
    pub page: bool,
    /// The link is a stream playlist (HLS .m3u8 / DASH .mpd).
    pub stream: bool,
    /// What the server says the file is.
    pub content_type: String,
    /// Safety check of the link (filled in by the Add dialog's lookup).
    pub safety: Option<crate::safety::Report>,
}

struct Probe {
    info: ProbeInfo,
    /// The data, positioned at `from`, when the server honoured the range.
    response: Option<ByteStream>,
}

#[derive(Debug)]
pub enum Fail {
    /// Worth retrying (network trouble, 5xx, 429).
    Temporary(String),
    /// Won't fix itself (404, 403, disk full…).
    Permanent(String),
    /// The file on the server isn't the one we started (new ETag / date / size).
    Changed,
}

impl Fail {
    pub fn text(&self) -> &str {
        match self {
            Fail::Temporary(s) | Fail::Permanent(s) => s,
            Fail::Changed => "The file changed on the server",
        }
    }
}

/// Connections for a file of this size when set to Auto: small files don't need many.
pub fn auto_connections(size: u64) -> u32 {
    const MB: u64 = 1024 * 1024;
    match size {
        0 => 8,
        s if s < 4 * MB => 1,
        s if s < 32 * MB => 4,
        s if s < 256 * MB => 8,
        s if s < 2048 * MB => 16,
        _ => 24,
    }
}

fn status_fail(code: reqwest::StatusCode) -> Fail {
    let text = match code.as_u16() {
        401 | 403 => "The server refused access (the link may have expired)".to_string(),
        404 | 410 => "The file isn't on the server any more (404)".to_string(),
        416 => "The server can't resume this file".to_string(),
        c => format!("The server answered {c} {}", code.canonical_reason().unwrap_or("")),
    };
    if code.is_server_error() || code.as_u16() == 429 || code.as_u16() == 408 {
        Fail::Temporary(text)
    } else {
        Fail::Permanent(text)
    }
}

fn net_fail(e: reqwest::Error) -> Fail {
    let mut msg = e.to_string();
    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(&e);
    while let Some(s) = src {
        msg = format!("{msg}: {s}");
        src = s.source();
    }
    if e.is_builder() {
        Fail::Permanent(format!("That link isn't valid: {msg}"))
    } else {
        Fail::Temporary(msg)
    }
}

/// File name from Content-Disposition (RFC 6266 / 5987).
pub fn disposition_name(v: &str) -> Option<String> {
    let mut plain = None;
    for part in v.split(';').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("filename*=").map(|_| &part["filename*=".len()..]) {
            let rest = rest.trim().trim_matches('"');
            let encoded = rest.splitn(3, '\'').nth(2).unwrap_or(rest);
            let decoded = percent_encoding::percent_decode_str(encoded).decode_utf8_lossy().to_string();
            if !decoded.is_empty() {
                return Some(decoded);
            }
        } else if lower.starts_with("filename=") {
            let raw = part["filename=".len()..].trim().trim_matches('"').to_string();
            if !raw.is_empty() {
                plain = Some(percent_encoding::percent_decode_str(&raw).decode_utf8_lossy().to_string());
            }
        }
    }
    plain
}

/// File name from the last path segment of a URL.
pub fn url_name(u: &str) -> Option<String> {
    let parsed = url::Url::parse(u).ok()?;
    let seg = parsed.path_segments()?.filter(|s| !s.is_empty()).next_back()?.to_string();
    let decoded = percent_encoding::percent_decode_str(&seg).decode_utf8_lossy().to_string();
    (!decoded.is_empty()).then_some(decoded)
}

/// "bytes 0-99/1234" → 1234
fn content_range_total(v: &str) -> Option<u64> {
    v.rsplit('/').next()?.trim().parse().ok()
}

async fn probe(req: &Request, from: u64, end: Option<u64>) -> Result<Probe, Fail> {
    if crate::ftp::is_ftp(&req.url) {
        let (info, response) = crate::ftp::probe(&req.url, from, true).await?;
        return Ok(Probe { info, response });
    }
    let range = match end {
        Some(e) => format!("bytes={from}-{}", e.saturating_sub(1)),
        None => format!("bytes={from}-"),
    };
    let mut rq = client().get(&req.url).headers(headers(req)).header(header::RANGE, range);
    if from > 0 && !req.if_range.is_empty() {
        rq = rq.header(header::IF_RANGE, &req.if_range);
    }
    let resp = rq.send().await.map_err(net_fail)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(status_fail(status));
    }
    // We asked for the middle of a known file and got the whole thing: it changed.
    if from > 0 && !req.if_range.is_empty() && status == reqwest::StatusCode::OK {
        return Err(Fail::Changed);
    }
    let h = resp.headers();
    let get = |k: header::HeaderName| h.get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let final_url = resp.url().to_string();
    let etag = get(header::ETAG);
    let last_modified = get(header::LAST_MODIFIED);
    let ranged = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let (size, resumable) = if ranged {
        (content_range_total(&get(header::CONTENT_RANGE)).unwrap_or(0), true)
    } else {
        (resp.content_length().unwrap_or(0), false)
    };
    let name = disposition_name(&get(header::CONTENT_DISPOSITION))
        .or_else(|| url_name(&final_url))
        .or_else(|| url_name(&req.url))
        .unwrap_or_else(|| "download".into());
    let mut name = model::sanitize(&name);
    let ct_full = get(header::CONTENT_TYPE).to_ascii_lowercase();
    let ct_main = ct_full.split(';').next().unwrap_or("").trim().to_string();
    let page = ct_main == "text/html" || ct_main == "application/xhtml+xml";
    let lower_url = final_url.to_ascii_lowercase();
    let path_only = lower_url.split(['?', '#']).next().unwrap_or("");
    let stream = path_only.ends_with(".m3u8")
        || path_only.ends_with(".mpd")
        || matches!(ct_main.as_str(), "application/vnd.apple.mpegurl" | "application/x-mpegurl" | "audio/mpegurl" | "application/dash+xml");
    // No extension but a clear content type: add one, so sorting works.
    if sorter::extension(&name).is_empty() {
        let ct = ct_full.clone();
        let ext = match ct.split(';').next().unwrap_or("").trim() {
            "application/zip" => "zip",
            "application/pdf" => "pdf",
            "video/mp4" => "mp4",
            "audio/mpeg" => "mp3",
            "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => "exe",
            "application/x-7z-compressed" => "7z",
            "application/vnd.rar" | "application/x-rar-compressed" => "rar",
            "image/jpeg" => "jpg",
            "image/png" => "png",
            _ => "",
        };
        if !ext.is_empty() {
            name = format!("{name}.{ext}");
        }
    }
    let size = if ranged || from == 0 { size } else { 0 };
    let s = settings::get();
    let category = sorter::categorize(&s.rules, &name, size, &final_url);
    let folder = sorter::folder_for(&s, &category).to_string_lossy().to_string();
    let response: Option<ByteStream> = (ranged || from == 0).then(|| {
        use futures_util::StreamExt;
        resp.bytes_stream().map(|r| r.map_err(net_fail)).boxed()
    });
    Ok(Probe {
        info: ProbeInfo { name, size, resumable, final_url, etag, last_modified, category, folder, debrid: false, page, stream, content_type: ct_main.clone(), safety: None },
        response,
    })
}

/// For the Add dialog: what the link is (name, size, where it would go).
pub async fn inspect(req: Request) -> Result<ProbeInfo, String> {
    if crate::ftp::is_ftp(&req.url) {
        return crate::ftp::probe(&req.url, 0, false).await.map(|(i, _)| i).map_err(|e| e.text().to_string());
    }
    match probe(&req, 0, None).await {
        Ok(p) => Ok(p.info),
        Err(e) => Err(e.text().to_string()),
    }
}

// ---------------------------------------------------------------- file helpers

fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".haul");
    PathBuf::from(s)
}

#[cfg(windows)]
fn set_sparse(f: &std::fs::File, on: bool) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    const FSCTL_SET_SPARSE: u32 = 0x0009_00C4;
    let flag: u8 = on as u8;
    let mut out = 0u32;
    unsafe {
        DeviceIoControl(
            f.as_raw_handle() as _,
            FSCTL_SET_SPARSE,
            &flag as *const u8 as _,
            1,
            std::ptr::null_mut(),
            0,
            &mut out,
            std::ptr::null_mut(),
        );
    }
}

#[cfg(not(windows))]
fn set_sparse(_f: &std::fs::File, _on: bool) {}

fn write_at(f: &std::fs::File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_write(f, buf, offset)?;
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::write_at(f, buf, offset)?;
        if n == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

fn disk_fail(e: std::io::Error) -> Fail {
    let raw = e.raw_os_error().unwrap_or(0);
    // ERROR_DISK_FULL / ERROR_HANDLE_DISK_FULL / ENOSPC
    if raw == 112 || raw == 39 || raw == 28 {
        Fail::Permanent("The disk is full".into())
    } else {
        Fail::Permanent(format!("Couldn't write the file: {e}"))
    }
}

// ---------------------------------------------------------------- the download

struct Job {
    req: Request,
    file: Arc<std::fs::File>,
    ctl: Arc<Control>,
    resumable: bool,
    max_retries: u32,
}

/// An If-Range value: a strong ETag, else the Last-Modified date (weak ETags can't be used).
pub fn strong_validator(etag: &str, last_modified: &str) -> String {
    let e = etag.trim();
    if !e.is_empty() && !e.starts_with("W/") {
        return e.to_string();
    }
    last_modified.trim().to_string()
}

/// Plans the ranges for a file of `size` bytes and `conns` connections.
pub fn plan(size: u64, conns: u32) -> Vec<Segment> {
    let n = ((size / MIN_SPLIT).max(1)).min(conns.max(1) as u64);
    let per = size / n;
    (0..n)
        .map(|i| {
            let start = i * per;
            let end = if i == n - 1 { size } else { (i + 1) * per };
            Segment { start, end, done: 0 }
        })
        .collect()
}

/// Splits the largest remaining range in two; returns the new range's index.
pub fn split_largest(segs: &mut Vec<Segment>) -> Option<usize> {
    let (idx, rem) = segs.iter().enumerate().map(|(i, s)| (i, s.remaining())).max_by_key(|(_, r)| *r)?;
    if rem < MIN_SPLIT * 2 {
        return None;
    }
    let s = &mut segs[idx];
    let cut = s.start + s.done + rem / 2;
    let new = Segment { start: cut, end: s.end, done: 0 };
    s.end = cut;
    segs.push(new);
    Some(segs.len() - 1)
}

/// Runs one download until it completes, fails or is stopped. Errors are returned as text.
pub async fn run(id: u64, ctl: Arc<Control>) -> Result<(), Fail> {
    // If the file changes on the server mid-way, start it over cleanly (once) instead of
    // stitching two different files together.
    match run_once(id, ctl.clone()).await {
        Err(Fail::Changed) => {
            restart_clean(id);
            ctl.downloaded.store(0, Ordering::Relaxed);
            ctl.segments.lock().clear();
            match run_once(id, ctl).await {
                Err(Fail::Changed) => Err(Fail::Temporary("The file keeps changing on the server".into())),
                other => other,
            }
        }
        other => other,
    }
}

/// Forget progress (the partial file is overwritten from the start).
fn restart_clean(id: u64) {
    model::with(|st| {
        if let Some(it) = st.get_mut(id) {
            it.segments.clear();
            it.done = 0;
            it.etag.clear();
            it.last_modified.clear();
            it.note = "The file changed on the server, so it started over".into();
        }
    });
}

async fn run_once(id: u64, ctl: Arc<Control>) -> Result<(), Fail> {
    let Some(mut item) = model::with(|s| s.get(id).cloned()) else { return Ok(()) };
    // Real-Debrid links: get (or renew) the direct download link first.
    if item.source.is_empty() && !item.debrid_link.is_empty() {
        let u = crate::debrid::unrestrict(&item.debrid_link, "").await.map_err(|e| {
            if e.starts_with("Couldn't reach") || e.contains("busy") { Fail::Temporary(e) } else { Fail::Permanent(e) }
        })?;
        model::with(|st| {
            if let Some(it) = st.get_mut(id) {
                it.source = u.download.clone();
            }
        });
        item.source = u.download;
    }
    let s = settings::get();
    let mut req = Request {
        url: item.source.clone(),
        referrer: item.referrer.clone(),
        cookies: item.cookies.clone(),
        user_agent: item.user_agent.clone(),
        bearer: if item.hf_auth { crate::hf::token() } else { String::new() },
        if_range: String::new(),
    };
    let is_ftp = crate::ftp::is_ftp(&req.url);

    let mut segments = item.segments.clone();
    let mut first_response: Option<ByteStream> = None;
    let fresh = segments.is_empty() || item.file_name.is_empty();
    if fresh {
        segments.clear();
    }
    let mut resumable = item.resumable;
    let mut size = item.size;

    if !fresh {
        // Resuming: check the file on the server is still the same one.
        let first = segments.iter().find(|s| s.remaining() > 0).cloned();
        if let Some(seg) = first {
            let end = if size > 0 && resumable { Some(seg.end) } else { None };
            // Check before resuming: same ETag, same date, same size. Otherwise start over
            // cleanly rather than making a file that's half old, half new.
            req.if_range = strong_validator(&item.etag, &item.last_modified);
            let p = match probe(&req, seg.start + seg.done, end).await {
                Err(Fail::Changed) => None,
                other => Some(other?),
            };
            let changed = match &p {
                None => true,
                Some(p) => {
                    (!item.etag.is_empty() && !p.info.etag.is_empty() && p.info.etag != item.etag)
                        || (!item.last_modified.is_empty() && !p.info.last_modified.is_empty() && p.info.last_modified != item.last_modified)
                        || (size > 0 && p.info.size > 0 && p.info.size != size && p.info.resumable)
                        || !p.info.resumable
                }
            };
            if changed || !resumable {
                if changed && resumable {
                    restart_clean(id);
                }
                segments.clear(); // start over
                req.if_range.clear();
            } else if let Some(p) = p {
                first_response = p.response;
            }
        }
    }

    if segments.is_empty() {
        let p = probe(&req, 0, None).await?;
        resumable = p.info.resumable && p.info.size > 0;
        size = p.info.size;
        first_response = p.response;
        // Name, category and folder are picked now unless the user already chose them.
        let (dir, name) = model::with(|st| {
            let taken: Vec<String> = st.items.iter().filter(|i| i.id != id).map(|i| i.file_name.clone()).collect();
            let it = st.get_mut(id).unwrap();
            let auto_name = it.file_name.is_empty();
            if it.category.is_empty() {
                it.category = p.info.category.clone();
            }
            if it.save_dir.is_empty() {
                it.save_dir = sorter::folder_for(&s, &it.category).to_string_lossy().to_string();
            }
            let dir = PathBuf::from(&it.save_dir);
            let wanted = if it.file_name.is_empty() { p.info.name.clone() } else { it.file_name.clone() };
            // Our own partial file from an earlier attempt can be reused.
            let reuse = dir.join(format!("{wanted}.haul")).exists() && !dir.join(&wanted).exists();
            it.file_name = if reuse { wanted } else { model::unique_name(&dir, &wanted, &taken) };
            if auto_name || it.name.is_empty() {
                it.name = it.file_name.clone();
            }
            it.size = size;
            it.resumable = resumable;
            it.final_url = p.info.final_url.clone();
            it.etag = p.info.etag.clone();
            it.last_modified = p.info.last_modified.clone();
            it.done = 0;
            (dir, it.file_name.clone())
        });
        let _ = std::fs::create_dir_all(&dir);
        let conns = if item.connections > 0 {
            item.connections
        } else if s.connections > 0 {
            s.connections
        } else {
            auto_connections(size)
        };
        // FTP servers often allow only a few connections per user.
        let conns = if is_ftp { conns.clamp(1, 4) } else { conns.clamp(1, 32) };
        segments = if resumable { plan(size, conns) } else { vec![Segment { start: 0, end: if size > 0 { size } else { u64::MAX }, done: 0 }] };
        let path = part_path(&dir.join(&name));
        let f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&path).map_err(disk_fail)?;
        if size > 0 {
            set_sparse(&f, true);
            f.set_len(size).map_err(disk_fail)?;
        }
    }
    let (dir, final_name) = model::with(|st| st.get(id).map(|i| (PathBuf::from(&i.save_dir), i.file_name.clone())))
        .ok_or_else(|| Fail::Permanent("removed".into()))?;
    let final_path = dir.join(&final_name);
    let part = part_path(&final_path);
    let file = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(&part).map_err(disk_fail)?;
    *ctl.segments.lock() = segments.clone();
    model::with(|st| {
        if let Some(it) = st.get_mut(id) {
            it.segments = segments.clone();
        }
    });

    // Every part must come from this exact file (see Request::if_range).
    req.if_range = model::with(|st| st.get(id).map(|i| strong_validator(&i.etag, &i.last_modified))).unwrap_or_default();
    let job = Arc::new(Job { req, file: Arc::new(file), ctl: ctl.clone(), resumable, max_retries: s.max_retries });

    // One worker per unfinished range; the first reuses the probe's response.
    let mut tasks = Vec::new();
    let pending: Vec<usize> = segments.iter().enumerate().filter(|(_, s)| s.remaining() > 0).map(|(i, _)| i).collect();
    for (n, idx) in pending.into_iter().enumerate() {
        let resp = if n == 0 { first_response.take() } else { None };
        let job = job.clone();
        tasks.push(tokio::spawn(async move { worker(job, idx, resp).await }));
    }
    // Ranges were all complete already (e.g. crash right before rename).
    let mut result = Ok(());
    for t in tasks {
        match t.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if result.is_ok() {
                    result = Err(e);
                }
                ctl.stop.store(true, Ordering::Relaxed);
            }
            Err(e) => result = Err(Fail::Temporary(format!("worker crashed: {e}"))),
        }
    }

    let segs = ctl.segments.lock().clone();
    let done: u64 = segs.iter().map(|s| s.done).sum();
    model::with(|st| {
        if let Some(it) = st.get_mut(id) {
            it.segments = segs.clone();
            it.done = done;
        }
    });
    result?;
    if ctl.stopped() {
        return Ok(());
    }
    if segs.iter().any(|s| s.remaining() > 0 && s.end != u64::MAX) {
        return Err(Fail::Temporary("Some parts didn't finish".into()));
    }

    // Complete: finish the file and put it in place.
    drop(job);
    {
        let f = std::fs::OpenOptions::new().write(true).open(&part).map_err(disk_fail)?;
        let real = if size > 0 { size } else { done };
        f.set_len(real).map_err(disk_fail)?;
        set_sparse(&f, false);
        let _ = f.sync_all();
    }
    let target = if final_path.exists() {
        let n = model::unique_name(&dir, &final_name, &[]);
        dir.join(n)
    } else {
        final_path.clone()
    };
    std::fs::rename(&part, &target).map_err(|e| Fail::Permanent(format!("Couldn't rename the finished file: {e}")))?;
    let real = if size > 0 { size } else { done };
    model::with(|st| {
        if let Some(it) = st.get_mut(id) {
            it.file_name = target.file_name().unwrap().to_string_lossy().to_string();
            it.size = real;
            it.done = real;
            it.segments.clear();
        }
    });
    Ok(())
}

async fn worker(job: Arc<Job>, mut idx: usize, mut resp: Option<ByteStream>) -> Result<(), Fail> {
    job.ctl.connections.fetch_add(1, Ordering::Relaxed);
    let r = worker_inner(&job, &mut idx, &mut resp).await;
    job.ctl.connections.fetch_sub(1, Ordering::Relaxed);
    r
}

async fn worker_inner(job: &Arc<Job>, idx: &mut usize, resp: &mut Option<ByteStream>) -> Result<(), Fail> {
    let mut failures = 0u32;
    loop {
        if job.ctl.stopped() {
            return Ok(());
        }
        let (pos, end) = {
            let segs = job.ctl.segments.lock();
            let s = &segs[*idx];
            (s.start + s.done, s.end)
        };
        if pos >= end {
            // Our range is done: help with the biggest one left.
            if !job.resumable {
                return Ok(());
            }
            let next = {
                let mut segs = job.ctl.segments.lock();
                split_largest(&mut segs)
            };
            match next {
                Some(n) => {
                    *idx = n;
                    *resp = None;
                    continue;
                }
                None => return Ok(()),
            }
        }
        let r = match resp.take() {
            Some(r) => Ok(r),
            None => {
                let range_end = if end == u64::MAX { None } else { Some(end) };
                probe(&job.req, pos, range_end).await.and_then(|p| {
                    if !p.info.resumable && pos > 0 {
                        Err(Fail::Permanent("The server stopped supporting resume".into()))
                    } else {
                        p.response.ok_or_else(|| Fail::Temporary("no response".into()))
                    }
                })
            }
        };
        match r {
            Ok(r) => match pump(job, *idx, r).await {
                Ok(progressed) => {
                    if progressed {
                        failures = 0;
                    }
                }
                Err(Fail::Temporary(e)) => {
                    failures += 1;
                    if failures > job.max_retries {
                        return Err(Fail::Temporary(e));
                    }
                    if !job.resumable {
                        // Can't continue mid-file: start this download over.
                        let mut segs = job.ctl.segments.lock();
                        segs[*idx].done = 0;
                    }
                    backoff(job, failures).await;
                }
                Err(other) => return Err(other),
            },
            Err(Fail::Temporary(e)) => {
                failures += 1;
                if failures > job.max_retries {
                    return Err(Fail::Temporary(e));
                }
                backoff(job, failures).await;
            }
            Err(other) => return Err(other),
        }
    }
}

async fn backoff(job: &Arc<Job>, failures: u32) {
    let wait = Duration::from_secs((1u64 << failures.min(5)).min(30));
    let step = Duration::from_millis(200);
    let mut waited = Duration::ZERO;
    while waited < wait && !job.ctl.stopped() {
        tokio::time::sleep(step).await;
        waited += step;
    }
}

/// Streams one response into its range. Ok(true) if any bytes arrived.
async fn pump(job: &Arc<Job>, idx: usize, mut stream: ByteStream) -> Result<bool, Fail> {
    let mut progressed = false;
    loop {
        if job.ctl.stopped() {
            return Ok(progressed);
        }
        let next = tokio::time::timeout(CHUNK_TIMEOUT, stream.next()).await;
        let chunk = match next {
            Err(_) => return Err(Fail::Temporary("The connection stalled".into())),
            Ok(None) => {
                // Stream ended. If the range isn't complete the server cut us off.
                let mut segs = job.ctl.segments.lock();
                let s = &mut segs[idx];
                if s.end == u64::MAX {
                    s.end = s.start + s.done; // unknown size: end of stream = end of file
                    return Ok(progressed);
                }
                if s.remaining() == 0 {
                    return Ok(progressed);
                }
                return Err(Fail::Temporary("The server closed the connection early".into()));
            }
            Ok(Some(Err(e))) => return Err(e),
            Ok(Some(Ok(c))) => c,
        };
        let n = chunk.len() as u64;
        crate::http::global_limiter().acquire(n).await;
        job.ctl.limiter.acquire(n).await;
        // The range may have been shortened by a split while we waited.
        let (pos, take) = {
            let segs = job.ctl.segments.lock();
            let s = &segs[idx];
            let pos = s.start + s.done;
            (pos, n.min(s.end.saturating_sub(pos)))
        };
        if take > 0 {
            write_at(&job.file, &chunk[..take as usize], pos).map_err(disk_fail)?;
            job.ctl.segments.lock()[idx].done += take;
            job.ctl.downloaded.fetch_add(take, Ordering::Relaxed);
            progressed = true;
        }
        if take < n || {
            let segs = job.ctl.segments.lock();
            segs[idx].remaining() == 0
        } {
            return Ok(progressed); // our (possibly shortened) range is complete
        }
    }
}

/// SHA-256 of a file, hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// After a download is stopped for good (removed), drop its partial file.
pub fn remove_partial(item: &model::Item) {
    if item.file_name.is_empty() {
        return;
    }
    let _ = std::fs::remove_file(part_path(&item.file_path()));
}

pub fn log_fail(id: u64, f: &Fail) {
    paths::log(format!("download {id}: {}", f.text()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_ranges() {
        let p = plan(100 * 1024 * 1024, 8);
        assert_eq!(p.len(), 8);
        assert_eq!(p.last().unwrap().end, 100 * 1024 * 1024);
        assert_eq!(p[1].start, p[0].end);
        assert_eq!(plan(1000, 8).len(), 1);
        assert_eq!(plan(5 * 1024 * 1024, 8).len(), 2);
    }

    #[test]
    fn splits() {
        let mut s = vec![Segment { start: 0, end: 10 << 20, done: 2 << 20 }, Segment { start: 10 << 20, end: 11 << 20, done: 0 }];
        let i = split_largest(&mut s).unwrap();
        assert_eq!(i, 2);
        assert_eq!(s[0].end, 6 << 20);
        assert_eq!(s[2].start, 6 << 20);
        assert_eq!(s[2].end, 10 << 20);
        let mut small = vec![Segment { start: 0, end: 3 << 20, done: 0 }];
        assert!(split_largest(&mut small).is_none());
    }

    #[test]
    fn names() {
        assert_eq!(disposition_name(r#"attachment; filename="a b.zip""#).unwrap(), "a b.zip");
        assert_eq!(disposition_name("attachment; filename*=UTF-8''caf%C3%A9.txt; filename=\"cafe.txt\"").unwrap(), "café.txt");
        assert_eq!(url_name("https://x.com/dl/file%20one.iso?x=1").unwrap(), "file one.iso");
        assert_eq!(url_name("https://x.com/"), None);
        assert_eq!(content_range_total("bytes 0-0/12345"), Some(12345));
    }
}

#[cfg(test)]
mod e2e {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A tiny HTTP server with Range support that drops some connections midway.
    async fn serve(data: Arc<Vec<u8>>, flaky: bool) -> u16 {
        serve_versions(Arc::new(parking_lot::Mutex::new((data, "\"v1\"".to_string()))), flaky).await
    }

    /// Like `serve`, but the file (and its ETag) can be swapped, and If-Range is honoured.
    async fn serve_versions(current: Arc<parking_lot::Mutex<(Arc<Vec<u8>>, String)>>, flaky: bool) -> u16 {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let counter = Arc::new(AtomicU64::new(0));
        tokio::spawn(async move {
            loop {
                let (mut s, _) = l.accept().await.unwrap();
                let (data, etag) = current.lock().clone();
                let counter = counter.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let len = data.len() as u64;
                    let if_range = req.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("if-range: ").map(|v| v.trim().to_string()));
                    let stale = if_range.map(|v| v != etag.to_ascii_lowercase()).unwrap_or(false);
                    let (start, end) = req
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("range: bytes=").map(|r| r.to_string()))
                        .map(|r| {
                            let (a, b) = r.split_once('-').unwrap();
                            let a: u64 = a.parse().unwrap();
                            let b: u64 = if b.is_empty() { len - 1 } else { b.parse().unwrap() };
                            (a, b.min(len - 1) + 1)
                        })
                        .unwrap_or((0, len));
                    // If-Range didn't match: the whole (new) file, as a 200.
                    let (start, end) = if stale { (0, len) } else { (start, end) };
                    let head = if stale {
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\nETag: {etag}\r\nContent-Disposition: attachment; filename=\"test file.bin\"\r\nConnection: close\r\n\r\n")
                    } else {
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nETag: {etag}\r\nContent-Disposition: attachment; filename=\"test file.bin\"\r\nConnection: close\r\n\r\n",
                            end - start,
                            start,
                            end - 1,
                            len
                        )
                    };
                    if s.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    let nth = counter.fetch_add(1, Ordering::Relaxed);
                    let mut pos = start;
                    while pos < end {
                        let e = (pos + 65536).min(end);
                        if s.write_all(&data[pos as usize..e as usize]).await.is_err() {
                            return;
                        }
                        pos = e;
                        // Every third connection dies after ~1 MB.
                        if flaky && nth % 3 == 1 && pos - start > 1_000_000 {
                            return;
                        }
                        tokio::time::sleep(Duration::from_micros(200)).await;
                    }
                });
            }
        });
        port
    }

    fn setup() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("haul-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LOCALAPPDATA", &dir);
        dir
    }

    fn data(n: usize) -> Arc<Vec<u8>> {
        Arc::new((0..n).map(|i| ((i * 2654435761usize) >> 13) as u8).collect())
    }

    fn sha(b: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(b))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn segmented_download_with_drops_and_resume() {
        let dir = setup();
        let out = dir.join("out");
        let d = data(24 * 1024 * 1024 + 123);
        let port = serve(d.clone(), true).await;
        let url = format!("http://127.0.0.1:{port}/files/x");
        let id = model::with(|s| {
            s.insert(model::Item {
                source: url.clone(),
                save_dir: out.to_string_lossy().to_string(),
                connections: 8,
                ..Default::default()
            })
        });

        // Run, stop partway, and resume (like a pause, or a restart).
        let ctl = Control::new();
        let c2 = ctl.clone();
        let h = tokio::spawn(async move { run(id, c2).await });
        tokio::time::sleep(Duration::from_millis(700)).await;
        ctl.stop.store(true, Ordering::Relaxed);
        h.await.unwrap().unwrap();
        let partial = model::with(|s| s.get(id).cloned()).unwrap();
        assert!(partial.resumable);
        assert!(partial.segments.len() >= 8, "segments {}", partial.segments.len());
        assert!(partial.done > 0 && partial.done < d.len() as u64, "done {}", partial.done);
        assert_eq!(partial.file_name, "test file.bin");

        let ctl = Control::new();
        run(id, ctl.clone()).await.unwrap();
        let it = model::with(|s| s.get(id).cloned()).unwrap();
        assert_eq!(it.size, d.len() as u64);
        let got = std::fs::read(out.join("test file.bin")).unwrap();
        assert_eq!(got.len(), d.len());
        assert_eq!(sha(&got), sha(&d));
        assert!(!out.join("test file.bin.haul").exists());
        assert_eq!(sha256_file(&out.join("test file.bin")).unwrap(), sha(&d));
    }

    /// Paused, then the file changed on the server: resuming must start over, not mix files.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn resume_after_server_file_changed() {
        let dir = setup();
        let out = dir.join("changed");
        let v1 = data(12 * 1024 * 1024);
        let v2: Arc<Vec<u8>> = Arc::new(v1.iter().map(|b| b.wrapping_add(7)).collect());
        let current = Arc::new(parking_lot::Mutex::new((v1.clone(), "\"v1\"".to_string())));
        let port = serve_versions(current.clone(), false).await;
        let id = model::with(|s| {
            s.insert(model::Item { source: format!("http://127.0.0.1:{port}/f"), save_dir: out.to_string_lossy().to_string(), file_name: "changed.bin".into(), connections: 6, ..Default::default() })
        });
        let ctl = Control::new();
        ctl.limiter.set_rate(4 * 1024 * 1024); // slow enough to pause it partway
        let c2 = ctl.clone();
        let h = tokio::spawn(async move { run(id, c2).await });
        tokio::time::sleep(Duration::from_millis(700)).await;
        ctl.stop.store(true, Ordering::Relaxed);
        h.await.unwrap().unwrap();
        let partial = model::with(|s| s.get(id).cloned()).unwrap();
        assert!(partial.done > 0 && partial.done < v1.len() as u64, "done {}", partial.done);
        assert_eq!(partial.etag, "\"v1\"");

        // The server now has a different file.
        *current.lock() = (v2.clone(), "\"v2\"".to_string());
        run(id, Control::new()).await.unwrap();
        let got = std::fs::read(out.join("changed.bin")).unwrap();
        assert_eq!(sha(&got), sha(&v2), "must be the new file, not a mix");
        let it = model::with(|s| s.get(id).cloned()).unwrap();
        assert_eq!(it.etag, "\"v2\"");
        assert!(it.note.contains("changed"), "note: {}", it.note);
    }
}
