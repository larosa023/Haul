//! FTP and FTPS downloads. They use the same parallel, resumable engine as HTTP (http.rs): each
//! connection logs in, jumps to its offset (REST) and reads its part.
//!
//! - `ftp://` plain FTP
//! - `ftps://` implicit TLS (port 990), or explicit TLS when the port is 21
//! - `ftpes://` explicit TLS (AUTH TLS on port 21)
//!
//! User and password come from the link (`ftp://user:pass@host/path`); otherwise anonymous.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use suppaftp::tokio::{AsyncRustlsConnector, AsyncRustlsFtpStream, ImplAsyncFtpStream};
use suppaftp::types::FileType;
use tokio::io::AsyncReadExt;

use crate::http::{ByteStream, Fail, ProbeInfo};
use crate::{model, settings, sorter};

pub fn is_ftp(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    l.starts_with("ftp://") || l.starts_with("ftps://") || l.starts_with("ftpes://")
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tls {
    None,
    Explicit,
    Implicit,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub path: String,
    pub tls: Tls,
}

pub fn parse(url: &str) -> Result<Target, Fail> {
    let u = url::Url::parse(url.trim()).map_err(|_| Fail::Permanent("That isn't a valid FTP link".into()))?;
    let host = u.host_str().ok_or_else(|| Fail::Permanent("The FTP link has no server".into()))?.to_string();
    let (tls, default_port) = match u.scheme() {
        "ftp" => (Tls::None, 21),
        "ftpes" => (Tls::Explicit, 21),
        "ftps" => {
            if u.port() == Some(21) { (Tls::Explicit, 21) } else { (Tls::Implicit, 990) }
        }
        _ => return Err(Fail::Permanent("Not an FTP link".into())),
    };
    let dec = |s: &str| percent_encoding::percent_decode_str(s).decode_utf8_lossy().to_string();
    let path = dec(u.path());
    if path.is_empty() || path.ends_with('/') {
        return Err(Fail::Permanent("That FTP link is a folder, not a file".into()));
    }
    Ok(Target {
        host,
        port: u.port().unwrap_or(default_port),
        user: if u.username().is_empty() { "anonymous".into() } else { dec(u.username()) },
        pass: u.password().map(dec).unwrap_or_else(|| if u.username().is_empty() { "haul@".into() } else { String::new() }),
        path,
        tls,
    })
}

fn tls_connector() -> AsyncRustlsConnector {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("tls versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    AsyncRustlsConnector::from(tokio_rustls::TlsConnector::from(Arc::new(cfg)))
}

fn ftp_fail(e: suppaftp::FtpError) -> Fail {
    use suppaftp::FtpError::*;
    match &e {
        ConnectionError(_) | SecureError(_) => Fail::Temporary(format!("FTP connection failed: {e}")),
        UnexpectedResponse(r) => {
            let code = r.status.code();
            let text = format!("The FTP server answered {code}: {}", String::from_utf8_lossy(&r.body).trim());
            // 4xx = try again later (busy, too many connections); 5xx = won't work.
            if (400..500).contains(&code) { Fail::Temporary(text) } else { Fail::Permanent(text) }
        }
        _ => Fail::Temporary(format!("FTP: {e}")),
    }
}

async fn connect(t: &Target) -> Result<AsyncRustlsFtpStream, Fail> {
    let addr = (t.host.as_str(), t.port);
    let fut = async {
        let mut ftp = match t.tls {
            Tls::Implicit => ImplAsyncFtpStream::connect_secure_implicit(addr, tls_connector(), &t.host).await.map_err(ftp_fail)?,
            Tls::Explicit => {
                let plain = AsyncRustlsFtpStream::connect(addr).await.map_err(ftp_fail)?;
                plain.into_secure(tls_connector(), &t.host).await.map_err(ftp_fail)?
            }
            Tls::None => AsyncRustlsFtpStream::connect(addr).await.map_err(ftp_fail)?,
        };
        ftp.login(t.user.as_str(), t.pass.as_str()).await.map_err(|e| match e {
            suppaftp::FtpError::UnexpectedResponse(_) => Fail::Permanent("The FTP server refused the user name or password".into()),
            other => ftp_fail(other),
        })?;
        ftp.transfer_type(FileType::Binary).await.map_err(ftp_fail)?;
        Ok::<_, Fail>(ftp)
    };
    tokio::time::timeout(Duration::from_secs(30), fut).await.map_err(|_| Fail::Temporary("The FTP server didn't answer".into()))?
}

/// Size, date and name of the file, and (with `open`) a stream of it from `from`.
pub async fn probe(url: &str, from: u64, open: bool) -> Result<(ProbeInfo, Option<ByteStream>), Fail> {
    let t = parse(url)?;
    let mut ftp = connect(&t).await?;
    let size = ftp.size(&t.path).await.ok().map(|s| s as u64).unwrap_or(0);
    let modified = ftp.mdtm(&t.path).await.ok().map(|d| d.to_string()).unwrap_or_default();
    let name = model::sanitize(t.path.rsplit('/').next().unwrap_or("download"));
    let s = settings::get();
    let category = sorter::categorize(&s.rules, &name, size, url);
    let folder = sorter::folder_for(&s, &category).to_string_lossy().to_string();
    let info = ProbeInfo {
        name,
        size,
        // REST works on practically every server; without a size we can't split anyway.
        resumable: size > 0,
        final_url: url.to_string(),
        etag: String::new(),
        last_modified: modified,
        category,
        folder,
        ..Default::default()
    };
    if !open {
        let _ = ftp.quit().await;
        return Ok((info, None));
    }
    if from > 0 {
        ftp.resume_transfer(from as usize).await.map_err(|_| Fail::Permanent("This FTP server can't resume".into()))?;
    }
    let transfer = ftp.retr_as_stream(&t.path).await.map_err(ftp_fail)?;
    // The control connection lives as long as the stream; it closes when the part is done.
    let stream = futures_util::stream::unfold((ftp, transfer), |(ftp, mut tr)| async move {
        let mut buf = vec![0u8; 64 * 1024];
        match tr.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok(bytes::Bytes::from(buf)), (ftp, tr)))
            }
            Err(e) => Some((Err(Fail::Temporary(format!("FTP transfer failed: {e}"))), (ftp, tr))),
        }
    })
    .boxed();
    Ok((info, Some(stream)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links() {
        let t = parse("ftp://user:p%40ss@example.com/pub/file%20a.iso").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.user.as_str(), t.pass.as_str(), t.path.as_str()), ("example.com", 21, "user", "p@ss", "/pub/file a.iso"));
        assert_eq!(t.tls, Tls::None);
        let t = parse("ftps://example.com/x.bin").unwrap();
        assert_eq!((t.tls, t.port, t.user.as_str()), (Tls::Implicit, 990, "anonymous"));
        assert_eq!(parse("ftps://example.com:21/x").unwrap().tls, Tls::Explicit);
        assert_eq!(parse("ftpes://example.com/x").unwrap().tls, Tls::Explicit);
        assert!(parse("ftp://example.com/dir/").is_err());
        assert!(is_ftp("FTP://x/y"));
    }
}

/// Real servers, run on CI (HAUL_TEST_NET=1): Rebex's public test server, plain and TLS.
#[cfg(test)]
mod net {
    use super::*;

    async fn fetch(url: &str, from: u64) -> (ProbeInfo, Vec<u8>) {
        let (info, stream) = probe(url, from, true).await.expect("probe");
        let mut s = stream.expect("stream");
        let mut out = Vec::new();
        while let Some(chunk) = s.next().await {
            out.extend_from_slice(&chunk.expect("chunk"));
        }
        (info, out)
    }

    #[test]
    fn real_ftp_and_ftps() {
        if std::env::var("HAUL_TEST_NET").is_err() {
            return;
        }
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async {
            for url in ["ftp://demo:password@test.rebex.net/readme.txt", "ftpes://demo:password@test.rebex.net/readme.txt"] {
                let (info, all) = fetch(url, 0).await;
                println!("{url}: {} bytes, size {}", all.len(), info.size);
                assert!(info.size > 0 && all.len() as u64 == info.size);
                let (_, tail) = fetch(url, 10).await;
                assert_eq!(&all[10..], &tail[..], "resume from an offset");
            }
        });
    }
}
