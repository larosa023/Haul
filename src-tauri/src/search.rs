//! Torrent search: Internet Archive (built in) and any Torznab indexers the user adds
//! (Prowlarr, Jackett, or a site's own Torznab feed). All sources are asked at once.

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::settings::{self, Indexer};

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub title: String,
    pub size: u64,
    /// -1 when the source doesn't say.
    pub seeders: i64,
    pub leechers: i64,
    /// Where it came from (the indexer's name).
    pub source: String,
    /// A magnet link, or a URL of a .torrent file.
    pub link: String,
    /// A page about it.
    pub info_url: String,
    /// Unix seconds, or 0.
    pub published: i64,
    pub category: String,
    /// Internet Archive: how many times it's been downloaded.
    pub downloads: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    pub hits: Vec<Hit>,
    /// Sources that failed, and why.
    pub errors: Vec<(String, String)>,
}

/// Search categories, as Torznab numbers and Internet Archive media types.
fn torznab_cat(cat: &str) -> &'static str {
    match cat {
        "movies" => "2000",
        "tv" => "5000",
        "music" => "3000",
        "software" => "4000",
        "games" => "1000,4050",
        "books" => "7000",
        _ => "",
    }
}

fn archive_type(cat: &str) -> &'static str {
    match cat {
        "movies" | "tv" => "mediatype:movies",
        "music" => "mediatype:audio",
        "software" | "games" => "mediatype:software",
        "books" => "mediatype:texts",
        _ => "mediatype:(movies OR audio OR software OR texts)",
    }
}

fn cat_name(n: u64) -> String {
    match n / 1000 {
        1 => "Games",
        2 => "Movies",
        3 => "Music",
        4 => "Software",
        5 => "TV",
        7 => "Books",
        _ => "",
    }
    .into()
}

pub async fn search(query: &str, cat: &str) -> Results {
    let q = query.trim();
    if q.is_empty() {
        return Results::default();
    }
    let s = settings::get();
    let mut jobs: Vec<futures_util::future::BoxFuture<'static, (String, Result<Vec<Hit>, String>)>> = Vec::new();
    if s.search_archive {
        let (q, c) = (q.to_string(), cat.to_string());
        jobs.push(Box::pin(async move { ("Internet Archive".to_string(), archive(&q, &c).await) }));
    }
    for ix in s.search_indexers.into_iter().filter(|i| i.enabled && !i.url.trim().is_empty()) {
        let (q, c) = (q.to_string(), cat.to_string());
        jobs.push(Box::pin(async move {
            let name = ix.name.clone();
            (name, torznab(&ix, &q, &c).await)
        }));
    }
    let mut out = Results::default();
    for (name, r) in join_all(jobs).await {
        match r {
            Ok(h) => out.hits.extend(h),
            Err(e) => out.errors.push((name, e)),
        }
    }
    // Same torrent from several sources: keep the one with the most seeders.
    let mut seen = std::collections::HashMap::<String, usize>::new();
    let mut deduped: Vec<Hit> = Vec::new();
    for h in out.hits {
        let k = infohash(&h.link).unwrap_or_else(|| h.link.clone());
        match seen.get(&k) {
            Some(&i) if deduped[i].seeders >= h.seeders => {}
            Some(&i) => deduped[i] = h,
            None => {
                seen.insert(k, deduped.len());
                deduped.push(h);
            }
        }
    }
    // Most seeders first; sources that don't count seeders keep their own relevance order.
    deduped.sort_by(|a, b| b.seeders.cmp(&a.seeders));
    out.hits = deduped;
    out
}

