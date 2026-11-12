//! The user's own settings (`~/.config/obmem/config.toml`) and their personal vault's allowlist,
//! which apply to every project.

use crate::data::{self, ConnectField, Personal, SyncInfo, Vault};
use crate::form::{Problems, said};
use crate::state::{self, Load, Work};
use crate::widgets::{
    Level, Marked, action, card, check, checked_field, field, heading, loading, muted, notice_area,
    page, page_header, primary, problem_line, switch_control, text_box,
};
use crate::words::{self, tr};
use nsh_core::prelude::*;
use obmem_core::allowlist::{self, Allowlist, Kind};
use obmem_core::settings::{GlobalField, GlobalSettings, SyncField, SyncSettings};
use obmem_sync::Outcome;

pub(crate) fn personal_page() -> impl IntoElement {
    page(
        page_header(tr!("personal.title"), muted(tr!("personal.about")), vec![]),
        view()
            .gap_4()
            .child(notice_area())
            .child(
                view()
                    .gap_4()
                    .child(dynamic(move || match state::personal() {
                        Load::Loading => vec![loading(tr!("common.loading")).into_element()],
                        Load::Failed(e) => {
                            vec![check(Level::Error, tr!("personal.unreadable"), e).into_element()]
                        }
                        Load::Ready(p) => vec![
                            form(&p).into_element(),
                            sync_card(&p).into_element(),
                            allowlist(&p).into_element(),
                        ],
                    })),
            ),
    )
}

fn form(p: &Personal) -> View {
    let vault = signal(p.settings.vault.clone());
    let scan_env = signal(p.settings.scan_process_env);
    let auto_approve = signal(p.settings.auto_approve);
    let draft = move || GlobalSettings {
        vault: vault.get(),
        scan_process_env: scan_env.get(),
        auto_approve: auto_approve.get(),
    };
    let problems = Problems::of(move || said(draft().problems()));
    let save = move || {
        if problems.none() {
            state::save_personal(untracked(draft));
        }
    };
    let now = p.vault.display().to_string();
    card()
        .test_id("personal-settings")
        .child(heading(tr!("personal.settings")))
        .child(checked_field(
            tr!("common.personal_vault"),
            tr!("personal.vault_help", now = now),
            text_box(vault, "~/Obsidian/AgentMemory")
                .marked(problems.marks(GlobalField::Vault))
                .test_id("personal-vault"),
            problems.at(GlobalField::Vault),
        ))
        .child(field(
            tr!("personal.scan_env"),
            tr!("personal.scan_env_help"),
            switch_control(
                move || scan_env.get(),
                tr!("personal.scan_env"),
                move |v| scan_env.set(v),
            ),
        ))
        .child(field(
            tr!("personal.auto_approve"),
            tr!("personal.auto_approve_help"),
            switch_control(
                move || auto_approve.get(),
                tr!("personal.auto_approve"),
                move |v| auto_approve.set(v),
            ),
        ))
        .child(
            view()
                .flex_row()
                .pt_2()
                .child(primary(tr!("common.save"), save).busy(state::running(Work::SavePersonal))),
        )
}

fn sync_card(p: &Personal) -> View {
    let c = card()
        .test_id("personal-sync")
        .child(heading(tr!("sync.title")))
        .child(muted(tr!("sync.about")));
    let info = match &p.sync {
        Ok(i) => i.clone(),
        Err(e) => {
            return c.child(check(Level::Error, tr!("sync.unreadable"), e.clone()));
        }
    };
    let c = c
        .child(sync_status(&info))
        .child(sync_settings(&info.settings));
    if info.settings.bucket.is_empty() || info.connected == Ok(true) {
        return c;
    }
    c.child(connect_form())
}

