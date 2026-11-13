//! How well `memory_context` finds the notes a task needs, measured on this repo's own project
//! vault (`docs/kb`) with the built-in search, so the numbers are the same on every machine.
//!
//! Each case is a task the way a person or an agent would put it, and the notes a session doing
//! that task should see. A note counts as found when it is among the matches, as near the top
//! when it is one of the first three, and as reachable when it is a match or listed one link away
//! from the matches. The vault is small, so "found" is easy: "near the top" tells more about the
//! ranking.
//!
//! Run `cargo test -p obmem-core --test recall -- --nocapture` for the table. Change the ranking
//! and the table says whether it got better. The test fails when a case names a note that is
//! gone (update the case), or when recall falls below the floors at the bottom. The floors are
//! a little under what was measured, so that a new note in the vault does not fail the build.

use obmem_core::config::Config;
use obmem_core::context::recall;
use obmem_core::obsidian::Backend;
use obmem_core::store::Store;
use std::path::PathBuf;

/// What `memory_context` asks for by default.
const LIMIT: usize = 8;

struct Case {
    task: &'static str,
    expect: &'static [&'static str],
}

const WORKTREE: &str = "obmem-in-a-git-worktree-cannot-reach-obsidian";
const HEADLESS: &str = "a-headless-backend-answers-for-obsidian-where-the-app-is-not-installed";
const OBSIDIAN_CLI: &str =
    "use-the-obsidian-cli-for-search-and-link-checks-instead-of-an-own-index";
const CLI_BEHAVIOUR: &str = "obsidian-cli-behaviour";
const RELEASING: &str = "releasing-a-version-bump-last-then-tag-the-merged-commit";
const NPM_SCOPE: &str = "npm-packages-are-published-under-the-chia1104-scope";
const NPM_404: &str = "a-new-npm-package-name-answers-404-for-minutes-after-its-first-publish";
const TOKEN_FIXTURES: &str = "token-shaped-test-fixtures-must-be-split-with-concat";
const RUSTFMT: &str = "rustfmt-check-exits-0-on-stdin-even-when-it-prints-a-diff";
const RETENTION: &str =
    "sync-retention-keeps-a-week-then-dailies-then-weeklies-blobs-are-collected-after";
const SYNC: &str = "personal-vault-backup-and-sync-are-built-into-obmem-over-user-provided-s3-r2";
const REMOTE_SYNC: &str =
    "remote-machines-sync-with-credentials-from-the-environment-and-an-ephemeral-sett";
const TRANSLATES: &str =
    "the-desktop-app-translates-in-the-window-and-libraries-report-typed-values-with-";
const GUI_SCOPE: &str =
    "obmem-gui-scope-control-panel-for-macos-and-windows-notes-stay-in-obsidian";
const UPDATE: &str =
    "obmem-update-replaces-only-standalone-installs-by-running-the-release-s-install-";
const TARGET: &str =
    "target-grows-because-p-obmem-gui-and-workspace-build-native-shell-with-different";
const CURSOR_HOME: &str = "t3-code-s-cursor-driver-starts-mcp-servers-in-the-home-directory";
const CURSOR_HOOKS: &str = "cursor-hook-payloads-and-agent-transcript-format-seen-2026-11-09";
const KEY_ALLOWLIST: &str =
    "a-private-key-s-allowlist-fingerprint-covers-the-whole-key-not-its-begin-line";
const ENV_FILES: &str = "the-secret-scanner-never-looks-up-a-value-from-an-env-file-on-disk";
const PRIVATE_VAULT: &str =
    "a-project-vault-can-be-private-outside-the-repo-named-by-the-user-s-config";
const UNLOGGED: &str = "detect-unlogged-work-from-git-for-agents-other-than-claude-code";
const GUIDANCE: &str =
    "agent-guidance-is-a-short-always-loaded-guide-plus-skills-that-setup-installs";
const WORKSPACE: &str = "split-obmem-into-a-cargo-workspace-so-a-gui-can-reuse-the-core";
const AGENT_LOOP: &str =
    "the-calling-agent-is-the-retrieval-loop-memory-tools-say-where-to-search-next-an";

