//! One project: its overview (health and each agent's setup), settings and secrets.

use crate::data::ClaudeMemory;
use crate::data::{AgentState, Detail};
use crate::state::{self, Load, Tab};
use crate::widgets::{
    Level, action, card, check, checkbox_control, heading, muted, notice_area, page, page_header,
    path_text, primary, tabs,
};
use crate::{agent_tab, secrets_tab, sessions_tab, settings_tab};
use nsh_base::theme;
use nsh_core::prelude::*;
use obmem_core::status::{Registration, VaultStatus};

const REVEAL: &str = if cfg!(windows) {
    "Show in Explorer"
} else {
    "Show in Finder"
};

pub(crate) fn project_page() -> impl IntoElement {
    page(
        dynamic(header),
        view()
            .gap_4()
            .child(dynamic(move || {
                let ready = matches!(state::detail(), Load::Ready(ref d) if d.status.initialized);
                ready.then(tab_bar)
            }))
            .child(notice_area())
            .child(view().gap_4().child(dynamic(content))),
    )
}

/// The project's name and actions; also what shows before it loads or when it cannot.
fn header() -> Vec<AnyElement> {
    match state::detail() {
        Load::Loading if state::selected().is_none() => {
            vec![muted("Add a project to see its memory setup.").into_element()]
        }
        Load::Loading => vec![muted("Loading…").into_element()],
        Load::Failed(e) => vec![
            check(Level::Error, "Could not read this project", e).into_element(),
            view()
                .flex_row()
                .gap_2()
                .child(action("Try again", state::refresh))
                .child(action("Remove from list", state::remove_selected))
                .into_element(),
        ],
        Load::Ready(d) => {
            let root = d.status.project_root.clone();
            let shown = root.display().to_string();
            vec![
                page_header(
                    d.status.project_name.clone(),
                    path_text(shown),
                    vec![
                        action("Refresh", state::refresh).into_element(),
                        action(REVEAL, move || drop(reveal_path(root.clone()))).into_element(),
                        action("Remove from list", state::remove_selected).into_element(),
                    ],
                )
                .into_element(),
            ]
        }
    }
}

fn tab_bar() -> AnyElement {
    tabs(
        &[
            (Tab::Overview, "Overview"),
            (Tab::Agent, "Agent view"),
            (Tab::Sessions, "Sessions"),
            (Tab::Settings, "Settings"),
            (Tab::Secrets, "Secrets"),
        ],
        state::tab,
        state::show_tab,
    )
    .into_element()
}

/// The shown tab. It re-renders when the project is read again (after a save) or the tab changes,
/// never for a notice, so a form keeps what the user typed when a save fails.
fn content() -> Vec<AnyElement> {
    let Load::Ready(d) = state::detail() else {
        return vec![];
    };
    if !d.status.initialized {
        return vec![
            card()
                .child(text("obmem is not set up in this folder"))
                .child(muted("Setting it up creates the project vault and .obmem.toml. Until then, hooks do nothing here."))
                .child(view().flex_row().pt_2().child(primary("Set up obmem here", state::initialize)))
                .into_element(),
        ];
    }
    match state::tab() {
        Tab::Overview => {
            let mut out = vec![health(&d).into_element(), agents(d.agents).into_element()];
            out.extend(claude_memory(&d.claude_memory).map(IntoElement::into_element));
            out
        }
        Tab::Agent => vec![agent_tab::agent_tab().into_element()],
        Tab::Sessions => vec![sessions_tab::sessions_tab()],
        Tab::Settings => vec![settings_tab::settings_tab(&d).into_element()],
        Tab::Secrets => vec![secrets_tab::secrets_tab(&d).into_element()],
    }
}

fn health(d: &Detail) -> View {
    let app = env!("CARGO_PKG_VERSION");
    let cli = match &d.cli {
        Some(v) if v == app => check(
            Level::Ok,
            "obmem command",
            format!("obmem {v} is on PATH, where hooks and MCP configs run it."),
        ),
        Some(v) => check(
            Level::Warn,
            "obmem command",
            format!(
                "obmem {v} is on PATH, but this app is {app}. Install the matching version so agents run the same obmem."
            ),
        ),
        None => check(
            Level::Error,
            "obmem command",
            "obmem is not on PATH. Every hook and MCP config runs it from there: install it first.",
        ),
    };
    let st = &d.status;
    card()
        .child(heading("Health"))
        .child(cli)
        .child(vault("Project vault", &st.project_vault))
        .child(vault("Personal vault", &st.global_vault))
        .child(
            check(
                if st.open_findings > 0 {
                    Level::Warn
                } else {
                    Level::Ok
                },
                "Review",
                format!(
                    "{} drafts and {} open findings in the project vault.",
                    st.drafts, st.open_findings
                ),
            )
            .child(action("Review", state::show_review)),
        )
}

