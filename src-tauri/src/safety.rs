//! Safety score: an A–F grade for a download, with the reasons behind it.
//!
//! Before downloading, only the link can be judged (encryption, the site, the file type, names
//! made to look like something else). After downloading, Haul looks at the file itself: does
//! the content match its name, is a program signed, what's inside an archive, and Windows
//! Defender scans it. With a VirusTotal key, the file's SHA-256 (never the file) is looked up.
//! Nothing else leaves the PC.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Finding {
    /// "good", "info", "warn" or "bad".
    pub level: String,
    pub text: String,
    /// Points taken off 100.
    pub points: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Report {
    pub score: u32,
    pub grade: String,
    /// "link" (before downloading) or "file" (scanned).
    pub stage: String,
    /// Windows Defender: "clean", "threat", "unavailable" or "" (not scanned).
    pub defender: String,
    pub findings: Vec<Finding>,
    pub checked_at: i64,
}

fn f(level: &str, points: u32, text: impl Into<String>) -> Finding {
    Finding { level: level.into(), text: text.into(), points }
}

impl Report {
    fn build(stage: &str, mut findings: Vec<Finding>, cap: Option<u32>) -> Report {
        // Worst first, then the good news.
        let rank = |l: &str| match l {
            "bad" => 0,
            "warn" => 1,
            "info" => 2,
            _ => 3,
        };
        findings.sort_by_key(|x| (rank(&x.level), std::cmp::Reverse(x.points)));
        let lost: u32 = findings.iter().map(|x| x.points).sum();
        let mut score = 100u32.saturating_sub(lost);
        if let Some(c) = cap {
            score = score.min(c);
        }
        // Any serious problem means at most a D, however good the rest looks.
        if findings.iter().any(|x| x.level == "bad") {
            score = score.min(45);
        }
        let grade = match score {
            90.. => "A",
            75..=89 => "B",
            60..=74 => "C",
            40..=59 => "D",
            _ => "F",
        };
        Report { score, grade: grade.into(), stage: stage.into(), defender: String::new(), findings, checked_at: chrono::Utc::now().timestamp() }
    }
}

// ---------------------------------------------------------------- file types

const PROGRAMS: &[&str] = &[
    "exe", "scr", "com", "pif", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "msi", "msix", "msixbundle", "appx",
    "appxbundle", "lnk", "url", "reg", "jar", "dll", "cpl", "msc", "scf", "inf", "gadget", "application", "appref-ms", "sys", "chm", "xll", "library-ms",
    "search-ms", "settingcontent-ms",
];
const MACRO_DOCS: &[&str] = &["docm", "dotm", "xlsm", "xltm", "xlam", "pptm", "potm", "ppam", "sldm"];
const DISK_IMAGES: &[&str] = &["iso", "img", "vhd", "vhdx", "udf"];
const ARCHIVES: &[&str] = &["zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "xz", "cab", "zst"];
const LOW_RISK: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv", "flv", "ts", "mp3", "m4a", "flac", "wav", "ogg", "opus", "aac", "jpg", "jpeg", "png", "gif",
    "webp", "bmp", "tif", "tiff", "heic", "avif", "txt", "srt", "vtt", "ass", "csv", "json", "md", "epub", "pdf", "docx", "xlsx", "pptx", "odt",
    "ods", "safetensors", "gguf", "onnx", "torrent", "nfo", "bin", "dmg", "deb", "rpm", "apk", "pt", "pth", "ckpt",
];
const LOOKS_LIKE: &[&str] = &["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "jpg", "jpeg", "png", "gif", "mp4", "mkv", "avi", "mp3", "zip", "rar", "7z", "csv", "rtf"];

pub fn ext(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).filter(|e| e.len() <= 18 && !e.contains(' ')).unwrap_or_default()
}

fn is_program(e: &str) -> bool {
    PROGRAMS.contains(&e)
}

