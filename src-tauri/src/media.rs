//! Video and music downloads through yt-dlp (YouTube and the 1,800+ other sites it supports,
//! plus HLS .m3u8 and DASH .mpd streams, whose segments it fetches in parallel and joins).
//! ffmpeg merges video and audio, converts audio, and embeds subtitles, chapters and
//! thumbnails. Both tools are fetched on first use (see tools.rs).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::http::{Control, Fail};
use crate::{model, paths, settings, sorter, tools};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct MediaOptions {
    /// "video" or "audio"
    pub mode: String,
    /// Highest video height (4320 = 8K); 0 = best available.
    pub height: u32,
    /// Prefer HDR when there's a choice.
    pub hdr: bool,
    /// Audio-only format: "mp3", "m4a" or "opus".
    pub audio_format: String,
    pub subtitles: bool,
    /// Subtitle languages for yt-dlp ("en", "en,es", "all").
    pub sub_langs: String,
    pub chapters: bool,
    pub thumbnail: bool,
    /// SponsorBlock: "off", "remove" (cut the segments out) or "mark" (as chapters).
    pub sponsorblock: String,
}

impl Default for MediaOptions {
    fn default() -> Self {
        Self {
            mode: "video".into(),
            height: 0,
            hdr: false,
            audio_format: "mp3".into(),
            subtitles: false,
            sub_langs: "en".into(),
            chapters: true,
            thumbnail: true,
            sponsorblock: "off".into(),
        }
    }
}

impl MediaOptions {
    pub fn audio(&self) -> bool {
        self.mode == "audio"
    }

    /// Short label for the list ("1080p MP4", "MP3").
    pub fn label(&self) -> String {
        if self.audio() {
            return self.audio_format.to_ascii_uppercase();
        }
        let q = match self.height {
            0 => "Best".to_string(),
            4320 => "8K".into(),
            2160 => "4K".into(),
            h => format!("{h}p"),
        };
        format!("{q}{} MP4", if self.hdr { " HDR" } else { "" })
    }
}

/// yt-dlp's format selection for these options.
pub fn format_args(o: &MediaOptions) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    if o.audio() {
        let fmt = match o.audio_format.as_str() {
            "m4a" | "opus" => o.audio_format.as_str(),
            _ => "mp3",
        };
        a.extend(["-f", "ba/b", "-x", "--audio-format", fmt, "--audio-quality", "0"].map(String::from));
    } else {
        let h = if o.height > 0 { format!("[height<={}]", o.height) } else { String::new() };
        // SDR first unless HDR is wanted; each step falls back to the next.
        let sel = if o.hdr {
            format!("bv*{h}+ba/b{h}/bv*+ba/b")
        } else {
            format!("bv*{h}[dynamic_range=SDR]+ba/bv*{h}+ba/b{h}/bv*+ba/b")
        };
        a.extend(["-f".to_string(), sel, "--merge-output-format".into(), "mp4".into()]);
        if o.subtitles {
            let langs = if o.sub_langs.trim().is_empty() { "en".to_string() } else { o.sub_langs.trim().to_string() };
            a.extend(["--write-subs", "--write-auto-subs", "--sub-langs"].map(String::from));
            a.push(format!("{langs},-live_chat"));
            a.push("--embed-subs".into());
        }
    }
    if o.chapters {
        a.push("--embed-chapters".into());
    }
    if o.thumbnail {
        a.push("--embed-thumbnail".into());
        if !o.audio() {
            a.extend(["--convert-thumbnails", "jpg"].map(String::from));
        }
    }
    match o.sponsorblock.as_str() {
        "remove" => a.extend(["--sponsorblock-remove", "sponsor,selfpromo,interaction"].map(String::from)),
        "mark" => a.extend(["--sponsorblock-mark", "all"].map(String::from)),
        _ => {}
    }
    a
}

fn command(exe: &std::path::Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(exe);
    c.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1").stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    c
}

/// Stops yt-dlp and the ffmpeg it started.
fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

fn base_args() -> Vec<String> {
    let mut a: Vec<String> = ["--encoding", "utf-8", "--no-warnings", "--color", "never", "--ignore-config"].map(String::from).to_vec();
    if tools::ffmpeg().exists() {
        a.push("--ffmpeg-location".into());
        a.push(tools::dir().to_string_lossy().to_string());
    }
    a
}

