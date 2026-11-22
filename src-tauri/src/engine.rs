//! Runs the list: starts and stops downloads to honour the queue, limits and schedule, keeps
//! progress up to date, handles finished downloads (checksum, extract, notify) and tells the UI.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::http::{self, Control, Fail};
use crate::model::{self, Item, Kind, Status};
use crate::{debrid, extract, notify, paths, settings, sorter, torrent};

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn app() -> Option<&'static AppHandle> {
    APP.get()
}

pub fn emit<S: Serialize + Clone>(event: &str, payload: S) {
    if let Some(a) = APP.get() {
        let _ = a.emit(event, payload);
    }
}

pub fn toast(text: impl Into<String>, kind: &str) {
    emit("toast", serde_json::json!({ "text": text.into(), "kind": kind }));
}

struct HttpTask {
    ctl: Arc<Control>,
    handle: tokio::task::JoinHandle<Result<(), Fail>>,
    last_bytes: u64,
}

#[derive(Default)]
struct Runtime {
    http: HashMap<u64, HttpTask>,
    /// Magnet / .torrent metadata lookups in progress.
    meta: HashMap<u64, tokio::task::JoinHandle<()>>,
    /// Torrents waiting for the user to pick files in the Add dialog.
    awaiting: HashSet<u64>,
    /// Their file lists, once known (shown one dialog at a time).
    pending: Vec<TorrentMetaView>,
    /// Per torrent: which live session we've configured and the upload count before it.
    live: HashMap<u64, (usize, u64)>,
    /// Smoothed speeds.
    speed: HashMap<u64, f64>,
    /// When an errored download may retry.
    retry_at: HashMap<u64, Instant>,
    /// Finished items whose after-steps (hash, extract, notify) are running.
    finishing: HashSet<u64>,
    /// What the UI last got, to only send changes.
    sent: HashMap<u64, u64>,
    /// Source bytes for torrents added from a file, until their metadata is read.
    sources: HashMap<u64, Vec<u8>>,
    /// Real-Debrid torrents being followed.
    debrid_jobs: HashMap<u64, tokio::task::JoinHandle<()>>,
    /// Real-Debrid file ids, in the order the file picker showed them.
    rd_files: HashMap<u64, Vec<u64>>,
}

static RT: OnceLock<Mutex<Runtime>> = OnceLock::new();

fn rt() -> parking_lot::MutexGuard<'static, Runtime> {
    RT.get_or_init(|| Mutex::new(Runtime::default())).lock()
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub down: u64,
    pub up: u64,
    pub downloading: usize,
    pub seeding: usize,
    pub alt: bool,
    pub engine: torrent::EngineStatus,
}

static TOTALS: Mutex<Option<Totals>> = Mutex::new(None);

pub fn totals() -> Totals {
    TOTALS.lock().clone().unwrap_or_default()
}

/// Called once at startup.
pub fn init(app: AppHandle) {
    let _ = APP.set(app);
    // Whatever was running when Haul closed picks up again; transient states reset.
    model::with(|s| {
        for it in &mut s.items {
            match it.status {
                Status::Downloading | Status::Checking => it.status = Status::Queued,
                Status::Seeding if it.kind == Kind::Http => it.status = Status::Completed,
                _ => {}
            }
            it.note.clear();
        }
    });
    model::mark_dirty();
    tauri::async_runtime::spawn(async {
        if let Err(e) = torrent::start().await {
            paths::log(format!("torrent engine: {e:#}"));
        }
        restore_torrents().await;
        loop_forever().await;
    });
    tauri::async_runtime::spawn(torrent::watch_adapter());
}

/// Puts torrents back into the session if its own state lost them (e.g. its folder was cleared).
async fn restore_torrents() {
    let known: HashSet<String> = torrent::all_hashes().into_iter().collect();
    let missing: Vec<Item> = model::with(|s| {
        s.items
            .iter()
            .filter(|i| i.kind == Kind::Torrent && !i.info_hash.is_empty() && !i.file_prio.is_empty() && !known.contains(&i.info_hash))
            .cloned()
            .collect()
    });
    for it in missing {
        if torrent::torrent_file_path(&it.info_hash).exists() {
            let only = selected_files(&it.file_prio);
            if let Err(e) = torrent::add(&it.info_hash, &it.save_dir, Some(only), true).await {
                paths::log(format!("restore {}: {e:#}", it.name));
            }
        }
    }
    // Torrents librqbit has that Haul doesn't list anymore (removed while it was stopping).
    let listed: HashSet<String> = model::with(|s| s.items.iter().map(|i| i.info_hash.clone()).collect());
    for h in torrent::all_hashes() {
        if !listed.contains(&h) {
            torrent::remove(&h, false).await;
        }
    }
}

pub fn selected_files(prio: &[u8]) -> Vec<usize> {
    prio.iter().enumerate().filter(|(_, p)| **p > 0).map(|(i, _)| i).collect()
}

async fn loop_forever() {
    let mut last_save = Instant::now();
    let mut last = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let dt = last.elapsed().as_secs_f64().max(0.2);
        last = Instant::now();
        tick(dt).await;
        if last_save.elapsed() > Duration::from_secs(3) {
            model::save_if_dirty();
            last_save = Instant::now();
        }
    }
}

/// Splits the global limit between direct downloads and torrents by what each is using.
fn split_limit(limit: u64, http_speed: u64, torrent_speed: u64) -> (u64, u64) {
    if limit == 0 {
        return (0, 0);
    }
    let half = limit / 2;
    let http = limit - torrent_speed.min(half);
    let tor = limit - http_speed.min(half);
    (http.max(1024), tor.max(1024))
}