/// Checks a file name on its own (also used for files inside archives).
fn name_findings(name: &str, out: &mut Vec<Finding>) {
    let e = ext(name);
    if name.contains('\u{202E}') || name.contains('\u{202D}') {
        out.push(f("bad", 55, "The name contains a hidden character that reverses text, a trick to disguise the real file type"));
    }
    // "invoice.pdf.exe", "photo.jpg      .scr"
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or("");
    let inner = ext(stem.trim_end());
    if is_program(&e) && LOOKS_LIKE.contains(&inner.as_str()) {
        out.push(f("bad", 50, format!("Disguised: named like a .{inner} file but it's a .{e} program")));
    } else if is_program(&e) && stem.ends_with("   ") {
        out.push(f("bad", 40, "Lots of spaces before the extension, often used to hide that it's a program"));
    }
    if is_program(&e) {
        out.push(f("warn", 22, format!("A .{e} file is a program or script: it can change your PC. Only run it if you trust where it came from")));
    } else if MACRO_DOCS.contains(&e.as_str()) {
        out.push(f("warn", 15, format!("A .{e} Office file can contain macros that run code")));
    } else if DISK_IMAGES.contains(&e.as_str()) {
        out.push(f("warn", 10, format!("A .{e} disk image can hide programs and skip some Windows warnings")));
    } else if ARCHIVES.contains(&e.as_str()) {
        out.push(f("info", 3, "An archive: what's inside is checked after it downloads"));
    } else if LOW_RISK.contains(&e.as_str()) {
        out.push(f("good", 0, format!("A .{e} file is a low-risk type (it can't run by itself)")));
    } else if e.is_empty() {
        out.push(f("info", 4, "No file extension, so its type isn't clear from the name"));
    }
}

// ---------------------------------------------------------------- the link

const SHORTENERS: &[&str] = &["bit.ly", "tinyurl.com", "t.co", "goo.gl", "is.gd", "cutt.ly", "rb.gy", "shorturl.at", "ow.ly", "buff.ly", "tiny.cc", "s.id", "v.gd"];
const RISKY_TLDS: &[&str] = &["zip", "mov", "xyz", "top", "click", "country", "gq", "tk", "ml", "cf", "ga", "work", "loan", "rest", "cam", "monster", "quest", "lol", "sbs", "cfd"];
const KNOWN: &[&str] = &[
    "github.com", "githubusercontent.com", "microsoft.com", "windowsupdate.com", "apple.com", "google.com", "googleapis.com", "mozilla.org", "mozilla.net",
    "python.org", "pypi.org", "nodejs.org", "huggingface.co", "hf.co", "sourceforge.net", "ubuntu.com", "debian.org", "fedoraproject.org", "archive.org",
    "videolan.org", "blender.org", "steampowered.com", "nvidia.com", "amd.com", "intel.com", "oracle.com", "jetbrains.com", "adobe.com", "gitlab.com",
    "kernel.org", "apache.org", "gnu.org", "7-zip.org", "rarlab.com", "real-debrid.com", "youtube.com", "googlevideo.com", "proton.me", "obsproject.com",
];

/// The registrable part of a host ("dl.cdn.example.co.uk" → roughly "example.co.uk").
fn site(host: &str) -> String {
    let parts: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    if parts.len() <= 2 {
        return host.to_string();
    }
    let n = parts.len();
    let two_level = parts[n - 2].len() <= 3 && parts[n - 1].len() == 2; // co.uk, com.au
    parts[n - if two_level { 3 } else { 2 }..].join(".")
}

fn known(host: &str) -> bool {
    let s = site(host);
    KNOWN.iter().any(|k| s == *k || host.ends_with(&format!(".{k}")) || host == *k)
}

