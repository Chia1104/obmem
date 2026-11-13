//! A project's settings: `.obmem.toml`, and whether Codex may run obmem's tools without asking.

use crate::data::Detail;
use crate::form::{Problems, said};
use crate::state::{self, Work};
use crate::widgets::{
    FirstRow, Level, Marked, card, check, checked_field, choice, field, heading, input_field,
    muted, primary, switch_control, text_box,
};
use crate::words::tr;
use nsh_core::prelude::*;
use obmem_core::config::SecretMode;
use obmem_core::settings::{ProjectField, ProjectSettings, default_env_files};

pub(crate) fn settings_tab(d: &Detail) -> View {
    let form = match &d.settings {
        Ok(s) => project_form(s.clone(), d.status.project_vault.path.display().to_string()),
        Err(e) => card().child(check(
            Level::Error,
            tr!("common.unreadable", file = ".obmem.toml"),
            e.clone(),
        )),
    };
    view().gap_4().child(form).child(codex(d))
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
            field(
                tr!("common.project_vault"),
                tr!("settings.vault_private", path = vault_path),
                view(),
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