fn infohash(link: &str) -> Option<String> {
    let l = link.to_ascii_lowercase();
    let i = l.find("btih:")?;
    Some(l[i + 5..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect())
}

async fn archive(q: &str, cat: &str) -> Result<Vec<Hit>, String> {
    // Match the title (or subjects), not every word of long descriptions; keep the Archive's
    // own relevance order.
    let words = q.replace(['(', ')', ':', '"'], " ");
    let query = format!("(title:({words}) OR subject:({words})) AND {}", archive_type(cat));
    let r = crate::http::client()
        .get("https://archive.org/advancedsearch.php")
        .query(&[
            ("q", query.as_str()),
            ("fl[]", "identifier"),
            ("fl[]", "title"),
            ("fl[]", "item_size"),
            ("fl[]", "downloads"),
            ("fl[]", "mediatype"),
            ("fl[]", "publicdate"),
            ("rows", "60"),
            ("output", "json"),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Internet Archive: {e}"))?;
    let v: serde_json::Value = r.json().await.map_err(|_| "Internet Archive sent an unexpected answer".to_string())?;
    Ok(v["response"]["docs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let id = d["identifier"].as_str()?;
            let title = match &d["title"] {
                serde_json::Value::Array(a) => a.first().and_then(|t| t.as_str()).unwrap_or(id).to_string(),
                t => t.as_str().unwrap_or(id).to_string(),
            };
            Some(Hit {
                title,
                size: d["item_size"].as_u64().unwrap_or(0),
                seeders: -1,
                leechers: -1,
                source: "Internet Archive".into(),
                link: format!("https://archive.org/download/{id}/{id}_archive.torrent"),
                info_url: format!("https://archive.org/details/{id}"),
                published: d["publicdate"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp()).unwrap_or(0),
                category: match d["mediatype"].as_str().unwrap_or("") {
                    "movies" => "Video",
                    "audio" => "Music",
                    "software" => "Software",
                    "texts" => "Books",
                    _ => "",
                }
                .into(),
                downloads: d["downloads"].as_u64().unwrap_or(0),
            })
        })
        .collect())
}

/// The Torznab API URL to call: accepts the full ".../api" URL Prowlarr and Jackett show.
fn api_url(ix: &Indexer) -> String {
    let u = ix.url.trim().trim_end_matches('/');
    if u.ends_with("/api") || u.contains("/api?") || u.ends_with("/torznab") { u.to_string() } else { format!("{u}/api") }
}

async fn torznab(ix: &Indexer, q: &str, cat: &str) -> Result<Vec<Hit>, String> {
    let mut query = vec![("t", "search".to_string()), ("q", q.to_string())];
    if !ix.key.is_empty() {
        query.push(("apikey", crate::secret::open(&ix.key)));
    }
    let c = torznab_cat(cat);
    if !c.is_empty() {
        query.push(("cat", c.into()));
    }
    // Sites rate-limit busy searches (429): wait and try again, up to twice.
    let mut tries = 0;
    let r = loop {
        let r = crate::http::client()
            .get(api_url(ix))
            .query(&query)
            .timeout(Duration::from_secs(25))
            .send()
            .await
            .map_err(|e| if e.is_timeout() { "took too long to answer".to_string() } else { format!("couldn't be reached ({}). Is Prowlarr running?", e.without_url()) })?;
        if r.status().as_u16() == 429 && tries < 2 {
            tries += 1;
            let wait = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(3).clamp(1, 8);
            tokio::time::sleep(Duration::from_secs(wait)).await;
            continue;
        }
        break r;
    };
    match r.status().as_u16() {
        200..=299 => {}
        429 => return Err("is limiting searches right now (too many in a short time); try again in a minute".into()),
        401 | 403 => return Err("didn't accept the API key".into()),
        s => return Err(format!("answered with an error ({s})")),
    }
    let text = r.text().await.map_err(|e| e.to_string())?;
    parse_torznab(&text, &ix.name)
}

pub fn parse_torznab(xml: &str, source: &str) -> Result<Vec<Hit>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|_| "didn't send a Torznab feed (check the URL)".to_string())?;
    if let Some(err) = doc.descendants().find(|n| n.has_tag_name("error")) {
        let d = err.attribute("description").unwrap_or("error");
        return Err(if d.to_ascii_lowercase().contains("api") { "didn't accept the API key".into() } else { d.to_string() });
    }
    let mut out = Vec::new();
    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let child = |tag: &str| item.children().find(|c| c.tag_name().name() == tag && c.tag_name().namespace().is_none_or(|ns| !ns.contains("torznab")));
        let text = |tag: &str| child(tag).and_then(|c| c.text()).unwrap_or("").trim().to_string();
        let attr = |name: &str| item.children().find(|c| c.tag_name().name() == "attr" && c.attribute("name") == Some(name)).and_then(|c| c.attribute("value")).map(|s| s.to_string());
        let enclosure = item.children().find(|c| c.has_tag_name("enclosure"));
        let magnet = attr("magneturl").filter(|m| m.starts_with("magnet:"));
        let link = magnet
            .or_else(|| Some(text("link")).filter(|l| !l.is_empty()))
            .or_else(|| enclosure.and_then(|e| e.attribute("url")).map(|s| s.to_string()))
            .unwrap_or_default();
        if link.is_empty() {
            continue;
        }
        let size = text("size").parse().ok().or_else(|| attr("size").and_then(|s| s.parse().ok())).or_else(|| enclosure.and_then(|e| e.attribute("length")).and_then(|l| l.parse().ok())).unwrap_or(0);
        let seeders: i64 = attr("seeders").and_then(|s| s.parse().ok()).unwrap_or(-1);
        let peers: i64 = attr("peers").and_then(|s| s.parse().ok()).unwrap_or(-1);
        let cat = attr("category").and_then(|c| c.parse::<u64>().ok()).or_else(|| text("category").parse().ok()).map(cat_name).unwrap_or_default();
        let info = [text("comments"), text("guid")].into_iter().find(|u| u.starts_with("http")).unwrap_or_default();
        out.push(Hit {
            title: text("title"),
            size,
            seeders,
            leechers: if peers >= 0 && seeders >= 0 { (peers - seeders).max(0) } else { attr("leechers").and_then(|s| s.parse().ok()).unwrap_or(-1) },
            source: if let Some(ix) = item.children().find(|c| c.tag_name().name() == "jackettindexer").and_then(|c| c.text()) { ix.to_string() } else { source.to_string() },
            link,
            info_url: info,
            published: chrono::DateTime::parse_from_rfc2822(&text("pubDate")).map(|d| d.timestamp()).unwrap_or(0),
            category: cat,
            downloads: 0,
        });
    }
    Ok(out)
}