/// What can be told before downloading.
pub fn link_findings(url: &str, final_url: &str, name: &str, content_type: &str, size: u64) -> Vec<Finding> {
    let mut out = Vec::new();
    let Ok(u) = url::Url::parse(if final_url.is_empty() { url } else { final_url }) else {
        return out;
    };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    match u.scheme() {
        "https" | "ftps" | "ftpes" => out.push(f("good", 0, "Encrypted connection, so the file can't be swapped on the way")),
        "http" => out.push(f("warn", 10, "Not encrypted (http): someone on the network could swap the file")),
        "ftp" => out.push(f("warn", 8, "Plain FTP isn't encrypted: someone on the network could swap the file")),
        _ => {}
    }
    if u.host().is_some_and(|h| matches!(h, url::Host::Ipv4(_) | url::Host::Ipv6(_))) {
        out.push(f("warn", 15, "The link is a bare IP address instead of a website name"));
    }
    if host.split('.').any(|p| p.starts_with("xn--")) {
        out.push(f("warn", 20, format!("The site name ({host}) uses look-alike characters that can imitate another site")));
    }
    if let Some(tld) = host.rsplit('.').next() {
        if RISKY_TLDS.contains(&tld) {
            out.push(f("warn", 8, format!("The .{tld} domain ending is cheap and often used for scams")));
        }
    }
    if let Ok(orig) = url::Url::parse(url) {
        let oh = orig.host_str().unwrap_or("").to_ascii_lowercase();
        if SHORTENERS.contains(&oh.as_str()) {
            out.push(f("warn", 8, format!("A short link ({oh}) hides where the file really comes from")));
        }
        if !final_url.is_empty() && !oh.is_empty() && site(&oh) != site(&host) && !SHORTENERS.contains(&oh.as_str()) {
            out.push(f("info", 2, format!("Redirected from {} to {}", site(&oh), site(&host))));
        }
    }
    if !host.is_empty() && known(&host) {
        out.push(f("good", 0, format!("Comes from a well-known site ({})", site(&host))));
    }
    name_findings(name, &mut out);
    // The server says it's a program, but the name says something else.
    let ct = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let e = ext(name);
    let ct_program = matches!(ct.as_str(), "application/x-msdownload" | "application/x-dosexec" | "application/vnd.microsoft.portable-executable" | "application/x-msi" | "application/x-ms-installer");
    if ct_program && !is_program(&e) {
        out.retain(|x| !x.text.contains("low-risk type"));
        out.push(f("bad", 40, format!("The server says it's a Windows program, but it's named .{e}")));
    }
    if matches!(e.as_str(), "mp4" | "mkv" | "avi" | "mov") && size > 0 && size < 512 * 1024 {
        out.push(f("warn", 8, "Very small for a video, so it may not be what it says"));
    }
    out
}

pub fn link_report(url: &str, final_url: &str, name: &str, content_type: &str, size: u64) -> Report {
    Report::build("link", link_findings(url, final_url, name, content_type, size), None)
}

// ---------------------------------------------------------------- the file

/// What the first bytes say the file really is.
pub fn sniff(head: &[u8]) -> &'static str {
    let s = |p: &[u8]| head.starts_with(p);
    if s(b"MZ") {
        "exe"
    } else if s(b"PK\x03\x04") || s(b"PK\x05\x06") {
        "zip"
    } else if s(b"7z\xBC\xAF\x27\x1C") {
        "7z"
    } else if s(b"Rar!\x1A\x07") {
        "rar"
    } else if s(b"%PDF") {
        "pdf"
    } else if s(b"\x89PNG") {
        "png"
    } else if s(b"\xFF\xD8\xFF") {
        "jpg"
    } else if s(b"GIF8") {
        "gif"
    } else if head.len() > 11 && &head[4..8] == b"ftyp" {
        "mp4"
    } else if s(b"\x1A\x45\xDF\xA3") {
        "mkv"
    } else if s(b"ID3") || (head.len() > 1 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0) {
        "mp3"
    } else if s(b"fLaC") {
        "flac"
    } else if s(b"OggS") {
        "ogg"
    } else if s(b"RIFF") {
        "riff"
    } else if s(b"\xD0\xCF\x11\xE0") {
        "ole"
    } else if s(b"\x1F\x8B") {
        "gz"
    } else if s(b"\x7FELF") {
        "elf"
    } else if s(b"MSCF") {
        "cab"
    } else if head.len() > 0x8006 && &head[0x8001..0x8006] == b"CD001" {
        "iso"
    } else {
        let t = String::from_utf8_lossy(&head[..head.len().min(512)]).trim_start().to_ascii_lowercase();
        if t.starts_with("<!doctype html") || t.starts_with("<html") {
            "html"
        } else if t.starts_with("#!") || t.starts_with("@echo") {
            "script"
        } else {
            ""
        }
    }
}

