//! How the window looks: the mode (the computer's appearance, light or dark), the theme each
//! of the two appearances shows, and the language of its text.
//!
//! A theme is a Native Shell theme file: shadcn/ui's `cssVars`, as tweakcn and shadcn's themes
//! export them, with the app's own tokens (`ok`, `warn`, `highlight`) beside shadcn's. The themes
//! obmem ships are the files in `themes/`, built into the binary. The user's own are the `.json`
//! files in the `themes` folder next to the global config, and what they chose is
//! `appearance.toml` beside it. Everything that touches those files blocks.
//!
//! The user may also put a picture behind the sidebar and around the page's panel, and let the
//! panel show some of what is behind it. obmem keeps its own copy of the picture, in the
//! `backgrounds` folder beside the themes folder, so the picture stays when its file is moved.

use nsh_core::prelude::{AdaptiveColor, Tokens};
use obmem_core::config::global_config_path;
use obmem_core::store::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

/// The theme a new install shows, and the one that stands in for a chosen theme that is gone.
pub(crate) const DEFAULT: &str = "ink";

/// The themes obmem ships, in the order they are listed. A test checks that every file in
/// `themes/` is here.
const BUILT_IN: &[(&str, &str)] = &[
    ("ink", include_str!("themes/ink.json")),
    ("blueprint", include_str!("themes/blueprint.json")),
    ("graphite", include_str!("themes/graphite.json")),
    ("sepia", include_str!("themes/sepia.json")),
    ("iris", include_str!("themes/iris.json")),
    ("lagoon", include_str!("themes/lagoon.json")),
    ("asuka", include_str!("themes/asuka.json")),
    ("rei", include_str!("themes/rei.json")),
    ("unit-01", include_str!("themes/unit-01.json")),
    ("mari", include_str!("themes/mari.json")),
    ("cyberpunk", include_str!("themes/cyberpunk.json")),
    ("lucy", include_str!("themes/lucy.json")),
    ("motoko", include_str!("themes/motoko.json")),
];

/// The default theme's tokens. A theme that leaves out the app's own tokens takes them from here.
static BASE: LazyLock<Arc<Tokens>> = LazyLock::new(|| {
    let theme = nsh_base::Theme::from_json(BUILT_IN[0].1).expect("the default theme parses");
    Arc::new(theme.into_tokens())
});

/// Which appearance the window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    /// The computer's.
    #[default]
    System,
    Light,
    Dark,
}

impl From<Mode> for nsh_core::platform::Appearance {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::System => Self::System,
            Mode::Light => Self::Light,
            Mode::Dark => Self::Dark,
        }
    }
}

/// One of the two appearances a theme has colors for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        }
    }
}

/// The language the window's text shows in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum Language {
    /// The first of the computer's languages obmem has: Chinese or English, else English.
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh")]
    Chinese,
}

impl Language {
    /// What Native Shell's `set_locale` takes: `None` follows the computer.
    ///
    /// obmem's Chinese is Traditional Chinese under the tag `zh` (`locales/app.yml`), so that
    /// every Chinese the computer may name finds it: macOS says `zh-Hant-TW`, Windows `zh-TW`,
    /// and Native Shell looks a tag up by cutting its last part off. Picked here it is `zh-Hant`,
    /// which finds `zh` the same way and has dates and numbers written as Traditional Chinese
    /// writes them.
    pub(crate) fn locale(self) -> Option<nsh_core::platform::Locale> {
        match self {
            Language::System => None,
            Language::English => nsh_core::platform::Locale::parse("en"),
            Language::Chinese => nsh_core::platform::Locale::parse("zh-Hant"),
        }
    }
}

/// The kinds of picture that can stand behind the window's content, by extension. macOS and
/// Windows draw PNG and JPEG themselves. AVIF is drawn by macOS 13 and later, and by Windows
/// where its AV1 extension is installed; where it is not drawn, the window shows no picture.
pub(crate) const PICTURES: &[&str] = &["png", "jpg", "jpeg", "avif"];

/// The least a page's panel covers of what is behind it, in percent: under this the page's own
/// text is hard to read over a picture.
pub(crate) const PANEL_MIN: u8 = 50;

