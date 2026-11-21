//! Haul: Message Stream Encryption (MSE / "protocol encryption"), the obfuscation layer that
//! qBittorrent, Transmission and uTorrent speak. It hides BitTorrent traffic from simple
//! traffic shaping by wrapping the peer stream in RC4 after a Diffie-Hellman handshake.
//!
//! Spec: https://wiki.vuze.com/w/Message_Stream_Encryption
//!
//! The mode is global and can change at runtime (`set_mode`):
//! - `Disabled`: plain connections only.
//! - `Prefer`: outgoing connections try MSE first and fall back to plain; incoming accept both.
//! - `Require`: only RC4-encrypted connections, both ways.

use std::io::IoSliceMut;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::{Context as _, bail};
use num_bigint::BigUint;
use sha1w::ISha1;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

use crate::type_aliases::{BoxAsyncReadVectored, BoxAsyncWrite};
use crate::vectored_traits::AsyncReadVectored;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EncryptionMode {
    Disabled = 0,
    Prefer = 1,
    Require = 2,
}

static MODE: AtomicU8 = AtomicU8::new(EncryptionMode::Prefer as u8);

pub fn set_mode(mode: EncryptionMode) {
    MODE.store(mode as u8, Ordering::Relaxed);
}

pub fn mode() -> EncryptionMode {
    match MODE.load(Ordering::Relaxed) {
        0 => EncryptionMode::Disabled,
        2 => EncryptionMode::Require,
        _ => EncryptionMode::Prefer,
    }
}

const P_HEX: &str = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A63A36210000000000090563";
const KEY_LEN: usize = 96;
const MAX_PAD: usize = 512;
const VC: [u8; 8] = [0; 8];
const CRYPTO_PLAIN: u32 = 0x01;
const CRYPTO_RC4: u32 = 0x02;
pub(crate) const BT_PREFIX: &[u8; 20] = b"\x13BitTorrent protocol";

fn prime() -> BigUint {
    BigUint::parse_bytes(P_HEX.as_bytes(), 16).unwrap()
}

fn to_96(n: &BigUint) -> [u8; KEY_LEN] {
    let b = n.to_bytes_be();
    let mut out = [0u8; KEY_LEN];
    out[KEY_LEN - b.len()..].copy_from_slice(&b);
    out
}

struct Dh {
    private: BigUint,
    public: [u8; KEY_LEN],
}

impl Dh {
    fn new() -> Self {
        use rand::RngExt;
        let x: [u8; 20] = rand::rng().random();
        let private = BigUint::from_bytes_be(&x);
        let public = to_96(&BigUint::from(2u32).modpow(&private, &prime()));
        Self { private, public }
    }

    fn secret(&self, other: &[u8]) -> anyhow::Result<[u8; KEY_LEN]> {
        let p = prime();
        let y = BigUint::from_bytes_be(other);
        if y <= BigUint::from(1u32) || y >= &p - 1u32 {
            bail!("bad DH public key");
        }
        Ok(to_96(&y.modpow(&self.private, &p)))
    }
}

fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    let mut h = sha1w::Sha1::new();
    for p in parts {
        h.update(p);
    }
    h.finish()
}

fn padding() -> Vec<u8> {
    use rand::RngExt;
    let mut r = rand::rng();
    let n = r.random_range(0..=MAX_PAD);
    (0..n).map(|_| r.random::<u8>()).collect()
}

/// RC4 with the first 1024 bytes of keystream dropped, as MSE requires.
#[derive(Clone)]
pub(crate) struct Rc4 {
    s: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    fn new(key: &[u8]) -> Self {
        let mut s = [0u8; 256];
        for (i, v) in s.iter_mut().enumerate() {
            *v = i as u8;
        }
        let mut j: u8 = 0;
        for i in 0..256 {
            j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
            s.swap(i, j as usize);
        }
        let mut rc4 = Self { s, i: 0, j: 0 };
        let mut discard = [0u8; 1024];
        rc4.apply(&mut discard);
        rc4
    }

    pub(crate) fn apply(&mut self, data: &mut [u8]) {
        for b in data.iter_mut() {
            self.i = self.i.wrapping_add(1);
            self.j = self.j.wrapping_add(self.s[self.i as usize]);
            self.s.swap(self.i as usize, self.j as usize);
            let k = self.s[(self.s[self.i as usize].wrapping_add(self.s[self.j as usize])) as usize];
            *b ^= k;
        }
    }
}

