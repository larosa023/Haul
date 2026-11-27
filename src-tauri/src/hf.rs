//! Hugging Face models (and datasets): list a repo's files, download the ones you pick with
//! multiple connections, and check every file: large (LFS) files against their SHA-256, small
//! ones against their git blob SHA-1.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{secret, settings};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Repo {
    /// "org/name"
    pub id: String,
    /// "model" or "dataset"
    pub kind: String,
    pub revision: String,
    /// A file the link pointed at (pre-selected).
    pub file: String,
}

/// Accepts "org/name", "https://huggingface.co/org/name", "hf.co/datasets/org/name",
/// ".../tree/<rev>", ".../blob/<rev>/<file>" and ".../resolve/<rev>/<file>".
pub fn parse(input: &str) -> Result<Repo, String> {
    let t = input.trim().trim_end_matches('/');
    let rest = t
        .strip_prefix("https://")
        .or_else(|| t.strip_prefix("http://"))
        .map(|r| r.trim_start_matches("www."))
        .map(|r| r.strip_prefix("huggingface.co/").or_else(|| r.strip_prefix("hf.co/")).ok_or(()))
        .unwrap_or(Ok(t))
        .map_err(|_| "That isn't a Hugging Face link".to_string())?;
    let mut parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    let mut kind = "model";
    if parts.first() == Some(&"datasets") {
        kind = "dataset";
        parts.remove(0);
    } else if parts.first() == Some(&"models") {
        parts.remove(0);
    }
    if parts.len() < 2 || parts[0].contains(' ') || parts[1].contains(' ') {
        return Err("Use a model name like org/model, or its huggingface.co link".into());
    }
    let id = format!("{}/{}", parts[0], parts[1]);
    let mut revision = "main".to_string();
    let mut file = String::new();
    if parts.len() >= 4 && matches!(parts[2], "tree" | "blob" | "resolve") {
        revision = parts[3].to_string();
        if parts[2] != "tree" && parts.len() > 4 {
            file = parts[4..].join("/");
        }
    }
    Ok(Repo { id, kind: kind.into(), revision, file })
}

fn api_prefix(kind: &str) -> &'static str {
    if kind == "dataset" { "datasets" } else { "models" }
}

pub fn resolve_url(repo: &Repo, path: &str) -> String {
    let prefix = if repo.kind == "dataset" { "datasets/" } else { "" };
    let enc: Vec<String> = path.split('/').map(|p| percent_encoding::utf8_percent_encode(p, percent_encoding::NON_ALPHANUMERIC).to_string()).collect();
    format!("https://huggingface.co/{prefix}{}/resolve/{}/{}", repo.id, repo.revision, enc.join("/"))
}

pub fn token() -> String {
    secret::open(&settings::with(|s| s.hf_token.clone()))
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
struct Lfs {
    oid: String,
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
struct TreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    size: u64,
    oid: String,
    lfs: Option<Lfs>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HfFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub git_sha1: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub repo: Repo,
    pub files: Vec<HfFile>,
}

fn next_link(h: &reqwest::header::HeaderMap) -> Option<String> {
    let v = h.get(reqwest::header::LINK)?.to_str().ok()?;
    v.split(',').find(|p| p.contains("rel=\"next\"")).and_then(|p| p.split(';').next()).map(|u| u.trim().trim_matches(['<', '>']).to_string())
}

pub async fn list(input: &str) -> Result<Listing, String> {
    let repo = parse(input)?;
    let tok = token();
    let mut url = Some(format!("https://huggingface.co/api/{}/{}/tree/{}?recursive=true&expand=false", api_prefix(&repo.kind), repo.id, repo.revision));
    let mut files = Vec::new();
    while let Some(u) = url.take() {
        let mut rq = crate::http::client().get(&u).timeout(Duration::from_secs(30));
        if !tok.is_empty() {
            rq = rq.bearer_auth(&tok);
        }
        let r = rq.send().await.map_err(|e| format!("Couldn't reach Hugging Face: {e}"))?;
        match r.status().as_u16() {
            200 => {}
            401 | 403 => {
                return Err(if tok.is_empty() {
                    "This repo is gated or private. Add a Hugging Face token in Settings → Hugging Face (and accept its terms on the website).".into()
                } else {
                    "Your Hugging Face token can't open this repo (accept its terms on the website first)".into()
                })
            }
            404 => return Err(format!("Hugging Face has no {} called {} (revision {})", repo.kind, repo.id, repo.revision)),
            c => return Err(format!("Hugging Face answered {c}")),
        }
        url = next_link(r.headers());
        let page: Vec<TreeEntry> = r.json().await.map_err(|e| format!("Unexpected answer from Hugging Face: {e}"))?;
        for e in page.into_iter().filter(|e| e.kind == "file") {
            let lfs = e.lfs.map(|l| l.oid).unwrap_or_default();
            files.push(HfFile { sha256: lfs.clone(), git_sha1: if lfs.is_empty() { e.oid } else { String::new() }, path: e.path, size: e.size });
        }
        if files.len() > 50_000 {
            break;
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Listing { repo, files })
}

/// Git's blob hash: SHA-1 of "blob <size>\0" + the content.
pub fn git_blob_sha1(path: &Path) -> std::io::Result<String> {
    use sha1::{Digest, Sha1};
    use std::io::Read;
    let len = std::fs::metadata(path)?.len();
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha1::new();
    h.update(format!("blob {len}\0").as_bytes());
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Checks a token and returns the account name.
pub async fn whoami(token: &str) -> Result<String, String> {
    let r = crate::http::client()
        .get("https://huggingface.co/api/whoami-v2")
        .bearer_auth(token)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Hugging Face: {e}"))?;
    if !r.status().is_success() {
        return Err("Hugging Face didn't accept that token".into());
    }
    let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
    Ok(v.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inputs() {
        let r = parse("meta-llama/Llama-3.1-8B").unwrap();
        assert_eq!((r.id.as_str(), r.kind.as_str(), r.revision.as_str()), ("meta-llama/Llama-3.1-8B", "model", "main"));
        let r = parse("https://huggingface.co/datasets/org/data/tree/v2").unwrap();
        assert_eq!((r.id.as_str(), r.kind.as_str(), r.revision.as_str()), ("org/data", "dataset", "v2"));
        let r = parse("https://hf.co/org/m/blob/main/sub/model.safetensors").unwrap();
        assert_eq!(r.file, "sub/model.safetensors");
        assert!(parse("https://example.com/org/m").is_err());
        assert!(parse("justaname").is_err());
        assert_eq!(resolve_url(&parse("org/m").unwrap(), "a b/c.json"), "https://huggingface.co/org/m/resolve/main/a%20b/c%2Ejson");
    }

    #[test]
    fn git_hash() {
        let p = std::env::temp_dir().join(format!("haul-git-{}", std::process::id()));
        std::fs::write(&p, b"hello\n").unwrap();
        // `git hash-object` of "hello\n"
        assert_eq!(git_blob_sha1(&p).unwrap(), "ce013625030ba8dba906f756967f9e9ca394464a");
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn link_header() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(reqwest::header::LINK, "<https://huggingface.co/api/models/x/tree/main?cursor=abc>; rel=\"next\"".parse().unwrap());
        assert_eq!(next_link(&h).unwrap(), "https://huggingface.co/api/models/x/tree/main?cursor=abc");
    }
}
