//! Reading a project's session logs (`<personal vault>/Projects/<name>/Sessions/*.md`), which
//! `memory_log` and the hooks append to: one file per day, or per day and session when sessions
//! run side by side.

use crate::config::Config;
use crate::note::Note;
use anyhow::Result;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionLog {
    /// The file's name: the day, maybe with a session suffix (`2026-11-09`, `2026-11-09-abc`).
    pub name: String,
    /// Path inside the personal vault.
    pub rel: String,
    /// Oldest first, each with its time (`- 13:05 …`) and continuation lines.
    pub entries: Vec<String>,
}

/// The most recently changed logs first, at most `limit`; logs without entries are left out.
pub fn logs(cfg: &Config, limit: usize) -> Result<Vec<SessionLog>> {
    let dir = cfg.global_project_dir().join("Sessions");
    let Ok(read) = std::fs::read_dir(&dir) else { return Ok(vec![]) };
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
        let body = Note::parse(&text).map(|n| n.body).unwrap_or(text);
        let entries = entries(&body);
        if entries.is_empty() {
            continue;
        }
        out.push(SessionLog {
            name: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
            rel: path.strip_prefix(&cfg.global_vault).unwrap_or(&path).to_string_lossy().replace('\\', "/"),
            entries,
        });
    }
    Ok(out)
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
}
