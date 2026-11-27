//! Auto-extract finished archives (zip, 7z, rar), optionally deleting the archive after.
//!
//! zip and 7z are extracted in-process. rar uses Windows' own `tar.exe` (libarchive, which reads
//! RAR on Windows 11), so there's no extra unrar library to ship.

use std::path::{Component, Path, PathBuf};

pub fn is_archive(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".zip") || l.ends_with(".7z") || l.ends_with(".rar")
}

/// For "x.part2.rar" (and later parts) only the first part is extracted.
fn rar_part(name: &str) -> Option<(String, u32)> {
    let l = name.to_ascii_lowercase();
    let stem = l.strip_suffix(".rar")?;
    let (base, part) = stem.rsplit_once(".part")?;
    let n: u32 = part.parse().ok()?;
    Some((base.to_string(), n))
}

/// Archives that shouldn't be extracted on their own (later RAR volumes, split 7z/zip parts).
pub fn is_secondary_part(name: &str) -> bool {
    if let Some((_, n)) = rar_part(name) {
        return n > 1;
    }
    let l = name.to_ascii_lowercase();
    // x.7z.002 / x.zip.002 / x.r00 style volumes
    l.ends_with(".r00") || l.rsplit_once('.').map(|(_, e)| e.len() == 3 && e.chars().all(|c| c.is_ascii_digit()) && e != "001").unwrap_or(false)
}

/// All volumes of a multi-part RAR, if every one of them is present (and not still downloading).
fn rar_volumes(path: &Path) -> Option<Vec<PathBuf>> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let Some((base, _)) = rar_part(&name) else { return Some(vec![path.to_path_buf()]) };
    let dir = path.parent()?;
    let mut parts: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            let (b, i) = rar_part(&n)?;
            (b == base).then(|| (i, e.path()))
        })
        .collect();
    parts.sort_by_key(|(i, _)| *i);
    // Volumes must run 1..=n with no gaps, and none may be mid-download.
    for (k, (i, p)) in parts.iter().enumerate() {
        if *i as usize != k + 1 || p.with_file_name(format!("{}.haul", p.file_name()?.to_string_lossy())).exists() {
            return None;
        }
    }
    Some(parts.into_iter().map(|(_, p)| p).collect())
}

fn safe_relative(name: &str) -> bool {
    let p = Path::new(name);
    !p.is_absolute() && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn extract_zip(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let f = std::fs::File::open(archive)?;
    let mut z = zip::ZipArchive::new(std::io::BufReader::new(f))?;
    // zip's extract() sanitizes every path (no "..", no absolute paths).
    z.extract(dest)?;
    Ok(())
}

fn extract_7z(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let root = dest.to_path_buf();
    sevenz_rust2::decompress_file_with_extract_fn(archive, dest, move |entry, reader, path| {
        if !safe_relative(entry.name()) || !path.starts_with(&root) {
            return Ok(true); // skip entries that would land outside the folder
        }
        sevenz_rust2::default_entry_extract_fn(entry, reader, path)
    })?;
    Ok(())
}

fn extract_with_tar(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new(system_tar());
    cmd.arg("-xf").arg(archive).arg("-C").arg(dest);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        anyhow::bail!(if err.is_empty() { "Windows couldn't open this RAR file (needs Windows 11).".to_string() } else { err });
    }
    Ok(())
}

pub fn system_tar() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(|r| PathBuf::from(r).join("System32").join("tar.exe"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("tar"))
}

fn unique_dir(dir: &Path, name: &str) -> PathBuf {
    let mut p = dir.join(name);
    let mut n = 1;
    while p.exists() {
        p = dir.join(format!("{name} ({n})"));
        n += 1;
    }
    p
}

/// Extracts `archive` next to itself. One top-level entry is placed as-is; several go into a
/// folder named after the archive. Returns where the files went.
pub fn extract(archive: &Path, delete_after: bool) -> anyhow::Result<PathBuf> {
    let name = archive.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let dir = archive.parent().ok_or_else(|| anyhow::anyhow!("bad path"))?;
    let lower = name.to_ascii_lowercase();
    let volumes = if lower.ends_with(".rar") {
        rar_volumes(archive).ok_or_else(|| anyhow::anyhow!("Waiting for the other parts of this archive"))?
    } else {
        vec![archive.to_path_buf()]
    };
    let stem = {
        let s = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name);
        rar_part(&name).map(|(b, _)| b).map(|b| name[..b.len()].to_string()).unwrap_or_else(|| s.to_string())
    };
    let temp = dir.join(format!(".{stem}.extracting"));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp)?;
    let result = if lower.ends_with(".zip") {
        extract_zip(&volumes[0], &temp)
    } else if lower.ends_with(".7z") {
        extract_7z(&volumes[0], &temp)
    } else {
        extract_with_tar(&volumes[0], &temp)
    };
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(e);
    }
    let entries: Vec<_> = std::fs::read_dir(&temp)?.flatten().collect();
    let target = if entries.len() == 1 {
        let only = entries[0].path();
        let n = only.file_name().unwrap().to_string_lossy().to_string();
        let t = if only.is_dir() { unique_dir(dir, &n) } else { dir.join(crate::model::unique_name(dir, &n, &[])) };
        std::fs::rename(&only, &t)?;
        let _ = std::fs::remove_dir_all(&temp);
        t
    } else {
        let t = unique_dir(dir, &stem);
        std::fs::rename(&temp, &t)?;
        t
    };
    if delete_after {
        for v in volumes {
            let _ = std::fs::remove_file(v);
        }
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parts() {
        assert!(is_secondary_part("a.part2.rar"));
        assert!(!is_secondary_part("a.part1.rar"));
        assert!(!is_secondary_part("a.rar"));
        assert!(is_secondary_part("a.7z.002"));
        assert!(!is_secondary_part("a.7z.001"));
        assert!(!is_secondary_part("a.mp4"));
    }

    #[test]
    fn zip_roundtrip() {
        let dir = std::env::temp_dir().join(format!("haul-x-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zpath = dir.join("pack.zip");
        {
            let f = std::fs::File::create(&zpath).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let o: zip::write::SimpleFileOptions = Default::default();
            z.start_file("a.txt", o).unwrap();
            z.write_all(b"hello").unwrap();
            z.start_file("b/c.txt", o).unwrap();
            z.write_all(b"world").unwrap();
            z.finish().unwrap();
        }
        let out = extract(&zpath, true).unwrap();
        assert_eq!(out, dir.join("pack"));
        assert_eq!(std::fs::read_to_string(out.join("b/c.txt")).unwrap(), "world");
        assert!(!zpath.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