/// What the user chose (`appearance.toml`): the mode, the id of the theme each appearance shows,
/// the language, and what is behind the window's content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Chosen {
    pub mode: Mode,
    pub light: String,
    pub dark: String,
    pub language: Language,
    /// The picture behind the sidebar and around the page's panel: the name of a file in the
    /// backgrounds folder. Both appearances show the same one.
    pub background: Option<String>,
    /// How much a page's panel covers of what is behind it, in percent ([`Chosen::panel`]).
    pub panel: u8,
    /// Whether a page stands on a panel at all. Without one its cards stand right on what is
    /// behind the window's content.
    pub show_panel: bool,
}

impl Default for Chosen {
    fn default() -> Self {
        Chosen {
            mode: Mode::System,
            light: DEFAULT.to_owned(),
            dark: DEFAULT.to_owned(),
            language: Language::System,
            background: None,
            panel: 100,
            show_panel: true,
        }
    }
}

impl Chosen {
    pub(crate) fn theme(&self, scheme: Scheme) -> &str {
        match scheme {
            Scheme::Light => &self.light,
            Scheme::Dark => &self.dark,
        }
    }

    pub(crate) fn set_theme(&mut self, scheme: Scheme, id: String) {
        match scheme {
            Scheme::Light => self.light = id,
            Scheme::Dark => self.dark = id,
        }
    }

    /// How much a page's panel covers of what is behind it, in percent: what the file says, held
    /// between [`PANEL_MIN`] and 100.
    pub(crate) fn panel(&self) -> u8 {
        self.panel.clamp(PANEL_MIN, 100)
    }
}

/// A theme file, read.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThemeFile {
    /// What `appearance.toml` names it by: the file's name without `.json`.
    pub id: String,
    /// Its `name`, or its id when it has none.
    pub name: String,
    /// Where it is; `None` for a built-in one.
    pub file: Option<PathBuf>,
    /// Its tokens, or why it cannot be used.
    pub tokens: Result<Arc<Tokens>, String>,
    /// What of it Native Shell could not read and left out.
    pub ignored: Vec<String>,
}

impl ThemeFile {
    fn read(id: &str, file: Option<PathBuf>, json: &str) -> Self {
        let name = serde_json::from_str::<serde_json::Value>(json)
            .ok()
            .and_then(|v| v.get("name")?.as_str().map(str::to_owned))
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| id.to_owned());
        let (tokens, ignored) = match nsh_base::Theme::from_json_lenient(json) {
            Ok((theme, ignored)) => (Ok(Arc::new(theme.into_tokens().over(&BASE))), ignored),
            Err(e) => (Err(format!("It is not JSON: {e}")), Vec::new()),
        };
        ThemeFile {
            id: id.to_owned(),
            name,
            file,
            tokens,
            ignored,
        }
    }

    /// The color of token `name` in `scheme`, to preview the theme with.
    pub(crate) fn color(&self, name: &str, scheme: Scheme) -> Option<AdaptiveColor> {
        let color = self.tokens.as_ref().ok()?.color(name)?;
        Some(AdaptiveColor::from(match scheme {
            Scheme::Light => color.light,
            Scheme::Dark => color.dark,
        }))
    }
}

/// The theme that shows where `id` is chosen: that one, or the default when it is gone or cannot
/// be used. `themes` are all of them ([`themes`]).
pub(crate) fn shown<'a>(themes: &'a [ThemeFile], id: &str) -> &'a ThemeFile {
    let usable = |id: &str| themes.iter().find(|t| t.id == id && t.tokens.is_ok());
    usable(id)
        .or_else(|| usable(DEFAULT))
        .expect("the default theme is built in")
}

