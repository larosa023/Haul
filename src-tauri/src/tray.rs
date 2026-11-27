//! The tray icon: Haul lives here when its window is closed.

use std::sync::OnceLock;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

static TRAY: OnceLock<TrayIcon> = OnceLock::new();
static ALT: OnceLock<CheckMenuItem<Wry>> = OnceLock::new();

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Open Haul", true, None::<&str>)?;
    let add = MenuItem::with_id(app, "add", "New download…", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause_all", "Pause all", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume_all", "Resume all", true, None::<&str>)?;
    let hk = crate::settings::with(|s| s.mini_hotkey.clone());
    let mini_label = if hk.trim().is_empty() { "Mini window".to_string() } else { format!("Mini window\t{hk}") };
    let mini = MenuItem::with_id(app, "mini", mini_label, true, None::<&str>)?;
    let alt = CheckMenuItem::with_id(app, "alt", "Limit speed", true, crate::settings::with(|s| s.alt_enabled), None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Haul", true, None::<&str>)?;
    let sep = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(app, &[&show, &add, &mini, &sep()?, &pause, &resume, &alt, &sep()?, &quit])?;
    let _ = ALT.set(alt);

    let tray = TrayIconBuilder::with_id("haul")
        .icon(app.default_window_icon().cloned().unwrap())
        .tooltip("Haul")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "show" => crate::show_main_window(),
            "add" => {
                crate::show_main_window();
                crate::engine::emit("open-add", ());
            }
            "pause_all" => {
                tauri::async_runtime::spawn(async {
                    let ids = crate::model::with(|s| s.items.iter().filter(|i| !i.paused).map(|i| i.id).collect());
                    crate::engine::pause(ids).await;
                });
            }
            "resume_all" => {
                let ids = crate::model::with(|s| s.items.iter().filter(|i| i.paused && !i.is_finished()).map(|i| i.id).collect());
                crate::engine::resume(ids);
            }
            "alt" => {
                let s = crate::settings::update(|s| s.alt_enabled = !s.alt_enabled);
                set_alt(s.alt_enabled);
                crate::engine::emit("settings", s);
            }
            "mini" => crate::toggle_mini(app),
            "quit" => crate::quit(app.clone()),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                let _ = tray.app_handle();
                crate::show_main_window();
            }
        })
        .build(app)?;
    let _ = TRAY.set(tray);
    let _ = app.get_webview_window("main");
    Ok(())
}

pub fn set_alt(on: bool) {
    if let Some(a) = ALT.get() {
        let _ = a.set_checked(on);
    }
}

fn speed(b: u64) -> String {
    let kb = b as f64 / 1024.0;
    if kb >= 1024.0 { format!("{:.1} MB/s", kb / 1024.0) } else { format!("{kb:.0} KB/s") }
}

pub fn update_tooltip(t: &crate::engine::Totals) {
    if let Some(tray) = TRAY.get() {
        let text = if t.downloading == 0 && t.seeding == 0 {
            "Haul".to_string()
        } else {
            format!("Haul\n↓ {}  ↑ {}\n{} downloading · {} seeding", speed(t.down), speed(t.up), t.downloading, t.seeding)
        };
        let _ = tray.set_tooltip(Some(text));
    }
}
