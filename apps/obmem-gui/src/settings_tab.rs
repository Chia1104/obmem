//! A project's settings: `.obmem.toml`, and whether Codex may run obmem's tools without asking.

use crate::data::Detail;
use crate::state;
use crate::widgets::{Level, card, check, choice, field, heading, muted, primary, text_box};
use nsh_core::prelude::*;
use obmem_core::config::SecretMode;
use obmem_core::settings::{ProjectSettings, default_env_files};

pub(crate) fn settings_tab(d: &Detail) -> View {
    let form = match &d.settings {
        Ok(s) => project_form(s.clone()),
        Err(e) => card().child(check(Level::Error, "Could not read .obmem.toml", e.clone())),
    };
    view().gap_4().child(form).child(codex(d))
}

fn project_form(s: ProjectSettings) -> View {
    // The form's own copy: it starts from the file and is written back on save.
    let name = signal(s.name.clone());
    let vault = signal(s.vault.clone());
    let mode = signal(s.secret_mode);
    let env_files = signal(s.extra_env_files.join("\n"));
    let stale = signal(s.stale_days.to_string());
    let threshold = signal(s.duplicate_threshold.to_string());
    let save = move || {
        let Ok(stale_days) = stale.get_untracked().trim().parse::<i64>() else {
            return state::report("Stale days must be a whole number.".into());
        };
        let Ok(duplicate_threshold) = threshold.get_untracked().trim().parse::<f64>() else {
            return state::report("The duplicate threshold must be a number such as 0.75.".into());
        };
        state::save_settings(ProjectSettings {
            name: name.get_untracked(),
            vault: vault.get_untracked(),
            secret_mode: mode.get_untracked(),
            extra_env_files: env_files.get_untracked().lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect(),
            stale_days,
            duplicate_threshold,
        });
    };
    card()
        .test_id("project-settings")
        .child(heading("Project settings"))
        .child(muted(".obmem.toml is committed with the project, so these apply to everyone working on it."))
        .child(field("Name", "The folder for this project's session logs in your personal vault. Empty: the folder's name.", text_box(name, "Folder name").test_id("settings-name")))
        .child(field("Project vault", "Where the project's notes live, relative to the project.", text_box(vault, "docs/kb").test_id("settings-vault")))
        .child(field(
            "When a write contains a secret",
            "Refuse it, or replace the secret with [REDACTED] and write the rest.",
            choice(&[(SecretMode::Block, "Refuse"), (SecretMode::Redact, "Redact")], move || mode.get(), move |m| mode.set(m)),
        ))
        .child(field(
            "More env files",
            &format!("Their values count as known secrets, one path per line. Always scanned: {}.", default_env_files().join(", ")),
            text_box(env_files, "config/.env.secrets").multiline(true).min_h(px(64.0)),
        ))
        .child(field("Stale after (days)", "Active notes not checked for this long are reported by the audit.", text_box(stale, "180")))
        .child(field(
            "Duplicate threshold",
            "How similar two titles must be (0 to 1) for the audit to call them duplicates.",
            text_box(threshold, "0.75"),
        ))
        .child(view().flex_row().pt_2().child(primary("Save", save)))
}

fn codex(d: &Detail) -> View {
    let control: AnyElement = match d.codex_auto_approve {
        Ok(None) => muted("obmem is not set up for Codex in this project. Set it up in Overview first.").into_element(),
        Ok(Some(on)) => choice(&[(true, "On"), (false, "Off")], move || on, state::set_codex_auto_approve).into_element(),
        Err(ref e) => check(Level::Error, "Could not read .codex/config.toml", e.clone()).into_element(),
    };
    card().test_id("codex").child(heading("Codex")).child(field(
        "Run obmem's tools without asking",
        "Codex asks before each MCP tool call. `codex exec` cannot ask, so it can use obmem only with this on.",
        control,
    ))
}
