//! What waits for a person across projects: agent drafts to approve, and findings to resolve.
//! Notes are read in Obsidian; this page decides on them.

use crate::data::ReviewGroup;
use crate::icons::REFRESH;
use crate::state::{self, Decision, Load, Work};
use crate::widgets::{
    Level, Press, about, action, card, check, dialog_box, heading, icon_button, loading, muted,
    notice_area, page, page_bar, primary,
};
use crate::words::{self, tr};
use nsh_base::theme;
use nsh_core::prelude::*;
use obmem_core::note::Scope;
use obmem_core::obsidian;
use obmem_core::review::{Item, Kind};
use obmem_core::store::Verdict;
use std::path::{Path, PathBuf};

pub(crate) fn review_page() -> impl IntoElement {
    page(
        page_bar(
            tr!("review.title"),
            view(),
            vec![
                icon_button(REFRESH, tr!("common.refresh"), state::load_review)
                    .busy(state::running(Work::LoadReview))
                    .into_element(),
            ],
        ),
        view()
            .gap_4()
            .child(about(tr!("review.about")))
            .child(notice_area())
            .child(view().gap_4().child(dynamic(move || {
                match state::review() {
                    Load::Loading => vec![loading(tr!("common.loading")).into_element()],
                    Load::Failed(e) => {
                        vec![check(Level::Error, tr!("review.unreadable"), e).into_element()]
                    }
                    Load::Ready(groups) if groups.is_empty() => {
                        vec![muted(tr!("review.no_projects")).into_element()]
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
    // The personal vault is every project's: it has no project, and no name but that.
    let (id, title) = match g.project {
        Some(_) => (format!("review-{}", g.name), heading(g.name.clone())),
        None => (
            "review-personal".to_owned(),
            heading(tr!("common.personal_vault")),
        ),
    };
    let mut head = view()
        .flex_row()
        .items_center()
        .gap_3()
        .pb_1()
        .child(view().flex_1().child(title));
    if let Some(project) = g.project.clone() {
        head = head.child(
            action(tr!("review.run_audit"), {
                let project = project.clone();
                move || state::run_audit(project.clone())
            })
            .busy(state::running(Work::Audit(project))),
        );
    }
    let mut c = card().test_id(&id).child(head);
    if g.items.is_empty() {
        return c.child(muted(tr!("review.nothing")));
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

    /// `button`, busy while `decision` runs on this item, and waiting while another one does:
    /// two decisions on one item must not race.
    fn decides(&self, decision: Decision, button: Press) -> Press {
        button
            .busy(state::running(Work::Review(self.item.clone(), decision)))
            .blocked(state::reviewing(self.item.clone()))
    }

    /// The button that resolves the finding with `verdict`.
    fn resolves(&self, verdict: Verdict, main: bool) -> Press {
        let label = move || words::verdict(verdict);
        let at = self.clone();
        let press = move || at.resolve(verdict);
        self.decides(
            Decision::Resolve(verdict),
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
    // A note's type, status, rule and severity are what its file says: data, as in Obsidian.
    let (note_type, status, author, updated) = (
        item.note_type.clone(),
        item.status.clone(),
        item.author.clone(),
        item.updated.clone(),
    );
    let about = match item.kind {
        Kind::Draft => muted(tr!(
            "review.draft_about",
            note_type = note_type,
            author = author,
            updated = updated
        )),
        Kind::Finding => match (&item.rule, &item.severity) {
            (Some(rule), Some(sev)) => muted(format!("{rule} ({sev}) · {status} · {updated}")),
            _ => muted(tr!(
                "review.finding_about",
                status = status,
                author = author,
                updated = updated
            )),
        },
    };
    let mut actions = view().flex_row().flex_wrap().gap_2().pt_1();
    match item.kind {
        Kind::Draft => {
            let a = at.clone();
            actions = actions.child(at.decides(
                Decision::Approve,
                primary(tr!("review.approve"), move || {
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
                    Decision::Verify,
                    action(tr!("review.verify"), move || {
                        state::verify(a.item.clone(), a.root.clone(), a.scope, a.rel.clone())
                    }),
                ));
            }
            actions = actions.child(reject(at.clone()));
        }
    }
    if let Some(id) = vault_id {
        let url = obsidian::open_uri(id, &item.rel);
        actions = actions.child(action(tr!("common.open_in_obsidian"), move || {
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
        .child(about);
    if let Some(target) = item.target.clone() {
        body = body.child(muted(tr!("review.about_note", target = target)));
    }
    if !item.summary.is_empty() {
        body = body.child(text(item.summary.clone()).text_sm().number_of_lines(3u32));
    }
    body.child(actions)
}

/// Rejecting silences the check for that spot for good, so it asks first, in a dialog.
fn reject(at: At) -> View {
    let asking = signal(false);
    let close = move || asking.set(false);
    view()
        .child(
            action(tr!("review.reject_ask"), move || asking.set(true))
                .blocked(state::reviewing(at.item.clone())),
        )
        .child(dynamic(move || {
            asking.get().then(|| {
                dialog_box(
                    "reject",
                    tr!("review.reject_title"),
                    420.0,
                    muted(tr!("review.reject_about")),
                    vec![
                        action(tr!("common.cancel"), close).into_element(),
                        at.resolves(Verdict::Rejected, true).into_element(),
                    ],
                    close,
                )
            })
        }))
}
