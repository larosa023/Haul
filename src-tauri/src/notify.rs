//! Windows notifications with "Open" and "Show in folder" buttons.

#[allow(dead_code)]
pub const APP_ID: &str = "larosa023.Haul";

/// Registers Haul's notification identity (name + icon) for the current user, so toasts work
/// in portable mode too, without a Start menu shortcut.
pub fn register() {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let p = crate::paths::get();
        if let Ok((k, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(format!(r"Software\Classes\AppUserModelId\{APP_ID}")) {
            let _ = k.set_value("DisplayName", &"Haul");
            let icon = p.data.join("haul-icon.png");
            if !icon.exists() {
                let _ = std::fs::write(&icon, include_bytes!("../icons/128x128.png"));
            }
            let _ = k.set_value("IconUri", &icon.to_string_lossy().to_string());
            let _ = k.set_value("IconBackgroundColor", &"FF16161A");
        }
    }
}

/// Shows a notification for an item. With `actions`, the toast gets Open / Show in folder.
pub fn show(id: u64, title: &str, body: &str, actions: bool) {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::{Duration, Sound, Toast};
        let mut t = Toast::new(APP_ID).title(title).text1(body).sound(Some(Sound::Default)).duration(Duration::Short);
        if actions {
            t = t.add_button("Open", &format!("open:{id}")).add_button("Show in folder", &format!("folder:{id}"));
        }
        let t = t.on_activated(move |action| {
            match action.as_deref() {
                Some(a) if a.starts_with("open:") => crate::commands::open_item_path(id),
                Some(a) if a.starts_with("folder:") => crate::commands::reveal_item(id),
                _ => crate::show_main_window(),
            }
            Ok(())
        });
        if let Err(e) = t.show() {
            crate::paths::log(format!("notification failed: {e}"));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (id, title, body, actions);
    }
}
