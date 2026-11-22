//! Everything the UI can ask the backend to do.

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::engine::{self, HttpAdd, ItemOptions, TorrentAdd, TorrentConfirm};
use crate::settings::Settings;
use crate::{assoc, bridge, debrid, guard, http, model, netif, paths, secret, settings, torrent, updater};

type R<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    items: Vec<serde_json::Value>,
    settings: Settings,
    totals: engine::Totals,
    update: updater::UpdateState,
    version: String,
    portable: bool,
    data_dir: String,
    extension_dir: String,
}

#[tauri::command]
pub fn get_state() -> AppState {
    let p = paths::get();
    AppState {
        items: model::with(|s| s.items.iter().map(|i| i.view()).collect()),
        settings: settings::get(),
        totals: engine::totals(),
        update: updater::state(),
        version: env!("CARGO_PKG_VERSION").into(),
        portable: p.portable,
        data_dir: p.data.to_string_lossy().into(),
        extension_dir: extension_dir().to_string_lossy().into(),
    }
}

/// The extension files, built into Haul so portable copies have them too.
const EXTENSION_FILES: &[(&str, &[u8])] = &[
    ("manifest.json", include_bytes!("../../extension/manifest.json")),
    ("background.js", include_bytes!("../../extension/background.js")),
    ("alt-click.js", include_bytes!("../../extension/alt-click.js")),
    ("popup.html", include_bytes!("../../extension/popup.html")),
    ("popup.js", include_bytes!("../../extension/popup.js")),
    ("icons/16.png", include_bytes!("../../extension/icons/16.png")),
    ("icons/32.png", include_bytes!("../../extension/icons/32.png")),
    ("icons/48.png", include_bytes!("../../extension/icons/48.png")),
    ("icons/128.png", include_bytes!("../../extension/icons/128.png")),
];

fn extension_dir() -> std::path::PathBuf {
    let p = paths::get();
    let shipped = p.exe_dir.join("extension");
    if shipped.join("manifest.json").is_file() {
        return shipped;
    }
    // Portable (or a manual copy): unpack the built-in copy next to Haul's data.
    let dir = p.data.join("extension");
    for (name, bytes) in EXTENSION_FILES {
        let f = dir.join(name);
        if std::fs::read(&f).map(|b| b != *bytes).unwrap_or(true) {
            if let Some(parent) = f.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&f, bytes);
        }
    }
    dir
}

/// Saves settings and applies whatever changed.
#[tauri::command]
pub async fn set_settings(mut new: Settings) -> R<Settings> {
    let old = settings::get();
    // The API key only changes through debrid_connect / debrid_disconnect.
    new.debrid_key = old.debrid_key.clone();
    let saved = settings::replace(new);
    if old.launch_at_startup != saved.launch_at_startup {
        assoc::set_startup(saved.launch_at_startup).map_err(|e| e.to_string())?;
    }
    if old.register_handlers != saved.register_handlers {
        if saved.register_handlers {
            assoc::register(true).map_err(|e| e.to_string())?;
        } else {
            assoc::unregister();
        }
    }
    if old.encryption != saved.encryption {
        torrent::apply_encryption(&saved.encryption);
    }
    crate::tray::set_alt(saved.alt_enabled);
    let restart = old.listen_port != saved.listen_port
        || old.dht != saved.dht
        || old.upnp != saved.upnp
        || old.natpmp != saved.natpmp
        || old.bind_adapter != saved.bind_adapter
        || old.blocklist_enabled != saved.blocklist_enabled
        || (saved.blocklist_enabled && old.blocklist != saved.blocklist);
    if restart {
        tauri::async_runtime::spawn(async {
            if let Err(e) = torrent::start().await {
                engine::toast(format!("Torrents couldn't start: {e:#}"), "error");
            }
        });
    }
    if old.channel != saved.channel {
        tauri::async_runtime::spawn(updater::check(false));
    }
    Ok(saved)
}

#[tauri::command]
pub fn reset_rules() -> Settings {
    settings::update(|s| s.rules = settings::default_rules())
}

