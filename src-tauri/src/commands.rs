//! Everything the UI can ask the backend to do.

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::engine::{self, HttpAdd, ItemOptions, MediaAdd, TorrentAdd, TorrentConfirm};
use crate::settings::Settings;
use crate::{assoc, bridge, debrid, grab, guard, hf, http, media, model, netif, paths, secret, settings, tools, torrent, updater};

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

/// The extension files, built into Haul so portable copies have them too. The Firefox copy
/// uses its own manifest; everything else is shared.
const EXTENSION_FILES: &[(&str, &[u8])] = &[
    ("background.js", include_bytes!("../../extension/background.js")),
    ("alt-click.js", include_bytes!("../../extension/alt-click.js")),
    ("video-button.js", include_bytes!("../../extension/video-button.js")),
    ("popup.html", include_bytes!("../../extension/popup.html")),
    ("popup.js", include_bytes!("../../extension/popup.js")),
    ("icons/16.png", include_bytes!("../../extension/icons/16.png")),
    ("icons/32.png", include_bytes!("../../extension/icons/32.png")),
    ("icons/48.png", include_bytes!("../../extension/icons/48.png")),
    ("icons/128.png", include_bytes!("../../extension/icons/128.png")),
];
const MANIFEST_CHROMIUM: &[u8] = include_bytes!("../../extension/manifest.json");
const MANIFEST_FIREFOX: &[u8] = include_bytes!("../../extension/manifest.firefox.json");

fn unpack(dir: &std::path::Path, manifest: &[u8]) {
    let files = EXTENSION_FILES.iter().copied().chain(std::iter::once(("manifest.json", manifest)));
    for (name, bytes) in files {
        let f = dir.join(name);
        if std::fs::read(&f).map(|b| b != bytes).unwrap_or(true) {
            if let Some(parent) = f.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&f, bytes);
        }
    }
}

fn extension_dir() -> std::path::PathBuf {
    // Installed copies ship the extension next to Haul.exe, updated with each release; keep
    // using it so an extension loaded from there before keeps working.
    let shipped = paths::get().exe_dir.join("extension");
    if shipped.join("manifest.json").is_file() {
        return shipped;
    }
    let dir = paths::get().data.join("extension");
    unpack(&dir, MANIFEST_CHROMIUM);
    dir
}

fn extension_dir_firefox() -> std::path::PathBuf {
    let dir = paths::get().data.join("extension-firefox");
    unpack(&dir, MANIFEST_FIREFOX);
    dir
}

