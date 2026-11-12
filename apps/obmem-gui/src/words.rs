//! The app's words. Every text the user reads is a key in `locales/app.yml`, with its English and
//! its Chinese, and shows in the language the window is in (`state::language`).
//!
//! - Text on screen is `tr!("key", name = value)`: a closure, so the text follows the language.
//! - Text that is said once (a toast, a notice, a dialog's title) is `t!("key")`, when it is said.
//! - What obmem's libraries report is typed (`Rule`, `Decided`, `ManualStep`, `Summary`), with the
//!   English the CLI prints as its `Display`. The functions here say the same in the window's
//!   language. They read the language, so they run on the engine thread, never in `data`.
//!
//! Three things are not translated: the reason an action failed (the library's error), names and
//! values that are data (a path, a note's type or status, a scanner rule), and what the Agent view
//! shows, which is exactly what an agent receives.

use nsh_core::prelude::{SharedString, t};
use obmem_agents::setup::{COPILOT_MCP_CONFIG, ManualStep};
use obmem_core::import::Skip;
use obmem_core::secrets::Ignored;
use obmem_core::settings::Rule;
use obmem_core::store::{Decided, Verdict};
use obmem_sync::{Cleanup, Outcome, Summary};

/// A text that follows the language: `tr!("key", name = value)` is a closure giving the text of
/// `key` now, for anything that takes `impl IntoReactive<SharedString>`.
macro_rules! tr {
    ($($t:tt)*) => {
        move || ::nsh_core::t!($($t)*)
    };
}
pub(crate) use tr;

/// The text of `key`, following the language, for a prop that may have none: a control's
/// accessibility label.
pub(crate) fn named(key: &'static str) -> impl Fn() -> Option<SharedString> + Copy {
    move || Some(t!(key).into())
}

/// The rule a setting's value breaks.
pub(crate) fn rule(rule: &Rule) -> String {
    match rule {
        Rule::VaultInsideProject => t!("rule.vault_inside_project"),
        Rule::EnvFileInsideProject(file) => t!("rule.env_file_inside_project", file = file),
        Rule::StaleDaysAtLeastOne => t!("rule.stale_days_at_least_one"),
        Rule::DuplicateThresholdInRange => t!("rule.duplicate_threshold_in_range"),
        Rule::PersonalVaultAbsolute => t!("rule.personal_vault_absolute"),
        Rule::EndpointIsUrl => t!("rule.endpoint_is_url"),
        Rule::BucketIsName => t!("rule.bucket_is_name"),
    }
}

/// Why the scanner would ignore the allowlist entry `entry`.
pub(crate) fn ignored(entry: &str, why: &Ignored) -> String {
    let entry = entry.trim();
    match why {
        Ignored::Empty => t!("ignored.empty", entry = entry),
        Ignored::MatchesAnything => t!("ignored.matches_anything", entry = entry),
        Ignored::InvalidRegex(e) => t!("ignored.invalid_regex", entry = entry, why = e),
        Ignored::TooShort(min) => t!("ignored.too_short", entry = entry, min = min),
        Ignored::TooGeneric => t!("ignored.too_generic", entry = entry),
    }
}

/// What a decision on a review item did.
pub(crate) fn decided(done: &Decided) -> String {
    match done {
        Decided::Approved { id } => t!("decided.approved", id = id),
        Decided::Resolved {
            id,
            verdict,
            suppressed_in: Some(file),
        } => t!(
            "decided.resolved_and_suppressed",
            id = id,
            status = verdict.as_str(),
            file = file.display()
        ),
        Decided::Resolved {
            id,
            verdict,
            suppressed_in: None,
        } => t!("decided.resolved", id = id, status = verdict.as_str()),
        Decided::Verified {
            id,
            date,
            commit: Some(commit),
        } => t!(
            "decided.verified_at_commit",
            id = id,
            date = date,
            commit = commit
        ),
        Decided::Verified {
            id,
            date,
            commit: None,
        } => t!("decided.verified", id = id, date = date),
    }
}

/// The label of the button that resolves a finding with `verdict`.
pub(crate) fn verdict(verdict: Verdict) -> String {
    match verdict {
        Verdict::Fixed => t!("review.fixed"),
        Verdict::Accepted => t!("review.accept"),
        Verdict::Rejected => t!("review.reject"),
        Verdict::Open => t!("review.reopen"),
    }
}

/// A step of an agent's setup that only the user can take.
pub(crate) fn manual(step: &ManualStep) -> String {
    match step {
        ManualStep::ApproveInClaude => t!("manual.approve_in_claude"),
        ManualStep::TrustInCodex { project } => {
            t!("manual.trust_in_codex", project = project.display())
        }
        ManualStep::EnableInCursor => t!("manual.enable_in_cursor"),
        ManualStep::RegisterWithCopilot => {
            t!("manual.register_with_copilot", config = COPILOT_MCP_CONFIG)
        }
        ManualStep::ApproveInAmp { project } => {
            t!("manual.approve_in_amp", project = project.display())
        }
    }
}

