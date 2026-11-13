//! The pages of the settings that are about the personal vault, which apply to every project:
//! the vault itself (`~/.config/obmem/config.toml`), syncing it, and its allowlist.

use crate::data::{self, ConnectField, Personal, SyncInfo, Vault};
use crate::form::{Problems, said};
use crate::state::{self, Load, Work};
use crate::widgets::{
    FirstRow, Level, Marked, about, action, card, check, checked_field, dialog_box, field, heading,
    input_field, loading, muted, notice_area, page, page_bar, primary, problem_line,
    switch_control, text_box,
};
use crate::words::{self, tr};
use nsh_core::prelude::*;
use obmem_core::allowlist::{self, Allowlist, Kind};
use obmem_core::settings::{GlobalField, GlobalSettings, SyncField, SyncSettings};
use obmem_sync::Outcome;

/// Where the personal vault is, and how agents' notes get into it.
pub(crate) fn vault_page() -> impl IntoElement {
    personal("common.personal_vault", "personal.about", |p| {
        vec![form(p).into_element()]
    })
}

/// Syncing the personal vault between devices.
pub(crate) fn sync_page() -> impl IntoElement {
    personal("sync.title", "sync.about", sync_blocks)
}

/// What the secret scanner lets pass in every project.
pub(crate) fn secrets_page() -> impl IntoElement {
    personal("prefs.secrets", "allowlist.about", |p| {
        vec![allowlist(p).into_element()]
    })
}

/// A page about the personal vault: `title` and `about` are translation keys, and `blocks` is
/// what it shows once the settings are read.
fn personal(
    title: &'static str,
    said: &'static str,
    blocks: fn(&Personal) -> Vec<AnyElement>,
) -> impl IntoElement {
    page(
        page_bar(tr!(title), view(), vec![]),
        view()
            .gap_4()
            .child(about(tr!(said)))
            .child(notice_area())
            .child(
                view()
                    .gap_4()
                    .child(dynamic(move || match state::personal() {
                        Load::Loading => vec![loading(tr!("common.loading")).into_element()],
                        Load::Failed(e) => {
                            vec![check(Level::Error, tr!("personal.unreadable"), e).into_element()]
                        }
                        Load::Ready(p) => blocks(&p),
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
        .child(
            checked_field(
                tr!("common.personal_vault"),
                tr!("personal.vault_help", now = now),
                text_box(vault, "~/Obsidian/AgentMemory")
                    .marked(problems.marks(GlobalField::Vault))
                    .test_id("personal-vault"),
                problems.at(GlobalField::Vault),
            )
            .first_row(),
        )
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

/// How the sync stands, with what can be done about it, then its settings.
fn sync_blocks(p: &Personal) -> Vec<AnyElement> {
    let info = match &p.sync {
        Ok(i) => i.clone(),
        Err(e) => {
            return vec![
                card()
                    .test_id("personal-sync")
                    .child(check(Level::Error, tr!("sync.unreadable"), e.clone()))
                    .into_element(),
            ];
        }
    };
    vec![
        sync_status(&info).into_element(),
        card()
            .test_id("personal-sync")
            .child(heading(tr!("sync.settings")))
            .child(sync_settings(&info.settings))
            .into_element(),
    ]
}

fn sync_status(info: &SyncInfo) -> View {
    let block = card().test_id("sync-status").gap_2();
    if info.settings.bucket.is_empty() {
        return block.child(muted(tr!("sync.off")));
    }
    let why = match &info.connected {
        Ok(true) => None,
        Ok(false) => Some(check(
            Level::Warn,
            tr!("sync.not_connected"),
            tr!("sync.not_connected_about"),
        )),
        Err(e) => Some(check(
            Level::Error,
            tr!("sync.keychain_unreadable"),
            e.clone(),
        )),
    };
    if let Some(why) = why {
        // Made with the page: the dialog closes when the settings are read again, which a
        // connection ends with.
        let connecting = signal(false);
        let close = move || connecting.set(false);
        return block
            .child(why)
            .child(
                view()
                    .flex_row()
                    .child(primary(tr!("sync.connect_title"), move || {
                        connecting.set(true)
                    })),
            )
            .child(dynamic(move || {
                connecting.get().then(|| connect_dialog(close))
            }));
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
    block.child(line).child(
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
        .child(
            checked_field(
                tr!("sync.endpoint"),
                tr!("sync.endpoint_help"),
                text_box(endpoint, "https://<account-id>.r2.cloudflarestorage.com")
                    .marked(problems.marks(SyncField::Endpoint))
                    .test_id("sync-endpoint"),
                problems.at(SyncField::Endpoint),
            )
            .first_row(),
        )
        .child(checked_field(
            tr!("sync.bucket"),
            tr!("sync.bucket_help"),
            text_box(bucket, "my-obmem-vault")
                .marked(problems.marks(SyncField::Bucket))
                .test_id("sync-bucket"),
            problems.at(SyncField::Bucket),
        ))
        .child(input_field(
            tr!("sync.region"),
            tr!("sync.region_help"),
            text_box(region, "auto"),
        ))
        .child(input_field(
            tr!("sync.prefix"),
            tr!("sync.prefix_help"),
            text_box(prefix, "obmem"),
        ))
        .child(input_field(
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

/// Connecting this device: the bucket's access key and the vault's passphrase, asked in a
/// dialog. What is typed is gone when it closes.
fn connect_dialog(close: impl Fn() + Clone + 'static) -> AnyElement {
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
    // `label` is a translation key. A dialog is narrow: the label goes above its box.
    let row = move |field: ConnectField,
                    label: &'static str,
                    value: Signal<String>,
                    masked: bool,
                    id: &str| {
        view()
            .gap_1()
            .child(text(tr!(label)).text_sm().font_weight(FontWeight::MEDIUM))
            .child(
                text_box(value, "")
                    .marked(problems.marks(field))
                    .masked(masked)
                    .test_id(id),
            )
            .child(problem_line(problems.at(field)))
    };
    let fields = view()
        .gap_3()
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
        ));
    dialog_box(
        "sync-connect",
        tr!("sync.connect_title"),
        460.0,
        fields,
        vec![
            action(tr!("common.cancel"), close.clone()).into_element(),
            primary(tr!("sync.connect"), connect)
                .busy(state::running(Work::ConnectSync))
                .into_element(),
        ],
        close,
    )
}

fn allowlist(p: &Personal) -> View {
    let mut c = card()
        .test_id("personal-allowlist")
        .child(heading(tr!("allowlist.title")));
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
