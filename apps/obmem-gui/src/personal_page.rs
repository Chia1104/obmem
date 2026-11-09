//! The user's own settings (`~/.config/obmem/config.toml`) and their personal vault's allowlist,
//! which apply to every project.

use crate::data::{Personal, Vault};
use crate::state::{self, Load};
use crate::widgets::{Level, action, card, check, field, heading, muted, notice_area, page, page_header, primary, switch_control, text_box};
use nsh_core::prelude::*;
use obmem_core::allowlist::{Allowlist, Kind};
use obmem_core::settings::GlobalSettings;

pub(crate) fn personal_page() -> impl IntoElement {
    page(
        page_header(
            "Personal settings",
            muted("Yours alone, for every project: ~/.config/obmem/config.toml and your personal vault."),
            vec![],
        ),
        view().gap_4().child(notice_area()).child(view().gap_4().child(dynamic(move || match state::personal() {
            Load::Loading => vec![muted("Loading…").into_element()],
            Load::Failed(e) => vec![check(Level::Error, "Could not read your settings", e).into_element()],
            Load::Ready(p) => vec![form(&p).into_element(), allowlist(&p).into_element()],
        }))),
    )
}

fn form(p: &Personal) -> View {
    let vault = signal(p.settings.vault.clone());
    let scan_env = signal(p.settings.scan_process_env);
    let save = move || state::save_personal(GlobalSettings { vault: vault.get_untracked(), scan_process_env: scan_env.get_untracked() });
    card()
        .test_id("personal-settings")
        .child(heading("Settings"))
        .child(field(
            "Personal vault",
            &format!("Your feedback, preferences and session logs. Now: {}. Empty: ~/Obsidian/AgentMemory.", p.vault.display()),
            text_box(vault, "~/Obsidian/AgentMemory"),
        ))
        .child(field(
            "Scan environment variables",
            "Treat the values of sensitive-looking environment variables as known secrets. Only you can turn this off; a project's settings cannot.",
            switch_control(move || scan_env.get(), "Scan environment variables", move |v| scan_env.set(v)),
        ))
        .child(view().flex_row().pt_2().child(primary("Save", save)))
}

fn allowlist(p: &Personal) -> View {
    let mut c = card()
        .test_id("personal-allowlist")
        .child(heading("Allowed everywhere"))
        .child(muted("False positives the scanner lets through in every project. Only this list may hold patterns and words."));
    let a = match &p.allowlist {
        Ok(a) => a.clone(),
        Err(e) => return c.child(check(Level::Error, "Could not read your allowlist", e.clone())),
    };
    let Allowlist { fingerprints, regexes, stopwords } = a;
    c = c.child(entries("Fingerprints", "Exact values, added from a scan.", Kind::Fingerprint, fingerprints, false));
    c = c.child(entries("Patterns", "Regexes matched against a suspected secret.", Kind::Regex, regexes, true));
    c.child(entries("Words", "A value containing one of these (4+ letters) is not a secret.", Kind::Stopword, stopwords, true))
}

fn entries(title: &str, help: &str, kind: Kind, list: Vec<String>, addable: bool) -> View {
    let mut block = view().test_id(&format!("entries-{}", title.to_lowercase())).gap_1().pt_3().child(text(title.to_string()).font_weight(FontWeight::MEDIUM)).child(muted(help.to_string()));
    if list.is_empty() {
        block = block.child(muted("None."));
    }
    for e in list {
        let entry = e.clone();
        block = block.child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .py_1()
                .child(text(e).flex_1().text_sm())
                .child(action("Remove", move || state::disallow(Vault::Personal, kind, entry.clone()))),
        );
    }
    if addable {
        let draft = signal(String::new());
        block = block.child(
            view()
                .flex_row()
                .gap_2()
                .pt_1()
                .child(text_box(draft, &format!("Add to {}", title.to_lowercase())).test_id(&format!("add-{}", title.to_lowercase())).flex_1())
                .child(action("Add", move || state::allow(Vault::Personal, kind, draft.get_untracked()))),
        );
    }
    block
}
