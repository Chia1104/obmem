//! Keeping a project's memory in a private vault, outside the repo: the button that offers it and
//! the dialog that asks for the folder. What it does is `obmem init --private <folder>`.

use crate::form::Problems;
use crate::state::{self, Work};
use crate::widgets::{Marked, action, dialog_box, muted, primary, problem_line, text_box};
use crate::words::{self, tr};
use nsh_core::prelude::*;

/// A button that opens the dialog, and the dialog while it is open. `label` is a translation key.
/// `current` is the folder the vault is in now, when it is private: the dialog starts from it.
/// `strong`: the button is what the page is there for.
///
/// Make it with the page's content: the dialog then closes when the project is read again, which
/// a change of folder ends with.
pub(crate) fn offer(label: &'static str, current: Option<String>, strong: bool) -> View {
    let open = signal(false);
    let close = move || open.set(false);
    let press = move || open.set(true);
    let button = if strong {
        primary(tr!(label), press)
    } else {
        action(tr!(label), press)
    };
    view().flex_row().child(button).child(dynamic(move || {
        open.get().then(|| dialog(current.clone(), close))
    }))
}

fn dialog(current: Option<String>, close: impl Fn() + Clone + 'static) -> AnyElement {
    let folder = signal(current.unwrap_or_default());
    // The rule needs the file system, so obmem checks it when the folder is sent, and the
    // problem is about the folder that was sent: it goes when the field changes.
    let problems = Problems::of(move || {
        state::vault_rule(&folder.get())
            .map(|rule| ((), words::rule(&rule)))
            .into_iter()
            .collect()
    });
    let send = move || {
        if problems.none() {
            state::make_private(folder.get_untracked());
        }
    };
    let field = view()
        .gap_1()
        .child(
            text(tr!("private.folder"))
                .text_sm()
                .font_weight(FontWeight::MEDIUM),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    view().flex_1().min_w(px(0.0)).child(
                        text_box(folder, "~/Obsidian/project-memory")
                            .marked(problems.marks(()))
                            .test_id("private-folder"),
                    ),
                )
                .child(action(tr!("private.choose"), move || {
                    state::pick_vault_folder(folder)
                })),
        )
        .child(problem_line(problems.at(())));
    let body = view()
        .gap_3()
        .child(muted(tr!("private.about")))
        .child(field)
        .child(muted(tr!("private.each_machine")));
    dialog_box(
        "private-vault",
        tr!("private.title"),
        520.0,
        body,
        vec![
            action(tr!("common.cancel"), close.clone()).into_element(),
            primary(tr!("private.confirm"), send)
                .busy(state::running(Work::MakePrivate))
                .into_element(),
        ],
        close,
    )
}
