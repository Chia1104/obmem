//! The user's own settings (`~/.config/obmem/config.toml`) and their personal vault's allowlist,
//! which apply to every project.

use crate::data::{Personal, SyncInfo, Vault};
use crate::state::{self, Load};
use crate::widgets::{Level, action, card, check, field, heading, muted, notice_area, page, page_header, primary, switch_control, text_box};
use nsh_core::prelude::*;
use obmem_core::allowlist::{Allowlist, Kind};
use obmem_core::settings::{GlobalSettings, SyncSettings};

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
            Load::Ready(p) => vec![form(&p).into_element(), sync_card(&p).into_element(), allowlist(&p).into_element()],
        }))),
    )
}

fn form(p: &Personal) -> View {
    let vault = signal(p.settings.vault.clone());
    let scan_env = signal(p.settings.scan_process_env);
    let auto_approve = signal(p.settings.auto_approve);
    let save = move || {
        state::save_personal(GlobalSettings {
            vault: vault.get_untracked(),
            scan_process_env: scan_env.get_untracked(),
            auto_approve: auto_approve.get_untracked(),
        })
    };
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
        .child(field(
            "Approve agent notes automatically",
            "Agents' notes are saved active instead of waiting in Review as drafts. Only you can turn this on; a project's .obmem.toml can turn it off with [review] auto_approve = false.",
            switch_control(move || auto_approve.get(), "Approve agent notes automatically", move |v| auto_approve.set(v)),
        ))
        .child(view().flex_row().pt_2().child(primary("Save", save)))
}

fn sync_card(p: &Personal) -> View {
    let c = card()
        .test_id("personal-sync")
        .child(heading("Sync"))
        .child(muted("Keep this personal vault the same on your Macs and PCs through an S3-compatible bucket you own: AWS S3, Cloudflare R2 or MinIO. Everything in the bucket is encrypted, and only your passphrase unlocks it."));
    let info = match &p.sync {
        Ok(i) => i.clone(),
        Err(e) => return c.child(check(Level::Error, "Could not read the sync settings", e.clone())),
    };
    let c = c.child(sync_status(&info)).child(sync_settings(&info.settings));
    if info.settings.bucket.is_empty() || info.connected == Ok(true) {
        return c;
    }
    c.child(connect_form())
}

fn sync_status(info: &SyncInfo) -> View {
    let row = view().gap_2().pt_2();
    if info.settings.bucket.is_empty() {
        return row.child(muted("Off: no bucket yet."));
    }
    match &info.connected {
        Err(e) => return row.child(check(Level::Error, "Could not read the keychain", e.clone())),
        Ok(false) => {
            return row.child(check(Level::Warn, "This device is not connected", "Enter the bucket's access key and your passphrase below."));
        }
        Ok(true) => {}
    }
    let line = match &info.last {
        None => check(Level::Warn, "Connected, not synced yet", "Sync now, or start an agent session in an obmem project."),
        Some(s) if s.ok => check(Level::Ok, "Last sync", format!("{} ({})", s.message, s.at)),
        Some(s) => check(Level::Error, "The last sync failed", format!("{} ({})", s.message, s.at)),
    };
    row.child(line).child(view().flex_row().child(primary("Sync now", state::sync_now)))
}

fn sync_settings(s: &SyncSettings) -> View {
    let endpoint = signal(s.endpoint.clone());
    let bucket = signal(s.bucket.clone());
    let region = signal(s.region.clone());
    let prefix = signal(s.prefix.clone());
    let device = signal(s.device.clone());
    let auto = signal(s.auto);
    let plugins = signal(s.community_plugins);
    let save = move || {
        state::save_sync(SyncSettings {
            endpoint: endpoint.get_untracked(),
            region: region.get_untracked(),
            bucket: bucket.get_untracked(),
            prefix: prefix.get_untracked(),
            device: device.get_untracked(),
            auto: auto.get_untracked(),
            community_plugins: plugins.get_untracked(),
        })
    };
    view()
        .gap_1()
        .child(field("Endpoint", "The S3 API URL, e.g. https://<account-id>.r2.cloudflarestorage.com. Empty: AWS S3.", text_box(endpoint, "https://<account-id>.r2.cloudflarestorage.com").test_id("sync-endpoint")))
        .child(field("Bucket", "Empty turns sync off.", text_box(bucket, "my-obmem-vault").test_id("sync-bucket")))
        .child(field("Region", "AWS needs the bucket's region. Empty: auto, which R2 expects.", text_box(region, "auto")))
        .child(field("Prefix", "Where in the bucket the vault goes. Empty: obmem.", text_box(prefix, "obmem")))
        .child(field("This device's name", "Shown in snapshots and conflict copies, and part of session log names. Connecting sets it to the host name plus four random hex digits, so two machines never share one.", text_box(device, "work-mac").test_id("sync-device")))
        .child(field(
            "Sync automatically",
            "Sync in the background when an agent session starts or a turn ends in an obmem project, at most once a minute.",
            switch_control(move || auto.get(), "Sync automatically", move |v| auto.set(v)),
        ))
        .child(field(
            "Community plugins",
            "Also sync .obsidian/plugins. Settings, themes and snippets always sync; the workspace layout never does.",
            switch_control(move || plugins.get(), "Sync community plugins", move |v| plugins.set(v)),
        ))
        .child(view().flex_row().pt_2().child(primary("Save", save)))
}

fn connect_form() -> View {
    let key_id = signal(String::new());
    let secret = signal(String::new());
    let passphrase = signal(String::new());
    let repeat = signal(String::new());
    let connect = move || {
        state::connect_sync(key_id.get_untracked(), secret.get_untracked(), passphrase.get_untracked(), repeat.get_untracked())
    };
    view()
        .test_id("sync-connect")
        .gap_1()
        .pt_3()
        .child(text("Connect this device").font_weight(FontWeight::MEDIUM))
        .child(muted("The access key stays in this device's keychain. On the first device the passphrase you choose starts the vault; every other device unlocks it with the same one. If you forget it, the bucket cannot be read."))
        .child(field("Access key ID", "", text_box(key_id, "Access key ID")))
        .child(field("Secret access key", "", text_box(secret, "Secret access key").masked(true)))
        .child(field("Passphrase", "", text_box(passphrase, "Passphrase").masked(true)))
        .child(field("Repeat passphrase", "", text_box(repeat, "Repeat passphrase").masked(true)))
        .child(view().flex_row().pt_2().child(primary("Connect", connect)))
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