/// Saves settings and applies whatever changed.
#[tauri::command]
pub async fn set_settings(mut new: Settings) -> R<Settings> {
    let old = settings::get();
    // The API key only changes through debrid_connect / debrid_disconnect.
    new.debrid_key = old.debrid_key.clone();
    new.debrid_keys = old.debrid_keys.clone();
    new.search_indexers = old.search_indexers.clone();
    new.hf_token = old.hf_token.clone();
    new.vt_key = old.vt_key.clone();
    new.schema = old.schema;
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
    if old.mini_hotkey != saved.mini_hotkey {
        if let Err(e) = crate::register_hotkey(&saved.mini_hotkey) {
            engine::toast(format!("That shortcut can't be used: {e}"), "error");
        }
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
    // Hoster links go through the debrid service, so show what you'll actually get.
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
    let mut info = http::inspect(http::Request { url: url.clone(), referrer: referrer.unwrap_or_default(), cookies: cookies.unwrap_or_default(), ..Default::default() }).await?;
    info.safety = Some(crate::safety::link_report(&url, &info.final_url, &info.name, &info.content_type, info.size));
    Ok(info)
}

/// Checks a finished download again (safety score).
#[tauri::command]
pub async fn rescan(id: u64) -> R<crate::safety::Report> {
    tauri::async_runtime::spawn_blocking(move || engine::scan_item(id).ok_or_else(|| "The file isn't there any more".to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Saves (or with an empty key, removes) the VirusTotal API key, after checking it works.
#[tauri::command]
pub async fn set_vt_key(key: String) -> R<Settings> {
    let key = key.trim().to_string();
    if !key.is_empty() {
        let r = http::client()
            .get("https://www.virustotal.com/api/v3/users/me")
            .header("x-apikey", &key)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|_| "Couldn't reach VirusTotal".to_string())?;
        if r.status().as_u16() == 401 || r.status().as_u16() == 403 {
            return Err("VirusTotal didn't accept that API key".into());
        }
    }
    let s = settings::update(|s| s.vt_key = if key.is_empty() { String::new() } else { secret::seal(&key) });
    engine::emit("settings", s.clone());
    Ok(s)
}

#[tauri::command]
pub async fn add_http(mut add: HttpAdd) -> R<u64> {
    add.debrid = debrid::should_unrestrict(add.url.trim()).await;
    engine::add_http(add)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebridState {
    service: String,
    /// Services that have a key saved.
    saved: Vec<String>,
    connected: bool,
    account: Option<debrid::Account>,
    error: String,
}

fn debrid_state(connected: bool, account: Option<debrid::Account>, error: String) -> DebridState {
    let s = settings::get();
    DebridState { service: s.debrid_service.clone(), saved: s.debrid_keys.keys().cloned().collect(), connected, account, error }
}

/// Checks an API key with the chosen service and, if it works, saves it (encrypted) and makes
/// that service the one Haul uses.
#[tauri::command]
pub async fn debrid_connect(key: String, service: Option<String>) -> R<DebridState> {
    let key = key.trim().to_string();
    let svc = debrid::Service::from_id(&service.unwrap_or_else(|| settings::with(|s| s.debrid_service.clone())));
    let account = debrid::account_for(svc, &key).await?;
    settings::update(|s| {
        s.debrid_keys.insert(svc.id().into(), secret::seal(&key));
        s.debrid_service = svc.id().into();
        s.debrid_enabled = true;
    });
    engine::emit("settings", settings::get());
    Ok(debrid_state(true, Some(account), String::new()))
}

/// Switches to another service whose key is already saved.
#[tauri::command]
pub async fn debrid_use(service: String) -> DebridState {
    let svc = debrid::Service::from_id(&service);
    settings::update(|s| s.debrid_service = svc.id().into());
    engine::emit("settings", settings::get());
    debrid_status().await
}

#[tauri::command]
pub fn debrid_disconnect(service: Option<String>) -> Settings {
    let s = settings::update(|s| {
        let id = service.clone().unwrap_or_else(|| s.debrid_service.clone());
        s.debrid_keys.remove(&id);
    });
    engine::emit("settings", s.clone());
    s
}

#[tauri::command]
pub async fn debrid_status() -> DebridState {
    if !settings::with(|s| s.debrid_keys.contains_key(&s.debrid_service)) {
        return debrid_state(false, None, String::new());
    }
    if debrid::key().is_empty() {
        return debrid_state(false, None, "The saved key can't be read on this PC. Enter it again.".into());
    }
    match debrid::account().await {
        Ok(a) => debrid_state(true, Some(a), String::new()),
        Err(e) => debrid_state(true, None, e),
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
pub fn open_extension_folder(firefox: Option<bool>) {
    if firefox.unwrap_or(false) {
        open_path(&extension_dir_firefox());
        return;
    }
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

// ---------------------------------------------------------------- video & music

#[tauri::command]
pub async fn media_inspect(url: String) -> R<media::MediaInfo> {
    media::inspect(url.trim()).await
}

#[tauri::command]
pub fn add_media(items: Vec<MediaAdd>) -> R<Vec<u64>> {
    let mut ids = Vec::new();
    for it in items {
        ids.push(engine::add_media(it)?);
    }
    Ok(ids)
}

#[tauri::command]
pub fn tools_state() -> tools::ToolsState {
    tools::state()
}

/// Installs anything missing, then checks yt-dlp for a newer version.
#[tauri::command]
pub async fn tools_update() -> R<tools::ToolsState> {
    tools::ensure(true).await?;
    tools::update_ytdlp(true).await?;
    Ok(tools::state())
}

#[tauri::command]
pub fn tools_remove() -> tools::ToolsState {
    tools::remove();
    tools::state()
}

// ---------------------------------------------------------------- grabber, Hugging Face

#[tauri::command]
pub async fn grab_links(url: String) -> R<Vec<grab::Link>> {
    grab::grab(&url).await
}

#[tauri::command]
pub async fn hf_list(input: String) -> R<hf::Listing> {
    hf::list(&input).await
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HfAdd {
    repo: hf::Repo,
    files: Vec<hf::HfFile>,
    /// "" = Downloads\Hugging Face\org\name
    folder: String,
}

#[tauri::command]
pub fn hf_add(add: HfAdd) -> R<Vec<u64>> {
    let base = if add.folder.trim().is_empty() {
        let dl = std::path::PathBuf::from(paths::expand_env(&settings::with(|s| s.download_dir.clone())));
        add.repo.id.split('/').fold(dl.join("Hugging Face"), |p, part| p.join(model::sanitize(part)))
    } else {
        std::path::PathBuf::from(paths::expand_env(add.folder.trim()))
    };
    let auth = !hf::token().is_empty();
    let mut ids = Vec::new();
    for f in add.files {
        let mut parts: Vec<String> = f.path.split('/').filter(|p| !p.is_empty()).map(model::sanitize).collect();
        let name = parts.pop().unwrap_or_else(|| "file".into());
        let dir = parts.iter().fold(base.clone(), |p, c| p.join(c));
        let id = engine::add_http(HttpAdd {
            url: hf::resolve_url(&add.repo, &f.path),
            name,
            folder: dir.to_string_lossy().to_string(),
            category: "Hugging Face".into(),
            sha256: f.sha256.clone(),
            git_sha1: f.git_sha1.clone(),
            hf_auth: auth,
            ..Default::default()
        })?;
        ids.push(id);
    }
    Ok(ids)
}

#[tauri::command]
pub async fn hf_connect(token: String) -> R<String> {
    let name = hf::whoami(token.trim()).await?;
    let sealed = secret::seal(token.trim());
    let s = settings::update(|s| s.hf_token = sealed);
    engine::emit("settings", s);
    Ok(name)
}

#[tauri::command]
pub fn hf_disconnect() -> Settings {
    let s = settings::update(|s| s.hf_token.clear());
    engine::emit("settings", s.clone());
    s
}

#[tauri::command]
pub async fn hf_status() -> R<String> {
    let t = hf::token();
    if t.is_empty() {
        return Ok(String::new());
    }
    hf::whoami(&t).await
}

/// A download handed to Haul from the browser extension, the clipboard or the command line.
pub async fn incoming(req: bridge::AddRequest) -> Result<(), String> {
    let url = req.url.trim().to_string();
    // Video pages ("Download video with Haul") and HLS / DASH streams go to yt-dlp.
    if req.media || media::is_stream(&url) {
        if settings::with(|s| s.show_add_dialog) {
            engine::emit("media-request", &req);
            crate::show_main_window();
        } else {
            let options = settings::with(|s| s.media_defaults.clone());
            let id = engine::add_media(MediaAdd { url, options, referrer: req.referrer.clone(), ..Default::default() })?;
            engine::toast("Downloading video", "info");
            engine::emit("select", id);
        }
        return Ok(());
    }
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
    // Already downloaded (or in the list)? Show the Add window so the warning is seen.
    if settings::with(|s| s.show_add_dialog) || engine::duplicate_of(&url, &req.filename).is_some() {
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
pub fn toggle_mini(app: AppHandle) {
    crate::toggle_mini(&app);
}

#[tauri::command]
pub fn show_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

// ---------------------------------------------------------------- torrent search

#[tauri::command]
pub async fn search_torrents(query: String, category: Option<String>) -> crate::search::Results {
    crate::search::search(&query, category.as_deref().unwrap_or("")).await
}

/// Adds a search result: a magnet, or a .torrent fetched from the indexer.
#[tauri::command]
pub async fn search_add(link: String, debrid: Option<bool>) -> R<engine::AddResult> {
    let add = match crate::search::fetch(&link).await? {
        Ok(magnet) => TorrentAdd { source: magnet, debrid, ..Default::default() },
        Err(bytes) => TorrentAdd { source: link, bytes: Some(bytes), debrid, ..Default::default() },
    };
    add_torrent(add).await
}

/// Checks a Torznab indexer and saves it (its key encrypted).
#[tauri::command]
pub async fn search_add_indexer(indexer: crate::search::IndexerInput) -> R<Settings> {
    let mut ix = settings::Indexer { name: indexer.name.trim().to_string(), url: indexer.url.trim().to_string(), key: String::new(), enabled: true };
    if !indexer.key.trim().is_empty() {
        ix.key = secret::seal(indexer.key.trim());
    }
    if !ix.url.starts_with("http") {
        return Err("The URL should start with http:// or https://".into());
    }
    let title = crate::search::test(&ix).await?;
    if ix.name.is_empty() {
        ix.name = title;
    }
    let s = settings::update(|s| {
        s.search_indexers.retain(|i| i.url != ix.url);
        s.search_indexers.push(ix.clone());
    });
    engine::emit("settings", s.clone());
    Ok(s)
}

#[tauri::command]
pub fn search_remove_indexer(url: String) -> Settings {
    let s = settings::update(|s| s.search_indexers.retain(|i| i.url != url));
    engine::emit("settings", s.clone());
    s
}

#[tauri::command]
pub fn search_toggle_indexer(url: String, enabled: bool) -> Settings {
    let s = settings::update(|s| {
        for i in s.search_indexers.iter_mut().filter(|i| i.url == url) {
            i.enabled = enabled;
        }
    });
    engine::emit("settings", s.clone());
    s
}

#[tauri::command]
pub async fn prowlarr_status() -> crate::prowlarr::Status {
    crate::prowlarr::status().await
}

/// Opens Prowlarr's own page (address from its config, always on this PC).
#[tauri::command]
pub async fn prowlarr_open() -> R<()> {
    let c = crate::prowlarr::ensure().await.or_else(crate::prowlarr::conn).ok_or("Prowlarr isn't installed")?;
    tauri_plugin_opener::open_url(c.base.replace("127.0.0.1", "localhost"), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jackett_status() -> crate::jackett::Status {
    crate::jackett::status().await
}

#[tauri::command]
pub async fn jackett_open() -> R<()> {
    crate::jackett::open().await
}

#[tauri::command]
pub async fn prowlarr_start() -> crate::prowlarr::Status {
    crate::prowlarr::start().await
}

#[tauri::command]
pub async fn flaresolverr_status() -> crate::flaresolverr::Status {
    crate::flaresolverr::status().await
}