/// What yt-dlp's errors mean, in plain words, and whether trying again could help.
pub fn classify(err: &str) -> Fail {
    let e = err.trim().trim_start_matches("ERROR:").trim().to_string();
    let l = e.to_ascii_lowercase();
    if l.contains("unsupported url") {
        Fail::Permanent("No video found at this link".into())
    } else if l.contains("private video") || l.contains("video unavailable") || l.contains("has been removed") || l.contains("members-only") {
        Fail::Permanent(e)
    } else if l.contains("sign in to confirm") || l.contains("login required") || l.contains("age") && l.contains("confirm") {
        Fail::Permanent(format!("The site wants you signed in: {e}"))
    } else if l.contains("timed out") || l.contains("connection") || l.contains("unable to download") || l.contains("http error 5") || l.contains("temporarily") {
        Fail::Temporary(e)
    } else if e.is_empty() {
        Fail::Temporary("yt-dlp stopped unexpectedly".into())
    } else {
        Fail::Permanent(e)
    }
}

/// Errors that usually mean yt-dlp needs updating (the site changed).
fn looks_outdated(err: &str) -> bool {
    let l = err.to_ascii_lowercase();
    l.contains("unable to extract")
        || l.contains("unable to parse")
        || l.contains("nsig")
        || l.contains("signature")
        || l.contains("please report this issue")
        || l.contains("update to the latest")
        || l.contains("http error 403")
}

// ---------------------------------------------------------------- looking a link up

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub url: String,
    pub title: String,
    pub duration: f64,
    pub uploader: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    /// "video" or "playlist"
    pub kind: String,
    pub url: String,
    pub title: String,
    pub uploader: String,
    pub duration: f64,
    pub thumbnail: String,
    pub site: String,
    /// Video heights on offer, highest first.
    pub heights: Vec<u32>,
    pub hdr: bool,
    pub subtitles: Vec<String>,
    pub chapters: bool,
    pub is_live: bool,
    /// Estimated size per height (bytes), when known.
    pub sizes: HashMap<u32, u64>,
    pub audio_size: u64,
    pub entries: Vec<Entry>,
}

fn s(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn f(v: &serde_json::Value, k: &str) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0)
}

/// Turns yt-dlp's -J output into what the Add window shows.
pub fn parse_info(v: &serde_json::Value, url: &str) -> MediaInfo {
    let mut info = MediaInfo {
        url: url.to_string(),
        title: s(v, "title"),
        uploader: { let u = s(v, "uploader"); if u.is_empty() { s(v, "channel") } else { u } },
        duration: f(v, "duration"),
        thumbnail: s(v, "thumbnail"),
        site: s(v, "extractor_key"),
        is_live: v.get("is_live").and_then(|x| x.as_bool()).unwrap_or(false),
        ..Default::default()
    };
    let kind = s(v, "_type");
    if kind == "playlist" || kind == "multi_video" {
        info.kind = "playlist".into();
        if let Some(es) = v.get("entries").and_then(|e| e.as_array()) {
            for e in es.iter().filter(|e| !e.is_null()) {
                let mut u = s(e, "url");
                if u.is_empty() {
                    u = s(e, "webpage_url");
                }
                if u.is_empty() {
                    continue;
                }
                info.entries.push(Entry {
                    url: u,
                    title: { let t = s(e, "title"); if t.is_empty() { s(e, "id") } else { t } },
                    duration: f(e, "duration"),
                    uploader: s(e, "uploader"),
                });
            }
        }
        if info.thumbnail.is_empty() {
            if let Some(t) = v.get("thumbnails").and_then(|t| t.as_array()).and_then(|t| t.last()) {
                info.thumbnail = s(t, "url");
            }
        }
        return info;
    }
    info.kind = "video".into();
    let formats = v.get("formats").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let mut heights: Vec<u32> = Vec::new();
    let mut best_audio = 0u64;
    for fm in &formats {
        let size = fm.get("filesize").and_then(|x| x.as_u64()).or_else(|| fm.get("filesize_approx").and_then(|x| x.as_u64())).unwrap_or(0);
        let vcodec = s(fm, "vcodec");
        let acodec = s(fm, "acodec");
        if vcodec != "none" && !vcodec.is_empty() || fm.get("height").and_then(|h| h.as_u64()).is_some() {
            let h = fm.get("height").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            if h > 0 {
                if !heights.contains(&h) {
                    heights.push(h);
                }
                let e = info.sizes.entry(h).or_insert(0);
                *e = (*e).max(size);
            }
            let dr = s(fm, "dynamic_range").to_ascii_uppercase();
            if !dr.is_empty() && dr != "SDR" {
                info.hdr = true;
            }
        } else if acodec != "none" && !acodec.is_empty() {
            best_audio = best_audio.max(size);
        }
    }
    heights.sort_unstable_by(|a, b| b.cmp(a));
    info.heights = heights;
    info.audio_size = best_audio;
    for v2 in info.sizes.values_mut() {
        if *v2 > 0 {
            *v2 += best_audio;
        }
    }
    let mut subs: Vec<String> = v.get("subtitles").and_then(|x| x.as_object()).map(|o| o.keys().cloned().collect()).unwrap_or_default();
    subs.retain(|k| k != "live_chat");
    subs.sort();
    info.subtitles = subs;
    info.chapters = v.get("chapters").and_then(|c| c.as_array()).map(|c| !c.is_empty()).unwrap_or(false);
    info
}

