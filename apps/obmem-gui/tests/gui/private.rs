//! A project's vault made private, and a private project on a computer that has no vault for it.

use crate::common::{SLOW, project, sandbox, start};
use obmem_core::config::{Config, PROJECT_CONFIG_FILE};
use obmem_core::{init, settings};

const USE: &str = "id:private-vault >> role:button[Use this folder]";

#[test]
fn a_projects_vault_is_made_private_from_its_settings() {
    let Some(dir) = sandbox("private") else {
        return;
    };
    let repo = project(&dir, "private-repo");
    init::run(&Config::load(Some(&repo)).unwrap()).unwrap();

    let mut app = start();
    app.wait_for_within("role:tab[Settings]", SLOW).unwrap();
    app.press("role:tab[Settings]").unwrap();
    app.wait_for_within("id:settings-make-private", SLOW)
        .unwrap();
    // A vault in the repo travels with git: it has no sync of its own to set up.
    assert!(!app.screen().shows("Sync settings"));
    app.press("id:settings-make-private >> role:button[Make private…]")
        .unwrap();
    app.wait_for_within("id:private-vault", SLOW).unwrap();

    // A folder a private vault cannot be: the field says which rule, and nothing is written.
    const OUTSIDE: &str = "A private vault is kept outside the project.";
    let inside = repo.join("memory").display().to_string();
    app.type_text("id:private-folder", &inside).unwrap();
    assert!(!app.screen().shows(OUTSIDE));
    app.press(USE).unwrap();
    app.wait_for_within(OUTSIDE, SLOW).unwrap();
    assert!(!settings::read_project(&repo).unwrap().private);
    assert!(Config::load(Some(&repo)).is_ok_and(|c| !c.vault_is_private()));

    // The problem is about the folder that was sent: it goes when the field changes.
    let vault = dir.join("memory");
    app.clear("id:private-folder").unwrap();
    app.wait_for_gone(&format!("text:{OUTSIDE}"), SLOW).unwrap();
    app.type_text("id:private-folder", &vault.display().to_string())
        .unwrap();
    app.press(USE).unwrap();

    // The dialog goes, the row shows the folder, and the notes left in the repo are named.
    app.wait_for_within("id:settings-vault-private", SLOW)
        .unwrap();
    let left = format!(
        "The notes in {} were not moved. Move them to the private vault yourself, and take the \
         folder out of git.",
        repo.join("docs/kb").display()
    );
    app.wait_for_within(&left, SLOW).unwrap();
    app.wait_for_gone("id:private-vault", SLOW).unwrap();
    let cfg = Config::load(Some(&repo)).unwrap();
    assert!(cfg.vault_is_private());
    assert!(cfg.project_vault.join("Index.md").is_file());
    assert!(cfg.project_vault.ends_with("memory"));
    assert!(settings::read_project(&repo).unwrap().private);

    // The folder can be changed from the same row.
    app.press("id:settings-vault-private >> role:button[Change folder…]")
        .unwrap();
    app.wait_for_within("id:private-vault", SLOW).unwrap();
    assert_eq!(
        app.screen().input_text("id:private-folder").unwrap(),
        Some(cfg.project_vault.display().to_string().as_str()),
        "the dialog starts from the folder the vault is in"
    );
}

