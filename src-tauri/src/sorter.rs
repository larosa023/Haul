//! Picks a category (and so a folder) for a download from its file name, size and site.

use crate::settings::{Rule, Settings};
use std::path::PathBuf;

pub fn extension(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    // Multi-part archives and double extensions sort by what they really are.
    for double in ["tar.gz", "tar.bz2", "tar.xz", "tar.zst"] {
        if lower.ends_with(&format!(".{double}")) {
            return "gz".into();
        }
    }
    match lower.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() && ext.len() <= 12 && !ext.contains(' ') => ext.to_string(),
        _ => String::new(),
    }
}

/// Host of a URL ("" for magnets and bad URLs).
pub fn site(source: &str) -> String {
    url::Url::parse(source).ok().and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())).unwrap_or_default()
}

fn rule_matches(r: &Rule, ext: &str, size: u64, site: &str) -> bool {
    if !r.enabled {
        return false;
    }
    if !r.extensions.is_empty() && !r.extensions.iter().any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(ext)) {
        return false;
    }
    let mb = size as f64 / 1_048_576.0;
    // Size limits only apply once the size is known.
    if size > 0 {
        if r.min_size_mb > 0.0 && mb < r.min_size_mb {
            return false;
        }
        if r.max_size_mb > 0.0 && mb > r.max_size_mb {
            return false;
        }
    } else if r.min_size_mb > 0.0 {
        return false;
    }
    let want = r.site.trim().to_ascii_lowercase();
    if !want.is_empty() && !site.contains(&want) {
        return false;
    }
    // A rule with no conditions at all never matches (it would swallow everything).
    !(r.extensions.is_empty() && r.min_size_mb <= 0.0 && r.max_size_mb <= 0.0 && want.is_empty())
}

/// The category for a file. First matching rule wins; otherwise "Other".
pub fn categorize(rules: &[Rule], name: &str, size: u64, source: &str) -> String {
    let ext = extension(name);
    let site = site(source);
    rules
        .iter()
        .find(|r| rule_matches(r, &ext, size, &site))
        .map(|r| r.category.clone())
        .unwrap_or_else(|| "Other".into())
}

/// Where a download goes: the category's folder when sorting is on, else the default folder.
pub fn folder_for(s: &Settings, category: &str) -> PathBuf {
    if s.sort_enabled {
        if let Some(f) = s.folders.get(category).filter(|f| !f.trim().is_empty()) {
            return PathBuf::from(crate::paths::expand_env(f));
        }
    }
    PathBuf::from(crate::paths::expand_env(&s.download_dir))
}

/// For a torrent, sort by its biggest file (a movie with subtitles is a Video).
pub fn main_file<'a>(files: impl Iterator<Item = (&'a str, u64)>) -> Option<(&'a str, u64)> {
    files.max_by_key(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::default_rules;

    #[test]
    fn categories() {
        let r = default_rules();
        assert_eq!(categorize(&r, "Movie.2024.mkv", 0, ""), "Video");
        assert_eq!(categorize(&r, "song.FLAC", 0, ""), "Music");
        assert_eq!(categorize(&r, "setup.exe", 0, ""), "Software");
        assert_eq!(categorize(&r, "x.tar.gz", 0, ""), "Archives");
        assert_eq!(categorize(&r, "notes", 0, ""), "Other");
    }

    #[test]
    fn site_and_size_rules() {
        let mut r = vec![Rule { category: "Games".into(), site: "itch.io".into(), ..Default::default() }];
        r.extend(default_rules());
        assert_eq!(categorize(&r, "game.zip", 10, "https://cdn.itch.io/x/game.zip"), "Games");
        assert_eq!(categorize(&r, "game.zip", 10, "https://example.com/game.zip"), "Archives");
        let big = vec![Rule { category: "Video".into(), extensions: vec!["zip".into()], min_size_mb: 1000.0, ..Default::default() }];
        assert_eq!(categorize(&big, "a.zip", 2_000 * 1_048_576, ""), "Video");
        assert_eq!(categorize(&big, "a.zip", 2 * 1_048_576, ""), "Other");
        assert_eq!(categorize(&[Rule::default()], "a.zip", 1, ""), "Other");
    }
}