/// Extensions that the sniffed kind fits.
fn fits(kind: &str, e: &str) -> bool {
    match kind {
        "exe" => matches!(e, "exe" | "dll" | "scr" | "sys" | "cpl" | "com" | "efi" | "ocx" | "mui" | "node" | "pyd" | "bin"),
        "zip" => matches!(e, "zip" | "docx" | "xlsx" | "pptx" | "docm" | "xlsm" | "pptm" | "jar" | "apk" | "epub" | "odt" | "ods" | "odp" | "msix" | "appx" | "nupkg" | "whl" | "xpi" | "crx" | "cbz" | "kmz" | "ipa" | "aab" | "vsix" | "3mf"),
        "ole" => matches!(e, "msi" | "doc" | "xls" | "ppt" | "msg" | "msp"),
        "riff" => matches!(e, "wav" | "avi" | "webp" | "ani"),
        "mp4" => matches!(e, "mp4" | "m4a" | "m4v" | "mov" | "3gp" | "heic" | "avif" | "m4b"),
        "mkv" => matches!(e, "mkv" | "webm" | "mka"),
        "jpg" => matches!(e, "jpg" | "jpeg" | "jfif"),
        "ogg" => matches!(e, "ogg" | "opus" | "oga" | "ogv"),
        "gz" => matches!(e, "gz" | "tgz"),
        "iso" => matches!(e, "iso" | "img"),
        "mp3" => matches!(e, "mp3"),
        k => k == e,
    }
}

fn kind_name(k: &str) -> &'static str {
    match k {
        "exe" => "a Windows program",
        "zip" => "a ZIP archive",
        "7z" => "a 7-Zip archive",
        "rar" => "a RAR archive",
        "pdf" => "a PDF",
        "png" | "jpg" | "gif" => "an image",
        "mp4" | "mkv" => "a video",
        "mp3" | "flac" | "ogg" => "audio",
        "ole" => "an installer or old Office file",
        "html" => "a web page",
        "script" => "a script",
        "elf" => "a Linux program",
        "iso" => "a disk image",
        _ => "something else",
    }
}

fn read_head(path: &Path) -> Vec<u8> {
    use std::io::Read;
    let mut buf = vec![0u8; 0x8010];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)).unwrap_or(0);
    buf.truncate(n);
    buf
}

fn content_findings(path: &Path, name: &str, out: &mut Vec<Finding>) -> Option<u32> {
    let head = read_head(path);
    let e = ext(name);
    let kind = sniff(&head);
    if kind.is_empty() || e.is_empty() {
        return None;
    }
    if kind == "exe" && !fits("exe", &e) && !is_program(&e) {
        out.retain(|x| !x.text.contains("low-risk type"));
        out.push(f("bad", 60, format!("It's really a Windows program disguised as a .{e} file")));
        return Some(10);
    }
    if kind == "html" && !matches!(e.as_str(), "html" | "htm" | "xhtml" | "mhtml") {
        out.push(f("warn", 15, format!("The file is a web page, not a .{e}: the link may have expired or need a sign-in")));
        return None;
    }
    if fits(kind, &e) || (is_program(&e) && matches!(kind, "exe" | "ole" | "script" | "zip")) {
        out.push(f("good", 0, format!("Its contents match the .{e} name")));
    } else if !matches!(kind, "riff" | "gz") {
        out.push(f("warn", 15, format!("Its contents look like {} rather than a .{e} file", kind_name(kind))));
    }
    None
}

