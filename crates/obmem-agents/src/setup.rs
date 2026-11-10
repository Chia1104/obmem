//! `obmem setup <agent>`: register the MCP server and the lifecycle hooks in an agent's
//! project-level config. Existing files are merged, never overwritten, and an entry that is
//! already there (anything mentioning `obmem serve` / `obmem hook`) is left alone, so running
//! it again is safe. Commands use `obmem` from PATH so the files can be committed.

use crate::hooks::Agent;
use anyhow::{Context, Result};
use obmem_core::config::Config;
use serde_json::{Value, json};
use std::path::Path;

pub struct Report {
    /// Files `setup` wrote; for `check`, the files it would write (missing or outdated entries).
    pub changed: Vec<String>,
    pub unchanged: Vec<String>,
    /// Steps only the user can do (trust prompts, app settings).
    pub manual: Vec<String>,
}

impl Report {
    fn new() -> Self {
        Self {
            changed: vec![],
            unchanged: vec![],
            manual: vec![],
        }
    }
    fn note(&mut self, path: &Path, changed: bool) {
        let p = path.display().to_string();
        if changed {
            self.changed.push(p)
        } else {
            self.unchanged.push(p)
        }
    }
}

/// The version of the `obmem` that agents run: every command `setup` writes calls `obmem` from
/// PATH. `None` when it is not on PATH.
pub fn cli_version() -> Option<String> {
    let out = std::process::Command::new("obmem")
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    out.status
        .success()
        .then(|| text.trim().trim_start_matches("obmem").trim().to_string())
}

const CODEX_CONFIG: &str = ".codex/config.toml";
const CODEX_APPROVAL: &str = "default_tools_approval_mode";

/// Whether Codex runs obmem's tools without asking (`default_tools_approval_mode = "approve"` on
/// the project's `[mcp_servers.obmem]`). `None` when obmem is not set up for Codex. Codex asks
/// before every MCP tool call by default, and `codex exec` cannot ask, so it cannot use obmem
/// without this.
pub fn codex_auto_approve(cfg: &Config) -> Result<Option<bool>> {
    let doc = codex_config(cfg)?;
    let Some(server) = doc.get("mcp_servers").and_then(|t| t.get("obmem")) else {
        return Ok(None);
    };
    Ok(Some(
        server.get(CODEX_APPROVAL).and_then(|v| v.as_str()) == Some("approve"),
    ))
}

/// Turns Codex's approval of obmem's tools on or off for this project.
pub fn set_codex_auto_approve(cfg: &Config, on: bool) -> Result<()> {
    let path = cfg.project_root.join(CODEX_CONFIG);
    let mut doc = codex_config(cfg)?;
    let Some(server) = doc
        .get_mut("mcp_servers")
        .and_then(|t| t.get_mut("obmem"))
        .and_then(|t| t.as_table_like_mut())
    else {
        anyhow::bail!(
            "obmem is not set up for Codex in this project; run `obmem setup codex` first"
        );
    };
    if on {
        server.insert(CODEX_APPROVAL, toml_edit::value("approve"));
    } else {
        server.remove(CODEX_APPROVAL);
    }
    Writer {
        root: &cfg.project_root,
        apply: true,
    }
    .write(&path, &doc.to_string())
}

fn codex_config(cfg: &Config) -> Result<toml_edit::DocumentMut> {
    let path = cfg.project_root.join(CODEX_CONFIG);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    text.parse()
        .with_context(|| format!("parsing {}", path.display()))
}

fn mentions(v: &Value, needle: &str) -> bool {
    v.to_string().contains(needle)
}

/// Read a JSON object (or start one), let `edit` change it, write it back if it changed.
fn merge_json(path: &Path, w: &Writer, edit: impl FnOnce(&mut Value) -> bool) -> Result<bool> {
    let mut v: Value = if path.exists() {
        serde_json::from_str(&std::fs::read_to_string(path)?)
            .with_context(|| format!("parsing {}", path.display()))?
    } else {
        json!({})
    };
    if !v.is_object() {
        anyhow::bail!(
            "{} is not a JSON object; add the obmem entries by hand",
            path.display()
        );
    }
    if !edit(&mut v) {
        return Ok(false);
    }
    if !w.apply {
        return Ok(true);
    }
    w.write(path, &(serde_json::to_string_pretty(&v)? + "\n"))?;
    Ok(true)
}