/// Looks a link up with yt-dlp (single video, or a playlist / channel's list of videos).
pub async fn inspect(url: &str) -> Result<MediaInfo, String> {
    tools::ensure(false).await?;
    let run = |update_first: bool| async move {
        if update_first {
            let _ = tools::update_ytdlp(true).await;
        }
        let mut c = command(&tools::ytdlp());
        c.args(base_args()).args(["-J", "--flat-playlist", "--playlist-end", "5000", "--", url]);
        c.stdout(Stdio::piped()).stderr(Stdio::piped());
        let out = tokio::time::timeout(Duration::from_secs(120), c.output())
            .await
            .map_err(|_| "Looking up the link took too long".to_string())?
            .map_err(|e| format!("Couldn't run yt-dlp: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).lines().filter(|l| l.contains("ERROR")).last().unwrap_or("").to_string();
            return Err(err);
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("Unexpected answer from yt-dlp: {e}"))?;
        Ok(parse_info(&v, url))
    };
    match run(false).await {
        Ok(i) => Ok(i),
        Err(e) if looks_outdated(&e) => run(true).await.map_err(|e| classify(&e).text().to_string()),
        Err(e) => Err(classify(&e).text().to_string()),
    }
}

// ---------------------------------------------------------------- downloading

/// Progress line yt-dlp prints for us (see --progress-template below).
pub struct Progress {
    pub format: String,
    pub done: u64,
    pub total: u64,
}

pub fn parse_progress(line: &str) -> Option<Progress> {
    let rest = line.strip_prefix("HAULP ")?;
    let p: Vec<&str> = rest.split('|').collect();
    if p.len() < 6 {
        return None;
    }
    let num = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).map(|v| v as u64);
    let done = num(p[1])?;
    let total = num(p[2]).or_else(|| num(p[3])).unwrap_or(0);
    Some(Progress { format: p[5].trim().to_string(), done, total })
}

/// What yt-dlp is doing after the download (shown under the name).
fn phase_note(line: &str) -> Option<&'static str> {
    let tag = line.split(']').next()?.trim_start_matches('[');
    Some(match tag {
        "Merger" => "Merging video and audio…",
        "ExtractAudio" => "Converting audio…",
        "VideoConvertor" | "VideoRemuxer" => "Converting…",
        "SponsorBlock" | "ModifyChapters" => "Removing sponsor segments…",
        "EmbedSubtitle" | "SubtitlesConvertor" => "Adding subtitles…",
        "EmbedThumbnail" | "ThumbnailsConvertor" => "Adding the thumbnail…",
        "Metadata" | "FixupM3u8" | "FixupM4a" | "FixupStretched" | "FixupDuplicateMoov" => "Finishing…",
        "hlsnative" => "Downloading stream segments…",
        "dashsegments" => "Downloading stream segments…",
        _ => return None,
    })
}

