//! Settings, Codex, secrets and personal settings, as a user changes them.

use crate::common::{SLOW, open_settings, project, sandbox, show_settings, start};
use obmem_agents::hooks::Agent;
use obmem_core::config::{Config, SecretMode};
use obmem_core::{allowlist, config, init, settings};

#[test]
fn settings_secrets_and_personal_settings_are_saved() {
    let Some(dir) = sandbox("settings") else {
        return;
    };
    let repo = project(&dir, "settings-repo");
    let cfg = Config::load(Some(&repo)).unwrap();
    init::run(&cfg).unwrap();
    obmem_agents::setup::setup(&cfg, Agent::Codex).unwrap();

    let mut app = start();
    app.wait_for_within("role:tab[Settings]", SLOW).unwrap();
    app.press("role:tab[Settings]").unwrap();
    app.wait_for_within("id:project-settings", SLOW).unwrap();

    // Values that break a rule are not saved. Each field says what is wrong with it, all of
    // them at once, and keeps what the user typed.
    const VAULT_RULE: &str =
        "The vault must be a relative path inside the project, without `..` or `~`.";
    const STALE_RULE: &str = "Stale days must be a whole number.";
    app.clear("id:settings-vault").unwrap();
    app.type_text("id:settings-vault", "../elsewhere").unwrap();
    app.clear("id:settings-stale").unwrap();
    app.type_text("id:settings-stale", "soon").unwrap();
    assert!(
        !app.screen().shows(VAULT_RULE),
        "nothing is marked before the first save"
    );
    app.press("id:project-settings >> role:button[Save]")
        .unwrap();
    app.wait_for_within(VAULT_RULE, SLOW).unwrap();
    assert!(app.screen().shows(STALE_RULE));
    assert_eq!(
        app.screen().input_text("id:settings-vault").unwrap(),
        Some("../elsewhere")
    );
    assert_eq!(
        settings::read_project(&repo).unwrap().vault,
        "docs/kb",
        "nothing was written"
    );

    // A problem goes as soon as its field is put right, without another save.
    app.clear("id:settings-vault").unwrap();
    app.type_text("id:settings-vault", "docs/kb").unwrap();
    app.wait_for_gone(&format!("text:{VAULT_RULE}"), SLOW)
        .unwrap();
    assert!(app.screen().shows(STALE_RULE));
    app.clear("id:settings-stale").unwrap();
    app.type_text("id:settings-stale", "90").unwrap();
    app.wait_for_gone(&format!("text:{STALE_RULE}"), SLOW)
        .unwrap();
    app.clear("id:settings-name").unwrap();
    app.type_text("id:settings-name", "renamed").unwrap();
    app.press("role:button[Redact]").unwrap();
    app.press("id:project-settings >> role:button[Save]")
        .unwrap();
    app.wait_for_within("Settings saved.", SLOW).unwrap();
    let saved = settings::read_project(&repo).unwrap();
    assert_eq!(
        (saved.name.as_str(), saved.secret_mode, saved.stale_days),
        ("renamed", SecretMode::Redact, 90)
    );
    assert_eq!(Config::load(Some(&repo)).unwrap().project_name, "renamed");

    app.press("id:codex >> role:switch").unwrap();
    app.wait_for_within("Codex now runs obmem's tools without asking.", SLOW)
        .unwrap();
    assert_eq!(
        obmem_agents::setup::codex_auto_approve(&cfg).unwrap(),
        Some(true)
    );

    // A token-shaped test value (split so the source holds no literal token).
    let token = concat!("gh", "p_", "Zr8Lx2Vb6Nc9Tw3Yh7Kp1Sd5Fg0Jk2LmAq9W");
    app.press("role:tab[Secrets]").unwrap();
    app.wait_for_within("id:scan-input", SLOW).unwrap();
    app.type_text("id:scan-input", &format!("deploy with {token}"))
        .unwrap();
    app.press("role:button[Scan]").unwrap();
    app.wait_for_within("role:button[Allow in this project]", SLOW)
        .unwrap();
    app.press("role:button[Allow in this project]").unwrap();
    // The text is scanned again with the new allowlist.
    app.wait_for_within("Nothing found", SLOW).unwrap();
    assert_eq!(
        allowlist::read(&cfg.project_vault)
            .unwrap()
            .fingerprints
            .len(),
        1
    );

    open_settings(&mut app, "secrets");
    app.wait_for_within("id:personal-allowlist", SLOW).unwrap();
    // An entry the scanner would ignore is not added, and its box says why.
    const SHORT_WORD: &str = "The scanner would ignore \"abc\": it is shorter than 4 characters.";
    app.type_text("id:add-words", "abc").unwrap();
    app.press("id:entries-words >> role:button[Add]").unwrap();
    app.wait_for_within(SHORT_WORD, SLOW).unwrap();
    app.clear("id:add-words").unwrap();
    app.type_text("id:add-words", "sandbox-only").unwrap();
    app.wait_for_gone(&format!("text:{SHORT_WORD}"), SLOW)
        .unwrap();
    app.press("id:entries-words >> role:button[Add]").unwrap();
    app.wait_for_within("Allowed in every project.", SLOW)
        .unwrap();
    let personal_vault = config::global_vault().unwrap();
    assert_eq!(
        allowlist::read(&personal_vault).unwrap().stopwords,
        vec!["sandbox-only"]
    );

    // The personal vault's rule shows at its box, and what was typed stays.
    show_settings(&mut app, "vault");
    app.wait_for_within("id:personal-settings", SLOW).unwrap();
    const PERSONAL_RULE: &str = "The personal vault must be an absolute path or start with ~/.";
    let vault_before = settings::read_global().unwrap().vault;
    app.clear("id:personal-vault").unwrap();
    app.type_text("id:personal-vault", "relative/path").unwrap();
    app.press("id:personal-settings >> role:button[Save]")
        .unwrap();
    app.wait_for_within(PERSONAL_RULE, SLOW).unwrap();
    assert_eq!(
        app.screen().input_text("id:personal-vault").unwrap(),
        Some("relative/path")
    );
    assert_eq!(settings::read_global().unwrap().vault, vault_before);
    app.clear("id:personal-vault").unwrap();
    app.type_text("id:personal-vault", &vault_before).unwrap();
    app.wait_for_gone(&format!("text:{PERSONAL_RULE}"), SLOW)
        .unwrap();

    app.press("id:personal-settings >> role:switch[Scan environment variables]")
        .unwrap();
    app.press("id:personal-settings >> role:switch[Approve agent notes automatically]")
        .unwrap();
    app.press("id:personal-settings >> role:button[Save]")
        .unwrap();
    app.wait_for_within("Personal settings saved.", SLOW)
        .unwrap();
    let saved = settings::read_global().unwrap();
    assert!(!saved.scan_process_env);
    assert!(saved.auto_approve);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sync_settings_are_saved_and_ask_to_connect() {
    let Some(dir) = sandbox("sync-settings") else {
        return;
    };
    let mut app = start();
    open_settings(&mut app, "sync");
    app.wait_for_within("id:personal-sync", SLOW).unwrap();
    assert!(app.screen().shows("Off: no bucket yet."));

    // The rules the config loader applies show at the field that breaks them. Nothing is
    // saved, and what was typed stays.
    const ENDPOINT_RULE: &str = "The endpoint must be a URL starting with https://.";
    app.type_text("id:sync-endpoint", "acct.r2.cloudflarestorage.com")
        .unwrap();
    app.type_text("id:sync-bucket", "my-vault").unwrap();
    app.press("id:personal-sync >> role:button[Save]").unwrap();
    app.wait_for_within(ENDPOINT_RULE, SLOW).unwrap();
    assert_eq!(
        app.screen().input_text("id:sync-bucket").unwrap(),
        Some("my-vault")
    );
    assert_eq!(settings::read_sync().unwrap().bucket, "");

    app.clear("id:sync-endpoint").unwrap();
    app.type_text("id:sync-endpoint", "https://acct.r2.cloudflarestorage.com")
        .unwrap();
    app.type_text("id:sync-device", "test-box").unwrap();
    app.press("id:personal-sync >> role:switch[Sync automatically]")
        .unwrap();
    app.press("id:personal-sync >> role:button[Save]").unwrap();
    app.wait_for_within("Sync settings saved.", SLOW).unwrap();
    let saved = settings::read_sync().unwrap();
    assert_eq!(
        (saved.bucket.as_str(), saved.device.as_str(), saved.auto),
        ("my-vault", "test-box", false)
    );
    let cfg = config::sync_config().unwrap();
    assert_eq!(
        cfg.endpoint.as_deref(),
        Some("https://acct.r2.cloudflarestorage.com")
    );
    assert!(!cfg.auto_sync());

    // This test bucket is in no keychain: the page asks to connect (unless the keychain itself
    // cannot be read here, which it then says). The card is already on screen and takes the
    // saved settings a moment after the notice, so wait for its old status to go.
    app.wait_for_gone("text:Off: no bucket yet.", SLOW).unwrap();
    let screen = app.screen();
    assert!(
        screen.shows("This device is not connected") || screen.shows("Could not read the keychain"),
        "neither the connect prompt nor a keychain error"
    );

    // Connecting is a dialog, which asks for every value at its own box before it tries
    // anything.
    app.press("id:sync-status >> role:button[Connect this device]")
        .unwrap();
    app.wait_for_within("id:sync-connect", SLOW).unwrap();
    app.type_text("id:connect-passphrase", "one").unwrap();
    app.type_text("id:connect-repeat", "another").unwrap();
    app.press("id:sync-connect >> role:button[Connect]")
        .unwrap();
    app.wait_for_within("Enter the bucket's access key ID.", SLOW)
        .unwrap();
    assert!(app.screen().shows("Enter the bucket's secret access key."));
    assert!(app.screen().shows("The passphrases differ."));
    let _ = std::fs::remove_dir_all(&dir);
}