fn sync_status(info: &SyncInfo) -> View {
    let row = view().gap_2().pt_2();
    if info.settings.bucket.is_empty() {
        return row.child(muted(tr!("sync.off")));
    }
    match &info.connected {
        Err(e) => {
            return row.child(check(
                Level::Error,
                tr!("sync.keychain_unreadable"),
                e.clone(),
            ));
        }
        Ok(false) => {
            return row.child(check(
                Level::Warn,
                tr!("sync.not_connected"),
                tr!("sync.not_connected_about"),
            ));
        }
        Ok(true) => {}
    }
    let line = match info.last.clone() {
        None => check(
            Level::Warn,
            tr!("sync.not_synced"),
            tr!("sync.not_synced_about"),
        ),
        Some(last) => {
            let (level, title) = match last.outcome {
                Outcome::Failed(_) => (Level::Error, "sync.last_failed"),
                Outcome::Synced { .. } => (Level::Ok, "sync.last"),
            };
            check(
                level,
                tr!(title),
                tr!(
                    "sync.last_about",
                    what = words::outcome(&last.outcome),
                    at = last.at
                ),
            )
        }
    };
    row.child(line).child(
        view()
            .flex_row()
            .child(primary(tr!("sync.now"), state::sync_now).busy(state::running(Work::Sync))),
    )
}

fn sync_settings(s: &SyncSettings) -> View {
    let endpoint = signal(s.endpoint.clone());
    let bucket = signal(s.bucket.clone());
    let region = signal(s.region.clone());
    let prefix = signal(s.prefix.clone());
    let device = signal(s.device.clone());
    let auto = signal(s.auto);
    let plugins = signal(s.community_plugins);
    let draft = move || SyncSettings {
        endpoint: endpoint.get(),
        region: region.get(),
        bucket: bucket.get(),
        prefix: prefix.get(),
        device: device.get(),
        auto: auto.get(),
        community_plugins: plugins.get(),
    };
    let problems = Problems::of(move || said(draft().problems()));
    let save = move || {
        if problems.none() {
            state::save_sync(untracked(draft));
        }
    };
    view()
        .gap_1()
        .child(checked_field(
            tr!("sync.endpoint"),
            tr!("sync.endpoint_help"),
            text_box(endpoint, "https://<account-id>.r2.cloudflarestorage.com")
                .marked(problems.marks(SyncField::Endpoint))
                .test_id("sync-endpoint"),
            problems.at(SyncField::Endpoint),
        ))
        .child(checked_field(
            tr!("sync.bucket"),
            tr!("sync.bucket_help"),
            text_box(bucket, "my-obmem-vault")
                .marked(problems.marks(SyncField::Bucket))
                .test_id("sync-bucket"),
            problems.at(SyncField::Bucket),
        ))
        .child(field(
            tr!("sync.region"),
            tr!("sync.region_help"),
            text_box(region, "auto"),
        ))
        .child(field(
            tr!("sync.prefix"),
            tr!("sync.prefix_help"),
            text_box(prefix, "obmem"),
        ))
        .child(field(
            tr!("sync.device"),
            tr!("sync.device_help"),
            text_box(device, "work-mac").test_id("sync-device"),
        ))
        .child(field(
            tr!("sync.auto"),
            tr!("sync.auto_help"),
            switch_control(move || auto.get(), tr!("sync.auto"), move |v| auto.set(v)),
        ))
        .child(field(
            tr!("sync.plugins"),
            tr!("sync.plugins_help"),
            switch_control(
                move || plugins.get(),
                tr!("sync.plugins_switch"),
                move |v| plugins.set(v),
            ),
        ))
        .child(
            view()
                .flex_row()
                .pt_2()
                .child(primary(tr!("common.save"), save).busy(state::running(Work::SaveSync))),
        )
}