/// `{"<key>": {"obmem": {...}}}`: `mcpServers` in Claude Code's `.mcp.json` and Cursor's
/// `.cursor/mcp.json`, `amp.mcpServers` in Amp's `.amp/settings.json`.
fn add_mcp_server(path: &Path, w: &Writer, key: &str, args: &[&str]) -> Result<bool> {
    merge_json(path, w, |v| {
        let servers = v
            .as_object_mut()
            .unwrap()
            .entry(key)
            .or_insert_with(|| json!({}));
        if mentions(servers, "obmem") {
            return false;
        }
        servers["obmem"] = json!({ "command": "obmem", "args": args });
        true
    })
}

/// Claude Code / Codex shape: `hooks.<Event>[] = {matcher?, hooks: [{type, command}]}`.
fn add_nested_hooks(path: &Path, w: &Writer, events: &[(&str, String)]) -> Result<bool> {
    merge_json(path, w, |v| {
        let hooks = v
            .as_object_mut()
            .unwrap()
            .entry("hooks")
            .or_insert_with(|| json!({}));
        let mut changed = false;
        for (event, command) in events {
            let list = hooks
                .as_object_mut()
                .unwrap()
                .entry(*event)
                .or_insert_with(|| json!([]));
            if mentions(list, "obmem hook") {
                continue;
            }
            list.as_array_mut()
                .unwrap()
                .push(json!({ "hooks": [{ "type": "command", "command": command }] }));
            changed = true;
        }
        changed
    })
}

/// Cursor / Copilot shape: `{"version": 1, "hooks": {"<event>": [{...entry}]}}`.
fn add_flat_hooks(path: &Path, w: &Writer, events: &[(&str, Value)]) -> Result<bool> {
    merge_json(path, w, |v| {
        let obj = v.as_object_mut().unwrap();
        obj.entry("version").or_insert(json!(1));
        let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
        let mut changed = false;
        for (event, entry) in events {
            let list = hooks
                .as_object_mut()
                .unwrap()
                .entry(*event)
                .or_insert_with(|| json!([]));
            if mentions(list, "obmem hook") {
                continue;
            }
            list.as_array_mut().unwrap().push(entry.clone());
            changed = true;
        }
        changed
    })
}

/// Append a block to a text file unless `marker` is already in it.
fn append_once(path: &Path, w: &Writer, marker: &str, block: &str) -> Result<bool> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if existing.contains(marker) {
        return Ok(false);
    }
    if !w.apply {
        return Ok(true);
    }
    let sep = if existing.is_empty() || existing.ends_with("\n\n") {
        ""
    } else if existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    w.write(path, &format!("{existing}{sep}{block}"))?;
    Ok(true)
}

const AGENTS_MARKER: &str = "<!-- obmem:guide -->";
const AGENTS_END: &str = "<!-- /obmem:guide -->";

/// Add the guide block to AGENTS.md, or refresh it in place when the guide changed. Text outside
/// the markers is never touched.
fn upsert_guide(path: &Path, w: &Writer) -> Result<bool> {
    let block = agents_md_block();
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if let (Some(start), Some(end)) = (existing.find(AGENTS_MARKER), existing.find(AGENTS_END))
        && end > start
    {
        let end = end
            + AGENTS_END.len()
            + usize::from(existing[end + AGENTS_END.len()..].starts_with('\n'));
        let updated = format!("{}{block}{}", &existing[..start], &existing[end..]);
        if updated == existing {
            return Ok(false);
        }
        if w.apply {
            w.write(path, &updated)?;
        }
        return Ok(true);
    }
    append_once(path, w, AGENTS_MARKER, &block)
}

fn agents_md_block() -> String {
    format!(
        "{AGENTS_MARKER}\n## Project memory (obmem)\n\n{}\n{AGENTS_END}\n",
        obmem_core::guide::GUIDE
    )
}

const AMP_PLUGIN: &str = r#"// obmem: project memory for Amp. Generated by `obmem setup amp`; safe to edit.
// At the first turn of a thread it adds the memory context; when a turn ends after real work that
// is not logged yet, it asks the agent (once, with a marker so it cannot loop) to log it.
import type { PluginAPI } from '@ampcode/plugin'

const MARKER = '[obmem]'

