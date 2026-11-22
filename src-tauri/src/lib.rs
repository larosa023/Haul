mod assoc;
mod bridge;
mod clipboard;
mod commands;
mod debrid;
mod secret;
mod engine;
mod extract;
mod guard;
mod http;
mod limiter;
mod model;
mod netif;
mod notify;
mod paths;
mod settings;
mod sorter;
mod torrent;
mod tray;
mod updater;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// `Haul.exe --guard <plan.json>`: the update guardian (see guard.rs). No window, no Tauri.
pub fn guard_main(plan: &str) -> i32 {
    guard::run(std::path::Path::new(plan))
}

pub fn show_main_window() {
    if let Some(app) = engine::app() {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}

/// Is someone looking at Haul right now? (Updates wait until it's hidden or minimized.)
pub fn window_in_use() -> bool {
    let Some(app) = engine::app() else { return false };
    let Some(w) = app.get_webview_window("main") else { return false };
    w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)
}

pub fn save_window_state(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    let maximized = w.is_maximized().unwrap_or(false);
    let minimized = w.is_minimized().unwrap_or(false);
    if minimized {
        return;
    }
    let scale = w.scale_factor().unwrap_or(1.0);
    let pos = w.outer_position().ok();
    let size = w.inner_size().ok();
    settings::update(|s| {
        s.window.maximized = maximized;
        if !maximized {
            if let (Some(p), Some(sz)) = (pos, size) {
                s.window.x = p.x;
                s.window.y = p.y;
                s.window.width = (sz.width as f64 / scale) as u32;
                s.window.height = (sz.height as f64 / scale) as u32;
                s.window.saved = true;
            }
        }
    });
}

pub fn quit(app: AppHandle) {
    save_window_state(&app);
    tauri::async_runtime::spawn(async move {
        engine::shutdown().await;
        app.exit(0);
    });
}

/// Mica / Acrylic behind the window, matching the theme (Windows 11).
#[tauri::command]
fn set_backdrop(app: AppHandle, kind: String, dark: bool) {
    let Some(w) = app.get_webview_window("main") else { return };
    let _ = w.set_theme(Some(if dark { tauri::Theme::Dark } else { tauri::Theme::Light }));
    #[cfg(windows)]
    {
        use tauri::window::{Effect, EffectsBuilder};
        let effect = match kind.as_str() {
            "mica" => Some(Effect::Mica),
            "acrylic" => Some(Effect::Acrylic),
            _ => None,
        };
        let _ = match effect {
            Some(e) => w.set_effects(EffectsBuilder::new().effect(e).build()),
            None => w.set_effects(None),
        };
    }
    #[cfg(not(windows))]
    let _ = kind;
}

#[tauri::command]
fn set_native_frame(app: AppHandle, native: bool) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_decorations(native);
    }
}

pub fn run() {
    let p = paths::get();
    paths::log(format!("Haul {} starting{}", env!("CARGO_PKG_VERSION"), if p.portable { " (portable)" } else { "" }));
    let args: Vec<String> = std::env::args().collect();
    let start_hidden = args.iter().any(|a| a == "--minimized" || a == "--hidden") || settings::with(|s| s.start_minimized);

    tauri::Builder::default()
        // A second launch (e.g. clicking a magnet link) hands its arguments to the running Haul.
        .plugin(tauri_plugin_single_instance::init(|_app, argv, _cwd| {
            if argv.iter().skip(1).all(|a| a.starts_with('-')) {
                show_main_window();
            }
            commands::handle_args(argv);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            notify::register();
            let s = settings::get();
            if s.register_handlers {
                // First run takes over magnet / .torrent; later runs only keep Haul's own
                // entries pointing at this exe (in case it moved).
                let _ = assoc::register(!s.initialized);
            }
            if !s.initialized {
                settings::update(|s| s.initialized = true);
            }
            if s.launch_at_startup {
                let _ = assoc::set_startup(true);
            }
            guard::cleanup();

            // The window is made here (not in tauri.conf.json) so portable mode can keep the
            // WebView's data next to the exe.
            let w = &s.window;
            let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Haul")
                .min_inner_size(760.0, 460.0)
                .inner_size(if w.saved { w.width.max(760) as f64 } else { 1180.0 }, if w.saved { w.height.max(460) as f64 } else { 740.0 })
                .decorations(s.native_title_bar)
                .transparent(true)
                .shadow(true)
                .visible(false);
            if w.saved {
                b = b.position(w.x as f64, w.y as f64);
            } else {
                b = b.center();
            }
            if p.portable {
                b = b.data_directory(p.webview());
            }
            let win = b.build()?;
            if w.saved && w.maximized {
                let _ = win.maximize();
            }
            let h2 = handle.clone();
            win.on_window_event(move |e| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                    save_window_state(&h2);
                    if settings::with(|s| s.close_to_tray) {
                        api.prevent_close();
                        if let Some(w) = h2.get_webview_window("main") {
                            let _ = w.hide();
                        }
                    } else {
                        api.prevent_close();
                        quit(h2.clone());
                    }
                }
            });
            // Shown by the UI once it has drawn (no white flash); this is a fallback.
            if !start_hidden {
                let h3 = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    if let Some(w) = h3.get_webview_window("main") {
                        if !w.is_visible().unwrap_or(true) && !START_HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
                            let _ = w.show();
                        }
                    }
                });
            }
            START_HIDDEN.store(start_hidden, std::sync::atomic::Ordering::Relaxed);

            tray::create(&handle)?;
            engine::init(handle.clone());
            tauri::async_runtime::spawn(bridge::serve());
            tauri::async_runtime::spawn(clipboard::watch());
            tauri::async_runtime::spawn(updater::background());
            commands::handle_args(std::env::args().collect());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::set_settings,
            commands::reset_rules,
            commands::inspect_url,
            commands::add_http,
            commands::debrid_connect,
            commands::debrid_disconnect,
            commands::debrid_status,
            commands::add_torrent,
            commands::confirm_torrent,
            commands::cancel_torrent,
            commands::pending_torrents,
            commands::pause,
            commands::resume,
            commands::remove,
            commands::retry,
            commands::move_items,
            commands::get_order,
            commands::set_options,
            commands::set_file_priorities,
            commands::get_files,
            commands::get_item,
            commands::open_item,
            commands::reveal,
            commands::open_folder,
            commands::adapters,
            commands::open_link,
            commands::handler_state,
            commands::make_default,
            commands::check_update,
            commands::apply_update,
            commands::update_state,
            commands::list_themes,
            commands::save_theme,
            commands::open_themes_folder,
            commands::system_theme,
            commands::open_extension_folder,
            commands::open_data_folder,
            commands::engine_status,
            commands::restart_torrents,
            commands::app_ready,
            commands::quit_app,
            commands::save_window,
            commands::show_window,
            ui_ready,
            set_backdrop,
            set_native_frame,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Haul")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
                // Closing the last window keeps Haul in the tray; only Quit really exits.
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            if let tauri::RunEvent::Exit = event {
                model::save_if_dirty();
            }
        });
}

static START_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The UI finished its first paint: show the window (unless starting in the tray).
#[tauri::command]
fn ui_ready(app: AppHandle) {
    commands::app_ready();
    if !START_HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}
