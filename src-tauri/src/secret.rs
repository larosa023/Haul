//! Keeps secrets (the debrid API key) encrypted with Windows DPAPI, tied to your Windows
//! account, so settings.json never holds the key in plain text.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;

#[cfg(windows)]
fn protect(data: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe { CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut out) };
    if ok == 0 {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe { LocalFree(out.pbData as _) };
    Some(v)
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut out)
    };
    if ok == 0 {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe { LocalFree(out.pbData as _) };
    Some(v)
}

// Dev builds on other systems: not encrypted (Haul only ships for Windows).
#[cfg(not(windows))]
fn protect(data: &[u8]) -> Option<Vec<u8>> {
    Some(data.to_vec())
}

#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    Some(data.to_vec())
}

/// "dpapi:<base64>" for storing.
pub fn seal(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    protect(plain.as_bytes()).map(|b| format!("dpapi:{}", B64.encode(b))).unwrap_or_default()
}

/// The plain secret, or "" if it can't be read (e.g. settings copied from another PC).
pub fn open(sealed: &str) -> String {
    let Some(b64) = sealed.strip_prefix("dpapi:") else { return String::new() };
    B64.decode(b64).ok().and_then(|b| unprotect(&b)).and_then(|b| String::from_utf8(b).ok()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn roundtrip() {
        let s = super::seal("ABC123");
        assert!(s.starts_with("dpapi:"));
        assert_eq!(super::open(&s), "ABC123");
        assert_eq!(super::open("garbage"), "");
        assert_eq!(super::seal(""), "");
    }
}