fn vault(title: &str, v: &VaultStatus) -> View {
    let path = v.path.display().to_string();
    let row = match (&v.obsidian, v.exists) {
        (_, false) => check(
            Level::Error,
            title.to_string(),
            format!("{path} does not exist."),
        ),
        (Registration::Registered { .. }, true) => check(
            Level::Ok,
            title.to_string(),
            format!("{path} is open in Obsidian."),
        ),
        (Registration::NotRegistered, true) => check(
            Level::Warn,
            title.to_string(),
            format!(
                "{path} is not open in Obsidian, so search and link checks skip it. In Obsidian, choose “Open folder as vault” and pick this folder."
            ),
        ),
        (Registration::Unknown(e), true) => check(
            Level::Warn,
            title.to_string(),
            format!("Could not read Obsidian's vault list: {e}"),
        ),
    };
    let folder = v.path.clone();
    match (&v.obsidian, v.exists) {
        (Registration::NotRegistered, true) => {
            row.child(action(REVEAL, move || drop(reveal_path(folder.clone()))))
        }
        _ => row,
    }
}

fn agents(list: Vec<AgentState>) -> View {
    let mut c = card().child(heading("Agents"));
    for a in list {
        let agent = a.agent;
        let mut row = view()
            .test_id(&format!("agent-{}", agent.id()))
            .flex_row()
            .items_center()
            .gap_3()
            .py_2()
            .flex_wrap()
            .child(
                text(agent.label())
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .w(px(170.0)),
            );
        row = match a.missing {
            Ok(files) if files.is_empty() => row.child(
                view().flex_1().child(
                    text("Set up")
                        .text_sm()
                        .text_color(crate::widgets::tone("ok")),
                ),
            ),
            Ok(files) => row
                .child(view().flex_1().min_w(px(160.0)).child(muted(format!(
                    "Not set up: {} file(s) to update",
                    files.len()
                ))))
                .child(primary("Set up", move || state::set_up(agent))),
            Err(e) => row.child(
                view()
                    .flex_1()
                    .child(text(e).text_sm().text_color(move || theme().destructive)),
            ),
        };
        c = c.child(row);
    }
    c
}

/// Claude Code's own memory for this project, offered for import while some of it is not in the
/// vaults yet. Imported memories arrive as drafts, for review like any agent's.
fn claude_memory(m: &Result<ClaudeMemory, String>) -> Option<View> {
    let m = match m {
        Err(e) => {
            return Some(card().child(check(
                Level::Error,
                "Could not read Claude Code's memory",
                e.clone(),
            )));
        }
        Ok(m) if m.items.iter().all(|i| i.exists) => return None,
        Ok(m) => m,
    };
    let fresh: Vec<std::path::PathBuf> = m
        .items
        .iter()
        .filter(|i| !i.exists)
        .map(|i| i.source.clone())
        .collect();
    let chosen = signal(fresh.clone());
    let mut c = card().test_id("claude-memory").child(heading("Claude Code memory")).child(muted(format!(
        "{} of the {} memories in {} are not in the vaults yet. They come in as drafts for you to approve.",
        fresh.len(),
        m.items.len(),
        m.dir.display()
    )));
    for item in &m.items {
        let stem = item
            .source
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let row = view()
            .test_id(&format!("memory-{stem}"))
            .flex_row()
            .flex_wrap()
            .items_start()
            .gap_x_3()
            .gap_y_1()
            .py_1();
        let row = if item.exists {
            row.child(view().w(px(16.0)))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(200.0))
                        .child(text(item.title.clone()).text_sm()),
                )
                .child(muted("already imported"))
        } else {
            let source = item.source.clone();
            let on = {
                let source = source.clone();
                move || chosen.with(|c| c.contains(&source))
            };
            let toggle = move |pick: bool| {
                let source = source.clone();
                chosen.update(|c| {
                    c.retain(|s| s != &source);
                    if pick {
                        c.push(source);
                    }
                })
            };
            row.child(view().flex_1().min_w(px(200.0)).child(checkbox_control(
                on,
                toggle,
                text(item.title.clone()).text_sm(),
            )))
            .child(muted(item.destination.clone()))
        };
        c = c.child(row);
    }
    Some(
        c.child(
            view()
                .flex_row()
                .pt_2()
                .child(primary("Import selected", move || {
                    state::import_claude(chosen.get_untracked())
                })),
        ),
    )
}