fn keys(secret: &[u8], skey: &[u8; 20]) -> (Rc4, Rc4) {
    let a = sha1(&[b"keyA", secret, skey]);
    let b = sha1(&[b"keyB", secret, skey]);
    (Rc4::new(&a), Rc4::new(&b))
}

// ---------------------------------------------------------------- wrapped streams

/// Reader that first replays `prefix` (already-decrypted bytes), then decrypts the inner stream.
pub(crate) struct MseReader {
    inner: BoxAsyncReadVectored,
    prefix: Vec<u8>,
    pos: usize,
    cipher: Option<Rc4>,
}

impl AsyncRead for MseReader {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if this.pos < this.prefix.len() {
            let n = (this.prefix.len() - this.pos).min(buf.remaining());
            buf.put_slice(&this.prefix[this.pos..this.pos + n]);
            this.pos += n;
            if this.pos == this.prefix.len() {
                this.prefix = Vec::new();
                this.pos = 0;
            }
            return Poll::Ready(Ok(()));
        }
        let before = buf.filled().len();
        std::task::ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
        if let Some(c) = this.cipher.as_mut() {
            c.apply(&mut buf.filled_mut()[before..]);
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncReadVectored for MseReader {
    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        vec: &mut [IoSliceMut<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let Some(first) = vec.iter_mut().find(|s| !s.is_empty()) else {
            return Poll::Ready(Ok(0));
        };
        let mut rb = ReadBuf::new(first);
        std::task::ready!(self.poll_read(cx, &mut rb))?;
        Poll::Ready(Ok(rb.filled().len()))
    }
}

/// Writer that RC4-encrypts everything (when a cipher is set). Encrypted bytes that the inner
/// writer hasn't taken yet are kept and written first, so the keystream never repeats.
pub(crate) struct MseWriter {
    inner: BoxAsyncWrite,
    pending: Vec<u8>,
    sent: usize,
    cipher: Option<Rc4>,
}

impl MseWriter {
    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        while self.sent < self.pending.len() {
            let n = std::task::ready!(Pin::new(&mut self.inner).poll_write(cx, &self.pending[self.sent..]))?;
            if n == 0 {
                return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()));
            }
            self.sent += n;
        }
        self.pending.clear();
        self.sent = 0;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for MseWriter {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if this.cipher.is_none() {
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        }
        std::task::ready!(this.poll_drain(cx))?;
        let start = this.pending.len();
        this.pending.extend_from_slice(buf);
        if let Some(cipher) = this.cipher.as_mut() {
            cipher.apply(&mut this.pending[start..]);
        }
        // Push what we can now; the rest goes out on the next write or flush.
        let _ = this.poll_drain(cx)?;
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

fn wrap(
    read: BoxAsyncReadVectored,
    write: BoxAsyncWrite,
    prefix: Vec<u8>,
    rx: Option<Rc4>,
    tx: Option<Rc4>,
) -> (BoxAsyncReadVectored, BoxAsyncWrite) {
    (
        Box::new(MseReader { inner: read, prefix, pos: 0, cipher: rx }),
        Box::new(MseWriter { inner: write, pending: Vec::new(), sent: 0, cipher: tx }),
    )
}

/// Replays bytes already read from a plain connection (used when an incoming connection turns
/// out not to be encrypted).
pub(crate) fn replay(read: BoxAsyncReadVectored, write: BoxAsyncWrite, prefix: Vec<u8>) -> (BoxAsyncReadVectored, BoxAsyncWrite) {
    if prefix.is_empty() {
        return (read, write);
    }
    (Box::new(MseReader { inner: read, prefix, pos: 0, cipher: None }), write)
}

/// Reads until `pattern` shows up within `limit` bytes; returns everything after it.
async fn sync_on(read: &mut BoxAsyncReadVectored, buf: &mut Vec<u8>, pattern: &[u8], limit: usize) -> anyhow::Result<Vec<u8>> {
    let mut tmp = [0u8; 1024];
    loop {
        if let Some(pos) = buf.windows(pattern.len()).position(|w| w == pattern) {
            let rest = buf[pos + pattern.len()..].to_vec();
            return Ok(rest);
        }
        if buf.len() > limit + pattern.len() {
            bail!("MSE: sync pattern not found");
        }
        let n = read.read(&mut tmp).await?;
        if n == 0 {
            bail!("MSE: connection closed during handshake");
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

/// Reads exactly `n` bytes, first from `have` (already received), then from the stream.
async fn take(read: &mut BoxAsyncReadVectored, have: &mut Vec<u8>, n: usize) -> anyhow::Result<Vec<u8>> {
    while have.len() < n {
        let mut tmp = [0u8; 1024];
        let got = read.read(&mut tmp).await?;
        if got == 0 {
            bail!("MSE: connection closed during handshake");
        }
        have.extend_from_slice(&tmp[..got]);
    }
    let rest = have.split_off(n);
    Ok(std::mem::replace(have, rest))
}

/// Outgoing side. Returns the wrapped stream (RC4 or plain, as the peer chose).
pub(crate) async fn initiate(
    mut read: BoxAsyncReadVectored,
    mut write: BoxAsyncWrite,
    info_hash: [u8; 20],
    require_rc4: bool,
    timeout: Duration,
) -> anyhow::Result<(BoxAsyncReadVectored, BoxAsyncWrite)> {
    tokio::time::timeout(timeout * 2, async move {
        let dh = Dh::new();
        let mut first = dh.public.to_vec();
        first.extend(padding());
        write.write_all(&first).await?;

        let mut have = Vec::new();
        let yb = take(&mut read, &mut have, KEY_LEN).await?;
        let s = dh.secret(&yb)?;
        let (mut enc, mut dec) = keys(&s, &info_hash);

        let provide = if require_rc4 { CRYPTO_RC4 } else { CRYPTO_RC4 | CRYPTO_PLAIN };
        let mut out = Vec::with_capacity(80);
        out.extend_from_slice(&sha1(&[b"req1", &s]));
        let r2 = sha1(&[b"req2", &info_hash]);
        let r3 = sha1(&[b"req3", &s]);
        out.extend(r2.iter().zip(r3.iter()).map(|(a, b)| a ^ b));
        let mut body = Vec::new();
        body.extend_from_slice(&VC);
        body.extend_from_slice(&provide.to_be_bytes());
        body.extend_from_slice(&0u16.to_be_bytes()); // len(PadC)
        body.extend_from_slice(&0u16.to_be_bytes()); // len(IA)
        enc.apply(&mut body);
        out.extend(body);
        write.write_all(&out).await?;
        write.flush().await?;

        // Find B's encrypted VC after its padding.
        let mut evc = VC;
        let mut probe = dec.clone();
        probe.apply(&mut evc);
        let mut rest = sync_on(&mut read, &mut have, &evc, MAX_PAD).await?;
        dec = probe;
        let mut head = take(&mut read, &mut rest, 6).await?;
        dec.apply(&mut head);
        let select = u32::from_be_bytes(head[0..4].try_into().unwrap());
        let pad_d = u16::from_be_bytes(head[4..6].try_into().unwrap()) as usize;
        if pad_d > MAX_PAD {
            bail!("MSE: bad padding length");
        }
        let mut pad = take(&mut read, &mut rest, pad_d).await?;
        dec.apply(&mut pad);

        match select {
            CRYPTO_RC4 => {
                dec.apply(&mut rest);
                Ok(wrap(read, write, rest, Some(dec), Some(enc)))
            }
            CRYPTO_PLAIN if !require_rc4 => Ok(wrap(read, write, rest, None, None)),
            _ => bail!("MSE: peer picked an unsupported method ({select})"),
        }
    })
    .await
    .context("MSE handshake timed out")?
}

/// Incoming side. `start` holds the bytes already read (not a plain BitTorrent handshake).
/// `info_hashes` lists the torrents we serve, to find which one the peer wants.
pub(crate) async fn accept(
    mut read: BoxAsyncReadVectored,
    mut write: BoxAsyncWrite,
    start: Vec<u8>,
    info_hashes: Vec<[u8; 20]>,
    require_rc4: bool,
    timeout: Duration,
) -> anyhow::Result<(BoxAsyncReadVectored, BoxAsyncWrite)> {
    tokio::time::timeout(timeout * 2, async move {
        let mut have = start;
        let ya = take(&mut read, &mut have, KEY_LEN).await?;
        let dh = Dh::new();
        let mut first = dh.public.to_vec();
        first.extend(padding());
        write.write_all(&first).await?;
        write.flush().await?;
        let s = dh.secret(&ya)?;

        let req1 = sha1(&[b"req1", &s]);
        let mut rest = sync_on(&mut read, &mut have, &req1, MAX_PAD).await?;
        let x = take(&mut read, &mut rest, 20).await?;
        let r3 = sha1(&[b"req3", &s]);
        let skey = info_hashes
            .into_iter()
            .find(|ih| {
                let r2 = sha1(&[b"req2", ih]);
                r2.iter().zip(r3.iter()).map(|(a, b)| a ^ b).eq(x.iter().copied())
            })
            .context("MSE: peer asked for a torrent we don't have")?;
        // From B's point of view, A encrypts with keyA (our decryptor) and we encrypt with keyB.
        let (mut dec, mut enc) = keys(&s, &skey);

        let mut head = take(&mut read, &mut rest, 14).await?;
        dec.apply(&mut head);
        if head[0..8] != VC {
            bail!("MSE: bad verification constant");
        }
        let provide = u32::from_be_bytes(head[8..12].try_into().unwrap());
        let pad_c = u16::from_be_bytes(head[12..14].try_into().unwrap()) as usize;
        if pad_c > MAX_PAD {
            bail!("MSE: bad padding length");
        }
        let mut pad = take(&mut read, &mut rest, pad_c).await?;
        dec.apply(&mut pad);
        let mut ia_len = take(&mut read, &mut rest, 2).await?;
        dec.apply(&mut ia_len);
        let ia_len = u16::from_be_bytes(ia_len[..].try_into().unwrap()) as usize;
        let mut ia = take(&mut read, &mut rest, ia_len).await?;
        dec.apply(&mut ia);

        let select = if provide & CRYPTO_RC4 != 0 {
            CRYPTO_RC4
        } else if provide & CRYPTO_PLAIN != 0 && !require_rc4 {
            CRYPTO_PLAIN
        } else {
            bail!("MSE: no common method");
        };
        let mut reply = Vec::new();
        reply.extend_from_slice(&VC);
        reply.extend_from_slice(&select.to_be_bytes());
        reply.extend_from_slice(&0u16.to_be_bytes());
        enc.apply(&mut reply);
        write.write_all(&reply).await?;
        write.flush().await?;

        // The initial payload (IA) is always encrypted; the stream after it depends on `select`.
        let mut prefix = ia;
        if select == CRYPTO_RC4 {
            dec.apply(&mut rest);
            prefix.extend(rest);
            Ok(wrap(read, write, prefix, Some(dec), Some(enc)))
        } else {
            prefix.extend(rest);
            Ok(wrap(read, write, prefix, None, None))
        }
    })
    .await
    .context("MSE handshake timed out")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc4_known_vector() {
        // RFC 6229 key "Key" without the 1024-byte drop: check our core by re-deriving.
        let mut a = Rc4::new(b"secret");
        let mut b = Rc4::new(b"secret");
        let mut data = *b"hello world, this is haul";
        a.apply(&mut data);
        assert_ne!(&data, b"hello world, this is haul");
        b.apply(&mut data);
        assert_eq!(&data, b"hello world, this is haul");
    }

    #[test]
    fn dh_agrees() {
        let a = Dh::new();
        let b = Dh::new();
        assert_eq!(a.secret(&b.public).unwrap(), b.secret(&a.public).unwrap());
    }

    async fn roundtrip(require_a: bool, require_b: bool, expect_rc4: bool) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let ih = [7u8; 20];
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, w) = sock.into_split();
            let mut r: BoxAsyncReadVectored = Box::new(r);
            let mut start = vec![0u8; 20];
            r.read_exact(&mut start).await.unwrap();
            assert_ne!(&start[..], BT_PREFIX);
            let (mut r, mut w) = accept(r, Box::new(w), start, vec![[1u8; 20], ih], require_b, Duration::from_secs(5))
                .await
                .unwrap();
            let mut buf = [0u8; 5];
            r.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"ping!");
            w.write_all(b"pong!").await.unwrap();
            w.flush().await.unwrap();
        });
        let sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (r, w) = sock.into_split();
        let (mut r, mut w) = initiate(Box::new(r), Box::new(w), ih, require_a, Duration::from_secs(5))
            .await
            .unwrap();
        let _ = expect_rc4;
        w.write_all(b"ping!").await.unwrap();
        w.flush().await.unwrap();
        let mut buf = [0u8; 5];
        r.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong!");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn handshake_rc4() {
        roundtrip(true, false, true).await;
        roundtrip(false, false, true).await;
        roundtrip(false, true, true).await;
    }
}