/// Tasks in the notes' language.
const ENGLISH: &[Case] = &[
    Case {
        task: "memory search fails when the agent runs in a git worktree",
        expect: &[WORKTREE, HEADLESS, CLI_BEHAVIOUR],
    },
    Case {
        task: "call another Obsidian CLI command to list the tags of a vault",
        expect: &[CLI_BEHAVIOUR, OBSIDIAN_CLI],
    },
    Case {
        task: "search returns nothing in a cloud sandbox that has no Obsidian app",
        expect: &[HEADLESS, OBSIDIAN_CLI],
    },
    Case {
        task: "publish the next version to npm",
        expect: &[RELEASING, NPM_SCOPE, NPM_404],
    },
    Case {
        task: "git push was rejected because a test holds a fake GitHub token",
        expect: &[TOKEN_FIXTURES],
    },
    Case {
        task: "the pre-commit hook lets unformatted Rust files through",
        expect: &[RUSTFMT],
    },
    Case {
        task: "change how long old backups are kept before prune deletes them",
        expect: &[RETENTION, SYNC],
    },
    Case {
        task: "run sync on a CI server that has no keychain",
        expect: &[REMOTE_SYNC, SYNC],
    },
    Case {
        task: "add a French translation to the desktop app",
        expect: &[TRANSLATES, GUI_SCOPE],
    },
    Case {
        task: "build a note editor into the GUI",
        expect: &[GUI_SCOPE],
    },
    Case {
        task: "make obmem update itself when it was installed with npm",
        expect: &[UPDATE, NPM_SCOPE],
    },
    Case {
        task: "the target directory takes too much disk space",
        expect: &[TARGET],
    },
    Case {
        task: "Cursor cannot find the project when it starts the MCP server",
        expect: &[CURSOR_HOME],
    },
    Case {
        task: "read the agent transcript in a Cursor stop hook",
        expect: &[CURSOR_HOOKS, UNLOGGED],
    },
    Case {
        task: "allowlist an SSH private key that the secret scanner reports",
        expect: &[KEY_ALLOWLIST],
    },
    Case {
        task: "the scanner should treat values from .env as secrets",
        expect: &[ENV_FILES],
    },
    Case {
        task: "keep a team's notes out of the repository and share them another way",
        expect: &[PRIVATE_VAULT, SYNC],
    },
    Case {
        task: "tell agents more about memory at the start of every session",
        expect: &[GUIDANCE],
    },
    Case {
        task: "move the sync code into the core crate",
        expect: &[WORKSPACE, SYNC],
    },
];

/// Tasks put as questions, or with a short word in them. Their common words (`does`, `not`) are
/// in most notes, and a short word is inside longer ones (`rag` in `storage`, `app` in `happen`).
const LOOSE: &[Case] = &[
    Case {
        task: "why does search not work in a git worktree",
        expect: &[WORKTREE],
    },
    Case {
        task: "why does the stop hook not see the work that Codex did",
        expect: &[UNLOGGED],
    },
    Case {
        task: "what should the desktop app not do",
        expect: &[GUI_SCOPE],
    },
    Case {
        task: "how does a new machine get the notes of the team",
        expect: &[PRIVATE_VAULT],
    },
    Case {
        task: "should obmem use RAG with embeddings to find notes",
        expect: &[AGENT_LOOP],
    },
    Case {
        task: "which words can go in a log line of the app",
        expect: &[TRANSLATES],
    },
    Case {
        task: "where does the key of a vault go when sync is set up",
        expect: &[SYNC],
    },
];

/// The same kind of task in Chinese. The notes are in English and matching is literal, so only
/// the words a task leaves in English can find anything.
const CHINESE: &[Case] = &[
    Case {
        task: "在 git worktree 裡記憶搜尋失敗",
        expect: &[WORKTREE, HEADLESS],
    },
    Case {
        task: "把下一個版本發佈到 npm",
        expect: &[RELEASING, NPM_SCOPE],
    },
    Case {
        task: "調整舊備份保留多久才刪除",
        expect: &[RETENTION],
    },
    Case {
        task: "幫桌面應用程式加上法文翻譯",
        expect: &[TRANSLATES],
    },
    Case {
        task: "把掃描器誤報的私鑰加入允許清單",
        expect: &[KEY_ALLOWLIST],
    },
    Case {
        task: "沒有安裝 Obsidian 的雲端機器搜尋不到東西",
        expect: &[HEADLESS, OBSIDIAN_CLI],
    },
];

#[derive(Default)]
struct Tally {
    expected: usize,
    found: usize,
    top: usize,
    reachable: usize,
    /// Cases that missed a note without naming a part of the task that no note contains.
    silent_misses: usize,
    /// Notes listed one link away, over all cases.
    linked: usize,
}

impl Tally {
    fn share(part: usize, whole: usize) -> f64 {
        part as f64 / whole.max(1) as f64
    }
    fn found(&self) -> f64 {
        Self::share(self.found, self.expected)
    }
    fn top(&self) -> f64 {
        Self::share(self.top, self.expected)
    }
    fn reachable(&self) -> f64 {
        Self::share(self.reachable, self.expected)
    }
}