/// The tokens the window is drawn with while it shows the dark appearance (`dark`) or the light
/// one: each color has the light theme's light value and the dark theme's dark value, so both
/// appearances are right whichever shows. A theme's corner radius and fonts have one value, so
/// they are those of the theme that shows.
pub(crate) fn tokens(themes: &[ThemeFile], chosen: &Chosen, dark: bool) -> Tokens {
    let of = |scheme| {
        let theme = shown(themes, chosen.theme(scheme));
        theme.tokens.clone().expect("a shown theme is usable")
    };
    let (light, dark_theme) = (of(Scheme::Light), of(Scheme::Dark));
    let mut out = Tokens::clone(if dark { &dark_theme } else { &light });
    for (name, color) in light.colors() {
        let dark_value = dark_theme.color(name).unwrap_or(color).dark;
        out.set_color(name, AdaptiveColor::new(color.light, dark_value));
    }
    for (name, color) in dark_theme.colors() {
        if light.color(name).is_none() {
            out.set_color(name, color);
        }
    }
    out
}

/// The themes obmem ships.
pub(crate) fn built_in() -> Vec<ThemeFile> {
    BUILT_IN
        .iter()
        .map(|(id, json)| ThemeFile::read(id, None, json))
        .collect()
}

/// The folder the user's themes are in, next to the global config.
pub(crate) fn themes_dir() -> PathBuf {
    global_config_path().with_file_name("themes")
}

/// The folder obmem keeps the picture behind the window's content in, next to the global config.
pub(crate) fn backgrounds_dir() -> PathBuf {
    global_config_path().with_file_name("backgrounds")
}

/// The chosen picture's file, while it is there.
pub(crate) fn background(chosen: &Chosen) -> Option<PathBuf> {
    background_in(&backgrounds_dir(), chosen)
}

fn background_in(dir: &Path, chosen: &Chosen) -> Option<PathBuf> {
    let file = dir.join(plain_name(chosen.background.as_deref()?)?);
    file.is_file().then_some(file)
}

/// `name` when it names a file in a folder and nothing outside it. The name comes from a file
/// the user may have edited, and the file it names is one obmem deletes.
fn plain_name(name: &str) -> Option<&str> {
    (Path::new(name).file_name()?.to_str()? == name).then_some(name)
}

/// Copies `picture` into the backgrounds folder and returns the copy's name, which
/// `appearance.toml` names it by. The picture kept `before` goes: there is one at a time.
pub(crate) fn keep_background(picture: &Path, before: Option<&str>) -> Result<String, String> {
    keep_background_in(&backgrounds_dir(), picture, before)
}

fn keep_background_in(dir: &Path, picture: &Path, before: Option<&str>) -> Result<String, String> {
    let name = picture
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{} has no name obmem can keep.", picture.display()))?;
    let extension = picture.extension().and_then(|e| e.to_str());
    if !extension.is_some_and(|e| PICTURES.iter().any(|p| p.eq_ignore_ascii_case(e))) {
        return Err(format!(
            "{name} is not a picture obmem can show. It shows {}.",
            PICTURES.join(", ")
        ));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let copy = dir.join(name);
    // A picture picked from the folder itself is the copy already. Copying a file onto itself
    // would empty it.
    let same = matches!((picture.canonicalize(), copy.canonicalize()), (Ok(a), Ok(b)) if a == b);
    if !same {
        std::fs::copy(picture, &copy).map_err(|e| format!("{}: {e}", picture.display()))?;
    }
    if let Some(before) = before.filter(|before| *before != name) {
        drop_background_in(dir, before)?;
    }
    Ok(name.to_owned())
}

/// Takes obmem's copy of the picture `name` out of the backgrounds folder.
pub(crate) fn drop_background(name: &str) -> Result<(), String> {
    drop_background_in(&backgrounds_dir(), name)
}

fn drop_background_in(dir: &Path, name: &str) -> Result<(), String> {
    let Some(name) = plain_name(name) else {
        return Ok(());
    };
    match std::fs::remove_file(dir.join(name)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{name}: {e}")),
        _ => Ok(()),
    }
}

fn chosen_path() -> PathBuf {
    global_config_path().with_file_name("appearance.toml")
}

/// What the user chose. A missing or unreadable `appearance.toml` reads as the defaults.
pub(crate) fn chosen() -> Chosen {
    read_chosen(&chosen_path())
}

/// Every theme: the built-in ones, then the `.json` files of the user's themes folder by name.
/// One that cannot be used is listed with the reason.
pub(crate) fn themes() -> Vec<ThemeFile> {
    themes_in(&themes_dir())
}

fn read_chosen(path: &Path) -> Chosen {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Chosen::default();
    };
    toml::from_str(&text).unwrap_or_else(|e| {
        log::warn!("{}: {e}; showing the default appearance", path.display());
        Chosen::default()
    })
}

