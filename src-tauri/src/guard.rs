//! The update guardian: a temporary copy of the *current* Haul, started with `--guard <plan>`.
//! It installs the update after Haul exits, starts the new version, and waits for it to say
//! it's healthy. If the new version doesn't come up, it restores the previous files and starts
//! the old version again (which then skips that release).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Plan {
    pub portable: bool,
    /// The verified installer, or the new Haul.exe in portable mode.
    pub file: PathBuf,
    /// Haul.exe to start after updating.
    pub target_exe: PathBuf,
    pub install_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub from: String,
    pub to: String,
    pub parent_pid: u32,
    pub data_dir: PathBuf,
    pub guardian: PathBuf,
}

const SHIPPED_DIRS: &[&str] = &["extension"];

pub fn health_marker(data: &Path, version: &str) -> PathBuf {
    data.join("updates").join(format!("healthy-{version}"))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let p = e.path();
        let t = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &t)?;
        } else {
            std::fs::copy(&p, &t)?;
        }
    }
    Ok(())
}

/// Backs up the current version and writes the plan. Runs in the old Haul.
pub fn prepare(file: &Path, portable: bool, from: &str, to: &str) -> Result<Plan, String> {
    let p = paths::get();
    let updates = p.updates();
    let backup_dir = updates.join(format!("backup-{from}"));
    let _ = std::fs::remove_dir_all(&backup_dir);
    std::fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
    let exe_name = p.exe.file_name().ok_or("bad exe path")?;
    std::fs::copy(&p.exe, backup_dir.join(exe_name)).map_err(|e| format!("Couldn't back up Haul: {e}"))?;
    for d in SHIPPED_DIRS {
        let src = p.exe_dir.join(d);
        if src.is_dir() {
            copy_dir(&src, &backup_dir.join(d)).map_err(|e| format!("Couldn't back up {d}: {e}"))?;
        }
    }
    // The guardian is a copy of this exe, so the installer can replace the real one.
    let guardian = updates.join("Haul-guardian.exe");
    let _ = std::fs::remove_file(&guardian);
    std::fs::copy(&p.exe, &guardian).map_err(|e| format!("Couldn't prepare the update: {e}"))?;
    let _ = std::fs::remove_file(health_marker(&p.data, to));
    Ok(Plan {
        portable,
        file: file.to_path_buf(),
        target_exe: p.exe.clone(),
        install_dir: p.exe_dir.clone(),
        backup_dir,
        from: from.to_string(),
        to: to.to_string(),
        parent_pid: std::process::id(),
        data_dir: p.data.clone(),
        guardian,
    })
}

pub fn launch(plan: &Plan) -> Result<(), String> {
    let path = plan.data_dir.join("updates").join("plan.json");
    std::fs::write(&path, serde_json::to_vec_pretty(plan).unwrap()).map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(&plan.guardian);
    cmd.arg("--guard").arg(&path);
    detach(&mut cmd);
    cmd.spawn().map_err(|e| format!("Couldn't start the updater: {e}"))?;
    Ok(())
}