/// Programs: is it signed, by whom, and is the signature intact?
#[cfg(windows)]
fn signature_findings(path: &Path, out: &mut Vec<Finding>) {
    use base64::Engine;
    use std::os::windows::process::CommandExt;
    // -EncodedCommand avoids every quoting pitfall; the path comes in through an env variable.
    let script = "$s = Get-AuthenticodeSignature -LiteralPath $env:HAUL_SIG; [Console]::Out.Write($s.Status.ToString() + '|' + $s.SignerCertificate.Subject)";
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let res = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &base64::engine::general_purpose::STANDARD.encode(utf16)])
        .env("HAUL_SIG", path)
        .creation_flags(0x0800_0000)
        .output();
    let Ok(o) = res else { return };
    let text = String::from_utf8_lossy(&o.stdout).trim().trim_start_matches('\u{FEFF}').to_string();
    if cfg!(test) {
        println!("powershell: code {:?} out {text:?} err {:?}", o.status.code(), String::from_utf8_lossy(&o.stderr));
    }
    let (status, subject) = text.split_once('|').unwrap_or((&text, ""));
    let signer = subject
        .split(',')
        .find_map(|p| p.trim().strip_prefix("CN="))
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_default();
    match status {
        "Valid" => {
            // A program from a verified publisher is much less of a worry.
            for x in out.iter_mut().filter(|x| x.text.contains("is a program or script")) {
                x.points = 8;
                x.level = "info".into();
            }
            out.push(f("good", 0, if signer.is_empty() { "Digitally signed, and the signature is intact".to_string() } else { format!("Digitally signed by {signer}, and the signature is intact") }));
        }
        "NotSigned" => out.push(f("warn", 18, "Not digitally signed, so who made it can't be verified")),
        "HashMismatch" => out.push(f("bad", 50, "The signature doesn't match: the program was changed after it was signed")),
        "NotTrusted" | "UnknownError" => out.push(f("warn", 25, if signer.is_empty() { "Signed, but by a certificate Windows doesn't trust".to_string() } else { format!("Signed by {signer}, but Windows doesn't trust that certificate") })),
        _ => {}
    }
}

#[cfg(not(windows))]
fn signature_findings(_path: &Path, _out: &mut Vec<Finding>) {}

/// Archives: programs, disguised names and passwords inside.
fn archive_findings(path: &Path, name: &str, out: &mut Vec<Finding>) {
    let e = ext(name);
    let mut names: Vec<String> = Vec::new();
    let mut encrypted = false;
    let mut readable = true;
    if e == "zip" || sniff(&read_head(path)) == "zip" && ARCHIVES.contains(&e.as_str()) {
        match std::fs::File::open(path).ok().and_then(|f| zip::ZipArchive::new(std::io::BufReader::new(f)).ok()) {
            Some(mut z) => {
                for i in 0..z.len().min(20_000) {
                    if let Ok(entry) = z.by_index_raw(i) {
                        encrypted |= entry.encrypted();
                        names.push(entry.name().to_string());
                    }
                }
            }
            None => readable = false,
        }
    } else if matches!(e.as_str(), "7z" | "rar" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "cab") && !crate::extract::is_secondary_part(name) {
        let mut cmd = std::process::Command::new(crate::extract::system_tar());
        cmd.arg("-tf").arg(path);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        match cmd.output() {
            Ok(o) if o.status.success() => names = String::from_utf8_lossy(&o.stdout).lines().map(|l| l.to_string()).collect(),
            Ok(o) => {
                readable = false;
                encrypted = String::from_utf8_lossy(&o.stderr).to_ascii_lowercase().contains("encrypt") || String::from_utf8_lossy(&o.stderr).to_ascii_lowercase().contains("passphrase");
            }
            Err(_) => readable = false,
        }
    } else {
        return;
    }
    if encrypted {
        out.push(f("warn", 15, "Password-protected: virus scanners can't look inside"));
    } else if !readable {
        out.push(f("info", 5, "Haul couldn't look inside this archive"));
        return;
    }
    let files: Vec<&String> = names.iter().filter(|n| !n.ends_with('/')).collect();
    let programs: Vec<&String> = files.iter().copied().filter(|n| is_program(&ext(n))).collect();
    let mut disguised = Vec::new();
    for n in &files {
        let mut tmp = Vec::new();
        name_findings(n.rsplit(['/', '\\']).next().unwrap_or(n), &mut tmp);
        if tmp.iter().any(|x| x.level == "bad") {
            disguised.push(n.rsplit(['/', '\\']).next().unwrap_or(n).to_string());
        }
    }
    if !disguised.is_empty() {
        out.push(f("bad", 50, format!("Inside: {} disguised as a different file type ({})", if disguised.len() == 1 { "a program" } else { "programs" }, disguised.iter().take(3).cloned().collect::<Vec<_>>().join(", "))));
    }
    if !programs.is_empty() {
        let shown: Vec<String> = programs.iter().take(3).map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n).to_string()).collect();
        let more = if programs.len() > 3 { format!(" and {} more", programs.len() - 3) } else { String::new() };
        out.push(f("warn", 12, format!("Contains {} program{} ({}{more})", programs.len(), if programs.len() == 1 { "" } else { "s" }, shown.join(", "))));
    } else if !files.is_empty() && !encrypted {
        out.push(f("good", 0, format!("Inside: {} file{}, none of them programs", files.len(), if files.len() == 1 { "" } else { "s" })));
    }
}

