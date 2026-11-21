//! Where Haul keeps its files.
//!
//! Normal install: `%LOCALAPPDATA%\Haul\` (settings.json, items.json, torrents\, themes\, updates\).
//! Portable mode: an empty file named `portable` next to Haul.exe puts everything in `.\data\`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct Paths {
    pub exe: PathBuf,
    pub exe_dir: PathBuf,
    pub data: PathBuf,
    pub portable: bool,
}

static PATHS: OnceLock<Paths> = OnceLock::new();

pub fn get() -> &'static Paths {
    PATHS.get_or_init(|| {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Haul.exe"));
        let exe_dir = exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let portable = exe_dir.join("portable").exists();
        let data = if portable {
            exe_dir.join("data")
        } else if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            PathBuf::from(local).join("Haul")
        } else {
            dirs_home().join(".haul")
        };
        let _ = std::fs::create_dir_all(&data);
        Paths { exe, exe_dir, data, portable }
    })
}

fn dirs_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

impl Paths {
    pub fn settings(&self) -> PathBuf {
        self.data.join("settings.json")
    }
    pub fn items(&self) -> PathBuf {
        self.data.join("items.json")
    }
    /// librqbit's session state and fast-resume data.
    pub fn session(&self) -> PathBuf {
        self.data.join("session")
    }
    /// Copies of every .torrent we've added, so a torrent can always be re-added.
    pub fn torrents(&self) -> PathBuf {
        self.data.join("torrents")
    }
    pub fn themes(&self) -> PathBuf {
        self.data.join("themes")
    }
    /// Themes shared by the whole suite (Hangar's folder).
    pub fn suite_themes(&self) -> Option<PathBuf> {
        std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Hangar").join("themes"))
    }
    pub fn updates(&self) -> PathBuf {
        self.data.join("updates")
    }
    pub fn log(&self) -> PathBuf {
        self.data.join("haul.log")
    }
    pub fn webview(&self) -> PathBuf {
        self.data.join("webview")
    }
}

/// The user's Downloads folder.
pub fn downloads_dir() -> PathBuf {
    #[cfg(windows)]
    {
        // The real location (users often move Downloads to another drive).
        use winreg::enums::HKEY_CURRENT_USER;
        if let Ok(k) = winreg::RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders")
        {
            if let Ok(v) = k.get_value::<String, _>("{374DE290-123F-4565-9164-39C4925E467B}") {
                let expanded = expand_env(&v);
                if !expanded.is_empty() {
                    return PathBuf::from(expanded);
                }
            }
        }
    }
    dirs_home().join("Downloads")
}

/// Expands %VAR% references.
pub fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('%') {
            let name = &after[..end];
            match std::env::var(name) {
                Ok(v) => out.push_str(&v),
                Err(_) => {
                    out.push('%');
                    out.push_str(name);
                    out.push('%');
                }
            }
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

/// Simple append-only log, trimmed when it gets big.
pub fn log(msg: impl AsRef<str>) {
    use std::io::Write;
    let path = get().log();
    if std::fs::metadata(&path).map(|m| m.len() > 2_000_000).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), msg.as_ref());
    }
}

/// Writes a file atomically (temp file + rename) so a crash never leaves it half-written.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn expands() {
        std::env::set_var("HAUL_T", "x");
        assert_eq!(super::expand_env("a%HAUL_T%b"), "axb");
        assert_eq!(super::expand_env("%NOPE_HAUL%"), "%NOPE_HAUL%");
        assert_eq!(super::expand_env("50%"), "50%");
    }
}
