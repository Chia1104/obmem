//! One project: its health and each agent's setup.

use crate::data::{AgentState, Detail};
use crate::state::{self, Load, Notice};
use crate::widgets::{Level, action, card, check, heading, muted, primary};
use nsh_base::theme;
use nsh_core::prelude::*;
use obmem_core::status::{Registration, VaultStatus};

const REVEAL: &str = if cfg!(windows) { "Show in Explorer" } else { "Show in Finder" };

pub(crate) fn project_page() -> impl IntoElement {
    scroll().flex_1().child(view().gap_4().p_6().child(dynamic(move || match state::detail() {
        Load::Loading if state::selected().is_none() => vec![muted("Add a project to see its memory setup.").into_element()],
        Load::Loading => vec![muted("Loading…").into_element()],
        Load::Failed(e) => vec![
            check(Level::Error, "Could not read this project", e).into_element(),
            view().flex_row().gap_2().child(action("Try again", state::refresh)).child(action("Remove from list", state::remove_selected)).into_element(),
        ],
        Load::Ready(d) => page(d),
    })))
}

fn page(d: Detail) -> Vec<AnyElement> {
    let st = &d.status;
    let root = st.project_root.clone();
    let mut out = vec![
        view()
            .flex_row()
            .items_center()
            .gap_3()
            .child(view().flex_1().gap_1().child(text(st.project_name.clone()).text_xl().font_weight(FontWeight::SEMIBOLD)).child(muted(root.display().to_string())))
            .child(action("Refresh", state::refresh))
            .child(action(REVEAL, move || drop(reveal_path(root.clone()))))
            .child(action("Remove from list", state::remove_selected))
            .into_element(),
    ];
    if let Some(n) = state::notice() {
        out.push(notice(n).into_element());
    }
    if !st.initialized {
        out.push(
            card()
                .child(text("obmem is not set up in this folder"))
                .child(muted("Setting it up creates the project vault and .obmem.toml. Until then, hooks do nothing here."))
                .child(view().flex_row().pt_2().child(primary("Set up obmem here", state::initialize)))
                .into_element(),
        );
        return out;
    }
    out.push(health(&d).into_element());
    out.push(agents(d.agents).into_element());
    out
}

fn notice(n: Notice) -> impl IntoElement {
    let body = match n {
        Notice::Error(e) => card().child(check(Level::Error, "That did not work", e)),
        Notice::SetUp(done) => {
            let mut c = card().child(text(format!("{} is set up", done.agent.label())).font_weight(FontWeight::MEDIUM));
            for f in &done.changed {
                c = c.child(muted(format!("Updated {f}")));
            }
            for m in &done.manual {
                c = c.child(muted(format!("Next: {m}")));
            }
            c
        }
    };
    body.child(view().flex_row().pt_2().child(action("Dismiss", state::dismiss_notice)))
}

fn health(d: &Detail) -> View {
    let app = env!("CARGO_PKG_VERSION");
    let cli = match &d.cli {
        Some(v) if v == app => check(Level::Ok, "obmem command", format!("obmem {v} is on PATH, where hooks and MCP configs run it.")),
        Some(v) => check(
            Level::Warn,
            "obmem command",
            format!("obmem {v} is on PATH, but this app is {app}. Install the matching version so agents run the same obmem."),
        ),
        None => check(Level::Error, "obmem command", "obmem is not on PATH. Every hook and MCP config runs it from there: install it first."),
    };
    let st = &d.status;
    card()
        .child(heading("Health"))
        .child(cli)
        .child(vault("Project vault", &st.project_vault))
        .child(vault("Personal vault", &st.global_vault))
        .child(check(
            if st.open_findings > 0 { Level::Warn } else { Level::Ok },
            "Review",
            format!("{} drafts and {} open findings in the project vault. Review them in Obsidian.", st.drafts, st.open_findings),
        ))
}

fn vault(title: &str, v: &VaultStatus) -> View {
    let path = v.path.display().to_string();
    let row = match (&v.obsidian, v.exists) {
        (_, false) => check(Level::Error, title.to_string(), format!("{path} does not exist.")),
        (Registration::Registered { .. }, true) => check(Level::Ok, title.to_string(), format!("{path} is open in Obsidian.")),
        (Registration::NotRegistered, true) => check(
            Level::Warn,
            title.to_string(),
            format!("{path} is not open in Obsidian, so search and link checks skip it. In Obsidian, choose “Open folder as vault” and pick this folder."),
        ),
        (Registration::Unknown(e), true) => check(Level::Warn, title.to_string(), format!("Could not read Obsidian's vault list: {e}")),
    };
    let folder = v.path.clone();
    match (&v.obsidian, v.exists) {
        (Registration::NotRegistered, true) => row.child(action(REVEAL, move || drop(reveal_path(folder.clone())))),
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
            .child(text(agent.label()).w(px(170.0)));
        row = match a.missing {
            Ok(files) if files.is_empty() => row.child(view().flex_1().child(muted("Set up"))),
            Ok(files) => row
                .child(view().flex_1().child(muted(format!("Not set up: {} file(s) to update", files.len()))))
                .child(primary("Set up", move || state::set_up(agent))),
            Err(e) => row.child(view().flex_1().child(text(e).text_sm().text_color(move || theme().destructive))),
        };
        c = c.child(row);
    }
    c
}