/// Windows Defender's command-line scanner (no admin needed for a custom scan).
#[cfg(windows)]
fn defender_exe() -> Option<std::path::PathBuf> {
    let platform = std::env::var_os("ProgramData").map(|p| std::path::PathBuf::from(p).join(r"Microsoft\Windows Defender\Platform"));
    if let Some(dir) = platform {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut v: Vec<_> = rd.flatten().map(|e| e.path().join("MpCmdRun.exe")).filter(|p| p.is_file()).collect();
            v.sort();
            if let Some(p) = v.pop() {
                return Some(p);
            }
        }
    }
    let pf = std::env::var_os("ProgramFiles").map(|p| std::path::PathBuf::from(p).join(r"Windows Defender\MpCmdRun.exe"))?;
    pf.is_file().then_some(pf)
}

/// Scans with Defender. Returns ("clean" | "threat" | "unavailable", threat names).
#[cfg(windows)]
pub fn defender_scan(path: &Path) -> (String, Vec<String>) {
    use std::os::windows::process::CommandExt;
    let Some(exe) = defender_exe() else { return ("unavailable".into(), vec![]) };
    let out = std::process::Command::new(exe)
        .args(["-Scan", "-ScanType", "3", "-File"])
        .arg(path)
        .creation_flags(0x0800_0000)
        .output();
    let Ok(o) = out else { return ("unavailable".into(), vec![]) };
    let text = String::from_utf8_lossy(&o.stdout).to_string();
    let threats: Vec<String> = text
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix("Threat").and_then(|r| r.trim_start().strip_prefix(':')).map(|t| t.trim().to_string())
        })
        .filter(|t| !t.is_empty())
        .collect();
    match o.status.code() {
        Some(0) => ("clean".into(), vec![]),
        Some(2) => ("threat".into(), threats),
        _ if !threats.is_empty() => ("threat".into(), threats),
        _ => ("unavailable".into(), vec![]),
    }
}

#[cfg(not(windows))]
pub fn defender_scan(_path: &Path) -> (String, Vec<String>) {
    ("unavailable".into(), vec![])
}

/// VirusTotal lookup by SHA-256 only (with the user's own key).
fn virustotal(sha: &str, key: &str, out: &mut Vec<Finding>) -> Option<u32> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
    let res = rt.block_on(async {
        crate::http::client()
            .get(format!("https://www.virustotal.com/api/v3/files/{sha}"))
            .header("x-apikey", key)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
    });
    let Ok(r) = res else {
        out.push(f("info", 0, "VirusTotal didn't answer"));
        return None;
    };
    match r.status().as_u16() {
        404 => {
            out.push(f("info", 3, "VirusTotal hasn't seen this file before (rare files are more often new or custom-made)"));
            None
        }
        401 | 403 => {
            out.push(f("info", 0, "VirusTotal refused the API key"));
            None
        }
        200 => {
            let v: serde_json::Value = rt.block_on(r.json()).unwrap_or_default();
            let st = &v["data"]["attributes"]["last_analysis_stats"];
            let n = |k: &str| st[k].as_u64().unwrap_or(0);
            let bad = n("malicious");
            let sus = n("suspicious");
            let total = bad + sus + n("harmless") + n("undetected");
            if bad >= 3 {
                out.push(f("bad", 60, format!("VirusTotal: {bad} of {total} antivirus engines flag it as malicious")));
                Some(15)
            } else if bad + sus > 0 {
                out.push(f("warn", 12 * (bad + sus) as u32, format!("VirusTotal: {} of {total} engines flag it (could be a false alarm)", bad + sus)));
                None
            } else {
                out.push(f("good", 0, format!("VirusTotal: none of {total} antivirus engines flag it")));
                None
            }
        }
        429 => {
            out.push(f("info", 0, "VirusTotal's free limit was reached; try again in a minute"));
            None
        }
        _ => None,
    }
}

