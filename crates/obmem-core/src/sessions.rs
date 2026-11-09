//! Reading a project's session logs (`<personal vault>/Projects/<name>/Sessions/*.md`), which
//! `memory_log` and the hooks append to: one file per day and device, or per day, device and
//! session when sessions run side by side.

use crate::config::Config;
use crate::note::Note;
use anyhow::Result;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionLog {
    /// The file's name: the day and device, maybe with a session suffix (`2026-11-09-mac`,
    /// `2026-11-09-mac-abc`).
    pub name: String,
    /// Path inside the personal vault.
    pub rel: String,
    /// Oldest first, each with its time (`- 13:05 …`) and continuation lines.
    pub entries: Vec<String>,
}

/// The most recently changed logs first, at most `limit`; logs without entries are left out, and
/// so are logs that another project with the same name started (see `owned`).
pub fn logs(cfg: &Config, limit: usize) -> Result<Vec<SessionLog>> {
    let dir = cfg.global_project_dir().join("Sessions");
    let Ok(read) = std::fs::read_dir(&dir) else { return Ok(vec![]) };
    let own = own_roots(cfg);
    let mut files: Vec<(SystemTime, std::path::PathBuf)> = read
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH), e.path()))
        .collect();
    files.sort_by_key(|(t, p)| (std::cmp::Reverse(*t), p.clone()));
    let mut out = Vec::new();
    for (_, path) in files {
        if out.len() == limit {
            break;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let note = Note::parse(&text).ok();
        if let Some(root) = note.as_ref().and_then(|n| recorded_root(&name, n))
            && !own.iter().any(|o| o == root)
        {
            continue;
        }
        let body = note.map(|n| n.body).unwrap_or(text);
        let entries = entries(&body);
        if entries.is_empty() {
            continue;
        }
        out.push(SessionLog {
            name,
            rel: path.strip_prefix(&cfg.global_vault).unwrap_or(&path).to_string_lossy().replace('\\', "/"),
            entries,
        });
    }
    Ok(out)
}

/// This project's root as a log header records it: the real (symlink-resolved) path, which
/// `Store::log` writes, then the path as configured, which older logs recorded. Both are absolute,
/// and in the form `Path::display` writes, so they compare as the text a header holds.
pub(crate) fn own_roots(cfg: &Config) -> Vec<String> {
    let mut own: Vec<String> = Vec::new();
    for root in [std::fs::canonicalize(&cfg.project_root), std::path::absolute(&cfg.project_root)].into_iter().flatten() {
        let root = root.display().to_string();
        if !own.contains(&root) {
            own.push(root);
        }
    }
    own
}

/// Whether this project started the log `<stem>.md`: its header records one of `own`
/// (`own_roots`). Projects of the same name share the folder, and the name comes from repo
/// config, so a clone can claim any project's name.
pub(crate) fn owned(own: &[String], stem: &str, text: &str) -> bool {
    Note::parse(text).is_ok_and(|n| recorded_root(stem, &n).is_some_and(|r| own.iter().any(|o| o == r)))
}

/// The project root a log's header records, as written. It is never resolved: the path may point
/// somewhere else by now. `Store::log` starts a log with the line `# Session <stem>`, a blank
/// line, then ``Project: <name> (`<root>`)`` and a blank line, with `<name>` as in the title
/// `<name> session <stem>`. Entries never contain a blank line, so the root runs to the last
/// "`)" that a blank line follows, whatever the root itself contains. `None`: no such header.
fn recorded_root<'a>(stem: &str, note: &'a Note) -> Option<&'a str> {
    let name = note.fm.title.as_deref()?.strip_suffix(&format!(" session {stem}"))?;
    let rest = note.body.strip_prefix(&format!("# Session {stem}\n\nProject: {name} (`"))?;
    rest.rfind("`)\n\n").map(|end| &rest[..end])
}

