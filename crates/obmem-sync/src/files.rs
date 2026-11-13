//! Which vault files are backed up, and under what name.
//!
//! A name is the vault-relative path with `/` separators, in Unicode NFC (macOS may hand out
//! NFD names), and it must be portable: a backup made on macOS has to restore on Windows.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

/// How much of a vault's `.obsidian/` folder a sync carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppFiles {
    /// None of it. A project vault is a team's, and the app's settings are each person's own.
    None,
    /// Settings, themes and snippets.
    Settings,
    /// Community plugins too.
    WithPlugins,
}

pub struct Scan {
    /// Backup name → file on disk, sorted by name.
    pub files: Vec<(String, PathBuf)>,
    /// Files left out, with the reason, so the user can rename them.
    pub skipped: Vec<(String, String)>,
}

/// Walk the vault. Dot-folders and dot-files are left out (`.git`, `.trash`, editor folders,
/// obmem's temp files), except the parts of `.obsidian/` that are settings rather than state.
/// Symlinks are never followed.
pub fn scan(vault: &Path, app: AppFiles) -> Result<Scan> {
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let walk = walkdir::WalkDir::new(vault)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0
                || !is_hidden(e.file_name())
                || (e.depth() == 1 && e.file_name() == ".obsidian")
        });
    for entry in walk {
        let entry = entry.with_context(|| format!("reading {}", vault.display()))?;
        if entry.file_type().is_dir() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(vault)
            .expect("walkdir stays under its root");
        let Some(name) = name_of(rel) else {
            skipped.push((
                rel.to_string_lossy().into_owned(),
                "the name is not valid Unicode".into(),
            ));
            continue;
        };
        if !included(&name, app) {
            continue;
        }
        if entry.file_type().is_symlink() {
            skipped.push((name, "symlinks are not backed up".into()));
            continue;
        }
        if let Err(why) = check_portable(&name) {
            skipped.push((name, why));
            continue;
        }
        files.push((name, entry.path().to_path_buf()));
    }
    files.sort();
    Ok(Scan { files, skipped })
}

/// Whether this device syncs `name`. Names it does not sync are left alone on both sides, so a
/// device without community plugins neither downloads nor deletes another device's plugins.
pub fn included(name: &str, app: AppFiles) -> bool {
    match name.strip_prefix(".obsidian/") {
        Some(rest) => obsidian_setting(rest, app),
        None => !name.split('/').any(|part| part.starts_with('.')),
    }
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

fn name_of(rel: &Path) -> Option<String> {
    let parts: Option<Vec<String>> = rel
        .components()
        .map(|c| c.as_os_str().to_str().map(|s| s.nfc().collect()))
        .collect();
    Some(parts?.join("/"))
}

/// The parts of `.obsidian/` worth carrying to another device, modeled on what Obsidian Sync
/// syncs: settings, themes and snippets. The workspace layout changes with every click and
/// would only conflict; community plugins are code and come along only when the user asks.
fn obsidian_setting(rest: &str, app: AppFiles) -> bool {
    if app == AppFiles::None {
        return false;
    }
    let community_plugins = app == AppFiles::WithPlugins;
    match rest.split_once('/') {
        None => {
            rest.ends_with(".json")
                && !rest.starts_with("workspace")
                && (community_plugins || rest != "community-plugins.json")
        }
        Some(("themes" | "snippets", _)) => true,
        Some(("plugins", _)) => community_plugins,
        Some(_) => false,
    }
}

/// A name that every supported file system can hold: no Windows-reserved characters or device
/// names, no trailing dot or space, no `..`. Also checked on restore, where the name comes from
/// the bucket.
pub fn check_portable(name: &str) -> std::result::Result<(), String> {
    if name.is_empty() || name.starts_with('/') {
        return Err("not a relative path".into());
    }
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("`{part}` is not a valid path component"));
        }
        if let Some(c) = part
            .chars()
            .find(|c| matches!(c, '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*') || c.is_control())
        {
            return Err(format!(
                "`{}` cannot be used in a file name on Windows",
                c.escape_default()
            ));
        }
        if part.ends_with('.') || part.ends_with(' ') {
            return Err("names ending in a dot or space are not allowed on Windows".into());
        }
        let stem = part
            .split('.')
            .next()
            .unwrap_or(part)
            .trim_end()
            .to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit()
                && stem.as_bytes()[3] != b'0');
        if reserved {
            return Err(format!("`{part}` is a reserved device name on Windows"));
        }
    }
    Ok(())
}

