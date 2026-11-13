//! A project's settings: `.obmem.toml`, and whether Codex may run obmem's tools without asking.

use crate::data::{Detail, Vault};
use crate::form::{Problems, said};
use crate::state::{self, Work};
use crate::widgets::{
    FirstRow, Level, Marked, card, check, checked_field, choice, field, heading, input_field,
    muted, primary, switch_control, text_box,
};
use crate::words::tr;
use crate::{private_vault, sync_card};
use nsh_core::prelude::*;
use obmem_core::config::SecretMode;
use obmem_core::settings::{
    ProjectField, ProjectSettings, SyncField, SyncSettings, default_env_files,
};

pub(crate) fn settings_tab(d: &Detail) -> View {
    let form = match &d.settings {
        Ok(s) => project_form(s.clone(), d.status.project_vault.path.display().to_string()),
        Err(e) => card().child(check(
            Level::Error,
            tr!("common.unreadable", file = ".obmem.toml"),
            e.clone(),
        )),
    };
    view().gap_4().child(form).children(sync(d)).child(codex(d))
}

/// A private vault's sync: how it stands, then the bucket it goes through. A vault in the repo
/// has none, because git carries it.
fn sync(d: &Detail) -> Vec<AnyElement> {
    match &d.sync {
        None => vec![],
        Some(Err(e)) => vec![
            card()
                .test_id("project-sync")
                .child(check(Level::Error, tr!("sync.unreadable"), e.clone()))
                .into_element(),
        ],
        Some(Ok(info)) => vec![
            sync_card::status(info, Vault::Project).into_element(),
            card()
                .test_id("project-sync")
                .child(heading(tr!("sync.settings")))
                .child(bucket_form(&info.settings))
                .into_element(),
        ],
    }
}

/// The bucket's four settings. What is about the machine (its name, automatic sync) is on the
/// settings' Sync page, for every vault.
fn bucket_form(s: &SyncSettings) -> View {
    let endpoint = signal(s.endpoint.clone());
    let bucket = signal(s.bucket.clone());
    let region = signal(s.region.clone());
    let prefix = signal(s.prefix.clone());
    let draft = move || SyncSettings {
        endpoint: endpoint.get(),
        region: region.get(),
        bucket: bucket.get(),
        prefix: prefix.get(),
        ..Default::default()
    };
    let problems = Problems::of(move || said(draft().problems()));
    let save = move || {
        if problems.none() {
            state::save_project_sync(untracked(draft));
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
                    .test_id("project-sync-endpoint"),
                problems.at(SyncField::Endpoint),
            )
            .first_row(),
        )
        .child(checked_field(
            tr!("sync.bucket"),
            tr!("project_sync.bucket_help"),
            text_box(bucket, "acme-memory")
                .marked(problems.marks(SyncField::Bucket))
                .test_id("project-sync-bucket"),
            problems.at(SyncField::Bucket),
        ))
        .child(input_field(
            tr!("sync.region"),
            tr!("sync.region_help"),
            text_box(region, "auto"),
        ))
        .child(input_field(
            tr!("sync.prefix"),
            tr!("project_sync.prefix_help"),
            text_box(prefix, "obmem").test_id("project-sync-prefix"),
        ))
        .child(
            view().flex_row().pt_2().child(
                primary(tr!("common.save"), save).busy(state::running(Work::SaveProjectSync)),
            ),
        )
}