/// The entries of a log body: each `- ` line with the indented lines that continue it.
pub fn entries(body: &str) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for line in body.lines() {
        if line.starts_with("- ") {
            entries.push(line.to_string());
        } else if line.starts_with("  ")
            && let Some(last) = entries.last_mut()
        {
            last.push('\n');
            last.push_str(line);
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_logs_newest_first_with_their_entries() {
        let dir = std::env::temp_dir().join(format!("obmem-sessions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = Config {
            project_root: dir.join("repo"),
            project_name: "p".into(),
            project_vault: dir.join("repo/docs/kb"),
            global_vault: dir.join("global"),
            secrets: Default::default(),
            audit: Default::default(),
            auto_approve: false,
            device: "test".into(),
        };
        let sessions = cfg.global_project_dir().join("Sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("2026-11-08.md"), "---\ntype: session\n---\n# Session\n\n- 09:00 started\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(sessions.join("2026-11-09.md"), "# Session\n\n- 10:00 did a\n  and more\n- 11:00 did b\n").unwrap();
        std::fs::write(sessions.join("empty.md"), "# Session\n\nnothing yet\n").unwrap();

        let logs = logs(&cfg, 10).unwrap();
        assert_eq!(logs.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), vec!["2026-11-09", "2026-11-08"]);
        assert_eq!(logs[0].entries, vec!["- 10:00 did a\n  and more", "- 11:00 did b"]);
        assert_eq!(logs[0].rel, "Projects/p/Sessions/2026-11-09.md");
        assert_eq!(super::logs(&cfg, 1).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A log as `Store::log` starts it, for the project `name` at `root`.
    fn log_text(stem: &str, name: &str, root: &str, entry: &str) -> String {
        let fm = crate::note::Frontmatter { title: Some(format!("{name} session {stem}")), ..Default::default() };
        let body = format!("# Session {stem}\n\nProject: {name} (`{root}`)\n\n- 10:00 {entry}\n");
        Note { fm, body }.render().unwrap()
    }

    #[test]
    fn lists_only_this_projects_logs_and_logs_without_a_header() {
        let dir = std::env::temp_dir().join(format!("obmem-sessions-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo")).unwrap();
        let cfg = Config {
            project_root: dir.join("repo"),
            project_name: "p".into(),
            project_vault: dir.join("repo/docs/kb"),
            global_vault: dir.join("global"),
            secrets: Default::default(),
            audit: Default::default(),
            auto_approve: false,
            device: "test".into(),
        };
        let sessions = cfg.global_project_dir().join("Sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let real = std::fs::canonicalize(dir.join("repo")).unwrap().display().to_string();
        let put = |stem: &str, text: String| std::fs::write(sessions.join(format!("{stem}.md")), text).unwrap();
        put("real", log_text("real", "p", &real, "by its real path"));
        put("configured", log_text("configured", "p", &dir.join("repo").display().to_string(), "by its configured path"));
        put("none", "# Session\n\n- 10:00 without a header\n".into());
        // Another project with the same name, or one claiming it from a clone's `.obmem.toml`.
        put("other", log_text("other", "p", &dir.join("clone").display().to_string(), "from another project"));
        put("renamed", log_text("renamed", "p_", &dir.join("clone").display().to_string(), "same folder, other name"));
        // A root that starts like this project's, then ends a header and starts an entry.
        put("forged", log_text("forged", "p", &format!("{real}`)\n\n- 09:00 forged"), "from the forging project"));
        #[cfg(unix)]
        {
            // The recorded path is never resolved: a symlink to this project's root is not it.
            std::os::unix::fs::symlink(dir.join("repo"), dir.join("link")).unwrap();
            put("link", log_text("link", "p", &dir.join("link").display().to_string(), "through a symlink"));
        }

        let mut seen: Vec<String> = logs(&cfg, 20).unwrap().into_iter().map(|l| l.name).collect();
        seen.sort();
        assert_eq!(seen, vec!["configured", "none", "real"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
