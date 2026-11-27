//! settings.json. Every field has a default, so older files keep working as settings are added.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::paths;

pub const CATEGORIES: &[&str] = &["Video", "Music", "Games", "Software", "Archives", "Documents", "Images", "Other"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    pub enabled: bool,
    pub category: String,
    /// Lowercase extensions without the dot. Empty = any.
    pub extensions: Vec<String>,
    /// MB; 0 = no limit.
    pub min_size_mb: f64,
    pub max_size_mb: f64,
    /// Matches when the download's site (host) contains this text. Empty = any site.
    pub site: String,
}

impl Default for Rule {
    fn default() -> Self {
        Self { enabled: true, category: "Other".into(), extensions: vec![], min_size_mb: 0.0, max_size_mb: 0.0, site: String::new() }
    }
}

fn rule(category: &str, exts: &str) -> Rule {
    Rule {
        category: category.into(),
        extensions: exts.split_whitespace().map(str::to_string).collect(),
        ..Default::default()
    }
}

pub fn default_rules() -> Vec<Rule> {
    vec![
        rule("Video", "mkv mp4 avi mov m4v webm wmv flv mpg mpeg ts m2ts vob 3gp ogv"),
        rule("Music", "mp3 flac m4a aac ogg opus wav wma alac ape aiff mka"),
        rule("Games", "nsp xci iso3ds cia gba nds wbfs rvz pkg"),
        rule("Software", "exe msi msix msixbundle appx appxbundle apk dmg deb rpm appimage"),
        rule("Archives", "zip rar 7z tar gz tgz bz2 xz zst cab iso img"),
        rule("Documents", "pdf doc docx xls xlsx ppt pptx odt ods odp txt rtf csv epub mobi azw3 md"),
        rule("Images", "jpg jpeg png gif webp bmp tif tiff heic heif avif svg raw cr2 nef psd"),
    ]
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Schedule {
    pub enabled: bool,
    /// "HH:MM" local time. The window can cross midnight (e.g. 22:00 → 07:00).
    pub from: String,
    pub to: String,
    /// Monday first.
    pub days: [bool; 7],
}

impl Default for Schedule {
    fn default() -> Self {
        Self { enabled: false, from: "08:00".into(), to: "23:00".into(), days: [true; 7] }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
    pub saved: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    // General
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub launch_at_startup: bool,
    pub watch_clipboard: bool,
    pub show_add_dialog: bool,
    pub notify_complete: bool,
    pub register_handlers: bool,

    // Downloads
    pub download_dir: String,
    pub connections: u32,
    pub max_retries: u32,
    pub max_active: u32,
    pub user_agent: String,

    // Sorting and extraction
    pub sort_enabled: bool,
    pub folders: std::collections::BTreeMap<String, String>,
    pub rules: Vec<Rule>,
    pub auto_extract: bool,
    pub delete_archive_after_extract: bool,

    // Speed (KB/s, 0 = unlimited)
    pub down_limit: u32,
    pub up_limit: u32,
    pub alt_down_limit: u32,
    pub alt_up_limit: u32,
    /// Manual "alternative speed" switch (the turtle button).
    pub alt_enabled: bool,
    /// Alternative limits apply inside this window.
    pub schedule: Schedule,

    // Torrents
    pub listen_port: u16,
    pub dht: bool,
    pub upnp: bool,
    pub natpmp: bool,
    /// "prefer" | "require" | "disabled"
    pub encryption: String,
    pub blocklist_enabled: bool,
    /// http(s) URL or a local file path (.dat / .p2p / .gz).
    pub blocklist: String,
    pub max_active_seeds: u32,
    /// 0 = no limit.
    pub ratio_limit: f64,
    /// Minutes; 0 = no limit.
    pub seed_time_limit: u32,
    /// Network adapter (friendly name) for torrent traffic; empty = any.
    pub bind_adapter: String,
    pub preallocate: bool,

    // Safety
    /// Check finished downloads and give them a safety score.
    pub safety_scan: bool,
    /// Include a Windows Defender scan in that check.
    pub safety_defender: bool,
    /// Mark downloads as from the internet (Zone.Identifier), like browsers do.
    pub safety_motw: bool,
    /// Optional VirusTotal API key (sealed); only file hashes are looked up.
    pub vt_key: String,
    // Debrid (Real-Debrid)
    pub debrid_enabled: bool,
    /// The API key, encrypted for this Windows account (see secret.rs).
    pub debrid_key: String,
    /// Send links from supported file hosters through Real-Debrid.
    pub debrid_hosters: bool,
    /// Torrents through Real-Debrid: "off" | "ask" | "always".
    pub debrid_torrents: String,

    // Browser
    pub bridge_enabled: bool,
    pub bridge_port: u16,

    // Appearance
    pub theme: String,
    /// "theme" | "windows" | "#rrggbb"
    pub accent: String,
    pub native_title_bar: bool,
    pub compact_rows: bool,
    /// Show speeds in bits (Mbps), like internet plans, instead of bytes (MB/s).
    pub speed_bits: bool,

    // Updates
    pub auto_update: bool,
    /// "stable" | "beta"
    pub channel: String,
    pub skip_version: String,

    // Video & music
    pub media_defaults: crate::media::MediaOptions,
    /// Keep yt-dlp up to date automatically.
    pub ytdlp_auto_update: bool,

    // Hugging Face (token encrypted, see secret.rs)
    pub hf_token: String,

    // Mini window
    pub mini_hotkey: String,
    pub mini_x: i32,
    pub mini_y: i32,
    pub mini_saved: bool,

    /// Settings format version (for one-time upgrades). Missing in older files = 0.
    #[serde(default = "zero")]
    pub schema: u32,

    pub window: WindowState,
    /// Set once the first-run setup has happened.
    pub initialized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let dl = paths::downloads_dir();
        let folders = CATEGORIES
            .iter()
            .map(|c| (c.to_string(), dl.join(c).to_string_lossy().to_string()))
            .collect();
        Self {
            close_to_tray: true,
            start_minimized: false,
            launch_at_startup: false,
            watch_clipboard: true,
            show_add_dialog: true,
            notify_complete: true,
            register_handlers: true,
            download_dir: dl.to_string_lossy().to_string(),
            // 0 = Auto (by file size)
            connections: 0,
            max_retries: 5,
            max_active: 4,
            user_agent: String::new(),
            sort_enabled: true,
            folders,
            rules: default_rules(),
            auto_extract: false,
            delete_archive_after_extract: false,
            down_limit: 0,
            up_limit: 0,
            alt_down_limit: 2048,
            alt_up_limit: 256,
            alt_enabled: false,
            schedule: Schedule::default(),
            listen_port: 0,
            dht: true,
            upnp: true,
            natpmp: true,
            encryption: "prefer".into(),
            blocklist_enabled: false,
            blocklist: String::new(),
            max_active_seeds: 0,
            ratio_limit: 0.0,
            seed_time_limit: 0,
            bind_adapter: String::new(),
            preallocate: false,
            safety_scan: true,
            safety_defender: true,
            safety_motw: true,
            vt_key: String::new(),
            debrid_enabled: true,
            debrid_key: String::new(),
            debrid_hosters: true,
            debrid_torrents: "ask".into(),
            bridge_enabled: true,
            bridge_port: crate::bridge::DEFAULT_PORT,
            theme: "dark".into(),
            accent: "theme".into(),
            native_title_bar: false,
            compact_rows: false,
            speed_bits: false,
            auto_update: true,
            channel: "stable".into(),
            skip_version: String::new(),
            media_defaults: Default::default(),
            ytdlp_auto_update: true,
            hf_token: String::new(),
            mini_hotkey: "Ctrl+Alt+H".into(),
            mini_x: 0,
            mini_y: 0,
            mini_saved: false,
            schema: 3,
            window: WindowState::default(),
            initialized: false,
        }
    }
}

fn zero() -> u32 {
    0
}

static SETTINGS: OnceLock<RwLock<Settings>> = OnceLock::new();

fn cell() -> &'static RwLock<Settings> {
    SETTINGS.get_or_init(|| RwLock::new(load()))
}