fn measure(store: &mut Store, name: &str, cases: &[Case]) -> Tally {
    let mut tally = Tally::default();
    println!("\n{name}");
    for case in cases {
        let r = recall(store, case.task, LIMIT).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let matched = |id: &str| r.notes.iter().any(|m| m.hit.id == id);
        let linked = |id: &str| r.linked.iter().any(|l| l.id == id);
        let marks: Vec<String> = case
            .expect
            .iter()
            .map(|id| {
                let mark = match r.notes.iter().position(|m| m.hit.id == *id) {
                    Some(at) => format!("#{}", at + 1),
                    None if linked(id) => "linked".into(),
                    None => "MISSED".into(),
                };
                format!("{mark} {}", id.chars().take(28).collect::<String>())
            })
            .collect();
        println!("  {}\n      {}", case.task, marks.join(" | "));
        // what outranks an expected note that is not near the top
        let last = case
            .expect
            .iter()
            .filter_map(|id| r.notes.iter().position(|m| m.hit.id == *id))
            .max();
        if let Some(last) = last.filter(|at| *at >= 3) {
            for (at, m) in r.notes.iter().enumerate().take(last + 1) {
                println!(
                    "        #{} {} ({})",
                    at + 1,
                    m.hit.id.chars().take(40).collect::<String>(),
                    m.terms.join(", ")
                );
            }
        }
        if case.expect.iter().any(|id| !matched(id) && !linked(id)) {
            println!("      said no note contains: {}", r.unmatched.join(", "));
            tally.silent_misses += usize::from(r.unmatched.is_empty());
        }
        tally.expected += case.expect.len();
        tally.found += case.expect.iter().filter(|id| matched(id)).count();
        tally.top += case
            .expect
            .iter()
            .filter(|id| r.notes.iter().take(3).any(|m| m.hit.id == **id))
            .count();
        tally.reachable += case
            .expect
            .iter()
            .filter(|id| matched(id) || linked(id))
            .count();
        tally.linked += r.linked.len();
    }
    println!(
        "  => found {}/{} ({:.0}%), near the top {}/{} ({:.0}%), reachable {}/{} ({:.0}%), \
         {:.1} linked notes listed per task",
        tally.found,
        tally.expected,
        100.0 * tally.found(),
        tally.top,
        tally.expected,
        100.0 * tally.top(),
        tally.reachable,
        tally.expected,
        100.0 * tally.reachable(),
        Tally::share(tally.linked, cases.len())
    );
    tally
}

#[test]
fn recall_on_this_repos_vault() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let vault = root.join("docs/kb");
    // no personal vault: a machine's own notes would change the numbers
    let global = std::env::temp_dir().join(format!("obmem-recall-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&global);
    std::fs::create_dir_all(&global).unwrap();
    let cfg = Config {
        project_root: root,
        project_name: "obmem".into(),
        project_vault: vault,
        global_vault: global.clone(),
        secrets: Default::default(),
        audit: Default::default(),
        auto_approve: false,
        device: "test".into(),
    };
    let mut store = Store::open_with(cfg, Backend::Headless).unwrap();

    for case in ENGLISH.iter().chain(LOOSE).chain(CHINESE) {
        for id in case.expect {
            assert!(
                store.get(id).is_ok(),
                "the case \"{}\" expects [[{id}]], which is not in docs/kb any more: update the case",
                case.task
            );
        }
    }

    let english = measure(&mut store, "English tasks", ENGLISH);
    let loose = measure(&mut store, "Questions and short words", LOOSE);
    let chinese = measure(&mut store, "Chinese tasks", CHINESE);
    let _ = std::fs::remove_dir_all(&global);

    // Measured on 2026-11-14: English 97% found, 81% near the top, 100% reachable; questions and
    // short words 86% near the top; Chinese 56% found, 67% reachable. Raise a floor when a change
    // lifts the number.
    assert!(english.found() >= 0.90, "English tasks, found");
    assert!(english.top() >= 0.75, "English tasks, near the top");
    assert!(english.reachable() >= 0.95, "English tasks, reachable");
    assert!(
        loose.top() >= 0.70,
        "questions and short words, near the top"
    );
    assert!(chinese.reachable() >= 0.55, "Chinese tasks, reachable");
    // A task that finds nothing must say which of its words no note has: that is what tells an
    // agent to search again in other words.
    assert_eq!(
        english.silent_misses + loose.silent_misses + chinese.silent_misses,
        0,
        "missed notes without saying what no note contains"
    );
}