/// Full check of a finished file or folder. Runs on a blocking thread (Defender can take a while).
pub fn scan(path: &Path, name: &str, source: &str, final_url: &str) -> Report {
    let mut out = if source.is_empty() { Vec::new() } else { link_findings(source, final_url, name, "", 0) };
    // The link's guesses about the type are replaced by what the file really is.
    out.retain(|x| !x.text.contains("checked after it downloads"));
    let mut cap: Option<u32> = None;
    let mut tighten = |c: Option<u32>| {
        if let Some(c) = c {
            cap = Some(cap.map_or(c, |x: u32| x.min(c)));
        }
    };
    let is_dir = path.is_dir();
    if !is_dir {
        tighten(content_findings(path, name, &mut out));
        let kind = sniff(&read_head(path));
        if kind == "exe" || (kind == "ole" && ext(name) == "msi") || ext(name) == "msi" {
            signature_findings(path, &mut out);
        }
        archive_findings(path, name, &mut out);
    } else {
        // A torrent folder: look for programs among its files.
        let mut programs = Vec::new();
        let mut bad = Vec::new();
        for e in walk(path, 20_000) {
            let n = e.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let mut tmp = Vec::new();
            name_findings(&n, &mut tmp);
            if tmp.iter().any(|x| x.level == "bad") {
                bad.push(n.clone());
            }
            if is_program(&ext(&n)) {
                programs.push(n);
            } else if !e.is_dir() && sniff(&read_head(&e)) == "exe" && !fits("exe", &ext(&n)) {
                bad.push(n);
            }
        }
        if !bad.is_empty() {
            out.push(f("bad", 55, format!("Disguised program{}: {}", if bad.len() == 1 { "" } else { "s" }, bad.iter().take(3).cloned().collect::<Vec<_>>().join(", "))));
            tighten(Some(15));
        }
        if !programs.is_empty() {
            out.push(f("warn", 15, format!("Contains {} program{} ({})", programs.len(), if programs.len() == 1 { "" } else { "s" }, programs.iter().take(3).cloned().collect::<Vec<_>>().join(", "))));
        } else if bad.is_empty() {
            out.push(f("good", 0, "No programs among the files"));
        }
    }
    let s = crate::settings::get();
    let (defender, threats) = if s.safety_defender { defender_scan(path) } else { (String::new(), vec![]) };
    match defender.as_str() {
        "clean" => out.push(f("good", 0, "Windows Defender scanned it and found nothing")),
        "threat" => {
            let what = if threats.is_empty() { "a threat".to_string() } else { threats.join(", ") };
            out.push(f("bad", 100, format!("Windows Defender found {what} (Defender removes or quarantines it)")));
            tighten(Some(0));
        }
        "unavailable" => out.push(f("info", 0, "Windows Defender couldn't scan it here (another antivirus may be in charge)")),
        _ => {}
    }
    let key = crate::secret::open(&s.vt_key);
    if !key.is_empty() && !is_dir && path.is_file() {
        if let Ok(sha) = crate::http::sha256_file(path) {
            tighten(virustotal(&sha, &key, &mut out));
        }
    }
    let mut r = Report::build("file", out, cap);
    r.defender = defender;
    r
}

fn walk(dir: &Path, cap: usize) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
            if out.len() >= cap {
                return out;
            }
        }
    }
    out
}

/// Marks a downloaded file as coming from the internet (like browsers do), so Windows
/// SmartScreen and Office Protected View check it when it's opened.
#[cfg(windows)]
pub fn mark_of_the_web(path: &Path, url: &str, referrer: &str) {
    if !path.is_file() {
        return;
    }
    let mut ads = path.as_os_str().to_owned();
    ads.push(":Zone.Identifier");
    let clean = |s: &str| s.replace(['\r', '\n'], "");
    let mut text = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if !referrer.is_empty() {
        text += &format!("ReferrerUrl={}\r\n", clean(referrer));
    }
    if url.starts_with("http") {
        text += &format!("HostUrl={}\r\n", clean(url));
    }
    let _ = std::fs::write(std::path::PathBuf::from(ads), text);
}