/// Runs one media download until it finishes, fails or is paused.
pub async fn run(id: u64, ctl: Arc<Control>) -> Result<(), Fail> {
    let note = |t: &str| {
        let t = t.to_string();
        model::with(|st| st.get_mut(id).map(|i| i.note = t));
    };
    if !tools::ready() {
        note("Getting yt-dlp and ffmpeg (first time only)…");
    }
    tools::ensure(true).await.map_err(Fail::Temporary)?;
    let mut updated = false;
    loop {
        match run_once(id, &ctl).await {
            Err(Fail::Permanent(e)) if !updated && looks_outdated(&e) && settings::with(|s| s.ytdlp_auto_update) => {
                // Sites change and break yt-dlp; a fresh yt-dlp usually fixes it.
                updated = true;
                note("Updating yt-dlp…");
                if !tools::update_ytdlp(true).await.unwrap_or(false) {
                    return Err(Fail::Permanent(e));
                }
            }
            other => return other,
        }
    }
}

async fn run_once(id: u64, ctl: &Arc<Control>) -> Result<(), Fail> {
    let Some(item) = model::with(|st| st.get(id).cloned()) else { return Ok(()) };
    let opts = item.media.clone().unwrap_or_default();
    let s = settings::get();
    // Folder: the sorting rules, as for a .mp4 / .mp3 from this site.
    let folder = if item.save_dir.is_empty() {
        let probe = if opts.audio() { "x.mp3" } else { "x.mp4" };
        let category = sorter::categorize(&s.rules, probe, 0, &item.source);
        let dir = sorter::folder_for(&s, &category).to_string_lossy().to_string();
        model::with(|st| {
            st.get_mut(id).map(|i| {
                i.category = category;
                i.save_dir = dir.clone();
            })
        });
        dir
    } else {
        item.save_dir.clone()
    };
    let _ = std::fs::create_dir_all(&folder);
    let conns = if item.connections > 0 { item.connections } else if s.connections > 0 { s.connections } else { 8 }.clamp(1, 32);
    let limit = if item.limit_down > 0 { item.limit_down as u64 } else { settings::effective_limits(&s).0 / 1024 };

    let mut c = command(&tools::ytdlp());
    c.args(base_args());
    c.args([
        "--newline",
        "--no-playlist",
        "--continue",
        "--no-mtime",
        "--embed-metadata",
        "--windows-filenames",
        "--retries",
        "10",
        "--fragment-retries",
        "10",
        "-N",
        &conns.to_string(),
        "-P",
        &folder,
        "-o",
        "%(title).180B [%(id)s].%(ext)s",
        "--progress-template",
        "download:HAULP %(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(info.format_id)s",
        "--print",
        "before_dl:HAULT %(title)s",
        "--print",
        "after_move:HAULF %(filepath)s",
        "--no-simulate",
    ]);
    if limit > 0 {
        c.args(["--limit-rate", &format!("{limit}K")]);
    }
    if !item.referrer.is_empty() {
        c.args(["--referer", &item.referrer]);
    }
    c.args(format_args(&opts));
    c.args(["--", &item.source]);
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| Fail::Permanent(format!("Couldn't run yt-dlp: {e}")))?;
    let pid = child.id();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let last_error = Arc::new(parking_lot::Mutex::new(String::new()));
    let le = last_error.clone();
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            if l.contains("ERROR") {
                *le.lock() = l;
            }
        }
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut per_format: HashMap<String, (u64, u64)> = HashMap::new();
    let mut final_path: Option<PathBuf> = None;
    let mut best = 0u64;
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                if let Some(p) = parse_progress(&line) {
                    let e = per_format.entry(p.format).or_insert((0, 0));
                    e.0 = p.done;
                    if p.total > 0 { e.1 = p.total; }
                    let done: u64 = per_format.values().map(|v| v.0).sum();
                    let total: u64 = per_format.values().map(|v| v.1.max(v.0)).sum();
                    best = best.max(done);
                    ctl.downloaded.store(best, Ordering::Relaxed);
                    model::with(|st| st.get_mut(id).map(|i| {
                        i.done = done;
                        if total > 0 { i.size = total; }
                        if i.note.starts_with("Getting") || i.note.ends_with('…') { i.note.clear(); }
                    }));
                } else if let Some(t) = line.strip_prefix("HAULT ") {
                    let t = t.trim().to_string();
                    if !t.is_empty() && t != "NA" {
                        model::with(|st| st.get_mut(id).map(|i| i.name = t.clone()));
                    }
                } else if let Some(p) = line.strip_prefix("HAULF ") {
                    final_path = Some(PathBuf::from(p.trim()));
                } else if let Some(n) = phase_note(&line) {
                    model::with(|st| st.get_mut(id).map(|i| i.note = n.to_string()));
                }
            }
            _ = tick.tick() => {
                if ctl.stopped() {
                    kill_tree(pid);
                    let _ = child.kill().await;
                    return Ok(());
                }
            }
        }
    }
    let status = child.wait().await.map_err(|e| Fail::Temporary(e.to_string()))?;
    let _ = err_task.await;
    if ctl.stopped() {
        return Ok(());
    }
    if !status.success() {
        let e = last_error.lock().clone();
        paths::log(format!("media {id}: {e}"));
        return Err(classify(&e));
    }
    let path = final_path.ok_or_else(|| Fail::Permanent("yt-dlp finished without a file (nothing to download?)".into()))?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    model::with(|st| {
        st.get_mut(id).map(|i| {
            i.file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if let Some(dir) = path.parent() {
                i.save_dir = dir.to_string_lossy().to_string();
            }
            i.size = size;
            i.done = size;
            i.note.clear();
        })
    });
    Ok(())
}

