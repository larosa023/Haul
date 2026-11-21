//! Notices download links copied to the clipboard and offers to grab them.

use std::time::Duration;

/// Is this a link worth offering? Magnets, .torrent links and links to files with a
/// download-type extension.
pub fn looks_downloadable(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.len() > 8000 || t.contains('\n') || t.contains(' ') {
        return false;
    }
    if t.starts_with("magnet:?") {
        return t.contains("xt=urn:bt");
    }
    let Ok(u) = url::Url::parse(t) else { return false };
    if !matches!(u.scheme(), "http" | "https") {
        return false;
    }
    let name = crate::http::url_name(t).unwrap_or_default();
    let ext = crate::sorter::extension(&name);
    if ext.is_empty() {
        return false;
    }
    if ext == "torrent" {
        return true;
    }
    // Pages aren't downloads.
    if matches!(ext.as_str(), "html" | "htm" | "php" | "asp" | "aspx" | "jsp" | "cgi") {
        return false;
    }
    crate::settings::with(|s| s.rules.iter().any(|r| r.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext))))
}

#[cfg(windows)]
fn sequence() -> u32 {
    unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

#[cfg(windows)]
fn read_text() -> Option<String> {
    clipboard_win::get_clipboard_string().ok()
}

#[cfg(not(windows))]
fn sequence() -> u32 {
    0
}

#[cfg(not(windows))]
fn read_text() -> Option<String> {
    None
}

pub async fn watch() {
    let mut last_seq = sequence();
    let mut last_offered = String::new();
    loop {
        tokio::time::sleep(Duration::from_millis(800)).await;
        if !crate::settings::with(|s| s.watch_clipboard) {
            last_seq = sequence();
            continue;
        }
        let seq = sequence();
        if seq == last_seq {
            continue;
        }
        last_seq = seq;
        let Some(text) = tokio::task::spawn_blocking(read_text).await.ok().flatten() else { continue };
        let text = text.trim().to_string();
        if text == last_offered || !looks_downloadable(&text) {
            continue;
        }
        // Skip links already in the list.
        let known = crate::model::with(|s| s.items.iter().any(|i| i.source == text));
        if known {
            continue;
        }
        last_offered = text.clone();
        crate::engine::emit("clipboard-link", serde_json::json!({ "url": text }));
    }
}

#[cfg(test)]
mod tests {
    use super::looks_downloadable;

    #[test]
    fn detects() {
        assert!(looks_downloadable("https://example.com/files/setup.exe"));
        assert!(looks_downloadable("https://x.org/a/b.torrent"));
        assert!(looks_downloadable("magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"));
        assert!(!looks_downloadable("https://example.com/page.html"));
        assert!(!looks_downloadable("https://example.com/"));
        assert!(!looks_downloadable("hello world"));
        assert!(!looks_downloadable("magnet:?dn=x"));
    }
}