async fn tick(dt: f64) {
    let s = settings::get();
    let (down_limit, up_limit) = settings::effective_limits(&s);
    let alt = settings::alt_active(&s);

    // ---- collect progress from running downloads
    let mut finished_http: Vec<(u64, Result<(), Fail>)> = Vec::new();
    let mut http_speed_total = 0u64;
    {
        let mut r = rt();
        let ids: Vec<u64> = r.http.keys().copied().collect();
        for id in ids {
            let (bytes, done_flag) = {
                let t = r.http.get(&id).unwrap();
                (t.ctl.downloaded.load(Ordering::Relaxed), t.handle.is_finished())
            };
            let delta = {
                let t = r.http.get_mut(&id).unwrap();
                let d = bytes.saturating_sub(t.last_bytes);
                t.last_bytes = bytes;
                d
            };
            let inst = delta as f64 / dt;
            let sp = r.speed.entry(id).or_insert(inst);
            *sp = *sp * 0.6 + inst * 0.4;
            let speed = *sp as u64;
            http_speed_total += speed;
            let t = r.http.get(&id).unwrap();
            let segs = t.ctl.segments.lock().clone();
            let conns = t.ctl.connections.load(Ordering::Relaxed);
            let limit = model::with(|st| st.get(id).map(|i| i.limit_down)).unwrap_or(0);
            t.ctl.limiter.set_rate(limit as u64 * 1024);
            model::with(|st| {
                if let Some(it) = st.get_mut(id) {
                    if !segs.is_empty() {
                        it.done = segs.iter().map(|x| x.done).sum();
                        it.segments = segs;
                    }
                    it.speed_down = speed;
                    it.eta = if speed > 0 && it.size > 0 { (it.size.saturating_sub(it.done) / speed) as i64 } else { -1 };
                    it.note = if conns > 1 { format!("{conns} connections") } else { String::new() };
                }
            });
            if done_flag {
                let t = r.http.remove(&id).unwrap();
                r.speed.remove(&id);
                let stopped = t.ctl.stopped();
                let res = match t.handle.now_or_never_result() {
                    Some(r) => r,
                    None => Ok(()),
                };
                if !stopped || res.is_err() {
                    finished_http.push((id, res));
                }
            }
        }
    }
    for (id, res) in finished_http {
        match res {
            Ok(()) => mark_finished(id),
            Err(f) => {
                http::log_fail(id, &f);
                let permanent = matches!(f, Fail::Permanent(_));
                // A Real-Debrid link that stopped working: get a fresh one once and carry on.
                let renew = permanent
                    && model::with(|st| {
                        st.get_mut(id)
                            .filter(|it| !it.debrid_link.is_empty() && !it.debrid_renewed && !f.text().starts_with("Real-Debrid"))
                            .map(|it| {
                                it.debrid_renewed = true;
                                it.source.clear();
                                it.status = Status::Queued;
                            })
                            .is_some()
                    });
                if renew {
                    continue;
                }
                model::with(|st| {
                    if let Some(it) = st.get_mut(id) {
                        it.status = Status::Error;
                        it.error = f.text().to_string();
                        it.speed_down = 0;
                        if permanent {
                            it.retries = u32::MAX / 2;
                        }
                    }
                });
                if !permanent {
                    let n = model::with(|st| st.get(id).map(|i| i.retries)).unwrap_or(0);
                    rt().retry_at.insert(id, Instant::now() + Duration::from_secs(10 * (n as u64 + 1)));
                }
            }
        }
    }

    // ---- torrents
    let torrent_items: Vec<(u64, String, Vec<u8>, bool, u32, u32, u64)> = model::with(|st| {
        st.items
            .iter()
            .filter(|i| i.kind == Kind::Torrent && !i.info_hash.is_empty() && !i.file_prio.is_empty())
            .map(|i| (i.id, i.info_hash.clone(), i.file_prio.clone(), i.sequential, i.limit_down, i.limit_up, i.uploaded))
            .collect()
    });
    let mut tor_down = 0u64;
    let mut tor_up = 0u64;
    let engine_running = torrent::session().is_some();
    for (id, hash, prio, seq, ld, lu, uploaded) in torrent_items {
        let Some(stv) = torrent::stats(&hash) else { continue };
        tor_down += stv.down;
        tor_up += stv.up;
        // Per-torrent settings live in the running torrent, so reapply when it (re)starts.
        let base = {
            let mut r = rt();
            let entry = r.live.entry(id).or_insert((0, uploaded));
            if stv.live_id != 0 && entry.0 != stv.live_id {
                *entry = (stv.live_id, uploaded);
                torrent::apply_live(&hash, &prio, seq, ld, lu);
            }
            entry.1
        };
        let just_finished = model::with(|st| {
            let Some(it) = st.get_mut(id) else { return false };
            it.size = stv.total;
            it.done = stv.done;
            it.speed_down = stv.down;
            it.speed_up = stv.up;
            it.peers = stv.peers;
            if stv.state == "live" {
                it.uploaded = base + stv.uploaded_live;
            }
            it.eta = if stv.down > 0 { (stv.total.saturating_sub(stv.done) / stv.down) as i64 } else { -1 };
            let was_finished = it.is_finished();
            match stv.state {
                "error" => {
                    it.status = Status::Error;
                    it.error = stv.error.clone();
                }
                "checking" => it.status = Status::Checking,
                "live" if stv.finished => {
                    it.status = Status::Seeding;
                    it.seeding_secs += dt.round() as u64;
                }
                "live" => it.status = Status::Downloading,
                _ => {
                    // paused: decided by the queue below
                    if stv.finished && it.status == Status::Seeding {
                        it.status = Status::Completed;
                    }
                }
            }
            if stv.finished && it.finished_at == 0 {
                it.finished_at = chrono::Utc::now().timestamp();
            }
            stv.finished && !was_finished && it.finished_at > 0 && !it.notified
        });
        if just_finished {
            mark_finished(id);
        }
    }

    // ---- limits
    let (http_lim, tor_lim) = split_limit(down_limit, http_speed_total, tor_down);
    http::global_limiter().set_rate(http_lim);
    torrent::set_global_limits(tor_lim, up_limit);

    // ---- seeding rules
    let seed_stop: Vec<u64> = model::with(|st| {
        st.items
            .iter()
            .filter(|i| i.kind == Kind::Torrent && i.status == Status::Seeding)
            .filter(|i| {
                let ratio = if i.ratio_limit < 0.0 { s.ratio_limit } else { i.ratio_limit };
                let mins = if i.seed_time_limit < 0 { s.seed_time_limit as i64 } else { i.seed_time_limit };
                (ratio > 0.0 && i.ratio() >= ratio) || (mins > 0 && i.seeding_secs >= mins as u64 * 60)
            })
            .map(|i| i.id)
            .collect()
    });
    for id in seed_stop {
        let hash = model::with(|st| {
            st.get_mut(id).map(|i| {
                i.status = Status::Completed;
                i.paused = true;
                i.note = "Seeding goal reached".into();
                i.info_hash.clone()
            })
        });
        if let Some(h) = hash {
            torrent::pause(&h).await;
        }
    }

    // ---- automatic retries
    let due: Vec<u64> = {
        let r = rt();
        r.retry_at.iter().filter(|(_, t)| Instant::now() >= **t).map(|(id, _)| *id).collect()
    };
    for id in due {
        rt().retry_at.remove(&id);
        model::with(|st| {
            if let Some(it) = st.get_mut(id) {
                if it.status == Status::Error && it.retries < s.max_retries {
                    it.retries += 1;
                    it.status = Status::Queued;
                    it.note = format!("Retry {} of {}", it.retries, s.max_retries);
                }
            }
        });
    }

    // ---- queue
    schedule(&s, engine_running).await;

    // ---- metadata lookups that need (re)starting
    if engine_running {
        let waiting: Vec<u64> = model::with(|st| {
            st.items.iter().filter(|i| i.kind == Kind::Torrent && i.status == Status::Metadata && i.file_prio.is_empty()).map(|i| i.id).collect()
        });
        for id in waiting {
            let running = rt().meta.contains_key(&id);
            if !running {
                start_meta(id);
            }
        }
    }

    // ---- Real-Debrid torrents (followed until they become direct downloads)
    let jobs: Vec<u64> = model::with(|st| {
        st.items.iter().filter(|i| i.kind == Kind::Debrid && !i.paused && i.status != Status::Error).map(|i| i.id).collect()
    });
    for id in jobs {
        let running = rt().debrid_jobs.get(&id).map(|h| !h.is_finished()).unwrap_or(false);
        if !running {
            start_debrid(id);
        }
    }

    // ---- tell the UI
    let mut changed = Vec::new();
    let mut downloading = 0;
    let mut seeding = 0;
    {
        let mut r = rt();
        model::with(|st| {
            for it in &st.items {
                match it.status {
                    Status::Downloading | Status::Metadata | Status::Checking => downloading += 1,
                    Status::Seeding => seeding += 1,
                    _ => {}
                }
                let v = it.view();
                let h = {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    v.to_string().hash(&mut hasher);
                    hasher.finish()
                };
                if r.sent.get(&it.id) != Some(&h) {
                    r.sent.insert(it.id, h);
                    changed.push(v);
                }
            }
        });
    }
    if !changed.is_empty() {
        emit("items", changed);
    }
    let totals = Totals {
        down: http_speed_total + tor_down,
        up: tor_up,
        downloading,
        seeding,
        alt,
        engine: torrent::status(),
    };
    *TOTALS.lock() = Some(totals.clone());
    crate::tray::update_tooltip(&totals);
    emit("totals", totals);
    crate::updater::maybe_apply_when_idle(downloading == 0);
}

