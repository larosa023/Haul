//! Per-user Windows integration (no admin rights needed): magnet links and .torrent files,
//! the Default Apps entry, and "start with Windows".

#[derive(serde::Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HandlerState {
    pub magnet: bool,
    pub torrent: bool,
}

#[cfg(windows)]
mod imp {
    use super::HandlerState;
    use winreg::RegKey;
    use winreg::enums::*;

    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    fn exe() -> String {
        crate::paths::get().exe.to_string_lossy().to_string()
    }

    fn set_default(path: &str, value: &str) -> std::io::Result<RegKey> {
        let (k, _) = hkcu().create_subkey(path)?;
        k.set_value("", &value)?;
        Ok(k)
    }

    /// `classic`: also point the plain `.torrent` / `magnet` keys at Haul (not just its own ProgIDs).
    pub fn register(classic: bool) -> std::io::Result<()> {
        let exe = exe();
        let cmd = format!("\"{exe}\" \"%1\"");
        let icon = format!("\"{exe}\",0");
        // ProgIDs
        set_default(r"Software\Classes\Haul.Torrent", "Torrent file")?;
        set_default(r"Software\Classes\Haul.Torrent\DefaultIcon", &icon)?;
        set_default(r"Software\Classes\Haul.Torrent\shell\open\command", &cmd)?;
        let m = set_default(r"Software\Classes\Haul.Magnet", "Magnet link")?;
        m.set_value("URL Protocol", &"")?;
        set_default(r"Software\Classes\Haul.Magnet\DefaultIcon", &icon)?;
        set_default(r"Software\Classes\Haul.Magnet\shell\open\command", &cmd)?;
        let (owp, _) = hkcu().create_subkey(r"Software\Classes\.torrent\OpenWithProgids")?;
        owp.set_value("Haul.Torrent", &"")?;
        // The classic keys (used when nothing else was chosen in Default Apps)
        let classic_ours = hkcu()
            .open_subkey(r"Software\Classes\magnet\shell\open\command")
            .and_then(|k| k.get_value::<String, _>(""))
            .map(|v| v.to_ascii_lowercase().contains("haul"))
            .unwrap_or(false);
        if classic || classic_ours {
            let (t, _) = hkcu().create_subkey(r"Software\Classes\.torrent")?;
            t.set_value("", &"Haul.Torrent")?;
            t.set_value("Content Type", &"application/x-bittorrent")?;
            let mg = set_default(r"Software\Classes\magnet", "URL:Magnet link")?;
            mg.set_value("URL Protocol", &"")?;
            set_default(r"Software\Classes\magnet\DefaultIcon", &icon)?;
            set_default(r"Software\Classes\magnet\shell\open\command", &cmd)?;
        }
        // Default Apps listing
        let (cap, _) = hkcu().create_subkey(r"Software\Haul\Capabilities")?;
        cap.set_value("ApplicationName", &"Haul")?;
        cap.set_value("ApplicationDescription", &"Download manager and torrent client")?;
        let (fa, _) = hkcu().create_subkey(r"Software\Haul\Capabilities\FileAssociations")?;
        fa.set_value(".torrent", &"Haul.Torrent")?;
        let (ua, _) = hkcu().create_subkey(r"Software\Haul\Capabilities\UrlAssociations")?;
        ua.set_value("magnet", &"Haul.Magnet")?;
        let (ra, _) = hkcu().create_subkey(r"Software\RegisteredApplications")?;
        ra.set_value("Haul", &r"Software\Haul\Capabilities")?;
        refresh_shell();
        Ok(())
    }