fn themes_in(dir: &Path) -> Vec<ThemeFile> {
    let mut themes = built_in();
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json") && p.is_file())
        .collect();
    files.sort();
    for file in files {
        let Some(id) = file.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let mut theme = match std::fs::read_to_string(&file) {
            Ok(json) => ThemeFile::read(&id, Some(file), &json),
            Err(e) => ThemeFile {
                name: id.clone(),
                id,
                file: Some(file),
                tokens: Err(format!("It could not be read: {e}")),
                ignored: Vec::new(),
            },
        };
        if BUILT_IN.iter().any(|(built_in, _)| *built_in == theme.id) {
            theme.tokens = Err(format!(
                "A built-in theme is already called {}. Rename the file.",
                theme.id
            ));
        }
        themes.push(theme);
    }
    themes
}

/// Keeps what the user chose.
pub(crate) fn save(chosen: &Chosen) -> Result<(), String> {
    save_at(&chosen_path(), chosen)
}

fn save_at(path: &Path, chosen: &Chosen) -> Result<(), String> {
    let body = toml::to_string(chosen).map_err(|e| e.to_string())?;
    let text = format!(
        "# How obmem's desktop app looks. `light` and `dark` name a theme: a built-in one, or a\n# file in the themes folder beside this file, without `.json`. `language` is `system`, `en`\n# or `zh`. `background` names a picture in the backgrounds folder beside this file, and\n# `panel` is how much a page's panel covers of what is behind it, in percent; `show_panel`\n# is whether a page has a panel at all. Read when the app starts.\n{body}"
    );
    let dir = path.parent().ok_or("the config has no folder")?;
    atomic_write(dir, path, &text).map_err(|e| format!("{e:#}"))
}

/// Saves a copy of theme `id` in the user's themes folder, under the first free `<id>-copy`
/// name, and returns the copy's id. The copy is a whole theme file, so every color is there to
/// change.
pub(crate) fn duplicate(id: &str) -> Result<String, String> {
    duplicate_in(&themes_dir(), id)
}

fn duplicate_in(dir: &Path, id: &str) -> Result<String, String> {
    let source = themes_in(dir)
        .into_iter()
        .find(|t| t.id == id && t.tokens.is_ok())
        .ok_or_else(|| format!("There is no theme {id}."))?;
    let json = match &source.file {
        Some(file) => std::fs::read_to_string(file).map_err(|e| e.to_string())?,
        None => BUILT_IN
            .iter()
            .find(|(built_in, _)| *built_in == id)
            .map(|(_, json)| (*json).to_owned())
            .ok_or_else(|| format!("There is no theme {id}."))?,
    };
    let mut doc: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    let Some(fields) = doc.as_object_mut() else {
        return Err(format!("{id} is not a theme file."));
    };
    fields.insert("name".into(), format!("{} copy", source.name).into());
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n";

    let mut copy = format!("{id}-copy");
    let mut n = 2;
    while dir.join(format!("{copy}.json")).exists() {
        copy = format!("{id}-copy-{n}");
        n += 1;
    }
    atomic_write(dir, &dir.join(format!("{copy}.json")), &text).map_err(|e| format!("{e:#}"))?;
    Ok(copy)
}

