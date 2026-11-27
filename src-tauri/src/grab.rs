//! Site grabber: lists every file link on a web page (links, images, video and audio sources,
//! magnets), with type and size, so you can pick what to download.

use std::collections::HashSet;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;

use crate::{settings, sorter};

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub url: String,
    pub name: String,
    pub ext: String,
    /// Sorting category (Video, Music, Archives…), "Torrent" for magnets/.torrent, "Page" for
    /// links to other pages.
    pub category: String,
    /// Bytes; 0 = unknown.
    pub size: u64,
    pub text: String,
}

const PAGE_EXTS: &[&str] = &["html", "htm", "php", "asp", "aspx", "jsp", "cgi", "shtml", "xhtml", "cfm"];

/// Every URL in href/src-like attributes, plus the link text for <a>.
pub fn extract(html: &str, base: &url::Url) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let attrs = ["href", "src", "data-src", "data-href", "data-url", "data-download", "srcset", "poster", "content"];
    let mut i = 0;
    while let Some(pos) = lower[i..].find('=') {
        let eq = i + pos;
        i = eq + 1;
        // attribute name right before '='
        let name_end = lower[..eq].trim_end().len();
        let name_start = lower[..name_end].rfind(|c: char| c.is_whitespace() || c == '<' || c == '"' || c == '\'').map(|p| p + 1).unwrap_or(0);
        let name = &lower[name_start..name_end];
        if !attrs.contains(&name) {
            continue;
        }
        let mut j = eq + 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let (start, end) = match bytes[j] {
            q @ (b'"' | b'\'') => {
                let s = j + 1;
                match html[s..].find(q as char) {
                    Some(e) => (s, s + e),
                    None => break,
                }
            }
            _ => {
                let s = j;
                let e = html[s..].find(|c: char| c.is_whitespace() || c == '>').map(|e| s + e).unwrap_or(html.len());
                (s, e)
            }
        };
        i = end;
        let raw = html[start..end].trim();
        let raw = if name == "srcset" { raw.split(',').next().unwrap_or("").split_whitespace().next().unwrap_or("") } else { raw };
        let raw = raw.replace("&amp;", "&");
        if raw.is_empty() || raw.starts_with('#') || raw.starts_with("javascript:") || raw.starts_with("mailto:") || raw.starts_with("data:") || raw.starts_with("tel:") {
            continue;
        }
        if name == "content" && !raw.contains("://") {
            continue;
        }
        let resolved = if raw.starts_with("magnet:") { raw.clone() } else { match base.join(&raw) { Ok(u) => u.to_string(), Err(_) => continue } };
        // Link text for <a href="...">text</a>
        let text = if name == "href" {
            let close = html[end..].find('>').map(|p| end + p + 1);
            close
                .and_then(|c| html[c..].find("</").map(|e| &html[c..c + e]))
                .map(|t| strip_tags(t).trim().chars().take(120).collect())
                .unwrap_or_default()
        } else {
            String::new()
        };
        out.push((resolved, text));
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn classify(url: &str, text: &str) -> Link {
    let rules = settings::with(|s| s.rules.clone());
    if url.starts_with("magnet:") {
        let name = crate::torrent::magnet_name(url).unwrap_or_else(|| "Magnet link".into());
        return Link { url: url.into(), name, ext: "magnet".into(), category: "Torrent".into(), size: 0, text: text.into() };
    }
    let name = crate::http::url_name(url).unwrap_or_else(|| url.to_string());
    let ext = sorter::extension(&name);
    let scheme_ok = url.starts_with("http://") || url.starts_with("https://") || crate::ftp::is_ftp(url);
    let category = if !scheme_ok {
        "Page".to_string()
    } else if ext == "torrent" {
        "Torrent".into()
    } else if ext.is_empty() || PAGE_EXTS.contains(&ext.as_str()) {
        "Page".into()
    } else {
        sorter::categorize(&rules, &name, 0, url)
    };
    Link { url: url.into(), name, ext, category, size: 0, text: text.into() }
}

pub async fn grab(page: &str) -> Result<Vec<Link>, String> {
    let base = url::Url::parse(page.trim()).map_err(|_| "That doesn't look like a link".to_string())?;
    let r = crate::http::client()
        .get(base.clone())
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Couldn't open the page: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("The site answered {}", r.status()));
    }
    let final_url = r.url().clone();
    let body = r.bytes().await.map_err(|e| e.to_string())?;
    let html = String::from_utf8_lossy(&body[..body.len().min(8 * 1024 * 1024)]).to_string();
    // <base href> changes how relative links resolve.
    let lower = html.to_ascii_lowercase();
    let base = lower
        .find("<base ")
        .and_then(|p| lower[p..].find('>').map(|e| &html[p..p + e + 1]))
        .and_then(|tag| extract(tag, &final_url).into_iter().next())
        .and_then(|(u, _)| url::Url::parse(&u).ok())
        .unwrap_or(final_url);
    let mut seen = HashSet::new();
    let mut links: Vec<Link> = extract(&html, &base)
        .into_iter()
        .filter(|(u, _)| seen.insert(u.split('#').next().unwrap_or(u).to_string()))
        .map(|(u, t)| classify(&u, &t))
        .collect();
    // Sizes for file links (HEAD, a few at a time).
    let idx: Vec<usize> = links.iter().enumerate().filter(|(_, l)| l.category != "Page" && l.url.starts_with("http")).map(|(i, _)| i).take(400).collect();
    let urls: Vec<(usize, String)> = idx.iter().map(|i| (*i, links[*i].url.clone())).collect();
    let sizes: Vec<(usize, u64)> = futures_util::stream::iter(urls)
        .map(|(i, u)| async move {
            let r = crate::http::client().head(&u).timeout(Duration::from_secs(8)).send().await;
            (i, r.ok().filter(|r| r.status().is_success()).and_then(|r| r.content_length()).unwrap_or(0))
        })
        .buffer_unordered(10)
        .collect()
        .await;
    for (i, s) in sizes {
        links[i].size = s;
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links() {
        let base = url::Url::parse("https://example.com/dir/page.html").unwrap();
        let html = r##"<a href="files/a.zip">Archive <b>A</b></a> <img src='/img/b.png'> <a href=c.mp4>c</a>
            <a href="#top">x</a><a href="mailto:x@y">m</a><video poster="p.jpg"><source src="//cdn.example.com/v.webm"></video>
            <a HREF="magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&amp;dn=Big">mag</a>
            <img srcset="s1.jpg 1x, s2.jpg 2x"> <meta property="og:video" content="https://example.com/og.mp4">"##;
        let got = extract(html, &base);
        let urls: Vec<&str> = got.iter().map(|(u, _)| u.as_str()).collect();
        assert!(urls.contains(&"https://example.com/dir/files/a.zip"));
        assert!(urls.contains(&"https://example.com/img/b.png"));
        assert!(urls.contains(&"https://example.com/dir/c.mp4"));
        assert!(urls.contains(&"https://cdn.example.com/v.webm"));
        assert!(urls.contains(&"https://example.com/dir/s1.jpg"));
        assert!(urls.contains(&"https://example.com/og.mp4"));
        assert!(urls.iter().any(|u| u.starts_with("magnet:") && u.contains("&dn=Big")));
        assert!(!urls.iter().any(|u| u.contains("mailto") || u.ends_with("#top")));
        assert_eq!(got.iter().find(|(u, _)| u.ends_with("a.zip")).unwrap().1, "Archive A");
    }

    #[test]
    fn classifies() {
        assert_eq!(classify("https://x.com/a.zip", "").category, "Archives");
        assert_eq!(classify("https://x.com/about.html", "").category, "Page");
        assert_eq!(classify("https://x.com/about", "").category, "Page");
        assert_eq!(classify("https://x.com/a.torrent", "").category, "Torrent");
    }
}
