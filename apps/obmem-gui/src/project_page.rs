//! One project: its overview (health and each agent's setup), settings and secrets.

use crate::data::ClaudeMemory;
use crate::data::{AgentState, Detail};
use crate::state::{self, Load, Tab, Work};
use crate::widgets::{
    Level, action, card, check, checkbox_control, heading, loading, muted, notice_area, page,
    page_header, path_text, primary, tabs,
};
use crate::words::tr;
use crate::{agent_tab, secrets_tab, sessions_tab, settings_tab};
use nsh_base::theme;
use nsh_core::prelude::*;
use obmem_core::status::{Registration, VaultStatus};

/// The translation key of "show this folder in the file manager", which each system names.
pub(crate) const REVEAL: &str = if cfg!(windows) {
    "common.show_in_explorer"
} else {
    "common.show_in_finder"
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
            vec![muted(tr!("project.none")).into_element()]
        }
        Load::Loading => vec![loading(tr!("common.loading")).into_element()],
        Load::Failed(e) => vec![
            check(Level::Error, tr!("project.unreadable"), e).into_element(),
            view()
                .flex_row()
                .gap_2()
                .child(
                    action(tr!("common.try_again"), state::refresh)
                        .busy(state::running(Work::Refresh)),
                )
                .child(remove_button())
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
                        action(tr!("common.refresh"), state::refresh)
                            .busy(state::running(Work::Refresh))
                            .into_element(),
                        action(tr!(REVEAL), move || drop(reveal_path(root.clone()))).into_element(),
                        remove_button().into_element(),
                    ],
                )
                .into_element(),
            ]
        }
    }
}

fn remove_button() -> impl IntoElement {
    action(tr!("project.remove"), state::remove_selected).busy(state::running(Work::RemoveProject))
}

fn tab_bar() -> AnyElement {
    tabs(
        &[
            (Tab::Overview, "project.tabs.overview"),
            (Tab::Agent, "project.tabs.agent"),
            (Tab::Sessions, "project.tabs.sessions"),
            (Tab::Settings, "project.tabs.settings"),
            (Tab::Secrets, "project.tabs.secrets"),
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
                .child(text(tr!("project.not_set_up")))
                .child(muted(tr!("project.not_set_up_about")))
                .child(
                    view().flex_row().pt_2().child(
                        primary(tr!("project.set_up_here"), state::initialize)
                            .busy(state::running(Work::Initialize)),
                    ),
                )
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
    let command = tr!("health.command");
    let cli = match d.cli.clone() {
        Some(v) if v == app => check(Level::Ok, command, tr!("health.command_ok", version = v)),
        Some(v) => check(
            Level::Warn,
            command,
            tr!("health.command_differs", version = v, app = app),
        ),
        None => check(Level::Error, command, tr!("health.command_missing")),
    };
    let st = &d.status;
    let (drafts, findings) = (st.drafts, st.open_findings);
    card()
        .child(heading(tr!("health.title")))
        .child(cli)
        .child(vault("common.project_vault", &st.project_vault))
        .child(vault("common.personal_vault", &st.global_vault))
        .child(
            check(
                if findings > 0 { Level::Warn } else { Level::Ok },
                tr!("review.title"),
                tr!("health.review", drafts = drafts, findings = findings),
            )
            .child(action(tr!("review.title"), state::show_review)),
        )
}

/// A vault's row: `title` is the translation key of its name.
fn vault(title: &'static str, v: &VaultStatus) -> View {
    let path = v.path.display().to_string();
    let title = tr!(title);
    let row = match (&v.obsidian, v.exists) {
        (_, false) => check(
            Level::Error,
            title,
            tr!("health.vault_missing", path = path),
        ),
        (Registration::Registered { .. }, true) => {
            check(Level::Ok, title, tr!("health.vault_open", path = path))
        }
        (Registration::NotRegistered, true) => check(
            Level::Warn,
            title,
            tr!("health.vault_not_open", path = path),
        ),
        (Registration::Unknown(e), true) => {
            let why = e.clone();
            check(Level::Warn, title, tr!("health.vault_unknown", why = why))
        }
    };
    let folder = v.path.clone();
    match (&v.obsidian, v.exists) {
        (Registration::NotRegistered, true) => row.child(action(tr!(REVEAL), move || {
            drop(reveal_path(folder.clone()))
        })),
        _ => row,
    }
}

fn agents(list: Vec<AgentState>) -> View {
    let mut c = card().child(heading(tr!("agents.title")));
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
                    text(tr!("agents.is_set_up"))
                        .text_sm()
                        .text_color(crate::widgets::tone("ok")),
                ),
            ),
            Ok(files) => row
                .child(
                    view()
                        .flex_1()
                        .min_w(px(160.0))
                        .child(muted(tr!("agents.not_set_up", files = files.len()))),
                )
                .child(
                    primary(tr!("agents.set_up"), move || state::set_up(agent))
                        .busy(state::running(Work::SetUp(agent))),
                ),
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
                tr!("claude_memory.unreadable"),
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
    let (pending, all, dir) = (fresh.len(), m.items.len(), m.dir.display().to_string());
    let mut c = card()
        .test_id("claude-memory")
        .child(heading(tr!("claude_memory.title")))
        .child(muted(tr!(
            "claude_memory.about",
            pending = pending,
            all = all,
            dir = dir
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
                .child(muted(tr!("claude_memory.already")))
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
            view().flex_row().pt_2().child(
                primary(tr!("claude_memory.import"), move || {
                    state::import_claude(chosen.get_untracked())
                })
                .busy(state::running(Work::ImportClaude)),
            ),
        ),
    )
}