    pub fn unregister() {
        let h = hkcu();
        let _ = h.delete_subkey_all(r"Software\Classes\Haul.Torrent");
        let _ = h.delete_subkey_all(r"Software\Classes\Haul.Magnet");
        let _ = h.delete_subkey_all(r"Software\Haul");
        if let Ok(k) = h.open_subkey_with_flags(r"Software\RegisteredApplications", KEY_WRITE) {
            let _ = k.delete_value("Haul");
        }
        if let Ok(k) = h.open_subkey_with_flags(r"Software\Classes\.torrent\OpenWithProgids", KEY_WRITE) {
            let _ = k.delete_value("Haul.Torrent");
        }
        // Only undo the classic keys if they still point at us.
        if let Ok(k) = h.open_subkey(r"Software\Classes\.torrent") {
            if k.get_value::<String, _>("").map(|v| v == "Haul.Torrent").unwrap_or(false) {
                let _ = h.open_subkey_with_flags(r"Software\Classes\.torrent", KEY_WRITE).and_then(|k| k.delete_value(""));
            }
        }
        if let Ok(k) = h.open_subkey(r"Software\Classes\magnet\shell\open\command") {
            if k.get_value::<String, _>("").map(|v| v.contains(&exe())).unwrap_or(false) {
                let _ = h.delete_subkey_all(r"Software\Classes\magnet");
            }
        }
        refresh_shell();
    }

    fn user_choice(path: &str) -> Option<String> {
        hkcu().open_subkey(path).ok()?.get_value::<String, _>("ProgId").ok()
    }

    /// Is Haul what Windows actually opens for each?
    pub fn state() -> HandlerState {
        let exe = exe().to_ascii_lowercase();
        let classic_magnet = hkcu()
            .open_subkey(r"Software\Classes\magnet\shell\open\command")
            .and_then(|k| k.get_value::<String, _>(""))
            .map(|v| v.to_ascii_lowercase().contains(&exe))
            .unwrap_or(false);
        let magnet = match user_choice(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\magnet\UserChoice") {
            Some(p) => p == "Haul.Magnet" || (p == "magnet" && classic_magnet),
            None => classic_magnet,
        };
        let classic_torrent = hkcu()
            .open_subkey(r"Software\Classes\.torrent")
            .and_then(|k| k.get_value::<String, _>(""))
            .map(|v| v == "Haul.Torrent")
            .unwrap_or(false);
        let torrent = match user_choice(r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.torrent\UserChoice") {
            Some(p) => p == "Haul.Torrent",
            None => classic_torrent,
        };
        HandlerState { magnet, torrent }
    }

    fn refresh_shell() {
        use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as _, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
    }

    pub fn set_startup(on: bool) -> std::io::Result<()> {
        let (k, _) = hkcu().create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")?;
        if on {
            k.set_value("Haul", &format!("\"{}\" --minimized", exe()))
        } else {
            match k.delete_value("Haul") {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        }
    }

    /// Windows' accent color as #rrggbb.
    pub fn accent_color() -> Option<String> {
        let k = hkcu().open_subkey(r"Software\Microsoft\Windows\DWM").ok()?;
        let v: u32 = k.get_value("AccentColor").or_else(|_| k.get_value("ColorizationColor")).ok()?;
        // AccentColor is ABGR.
        let (r, g, b) = (v & 0xFF, (v >> 8) & 0xFF, (v >> 16) & 0xFF);
        Some(format!("#{r:02x}{g:02x}{b:02x}"))
    }

    /// Mica/Acrylic backdrops need Windows 11 22H2 (build 22621) or later.
    pub fn supports_backdrop() -> bool {
        RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .and_then(|k| k.get_value::<String, _>("CurrentBuild"))
            .ok()
            .and_then(|b| b.parse::<u32>().ok())
            .map(|b| b >= 22621)
            .unwrap_or(false)
    }

    pub fn apps_use_light_theme() -> bool {
        hkcu()
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
            .map(|v| v != 0)
            .unwrap_or(false)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::HandlerState;
    pub fn register(_classic: bool) -> std::io::Result<()> {
        Ok(())
    }
    pub fn unregister() {}
    pub fn state() -> HandlerState {
        HandlerState::default()
    }
    pub fn set_startup(_on: bool) -> std::io::Result<()> {
        Ok(())
    }
    pub fn accent_color() -> Option<String> {
        None
    }
    pub fn apps_use_light_theme() -> bool {
        false
    }
    pub fn supports_backdrop() -> bool {
        false
    }
}

pub use imp::*;