/// The user's themes folder, made if it is not there yet, to show it in the file manager.
pub(crate) fn ensure_themes_dir() -> Result<PathBuf, String> {
    let dir = themes_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every color token the app's views read, by its name in a theme file.
    const READ: &[&str] = &[
        "background",
        "foreground",
        "card",
        "primary",
        "primary-foreground",
        "muted",
        "muted-foreground",
        "accent",
        "destructive",
        "border",
        "input",
        "ring",
        "sidebar",
        "sidebar-accent",
        "sidebar-border",
        "ok",
        "warn",
        "highlight",
        "highlight-foreground",
    ];

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("obmem-gui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_file_in_themes_is_built_in_and_sets_what_the_app_reads() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/themes");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .map(|p| p.file_stem().unwrap().to_str().unwrap().to_owned())
            .collect();
        on_disk.sort();
        let mut listed: Vec<String> = BUILT_IN.iter().map(|(id, _)| (*id).to_owned()).collect();
        listed.sort();
        assert_eq!(on_disk, listed);

        for (id, json) in BUILT_IN {
            let theme = ThemeFile::read(id, None, json);
            assert_eq!(theme.ignored, Vec::<String>::new(), "{id}");
            assert_ne!(theme.name, *id, "{id} has a name");
            let doc: serde_json::Value = serde_json::from_str(json).unwrap();
            for scheme in ["light", "dark"] {
                for token in READ {
                    assert!(
                        doc["cssVars"][scheme][token].is_string(),
                        "{id} sets {scheme} {token}"
                    );
                }
            }
        }
    }

    #[test]
    fn each_appearance_shows_its_own_theme() {
        let themes = built_in();
        let chosen = Chosen {
            light: "sepia".into(),
            dark: "lagoon".into(),
            ..Chosen::default()
        };
        let of = |id: &str| shown(&themes, id).tokens.clone().unwrap();
        let (sepia, lagoon) = (of("sepia"), of("lagoon"));
        for dark in [false, true] {
            let tokens = tokens(&themes, &chosen, dark);
            for name in ["background", "primary", "sidebar", "ok"] {
                let color = tokens.color(name).unwrap();
                assert_eq!(color.light, sepia.color(name).unwrap().light, "{name}");
                assert_eq!(color.dark, lagoon.color(name).unwrap().dark, "{name}");
            }
        }
    }

    #[test]
    fn a_theme_keeps_its_own_radius_while_it_shows() {
        let dir = temp("radius");
        std::fs::write(
            dir.join("round.json"),
            r##"{ "cssVars": { "theme": { "radius": "1rem" }, "dark": { "primary": "#ff0000" } } }"##,
        )
        .unwrap();
        let themes = themes_in(&dir);
        let chosen = Chosen {
            dark: "round".into(),
            ..Chosen::default()
        };
        assert_eq!(tokens(&themes, &chosen, true).number("radius"), Some(16.0));
        assert_eq!(tokens(&themes, &chosen, false).number("radius"), Some(6.0));
        // It leaves out the app's own tokens, so they are the default theme's.
        assert_eq!(tokens(&themes, &chosen, true).color("ok"), BASE.color("ok"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_theme_that_is_gone_or_broken_shows_the_default() {
        let dir = temp("broken");
        std::fs::write(dir.join("broken.json"), "not json").unwrap();
        std::fs::write(dir.join("ink.json"), "{}").unwrap();
        let themes = themes_in(&dir);
        let listed = |id: &str| {
            let file = themes.iter().find(|t| t.id == id && t.file.is_some());
            file.unwrap().tokens.clone()
        };
        assert!(listed("broken").unwrap_err().starts_with("It is not JSON"));
        assert!(listed("ink").unwrap_err().contains("built-in theme"));

        for chosen in ["broken", "gone"] {
            assert_eq!(shown(&themes, chosen).id, DEFAULT);
        }
        assert_eq!(shown(&themes, "sepia").id, "sepia");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_is_chosen_is_kept_and_a_bad_file_reads_as_the_defaults() {
        let dir = temp("chosen");
        let path = dir.join("appearance.toml");
        assert_eq!(read_chosen(&path), Chosen::default());

        let chosen = Chosen {
            mode: Mode::Dark,
            light: "sepia".into(),
            dark: "unit-01".into(),
            language: Language::Chinese,
            background: Some("hills.jpg".into()),
            panel: 80,
            show_panel: false,
        };
        save_at(&path, &chosen).unwrap();
        assert_eq!(read_chosen(&path), chosen);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("language = \"zh\"")
        );

        // A file that sets only some of it keeps the defaults for the rest.
        std::fs::write(&path, "mode = \"light\"\n").unwrap();
        assert_eq!(
            read_chosen(&path),
            Chosen {
                mode: Mode::Light,
                ..Chosen::default()
            }
        );
        std::fs::write(&path, "mode = \"sideways\"\n").unwrap();
        assert_eq!(read_chosen(&path), Chosen::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_picture_is_kept_as_a_copy_and_the_one_before_it_goes() {
        let dir = temp("picture");
        let (from, kept) = (dir.join("from"), dir.join("backgrounds"));
        std::fs::create_dir_all(&from).unwrap();
        for name in ["hills.jpg", "Sea.PNG", "dunes.avif", "notes.txt"] {
            std::fs::write(from.join(name), name).unwrap();
        }
        let chosen = |name: &str| Chosen {
            background: Some(name.into()),
            ..Chosen::default()
        };
        assert_eq!(background_in(&kept, &Chosen::default()), None);
        assert_eq!(background_in(&kept, &chosen("hills.jpg")), None);

        let name = keep_background_in(&kept, &from.join("hills.jpg"), None).unwrap();
        assert_eq!(name, "hills.jpg");
        assert_eq!(
            background_in(&kept, &chosen(&name)),
            Some(kept.join("hills.jpg"))
        );
        // The file it was copied from may go.
        std::fs::remove_file(from.join("hills.jpg")).unwrap();
        assert!(background_in(&kept, &chosen(&name)).is_some());

        let next = keep_background_in(&kept, &from.join("Sea.PNG"), Some(&name)).unwrap();
        assert_eq!(next, "Sea.PNG");
        assert!(!kept.join("hills.jpg").exists());
        // Picked from the folder itself, it is left as it is.
        keep_background_in(&kept, &kept.join("Sea.PNG"), Some(&next)).unwrap();
        assert_eq!(
            std::fs::read_to_string(kept.join("Sea.PNG")).unwrap(),
            "Sea.PNG"
        );

        let refused = keep_background_in(&kept, &from.join("notes.txt"), Some(&next));
        assert!(refused.unwrap_err().contains("not a picture"));
        assert!(kept.join("Sea.PNG").exists());
        let next = keep_background_in(&kept, &from.join("dunes.avif"), Some(&next)).unwrap();
        assert_eq!(next, "dunes.avif");

        drop_background_in(&kept, &next).unwrap();
        drop_background_in(&kept, &next).unwrap();
        assert_eq!(background_in(&kept, &chosen(&next)), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_that_leaves_the_folder_names_no_picture() {
        let dir = temp("outside");
        let kept = dir.join("backgrounds");
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::write(dir.join("outside.png"), "").unwrap();
        let chosen = Chosen {
            background: Some("../outside.png".into()),
            ..Chosen::default()
        };
        assert_eq!(background_in(&kept, &chosen), None);
        drop_background_in(&kept, "../outside.png").unwrap();
        assert!(dir.join("outside.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_panel_covers_at_least_half_of_what_is_behind_it() {
        let covers = |panel| {
            Chosen {
                panel,
                ..Chosen::default()
            }
            .panel()
        };
        assert_eq!(Chosen::default().panel(), 100);
        assert_eq!((covers(0), covers(75), covers(255)), (PANEL_MIN, 75, 100));
    }

    #[test]
    fn a_copy_is_a_whole_theme_file_under_a_free_name() {
        let dir = temp("copy").join("themes");
        assert_eq!(duplicate_in(&dir, "sepia").unwrap(), "sepia-copy");
        assert_eq!(duplicate_in(&dir, "sepia").unwrap(), "sepia-copy-2");
        // A copy of a copy starts from the user's file.
        assert_eq!(duplicate_in(&dir, "sepia-copy").unwrap(), "sepia-copy-copy");
        assert!(duplicate_in(&dir, "nothing").is_err());

        let themes = themes_in(&dir);
        let copy = themes.iter().find(|t| t.id == "sepia-copy").unwrap();
        let sepia = themes.iter().find(|t| t.id == "sepia").unwrap();
        assert_eq!(copy.name, "Sepia copy");
        assert_eq!(copy.tokens, sepia.tokens);
        assert_eq!(copy.file, Some(dir.join("sepia-copy.json")));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
