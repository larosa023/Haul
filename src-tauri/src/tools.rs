//! yt-dlp and ffmpeg: downloaded on first use (not bundled), checked against SHA-256, and kept
//! in `tools\`. yt-dlp is kept up to date automatically, since sites change and break it often.
//!
//! - yt-dlp: the latest release of github.com/yt-dlp/yt-dlp (yt-dlp.exe, checked against the
//!   release's SHA2-256SUMS and GitHub's own digest).
//! - ffmpeg: yt-dlp's FFmpeg builds (github.com/yt-dlp/FFmpeg-Builds, win64 GPL), checked
//!   against the release's checksums.sha256 and GitHub's digest; ffmpeg.exe and ffprobe.exe are
//!   unpacked from the zip.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::updater::{GhAsset, GhRelease, sum_for};
use crate::{engine, paths};

const YTDLP_REPO: &str = "yt-dlp/yt-dlp";
const FFMPEG_REPO: &str = "yt-dlp/FFmpeg-Builds";

#[cfg(windows)]
const YTDLP_ASSET: &str = "yt-dlp.exe";
#[cfg(not(windows))]
const YTDLP_ASSET: &str = "yt-dlp_linux";
#[cfg(windows)]
const FFMPEG_ASSET: &str = "ffmpeg-master-latest-win64-gpl.zip";
#[cfg(not(windows))]
const FFMPEG_ASSET: &str = "ffmpeg-master-latest-linux64-gpl.tar.xz";
const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

pub fn dir() -> PathBuf {
    paths::get().data.join("tools")
}

pub fn ytdlp() -> PathBuf {
    dir().join(format!("yt-dlp{EXE}"))
}

pub fn ffmpeg() -> PathBuf {
    dir().join(format!("ffmpeg{EXE}"))
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Versions {
    pub ytdlp: String,
    pub ffmpeg: String,
    /// Unix time of the last yt-dlp update check.
    pub checked: i64,
}

fn versions_path() -> PathBuf {
    dir().join("versions.json")
}

pub fn versions() -> Versions {
    std::fs::read(versions_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_versions(v: &Versions) {
    let _ = std::fs::create_dir_all(dir());
    let _ = paths::write_atomic(&versions_path(), &serde_json::to_vec_pretty(v).unwrap());
}

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ToolsState {
    pub ytdlp: String,
    pub ffmpeg: String,
    /// "" when idle, else what's being fetched ("yt-dlp" / "ffmpeg").
    pub busy: String,
    pub progress: f64,
    pub error: String,
}

static STATE: Mutex<Option<ToolsState>> = Mutex::new(None);
static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn state() -> ToolsState {
    let mut s = STATE.lock().clone().unwrap_or_default();
    let v = versions();
    s.ytdlp = if ytdlp().exists() { v.ytdlp } else { String::new() };
    s.ffmpeg = if ffmpeg().exists() { v.ffmpeg } else { String::new() };
    s
}

fn set(f: impl FnOnce(&mut ToolsState)) {
    {
        let mut g = STATE.lock();
        f(g.get_or_insert_with(Default::default));
    }
    engine::emit("tools", state());
}

pub fn ready() -> bool {
    ytdlp().exists() && ffmpeg().exists()
}

async fn release(repo: &str, tag: Option<&str>) -> Result<GhRelease, String> {
    let url = match tag {
        Some(t) => format!("https://api.github.com/repos/{repo}/releases/tags/{t}"),
        None => format!("https://api.github.com/repos/{repo}/releases/latest"),
    };
    let r = crate::http::client()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("GitHub answered {}", r.status()));
    }
    r.json().await.map_err(|e| format!("Unexpected answer from GitHub: {e}"))
}

/// The SHA-256 the release promises for `asset`: GitHub's digest and the checksum file must
/// agree when both exist; neither means we refuse.
async fn expected_sha(rel: &GhRelease, asset: &GhAsset, sums_names: &[&str]) -> Result<String, String> {
    let gh = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).map(|d| d.to_ascii_lowercase());
    let mut file = None;
    if let Some(a) = rel.assets.iter().find(|a| sums_names.iter().any(|n| a.name.eq_ignore_ascii_case(n))) {
        if let Ok(r) = crate::http::client().get(&a.browser_download_url).timeout(Duration::from_secs(30)).send().await {
            if let Ok(t) = r.text().await {
                file = sum_for(&t, &asset.name);
            }
        }
    }
    match (gh, file) {
        (Some(a), Some(b)) if a != b => Err(format!("{}: the checksums disagree, so it wasn't installed", asset.name)),
        (Some(a), _) => Ok(a),
        (None, Some(b)) => Ok(b),
        (None, None) => Err(format!("{} has no checksum, so it wasn't installed", asset.name)),
    }
}

