//! The torrent engine: a librqbit session (with Haul's patches, see vendor/README.md).
//!
//! librqbit keeps its own state in `session\` (with fast-resume), so torrents come back after a
//! restart without re-checking. Haul's item list adds what the user sees and controls on top:
//! category, limits, seeding rules, file priorities, queue position.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerMode, ListenerOptions,
    ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig,
};
use parking_lot::{Mutex, RwLock};
use std::str::FromStr;

type ManagedTorrentHandle = Arc<ManagedTorrent>;
use serde::Serialize;

use crate::{netif, paths, settings};

static SESSION: RwLock<Option<Arc<Session>>> = RwLock::new(None);
static RESTARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub running: bool,
    pub error: String,
    pub port: u16,
    pub dht: bool,
    pub upnp: bool,
    pub natpmp: String,
    pub blocklist: String,
    /// The adapter torrents are bound to ("" = any).
    pub adapter: String,
    /// True when bound to an adapter that's gone: torrents are stopped until it's back.
    pub adapter_down: bool,
    pub encryption: String,
}

static STATUS: Mutex<Option<EngineStatus>> = Mutex::new(None);

pub fn status() -> EngineStatus {
    STATUS.lock().clone().unwrap_or_default()
}

fn set_status(f: impl FnOnce(&mut EngineStatus)) {
    let mut g = STATUS.lock();
    let s = g.get_or_insert_with(Default::default);
    f(s);
}

pub fn session() -> Option<Arc<Session>> {
    SESSION.read().clone()
}

pub fn apply_encryption(mode: &str) {
    use librqbit::mse::{EncryptionMode, set_mode};
    set_mode(match mode {
        "require" => EncryptionMode::Require,
        "disabled" => EncryptionMode::Disabled,
        _ => EncryptionMode::Prefer,
    });
    set_status(|s| s.encryption = mode.to_string());
}

fn blocklist_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("file://") {
        return Some(raw.to_string());
    }
    url::Url::from_file_path(paths::expand_env(raw)).ok().map(|u| u.to_string())
}