trait NowOrNever<T> {
    fn now_or_never_result(self) -> Option<T>;
}

impl<T> NowOrNever<T> for tokio::task::JoinHandle<T> {
    fn now_or_never_result(self) -> Option<T> {
        use futures_util::FutureExt;
        match self.now_or_never() {
            Some(Ok(v)) => Some(v),
            _ => None,
        }
    }
}

/// Decides what runs: up to `max_active` downloads in queue order, seeds within their limit.
async fn schedule(s: &settings::Settings, engine_running: bool) {
    // Queue order: priority, then position in the list.
    let mut wanted: Vec<(i8, usize, u64, Kind)> = model::with(|st| {
        st.items
            .iter()
            .enumerate()
            .filter(|(_, i)| !i.paused && !i.is_finished() && i.status != Status::Error && i.status != Status::Metadata)
            .filter(|(_, i)| i.kind == Kind::Http || (i.kind == Kind::Torrent && !i.file_prio.is_empty()))
            .map(|(pos, i)| (i.priority, pos, i.id, i.kind))
            .collect()
    });
    wanted.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let max = if s.max_active == 0 { usize::MAX } else { s.max_active as usize };
    let run: HashSet<u64> = wanted.iter().take(max).map(|w| w.2).collect();

    // Running non-resumable downloads are never interrupted by the queue.
    let keep: HashSet<u64> = {
        let r = rt();
        model::with(|st| r.http.keys().filter(|id| st.get(**id).map(|i| !i.resumable).unwrap_or(false)).copied().collect())
    };

    for (_, _, id, kind) in &wanted {
        let should = run.contains(id) || keep.contains(id);
        match kind {
            Kind::Http => {
                let running = rt().http.contains_key(id);
                if should && !running {
                    start_http(*id);
                } else if !should && running {
                    stop_http(*id);
                }
                if !should {
                    set_status(*id, Status::Queued);
                }
            }
            // Followed by its own job (it's downloading on Real-Debrid's side).
            Kind::Debrid => {}
            Kind::Torrent => {
                let hash = model::with(|st| st.get(*id).map(|i| i.info_hash.clone())).unwrap_or_default();
                if !engine_running {
                    model::with(|st| {
                        if let Some(it) = st.get_mut(*id) {
                            it.status = Status::Queued;
                            it.speed_down = 0;
                            it.speed_up = 0;
                            it.note = "Torrents are stopped".into();
                        }
                    });
                    continue;
                }
                if should {
                    if torrent::handle(&hash).is_none() {
                        let (dir, prio) = model::with(|st| st.get(*id).map(|i| (i.save_dir.clone(), i.file_prio.clone()))).unwrap_or_default();
                        if let Err(e) = torrent::add(&hash, &dir, Some(selected_files(&prio)), false).await {
                            set_error(*id, &format!("{e:#}"));
                        }
                    } else if let Err(e) = torrent::unpause(&hash).await {
                        set_error(*id, &format!("{e:#}"));
                    }
                } else {
                    torrent::pause(&hash).await;
                    set_status(*id, Status::Queued);
                }
            }
        }
    }

    // Finished torrents: seed (within the active-seeds limit) unless paused.
    let seeds: Vec<(u64, String, bool)> = model::with(|st| {
        st.items
            .iter()
            .filter(|i| i.kind == Kind::Torrent && i.is_finished() && !i.info_hash.is_empty())
            .map(|i| (i.id, i.info_hash.clone(), i.paused))
            .collect()
    });
    let max_seeds = if s.max_active_seeds == 0 { usize::MAX } else { s.max_active_seeds as usize };
    let mut active_seeds = 0;
    for (id, hash, paused) in seeds {
        if !engine_running {
            break;
        }
        if paused || active_seeds >= max_seeds {
            torrent::pause(&hash).await;
            model::with(|st| {
                if let Some(it) = st.get_mut(id) {
                    if it.status == Status::Seeding {
                        it.status = Status::Completed;
                    }
                    it.speed_up = 0;
                }
            });
        } else {
            active_seeds += 1;
            let _ = torrent::unpause(&hash).await;
        }
    }

    // Paused items show as Paused.
    model::with(|st| {
        for it in st.items.iter_mut() {
            if it.paused && !it.is_finished() && it.status != Status::Error && it.status != Status::Paused {
                it.status = Status::Paused;
                it.speed_down = 0;
                it.speed_up = 0;
            }
        }
    });
}

fn set_status(id: u64, status: Status) {
    model::with(|st| {
        if let Some(it) = st.get_mut(id) {
            if it.status != status {
                it.status = status;
                it.speed_down = 0;
                it.speed_up = 0;
            }
        }
    });
}

fn set_error(id: u64, err: &str) {
    model::with(|st| {
        if let Some(it) = st.get_mut(id) {
            it.status = Status::Error;
            it.error = err.to_string();
        }
    });
}

fn start_http(id: u64) {
    let ctl = Control::new();
    model::with(|st| {
        if let Some(it) = st.get_mut(id) {
            it.status = Status::Downloading;
            it.error.clear();
            *ctl.segments.lock() = it.segments.clone();
        }
    });
    let c2 = ctl.clone();
    let handle = tauri::async_runtime::spawn(async move { http::run(id, c2).await });
    let tauri::async_runtime::JoinHandle::Tokio(handle) = handle;
    rt().http.insert(id, HttpTask { ctl, handle, last_bytes: 0 });
}

fn stop_http(id: u64) {
    if let Some(t) = rt().http.get(&id) {
        t.ctl.stop.store(true, Ordering::Relaxed);
    }
}