/// Downloads to `dest`, checking the SHA-256 as it goes.
async fn fetch(url: &str, dest: &Path, expected: &str, label: &str) -> Result<(), String> {
    let r = crate::http::client().get(url).send().await.map_err(|e| format!("Downloading {label} failed: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("Downloading {label} failed: {}", r.status()));
    }
    let total = r.content_length().unwrap_or(0).max(1);
    let tmp = dest.with_extension("part");
    let mut out = tokio::fs::File::create(&tmp).await.map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut got = 0u64;
    let mut stream = r.bytes_stream();
    let mut last = std::time::Instant::now();
    while let Some(c) = stream.next().await {
        let c = c.map_err(|e| format!("Downloading {label} failed: {e}"))?;
        h.update(&c);
        tokio::io::AsyncWriteExt::write_all(&mut out, &c).await.map_err(|e| e.to_string())?;
        got += c.len() as u64;
        if last.elapsed() > Duration::from_millis(300) {
            last = std::time::Instant::now();
            set(|s| s.progress = got as f64 / total as f64);
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut out).await.map_err(|e| e.to_string())?;
    drop(out);
    if hex::encode(h.finalize()) != expected {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{label} didn't match its checksum, so it wasn't installed"));
    }
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

async fn install_ytdlp() -> Result<(), String> {
    set(|s| {
        s.busy = "yt-dlp".into();
        s.progress = 0.0;
        s.error.clear();
    });
    let rel = release(YTDLP_REPO, None).await?;
    let asset = rel.assets.iter().find(|a| a.name == YTDLP_ASSET).ok_or("The yt-dlp release has no Windows download")?.clone();
    let sha = expected_sha(&rel, &asset, &["SHA2-256SUMS"]).await?;
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let dest = ytdlp();
    let tmp = dest.with_extension("new");
    fetch(&asset.browser_download_url, &tmp, &sha, "yt-dlp").await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    // Replace (yt-dlp may be running a download; a running exe can be renamed on Windows).
    if dest.exists() {
        let old = dest.with_extension("old");
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(&dest, &old);
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    let mut v = versions();
    v.ytdlp = rel.tag_name.clone();
    v.checked = chrono::Utc::now().timestamp();
    save_versions(&v);
    paths::log(format!("yt-dlp {} installed", rel.tag_name));
    Ok(())
}

async fn install_ffmpeg() -> Result<(), String> {
    set(|s| {
        s.busy = "ffmpeg".into();
        s.progress = 0.0;
        s.error.clear();
    });
    let rel = release(FFMPEG_REPO, Some("latest")).await?;
    let asset = rel.assets.iter().find(|a| a.name == FFMPEG_ASSET).ok_or("The ffmpeg release has no Windows download")?.clone();
    let sha = expected_sha(&rel, &asset, &["checksums.sha256"]).await?;
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let archive = dir().join(&asset.name);
    fetch(&asset.browser_download_url, &archive, &sha, "ffmpeg").await?;
    set(|s| s.busy = "ffmpeg (unpacking)".into());
    let out = dir();
    let res = tokio::task::spawn_blocking(move || unpack_ffmpeg(&archive, &out)).await.map_err(|e| e.to_string())?;
    res?;
    let mut v = versions();
    v.ffmpeg = asset.updated_at.clone().unwrap_or_else(|| rel.tag_name.clone()).chars().take(10).collect();
    save_versions(&v);
    paths::log("ffmpeg installed");
    Ok(())
}

/// Takes ffmpeg and ffprobe out of the build's bin\ folder.
fn unpack_ffmpeg(archive: &Path, out: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let f = std::fs::File::open(archive).map_err(|e| e.to_string())?;
        let mut z = zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|e| e.to_string())?;
        let mut found = 0;
        for i in 0..z.len() {
            let mut e = z.by_index(i).map_err(|e| e.to_string())?;
            let name = e.name().replace('\\', "/");
            let file = name.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
            if name.contains("/bin/") && (file == "ffmpeg.exe" || file == "ffprobe.exe") {
                let tmp = out.join(format!("{file}.new"));
                let mut w = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
                std::io::copy(&mut e, &mut w).map_err(|e| e.to_string())?;
                drop(w);
                let dest = out.join(&file);
                let _ = std::fs::remove_file(&dest);
                std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
                found += 1;
            }
        }
        let _ = std::fs::remove_file(archive);
        if found < 2 {
            return Err("The ffmpeg download didn't contain ffmpeg.exe and ffprobe.exe".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (archive, out);
        Err("ffmpeg is installed automatically on Windows only".into())
    }
}

/// Makes sure yt-dlp (and, with `need_ffmpeg`, ffmpeg) are installed.
pub async fn ensure(need_ffmpeg: bool) -> Result<(), String> {
    let _g = LOCK.lock().await;
    let mut r = Ok(());
    if !ytdlp().exists() {
        r = install_ytdlp().await;
    }
    if r.is_ok() && need_ffmpeg && !ffmpeg().exists() {
        r = install_ffmpeg().await;
    }
    set(|s| {
        s.busy.clear();
        s.error = r.as_ref().err().cloned().unwrap_or_default();
    });
    r
}

/// Updates yt-dlp if a newer release is out. `force` checks now, otherwise at most daily.
pub async fn update_ytdlp(force: bool) -> Result<bool, String> {
    if !ytdlp().exists() {
        return Ok(false);
    }
    let v = versions();
    let now = chrono::Utc::now().timestamp();
    if !force && now - v.checked < 20 * 3600 {
        return Ok(false);
    }
    let _g = LOCK.lock().await;
    let rel = release(YTDLP_REPO, None).await?;
    let mut v2 = versions();
    v2.checked = now;
    save_versions(&v2);
    if rel.tag_name == v.ytdlp {
        return Ok(false);
    }
    let r = install_ytdlp().await;
    set(|s| {
        s.busy.clear();
        s.error = r.as_ref().err().cloned().unwrap_or_default();
    });
    r.map(|_| true)
}

/// Background: keep yt-dlp current (checks shortly after launch, then every few hours).
pub async fn background() {
    tokio::time::sleep(Duration::from_secs(30)).await;
    loop {
        if crate::settings::with(|s| s.ytdlp_auto_update) {
            if let Err(e) = update_ytdlp(false).await {
                paths::log(format!("yt-dlp update check: {e}"));
            }
        }
        tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
    }
}

/// Removes the tools (they come back on next use).
pub fn remove() {
    let _ = std::fs::remove_dir_all(dir());
    set(|_| {});
}
