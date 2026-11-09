//! Settings, Codex, secrets and personal settings, as a user changes them.

mod common;
use common::{SLOW, project, sandbox, start};
use obmem_agents::hooks::Agent;
use obmem_core::config::{Config, SecretMode};
use obmem_core::{allowlist, config, init, settings};

#[test]
fn settings_secrets_and_personal_settings_are_saved() {
    let dir = sandbox("settings");
    let repo = project(&dir, "settings-repo");
    let cfg = Config::load(Some(&repo)).unwrap();
    init::run(&cfg).unwrap();
    obmem_agents::setup::setup(&cfg, Agent::Codex).unwrap();

    let mut app = start();
    app.wait_for_within("role:button[Settings]", SLOW).unwrap();
    app.press("role:button[Settings]").unwrap();
    app.wait_for_within("id:project-settings", SLOW).unwrap();

    // A save the rules refuse reports why and keeps what the user typed.
    app.clear("id:settings-vault").unwrap();
    app.type_text("id:settings-vault", "../elsewhere").unwrap();
    app.press("id:project-settings >> role:button[Save]").unwrap();
    app.wait_for_within("That did not work", SLOW).unwrap();
    assert_eq!(app.screen().input_text("id:settings-vault").unwrap(), Some("../elsewhere"));

    app.clear("id:settings-vault").unwrap();
    app.type_text("id:settings-vault", "docs/kb").unwrap();
    app.clear("id:settings-name").unwrap();
    app.type_text("id:settings-name", "renamed").unwrap();
    app.press("role:button[Redact]").unwrap();
    app.press("id:project-settings >> role:button[Save]").unwrap();
    app.wait_for_within("Settings saved.", SLOW).unwrap();
    let saved = settings::read_project(&repo).unwrap();
    assert_eq!((saved.name.as_str(), saved.secret_mode), ("renamed", SecretMode::Redact));
    assert_eq!(Config::load(Some(&repo)).unwrap().project_name, "renamed");

    app.press("id:codex >> role:button[On]").unwrap();
    app.wait_for_within("Codex now runs obmem's tools without asking.", SLOW).unwrap();
    assert_eq!(obmem_agents::setup::codex_auto_approve(&cfg).unwrap(), Some(true));

    // A token-shaped test value (split so the source holds no literal token).
    let token = concat!("gh", "p_", "Zr8Lx2Vb6Nc9Tw3Yh7Kp1Sd5Fg0Jk2LmAq9W");
    app.press("role:button[Secrets]").unwrap();
    app.wait_for_within("id:scan-input", SLOW).unwrap();
    app.type_text("id:scan-input", &format!("deploy with {token}")).unwrap();
    app.press("role:button[Scan]").unwrap();
    app.wait_for_within("role:button[Allow in this project]", SLOW).unwrap();
    app.press("role:button[Allow in this project]").unwrap();
    // The text is scanned again with the new allowlist.
    app.wait_for_within("Nothing found", SLOW).unwrap();
    assert_eq!(allowlist::read(&cfg.project_vault).unwrap().fingerprints.len(), 1);

    app.press("role:button[Personal settings]").unwrap();
    app.wait_for_within("id:personal-settings", SLOW).unwrap();
    app.type_text("id:add-words", "sandbox-only").unwrap();
    app.press("id:entries-words >> role:button[Add]").unwrap();
    app.wait_for_within("Allowed in every project.", SLOW).unwrap();
    let personal_vault = config::global_vault().unwrap();
    assert_eq!(allowlist::read(&personal_vault).unwrap().stopwords, vec!["sandbox-only"]);

    app.press("id:personal-settings >> role:button[Off]").unwrap();
    app.press("id:personal-settings >> role:button[Save]").unwrap();
    app.wait_for_within("Personal settings saved.", SLOW).unwrap();
    assert!(!settings::read_global().unwrap().scan_process_env);

    let _ = std::fs::remove_dir_all(&dir);
}