#[cfg(not(windows))]
pub fn mark_of_the_web(_path: &Path, _url: &str, _referrer: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grades_links() {
        let r = link_report("https://github.com/x/y/releases/download/v1/tool.zip", "", "tool.zip", "", 0);
        assert_eq!(r.grade, "A", "{r:?}");
        let r = link_report("http://203.0.113.9/invoice.pdf.exe", "", "invoice.pdf.exe", "", 0);
        assert_eq!(r.grade, "F", "{r:?}");
        assert!(r.findings[0].level == "bad");
        let r = link_report("https://example.com/setup.exe", "", "setup.exe", "", 0);
        assert!(matches!(r.grade.as_str(), "B" | "C"), "{r:?}");
        let r = link_report("https://xn--pple-43d.com/a.mp4", "", "a.mp4", "", 10 << 20);
        assert!(r.findings.iter().any(|x| x.text.contains("look-alike")));
        let r = link_report("https://example.com/movie.mp4", "", "movie.mp4", "application/x-msdownload", 0);
        assert!(r.score < 60, "{r:?}");
        let r = link_report("https://bit.ly/abc", "https://files.example.top/a.zip", "a.zip", "", 0);
        assert!(r.findings.iter().any(|x| x.text.contains("short link")) && r.findings.iter().any(|x| x.text.contains(".top")));
        let mut v = Vec::new();
        name_findings("photo\u{202E}gpj.exe", &mut v);
        assert!(v.iter().any(|x| x.level == "bad"));
    }

    #[test]
    fn checks_files() {
        let dir = std::env::temp_dir().join(format!("haul-safety-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A program pretending to be a video.
        let fake = dir.join("clip.mp4");
        std::fs::write(&fake, b"MZ\x90\x00 this is a program").unwrap();
        let r = scan(&fake, "clip.mp4", "https://example.com/clip.mp4", "");
        assert_eq!(r.grade, "F", "{r:?}");
        assert!(r.findings.iter().any(|x| x.text.contains("disguised")));
        // A real PNG.
        let png = dir.join("a.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n....").unwrap();
        let r = scan(&png, "a.png", "https://example.com/a.png", "");
        assert_eq!(r.grade, "A", "{r:?}");
        // A zip hiding a disguised program.
        let zp = dir.join("docs.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zp).unwrap());
            let o: zip::write::SimpleFileOptions = Default::default();
            z.start_file("readme.txt", o).unwrap();
            z.start_file("invoice.pdf.exe", o).unwrap();
            z.finish().unwrap();
        }
        let r = scan(&zp, "docs.zip", "https://example.com/docs.zip", "");
        assert!(r.score < 40, "{r:?}");
        assert!(r.findings.iter().any(|x| x.text.contains("invoice.pdf.exe")));
        // An expired link that saved a web page.
        let html = dir.join("data.zip");
        std::fs::write(&html, b"<!DOCTYPE html><html><body>Sign in</body></html>").unwrap();
        let r = scan(&html, "data.zip", "", "");
        assert!(r.findings.iter().any(|x| x.text.contains("web page")), "{r:?}");
    }
}

/// On Windows CI (HAUL_TEST_NET=1): Defender and the signature check on real files.
#[cfg(all(test, windows))]
mod real {
    use super::*;

    #[test]
    fn real_safety_windows() {
        if std::env::var("HAUL_TEST_NET").is_err() {
            return;
        }
        let notepad = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap()).join(r"System32\notepad.exe");
        let mut v = Vec::new();
        signature_findings(&notepad, &mut v);
        println!("notepad: {v:?}");
        assert!(v.iter().any(|x| x.level == "good" && x.text.contains("Microsoft")), "signature check failed: {v:?}");
        let dir = std::env::temp_dir().join(format!("haul-safe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("a.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n....").unwrap();
        let (d, t) = defender_scan(&png);
        println!("defender: {d} {t:?}");
        assert!(d == "clean" || d == "unavailable");
        let r = scan(&notepad, "notepad.exe", "https://example.com/notepad.exe", "");
        println!("{r:#?}");
    }
}