fn detach(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        cmd.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

fn log(data: &Path, msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(data.join("haul.log")) {
        let _ = writeln!(f, "{} [guardian] {msg}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

#[cfg(windows)]
fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return true; // already gone
        }
        let r = WaitForSingleObject(h, timeout.as_millis() as u32);
        CloseHandle(h);
        r == WAIT_OBJECT_0
    }
}

#[cfg(not(windows))]
fn wait_for_exit(_pid: u32, timeout: Duration) -> bool {
    std::thread::sleep(timeout.min(Duration::from_secs(1)));
    true
}

fn retry<T>(mut f: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut last = None;
    for _ in 0..40 {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(last.unwrap())
}

fn install(plan: &Plan) -> Result<(), String> {
    if plan.portable {
        retry(|| std::fs::copy(&plan.file, &plan.target_exe)).map_err(|e| format!("replacing Haul.exe: {e}"))?;
        Ok(())
    } else {
        let mut cmd = std::process::Command::new(&plan.file);
        cmd.arg("/S");
        let status = cmd.status().map_err(|e| format!("running the installer: {e}"))?;
        if !status.success() {
            return Err(format!("the installer failed ({status})"));
        }
        Ok(())
    }
}

fn restore(plan: &Plan) -> Result<(), String> {
    let exe_name = plan.target_exe.file_name().unwrap();
    retry(|| std::fs::copy(plan.backup_dir.join(exe_name), &plan.target_exe)).map_err(|e| e.to_string())?;
    for d in SHIPPED_DIRS {
        let b = plan.backup_dir.join(d);
        if b.is_dir() {
            let t = plan.install_dir.join(d);
            let _ = std::fs::remove_dir_all(&t);
            copy_dir(&b, &t).map_err(|e| e.to_string())?;
        }
    }
    #[cfg(windows)]
    if !plan.portable {
        // Apps & features should show the version that's actually installed.
        use winreg::RegKey;
        use winreg::enums::*;
        if let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Haul", KEY_WRITE) {
            let _ = k.set_value("DisplayVersion", &plan.from);
        }
    }
    Ok(())
}

fn skip_version(plan: &Plan) {
    let path = plan.data_dir.join("settings.json");
    let mut v: serde_json::Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(serde_json::json!({}));
    v["skipVersion"] = serde_json::Value::String(plan.to.clone());
    let _ = std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap());
}

/// Entry point for `Haul.exe --guard <plan.json>`. Never shows a window.
pub fn run(plan_path: &Path) -> i32 {
    let Ok(bytes) = std::fs::read(plan_path) else { return 1 };
    let Ok(plan) = serde_json::from_slice::<Plan>(&bytes) else { return 1 };
    let data = plan.data_dir.clone();
    log(&data, &format!("updating {} -> {}", plan.from, plan.to));

    if !wait_for_exit(plan.parent_pid, Duration::from_secs(60)) {
        log(&data, "Haul didn't close; update cancelled");
        return 1;
    }
    let ok = match install(&plan) {
        Ok(()) => start_and_wait(&plan),
        Err(e) => {
            log(&data, &format!("install failed: {e}"));
            false
        }
    };
    if ok {
        log(&data, &format!("{} is running", plan.to));
        let _ = std::fs::remove_dir_all(&plan.backup_dir);
        let _ = std::fs::remove_file(plan_path);
        return 0;
    }
    log(&data, "rolling back");
    if let Err(e) = restore(&plan) {
        log(&data, &format!("rollback failed: {e}"));
    }
    skip_version(&plan);
    let mut cmd = std::process::Command::new(&plan.target_exe);
    cmd.arg("--rolled-back").arg(&plan.to);
    detach(&mut cmd);
    let _ = cmd.spawn();
    2
}

/// Starts the new version and waits up to a minute for its "I'm healthy" marker.
fn start_and_wait(plan: &Plan) -> bool {
    let marker = health_marker(&plan.data_dir, &plan.to);
    let mut cmd = std::process::Command::new(&plan.target_exe);
    cmd.arg("--after-update").arg(&plan.to);
    detach(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            log(&plan.data_dir, &format!("couldn't start the new version: {e}"));
            return false;
        }
    };
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(60) {
        if marker.exists() {
            let _ = std::fs::remove_file(&marker);
            return true;
        }
        if let Ok(Some(status)) = child.try_wait() {
            log(&plan.data_dir, &format!("new version exited early ({status})"));
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    log(&plan.data_dir, "new version didn't report healthy in time");
    let _ = child.kill();
    let _ = child.wait();
    false
}

/// Called by the new version once its window has drawn.
pub fn report_healthy(version: &str) {
    let p = paths::get();
    let _ = std::fs::create_dir_all(p.updates());
    let _ = std::fs::write(health_marker(&p.data, version), b"ok");
}

/// Old update files (the guardian can't delete itself).
pub fn cleanup() {
    let u = paths::get().updates();
    let current = env!("CARGO_PKG_VERSION");
    if let Ok(rd) = std::fs::read_dir(&u) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let newer = match (crate::updater::parse_version(&name), crate::updater::parse_version(current)) {
                (Some(a), Some(b)) => a > b,
                _ => false,
            };
            let keep = newer || name.starts_with("healthy-") || name.starts_with("backup-") || name == "Haul-guardian.exe";
            if !keep {
                let p = e.path();
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                } else {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
    }
}