/// Waits (briefly) for a direct download to stop and save its progress.
async fn stop_http_wait(id: u64) {
    stop_http(id);
    for _ in 0..50 {
        let finished = rt().http.get(&id).map(|t| t.handle.is_finished()).unwrap_or(true);
        if finished {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    rt().http.remove(&id);
}

// ---------------------------------------------------------------- finishing

fn mark_finished(id: u64) {
    let kind = model::with(|st| {
        st.get_mut(id).map(|it| {
            if it.kind == Kind::Http {
                it.status = Status::Completed;
                it.speed_down = 0;
                it.eta = -1;
                it.note.clear();
            }
            if it.finished_at == 0 {
                it.finished_at = chrono::Utc::now().timestamp();
            }
            it.kind
        })
    });
    if kind.is_none() || !rt().finishing.insert(id) {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        after_finish(id);
        rt().finishing.remove(&id);
    });
}

fn after_finish(id: u64) {
    let Some(it) = model::with(|st| st.get(id).cloned()) else { return };
    let s = settings::get();
    // Checksum
    if it.kind == Kind::Http && !it.sha256.trim().is_empty() {
        model::with(|st| st.get_mut(id).map(|i| i.hash_state = "checking".into()));
        let state = match http::sha256_file(&it.file_path()) {
            Ok(h) if h.eq_ignore_ascii_case(it.sha256.trim()) => "ok",
            Ok(_) => "mismatch",
            Err(_) => "mismatch",
        };
        model::with(|st| {
            if let Some(i) = st.get_mut(id) {
                i.hash_state = state.into();
                if state == "mismatch" {
                    i.status = Status::Error;
                    i.error = "SHA-256 doesn't match: the file may be damaged or changed".into();
                }
            }
        });
        if state == "mismatch" {
            notify::show(id, "Checksum mismatch", &it.name, false);
            return;
        }
    }
    // Extract
    if s.auto_extract && !it.extracted {
        let archives: Vec<std::path::PathBuf> = if it.kind == Kind::Http {
            vec![it.file_path()]
        } else {
            torrent::files(&it.info_hash)
                .iter()
                .enumerate()
                .filter(|(i, _)| it.file_prio.get(*i).copied().unwrap_or(1) > 0)
                .map(|(_, (p, _))| std::path::Path::new(&it.save_dir).join(p))
                .collect()
        };
        for a in archives {
            let name = a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if !extract::is_archive(&name) || extract::is_secondary_part(&name) {
                continue;
            }
            model::with(|st| st.get_mut(id).map(|i| i.note = "Extracting…".into()));
            // Torrents keep their archives so they can still seed.
            let delete = s.delete_archive_after_extract && it.kind == Kind::Http;
            match extract::extract(&a, delete) {
                Ok(out) => {
                    model::with(|st| {
                        st.get_mut(id).map(|i| {
                            i.extracted = true;
                            i.note = format!("Extracted to {}", out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                        })
                    });
                }
                Err(e) => {
                    model::with(|st| st.get_mut(id).map(|i| i.note = format!("Couldn't extract: {e}")));
                }
            }
        }
    }
    // Notify
    if !it.notified {
        model::with(|st| st.get_mut(id).map(|i| i.notified = true));
        if s.notify_complete {
            notify::show(id, "Download complete", &it.name, true);
        }
    }
}

// ---------------------------------------------------------------- adding

#[derive(serde::Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct HttpAdd {
    pub url: String,
    pub name: String,
    pub folder: String,
    pub category: String,
    pub sha256: String,
    pub referrer: String,
    pub cookies: String,
    pub user_agent: String,
    pub connections: u32,
    pub paused: bool,
    pub priority: i8,
    /// Get the file through Real-Debrid (decided by the caller).
    pub debrid: bool,
}

pub fn add_http(a: HttpAdd) -> Result<u64, String> {
    let url = a.url.trim().to_string();
    let parsed = url::Url::parse(&url).map_err(|_| "That doesn't look like a link".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only http and https links can be downloaded directly".into());
    }
    let sha = a.sha256.trim().to_ascii_lowercase();
    if !sha.is_empty() && (sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit())) {
        return Err("A SHA-256 is 64 hexadecimal characters".into());
    }
    let name = model::sanitize(a.name.trim());
    let id = model::with(|st| {
        st.insert(Item {
            kind: Kind::Http,
            name: if a.name.trim().is_empty() { http::url_name(&url).unwrap_or_else(|| url.clone()) } else { name.clone() },
            file_name: if a.name.trim().is_empty() { String::new() } else { name.clone() },
            // Real-Debrid links get their direct link when they start (see http::run).
            source: if a.debrid { String::new() } else { url.clone() },
            debrid_link: if a.debrid { url.clone() } else { String::new() },
            referrer: a.referrer,
            cookies: a.cookies,
            user_agent: a.user_agent,
            save_dir: a.folder.trim().to_string(),
            category: a.category,
            sha256: sha,
            connections: a.connections,
            paused: a.paused,
            priority: a.priority,
            status: if a.paused { Status::Paused } else { Status::Queued },
            ratio_limit: -1.0,
            seed_time_limit: -1,
            eta: -1,
            ..Default::default()
        })
    });
    Ok(id)
}

#[derive(serde::Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct TorrentAdd {
    /// A magnet link, or a path to a .torrent file.
    pub source: String,
    /// Raw .torrent bytes (from a dropped/opened file in the UI).
    pub bytes: Option<Vec<u8>>,
    /// Skip the file-picking dialog.
    pub direct: bool,
    /// Download through Real-Debrid (None = Settings decide).
    pub debrid: Option<bool>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AddResult {
    pub id: u64,
    pub existing: bool,
}

pub fn add_torrent(a: TorrentAdd) -> Result<AddResult, String> {
    let via_debrid = a.debrid.unwrap_or_else(|| settings::with(|s| s.debrid_torrents == "always")) && debrid::active();
    let src = a.source.trim().to_string();
    let bytes: Vec<u8> = if let Some(b) = a.bytes {
        b
    } else if src.starts_with("magnet:") {
        src.clone().into_bytes()
    } else if src.starts_with("http://") || src.starts_with("https://") {
        return Err("download-torrent-url".into());
    } else {
        let path = src.trim_matches('"');
        std::fs::read(path).map_err(|e| format!("Couldn't open {path}: {e}"))?
    };
    let hash = torrent::quick_hash(&bytes).ok_or("That isn't a valid magnet link or .torrent file")?;
    if let Some(existing) = model::with(|st| st.by_hash(&hash).map(|i| i.id)) {
        return Ok(AddResult { id: existing, existing: true });
    }
    let is_magnet = bytes.starts_with(b"magnet:");
    let name = if is_magnet {
        torrent::magnet_name(&src).unwrap_or_else(|| hash.clone())
    } else {
        librqbit::torrent_from_bytes(&bytes)
            .ok()
            .and_then(|t| t.info.data.name.as_ref().map(|n| String::from_utf8_lossy(n.as_ref()).to_string()))
            .unwrap_or_else(|| hash.clone())
    };
    let show_dialog = settings::with(|s| s.show_add_dialog) && !a.direct;
    if via_debrid && !is_magnet {
        std::fs::create_dir_all(paths::get().torrents()).ok();
        let _ = std::fs::write(torrent::torrent_file_path(&hash), &bytes);
    }
    let id = model::with(|st| {
        st.insert(Item {
            kind: if via_debrid { Kind::Debrid } else { Kind::Torrent },
            name,
            source: if is_magnet { src.clone() } else { format!("magnet:?xt=urn:btih:{hash}") },
            info_hash: hash.clone(),
            status: Status::Metadata,
            ratio_limit: -1.0,
            seed_time_limit: -1,
            eta: -1,
            ..Default::default()
        })
    });
    {
        let mut r = rt();
        if show_dialog {
            r.awaiting.insert(id);
        }
        r.sources.insert(id, bytes);
    }
    if via_debrid {
        start_debrid(id);
    } else {
        start_meta(id);
    }
    Ok(AddResult { id, existing: false })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TorrentMetaView {
    pub id: u64,
    pub name: String,
    pub hash: String,
    pub files: Vec<(String, u64)>,
    pub folder: String,
    pub category: String,
    pub size: u64,
    /// Files are picked for Real-Debrid (no per-file priority).
    pub debrid: bool,
}

fn start_meta(id: u64) {
    if torrent::session().is_none() {
        return; // retried by the tick once the engine runs
    }
    let source = {
        let r = rt();
        r.sources.get(&id).cloned()
    }
    .or_else(|| {
        model::with(|st| st.get(id).map(|i| {
            let file = torrent::torrent_file_path(&i.info_hash);
            std::fs::read(file).unwrap_or_else(|_| i.source.clone().into_bytes())
        }))
    });
    let Some(source) = source else { return };
    let h = tauri::async_runtime::spawn(async move {
        let res = torrent::fetch_meta(&source).await;
        rt().meta.remove(&id);
        match res {
            Ok(meta) => on_meta(id, meta).await,
            Err(e) => {
                // The engine restarting cancels lookups; the tick starts them again.
                if torrent::session().is_some() {
                    set_error(id, &format!("Couldn't get the torrent's file list: {e:#}"));
                }
            }
        }
    });
    let tauri::async_runtime::JoinHandle::Tokio(h) = h;
    rt().meta.insert(id, h);
}

fn suggest_folder(name: &str, files: &[(String, u64)], source: &str) -> (String, String) {
    let s = settings::get();
    let (main, size) = sorter::main_file(files.iter().map(|(p, sz)| (p.as_str(), *sz))).unwrap_or((name, 0));
    let category = sorter::categorize(&s.rules, main, size, source);
    let base = sorter::folder_for(&s, &category);
    (category, base.to_string_lossy().to_string())
}

/// Where a torrent's files go: multi-file torrents get their own folder.
pub fn torrent_dir(base: &str, name: &str, file_count: usize) -> String {
    if file_count > 1 {
        std::path::Path::new(base).join(model::sanitize(name)).to_string_lossy().to_string()
    } else {
        base.to_string()
    }
}

async fn on_meta(id: u64, meta: torrent::Meta) {
    let total: u64 = meta.files.iter().map(|f| f.1).sum();
    let source = model::with(|st| st.get(id).map(|i| i.source.clone())).unwrap_or_default();
    let (category, base) = suggest_folder(&meta.name, &meta.files, &source);
    let awaiting = rt().awaiting.contains(&id);
    rt().sources.remove(&id);
    model::with(|st| {
        if let Some(it) = st.get_mut(id) {
            it.name = meta.name.clone();
            it.size = total;
            it.category = category.clone();
            it.note = if awaiting { "Choose files to start".into() } else { String::new() };
        }
    });
    if awaiting {
        let view = TorrentMetaView { id, name: meta.name.clone(), hash: meta.hash.clone(), files: meta.files.clone(), folder: base, category, size: total, debrid: false };
        rt().pending.push(view.clone());
        emit("torrent-meta", view);
        crate::show_main_window();
    } else {
        let prio = vec![2u8; meta.files.len()];
        let _ = confirm_torrent(TorrentConfirm { id, folder: base, priorities: prio, paused: false, sequential: false, category }).await;
    }
}

#[derive(serde::Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct TorrentConfirm {
    pub id: u64,
    pub folder: String,
    /// One per file: 0 skip, 1 low, 2 normal, 3 high.
    pub priorities: Vec<u8>,
    pub paused: bool,
    pub sequential: bool,
    pub category: String,
}