#[test]
fn a_private_project_without_a_vault_here_asks_for_its_folder() {
    let Some(dir) = sandbox("no-vault") else {
        return;
    };
    // A teammate's clone: the repo says its memory is private, this computer names no vault.
    let repo = project(&dir, "cloned-repo");
    std::fs::write(repo.join(PROJECT_CONFIG_FILE), "private = true\n").unwrap();
    assert!(Config::load(Some(&repo)).is_err());

    let mut app = start();
    app.wait_for_within("This project keeps its memory in a private vault", SLOW)
        .unwrap();
    assert!(!app.screen().shows("Could not read this project"));
    app.press("id:no-vault-here >> role:button[Choose the vault's folder…]")
        .unwrap();
    app.wait_for_within("id:private-vault", SLOW).unwrap();
    let vault = dir.join("team-memory");
    app.type_text("id:private-folder", &vault.display().to_string())
        .unwrap();
    app.press(USE).unwrap();

    // The project loads, with its vault where the user said.
    app.wait_for_within("role:tab[Settings]", SLOW).unwrap();
    let cfg = Config::load(Some(&repo)).unwrap();
    assert!(cfg.vault_is_private() && cfg.project_vault.ends_with("team-memory"));
    // The vault is the team's: the folder is there for a sync to fill, and holds no first files
    // that would meet the team's as conflicts.
    assert!(cfg.project_vault.is_dir());
    assert_eq!(std::fs::read_dir(&cfg.project_vault).unwrap().count(), 0);

    // Its sync is set up from Settings.
    app.press("role:tab[Settings]").unwrap();
    app.wait_for_within("id:project-sync-status", SLOW).unwrap();
}

#[test]
fn a_private_vaults_bucket_is_saved_and_asks_to_connect() {
    let Some(dir) = sandbox("project-sync") else {
        return;
    };
    let repo = project(&dir, "team-repo");
    let vault = dir.join("team-memory").display().to_string();
    settings::make_private(&repo, &vault, None).unwrap();
    init::run(&Config::load(Some(&repo)).unwrap()).unwrap();

    let mut app = start();
    app.wait_for_within("role:tab[Settings]", SLOW).unwrap();
    app.press("role:tab[Settings]").unwrap();
    app.wait_for_within("id:project-sync-status", SLOW).unwrap();
    assert!(
        app.screen()
            .shows("Off: no bucket yet. Without one, share the folder your own way.")
    );

    // A value that breaks a rule is not saved, and its field says why.
    const ENDPOINT_RULE: &str = "The endpoint must be a URL starting with https://.";
    const SAVE: &str = "id:project-sync >> role:button[Save]";
    app.type_text("id:project-sync-endpoint", "team.example")
        .unwrap();
    app.type_text("id:project-sync-bucket", "team-vault")
        .unwrap();
    app.press(SAVE).unwrap();
    app.wait_for_within(ENDPOINT_RULE, SLOW).unwrap();
    assert_eq!(settings::read_project_sync(&repo).unwrap().bucket, "");

    // Saved, the bucket is in the user's config under the project, and the device is asked to
    // connect with the team's passphrase.
    app.clear("id:project-sync-endpoint").unwrap();
    app.type_text("id:project-sync-endpoint", "http://127.0.0.1:9")
        .unwrap();
    app.type_text("id:project-sync-prefix", "acme").unwrap();
    app.press(SAVE).unwrap();
    app.wait_for_within("Sync settings saved.", SLOW).unwrap();
    app.wait_for_within("This device is not connected", SLOW)
        .unwrap();
    assert!(
        app.screen()
            .shows("Connect it with the bucket's access key and the team's passphrase.")
    );
    let saved = settings::read_project_sync(&repo).unwrap();
    assert_eq!(
        (
            saved.endpoint.as_str(),
            saved.bucket.as_str(),
            saved.prefix.as_str()
        ),
        ("http://127.0.0.1:9", "team-vault", "acme")
    );
    // The personal vault's own sync is untouched.
    assert_eq!(settings::read_sync().unwrap().bucket, "");

    // The connect dialog is the one the personal vault has, with the team's words.
    app.press("id:project-sync-status >> role:button[Connect this device]")
        .unwrap();
    app.wait_for_within("id:sync-connect", SLOW).unwrap();
    assert!(app.screen().shows(
        "The access key stays in this device's keychain. The first person's passphrase starts \
         the vault in the bucket; everyone else unlocks it with the same one. If it is lost, the \
         bucket cannot be read."
    ));
}
