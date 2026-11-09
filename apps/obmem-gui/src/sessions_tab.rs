//! The project's session logs, newest first: what each session did and left for the next one.

use crate::state::{self, Load};
use crate::widgets::{Level, action, card, check, heading, muted};
use nsh_core::prelude::*;
use obmem_core::obsidian;

pub(crate) fn sessions_tab() -> AnyElement {
    dynamic(move || match state::sessions() {
        Load::Loading => vec![muted("Loading…").into_element()],
        Load::Failed(e) => vec![check(Level::Error, "Could not read the session logs", e).into_element()],
        Load::Ready(s) if s.logs.is_empty() => {
            vec![muted("No sessions logged yet. Agents log here with memory_log, and the hooks before compaction.").into_element()]
        }
        Load::Ready(s) => s
            .logs
            .into_iter()
            .map(|log| {
                let mut head = view().flex_row().items_center().gap_3().child(view().flex_1().child(heading(log.name.clone())));
                if let Some(id) = &s.vault_id {
                    let url = obsidian::open_uri(id, &log.rel);
                    head = head.child(action("Open in Obsidian", move || drop(open_url(url.clone()))));
                }
                let mut c = card().test_id(&format!("session-{}", log.name)).child(head);
                // Newest entry first; the files keep them oldest first.
                for e in log.entries.iter().rev() {
                    let (time, what) = split_time(e.trim_start_matches("- "));
                    c = c.child(
                        view()
                            .flex_row()
                            .gap_3()
                            .py_1()
                            .child(muted(time.to_string()).font_family("monospace").w(px(44.0)))
                            .child(text(what.to_string()).text_sm().flex_1()),
                    );
                }
                c.into_element()
            })
            .collect(),
    })
}

/// `13:05 did things` → (`13:05`, `did things`); an entry without a time keeps all of its text.
fn split_time(entry: &str) -> (&str, &str) {
    match entry.split_once(' ') {
        Some((t, rest)) if t.len() == 5 && t.as_bytes()[2] == b':' && t.chars().filter(char::is_ascii_digit).count() == 4 => {
            (t, rest.trim_start())
        }
        _ => ("", entry),
    }
}
