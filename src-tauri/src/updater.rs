//! Haul's own updates, from GitHub Releases of larosa023/Haul.
//!
//! - Checks on launch and every few hours. Stable sees normal releases; Beta also sees
//!   pre-releases (and versions with a "-", like 0.2.0-beta.1).
//! - Downloads in the background and verifies SHA-256 against GitHub's asset digest and the
//!   release's SHA256SUMS.txt (both must agree when both exist; neither = refused).
//! - When ready it asks to restart, and waits until nothing is downloading (or you confirm).
//! - The swap is done by a small "guardian" copy of the current Haul: it installs the update,
//!   starts the new version and waits for it to report healthy. If it doesn't, the guardian puts
//!   the previous version back and starts it. Downloads resume either way.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{engine, paths, settings};

pub const REPO: &str = "larosa023/Haul";

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    /// idle | checking | none | downloading | ready | applying | error
    pub status: String,
    pub current: String,
    pub version: String,
    pub notes: String,
    pub page_url: String,
    pub progress: f64,
    pub error: String,
    /// Restart is queued for when downloads finish.
    pub waiting: bool,
    pub checked_at: i64,
}

#[derive(Clone)]
struct Ready {
    version: String,
    file: PathBuf,
    portable: bool,
}

static STATE: Mutex<Option<UpdateState>> = Mutex::new(None);
static READY: Mutex<Option<Ready>> = Mutex::new(None);
static APPLY_WHEN_IDLE: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);

pub fn state() -> UpdateState {
    let mut s = STATE.lock().clone().unwrap_or_default();
    s.current = env!("CARGO_PKG_VERSION").to_string();
    if s.status.is_empty() {
        s.status = "idle".into();
    }
    s.waiting = APPLY_WHEN_IDLE.load(Ordering::Relaxed);
    s
}

fn set(f: impl FnOnce(&mut UpdateState)) {
    {
        let mut g = STATE.lock();
        f(g.get_or_insert_with(Default::default));
    }
    engine::emit("update", state());
}

#[derive(Deserialize, Debug, Clone)]
pub struct GhAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GhRelease {
    pub tag_name: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<GhAsset>,
}

pub fn parse_version(v: &str) -> Option<semver::Version> {
    semver::Version::parse(v.trim().trim_start_matches('v')).ok()
}

/// The newest release for this channel that's newer than `current`.
pub fn pick<'a>(releases: &'a [GhRelease], current: &semver::Version, beta: bool, skip: &str) -> Option<(&'a GhRelease, semver::Version)> {
    releases
        .iter()
        .filter(|r| !r.draft)
        .filter_map(|r| parse_version(&r.tag_name).map(|v| (r, v)))
        .filter(|(r, v)| beta || (!r.prerelease && v.pre.is_empty()))
        .filter(|(_, v)| v > current)
        .filter(|(_, v)| skip.is_empty() || parse_version(skip).map(|s| &s != v).unwrap_or(true))
        .max_by(|a, b| a.1.cmp(&b.1))
}

pub fn asset_for(r: &GhRelease, portable: bool) -> Option<&GhAsset> {
    if portable {
        r.assets.iter().find(|a| a.name.eq_ignore_ascii_case("Haul.exe"))
    } else {
        r.assets.iter().find(|a| a.name.to_ascii_lowercase().ends_with("-setup.exe"))
    }
}

/// Expected SHA-256 from SHA256SUMS.txt ("<hash>  <name>" lines).
pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut p = l.split_whitespace();
        let h = p.next()?;
        let n = p.next()?.trim_start_matches('*');
        (n.eq_ignore_ascii_case(name) && h.len() == 64).then(|| h.to_ascii_lowercase())
    })
}

async fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
    let r = crate::http::client()
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", format!("Haul/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("GitHub answered {}", r.status()));
    }
    r.json().await.map_err(|e| format!("Unexpected answer from GitHub: {e}"))
}

async fn get_text(url: &str) -> Result<String, String> {
    let r = crate::http::client()
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("{}", r.status()));
    }
    r.text().await.map_err(|e| e.to_string())
}

/// Checks for an update; downloads it when auto-update is on (or `download` is true).
pub async fn check(manual: bool) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let r = check_inner(manual).await;
    BUSY.store(false, Ordering::SeqCst);
    if let Err(e) = r {
        paths::log(format!("update check: {e}"));
        set(|s| {
            s.status = "error".into();
            s.error = e;
            s.checked_at = chrono::Utc::now().timestamp();
        });
    }
}

