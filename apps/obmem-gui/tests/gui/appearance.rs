//! The look, on the settings' Preferences page: the mode and each appearance's theme are picked
//! and kept in `appearance.toml` (a theme in its dialog), a theme is copied to a file of the
//! user's, and files put in the themes folder are listed. A picture for behind the window's
//! content is picked, kept as obmem's own copy and taken away again.

use crate::common::{SLOW, open_settings, sandbox, show_settings, start};
use nsh_testing::Headless;
use std::path::Path;

/// Opens the dialog that picks the theme of `scheme` (`light` or `dark`).
fn pick_theme_of(app: &mut Headless, scheme: &str) {
    app.press(&format!("id:{scheme}-theme-row >> role:button[Change…]"))
        .unwrap();
    app.wait_for_within(&format!("id:{scheme}-theme"), SLOW)
        .unwrap();
}

/// Closes that dialog.
fn done(app: &mut Headless, scheme: &str) {
    app.press(&format!("id:{scheme}-theme >> role:button[Done]"))
        .unwrap();
    app.wait_for_gone(&format!("id:{scheme}-theme"), SLOW)
        .unwrap();
}

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
    open_settings(&mut app, "preferences");
    app.wait_for_within("text:Light theme", SLOW).unwrap();
    assert!(
        app.screen().shows("None yet."),
        "{:?}",
        app.screen().texts()
    );

    // A pick shows at once and is kept.
    pick_theme_of(&mut app, "light");
    app.press("id:light-theme >> role:radio[Sepia]").unwrap();
    kept(&mut app, &file, "light = \"sepia\"");
    done(&mut app, "light");
    app.press("id:appearance-mode >> role:button[Dark]")
        .unwrap();
    kept(&mut app, &file, "mode = \"dark\"");

    // Duplicate saves the shown theme as a file of the user's and shows the copy.
    pick_theme_of(&mut app, "dark");
    app.press("id:dark-theme >> role:button[Duplicate]")
        .unwrap();
    app.wait_for_within("id:dark-theme >> role:radio[Ink copy]", SLOW)
        .unwrap();
    kept(&mut app, &file, "dark = \"ink-copy\"");
    assert!(themes.join("ink-copy.json").is_file());
    done(&mut app, "dark");
    app.wait_for_within("text:ink-copy.json", SLOW).unwrap();

    // A file put in the folder is a theme the next time the screen opens; one that is not a
    // theme file is listed with the reason, and cannot be picked.
    std::fs::write(
        themes.join("mine.json"),
        r##"{ "name": "Mine", "cssVars": { "light": { "primary": "#aa0000" } } }"##,
    )
    .unwrap();
    std::fs::write(themes.join("broken.json"), "not a theme").unwrap();
    show_settings(&mut app, "preferences");
    app.wait_until("why broken.json is no theme", SLOW, |screen| {
        screen.texts().iter().any(|t| t.contains("It is not JSON"))
    })
    .unwrap();
    pick_theme_of(&mut app, "light");
    app.wait_for_within("id:light-theme >> role:radio[Mine]", SLOW)
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
    open_settings(&mut app, "preferences");
    app.wait_for_within("text:The theme ink-copy cannot be shown", SLOW)
        .unwrap();
    pick_theme_of(&mut app, "dark");
    app.press("id:dark-theme >> role:radio[Lagoon]").unwrap();
    kept(&mut app, &file, "dark = \"lagoon\"");
    done(&mut app, "dark");
    app.wait_for_gone("text:The theme ink-copy cannot be shown", SLOW)
        .unwrap();
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("light = \"sepia\"")
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picture_is_kept_as_a_copy_and_taken_away() {
    let Some(dir) = sandbox("background") else {
        return;
    };
    let file = dir.join("config/appearance.toml");
    let copies = dir.join("config/backgrounds");
    let picture = dir.join("hills.png");
    std::fs::write(&picture, "a picture").unwrap();
    std::fs::write(dir.join("notes.txt"), "no picture").unwrap();
    let choose = "id:background-row >> role:button[Choose a picture]";
    let remove = "id:background-row >> role:button[Remove]";

    let mut app = start();
    open_settings(&mut app, "preferences");
    app.wait_for_within(choose, SLOW).unwrap();
    assert!(app.wait_for(remove).is_err(), "there is none to remove yet");
    assert!(app.screen().shows("100%"), "{:?}", app.screen().texts());

    // The picked file is copied, so it may go.
    app.on_pick_paths(vec![picture.clone()]).unwrap();
    app.press(choose).unwrap();
    app.wait_for_within("text:hills.png", SLOW).unwrap();
    kept(&mut app, &file, "background = \"hills.png\"");
    std::fs::remove_file(&picture).unwrap();
    assert_eq!(
        std::fs::read_to_string(copies.join("hills.png")).unwrap(),
        "a picture"
    );

    // What is no picture is refused, and the one that shows stays.
    app.on_pick_paths(vec![dir.join("notes.txt")]).unwrap();
    app.press(choose).unwrap();
    app.wait_until("why notes.txt is refused", SLOW, |screen| {
        screen.texts().iter().any(|t| t.contains("not a picture"))
    })
    .unwrap();
    assert!(copies.join("hills.png").is_file());
    assert!(!copies.join("notes.txt").exists());

    // A page may have no panel. Its opacity then waits.
    app.press("id:background-show-panel >> role:switch")
        .unwrap();
    kept(&mut app, &file, "show_panel = false");
    app.press("id:background-show-panel >> role:switch")
        .unwrap();
    kept(&mut app, &file, "show_panel = true");

    app.press(remove).unwrap();
    app.wait_for_gone("text:hills.png", SLOW).unwrap();
    // The file's own comment says what `background` is, so look for the setting.
    app.wait_until("the copy is gone and nothing names it", SLOW, |_| {
        !copies.join("hills.png").exists()
            && std::fs::read_to_string(&file).is_ok_and(|text| !text.contains("background ="))
    })
    .unwrap();
    drop(app);

    // The next start reads what was kept. A picture whose copy is gone is said to be gone.
    std::fs::write(&file, "background = \"gone.png\"\npanel = 80\n").unwrap();
    let mut app = start();
    open_settings(&mut app, "preferences");
    app.wait_for_within("text:gone.png is gone", SLOW).unwrap();
    assert!(app.screen().shows("80%"), "{:?}", app.screen().texts());

    let _ = std::fs::remove_dir_all(&dir);
}
