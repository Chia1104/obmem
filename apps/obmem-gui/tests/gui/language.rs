//! The language: the window follows the computer's, shows Chinese for any Chinese and English
//! for a language obmem does not have, and a language picked in the window shows at once and is
//! kept in `appearance.toml`.

use crate::common::{SLOW, project, sandbox, start, start_in};

const OPEN: &str = "id:screen-appearance >> role:button";

#[test]
fn the_window_follows_the_computers_language() {
    let Some(dir) = sandbox("language-system") else {
        return;
    };
    project(&dir, "demo-repo");

    // macOS names Traditional Chinese `zh-Hant-TW`, Windows `zh-TW`; Simplified Chinese gets
    // the Chinese obmem has too.
    for (n, tag) in ["zh-Hant-TW", "zh-TW", "zh-Hans-CN"]
        .into_iter()
        .enumerate()
    {
        let mut app = start_in(Some(tag));
        app.wait_for_within("text:demo-repo", SLOW).unwrap();
        app.wait_for_within("text:專案", SLOW).unwrap();
        // The first one sets the project up: a sentence and a button are in Chinese.
        if n == 0 {
            app.wait_for_within("text:這個資料夾還沒有設定 obmem", SLOW)
                .unwrap();
            app.press("role:button[在這裡設定 obmem]").unwrap();
        }
        // So are a tab and a sentence made from what was read.
        app.wait_for_within("role:tab[總覽]", SLOW).unwrap();
        app.wait_for_within("text:尚未設定：有 4 個檔案要更新", SLOW)
            .unwrap();
        let screen = app.screen();
        assert!(!screen.shows("Projects"), "{tag}: {:?}", screen.texts());
        // No text shows as the key it has no translation for (`zh.some.key`).
        let untranslated: Vec<_> = screen
            .texts()
            .into_iter()
            .filter(|t| t.starts_with("zh.") || t.starts_with("en."))
            .collect();
        assert!(untranslated.is_empty(), "{tag}: {untranslated:?}");
    }

    // A language obmem does not have shows English.
    let mut app = start_in(Some("fr-FR"));
    app.wait_for_within("text:Projects", SLOW).unwrap();
    app.wait_for_within("role:tab[Overview]", SLOW).unwrap();

    let _ = std::fs::remove_dir_all(&dir);
}

/// Opens the language list from its trigger and chooses `language` in it.
fn pick(app: &mut nsh_testing::Headless, language: &str) {
    app.press("id:appearance-language >> role:combobox")
        .unwrap();
    let option = format!("role:option[{language}]");
    app.wait_for_within(&option, SLOW).unwrap();
    app.press(&option).unwrap();
}

#[test]
fn a_picked_language_shows_at_once_and_is_kept() {
    let Some(dir) = sandbox("language-picked") else {
        return;
    };
    let file = dir.join("config/appearance.toml");
    let kept = |app: &mut nsh_testing::Headless, line: &str| {
        app.wait_until(line, SLOW, |_| {
            std::fs::read_to_string(&file).is_ok_and(|text| text.contains(line))
        })
        .unwrap();
    };

    let mut app = start();
    app.wait_for_within(OPEN, SLOW).unwrap();
    app.press(OPEN).unwrap();
    app.wait_for_within("text:Light theme", SLOW).unwrap();

    // The open window changes: the page, and the sidebar beside it.
    pick(&mut app, "繁體中文");
    app.wait_for_within("text:淺色佈景主題", SLOW).unwrap();
    app.wait_for_within("text:個人設定", SLOW).unwrap();
    assert!(!app.screen().shows("Light theme"));
    kept(&mut app, "language = \"zh\"");
    drop(app);

    // The next start is in the language picked, whatever the computer's.
    let mut app = start_in(Some("en-US"));
    app.wait_for_within("text:個人設定", SLOW).unwrap();
    app.press(OPEN).unwrap();
    app.wait_for_within("text:淺色佈景主題", SLOW).unwrap();

    // English is picked the same way, and System follows the computer again.
    pick(&mut app, "English");
    app.wait_for_within("text:Light theme", SLOW).unwrap();
    kept(&mut app, "language = \"en\"");
    drop(app);

    let mut app = start_in(Some("zh-Hant-TW"));
    app.wait_for_within("text:Personal settings", SLOW).unwrap();
    app.press(OPEN).unwrap();
    app.wait_for_within("text:Light theme", SLOW).unwrap();
    pick(&mut app, "System");
    app.wait_for_within("text:淺色佈景主題", SLOW).unwrap();
    kept(&mut app, "language = \"system\"");

    let _ = std::fs::remove_dir_all(&dir);
}