async fn check_inner(manual: bool) -> Result<(), String> {
    if READY.lock().is_some() {
        set(|s| s.status = "ready".into());
        return Ok(());
    }
    set(|s| {
        s.status = "checking".into();
        s.error.clear();
    });
    let s = settings::get();
    let current = parse_version(env!("CARGO_PKG_VERSION")).unwrap();
    let releases: Vec<GhRelease> = get_json(&format!("https://api.github.com/repos/{REPO}/releases?per_page=30")).await?;
    let skip = if manual { String::new() } else { s.skip_version.clone() };
    let Some((rel, ver)) = pick(&releases, &current, s.channel == "beta", &skip) else {
        set(|st| {
            st.status = "none".into();
            st.checked_at = chrono::Utc::now().timestamp();
        });
        return Ok(());
    };
    let portable = paths::get().portable;
    let asset = asset_for(rel, portable).ok_or("The new release has no Windows download yet")?.clone();
    set(|st| {
        st.version = ver.to_string();
        st.notes = rel.body.clone().unwrap_or_default();
        st.page_url = rel.html_url.clone();
        st.checked_at = chrono::Utc::now().timestamp();
    });
    if !s.auto_update && !manual {
        set(|st| st.status = "available".into());
        return Ok(());
    }

    // Expected hashes: GitHub's digest and SHA256SUMS.txt.
    let gh = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).map(|d| d.to_ascii_lowercase());
    let sums = match rel.assets.iter().find(|a| a.name.eq_ignore_ascii_case("SHA256SUMS.txt")) {
        Some(a) => get_text(&a.browser_download_url).await.ok().and_then(|t| sum_for(&t, &asset.name)),
        None => None,
    };
    let expected = match (&gh, &sums) {
        (Some(a), Some(b)) if a != b => return Err("The release's checksums disagree, so it wasn't installed".into()),
        (Some(a), _) => a.clone(),
        (None, Some(b)) => b.clone(),
        (None, None) => return Err("The release has no checksum, so it wasn't installed".into()),
    };

    let dir = paths::get().updates().join(ver.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(&asset.name);
    if !(file.exists() && crate::http::sha256_file(&file).map(|h| h == expected).unwrap_or(false)) {
        set(|st| {
            st.status = "downloading".into();
            st.progress = 0.0;
        });
        download(&asset, &file, &expected).await?;
    }
    *READY.lock() = Some(Ready { version: ver.to_string(), file, portable });
    set(|st| {
        st.status = "ready".into();
        st.progress = 1.0;
    });
    // With auto-update on, restart by itself once nothing's downloading.
    if s.auto_update {
        APPLY_WHEN_IDLE.store(true, Ordering::Relaxed);
        set(|_| {});
    }
    Ok(())
}

async fn download(asset: &GhAsset, file: &Path, expected: &str) -> Result<(), String> {
    let r = crate::http::client()
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|e| format!("Update download failed: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("Update download failed: {}", r.status()));
    }
    let total = r.content_length().unwrap_or(asset.size).max(1);
    let tmp = file.with_extension("part");
    let mut out = tokio::fs::File::create(&tmp).await.map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut got = 0u64;
    let mut stream = r.bytes_stream();
    let mut last = std::time::Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Update download failed: {e}"))?;
        h.update(&chunk);
        tokio::io::AsyncWriteExt::write_all(&mut out, &chunk).await.map_err(|e| e.to_string())?;
        got += chunk.len() as u64;
        if last.elapsed() > Duration::from_millis(400) {
            last = std::time::Instant::now();
            set(|s| s.progress = got as f64 / total as f64);
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut out).await.map_err(|e| e.to_string())?;
    drop(out);
    let actual = hex::encode(h.finalize());
    if actual != expected {
        let _ = std::fs::remove_file(&tmp);
        return Err("The update didn't match its checksum, so it wasn't installed".into());
    }
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())?;
    Ok(())
}

/// Runs in the background: a check shortly after launch, then every 4 hours.
pub async fn background() {
    tokio::time::sleep(Duration::from_secs(8)).await;
    loop {
        check(false).await;
        tokio::time::sleep(Duration::from_secs(4 * 3600)).await;
    }
}

/// From the engine tick: restart into the update when nothing's downloading and the window
/// isn't in use (hidden in the tray or minimized).
pub fn maybe_apply_when_idle(idle: bool) {
    if !idle || !APPLY_WHEN_IDLE.load(Ordering::Relaxed) || READY.lock().is_none() {
        return;
    }
    if crate::window_in_use() {
        return;
    }
    APPLY_WHEN_IDLE.store(false, Ordering::Relaxed);
    tauri::async_runtime::spawn(async { apply(true).await.ok() });
}

/// "Restart to update". With `force` false and downloads running, it waits for them.
pub async fn apply(force: bool) -> Result<(), String> {
    let Some(ready) = READY.lock().clone() else { return Err("No update is ready".into()) };
    if !force && engine::busy() {
        APPLY_WHEN_IDLE.store(true, Ordering::Relaxed);
        set(|_| {});
        return Ok(());
    }
    set(|s| s.status = "applying".into());
    let p = paths::get();
    let plan = crate::guard::prepare(&ready.file, ready.portable, env!("CARGO_PKG_VERSION"), &ready.version).map_err(|e| {
        set(|s| {
            s.status = "error".into();
            s.error = e.clone();
        });
        e
    })?;
    paths::log(format!("updating {} -> {}", env!("CARGO_PKG_VERSION"), ready.version));
    engine::shutdown().await;
    crate::guard::launch(&plan).map_err(|e| {
        set(|s| {
            s.status = "error".into();
            s.error = e.clone();
        });
        e
    })?;
    let _ = p;
    if let Some(app) = engine::app() {
        app.exit(0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, pre: bool) -> GhRelease {
        GhRelease { tag_name: tag.into(), prerelease: pre, draft: false, body: None, html_url: String::new(), assets: vec![] }
    }

    #[test]
    fn channels() {
        let cur = parse_version("0.1.0").unwrap();
        let rs = vec![rel("v0.1.1", false), rel("v0.2.0-beta.1", true), rel("v0.1.2-rc.1", false), rel("v0.0.9", false)];
        assert_eq!(pick(&rs, &cur, false, "").unwrap().1.to_string(), "0.1.1");
        assert_eq!(pick(&rs, &cur, true, "").unwrap().1.to_string(), "0.2.0-beta.1");
        assert!(pick(&rs, &cur, false, "0.1.1").is_none());
        let cur2 = parse_version("0.2.0").unwrap();
        assert!(pick(&rs, &cur2, true, "").is_none());
    }

    #[test]
    fn sums() {
        let t = "aaaa  other.exe\nABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789 *Haul_0.1.1_x64-setup.exe\n";
        assert_eq!(sum_for(t, "Haul_0.1.1_x64-setup.exe").unwrap(), "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789");
        assert!(sum_for(t, "other.exe").is_none());
    }
}