/// Checks an indexer works (Torznab "caps").
pub async fn test(ix: &Indexer) -> Result<String, String> {
    let mut query = vec![("t", "caps".to_string())];
    if !ix.key.is_empty() {
        query.push(("apikey", crate::secret::open(&ix.key)));
    }
    let r = crate::http::client().get(api_url(ix)).query(&query).timeout(Duration::from_secs(15)).send().await.map_err(|e| format!("Couldn't reach it ({})", e.without_url()))?;
    let text = r.text().await.unwrap_or_default();
    let doc = roxmltree::Document::parse(&text).map_err(|_| "That URL didn't answer like a Torznab indexer".to_string())?;
    if let Some(err) = doc.descendants().find(|n| n.has_tag_name("error")) {
        return Err(err.attribute("description").unwrap_or("The indexer returned an error").to_string());
    }
    let title = doc.descendants().find(|n| n.has_tag_name("server")).and_then(|n| n.attribute("title")).unwrap_or("Torznab indexer");
    Ok(title.to_string())
}

/// What to add for a result: a magnet, or the .torrent file's bytes. Indexer download links
/// sometimes redirect to a magnet, which a normal HTTP client can't follow.
pub async fn fetch(link: &str) -> Result<Result<String, Vec<u8>>, String> {
    if link.starts_with("magnet:") {
        return Ok(Ok(link.to_string()));
    }
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).user_agent(crate::http::default_user_agent()).build().map_err(|e| e.to_string())?;
    let mut url = link.to_string();
    for _ in 0..6 {
        let r = client.get(&url).timeout(Duration::from_secs(30)).send().await.map_err(|e| format!("Couldn't get the torrent: {}", e.without_url()))?;
        if r.status().is_redirection() {
            let loc = r.headers().get("location").and_then(|l| l.to_str().ok()).unwrap_or("").to_string();
            if loc.starts_with("magnet:") {
                return Ok(Ok(loc));
            }
            url = url::Url::parse(&url).and_then(|b| b.join(&loc)).map(|u| u.to_string()).map_err(|_| "Bad redirect from the indexer".to_string())?;
            continue;
        }
        if !r.status().is_success() {
            return Err(format!("The indexer wouldn't give the torrent ({})", r.status()));
        }
        let b = r.bytes().await.map_err(|e| e.to_string())?;
        if b.len() > 20 << 20 || !b.starts_with(b"d") {
            return Err("That link didn't return a .torrent file".into());
        }
        return Ok(Err(b.to_vec()));
    }
    Err("Too many redirects from the indexer".into())
}

