//! What waits for a person across projects: agent drafts to approve, and findings to resolve.
//! Notes are read in Obsidian; this page decides on them.

use crate::data::ReviewGroup;
use crate::state::{self, Load};
use crate::widgets::{Level, action, card, check, heading, muted, notice_area, primary};
use nsh_base::theme;
use nsh_core::prelude::*;
use obmem_core::obsidian;
use obmem_core::review::{Item, Kind};
use obmem_core::store::Verdict;
use std::path::{Path, PathBuf};

pub(crate) fn review_page() -> impl IntoElement {
    scroll().flex_1().child(
        view()
            .gap_4()
            .p_6()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        view()
                            .flex_1()
                            .gap_1()
                            .child(text("Review").text_xl().font_weight(FontWeight::SEMIBOLD))
                            .child(muted("Agent drafts to approve and findings to resolve. Read a note in Obsidian, decide here.")),
                    )
                    .child(action("Refresh", state::load_review)),
            )
            .child(notice_area())
            .child(dynamic(move || match state::review() {
                Load::Loading => vec![muted("Loading…").into_element()],
                Load::Failed(e) => vec![check(Level::Error, "Could not read the review list", e).into_element()],
                Load::Ready(groups) if groups.is_empty() => {
                    vec![muted("No projects are set up yet.").into_element()]
                }
                Load::Ready(groups) => groups.into_iter().map(|g| group(g).into_element()).collect(),
            })),
    )
}

fn group(g: ReviewGroup) -> View {
    let mut head = view().flex_row().items_center().gap_3().child(view().flex_1().child(heading(g.title.clone())));
    if let Some(project) = g.project.clone() {
        head = head.child(action("Run audit", move || state::run_audit(project.clone())));
    }
    let mut c = card().test_id(&format!("review-{}", g.title)).child(head);
    if g.items.is_empty() {
        return c.child(muted("Nothing to review."));
    }
    for item in g.items {
        c = c.child(row(&g.root, g.vault_id.as_deref(), item));
    }
    c
}

fn row(root: &Path, vault_id: Option<&str>, item: Item) -> View {
    let about = match item.kind {
        Kind::Draft => format!("{} draft · by {} · {}", item.note_type, item.author, item.updated),
        Kind::Finding => match (&item.rule, &item.severity) {
            (Some(rule), Some(sev)) => format!("{rule} ({sev}) · {} · {}", item.status, item.updated),
            _ => format!("finding · {} · by {} · {}", item.status, item.author, item.updated),
        },
    };
    let mut actions = view().flex_row().gap_2().pt_1();
    let (r, rel) = (root.to_path_buf(), item.rel.clone());
    match item.kind {
        Kind::Draft => {
            actions = actions.child(primary("Approve", move || state::approve(r.clone(), rel.clone())));
        }
        Kind::Finding => {
            let fixed = (root.to_path_buf(), item.rel.clone());
            actions = actions.child(primary("Fixed", move || state::resolve(fixed.0.clone(), fixed.1.clone(), Verdict::Fixed)));
            if item.status != "accepted" {
                let accept = (root.to_path_buf(), item.rel.clone());
                actions = actions.child(action("Accept", move || {
                    state::resolve(accept.0.clone(), accept.1.clone(), Verdict::Accepted)
                }));
            }
            if let Some(target) = item.target.clone() {
                let r = root.to_path_buf();
                actions = actions.child(action("Verify note", move || state::verify(r.clone(), target.clone())));
            }
            actions = actions.child(reject(root.to_path_buf(), item.rel.clone()));
        }
    }
    if let Some(id) = vault_id {
        let url = obsidian::open_uri(id, &item.rel);
        actions = actions.child(action("Open in Obsidian", move || drop(open_url(url.clone()))));
    }
    let mut body = view()
        .test_id(&format!("item-{}", item.id))
        .gap_1()
        .py_3()
        .border_t_1()
        .border_t_color(move || theme().border)
        .child(text(item.title.clone()).font_weight(FontWeight::MEDIUM))
        .child(muted(about));
    if !item.summary.is_empty() {
        body = body.child(text(item.summary.clone()).text_sm().number_of_lines(3u32));
    }
    body.child(actions)
}

/// Rejecting silences the check for that spot for good, so it asks first.
fn reject(root: PathBuf, rel: String) -> AnyElement {
    let asking = signal(false);
    dynamic(move || {
        if !asking.get() {
            return vec![action("Reject…", move || asking.set(true)).into_element()];
        }
        let (r, p) = (root.clone(), rel.clone());
        vec![
            muted("A false positive? It is never reported again.").into_element(),
            primary("Reject", move || state::resolve(r.clone(), p.clone(), Verdict::Rejected)).into_element(),
            action("Cancel", move || asking.set(false)).into_element(),
        ]
    })
}
