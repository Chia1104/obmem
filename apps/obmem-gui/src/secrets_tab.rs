//! A project's secret scanning: what the scanner checks, trying it on text, and the project's
//! allowlist. Values are only ever shown masked.

use crate::data::{Detail, Hit, Vault};
use crate::state;
use crate::widgets::{Level, action, card, check, heading, muted, primary, text_box};
use nsh_core::prelude::*;
use obmem_core::allowlist::{Allowlist, Kind};
use obmem_core::config::SecretMode;

pub(crate) fn secrets_tab(d: &Detail) -> View {
    view().gap_4().child(scanner(d)).child(try_it()).child(allowlist(&d.allowlist))
}

fn scanner(d: &Detail) -> View {
    let st = &d.status;
    let mode = match st.secret_mode {
        SecretMode::Block => "A write that contains a secret is refused.",
        SecretMode::Redact => "A secret in a write is replaced with [REDACTED] and the rest is written.",
    };
    card()
        .child(heading("Scanner"))
        .child(check(Level::Ok, "Every write is scanned", mode))
        .child(check(
            Level::Ok,
            format!("{} format rules", st.secret_rules),
            "Known token formats, plus your own in the vault's _system/secret-rules.toml.",
        ))
        .child(check(
            Level::Ok,
            format!("{} known values", st.known_secrets),
            "Values from the project's env files and sensitive-looking environment variables, kept in memory only.",
        ))
}

fn try_it() -> View {
    let input = signal(state::last_scanned());
    card()
        .test_id("scan")
        .child(heading("Try it"))
        .child(muted("Paste text to see what the scanner would catch. Nothing is stored."))
        .child(text_box(input, "Text to scan").test_id("scan-input").multiline(true).min_h(px(80.0)))
        .child(view().flex_row().pt_2().child(primary("Scan", move || state::scan(input.get_untracked()))))
        .child(dynamic(move || {
            state::scan_result().map(|hits| {
                if hits.is_empty() {
                    return vec![check(Level::Ok, "Nothing found", "This text would be written as it is.").into_element()];
                }
                hits.into_iter().map(|h| hit(h).into_element()).collect()
            })
            .unwrap_or_default()
        }))
}

fn hit(h: Hit) -> View {
    let (here, everywhere) = (h.fingerprint.clone(), h.fingerprint.clone());
    check(Level::Error, format!("{}: {}", h.rule, h.description), format!("{}   fingerprint {}", h.preview, h.fingerprint))
        .child(
            view()
                .gap_1()
                .child(action("Allow in this project", move || state::allow(Vault::Project, Kind::Fingerprint, here.clone())))
                .child(action("Allow in every project", move || {
                    state::allow(Vault::Personal, Kind::Fingerprint, everywhere.clone())
                })),
        )
}

fn allowlist(list: &Result<Allowlist, String>) -> View {
    let mut c = card().test_id("project-allowlist").child(heading("This project's allowlist")).child(muted(
        "False positives allowed in this project, by fingerprint. The file comes with the project, so patterns \
         and words count only in your personal settings.",
    ));
    match list {
        Err(e) => c = c.child(check(Level::Error, "Could not read the allowlist", e.clone())),
        Ok(a) if a.fingerprints.is_empty() => c = c.child(muted("Nothing allowed yet.")),
        Ok(a) => {
            for f in &a.fingerprints {
                let entry = f.clone();
                c = c.child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .py_1()
                        .child(text(f.clone()).flex_1().text_sm())
                        .child(action("Remove", move || state::disallow(Vault::Project, Kind::Fingerprint, entry.clone()))),
                );
            }
        }
    }
    c
}