#[derive(Deserialize)]
pub struct IndexerInput {
    pub name: String,
    pub url: String,
    pub key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real network, on CI (HAUL_TEST_NET=1): Internet Archive search, and fetching a result.
    #[test]
    fn real_search_archive() {
        if std::env::var("HAUL_TEST_NET").is_err() {
            return;
        }
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let hits = archive("night of the living dead", "movies").await.expect("search");
            println!("{} hits, first: {:?}", hits.len(), hits.first().map(|h| (&h.title, h.size, &h.link)));
            assert!(!hits.is_empty());
            let got = fetch(&hits[0].link).await.expect("fetch torrent");
            match got {
                Ok(m) => println!("magnet {m}"),
                Err(b) => {
                    println!(".torrent {} bytes", b.len());
                    assert!(b.starts_with(b"d"));
                }
            }
        });
    }

    #[test]
    fn parses_torznab() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:torznab="http://torznab.com/schemas/2015/feed">
<channel><title>Prowlarr</title>
<item><title>Ubuntu 26.04 Desktop amd64</title><guid>https://ix.example/t/1</guid><jackettindexer id="x">LinuxTracker</jackettindexer>
<link>http://127.0.0.1:9696/1/download?link=abc</link><comments>https://ix.example/t/1</comments>
<pubDate>Thu, 23 Apr 2026 10:00:00 +0000</pubDate><size>6114519040</size>
<enclosure url="http://127.0.0.1:9696/1/download?link=abc" length="6114519040" type="application/x-bittorrent"/>
<torznab:attr name="category" value="4000"/><torznab:attr name="seeders" value="812"/><torznab:attr name="peers" value="840"/>
<torznab:attr name="magneturl" value="magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&amp;dn=ubuntu"/>
</item>
<item><title>No magnet</title><link>http://127.0.0.1:9696/1/download?link=def</link><size>100</size></item>
</channel></rss>"#;
        let h = parse_torznab(xml, "Prowlarr").unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].seeders, 812);
        assert_eq!(h[0].leechers, 28);
        assert_eq!(h[0].source, "LinuxTracker");
        assert!(h[0].link.starts_with("magnet:"));
        assert_eq!(h[0].category, "Software");
        assert_eq!(h[0].size, 6114519040);
        assert!(h[0].published > 0);
        assert_eq!(h[1].seeders, -1);
        assert_eq!(infohash(&h[0].link).as_deref(), Some("abcdef0123456789abcdef0123456789abcdef01"));
        let e = parse_torznab(r#"<error code="100" description="Invalid API Key"/>"#, "x").unwrap_err();
        assert!(e.contains("API key"));
    }
}
