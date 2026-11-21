//! The download list: one `Item` per direct download or torrent, saved to items.json.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::paths;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Http,
    Torrent,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Waiting for a free slot.
    #[default]
    Queued,
    /// Magnet link: finding the torrent's file list.
    Metadata,
    /// Torrent: checking pieces already on disk.
    Checking,
    Downloading,
    Seeding,
    Paused,
    Completed,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start: u64,
    /// Exclusive.
    pub end: u64,
    pub done: u64,
}

impl Segment {
    pub fn remaining(&self) -> u64 {
        self.end.saturating_sub(self.start + self.done)
    }
}

/// A file inside a torrent (built from the torrent's metadata, not saved).
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFile {
    pub index: usize,
    pub path: String,
    pub size: u64,
    pub done: u64,
    /// 0 = skip, 1 = low, 2 = normal, 3 = high.
    pub priority: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Item {
    pub id: u64,
    pub kind: Kind,
    pub name: String,
    /// URL, or magnet link for torrents.
    pub source: String,
    pub referrer: String,
    pub cookies: String,
    pub user_agent: String,
    /// Folder the file (or torrent folder) goes in.
    pub save_dir: String,
    /// Direct downloads: the final file name.
    pub file_name: String,
    pub category: String,
    pub size: u64,
    pub done: u64,
    pub uploaded: u64,
    pub added: i64,
    pub finished_at: i64,
    /// Queue priority: -1 low, 0 normal, 1 high.
    pub priority: i8,
    /// KB/s, 0 = no limit of its own.
    pub limit_down: u32,
    pub limit_up: u32,
    /// The user paused it (it won't start until resumed).
    pub paused: bool,
    pub status: Status,
    pub error: String,
    pub sha256: String,
    /// "", "checking", "ok", "mismatch"
    pub hash_state: String,

    // Direct downloads
    pub resumable: bool,
    pub segments: Vec<Segment>,
    pub etag: String,
    pub last_modified: String,
    pub final_url: String,
    /// 0 = the default from Settings.
    pub connections: u32,
    pub retries: u32,

    // Torrents
    pub info_hash: String,
    pub file_prio: Vec<u8>,
    pub sequential: bool,
    /// -1 = use the global limit, 0 = no limit.
    pub ratio_limit: f64,
    /// Minutes. -1 = use the global limit, 0 = no limit.
    pub seed_time_limit: i64,
    pub seeding_secs: u64,

    pub extracted: bool,
    pub notified: bool,

    // Live values (not meaningful across restarts)
    #[serde(skip)]
    pub speed_down: u64,
    #[serde(skip)]
    pub speed_up: u64,
    #[serde(skip)]
    pub eta: i64,
    #[serde(skip)]
    pub peers: u32,
    #[serde(skip)]
    pub note: String,
    #[serde(skip)]
    pub files: Vec<TorrentFile>,
}

impl Item {
    pub fn is_finished(&self) -> bool {
        matches!(self.status, Status::Seeding | Status::Completed)
    }

    pub fn ratio(&self) -> f64 {
        if self.size == 0 { 0.0 } else { self.uploaded as f64 / self.size.max(1) as f64 }
    }

    /// Full path of a direct download's file.
    pub fn file_path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.save_dir).join(&self.file_name)
    }

    /// What's shown in the list (no segments, files or cookies).
    pub fn view(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "kind": self.kind,
            "name": self.name,
            "source": if self.kind == Kind::Http { self.source.as_str() } else { "" },
            "saveDir": self.save_dir,
            "fileName": self.file_name,
            "category": self.category,
            "size": self.size,
            "done": self.done,
            "uploaded": self.uploaded,
            "added": self.added,
            "finishedAt": self.finished_at,
            "priority": self.priority,
            "limitDown": self.limit_down,
            "limitUp": self.limit_up,
            "paused": self.paused,
            "status": self.status,
            "error": self.error,
            "sha256": self.sha256,
            "hashState": self.hash_state,
            "resumable": self.resumable,
            "connections": self.connections,
            "segments": self.segments.len(),
            "infoHash": self.info_hash,
            "sequential": self.sequential,
            "ratioLimit": self.ratio_limit,
            "seedTimeLimit": self.seed_time_limit,
            "seedingSecs": self.seeding_secs,
            "speedDown": self.speed_down,
            "speedUp": self.speed_up,
            "eta": self.eta,
            "peers": self.peers,
            "note": self.note,
            "fileCount": if self.kind == Kind::Torrent { self.file_prio.len() } else { 1 },
        })
    }
}

pub struct Store {
    pub items: Vec<Item>,
    next_id: u64,
}

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
static DIRTY: AtomicBool = AtomicBool::new(false);

fn cell() -> &'static Mutex<Store> {
    STORE.get_or_init(|| {
        let items: Vec<Item> = std::fs::read(paths::get().items())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let next_id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        Mutex::new(Store { items, next_id })
    })
}

pub fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    f(&mut cell().lock())
}

pub fn mark_dirty() {
    DIRTY.store(true, Ordering::Relaxed);
}

impl Store {
    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Item> {
        mark_dirty();
        self.items.iter_mut().find(|i| i.id == id)
    }
    pub fn insert(&mut self, mut item: Item) -> u64 {
        item.id = self.next_id;
        self.next_id += 1;
        if item.added == 0 {
            item.added = chrono::Utc::now().timestamp();
        }
        let id = item.id;
        self.items.push(item);
        mark_dirty();
        id
    }
    pub fn remove(&mut self, id: u64) -> Option<Item> {
        let pos = self.items.iter().position(|i| i.id == id)?;
        mark_dirty();
        Some(self.items.remove(pos))
    }
    pub fn by_hash(&self, hash: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.kind == Kind::Torrent && i.info_hash.eq_ignore_ascii_case(hash))
    }
}

/// Saves items.json if anything changed. Called by the engine every few seconds and on exit.
pub fn save_if_dirty() {
    if !DIRTY.swap(false, Ordering::Relaxed) {
        return;
    }
    let bytes = with(|s| serde_json::to_vec(&s.items));
    if let Ok(b) = bytes {
        if let Err(e) = paths::write_atomic(&paths::get().items(), &b) {
            paths::log(format!("saving items failed: {e}"));
            mark_dirty();
        }
    }
}

/// Picks "name (1).ext", "name (2).ext"… when a file already exists or another item uses the name.
pub fn unique_name(dir: &std::path::Path, name: &str, taken: &[String]) -> String {
    let exists = |n: &str| dir.join(n).exists() || dir.join(format!("{n}.haul")).exists() || taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !exists(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 1..10_000 {
        let cand = format!("{stem} ({n}){ext}");
        if !exists(&cand) {
            return cand;
        }
    }
    format!("{stem}-{}{ext}", chrono::Utc::now().timestamp())
}

/// Removes characters Windows doesn't allow in file names.
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3"];
    let stem = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    let out = if reserved.contains(&stem.as_str()) { format!("_{trimmed}") } else { trimmed };
    if out.is_empty() { "download".into() } else { out.chars().take(200).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes() {
        assert_eq!(sanitize("a/b:c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize("con.txt"), "_con.txt");
        assert_eq!(sanitize("  "), "download");
        assert_eq!(sanitize("name. "), "name");
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("haul-un-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.zip"), b"x").unwrap();
        assert_eq!(unique_name(&dir, "a.zip", &[]), "a (1).zip");
        assert_eq!(unique_name(&dir, "b.zip", &["b.zip".into()]), "b (1).zip");
        assert_eq!(unique_name(&dir, "c", &[]), "c");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
