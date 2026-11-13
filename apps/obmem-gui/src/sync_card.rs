//! How a vault's sync stands, and connecting this device to its bucket. The personal vault's is
//! on the settings' Sync page; a private project vault's is on the project's Settings tab.

use crate::data::{self, ConnectField, SyncInfo, Vault};
use crate::form::Problems;
use crate::state::{self, Work};
use crate::widgets::{
    Level, Marked, action, card, check, dialog_box, heading, muted, primary, problem_line, text_box,
};
use crate::words::{self, tr};
use nsh_core::prelude::*;
use obmem_sync::Outcome;

/// How a vault's sync stands, with what can be done about it: connect this device, or sync now.
/// The personal vault's is a block of its own; a project vault's begins with what it is.
pub(crate) fn status(info: &SyncInfo, vault: Vault) -> View {
    let project = vault == Vault::Project;
    let mut block = card()
        .test_id(if project {
            "project-sync-status"
        } else {
            "sync-status"
        })
        .gap_2();
    if project {
        block = block
            .child(heading(tr!("project_sync.title")))
            .child(muted(tr!("project_sync.about")));
    }
    if let Some(problem) = info.problem.clone() {
        return block.child(check(Level::Error, tr!("project_sync.unusable"), problem));
    }
    if info.settings.bucket.is_empty() {
        return block.child(muted(tr!(if project {
            "project_sync.off"
        } else {
            "sync.off"
        })));
    }
    let why = match &info.connected {
        Ok(true) => None,
        Ok(false) => Some(check(
            Level::Warn,
            tr!("sync.not_connected"),
            tr!(if project {
                "project_sync.not_connected_about"
            } else {
                "sync.not_connected_about"
            }),
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
                connecting.get().then(|| connect_dialog(vault, close))
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
        view().flex_row().child(
            primary(tr!("sync.now"), move || state::sync_now(vault))
                .busy(state::running(Work::Sync(vault))),
        ),
    )
}

/// Connecting this device: the bucket's access key and the vault's passphrase, asked in a
/// dialog. What is typed is gone when it closes.
fn connect_dialog(vault: Vault, close: impl Fn() + Clone + 'static) -> AnyElement {
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
                vault,
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
        .child(muted(tr!(if vault == Vault::Project {
            "project_sync.connect_about"
        } else {
            "sync.connect_about"
        })))
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
                .busy(state::running(Work::ConnectSync(vault)))
                .into_element(),
        ],
        close,
    )
}
