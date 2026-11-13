//! Toasts: what an action did, said in a corner of the window without moving the page. A result
//! that is one line goes here (`done`), and so does a failure no field is to blame for
//! (`failed`). A result with more to read stays on the page (`widgets::notice_area`), and a rule
//! a field breaks shows at the field (`crate::form`).
//!
//! Built on nsh-base's unstyled parts: a `ToastManager` keeps the toasts and their lifecycle, a
//! `ToastStack` lays them out, a `Toast` is each one's root.

use crate::icons::{X, icon};
use crate::motion::Eased;
use crate::widgets::{Level, mark};
use crate::words::named;
use nsh_base::{
    Anchor, ToastManager, ToastMotion, ToastOptions, ToastStackState, ToastTransitionStatus,
    button, theme, toast, toast_stack,
};
use nsh_core::PointerEvents;
use nsh_core::prelude::*;
use nsh_core::time::now;
use std::time::Duration;

/// How many show at once; older ones wait behind them.
const SHOWN: usize = 3;
/// How often the lifecycle advances while a toast is up.
const TICK: Duration = Duration::from_millis(50);
/// How long a toast that reports success stays.
const STAY: Duration = Duration::from_secs(4);

#[derive(Clone, PartialEq)]
struct Report {
    level: Level,
    title: String,
    detail: String,
}

type Entry = (u64, Report, ToastTransitionStatus);

/// The window's toasts. Made by [`toaster`]; the actions reach it through a global.
#[derive(Clone, Copy)]
struct Host {
    /// The window's owner: the timer must outlive the button whose press showed a toast.
    owner: Owner,
    manager: Stored<ToastManager<u64, Report>>,
    shown: Signal<Vec<Entry>>,
    stack: ToastStackState,
    last_id: Stored<u64>,
    ticking: Stored<bool>,
}

#[derive(Clone, Default)]
struct Hosts(Option<Host>);

impl Host {
    fn sync(self) {
        self.shown.set(self.manager.with(|m| {
            m.visible(SHOWN)
                .map(|(id, report, status)| (*id, report.clone(), status))
                .collect()
        }));
    }

    fn push(self, report: Report, stay: Option<Duration>) {
        let id = self.last_id.with(|n| {
            *n += 1;
            *n
        });
        self.manager
            .with(|m| m.push(id, report, ToastOptions::timeout(stay), now()));
        self.sync();
        if !self.ticking.replace(true) {
            self.tick_later();
        }
    }

    fn dismiss(self, id: u64) {
        self.manager.with(|m| m.dismiss(&id, now()));
        self.sync();
    }

    /// Advances the lifecycle (enter, time out, exit), and goes on only while a toast is up: a
    /// window with nothing to say wakes nothing.
    fn tick(self) {
        // Pointing at the stack expands it and holds the toasts.
        let held = untracked(|| self.stack.is_expanded());
        let (changed, empty) = self.manager.with(|m| {
            let changed = m.advance(now(), held).changed;
            (changed, m.is_empty())
        });
        if changed {
            self.sync();
        }
        if empty {
            self.ticking.replace(false);
        } else {
            self.tick_later();
        }
    }

    fn tick_later(self) {
        self.owner
            .run(|| drop(set_timeout(TICK, move || self.tick())));
    }
}

fn push(report: Report, stay: Option<Duration>) {
    if let Some(host) = global::<Hosts>().get_untracked().0 {
        host.push(report, stay);
    }
}

/// Says that an action worked. It goes by itself.
pub(crate) fn done(message: impl Into<String>) {
    push(
        Report {
            level: Level::Ok,
            title: message.into(),
            detail: String::new(),
        },
        Some(STAY),
    );
}

/// Says that an action failed, and why. It stays until it is dismissed: the reason may be long,
/// and the user may be looking elsewhere when it comes.
pub(crate) fn failed(why: impl Into<String>) {
    push(
        Report {
            level: Level::Error,
            title: t!("toast.failed"),
            detail: why.into(),
        },
        None,
    );
}

/// Where the toasts show: the window's bottom right corner, over the page. Placed once, by the
/// root component.
pub(crate) fn toaster() -> impl IntoElement {
    let host = Host {
        owner: Owner::current(),
        manager: stored(ToastManager::new(ToastMotion::sonner())),
        shown: signal(Vec::new()),
        stack: ToastStackState::new(),
        last_id: stored(0),
        ticking: stored(false),
    };
    global::<Hosts>().set(Hosts(Some(host)));
    overlay()
        .right_0()
        .bottom_0()
        .p_4()
        // Only the toasts take presses: the page under the rest of the corner stays usable.
        .pointer_events(PointerEvents::BoxNone)
        .child(
            toast_stack()
                .placement(Anchor::BottomRight)
                .state(host.stack)
                .w(px(360.0))
                .child(each(
                    host.shown,
                    |(id, _, _)| *id,
                    move |entry, _| one(host, entry),
                )),
        )
}

fn one(host: Host, entry: ReadSignal<Entry>) -> impl IntoElement {
    let (id, report, _) = entry.get_untracked();
    let said = if report.detail.is_empty() {
        report.title.clone()
    } else {
        format!("{}. {}", report.title, report.detail)
    };
    let mut words = view()
        .test_id("toast")
        .flex_1()
        .min_w(px(0.0))
        .gap_1()
        .child(text(report.title).text_sm().font_weight(FontWeight::MEDIUM));
    if !report.detail.is_empty() {
        words = words.child(
            text(report.detail)
                .text_xs()
                .text_color(move || theme().muted_foreground),
        );
    }
    toast()
        .transition_status(move || entry.get().2)
        .accessibility_label(Some(SharedString::from(said)))
        .on_dismiss_request(move || host.dismiss(id))
        .flex_row()
        .items_start()
        .gap_3()
        .p_3()
        .rounded(move || theme().radius)
        .bg(move || theme().card)
        .border_1()
        .border_color(move || theme().border)
        .shadow_lg()
        .settles()
        .starting_class(Some("opacity-0 translate-y-4".into()))
        .ending_class(Some("opacity-0 scale-95".into()))
        .child(mark(report.level))
        .child(words)
        .child(
            button()
                .w(px(20.0))
                .h(px(20.0))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded(move || theme().radius)
                .text_color(move || theme().muted_foreground)
                .washes()
                .hover(|s| s.text_color(move || theme().foreground))
                .accessibility_label(named("common.dismiss"))
                .on_press(move || host.dismiss(id))
                .child(icon(X, 14.0)),
        )
}