/// Names that differ only in case: one file system, macOS and Windows by default, would see
/// them as the same file.
pub fn case_collisions<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<(String, String)> {
    let mut seen = std::collections::HashMap::new();
    let mut out = Vec::new();
    for name in names {
        if let Some(prev) = seen.insert(name.to_lowercase(), name) {
            out.push((prev.to_string(), name.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempdir;

    fn put(root: &Path, rel: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, rel).unwrap();
    }

    fn names(s: &Scan) -> Vec<&str> {
        s.files.iter().map(|(n, _)| n.as_str()).collect()
    }

    #[test]
    fn scan_keeps_notes_and_obsidian_settings_only() {
        let dir = tempdir();
        for rel in [
            "Global/a.md",
            "_system/secret-allowlist.toml",
            ".obsidian/app.json",
            ".obsidian/workspace.json",
            ".obsidian/workspace-mobile.json",
            ".obsidian/community-plugins.json",
            ".obsidian/plugins/x/main.js",
            ".obsidian/themes/t/theme.css",
            ".obsidian/snippets/s.css",
            ".obsidian/cache/blob",
            ".git/HEAD",
            ".trash/old.md",
            "Global/.a.md.0123.obmem-tmp",
        ] {
            put(&dir, rel);
        }
        let s = scan(&dir, AppFiles::Settings).unwrap();
        assert_eq!(
            names(&s),
            [
                ".obsidian/app.json",
                ".obsidian/snippets/s.css",
                ".obsidian/themes/t/theme.css",
                "Global/a.md",
                "_system/secret-allowlist.toml"
            ]
        );
        // A team's vault carries none of the app's settings.
        assert_eq!(
            names(&scan(&dir, AppFiles::None).unwrap()),
            ["Global/a.md", "_system/secret-allowlist.toml"]
        );
        let with_plugins = scan(&dir, AppFiles::WithPlugins).unwrap();
        assert!(names(&with_plugins).contains(&".obsidian/plugins/x/main.js"));
        assert!(names(&with_plugins).contains(&".obsidian/community-plugins.json"));
    }

    #[cfg(unix)]
    #[test]
    fn scan_skips_symlinks_and_names_windows_cannot_hold() {
        let dir = tempdir();
        put(&dir, "Global/ok.md");
        put(&dir, "Global/what?.md");
        std::os::unix::fs::symlink("/etc/hosts", dir.join("Global/link.md")).unwrap();
        let s = scan(&dir, AppFiles::Settings).unwrap();
        assert_eq!(names(&s), ["Global/ok.md"]);
        let skipped: Vec<&str> = s.skipped.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(skipped.len(), 2, "{skipped:?}");
    }

    #[test]
    fn names_are_nfc() {
        let dir = tempdir();
        // "é" as e + combining acute (NFD), the way HFS+ stores it.
        put(&dir, "Global/caf\u{0065}\u{0301}.md");
        let s = scan(&dir, AppFiles::Settings).unwrap();
        assert_eq!(names(&s), ["Global/caf\u{00e9}.md"]);
    }

    #[test]
    fn portable_names() {
        for ok in [
            "Global/a.md",
            "Projects/x/Sessions/2026-11-09.md",
            "Global/jwt-輪替策略-v2.md",
            "Global/console.md",
            "Global/com0.md",
        ] {
            assert!(check_portable(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "/abs.md",
            "a/../b.md",
            "a//b.md",
            "a/./b",
            "Global/a:b.md",
            "Global/a\\b.md",
            "Global/nul",
            "Global/Con.md",
            "Global/lpt1.txt",
            "Global/x.",
            "Global/x ",
            "Global/a\u{1}.md",
        ] {
            assert!(check_portable(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn case_collisions_are_found() {
        assert_eq!(
            case_collisions(["a/X.md", "a/y.md", "a/x.md"]),
            [("a/X.md".to_string(), "a/x.md".to_string())]
        );
        assert!(case_collisions(["a.md", "b.md"]).is_empty());
    }
}