#[tauri::command]
pub async fn inspect_url(url: String, referrer: Option<String>, cookies: Option<String>) -> R<http::ProbeInfo> {
    // Hoster links go through Real-Debrid, so show what you'll actually get.
    if debrid::should_unrestrict(&url).await {
        let u = debrid::unrestrict(&url, "").await?;
        let mut info = http::inspect(http::Request { url: u.download, ..Default::default() }).await?;
        if !u.filename.is_empty() {
            info.name = model::sanitize(&u.filename);
            let s = settings::get();
            info.category = crate::sorter::categorize(&s.rules, &info.name, info.size, &url);
            info.folder = crate::sorter::folder_for(&s, &info.category).to_string_lossy().to_string();
        }
        if info.size == 0 {
            info.size = u.filesize;
        }
        info.debrid = true;
        return Ok(info);
    }
    http::inspect(http::Request { url, referrer: referrer.unwrap_or_default(), cookies: cookies.unwrap_or_default(), user_agent: String::new() }).await
}

#[tauri::command]
pub async fn add_http(mut add: HttpAdd) -> R<u64> {
    add.debrid = debrid::should_unrestrict(add.url.trim()).await;
    engine::add_http(add)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebridState {
    connected: bool,
    account: Option<debrid::Account>,
    error: String,
}

/// Checks an API key with Real-Debrid and, if it works, saves it (encrypted).
#[tauri::command]
pub async fn debrid_connect(key: String) -> R<DebridState> {
    let key = key.trim().to_string();
    let account = debrid::account_for(&key).await?;
    settings::update(|s| {
        s.debrid_key = secret::seal(&key);
        s.debrid_enabled = true;
    });
    engine::emit("settings", settings::get());
    Ok(DebridState { connected: true, account: Some(account), error: String::new() })
}

#[tauri::command]
pub fn debrid_disconnect() -> Settings {
    let s = settings::update(|s| s.debrid_key.clear());
    engine::emit("settings", s.clone());
    s
}

#[tauri::command]
pub async fn debrid_status() -> DebridState {
    if settings::with(|s| s.debrid_key.is_empty()) {
        return DebridState { connected: false, account: None, error: String::new() };
    }
    if debrid::key().is_empty() {
        return DebridState { connected: false, account: None, error: "The saved key can't be read on this PC. Enter it again.".into() };
    }
    match debrid::account().await {
        Ok(a) => DebridState { connected: true, account: Some(a), error: String::new() },
        Err(e) => DebridState { connected: true, account: None, error: e },
    }
}

async fn fetch_torrent_file(url: &str) -> R<Vec<u8>> {
    let r = http::client().get(url).send().await.map_err(|e| format!("Couldn't download the .torrent: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("Couldn't download the .torrent ({})", r.status()));
    }
    let b = r.bytes().await.map_err(|e| e.to_string())?;
    if b.len() > 20 * 1024 * 1024 {
        return Err("That .torrent file is too big".into());
    }
    Ok(b.to_vec())
}

fn is_torrent_url(u: &str) -> bool {
    url::Url::parse(u)
        .ok()
        .map(|p| matches!(p.scheme(), "http" | "https") && p.path().to_ascii_lowercase().ends_with(".torrent"))
        .unwrap_or(false)
}

#[tauri::command]
pub async fn add_torrent(mut add: TorrentAdd) -> R<engine::AddResult> {
    if add.bytes.is_none() && is_torrent_url(add.source.trim()) {
        add.bytes = Some(fetch_torrent_file(add.source.trim()).await?);
    }
    let r = engine::add_torrent(add)?;
    if r.existing {
        engine::toast("That torrent is already in Haul", "info");
        engine::emit("select", r.id);
    }
    Ok(r)
}

#[tauri::command]
pub async fn confirm_torrent(confirm: TorrentConfirm) -> R<()> {
    engine::confirm_torrent(confirm).await
}

#[tauri::command]
pub fn pending_torrents() -> Vec<engine::TorrentMetaView> {
    engine::pending_meta()
}

#[tauri::command]
pub async fn cancel_torrent(id: u64) {
    engine::cancel_torrent(id).await
}

#[tauri::command]
pub async fn pause(ids: Vec<u64>) {
    engine::pause(ids).await
}

#[tauri::command]
pub fn resume(ids: Vec<u64>) {
    engine::resume(ids)
}

#[tauri::command]
pub async fn remove(ids: Vec<u64>, delete_files: bool) {
    engine::remove(ids.clone(), delete_files).await;
    engine::emit("removed", ids);
}

#[tauri::command]
pub fn retry(ids: Vec<u64>) {
    engine::retry(ids)
}

#[tauri::command]
pub fn move_items(ids: Vec<u64>, dir: String) -> Vec<u64> {
    engine::move_items(ids, &dir);
    engine::order()
}

#[tauri::command]
pub fn get_order() -> Vec<u64> {
    engine::order()
}

#[tauri::command]
pub fn set_options(ids: Vec<u64>, options: ItemOptions) {
    engine::set_options(ids, options)
}

#[tauri::command]
pub async fn set_file_priorities(id: u64, priorities: Vec<u8>) -> R<()> {
    engine::set_file_priorities(id, priorities).await
}

#[tauri::command]
pub fn get_files(id: u64) -> Vec<model::TorrentFile> {
    engine::files_of(id)
}

#[tauri::command]
pub fn get_item(id: u64) -> Option<serde_json::Value> {
    model::with(|s| {
        s.get(id).map(|i| {
            let mut v = i.view();
            v["source"] = serde_json::Value::String(i.source.clone());
            v["finalUrl"] = serde_json::Value::String(i.final_url.clone());
            v["referrer"] = serde_json::Value::String(i.referrer.clone());
            v["segmentList"] = serde_json::to_value(&i.segments).unwrap_or_default();
            v
        })
    })
}

pub fn open_item_path(id: u64) {
    if let Some(p) = engine::item_path(id) {
        open_path(&p);
    }
}

pub fn reveal_item(id: u64) {
    if let Some(p) = engine::item_path(id) {
        reveal_path(&p);
    }
}

fn open_path(p: &std::path::Path) {
    let _ = tauri_plugin_opener::open_path(p.to_string_lossy().to_string(), None::<&str>);
}

fn reveal_path(p: &std::path::Path) {
    if p.exists() {
        let _ = tauri_plugin_opener::reveal_item_in_dir(p);
    } else if let Some(parent) = p.parent().filter(|d| d.exists()) {
        open_path(parent);
    }
}

#[tauri::command]
pub fn open_item(id: u64) {
    open_item_path(id)
}

#[tauri::command]
pub fn reveal(id: u64) {
    reveal_item(id)
}

#[tauri::command]
pub fn open_folder(path: String) {
    let p = std::path::PathBuf::from(paths::expand_env(&path));
    let _ = std::fs::create_dir_all(&p);
    open_path(&p);
}

/// Opens a web page in the default browser (https only).
#[tauri::command]
pub fn open_link(url: String) -> R<()> {
    if !url.starts_with("https://") {
        return Err("Only https links can be opened".into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn adapters() -> Vec<netif::Adapter> {
    netif::adapters()
}

#[tauri::command]
pub fn handler_state() -> assoc::HandlerState {
    assoc::state()
}

#[tauri::command]
pub fn make_default() -> R<()> {
    assoc::register(true).map_err(|e| e.to_string())?;
    settings::update(|s| s.register_handlers = true);
    // Windows only lets the user pick defaults; this opens Haul's page in Default Apps.
    let _ = tauri_plugin_opener::open_url("ms-settings:defaultapps?registeredAppUser=Haul", None::<&str>);
    Ok(())
}

#[tauri::command]
pub async fn check_update() -> updater::UpdateState {
    updater::check(true).await;
    updater::state()
}

#[tauri::command]
pub async fn apply_update(force: bool) -> R<()> {
    updater::apply(force).await
}

#[tauri::command]
pub fn update_state() -> updater::UpdateState {
    updater::state()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeFile {
    file: String,
    json: String,
}

#[tauri::command]
pub fn list_themes() -> Vec<ThemeFile> {
    let p = paths::get();
    let mut out = Vec::new();
    let mut dirs = vec![p.themes()];
    if let Some(suite) = p.suite_themes() {
        dirs.push(suite);
    }
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().map(|x| x.eq_ignore_ascii_case("json")).unwrap_or(false) {
                if let Ok(json) = std::fs::read_to_string(&path) {
                    if json.len() < 64 * 1024 {
                        out.push(ThemeFile { file: path.to_string_lossy().into(), json });
                    }
                }
            }
        }
    }
    out
}

#[tauri::command]
pub fn save_theme(name: String, json: String) -> R<String> {
    let dir = paths::get().themes();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{}.json", model::sanitize(&name)));
    std::fs::write(&file, json).map_err(|e| e.to_string())?;
    Ok(file.to_string_lossy().into())
}

#[tauri::command]
pub fn open_themes_folder() {
    let d = paths::get().themes();
    let _ = std::fs::create_dir_all(&d);
    open_path(&d);
}

#[tauri::command]
pub fn system_theme() -> serde_json::Value {
    serde_json::json!({ "accent": assoc::accent_color(), "light": assoc::apps_use_light_theme(), "backdrop": assoc::supports_backdrop() })
}

#[tauri::command]
pub fn open_extension_folder() {
    open_path(&extension_dir());
}

#[tauri::command]
pub fn open_data_folder() {
    open_path(&paths::get().data);
}

#[tauri::command]
pub fn engine_status() -> torrent::EngineStatus {
    torrent::status()
}

#[tauri::command]
pub async fn restart_torrents() -> R<()> {
    torrent::start().await.map_err(|e| format!("{e:#}"))
}

/// The UI has drawn: confirm a fresh update is healthy and say what happened.
#[tauri::command]
pub fn app_ready() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--after-update") {
        if let Some(v) = args.get(i + 1) {
            guard::report_healthy(v);
            engine::toast(format!("Haul was updated to {}", env!("CARGO_PKG_VERSION")), "success");
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--rolled-back") {
        if let Some(v) = args.get(i + 1) {
            engine::toast(format!("Haul {v} didn't start properly, so the previous version was restored"), "error");
        }
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::quit(app)
}

#[tauri::command]
pub fn save_window(app: AppHandle) {
    crate::save_window_state(&app);
}

/// A download handed to Haul from the browser extension, the clipboard or the command line.
pub async fn incoming(req: bridge::AddRequest) -> Result<(), String> {
    let url = req.url.trim().to_string();
    if url.starts_with("magnet:") || is_torrent_url(&url) || url.to_ascii_lowercase().ends_with(".torrent") {
        // "Ask each time": the Add window lets you pick Real-Debrid or the torrent client.
        if asks_debrid() {
            engine::emit("add-request", &req);
            crate::show_main_window();
            return Ok(());
        }
        add_torrent(TorrentAdd { source: url, ..Default::default() }).await?;
        crate::show_main_window();
        return Ok(());
    }
    if settings::with(|s| s.show_add_dialog) {
        engine::emit("add-request", &req);
        crate::show_main_window();
        Ok(())
    } else {
        let id = add_http(HttpAdd {
            url,
            name: req.filename.clone(),
            referrer: req.referrer.clone(),
            cookies: req.cookies.clone(),
            user_agent: req.user_agent.clone(),
            ..Default::default()
        })
        .await?;
        engine::toast(format!("Downloading {}", if req.filename.is_empty() { "file" } else { &req.filename }), "info");
        engine::emit("select", id);
        Ok(())
    }
}

/// Should new torrents open the Add window to choose Real-Debrid or the torrent client?
fn asks_debrid() -> bool {
    debrid::active() && settings::with(|s| s.debrid_torrents == "ask" && s.show_add_dialog)
}

/// Handles command-line arguments (first launch, or forwarded from a second launch).
pub fn handle_args(args: Vec<String>) {
    let mut iter = args.into_iter().skip(1).peekable();
    let mut show = false;
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--minimized" | "--hidden" => {}
            "--after-update" | "--rolled-back" | "--guard" => {
                iter.next();
            }
            "--quit" => {
                if let Some(app) = engine::app() {
                    crate::quit(app.clone());
                }
                return;
            }
            "--show" => show = true,
            other if other.starts_with('-') => {}
            other => {
                let other = other.to_string();
                show = true;
                tauri::async_runtime::spawn(async move {
                    let res = if (other.starts_with("magnet:") || other.to_ascii_lowercase().ends_with(".torrent")) && asks_debrid() {
                        incoming(bridge::AddRequest { url: other, ..Default::default() }).await
                    } else if other.starts_with("magnet:") || other.to_ascii_lowercase().ends_with(".torrent") {
                        add_torrent(TorrentAdd { source: other, ..Default::default() }).await.map(|_| ())
                    } else if other.starts_with("http://") || other.starts_with("https://") {
                        incoming(bridge::AddRequest { url: other, ..Default::default() }).await
                    } else {
                        Ok(())
                    };
                    if let Err(e) = res {
                        engine::toast(e, "error");
                    }
                });
            }
        }
    }
    if show {
        crate::show_main_window();
    }
}

#[tauri::command]
pub fn show_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