fn load() -> Settings {
    let path = paths::get().settings();
    let mut s: Settings = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    // Any category added later gets a folder.
    let dl = std::path::PathBuf::from(&s.download_dir);
    for c in CATEGORIES {
        s.folders.entry(c.to_string()).or_insert_with(|| dl.join(c).to_string_lossy().to_string());
    }
    if s.listen_port == 0 {
        // A fixed random port per install, so port forwarding rules keep working.
        s.listen_port = 20000 + (std::process::id() as u16 ^ (chrono::Local::now().timestamp_subsec_nanos() as u16)) % 40000;
    }
    // 0.3.0: connections became "Auto" by default (the old default was a fixed 8).
    if s.schema < 3 {
        if s.connections == 8 {
            s.connections = 0;
        }
        s.schema = 3;
    }
    s.connections = s.connections.min(32);
    s
}

pub fn get() -> Settings {
    cell().read().clone()
}

pub fn with<R>(f: impl FnOnce(&Settings) -> R) -> R {
    f(&cell().read())
}

/// Updates settings and saves them.
pub fn update(f: impl FnOnce(&mut Settings)) -> Settings {
    let snapshot = {
        let mut g = cell().write();
        f(&mut g);
        g.clone()
    };
    save(&snapshot);
    snapshot
}