fn connect_form() -> View {
    let key_id = signal(String::new());
    let secret = signal(String::new());
    let passphrase = signal(String::new());
    let repeat = signal(String::new());
    let problems = Problems::of(move || {
        data::connect_problems(
            &key_id.get(),
            &secret.get(),
            &passphrase.get(),
            &repeat.get(),
        )
        .into_iter()
        .map(|field| {
            let rule = match field {
                ConnectField::KeyId => t!("rule.key_id_needed"),
                ConnectField::Secret => t!("rule.secret_needed"),
                ConnectField::Passphrase => t!("rule.passphrase_needed"),
                ConnectField::Repeat => t!("rule.passphrases_differ"),
            };
            (field, rule)
        })
        .collect()
    });
    let connect = move || {
        if problems.none() {
            state::connect_sync(
                key_id.get_untracked(),
                secret.get_untracked(),
                passphrase.get_untracked(),
                repeat.get_untracked(),
            )
        }
    };
    // `label` is a translation key.
    let row = move |field: ConnectField,
                    label: &'static str,
                    value: Signal<String>,
                    masked: bool,
                    id: &str| {
        checked_field(
            tr!(label),
            "",
            text_box(value, tr!(label))
                .marked(problems.marks(field))
                .masked(masked)
                .test_id(id),
            problems.at(field),
        )
    };
    view()
        .test_id("sync-connect")
        .gap_1()
        .pt_3()
        .child(text(tr!("sync.connect_title")).font_weight(FontWeight::MEDIUM))
        .child(muted(tr!("sync.connect_about")))
        .child(row(
            ConnectField::KeyId,
            "sync.key_id",
            key_id,
            false,
            "connect-key-id",
        ))
        .child(row(
            ConnectField::Secret,
            "sync.secret",
            secret,
            true,
            "connect-secret",
        ))
        .child(row(
            ConnectField::Passphrase,
            "sync.passphrase",
            passphrase,
            true,
            "connect-passphrase",
        ))
        .child(row(
            ConnectField::Repeat,
            "sync.repeat",
            repeat,
            true,
            "connect-repeat",
        ))
        .child(
            view().flex_row().pt_2().child(
                primary(tr!("sync.connect"), connect).busy(state::running(Work::ConnectSync)),
            ),
        )
}

fn allowlist(p: &Personal) -> View {
    let mut c = card()
        .test_id("personal-allowlist")
        .child(heading(tr!("allowlist.title")))
        .child(muted(tr!("allowlist.about")));
    let a = match &p.allowlist {
        Ok(a) => a.clone(),
        Err(e) => {
            return c.child(check(Level::Error, tr!("allowlist.unreadable"), e.clone()));
        }
    };
    let Allowlist {
        fingerprints,
        regexes,
        stopwords,
    } = a;
    c = c.child(entries(Kind::Fingerprint, fingerprints, false));
    c = c.child(entries(Kind::Regex, regexes, true));
    c.child(entries(Kind::Stopword, stopwords, true))
}

/// One kind of entry: what tests find it by, which is also the end of its translation keys
/// (`allowlist.<id>`, `allowlist.<id>_help`, `allowlist.add_<id>`).
fn entries(kind: Kind, list: Vec<String>, addable: bool) -> View {
    let id = match kind {
        Kind::Fingerprint => "fingerprints",
        Kind::Regex => "patterns",
        Kind::Stopword => "words",
    };
    let mut block = view()
        .test_id(&format!("entries-{id}"))
        .gap_1()
        .pt_3()
        .child(text(move || t!(&format!("allowlist.{id}"))).font_weight(FontWeight::MEDIUM))
        .child(muted(move || t!(&format!("allowlist.{id}_help"))));
    if list.is_empty() {
        block = block.child(muted(tr!("allowlist.none")));
    }
    for e in list {
        let entry = e.clone();
        block = block.child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .py_1()
                .child(text(e).flex_1().text_sm())
                .child(
                    action(tr!("common.remove"), {
                        let entry = entry.clone();
                        move || state::disallow(Vault::Personal, kind, entry.clone())
                    })
                    .busy(state::running(Work::Disallow(
                        Vault::Personal,
                        kind,
                        entry,
                    ))),
                ),
        );
    }
    if addable {
        let draft = signal(String::new());
        // The scanner's own rule for an entry: one it would ignore is not added.
        let problems = Problems::of(move || {
            let entry = draft.get();
            allowlist::problem(kind, &entry)
                .map(|why| ((), words::ignored(&entry, &why)))
                .into_iter()
                .collect()
        });
        let add = move || {
            if problems.none() {
                state::allow(Vault::Personal, kind, draft.get_untracked())
            }
        };
        let adding = move || {
            state::running(Work::Allow(
                Vault::Personal,
                kind,
                draft.get().trim().to_string(),
            ))()
        };
        let problem = problems.at(());
        block = block
            .child(
                view()
                    .flex_row()
                    .gap_2()
                    .pt_1()
                    .child(
                        text_box(draft, move || t!(&format!("allowlist.add_{id}")))
                            .marked(problems.marks(()))
                            .test_id(&format!("add-{id}"))
                            .flex_1(),
                    )
                    .child(action(tr!("common.add"), add).busy(adding)),
            )
            .child(problem_line(problem));
    }
    block
}