async fn build(s: &settings::Settings, adapter: Option<&netif::Adapter>, with_blocklist: bool) -> anyhow::Result<Arc<Session>> {
    let session_dir = paths::get().session();
    std::fs::create_dir_all(&session_dir).ok();
    let bind_ip: Option<IpAddr> = adapter.and_then(|a| a.ipv4).map(IpAddr::V4);
    let listen_addr = SocketAddr::new(bind_ip.unwrap_or(IpAddr::V6(Ipv6Addr::UNSPECIFIED)), s.listen_port);
    let (down, up) = settings::effective_limits(s);
    let opts = SessionOptions {
        dht: s.dht.then(|| DhtSessionConfig {
            persistence: Some(DhtPersistenceConfig {
                dump_interval: None,
                config_filename: Some(session_dir.join("dht.json")),
            }),
            ..Default::default()
        }),
        bind_device_name: adapter.map(|a| a.index.to_string()),
        bind_local_addr: bind_ip,
        fastresume: true,
        persistence: Some(SessionPersistenceConfig::Json { folder: Some(session_dir.clone()) }),
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpAndUtp,
            listen_addr,
            enable_upnp_port_forwarding: s.upnp,
            ipv4_only: bind_ip.is_some(),
            ..Default::default()
        }),
        ipv4_only: bind_ip.is_some(),
        ratelimits: librqbit::limits::LimitsConfig {
            download_bps: NonZeroU32::new(down.min(u32::MAX as u64) as u32),
            upload_bps: NonZeroU32::new(up.min(u32::MAX as u64) as u32),
        },
        blocklist_url: if with_blocklist && s.blocklist_enabled { blocklist_url(&s.blocklist) } else { None },
        client_name_and_version: Some(format!("Haul {}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };
    let default_out = PathBuf::from(paths::expand_env(&s.download_dir));
    std::fs::create_dir_all(&default_out).ok();
    Session::new_with_opts(default_out, opts).await
}

/// Starts (or restarts) the session with the current settings.
pub async fn start() -> anyhow::Result<()> {
    let _guard = RESTARTING.lock().await;
    let old = SESSION.write().take();
    if let Some(old) = old {
        old.stop().await;
        // Give sockets a moment to close before binding the same port again.
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let s = settings::get();
    apply_encryption(&s.encryption);

    let adapter = if s.bind_adapter.trim().is_empty() {
        None
    } else {
        match netif::find(&s.bind_adapter) {
            Some(a) if a.up => Some(a),
            _ => {
                // Kill switch: no session at all while the chosen adapter is missing.
                set_status(|st| {
                    st.running = false;
                    st.adapter = s.bind_adapter.clone();
                    st.adapter_down = true;
                    st.error = format!("Waiting for \"{}\" to connect", s.bind_adapter);
                });
                return Ok(());
            }
        }
    };

    let mut blocklist_note = if s.blocklist_enabled && !s.blocklist.trim().is_empty() { "loaded".to_string() } else { String::new() };
    let session = match build(&s, adapter.as_ref(), true).await {
        Ok(sess) => sess,
        Err(e) if s.blocklist_enabled && !s.blocklist.trim().is_empty() => {
            // A bad blocklist shouldn't stop torrents altogether.
            blocklist_note = format!("couldn't load: {e:#}");
            paths::log(format!("blocklist: {e:#}"));
            build(&s, adapter.as_ref(), false).await.context("starting the torrent engine")?
        }
        Err(e) => {
            set_status(|st| {
                st.running = false;
                st.error = format!("{e:#}");
            });
            return Err(e);
        }
    };
    if blocklist_note == "loaded" {
        blocklist_note = format!("{} ranges", session.blocklist.len());
    }
    let port = session.announce_port().or_else(|| session.listen_addr().map(|a| a.port())).unwrap_or(s.listen_port);
    *SESSION.write() = Some(session);
    set_status(|st| {
        st.running = true;
        st.error.clear();
        st.port = port;
        st.dht = s.dht;
        st.upnp = s.upnp;
        st.blocklist = blocklist_note;
        st.adapter = adapter.as_ref().map(|a| a.name.clone()).unwrap_or_default();
        st.adapter_down = false;
    });
    if s.natpmp {
        let local = adapter.as_ref().and_then(|a| a.ipv4);
        tokio::task::spawn_blocking(move || {
            let r = netif::natpmp_map(port, 7200, local);
            set_status(|st| {
                st.natpmp = match r {
                    Ok(p) => format!("mapped port {p}"),
                    Err(e) => e,
                }
            });
        });
    } else {
        set_status(|st| st.natpmp.clear());
    }
    Ok(())
}

pub async fn shutdown() {
    let old = SESSION.write().take();
    if let Some(s) = old {
        s.stop().await;
    }
}

fn id_of(hash: &str) -> Option<TorrentIdOrHash> {
    Id20::from_str(hash).ok().map(TorrentIdOrHash::Hash)
}

pub fn handle(hash: &str) -> Option<ManagedTorrentHandle> {
    session()?.get(id_of(hash)?)
}

pub fn torrent_file_path(hash: &str) -> PathBuf {
    paths::get().torrents().join(format!("{}.torrent", hash.to_ascii_lowercase()))
}

#[derive(Debug, Clone)]
pub struct Meta {
    pub hash: String,
    pub name: String,
    pub files: Vec<(String, u64)>,
}

/// Info hash of a magnet link or .torrent, without starting anything.
pub fn quick_hash(source: &[u8]) -> Option<String> {
    if let Ok(s) = std::str::from_utf8(source) {
        if s.trim_start().starts_with("magnet:") {
            return librqbit::Magnet::parse(s.trim()).ok()?.as_id20().map(|h| h.as_string());
        }
    }
    librqbit::torrent_from_bytes(source).ok().map(|t| t.info_hash.as_string())
}

pub fn magnet_name(magnet: &str) -> Option<String> {
    librqbit::Magnet::parse(magnet).ok()?.name
}

/// Gets a torrent's name and file list. For a magnet link this asks peers for the metadata,
/// which can take a while.
pub async fn fetch_meta(source: &[u8]) -> anyhow::Result<Meta> {
    let session = session().context("The torrent engine isn't running")?;
    let add = match std::str::from_utf8(source) {
        Ok(s) if s.trim_start().starts_with("magnet:") => AddTorrent::from_url(s.trim().to_string()),
        _ => AddTorrent::from_bytes(source.to_vec()),
    };
    let resp = session
        .add_torrent(add, Some(AddTorrentOptions { list_only: true, ..Default::default() }))
        .await
        .map_err(|e| anyhow::anyhow!("{e:#}"))?;
    let AddTorrentResponse::ListOnly(l) = resp else { anyhow::bail!("unexpected response") };
    let hash = l.info_hash.as_string();
    let name = l.info.name().map(|n| n.to_string()).unwrap_or_else(|| hash.clone());
    let files = l
        .info
        .iter_file_details()
        .map(|f| (f.filename.to_vec().join("/"), f.len))
        .collect();
    let bytes = l.torrent_bytes.clone();
    std::fs::create_dir_all(paths::get().torrents()).ok();
    let _ = std::fs::write(torrent_file_path(&hash), &bytes);
    Ok(Meta { hash, name, files })
}

/// Adds a torrent (from its saved .torrent) to the session.
pub async fn add(hash: &str, output_folder: &str, only_files: Option<Vec<usize>>, paused: bool) -> anyhow::Result<()> {
    let session = session().context("The torrent engine isn't running")?;
    if session.get(id_of(hash).context("bad hash")?).is_some() {
        return Ok(());
    }
    let bytes = std::fs::read(torrent_file_path(hash)).context("The .torrent file is missing")?;
    std::fs::create_dir_all(output_folder).ok();
    let opts = AddTorrentOptions {
        paused,
        overwrite: true,
        output_folder: Some(output_folder.to_string()),
        only_files,
        ..Default::default()
    };
    session
        .add_torrent(AddTorrent::from_bytes(bytes), Some(opts))
        .await
        .map_err(|e| anyhow::anyhow!("{e:#}"))?;
    Ok(())
}

pub async fn pause(hash: &str) {
    if let (Some(s), Some(h)) = (session(), handle(hash)) {
        if !h.is_paused() {
            let _ = s.pause(&h).await;
        }
    }
}

pub async fn unpause(hash: &str) -> anyhow::Result<()> {
    if let (Some(s), Some(h)) = (session(), handle(hash)) {
        if h.is_paused() {
            s.unpause(&h).await.map_err(|e| anyhow::anyhow!("{e:#}"))?;
        }
    }
    Ok(())
}

pub async fn remove(hash: &str, delete_files: bool) {
    if let (Some(s), Some(id)) = (session(), id_of(hash)) {
        let _ = s.delete(id, delete_files).await;
    }
    let _ = std::fs::remove_file(torrent_file_path(hash));
}

pub async fn set_only_files(hash: &str, files: &[usize]) -> anyhow::Result<()> {
    let (Some(s), Some(h)) = (session(), handle(hash)) else { return Ok(()) };
    let set: HashSet<usize> = files.iter().copied().collect();
    s.update_only_files(&h, &set).await.map_err(|e| anyhow::anyhow!("{e:#}"))
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub state: &'static str,
    pub error: String,
    pub done: u64,
    pub total: u64,
    pub finished: bool,
    /// Uploaded since this torrent last started.
    pub uploaded_live: u64,
    pub down: u64,
    pub up: u64,
    pub peers: u32,
    pub file_done: Vec<u64>,
    /// Changes every time the torrent goes live (to reapply per-torrent settings).
    pub live_id: usize,
}

pub fn stats(hash: &str) -> Option<Stats> {
    let h = handle(hash)?;
    let st = h.stats();
    let state = match st.state {
        librqbit::TorrentStatsState::Initializing { .. } => "checking",
        librqbit::TorrentStatsState::Paused => "paused",
        librqbit::TorrentStatsState::Live => "live",
        librqbit::TorrentStatsState::Error => "error",
    };
    let (down, up, peers) = st
        .live
        .as_ref()
        .map(|l| {
            (
                (l.download_speed.mbps * 1_048_576.0) as u64,
                (l.upload_speed.mbps * 1_048_576.0) as u64,
                l.snapshot.peer_stats.live as u32,
            )
        })
        .unwrap_or_default();
    let live_id = h.live().map(|l| Arc::as_ptr(&l) as usize).unwrap_or(0);
    Some(Stats {
        state,
        error: st.error.unwrap_or_default(),
        done: st.progress_bytes,
        total: st.total_bytes,
        finished: st.finished,
        uploaded_live: st.uploaded_bytes,
        down,
        up,
        peers,
        file_done: st.file_progress,
        live_id,
    })
}

/// Pushes per-torrent settings into a running torrent (they reset when it restarts).
pub fn apply_live(hash: &str, priorities: &[u8], sequential: bool, down_kbps: u32, up_kbps: u32) {
    let Some(h) = handle(hash) else { return };
    let Some(live) = h.live() else { return };
    live.set_sequential(sequential);
    live.set_file_priorities(priorities);
    live.set_ratelimits(NonZeroU32::new(down_kbps.saturating_mul(1024)), NonZeroU32::new(up_kbps.saturating_mul(1024)));
}

pub fn set_global_limits(down: u64, up: u64) {
    if let Some(s) = session() {
        let d = NonZeroU32::new(down.min(u32::MAX as u64) as u32);
        let u = NonZeroU32::new(up.min(u32::MAX as u64) as u32);
        if s.ratelimits.get_download_bps() != d {
            s.ratelimits.set_download_bps(d);
        }
        if s.ratelimits.get_upload_bps() != u {
            s.ratelimits.set_upload_bps(u);
        }
    }
}

/// Files of a torrent in the session, with progress.
pub fn files(hash: &str) -> Vec<(String, u64)> {
    let Some(h) = handle(hash) else { return files_from_disk(hash) };
    h.with_metadata(|m| {
        m.file_infos
            .iter()
            .map(|f| (f.relative_filename.to_string_lossy().replace('\\', "/"), f.len))
            .collect::<Vec<_>>()
    })
    .unwrap_or_else(|_| files_from_disk(hash))
}

fn files_from_disk(hash: &str) -> Vec<(String, u64)> {
    let Ok(bytes) = std::fs::read(torrent_file_path(hash)) else { return vec![] };
    let Ok(t) = librqbit::torrent_from_bytes(&bytes) else { return vec![] };
    let Ok(v) = t.info.data.clone().validate() else { return vec![] };
    v.iter_file_details().map(|f| (f.filename.to_vec().join("/"), f.len)).collect()
}

/// Every torrent hash the session knows (to tidy up orphans).
pub fn all_hashes() -> Vec<String> {
    session()
        .map(|s| s.with_torrents(|it| it.map(|(_, t)| t.info_hash().as_string()).collect()))
        .unwrap_or_default()
}

/// Watches the bound adapter: stops torrents when it drops, restarts the session when it's back.
pub async fn watch_adapter() {
    let mut was_ok: Option<bool> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let name = settings::with(|s| s.bind_adapter.clone());
        if name.trim().is_empty() {
            was_ok = None;
            continue;
        }
        let current = netif::find(&name);
        let ok = current.as_ref().map(|a| a.up).unwrap_or(false);
        let bound_ip_changed = {
            let st = status();
            st.running && !st.adapter_down && current.as_ref().and_then(|a| a.ipv4).is_none()
        };
        if was_ok == Some(ok) && !bound_ip_changed {
            continue;
        }
        if !ok {
            paths::log(format!("adapter \"{name}\" is down: stopping torrents"));
            shutdown().await;
            set_status(|st| {
                st.running = false;
                st.adapter_down = true;
                st.error = format!("\"{name}\" is disconnected, so torrents are stopped");
            });
        } else if was_ok.is_some() || !status().running {
            paths::log(format!("adapter \"{name}\" is up: starting torrents"));
            if let Err(e) = start().await {
                paths::log(format!("restart after adapter came back: {e:#}"));
            }
        }
        was_ok = Some(ok);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes() {
        let m = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny";
        assert_eq!(quick_hash(m.as_bytes()).unwrap(), "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
        assert_eq!(magnet_name(m).unwrap(), "Big Buck Bunny");
        assert!(quick_hash(b"nope").is_none());
    }

    #[test]
    fn blocklist_urls() {
        assert_eq!(blocklist_url(" "), None);
        assert_eq!(blocklist_url("https://x/y.gz").unwrap(), "https://x/y.gz");
        #[cfg(windows)]
        assert!(blocklist_url(r"C:\lists\a.p2p").unwrap().starts_with("file:///C:/"));
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// Real network: fetch metadata for Big Buck Bunny (public domain) and download a few MB
    /// with encryption preferred. Only runs with HAUL_TEST_TORRENT=1 (CI).
    #[test]
    fn real_torrent() {
        if std::env::var("HAUL_TEST_TORRENT").is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("haul-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LOCALAPPDATA", &dir);
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async {
            start().await.expect("engine");
            println!("status {:?}", status());
            let magnet = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=udp%3A%2F%2Fopen.demonii.com%3A1337%2Fannounce&tr=wss%3A%2F%2Ftracker.webtorrent.dev&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F";
            let meta = tokio::time::timeout(Duration::from_secs(240), fetch_meta(magnet.as_bytes())).await.expect("metadata timeout").expect("metadata");
            println!("{} files: {:?}", meta.name, meta.files);
            let out = dir.join("out").to_string_lossy().to_string();
            add(&meta.hash, &out, None, false).await.expect("add");
            let start_t = std::time::Instant::now();
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let s = stats(&meta.hash).expect("stats");
                println!("{} {}/{} peers {} down {}", s.state, s.done, s.total, s.peers, s.down);
                apply_live(&meta.hash, &[2, 3, 1], true, 0, 0);
                if s.done > 4 * 1024 * 1024 {
                    break;
                }
                assert!(start_t.elapsed() < Duration::from_secs(600), "too slow");
            }
            pause(&meta.hash).await;
            remove(&meta.hash, true).await;
            shutdown().await;
        });
    }
}
