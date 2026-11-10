//! What waits for a person across projects: agent drafts to approve, and findings to resolve.
//! Notes are read in Obsidian; this page decides on them.

use crate::data::ReviewGroup;
use crate::state::{self, Load, Work};
use crate::widgets::{
    Level, Press, action, card, check, heading, loading, muted, notice_area, page, page_header,
    primary,
};
use nsh_base::theme;
use nsh_core::prelude::*;
use obmem_core::note::Scope;
use obmem_core::obsidian;
use obmem_core::review::{Item, Kind};
use obmem_core::store::Verdict;
use std::path::{Path, PathBuf};

pub(crate) fn review_page() -> impl IntoElement {
    page(
        page_header(
            "Review",
            muted(
                "Agent drafts to approve and findings to resolve. Read a note in Obsidian, decide here.",
            ),
            vec![
                action("Refresh", state::load_review)
                    .busy(state::running(Work::LoadReview))
                    .into_element(),
            ],
        ),
        view()
            .gap_4()
            .child(notice_area())
            .child(view().gap_4().child(dynamic(move || {
                match state::review() {
                    Load::Loading => vec![loading("Loading…").into_element()],
                    Load::Failed(e) => vec![
                        check(Level::Error, "Could not read the review list", e).into_element(),
                    ],
                    Load::Ready(groups) if groups.is_empty() => {
                        vec![muted("No projects are set up yet.").into_element()]
                    }
                    Load::Ready(groups) => groups
                        .into_iter()
                        .map(|g| group(g).into_element())
                        .collect(),
                }
            }))),
    )
}

fn group(g: ReviewGroup) -> View {
    let mut head = view()
        .flex_row()
        .items_center()
        .gap_3()
        .pb_1()
        .child(view().flex_1().child(heading(g.title.clone())));
    if let Some(project) = g.project.clone() {
        head = head.child(
            action("Run audit", {
                let project = project.clone();
                move || state::run_audit(project.clone())
            })
            .busy(state::running(Work::Audit(project))),
        );
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

/// The note an action is about: exactly this path in this vault, for the review item `item`.
#[derive(Clone)]
struct At {
    item: String,
    root: PathBuf,
    scope: Scope,
    rel: String,
}

impl At {
    fn resolve(&self, verdict: Verdict) {
        state::resolve(
            self.item.clone(),
            self.root.clone(),
            self.scope,
            self.rel.clone(),
            verdict,
        );
    }

    /// `button`, busy while the decision named `label` runs on this item, and waiting while
    /// another one does: two decisions on one item must not race.
    fn decides(&self, label: &'static str, button: Press) -> Press {
        button
            .busy(state::running(Work::Review(self.item.clone(), label)))
            .blocked(state::reviewing(self.item.clone()))
    }

    /// The button that resolves the finding with `verdict`.
    fn resolves(&self, verdict: Verdict, main: bool) -> Press {
        let label = state::verdict_label(verdict);
        let at = self.clone();
        let press = move || at.resolve(verdict);
        self.decides(
            label,
            if main {
                primary(label, press)
            } else {
                action(label, press)
            },
        )
    }
}

fn row(root: &Path, vault_id: Option<&str>, item: Item) -> View {
    let scope = if item.scope == "global" {
        Scope::Global
    } else {
        Scope::Project
    };
    let at = At {
        item: item.id.clone(),
        root: root.to_path_buf(),
        scope,
        rel: item.rel.clone(),
    };
    let about = match item.kind {
        Kind::Draft => format!(
            "{} draft · by {} · {}",
            item.note_type, item.author, item.updated
        ),
        Kind::Finding => match (&item.rule, &item.severity) {
            (Some(rule), Some(sev)) => {
                format!("{rule} ({sev}) · {} · {}", item.status, item.updated)
            }
            _ => format!(
                "finding · {} · by {} · {}",
                item.status, item.author, item.updated
            ),
        },
    };
    let mut actions = view().flex_row().flex_wrap().gap_2().pt_1();
    match item.kind {
        Kind::Draft => {
            let a = at.clone();
            actions = actions.child(at.decides(
                state::APPROVE,
                primary(state::APPROVE, move || {
                    state::approve(a.item.clone(), a.root.clone(), a.scope, a.rel.clone())
                }),
            ));
        }
        Kind::Finding => {
            actions = actions.child(at.resolves(Verdict::Fixed, true));
            if item.status != "accepted" {
                actions = actions.child(at.resolves(Verdict::Accepted, false));
            }
            // The note the finding is about, in the finding's own vault only.
            if let Some(target) = item.target.clone() {
                let a = At {
                    rel: target,
                    ..at.clone()
                };
                actions = actions.child(at.decides(
                    state::VERIFY,
                    action(state::VERIFY, move || {
                        state::verify(a.item.clone(), a.root.clone(), a.scope, a.rel.clone())
                    }),
                ));
            }
            actions = actions.child(reject(at.clone()));
        }
    }
    if let Some(id) = vault_id {
        let url = obsidian::open_uri(id, &item.rel);
        actions = actions.child(action("Open in Obsidian", move || {
            drop(open_url(url.clone()))
        }));
    }
    let mut body = view()
        .test_id(&format!("item-{}", item.id))
        .gap_1()
        .py_3()
        .border_t_1()
        .border_t_color(move || theme().border)
        .child(text(item.title.clone()).font_weight(FontWeight::MEDIUM))
        .child(muted(about));
    if let Some(target) = &item.target {
        body = body.child(muted(format!("About {target}")));
    }
    if !item.summary.is_empty() {
        body = body.child(text(item.summary.clone()).text_sm().number_of_lines(3u32));
    }
    body.child(actions)
}

/// Rejecting silences the check for that spot for good, so it asks first.
fn reject(at: At) -> AnyElement {
    let asking = signal(false);
    dynamic(move || {
        if !asking.get() {
            return vec![
                action("Reject…", move || asking.set(true))
                    .blocked(state::reviewing(at.item.clone()))
                    .into_element(),
            ];
        }
        vec![
            muted("A false positive? It is never reported again.").into_element(),
            at.resolves(Verdict::Rejected, true).into_element(),
            action("Cancel", move || asking.set(false)).into_element(),
        ]
    })
}