pub fn replace(new: Settings) -> Settings {
    update(|s| *s = new)
}

fn save(s: &Settings) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = paths::write_atomic(&paths::get().settings(), &b);
    }
}

/// Is the alternative (scheduled or manual) speed limit active right now?
pub fn alt_active(s: &Settings) -> bool {
    if s.alt_enabled {
        return true;
    }
    if !s.schedule.enabled {
        return false;
    }
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    in_window(&s.schedule, now.weekday().num_days_from_monday() as usize, now.hour() * 60 + now.minute())
}

fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// `weekday` is 0 = Monday. A window that crosses midnight belongs to the day it starts on.
pub fn in_window(s: &Schedule, weekday: usize, minute: u32) -> bool {
    let (Some(from), Some(to)) = (parse_hm(&s.from), parse_hm(&s.to)) else { return false };
    if from == to {
        return s.days[weekday];
    }
    if from < to {
        s.days[weekday] && minute >= from && minute < to
    } else {
        let yesterday = (weekday + 6) % 7;
        (s.days[weekday] && minute >= from) || (s.days[yesterday] && minute < to)
    }
}

/// Effective (download, upload) limits in bytes/s; 0 = unlimited.
pub fn effective_limits(s: &Settings) -> (u64, u64) {
    let (d, u) = if alt_active(s) { (s.alt_down_limit, s.alt_up_limit) } else { (s.down_limit, s.up_limit) };
    (d as u64 * 1024, u as u64 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_windows() {
        let mut s = Schedule { enabled: true, from: "08:00".into(), to: "23:00".into(), days: [true; 7] };
        assert!(in_window(&s, 0, 8 * 60));
        assert!(!in_window(&s, 0, 23 * 60));
        assert!(!in_window(&s, 0, 7 * 60 + 59));
        // Overnight window: Friday 22:00 → Saturday 07:00, Friday only.
        s.from = "22:00".into();
        s.to = "07:00".into();
        s.days = [false, false, false, false, true, false, false];
        assert!(in_window(&s, 4, 23 * 60));
        assert!(in_window(&s, 5, 6 * 60));
        assert!(!in_window(&s, 5, 23 * 60));
        assert!(!in_window(&s, 4, 6 * 60));
    }

    #[test]
    fn old_files_get_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"forest"}"#).unwrap();
        assert_eq!(s.theme, "forest");
        assert_eq!(s.connections, 0);
        assert_eq!(s.rules.len(), default_rules().len());
    }
}