export default function (amp: PluginAPI) {
	const started = new Set<string>()

	// A hook's answer. What it says on stderr (a failed sync, say) goes to the plugin log:
	// on a remote machine nothing else shows it.
	const answer = (out: { stdout: string; stderr: string }) => {
		const problem = out.stderr.trim()
		if (problem) amp.logger.log(problem)
		return out.stdout.trim()
	}

	amp.on('agent.start', async (event: any, ctx: any) => {
		const thread = String(event.thread?.id ?? ctx?.thread?.id ?? 'default')
		if (started.has(thread)) return
		started.add(thread)
		const text = answer(await amp.$`obmem hook session-start --agent amp --session ${thread}`)
		if (!text) return
		return { message: { content: text, display: false } }
	})

	amp.on('agent.end', async (event: any, ctx: any) => {
		const thread = String(event.thread?.id ?? ctx?.thread?.id ?? 'default')
		// Every turn end runs the hook, also the turn we started ourselves: the hook syncs the
		// personal vault, and that turn is the one in which the agent wrote its log.
		const reason = answer(await amp.$`obmem hook stop --agent amp --session ${thread}`)
		// a turn we started ourselves carries the marker: never chain another one
		if (!reason || String(event.message ?? '').includes(MARKER)) return
		return { action: 'continue', userMessage: `${MARKER} ${reason}` }
	})
}
"#;

/// Register obmem for `agent` in the project: write what is missing, keep everything else.
pub fn setup(cfg: &Config, agent: Agent) -> Result<Report> {
    run(
        cfg,
        agent,
        &Writer {
            root: &cfg.project_root,
            apply: true,
        },
    )
}

/// What `setup` would do, without writing anything: an empty `changed` means the agent is set up.
pub fn check(cfg: &Config, agent: Agent) -> Result<Report> {
    run(
        cfg,
        agent,
        &Writer {
            root: &cfg.project_root,
            apply: false,
        },
    )
}

/// Where setup writes and whether it may. The files are in the project, which may be an untrusted
/// clone: a config file planted as a symlink must not redirect a write outside it (to the user's
/// own `~/.codex/config.toml`, say), so every write goes through `atomic_write`.
struct Writer<'a> {
    root: &'a Path,
    apply: bool,
}

impl Writer<'_> {
    fn write(&self, path: &Path, content: &str) -> Result<()> {
        obmem_core::store::atomic_write(self.root, path, content)
    }
}

