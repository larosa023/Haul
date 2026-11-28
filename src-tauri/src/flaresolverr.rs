//! FlareSolverr helper: Prowlarr sends Cloudflare-protected sites through it. Before a torrent
//! search Haul checks it's answering on port 8191 and, if not, starts it hidden (no console
//! window) from where it was found: the path in Settings, a Startup-folder shortcut, or the
//! usual unzip places. It's left running afterwards, as if started by hand.

use std::path::PathBuf;
use std::time::Duration;

const URL: &str = "http://127.0.0.1:8191/";

pub async fn running() -> bool {
    crate::http::client().get(URL).timeout(Duration::from_secs(2)).send().await.map(|r| r.status().is_success()).unwrap_or(false)
}

/// Where flaresolverr.exe is, if Haul can find it.
pub fn find() -> Option<PathBuf> {
    let set = crate::settings::with(|s| s.flaresolverr_path.clone());
    if !set.is_empty() && PathBuf::from(&set).is_file() {
        return Some(PathBuf::from(set));
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut tries: Vec<PathBuf> = Vec::new();
    if let Some(home) = env("USERPROFILE") {
        for d in ["Downloads", "Desktop", "Documents", ""] {
            let base = if d.is_empty() { home.clone() } else { home.join(d) };
            tries.push(base.join("flaresolverr").join("flaresolverr.exe"));
            tries.push(base.join("flaresolverr_windows_x64").join("flaresolverr").join("flaresolverr.exe"));
            tries.push(base.join("FlareSolverr").join("flaresolverr.exe"));
            // One folder deeper too (e.g. Downloads\Some folder\flaresolverr).
            if !d.is_empty() {
                for sub in std::fs::read_dir(&base).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).take(200) {
                    tries.push(sub.join("flaresolverr").join("flaresolverr.exe"));
                    tries.push(sub.join("flaresolverr.exe"));
                }
            }
        }
    }
    for k in ["LOCALAPPDATA", "ProgramFiles", "ProgramData"] {
        if let Some(b) = env(k) {
            tries.push(b.join("FlareSolverr").join("flaresolverr.exe"));
            tries.push(b.join("flaresolverr").join("flaresolverr.exe"));
        }
    }
    if let Some(p) = tries.into_iter().find(|p| p.is_file()) {
        return Some(p);
    }
    startup_shortcut()
}

/// A shortcut to it in the Startup folder (what the setup steps suggest).
#[cfg(windows)]
fn startup_shortcut() -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    let dir = PathBuf::from(std::env::var_os("APPDATA")?).join(r"Microsoft\Windows\Start Menu\Programs\Startup");
    let lnk = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        let n = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        n.contains("flaresolverr") && n.ends_with(".lnk")
    })?;
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "(New-Object -ComObject WScript.Shell).CreateShortcut($env:HAUL_LNK).TargetPath"])
        .env("HAUL_LNK", &lnk)
        .env_remove("PSModulePath")
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let target = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    target.is_file().then_some(target)
}

#[cfg(not(windows))]
fn startup_shortcut() -> Option<PathBuf> {
    None
}

/// Makes sure it's answering, starting it if needed (waits up to 40 s on first start).
pub async fn ensure() {
    if !crate::settings::with(|s| s.flaresolverr_auto) || running().await {
        return;
    }
    let Some(exe) = find() else { return };
    let mut cmd = std::process::Command::new(&exe);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window, and not tied to Haul (it keeps running if Haul closes).
        cmd.creation_flags(0x0800_0000 | 0x0000_0008);
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if cmd.spawn().is_err() {
        return;
    }
    crate::settings::update(|s| {
        if s.flaresolverr_path.is_empty() {
            s.flaresolverr_path = exe.to_string_lossy().into();
        }
    });
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if running().await {
            return;
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    pub path: String,
}

pub async fn status() -> Status {
    Status { running: running().await, path: find().map(|p| p.to_string_lossy().into()).unwrap_or_default() }
}