/// Torrents whose file list is waiting for the user (the UI shows them one at a time).
pub fn pending_meta() -> Vec<TorrentMetaView> {
    rt().pending.clone()
}

pub async fn confirm_torrent(c: TorrentConfirm) -> Result<(), String> {
    {
        let mut r = rt();
        r.awaiting.remove(&c.id);
        r.pending.retain(|p| p.id != c.id);
    }
    let Some(it) = model::with(|st| st.get(c.id).cloned()) else { return Err("gone".into()) };
    if it.kind == Kind::Debrid {
        // Real-Debrid: remember the choice; the job tells Real-Debrid.
        let ids = rt().rd_files.get(&c.id).cloned().unwrap_or_default();
        let picked: Vec<u64> = ids.iter().zip(c.priorities.iter().chain(std::iter::repeat(&2))).filter(|(_, p)| **p > 0).map(|(id, _)| *id).collect();
        if picked.is_empty() {
            return Err("Pick at least one file".into());
        }
        model::with(|st| {
            if let Some(i) = st.get_mut(c.id) {
                i.debrid_files = if picked.len() == ids.len() { vec![] } else { picked.clone() };
                i.save_dir = paths::expand_env(&c.folder);
                if !c.category.is_empty() {
                    i.category = c.category.clone();
                }
                i.paused = c.paused;
                i.note.clear();
            }
        });
        return Ok(());
    }
    let files = torrent::files(&it.info_hash);
    let mut prio = c.priorities.clone();
    prio.resize(files.len(), 2);
    if prio.iter().all(|p| *p == 0) {
        return Err("Pick at least one file".into());
    }
    let dir = torrent_dir(&paths::expand_env(&c.folder), &it.name, files.len());
    let size: u64 = files.iter().zip(&prio).filter(|(_, p)| **p > 0).map(|(f, _)| f.1).sum();
    model::with(|st| {
        if let Some(i) = st.get_mut(c.id) {
            i.save_dir = dir.clone();
            i.file_prio = prio.clone();
            i.sequential = c.sequential;
            i.paused = c.paused;
            i.size = size;
            i.note.clear();
            if !c.category.is_empty() {
                i.category = c.category.clone();
            }
            i.status = if c.paused { Status::Paused } else { Status::Queued };
        }
    });
    // Added paused; the queue starts it when there's a slot.
    if let Err(e) = torrent::add(&it.info_hash, &dir, Some(selected_files(&prio)), true).await {
        set_error(c.id, &format!("{e:#}"));
        return Err(format!("{e:#}"));
    }
    rt().live.remove(&c.id);
    Ok(())
}

/// The user closed the file dialog without adding.
pub async fn cancel_torrent(id: u64) {
    {
        let mut r = rt();
        r.awaiting.remove(&id);
        r.pending.retain(|p| p.id != id);
    }
    remove(vec![id], false).await;
}

// ---------------------------------------------------------------- Real-Debrid torrents

fn start_debrid(id: u64) {
    let h = tauri::async_runtime::spawn(async move {
        if let Err(e) = debrid_job(id).await {
            paths::log(format!("real-debrid {id}: {e}"));
            set_error(id, &e);
        }
    });
    let tauri::async_runtime::JoinHandle::Tokio(h) = h;
    if let Some(old) = rt().debrid_jobs.insert(id, h) {
        old.abort();
    }
}

fn transient(e: &str) -> bool {
    e.starts_with("Couldn't reach") || e.contains("busy")
}

