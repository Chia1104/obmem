//! Appearance: the mode and each appearance's theme are picked and kept in `appearance.toml`, a
//! theme is copied to a file of the user's, and files put in the themes folder are listed.

use crate::common::{SLOW, sandbox, start};
use nsh_testing::Headless;
use std::path::Path;

const OPEN: &str = "id:screen-appearance >> role:button";

/// Waits until `appearance.toml` has `line`: it is written in the background.
fn kept(app: &mut Headless, file: &Path, line: &str) {
    app.wait_until(line, SLOW, |_| {
        std::fs::read_to_string(file).is_ok_and(|text| text.contains(line))
    })
    .unwrap();
}

#[test]
fn the_look_is_picked_and_kept_and_themes_are_files() {
    let Some(dir) = sandbox("appearance") else {
        return;
    };
    let file = dir.join("config/appearance.toml");
    let themes = dir.join("config/themes");

    let mut app = start();
    app.wait_for_within(OPEN, SLOW).unwrap();
    app.press(OPEN).unwrap();
    app.wait_for_within("text:Light theme", SLOW).unwrap();
    assert!(
        app.screen().shows("None yet."),
        "{:?}",
        app.screen().texts()
    );

    // A pick shows at once and is kept.
    app.press("id:light-theme >> role:radio[Sepia]").unwrap();
    kept(&mut app, &file, "light = \"sepia\"");
    app.press("id:appearance-mode >> role:button[Dark]")
        .unwrap();
    kept(&mut app, &file, "mode = \"dark\"");

    // Duplicate saves the shown theme as a file of the user's and shows the copy.
    app.press("id:dark-theme >> role:button[Duplicate]")
        .unwrap();
    app.wait_for_within("id:dark-theme >> role:radio[Ink copy]", SLOW)
        .unwrap();
    kept(&mut app, &file, "dark = \"ink-copy\"");
    assert!(themes.join("ink-copy.json").is_file());
    app.wait_for_within("text:ink-copy.json", SLOW).unwrap();

    // A file put in the folder is a theme the next time the screen opens; one that is not a
    // theme file is listed with the reason, and cannot be picked.
    std::fs::write(
        themes.join("mine.json"),
        r##"{ "name": "Mine", "cssVars": { "light": { "primary": "#aa0000" } } }"##,
    )
    .unwrap();
    std::fs::write(themes.join("broken.json"), "not a theme").unwrap();
    app.press(OPEN).unwrap();
    app.wait_for_within("id:light-theme >> role:radio[Mine]", SLOW)
        .unwrap();
    app.wait_until("why broken.json is no theme", SLOW, |screen| {
        screen.texts().iter().any(|t| t.contains("It is not JSON"))
    })
    .unwrap();
    assert!(
        app.wait_for("id:light-theme >> role:radio[broken]")
            .is_err(),
        "a broken theme is not offered"
    );
    drop(app);

    // The next start reads what was kept. The copy is gone by then: the default shows instead,
    // and the screen says so.
    std::fs::remove_file(themes.join("ink-copy.json")).unwrap();
    let mut app = start();
    app.wait_for_within(OPEN, SLOW).unwrap();
    app.press(OPEN).unwrap();
    app.wait_for_within("text:The theme ink-copy cannot be shown", SLOW)
        .unwrap();
    app.press("id:dark-theme >> role:radio[Lagoon]").unwrap();
    app.wait_for_gone("text:The theme ink-copy cannot be shown", SLOW)
        .unwrap();
    kept(&mut app, &file, "dark = \"lagoon\"");
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("light = \"sepia\"")
    );

    let _ = std::fs::remove_dir_all(&dir);
}