/// Sites yt-dlp is likely to handle; other links are only looked up when they turn out to be
/// a web page or a stream.
pub fn is_media_site(url: &str) -> bool {
    let Some(host) = url::Url::parse(url.trim()).ok().and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())) else { return false };
    let host = host.trim_start_matches("www.").trim_start_matches("m.").trim_start_matches("music.");
    const SITES: &[&str] = &[
        "youtube.com", "youtu.be", "vimeo.com", "dailymotion.com", "twitch.tv", "tiktok.com", "instagram.com", "facebook.com",
        "fb.watch", "x.com", "twitter.com", "reddit.com", "v.redd.it", "soundcloud.com", "bandcamp.com", "bilibili.com",
        "nicovideo.jp", "rumble.com", "odysee.com", "streamable.com", "mixcloud.com", "bitchute.com", "kick.com",
        "archive.org", "ted.com", "vk.com", "ok.ru", "threads.net", "bsky.app", "pinterest.com", "imgur.com",
    ];
    SITES.iter().any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

pub fn is_stream(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    let p = l.split(['?', '#']).next().unwrap_or("");
    p.ends_with(".m3u8") || p.ends_with(".mpd")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_selection() {
        let mut o = MediaOptions { height: 1080, ..Default::default() };
        let a = format_args(&o).join(" ");
        assert!(a.contains("bv*[height<=1080][dynamic_range=SDR]+ba"));
        assert!(a.contains("--merge-output-format mp4"));
        assert!(a.contains("--embed-chapters") && a.contains("--embed-thumbnail"));
        o.hdr = true;
        o.height = 4320;
        o.subtitles = true;
        o.sub_langs = "en,es".into();
        o.sponsorblock = "remove".into();
        let a = format_args(&o).join(" ");
        assert!(a.starts_with("-f bv*[height<=4320]+ba"));
        assert!(a.contains("--sub-langs en,es,-live_chat --embed-subs"));
        assert!(a.contains("--sponsorblock-remove sponsor"));
        let au = MediaOptions { mode: "audio".into(), audio_format: "opus".into(), sponsorblock: "mark".into(), ..Default::default() };
        let a = format_args(&au).join(" ");
        assert!(a.contains("-x --audio-format opus"));
        assert!(!a.contains("--embed-subs"));
        assert!(a.contains("--sponsorblock-mark all"));
        assert_eq!(au.label(), "OPUS");
        assert_eq!(MediaOptions { height: 2160, hdr: true, ..Default::default() }.label(), "4K HDR MP4");
    }

    #[test]
    fn progress_lines() {
        let p = parse_progress("HAULP downloading|1048576|NA|10485760.5|524288.0|137").unwrap();
        assert_eq!((p.done, p.total, p.format.as_str()), (1048576, 10485760, "137"));
        assert!(parse_progress("[download] 5%").is_none());
        assert_eq!(phase_note("[Merger] Merging formats into \"x.mp4\""), Some("Merging video and audio…"));
    }

    #[test]
    fn parses_yt_dlp_json() {
        let v: serde_json::Value = serde_json::json!({
            "_type": "video", "title": "Clip", "uploader": "Someone", "duration": 61.0, "extractor_key": "Youtube",
            "formats": [
                {"format_id": "140", "vcodec": "none", "acodec": "mp4a.40.2", "filesize": 1000},
                {"format_id": "137", "vcodec": "avc1", "acodec": "none", "height": 1080, "filesize": 50000, "dynamic_range": "SDR"},
                {"format_id": "701", "vcodec": "av01", "acodec": "none", "height": 2160, "filesize_approx": 90000, "dynamic_range": "HDR10"},
                {"format_id": "18", "vcodec": "avc1", "acodec": "mp4a", "height": 360}
            ],
            "subtitles": {"en": [], "live_chat": []}, "chapters": [{"title": "a"}]
        });
        let i = parse_info(&v, "u");
        assert_eq!(i.kind, "video");
        assert_eq!(i.heights, vec![2160, 1080, 360]);
        assert!(i.hdr && i.chapters);
        assert_eq!(i.subtitles, vec!["en".to_string()]);
        assert_eq!(i.sizes[&1080], 51000);
        let pl = serde_json::json!({"_type": "playlist", "title": "List", "entries": [
            {"url": "https://www.youtube.com/watch?v=a", "title": "A", "duration": 10.0}, null, {"id": "b", "url": "https://www.youtube.com/watch?v=b"}
        ]});
        let i = parse_info(&pl, "u");
        assert_eq!(i.kind, "playlist");
        assert_eq!(i.entries.len(), 2);
        assert_eq!(i.entries[1].title, "b");
    }

    #[test]
    fn sites_and_errors() {
        assert!(is_media_site("https://www.youtube.com/watch?v=x"));
        assert!(is_media_site("https://music.youtube.com/watch?v=x"));
        assert!(!is_media_site("https://example.com/a.mp4"));
        assert!(is_stream("https://cdn.x/master.m3u8?token=1"));
        assert!(matches!(classify("ERROR: Unsupported URL: https://x"), Fail::Permanent(_)));
        assert!(matches!(classify("ERROR: Connection timed out"), Fail::Temporary(_)));
        assert!(looks_outdated("ERROR: [youtube] x: Unable to extract nsig"));
    }
}