/// Adds the torrent to Real-Debrid (once), lets the user pick files, follows Real-Debrid's
/// progress, then turns the finished files into direct downloads.
async fn debrid_job(id: u64) -> Result<(), String> {
    let Some(it) = model::with(|st| st.get(id).cloned()) else { return Ok(()) };
    let mut rd = it.debrid_id.clone();
    if rd.is_empty() {
        let saved = rt().sources.get(&id).cloned();
        let bytes = saved
            .or_else(|| std::fs::read(torrent::torrent_file_path(&it.info_hash)).ok())
            .unwrap_or_else(|| it.source.clone().into_bytes());
        rd = if bytes.starts_with(b"magnet:") {
            debrid::add_magnet(&String::from_utf8_lossy(&bytes)).await?
        } else {
            debrid::add_torrent_file(bytes).await?
        };
        model::with(|st| st.get_mut(id).map(|i| i.debrid_id = rd.clone()));
        rt().sources.remove(&id);
    }
    let mut selected = false;
    let mut shown_picker = false;
    loop {
        let info = match debrid::info(&rd).await {
            Ok(i) => i,
            Err(e) if transient(&e) => {
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
            Err(e) => return Err(e),
        };
        let phase = debrid::phase(&info.status);
        model::with(|st| {
            if let Some(i) = st.get_mut(id) {
                if !info.filename.is_empty() {
                    i.name = info.filename.clone();
                }
                if info.bytes > 0 {
                    i.size = info.bytes;
                    i.done = ((info.progress.clamp(0.0, 100.0) / 100.0) * info.bytes as f64) as u64;
                }
                i.eta = if info.speed > 0 && i.size > i.done { ((i.size - i.done) / info.speed) as i64 } else { -1 };
                match phase {
                    debrid::Phase::Converting | debrid::Phase::PickFiles => i.status = Status::Metadata,
                    debrid::Phase::Working => i.status = Status::Downloading,
                    _ => {}
                }
                if !matches!(phase, debrid::Phase::PickFiles) || !rt().awaiting.contains(&id) {
                    let speed = if info.speed > 0 { format!(" · {}/s", human(info.speed)) } else { String::new() };
                    i.note = format!("{}{speed}", debrid::status_note(&info));
                }
            }
        });
        match phase {
            debrid::Phase::Converting | debrid::Phase::Working => {}
            debrid::Phase::PickFiles => {
                if rt().awaiting.contains(&id) {
                    if !shown_picker {
                        shown_picker = true;
                        let files: Vec<(String, u64)> = info.files.iter().map(|f| (f.path.trim_start_matches('/').to_string(), f.bytes)).collect();
                        let total: u64 = files.iter().map(|f| f.1).sum();
                        let (category, folder) = suggest_folder(&info.filename, &files, &it.source);
                        rt().rd_files.insert(id, info.files.iter().map(|f| f.id).collect());
                        model::with(|st| {
                            st.get_mut(id).map(|i| {
                                i.size = total;
                                i.category = category.clone();
                                i.note = "Choose files to start".into();
                            })
                        });
                        let view = TorrentMetaView { id, name: info.filename.clone(), hash: info.hash.clone(), files, folder, category, size: total, debrid: true };
                        rt().pending.push(view.clone());
                        emit("torrent-meta", view);
                        crate::show_main_window();
                    }
                } else if !selected {
                    selected = true;
                    let picked = model::with(|st| st.get(id).map(|i| i.debrid_files.clone())).unwrap_or_default();
                    debrid::select_files(&rd, &picked).await?;
                }
            }
            debrid::Phase::Ready => return finish_debrid(id, &info).await,
            debrid::Phase::Failed(e) => return Err(e),
        }
        let wait = if matches!(phase, debrid::Phase::Working) { 4 } else { 2 };
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
}

fn human(b: u64) -> String {
    let mb = b as f64 / 1_048_576.0;
    if mb >= 1.0 { format!("{mb:.1} MB") } else { format!("{:.0} KB", b as f64 / 1024.0) }
}

/// Real-Debrid has the files: each becomes a normal multi-connection download. The first one
/// takes this item's place in the list.
async fn finish_debrid(id: u64, info: &debrid::TorrentInfo) -> Result<(), String> {
    let Some(it) = model::with(|st| st.get(id).cloned()) else { return Ok(()) };
    if info.links.is_empty() {
        return Err("Real-Debrid finished but returned no files".into());
    }
    let selected: Vec<&debrid::RdFile> = info.files.iter().filter(|f| f.selected == 1).collect();
    let (category, base) = if it.save_dir.is_empty() {
        let files: Vec<(String, u64)> = selected.iter().map(|f| (f.path.trim_start_matches('/').to_string(), f.bytes)).collect();
        suggest_folder(&info.filename, &files, &it.source)
    } else {
        (it.category.clone(), it.save_dir.clone())
    };
    let count = selected.len().max(info.links.len());
    let dir = torrent_dir(&base, &info.filename, count);
    let root = model::sanitize(&info.filename);
    let mut entries = Vec::new();
    for (n, link) in info.links.iter().enumerate() {
        model::with(|st| st.get_mut(id).map(|i| i.note = format!("Real-Debrid: getting links {}/{}", n + 1, info.links.len())));
        let u = loop {
            match debrid::unrestrict(link, "").await {
                Ok(u) => break u,
                Err(e) if transient(&e) => tokio::time::sleep(Duration::from_secs(5)).await,
                Err(e) => return Err(e),
            }
        };
        // Keep the torrent's folders when links line up with files (one link per file).
        let rel = if info.links.len() == selected.len() { selected[n].path.trim_start_matches('/').to_string() } else { u.filename.clone() };
        let mut parts: Vec<String> = rel.split('/').filter(|p| !p.is_empty()).map(model::sanitize).collect();
        if parts.len() > 1 && parts[0] == root && count > 1 {
            parts.remove(0); // the torrent's own top folder is already `dir`
        }
        let file = parts.pop().unwrap_or_else(|| model::sanitize(&u.filename));
        let folder = parts.iter().fold(std::path::PathBuf::from(&dir), |p, c| p.join(c));
        entries.push((folder.to_string_lossy().to_string(), file, u.download, link.clone(), u.filesize));
    }
    model::with(|st| {
        for (n, (folder, file, url, link, size)) in entries.into_iter().enumerate() {
            let template = Item {
                kind: Kind::Http,
                name: file.clone(),
                file_name: file,
                source: url,
                debrid_link: link,
                save_dir: folder,
                category: category.clone(),
                size,
                priority: it.priority,
                limit_down: it.limit_down,
                status: Status::Queued,
                ratio_limit: -1.0,
                seed_time_limit: -1,
                eta: -1,
                ..Default::default()
            };
            if n == 0 {
                if let Some(i) = st.get_mut(id) {
                    let added = i.added;
                    *i = Item { id, added, debrid_id: i.debrid_id.clone(), info_hash: i.info_hash.clone(), ..template };
                }
            } else {
                st.insert(template);
            }
        }
    });
    rt().rd_files.remove(&id);
    Ok(())
}

// ---------------------------------------------------------------- actions

pub async fn pause(ids: Vec<u64>) {
    for id in ids {
        let Some(it) = model::with(|st| {
            st.get_mut(id).map(|i| {
                i.paused = true;
                i.clone()
            })
        }) else { continue };
        match it.kind {
            Kind::Http => stop_http_wait(id).await,
            Kind::Torrent => {
                if let Some(h) = rt().meta.remove(&id) {
                    h.abort();
                }
                torrent::pause(&it.info_hash).await;
            }
            // Real-Debrid keeps working on its side; Haul just stops following it.
            Kind::Debrid => {
                if let Some(h) = rt().debrid_jobs.remove(&id) {
                    h.abort();
                }
            }
        }
        model::with(|st| {
            if let Some(i) = st.get_mut(id) {
                if !i.is_finished() {
                    i.status = Status::Paused;
                }
                i.speed_down = 0;
                i.speed_up = 0;
            }
        });
    }
}

pub fn resume(ids: Vec<u64>) {
    model::with(|st| {
        for id in ids {
            if let Some(i) = st.get_mut(id) {
                i.paused = false;
                if i.status == Status::Error {
                    i.error.clear();
                    i.retries = 0;
                }
                if (i.kind == Kind::Torrent && i.file_prio.is_empty()) || i.kind == Kind::Debrid {
                    i.status = Status::Metadata;
                } else if !i.is_finished() {
                    i.status = Status::Queued;
                } else if i.kind == Kind::Torrent {
                    // Resuming a finished torrent means seeding it again.
                    i.status = Status::Seeding;
                    i.note.clear();
                }
            }
        }
    });
}

pub async fn remove(ids: Vec<u64>, delete_files: bool) {
    for id in ids {
        let it = model::with(|st| st.get(id).cloned());
        let Some(it) = it else { continue };
        if let Some(h) = rt().meta.remove(&id) {
            h.abort();
        }
        match it.kind {
            Kind::Http => {
                stop_http_wait(id).await;
                http::remove_partial(&it);
                if delete_files && it.status == Status::Completed {
                    let _ = std::fs::remove_file(it.file_path());
                }
            }
            Kind::Debrid => {
                if let Some(h) = rt().debrid_jobs.remove(&id) {
                    h.abort();
                }
                if !it.debrid_id.is_empty() {
                    debrid::delete(&it.debrid_id).await;
                }
            }
            Kind::Torrent => {
                if !it.info_hash.is_empty() {
                    let others = model::with(|st| st.items.iter().filter(|i| i.id != id && i.info_hash == it.info_hash).count());
                    if others == 0 {
                        torrent::remove(&it.info_hash, delete_files).await;
                    }
                }
                if delete_files && !it.file_prio.is_empty() {
                    // librqbit removes files; tidy the torrent's own folder if now empty.
                    let _ = std::fs::remove_dir(&it.save_dir);
                }
            }
        }
        model::with(|st| st.remove(id));
        let mut r = rt();
        r.sent.remove(&id);
        r.live.remove(&id);
        r.retry_at.remove(&id);
        r.awaiting.remove(&id);
        r.pending.retain(|p| p.id != id);
        r.sources.remove(&id);
        r.rd_files.remove(&id);
    }
    model::save_if_dirty();
}

pub fn retry(ids: Vec<u64>) {
    model::with(|st| {
        for id in &ids {
            if let Some(i) = st.get_mut(*id) {
                i.retries = 0;
                i.error.clear();
                i.paused = false;
                i.hash_state.clear();
                if i.status == Status::Error {
                    i.status = if i.kind == Kind::Torrent && i.file_prio.is_empty() { Status::Metadata } else { Status::Queued };
                }
                // A checksum failure means the file is bad: download it again from scratch.
                if i.kind == Kind::Http && i.done >= i.size && i.size > 0 {
                    let _ = std::fs::remove_file(i.file_path());
                    i.segments.clear();
                    i.done = 0;
                    i.file_name.clear();
                    i.status = Status::Queued;
                    i.notified = false;
                    i.finished_at = 0;
                }
            }
        }
    });
    for id in ids {
        rt().retry_at.remove(&id);
    }
}

/// Moves items in the queue: "top", "up", "down", "bottom".
pub fn move_items(ids: Vec<u64>, dir: &str) {
    let set: HashSet<u64> = ids.iter().copied().collect();
    model::with(|st| {
        let items = &mut st.items;
        let len = items.len();
        match dir {
            "top" | "bottom" => {
                let (sel, rest): (Vec<Item>, Vec<Item>) = items.drain(..).partition(|i| set.contains(&i.id));
                *items = if dir == "top" { sel.into_iter().chain(rest).collect() } else { rest.into_iter().chain(sel).collect() };
            }
            "up" => {
                for i in 1..len {
                    if set.contains(&items[i].id) && !set.contains(&items[i - 1].id) {
                        items.swap(i, i - 1);
                    }
                }
            }
            _ => {
                for i in (0..len.saturating_sub(1)).rev() {
                    if set.contains(&items[i].id) && !set.contains(&items[i + 1].id) {
                        items.swap(i, i + 1);
                    }
                }
            }
        }
    });
    model::mark_dirty();
}

pub fn order() -> Vec<u64> {
    model::with(|st| st.items.iter().map(|i| i.id).collect())
}

#[derive(serde::Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemOptions {
    pub limit_down: Option<u32>,
    pub limit_up: Option<u32>,
    pub priority: Option<i8>,
    pub sequential: Option<bool>,
    pub ratio_limit: Option<f64>,
    pub seed_time_limit: Option<i64>,
    pub connections: Option<u32>,
    pub sha256: Option<String>,
}

pub fn set_options(ids: Vec<u64>, o: ItemOptions) {
    let mut touched = Vec::new();
    model::with(|st| {
        for id in &ids {
            if let Some(i) = st.get_mut(*id) {
                if let Some(v) = o.limit_down {
                    i.limit_down = v;
                }
                if let Some(v) = o.limit_up {
                    i.limit_up = v;
                }
                if let Some(v) = o.priority {
                    i.priority = v.clamp(-1, 1);
                }
                if let Some(v) = o.sequential {
                    i.sequential = v;
                }
                if let Some(v) = o.ratio_limit {
                    i.ratio_limit = v;
                }
                if let Some(v) = o.seed_time_limit {
                    i.seed_time_limit = v;
                }
                if let Some(v) = o.connections {
                    i.connections = v.min(32);
                }
                if let Some(v) = &o.sha256 {
                    i.sha256 = v.trim().to_ascii_lowercase();
                    i.hash_state.clear();
                }
                if i.kind == Kind::Torrent {
                    touched.push((i.info_hash.clone(), i.file_prio.clone(), i.sequential, i.limit_down, i.limit_up));
                }
            }
        }
    });
    for (h, p, s, d, u) in touched {
        torrent::apply_live(&h, &p, s, d, u);
    }
}

pub async fn set_file_priorities(id: u64, prio: Vec<u8>) -> Result<(), String> {
    if prio.iter().all(|p| *p == 0) {
        return Err("Keep at least one file".into());
    }
    let Some((hash, seq, d, u)) = model::with(|st| {
        st.get_mut(id).map(|i| {
            i.file_prio = prio.clone();
            (i.info_hash.clone(), i.sequential, i.limit_down, i.limit_up)
        })
    }) else {
        return Err("gone".into());
    };
    let files = torrent::files(&hash);
    let size: u64 = files.iter().zip(&prio).filter(|(_, p)| **p > 0).map(|(f, _)| f.1).sum();
    model::with(|st| {
        if let Some(i) = st.get_mut(id) {
            i.size = size;
            // Picking more files makes a finished torrent download again.
            if i.is_finished() && i.done < size {
                i.status = Status::Queued;
                i.notified = false;
                i.finished_at = 0;
                i.paused = false;
            }
        }
    });
    torrent::set_only_files(&hash, &selected_files(&prio)).await.map_err(|e| format!("{e:#}"))?;
    torrent::apply_live(&hash, &prio, seq, d, u);
    Ok(())
}

pub fn files_of(id: u64) -> Vec<model::TorrentFile> {
    let Some(it) = model::with(|st| st.get(id).cloned()) else { return vec![] };
    if it.kind != Kind::Torrent {
        return vec![];
    }
    let files = torrent::files(&it.info_hash);
    let done = torrent::stats(&it.info_hash).map(|s| s.file_done).unwrap_or_default();
    files
        .into_iter()
        .enumerate()
        .map(|(i, (path, size))| model::TorrentFile {
            index: i,
            path,
            size,
            done: done.get(i).copied().unwrap_or(0),
            priority: it.file_prio.get(i).copied().unwrap_or(2),
        })
        .collect()
}

/// The path to open for an item (the file itself, or its folder).
pub fn item_path(id: u64) -> Option<std::path::PathBuf> {
    let it = model::with(|st| st.get(id).cloned())?;
    match it.kind {
        Kind::Http => {
            let p = it.file_path();
            if p.exists() { Some(p) } else { Some(std::path::PathBuf::from(&it.save_dir)) }
        }
        Kind::Debrid => Some(std::path::PathBuf::from(&it.save_dir)),
        Kind::Torrent => {
            let files = torrent::files(&it.info_hash);
            let dir = std::path::PathBuf::from(&it.save_dir);
            if files.len() == 1 { Some(dir.join(&files[0].0)) } else { Some(dir) }
        }
    }
}

/// Stops everything cleanly (on quit or before an update).
pub async fn shutdown() {
    let ids: Vec<u64> = rt().http.keys().copied().collect();
    for id in &ids {
        stop_http(*id);
    }
    for id in ids {
        stop_http_wait(id).await;
    }
    model::with(|st| {
        for it in st.items.iter_mut() {
            if it.status == Status::Downloading {
                it.status = Status::Queued;
            }
        }
    });
    model::mark_dirty();
    model::save_if_dirty();
    torrent::shutdown().await;
}

/// Is anything downloading right now (seeding doesn't count)?
pub fn busy() -> bool {
    totals().downloading > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_split() {
        assert_eq!(split_limit(0, 5, 5), (0, 0));
        // Both busy: half each.
        assert_eq!(split_limit(1000_000, 600_000, 600_000), (500_000, 500_000));
        // Torrents idle: direct downloads get almost all of it.
        assert_eq!(split_limit(1000_000, 900_000, 0), (1000_000, 500_000));
    }

    #[test]
    fn torrent_dirs() {
        assert_eq!(torrent_dir("D:/V", "Show: S1", 3), std::path::Path::new("D:/V").join("Show_ S1").to_string_lossy());
        assert_eq!(torrent_dir("D:/V", "x", 1), "D:/V");
    }

    /// The whole Real-Debrid torrent flow against a fake Real-Debrid: add the magnet, wait for
    /// the file list, pick files, follow the download, then turn the result into downloads.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_debrid_torrent_flow() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = std::env::temp_dir().join(format!("haul-rd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LOCALAPPDATA", &dir);

        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let polls = Arc::new(AtomicU32::new(0));
        let selected = Arc::new(Mutex::new(String::new()));
        let (p2, s2) = (polls.clone(), selected.clone());
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = l.accept().await.unwrap();
                let (polls, selected) = (p2.clone(), s2.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let line = req.lines().next().unwrap_or("").to_string();
                    let authed = req.to_ascii_lowercase().contains("authorization: bearer key123");
                    let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                    let (code, json) = if !authed {
                        ("401 Unauthorized", r#"{"error":"bad_token","error_code":8}"#.to_string())
                    } else if line.starts_with("POST /torrents/addMagnet") {
                        ("201 Created", r#"{"id":"T1","uri":"x"}"#.to_string())
                    } else if line.starts_with("POST /torrents/selectFiles/T1") {
                        *selected.lock() = body;
                        ("204 No Content", String::new())
                    } else if line.starts_with("GET /torrents/info/T1") {
                        let n = polls.fetch_add(1, Ordering::SeqCst);
                        let picked = !selected.lock().is_empty();
                        let status = if n == 0 { "magnet_conversion" } else if !picked { "waiting_files_selection" } else if n < 4 { "downloading" } else { "downloaded" };
                        let links = if status == "downloaded" { r#"["https://real-debrid.com/d/AAA","https://real-debrid.com/d/BBB"]"# } else { "[]" };
                        let sel = if picked { 1 } else { 0 };
                        (
                            "200 OK",
                            format!(
                                r#"{{"id":"T1","filename":"My Show","hash":"dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c","bytes":3000,"progress":{},"status":"{status}","speed":2097152,"seeders":9,
                                "files":[{{"id":1,"path":"/My Show/S01/e1.mkv","bytes":2000,"selected":{sel}}},{{"id":2,"path":"/My Show/sample.mkv","bytes":10,"selected":0}},{{"id":3,"path":"/My Show/S01/e2.mkv","bytes":1000,"selected":{sel}}}],
                                "links":{links}}}"#,
                                if status == "downloaded" { 100 } else { 40 }
                            ),
                        )
                    } else if line.starts_with("POST /unrestrict/link") {
                        let which = if body.contains("AAA") { "e1.mkv" } else { "e2.mkv" };
                        ("200 OK", format!(r#"{{"filename":"{which}","filesize":123,"download":"https://download.example/{which}","host":"real-debrid.com"}}"#))
                    } else {
                        ("404 Not Found", r#"{"error":"unknown","error_code":7}"#.to_string())
                    };
                    let out = format!("HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}", json.len());
                    let _ = sock.write_all(out.as_bytes()).await;
                });
            }
        });
        let _ = debrid::TEST_BASE.set(format!("http://127.0.0.1:{port}"));
        settings::update(|s| {
            s.debrid_key = crate::secret::seal("key123");
            s.debrid_enabled = true;
            s.show_add_dialog = true;
            s.download_dir = dir.join("dl").to_string_lossy().to_string();
            s.sort_enabled = false;
        });

        // Add through Real-Debrid with the file picker.
        let magnet = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=My+Show";
        let r = add_torrent(TorrentAdd { source: magnet.into(), debrid: Some(true), ..Default::default() }).unwrap();
        let id = r.id;
        assert_eq!(model::with(|st| st.get(id).unwrap().kind), Kind::Debrid);
        // The picker appears once Real-Debrid has the file list; pick e1 and e2, skip the sample.
        let mut tries = 0;
        while pending_meta().is_empty() {
            tokio::time::sleep(Duration::from_millis(200)).await;
            tries += 1;
            assert!(tries < 100, "file picker never came");
        }
        let meta = pending_meta()[0].clone();
        assert!(meta.debrid);
        assert_eq!(meta.files.len(), 3);
        let out = dir.join("chosen").to_string_lossy().to_string();
        confirm_torrent(TorrentConfirm { id, folder: out.clone(), priorities: vec![2, 0, 2], paused: false, sequential: false, category: "Video".into() })
            .await
            .unwrap();
        // Follow it to the end.
        let mut tries = 0;
        while model::with(|st| st.get(id).map(|i| i.kind)) == Some(Kind::Debrid) {
            tokio::time::sleep(Duration::from_millis(250)).await;
            tries += 1;
            assert!(tries < 200, "never finished: {:?}", model::with(|st| st.get(id).map(|i| (i.status, i.error.clone(), i.note.clone()))));
        }
        assert_eq!(selected.lock().as_str(), "files=1%2C3");
        let items: Vec<Item> = model::with(|st| st.items.iter().filter(|i| i.debrid_link.starts_with("https://real-debrid.com/d/")).cloned().collect());
        assert_eq!(items.len(), 2);
        let first = items.iter().find(|i| i.id == id).expect("the item becomes the first download");
        assert_eq!(first.kind, Kind::Http);
        assert_eq!(first.file_name, "e1.mkv");
        assert_eq!(first.source, "https://download.example/e1.mkv");
        let base = std::path::Path::new(&out).join("My Show");
        assert_eq!(std::path::PathBuf::from(&first.save_dir), base.join("S01"));
        assert_eq!(first.category, "Video");
        let second = items.iter().find(|i| i.id != id).unwrap();
        assert_eq!(second.file_name, "e2.mkv");
        assert_eq!(std::path::PathBuf::from(&second.save_dir), base.join("S01"));

        // A bad key gives a plain-language error.
        assert!(debrid::account_for("wrong").await.unwrap_err().contains("API key"));
    }
}
