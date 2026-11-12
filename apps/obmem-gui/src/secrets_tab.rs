//! A project's secret scanning: what the scanner checks, trying it on text, and the project's
//! allowlist. Values are only ever shown masked.

use crate::data::{Detail, Hit, Vault};
use crate::state::{self, Work};
use crate::widgets::{Level, action, card, check, heading, muted, primary, text_box};
use crate::words::tr;
use nsh_core::prelude::*;
use obmem_core::allowlist::{Allowlist, Kind};
use obmem_core::config::SecretMode;

pub(crate) fn secrets_tab(d: &Detail) -> View {
    view()
        .gap_4()
        .child(scanner(d))
        .child(try_it())
        .child(allowlist(&d.allowlist))
}

fn scanner(d: &Detail) -> View {
    let st = &d.status;
    let mode = match st.secret_mode {
        SecretMode::Block => "secrets.refused",
        SecretMode::Redact => "secrets.redacted",
    };
    let (rules, known) = (st.secret_rules, st.known_secrets);
    card()
        .child(heading(tr!("secrets.scanner")))
        .child(check(Level::Ok, tr!("secrets.every_write"), tr!(mode)))
        .child(check(
            Level::Ok,
            tr!("secrets.rules", n = rules),
            tr!("secrets.rules_about"),
        ))
        .child(check(
            Level::Ok,
            tr!("secrets.known", n = known),
            tr!("secrets.known_about"),
        ))
}

fn try_it() -> View {
    let input = signal(state::last_scanned());
    card()
        .test_id("scan")
        .child(heading(tr!("secrets.try")))
        .child(muted(tr!("secrets.try_about")))
        .child(
            text_box(input, tr!("secrets.try_placeholder"))
                .test_id("scan-input")
                .multiline(true)
                .min_h(px(80.0)),
        )
        .child(
            view().flex_row().pt_2().child(
                primary(tr!("secrets.scan"), move || {
                    state::scan(input.get_untracked())
                })
                .busy(state::running(Work::Scan)),
            ),
        )
        .child(dynamic(move || {
            state::scan_result()
                .map(|hits| {
                    if hits.is_empty() {
                        return vec![
                            check(
                                Level::Ok,
                                tr!("secrets.nothing"),
                                tr!("secrets.nothing_about"),
                            )
                            .into_element(),
                        ];
                    }
                    hits.into_iter().map(|h| hit(h).into_element()).collect()
                })
                .unwrap_or_default()
        }))
}

fn hit(h: Hit) -> View {
    let (here, everywhere) = (h.fingerprint.clone(), h.fingerprint.clone());
    let (preview, fingerprint) = (h.preview.clone(), h.fingerprint.clone());
    let allowing =
        |vault| state::running(Work::Allow(vault, Kind::Fingerprint, h.fingerprint.clone()));
    check(
        Level::Error,
        format!("{}: {}", h.rule, h.description),
        tr!("secrets.hit", preview = preview, fingerprint = fingerprint),
    )
    .child(
        view()
            .gap_1()
            .child(
                action(tr!("secrets.allow_here"), move || {
                    state::allow(Vault::Project, Kind::Fingerprint, here.clone())
                })
                .busy(allowing(Vault::Project)),
            )
            .child(
                action(tr!("secrets.allow_everywhere"), move || {
                    state::allow(Vault::Personal, Kind::Fingerprint, everywhere.clone())
                })
                .busy(allowing(Vault::Personal)),
            ),
    )
}

fn allowlist(list: &Result<Allowlist, String>) -> View {
    let mut c = card()
        .test_id("project-allowlist")
        .child(heading(tr!("secrets.allowlist")))
        .child(muted(tr!("secrets.allowlist_about")));
    match list {
        Err(e) => {
            c = c.child(check(
                Level::Error,
                tr!("secrets.allowlist_unreadable"),
                e.clone(),
            ))
        }
        Ok(a) if a.fingerprints.is_empty() => c = c.child(muted(tr!("secrets.allowlist_empty"))),
        Ok(a) => {
            for f in &a.fingerprints {
                let entry = f.clone();
                c =
                    c.child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .py_1()
                            .child(text(f.clone()).flex_1().text_sm())
                            .child(
                                action(tr!("common.remove"), {
                                    let entry = entry.clone();
                                    move || {
                                        state::disallow(
                                            Vault::Project,
                                            Kind::Fingerprint,
                                            entry.clone(),
                                        )
                                    }
                                })
                                .busy(state::running(
                                    Work::Disallow(Vault::Project, Kind::Fingerprint, entry),
                                )),
                            ),
                    );
            }
        }
    }
    c
}