/// Real network checks, run on CI (HAUL_TEST_NET=1): installs yt-dlp and ffmpeg from their
/// releases (checking SHA-256), then downloads and joins a public HLS test stream.
#[cfg(test)]
mod net {
    use super::*;

    #[test]
    fn real_tools_and_hls() {
        if std::env::var("HAUL_TEST_NET").is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("haul-net-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LOCALAPPDATA", &dir);
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async {
            crate::tools::ensure(true).await.expect("install tools");
            assert!(crate::tools::ytdlp().is_file() && crate::tools::ffmpeg().is_file());
            println!("tools {:?}", crate::tools::state());
            let url = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";
            assert!(is_stream(url));
            let info = inspect(url).await.expect("inspect");
            println!("{} heights {:?}", info.title, info.heights);
            assert_eq!(info.kind, "video");
            let out = dir.join("out");
            let opts = MediaOptions { height: 360, thumbnail: false, chapters: false, ..Default::default() };
            let mut c = command(&crate::tools::ytdlp());
            c.args(format_args(&opts))
                .args(["--ffmpeg-location", &crate::tools::ffmpeg().to_string_lossy(), "-N", "8", "--download-sections", "*0-20", "-P"])
                .arg(&out)
                .args(["-o", "hls.%(ext)s", url]);
            let st = tokio::time::timeout(std::time::Duration::from_secs(300), c.status()).await.expect("timeout").expect("run");
            assert!(st.success());
            let f = std::fs::read_dir(&out).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with("hls.")).expect("output");
            let len = f.metadata().unwrap().len();
            println!("{} {} bytes", f.path().display(), len);
            assert!(len > 100_000);
        });
    }
}