/// Why a Claude Code memory was not imported.
pub(crate) fn skip(why: &Skip) -> String {
    match why {
        Skip::Exists(target) => t!("claude_memory.exists", target = target.display()),
        Skip::Failed(e) => e.clone(),
    }
}

/// What a sync did, in one line.
pub(crate) fn summary(s: &Summary) -> String {
    let head = match (s.seq, s.created) {
        (0, _) => t!("sync.summary.nothing"),
        (seq, true) => t!("sync.summary.snapshot", seq = seq),
        (seq, false) => t!("sync.summary.up_to_date", seq = seq),
    };
    let conflicts = if s.conflicts == 1 {
        "sync.summary.conflict"
    } else {
        "sync.summary.conflicts"
    };
    let parts: Vec<String> = [
        (s.up, "sync.summary.up"),
        (s.down, "sync.summary.down"),
        (s.conflicts, conflicts),
        (s.deferred, "sync.summary.deferred"),
        (s.skipped, "sync.summary.skipped"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, key)| t!(key, n = n))
    .collect();
    if parts.is_empty() {
        head
    } else {
        t!(
            "sync.summary.with_parts",
            head = head,
            parts = parts.join(&t!("common.list_separator"))
        )
    }
}

/// How the last sync went: what it did, or the library's reason when it failed.
pub(crate) fn outcome(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Failed(why) => why.clone(),
        Outcome::Synced {
            summary: s,
            cleanup,
        } => match cleanup {
            Cleanup::Nothing => summary(s),
            Cleanup::Removed { snapshots, blobs } => t!(
                "sync.cleanup.removed",
                summary = summary(s),
                snapshots = snapshots,
                blobs = blobs
            ),
            Cleanup::Failed(why) => t!("sync.cleanup.failed", summary = summary(s), why = why),
        },
    }
}

#[cfg(test)]
mod tests {
    use nsh_core::i18n::Translations;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// The keys the code builds instead of writing out.
    const BUILT: &[&str] = &[
        "allowlist.fingerprints",
        "allowlist.fingerprints_help",
        "allowlist.patterns",
        "allowlist.patterns_help",
        "allowlist.words",
        "allowlist.words_help",
        "allowlist.add_patterns",
        "allowlist.add_words",
    ];

    /// Every string in the sources that is shaped like a key, `area.name`, and starts with an
    /// area of `locales/app.yml`. A key is always written out whole, or is in [`BUILT`].
    fn keys_in_the_sources(areas: &BTreeSet<&str>) -> BTreeSet<String> {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut keys: BTreeSet<String> = BUILT.iter().map(|k| (*k).to_owned()).collect();
        for file in std::fs::read_dir(src).unwrap() {
            let path = file.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            // Every other piece between quotes is a string's content.
            for literal in text.split('"').skip(1).step_by(2) {
                let key_like = literal.contains('.')
                    && literal
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._".contains(c));
                let area = literal.split('.').next().unwrap_or_default();
                if key_like && areas.contains(area) {
                    keys.insert(literal.to_owned());
                }
            }
        }
        keys
    }

    #[test]
    fn every_key_the_code_uses_has_both_languages_and_none_is_left_over() {
        let json = include_str!(concat!(env!("OUT_DIR"), "/translations.json"));
        let translations = Translations::from_json(json).unwrap();
        assert_eq!(translations.locales(), ["en", "zh"]);
        let all: std::collections::HashMap<String, std::collections::HashMap<String, String>> =
            serde_json::from_str(json).unwrap();
        let (en, zh) = (&all["en"], &all["zh"]);
        let areas: BTreeSet<&str> = en.keys().filter_map(|k| k.split('.').next()).collect();

        let used = keys_in_the_sources(&areas);
        // Shaped like keys, but not keys: what the sidebar keeps its width and whether it is
        // collapsed under, and a file's name.
        let other = ["sidebar.width", "sidebar.collapsed", "appearance.toml"];
        let missing: Vec<&String> = used
            .iter()
            .filter(|k| !other.contains(&k.as_str()))
            .filter(|k| !en.contains_key(*k) || !zh.contains_key(*k))
            .collect();
        assert!(missing.is_empty(), "not in locales/app.yml: {missing:?}");

        let unused: Vec<&String> = en
            .keys()
            .chain(zh.keys())
            .filter(|k| !used.contains(*k))
            .collect();
        assert!(
            unused.is_empty(),
            "in locales/app.yml but not used: {unused:?}"
        );

        // A translation fills in what its English fills in.
        let slots = |text: &str| -> BTreeSet<String> {
            text.split("%{")
                .skip(1)
                .filter_map(|rest| rest.split('}').next())
                .map(str::to_owned)
                .collect()
        };
        for (key, english) in en {
            assert_eq!(slots(english), slots(&zh[key]), "the arguments of {key}");
        }
    }
}
