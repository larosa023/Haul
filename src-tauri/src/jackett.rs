//! Built-in Jackett, like prowlarr.rs: Haul reads Jackett's address and API key from its own
//! ServerConfig.json, starts it hidden if it isn't running, points it at FlareSolverr if that
//! isn't set yet, and searches every site added in Jackett with one call (its "all" feed).

use std::path::PathBuf;
use std::time::Duration;

use crate::search::Hit;

#[derive(Clone, Debug, PartialEq)]
pub struct Conn {
    pub base: String,
    pub key: String,
}

fn data_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    [env("ProgramData").map(|p| p.join("Jackett")), env("APPDATA").map(|p| p.join("Jackett")), env("LOCALAPPDATA").map(|p| p.join("Jackett"))]
        .into_iter()
        .flatten()
        .find(|d| d.join("ServerConfig.json").is_file())
}

/// The program to start it with (console build, run without a window).
pub fn find() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    for k in ["ProgramData", "ProgramFiles", "LOCALAPPDATA"] {
        let Some(b) = env(k) else { continue };
        for exe in ["JackettConsole.exe", "jackett.exe", "Jackett.exe"] {
            let p = b.join("Jackett").join(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn conn() -> Option<Conn> {
    parse_config(&std::fs::read_to_string(data_dir()?.join("ServerConfig.json")).ok()?)
}

pub fn parse_config(json: &str) -> Option<Conn> {
    let v: serde_json::Value = serde_json::from_str(json.trim_start_matches('\u{feff}')).ok()?;
    let key = v.get("APIKey")?.as_str()?.trim().to_string();
    if key.is_empty() {
        return None;
    }
    let port = v.get("Port").and_then(|p| p.as_u64()).unwrap_or(9117);
    let path = v.get("BasePathOverride").and_then(|p| p.as_str()).unwrap_or("").trim_matches('/').to_string();
    let base = if path.is_empty() { format!("http://127.0.0.1:{port}") } else { format!("http://127.0.0.1:{port}/{path}") };
    Some(Conn { base, key })
}

async fn running(c: &Conn) -> bool {
    crate::http::client().get(format!("{}/UI/Login", c.base)).timeout(Duration::from_secs(2)).send().await.map(|r| r.status().as_u16() < 500).unwrap_or(false)
}

pub async fn ensure() -> Option<Conn> {
    if !crate::settings::with(|s| s.jackett_auto) {
        return None;
    }
    let c = conn()?;
    if !running(&c).await {
        let exe = find()?;
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--NoUpdates");
        if let Some(dir) = exe.parent() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000 | 0x0000_0008);
        }
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        cmd.spawn().ok()?;
        let mut up = false;
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if running(&c).await {
                up = true;
                break;
            }
        }
        if !up {
            return None;
        }
    }
    link_flaresolverr(&c).await;
    Some(c)
}

/// Fills in Jackett's "FlareSolverr API URL" if it's empty (only when Jackett has no admin
/// password, so its settings can be read and saved back unchanged otherwise).
async fn link_flaresolverr(c: &Conn) {
    let url = format!("{}/api/v2.0/server/config", c.base);
    let Ok(r) = crate::http::client().get(&url).timeout(Duration::from_secs(5)).send().await else { return };
    let Ok(mut cfg) = r.json::<serde_json::Value>().await else { return };
    let empty = |k: &str| cfg.get(k).and_then(|v| v.as_str()).map(|s| s.trim().is_empty()).unwrap_or(true);
    if !cfg.is_object() || !empty("password") || !empty("flaresolverrurl") {
        return;
    }
    cfg["flaresolverrurl"] = serde_json::Value::String("http://127.0.0.1:8191".into());
    let _ = crate::http::client().post(&url).json(&cfg).timeout(Duration::from_secs(10)).send().await;
}

pub async fn search(c: &Conn, q: &str, cat: &str) -> Result<Vec<Hit>, String> {
    let mut query = vec![("apikey", c.key.clone()), ("t", "search".to_string()), ("q", q.to_string())];
    let cats = crate::search::torznab_cat(cat);
    if !cats.is_empty() {
        query.push(("cat", cats.to_string()));
    }
    let r = crate::http::client()
        .get(format!("{}/api/v2.0/indexers/all/results/torznab/api", c.base))
        .query(&query)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| if e.is_timeout() { "took too long to answer".to_string() } else { "couldn't be reached".to_string() })?;
    match r.status().as_u16() {
        200..=299 => {}
        401 | 403 => return Err("didn't accept its API key (restart Jackett, then search again)".into()),
        s => return Err(format!("answered with an error ({s})")),
    }
    let text = r.text().await.map_err(|e| e.to_string())?;
    match crate::search::parse_torznab(&text, "Jackett") {
        // No sites added yet: nothing to report.
        Err(e) if e.to_ascii_lowercase().contains("indexer") => Ok(Vec::new()),
        other => other,
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub found: bool,
    pub running: bool,
    /// Sites added in Jackett, or -1 if Jackett didn't say.
    pub sites: i64,
}

pub async fn status() -> Status {
    let Some(c) = conn() else { return Status { found: false, running: false, sites: 0 } };
    let up = running(&c).await;
    let mut sites = -1;
    if up {
        if let Ok(r) = crate::http::client().get(format!("{}/api/v2.0/indexers?configured=true", c.base)).timeout(Duration::from_secs(10)).send().await {
            if let Ok(serde_json::Value::Array(list)) = r.json::<serde_json::Value>().await {
                sites = list.len() as i64;
            }
        }
    }
    Status { found: true, running: up, sites }
}

pub async fn open() -> Result<(), String> {
    let c = match ensure().await {
        Some(c) => c,
        None => conn().ok_or("Jackett isn't installed")?,
    };
    tauri_plugin_opener::open_url(format!("{}/UI/Dashboard", c.base.replace("127.0.0.1", "localhost")), None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_config() {
        let c = parse_config("\u{feff}{\"Port\": 9117, \"APIKey\": \"k1\", \"BasePathOverride\": null}").unwrap();
        assert_eq!(c.base, "http://127.0.0.1:9117");
        assert_eq!(c.key, "k1");
        let c = parse_config("{\"Port\": 9000, \"APIKey\": \"k\", \"BasePathOverride\": \"/jackett\"}").unwrap();
        assert_eq!(c.base, "http://127.0.0.1:9000/jackett");
        assert!(parse_config("{\"Port\": 1}").is_none());
    }
}