/// `vault_path` is where the project vault is now, which a private project shows in place of
/// the setting: its vault is for the user's own config to name.
fn project_form(s: ProjectSettings, vault_path: String) -> View {
    // The form's own copy: it starts from the file and is written back on save.
    let name = signal(s.name.clone());
    let vault = signal(s.vault.clone());
    let private = s.private;
    let mode = signal(s.secret_mode);
    let env_files = signal(s.extra_env_files.join("\n"));
    let stale = signal(s.stale_days.to_string());
    let threshold = signal(s.duplicate_threshold.to_string());
    // What the form holds now: the settings, or every rule its values break. The two numbers
    // are typed as text, so they can also fail to be numbers; a stand-in that passes the rules
    // keeps each of them to one problem.
    let draft = move || {
        let mut problems = Vec::new();
        let stale_days = stale.get().trim().parse::<i64>().unwrap_or_else(|_| {
            problems.push((ProjectField::StaleDays, t!("rule.stale_days_whole")));
            1
        });
        let duplicate_threshold = threshold.get().trim().parse::<f64>().unwrap_or_else(|_| {
            problems.push((
                ProjectField::DuplicateThreshold,
                t!("rule.duplicate_threshold_number"),
            ));
            1.0
        });
        let settings = ProjectSettings {
            name: name.get(),
            vault: vault.get(),
            private,
            secret_mode: mode.get(),
            extra_env_files: env_files
                .get()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect(),
            stale_days,
            duplicate_threshold,
        };
        problems.extend(said(settings.problems()));
        if problems.is_empty() {
            Ok(settings)
        } else {
            Err(problems)
        }
    };
    let problems = Problems::of(move || draft().err().unwrap_or_default());
    let save = move || {
        if !problems.none() {
            return;
        }
        if let Ok(settings) = untracked(draft) {
            state::save_settings(settings);
        }
    };
    card()
        .test_id("project-settings")
        .child(heading(tr!("settings.title")))
        .child(muted(tr!("settings.about")))
        .child(
            input_field(
                tr!("settings.name"),
                tr!("settings.name_help"),
                text_box(name, tr!("settings.name_placeholder")).test_id("settings-name"),
            )
            .first_row(),
        )
        .child(if private {
            let shown = vault_path.clone();
            field(
                tr!("common.project_vault"),
                tr!("settings.vault_private", path = shown),
                private_vault::offer("private.change", Some(vault_path), false),
            )
            .test_id("settings-vault-private")
        } else {
            checked_field(
                tr!("common.project_vault"),
                tr!("settings.vault_help"),
                text_box(vault, "docs/kb")
                    .marked(problems.marks(ProjectField::Vault))
                    .test_id("settings-vault"),
                problems.at(ProjectField::Vault),
            )
        })
        .children((!private).then(|| {
            field(
                tr!("private.row"),
                tr!("private.row_help"),
                private_vault::offer("private.make", None, false),
            )
            .test_id("settings-make-private")
        }))
        .child(field(
            tr!("settings.secret_mode"),
            tr!("settings.secret_mode_help"),
            choice(
                &[
                    (SecretMode::Block, "settings.refuse"),
                    (SecretMode::Redact, "settings.redact"),
                ],
                move || mode.get(),
                move |m| mode.set(m),
            ),
        ))
        .child(checked_field(
            tr!("settings.env_files"),
            tr!(
                "settings.env_files_help",
                always = default_env_files().join(", ")
            ),
            text_box(env_files, "config/.env.secrets")
                .marked(problems.marks(ProjectField::EnvFiles))
                .test_id("settings-env-files")
                .multiline(true)
                .min_h(px(64.0)),
            problems.at(ProjectField::EnvFiles),
        ))
        .child(checked_field(
            tr!("settings.stale"),
            tr!("settings.stale_help"),
            text_box(stale, "180")
                .marked(problems.marks(ProjectField::StaleDays))
                .test_id("settings-stale"),
            problems.at(ProjectField::StaleDays),
        ))
        .child(checked_field(
            tr!("settings.threshold"),
            tr!("settings.threshold_help"),
            text_box(threshold, "0.75")
                .marked(problems.marks(ProjectField::DuplicateThreshold))
                .test_id("settings-threshold"),
            problems.at(ProjectField::DuplicateThreshold),
        ))
        .child(
            view()
                .flex_row()
                .pt_2()
                .child(primary(tr!("common.save"), save).busy(state::running(Work::SaveSettings))),
        )
}

fn codex(d: &Detail) -> View {
    let control: AnyElement = match d.codex_auto_approve {
        Ok(None) => muted(tr!("codex.not_set_up")).into_element(),
        Ok(Some(on)) => switch_control(
            move || on,
            tr!("codex.auto_approve"),
            state::set_codex_auto_approve,
        )
        .into_element(),
        Err(ref e) => check(
            Level::Error,
            tr!("common.unreadable", file = ".codex/config.toml"),
            e.clone(),
        )
        .into_element(),
    };
    card().test_id("codex").child(heading("Codex")).child(
        field(
            tr!("codex.auto_approve"),
            tr!("codex.auto_approve_help"),
            control,
        )
        .first_row(),
    )
}