fn run(cfg: &Config, agent: Agent, w: &Writer) -> Result<Report> {
    let root = &cfg.project_root;
    let mut r = Report::new();
    let cmd = |event: &str| match agent {
        Agent::Claude => format!("obmem hook {event}"),
        _ => format!("obmem hook {event} --agent {}", agent.id()),
    };
    match agent {
        Agent::Claude => {
            let p = root.join(".mcp.json");
            r.note(&p, add_mcp_server(&p, w, "mcpServers", &["serve"])?);
            let p = root.join(".claude/settings.json");
            let events = [
                ("SessionStart", cmd("session-start")),
                ("Stop", cmd("stop")),
                ("PreCompact", cmd("pre-compact")),
            ];
            r.note(&p, add_nested_hooks(&p, w, &events)?);
            r.manual.push("Start a new Claude Code session in this project and approve the `obmem` MCP server.".into());
        }
        Agent::Codex => {
            let p = root.join(CODEX_CONFIG);
            let block = "[mcp_servers.obmem]\ncommand = \"obmem\"\nargs = [\"serve\"]\n";
            r.note(&p, append_once(&p, w, "[mcp_servers.obmem]", block)?);
            // Codex has SessionStart (also after compaction) and Stop; no PreCompact.
            let p = root.join(".codex/hooks.json");
            r.note(
                &p,
                add_nested_hooks(
                    &p,
                    w,
                    &[
                        ("SessionStart", cmd("session-start")),
                        ("Stop", cmd("stop")),
                    ],
                )?,
            );
            r.manual.push(format!(
                "Codex loads project config only for trusted projects: trust {} when Codex asks (or set \
                 trust_level = \"trusted\" for it in ~/.codex/config.toml), then review and trust the two new hooks.",
                root.display()
            ));
        }
        Agent::Cursor => {
            // Some Cursor hosts (T3 Code's driver) start MCP servers in the home directory, so pin
            // the project; obmem ignores the placeholder if a host does not interpolate it.
            let p = root.join(".cursor/mcp.json");
            let args = ["serve", "--project", "${workspaceFolder}"];
            r.note(&p, add_mcp_server(&p, w, "mcpServers", &args)?);
            let p = root.join(".cursor/hooks.json");
            let events = [
                ("sessionStart", json!({ "command": cmd("session-start") })),
                ("stop", json!({ "command": cmd("stop") })),
                ("preCompact", json!({ "command": cmd("pre-compact") })),
            ];
            r.note(&p, add_flat_hooks(&p, w, &events)?);
            // sessionStart context injection is unreliable in some Cursor builds; the guide in
            // AGENTS.md (which Cursor reads) covers that case.
            let p = root.join("AGENTS.md");
            r.note(&p, upsert_guide(&p, w)?);
            r.manual
                .push("In Cursor: Settings → MCP, enable `obmem` for this project.".into());
        }
        Agent::Copilot => {
            let p = root.join(".github/hooks/obmem.json");
            let entry =
                |event: &str| json!({ "type": "command", "bash": cmd(event), "timeoutSec": 30 });
            let events = [
                ("sessionStart", entry("session-start")),
                ("agentStop", entry("stop")),
                ("preCompact", entry("pre-compact")),
            ];
            r.note(&p, add_flat_hooks(&p, w, &events)?);
            let p = root.join("AGENTS.md");
            r.note(&p, upsert_guide(&p, w)?);
            r.manual.push(
                "Register the MCP server for Copilot CLI: add {\"mcpServers\": {\"obmem\": {\"type\": \"local\", \
                 \"command\": \"obmem\", \"args\": [\"serve\"], \"tools\": [\"*\"]}}} to ~/.copilot/mcp-config.json \
                 (or run /mcp add inside copilot). Repository hooks need the workspace to be trusted."
                    .into(),
            );
        }
        Agent::Amp => {
            // The workspace settings, not the user's: an orb (Amp's remote machine) has the
            // repository and nothing from the user's own settings file.
            let p = root.join(".amp/settings.json");
            r.note(&p, add_mcp_server(&p, w, "amp.mcpServers", &["serve"])?);
            let p = root.join(".amp/plugins/obmem.ts");
            let changed = !p.exists();
            if changed && w.apply {
                w.write(&p, AMP_PLUGIN)?;
            }
            r.note(&p, changed);
            let p = root.join("AGENTS.md");
            r.note(&p, upsert_guide(&p, w)?);
            r.manual.push(format!(
                "Amp runs a workspace MCP server only after you approve it: cd {} && amp mcp approve obmem \
                 (in an orb, Amp asks in the thread).",
                root.display()
            ));
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(root: &Path) -> Config {
        Config {
            project_root: root.to_path_buf(),
            project_name: "t".into(),
            project_vault: root.join("docs/kb"),
            global_vault: root.join("g"),
            secrets: Default::default(),
            audit: Default::default(),
            auto_approve: false,
            device: "test".into(),
        }
    }

    #[test]
    fn merges_without_clobbering_and_is_idempotent() {
        let root = std::env::temp_dir().join(format!("obmem-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".cursor")).unwrap();
        std::fs::write(
            root.join(".cursor/hooks.json"),
            r#"{"version":1,"hooks":{"afterFileEdit":[{"command":"./fmt.sh"}]}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join(".cursor/mcp.json"),
            r#"{"mcpServers":{"other":{"command":"x"}}}"#,
        )
        .unwrap();
        let c = cfg(&root);

        let planned = check(&c, Agent::Cursor).unwrap();
        assert_eq!(planned.changed.len(), 3, "{:?}", planned.changed);
        assert!(!root.join("AGENTS.md").exists(), "check writes nothing");

        assert_eq!(codex_auto_approve(&c).unwrap(), None);
        assert!(set_codex_auto_approve(&c, true).is_err());

        let first = setup(&c, Agent::Cursor).unwrap();
        assert_eq!(first.changed.len(), 3, "{:?}", first.changed); // mcp.json, hooks.json, AGENTS.md
        let hooks: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".cursor/hooks.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(hooks["hooks"]["afterFileEdit"][0]["command"], "./fmt.sh"); // kept
        assert_eq!(
            hooks["hooks"]["stop"][0]["command"],
            "obmem hook stop --agent cursor"
        );
        let mcp: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".cursor/mcp.json")).unwrap())
                .unwrap();
        assert!(mcp["mcpServers"]["other"].is_object() && mcp["mcpServers"]["obmem"].is_object());

        let second = setup(&c, Agent::Cursor).unwrap();
        assert!(second.changed.is_empty(), "{:?}", second.changed);
        assert!(check(&c, Agent::Cursor).unwrap().changed.is_empty());

        setup(&c, Agent::Codex).unwrap();
        let toml = std::fs::read_to_string(root.join(".codex/config.toml")).unwrap();
        assert_eq!(toml.matches("[mcp_servers.obmem]").count(), 1);
        let codex: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".codex/hooks.json")).unwrap())
                .unwrap();
        assert_eq!(
            codex["hooks"]["Stop"][0]["hooks"][0]["command"],
            "obmem hook stop --agent codex"
        );
        assert!(setup(&c, Agent::Codex).unwrap().changed.is_empty());
        // Auto-approval is off until switched on, and switching keeps the rest of the file.
        assert_eq!(codex_auto_approve(&c).unwrap(), Some(false));
        set_codex_auto_approve(&c, true).unwrap();
        assert_eq!(codex_auto_approve(&c).unwrap(), Some(true));
        assert!(
            std::fs::read_to_string(root.join(CODEX_CONFIG))
                .unwrap()
                .contains("args = [\"serve\"]")
        );
        set_codex_auto_approve(&c, false).unwrap();
        assert_eq!(codex_auto_approve(&c).unwrap(), Some(false));
        assert!(check(&c, Agent::Codex).unwrap().changed.is_empty());
        // AGENTS.md got the guide once, even though two agents asked for it
        setup(&c, Agent::Amp).unwrap();
        let agents = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert_eq!(agents.matches(AGENTS_MARKER).count(), 1);
        // an outdated guide block is refreshed in place; surrounding text survives
        let stale =
            format!("# Team notes\n\n{AGENTS_MARKER}\nold guide\n{AGENTS_END}\n\nkeep me\n");
        std::fs::write(root.join("AGENTS.md"), stale).unwrap();
        let w = Writer {
            root: &root,
            apply: true,
        };
        assert!(upsert_guide(&root.join("AGENTS.md"), &w).unwrap());
        let fresh = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert!(
            fresh.starts_with("# Team notes")
                && fresh.ends_with("keep me\n")
                && !fresh.contains("old guide"),
            "{fresh}"
        );
        assert!(!upsert_guide(&root.join("AGENTS.md"), &w).unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn amp_gets_the_mcp_server_in_the_workspace_settings() {
        let root = std::env::temp_dir().join(format!("obmem-setup-amp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".amp")).unwrap();
        let theirs = r#"{"amp.mcpServers":{"other":{"command":"x"}},"amp.tools.disable":["y"]}"#;
        std::fs::write(root.join(".amp/settings.json"), theirs).unwrap();
        let c = cfg(&root);

        let first = setup(&c, Agent::Amp).unwrap();
        assert_eq!(first.changed.len(), 3, "{:?}", first.changed); // settings.json, obmem.ts, AGENTS.md
        let settings: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".amp/settings.json")).unwrap(),
        )
        .unwrap();
        // The shape `amp mcp add obmem --workspace -- obmem serve` writes (amp 2026-11-08).
        assert_eq!(
            settings["amp.mcpServers"]["obmem"],
            json!({ "command": "obmem", "args": ["serve"] })
        );
        assert!(settings["amp.mcpServers"]["other"].is_object());
        assert_eq!(settings["amp.tools.disable"], json!(["y"]));
        // The turn the plugin starts itself still runs the stop hook, which syncs the log.
        let plugin = std::fs::read_to_string(root.join(".amp/plugins/obmem.ts")).unwrap();
        let (hook, guard) = (
            plugin.find("obmem hook stop").unwrap(),
            plugin.find("includes(MARKER)").unwrap(),
        );
        assert!(hook < guard, "{plugin}");

        assert!(setup(&c, Agent::Amp).unwrap().changed.is_empty());
        assert!(check(&c, Agent::Amp).unwrap().changed.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn never_writes_through_a_planted_symlink() {
        let root = std::env::temp_dir().join(format!("obmem-setup-link-{}", std::process::id()));
        let outside =
            std::env::temp_dir().join(format!("obmem-setup-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        // A clone ships .codex/config.toml as a link to the user's own Codex config.
        let theirs = "[mcp_servers.obmem]\ncommand = \"obmem\"\n";
        std::fs::write(outside.join("config.toml"), theirs).unwrap();
        std::os::unix::fs::symlink(outside.join("config.toml"), root.join(".codex/config.toml"))
            .unwrap();
        // And .cursor as a link to a folder outside the project.
        std::os::unix::fs::symlink(&outside, root.join(".cursor")).unwrap();
        let c = cfg(&root);

        assert!(set_codex_auto_approve(&c, true).is_err());
        assert!(setup(&c, Agent::Cursor).is_err());
        assert_eq!(
            std::fs::read_to_string(outside.join("config.toml")).unwrap(),
            theirs
        );
        assert!(!outside.join("mcp.json").exists() && !outside.join("hooks.json").exists());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
