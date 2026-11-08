//! `obmem init`: create both vault skeletons and config, never overwriting existing files.

use crate::config::{Config, PROJECT_CONFIG_FILE, global_config_path};
use anyhow::Result;
use std::path::Path;

fn put(path: &Path, content: &str, created: &mut Vec<String>) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, content)?;
    created.push(path.display().to_string());
    Ok(())
}

const ALLOWLIST: &str = r#"# False positives for the write-time secret scanner.
# Each blocked write prints a `fingerprint=` value; paste it here to allow that exact string.
[allowlist]
fingerprints = []
# Regexes matched against the suspected secret value.
regexes = []
# Substrings that mark a value as harmless (case-insensitive).
stopwords = []
"#;

const RULES: &str = r#"# Extra secret rules, gitleaks format. You can paste the upstream gitleaks.toml here.
# [[rules]]
# id = "acme-internal-token"
# description = "ACME internal service token"
# regex = '''\b(acme_[A-Za-z0-9]{32})\b'''
# keywords = ["acme_"]
"#;

const REVIEW_BASE: &str = r#"filters:
  or:
    - status == "draft"
    - status == "open"
views:
  - type: table
    name: Review queue
    order:
      - file.name
      - type
      - status
      - author
      - confidence
      - updated
"#;

const FINDINGS_BASE: &str = r#"filters:
  and:
    - type == "finding"
views:
  - type: table
    name: Open findings
    filters:
      or:
        - status == "open"
        - status == "accepted"
    order:
      - file.name
      - severity
      - rule
      - target
      - status
      - updated
  - type: table
    name: Closed
    filters:
      or:
        - status == "fixed"
        - status == "rejected"
    order:
      - file.name
      - rule
      - target
      - status
      - updated
"#;

fn template(ty: &str, extra: &str) -> String {
    format!(
        "---\ntype: {ty}\ntitle: \"{{{{title}}}}\"\nstatus: active\nauthor: human\nconfidence: high\ncreated: {{{{date}}}}\ntags: []\n---\n# {{{{title}}}}\n\n{extra}"
    )
}

pub fn run(cfg: &Config) -> Result<Vec<String>> {
    let mut created = Vec::new();
    let pv = &cfg.project_vault;
    let gv = &cfg.global_vault;

    put(
        &cfg.project_root.join(PROJECT_CONFIG_FILE),
        &format!(
            "# obmem project config\nvault = \"{}\"\nname = \"{}\"\n\n[secrets]\nmode = \"block\"   # or \"redact\"\nenv_files = [\".env\", \".env.local\", \".env.development\", \".env.production\"]\nscan_process_env = true\n",
            pv.strip_prefix(&cfg.project_root).unwrap_or(pv).display(),
            cfg.project_name
        ),
        &mut created,
    )?;
    put(
        &global_config_path(),
        &format!("# obmem global config\nvault = \"{}\"\n", gv.display()),
        &mut created,
    )?;

    // Project vault (in repo, shared)
    put(
        &pv.join("Index.md"),
        &format!(
            "---\ntype: concept\ntitle: {name} index\nstatus: active\nauthor: human\n---\n# {name}\n\n\
             > Map of content. Loaded into every agent session — keep it short and current.\n\n\
             ## Architecture\n- \n\n## Key decisions\n- \n\n## Conventions & gotchas\n- \n\n## Runbooks\n- \n",
            name = cfg.project_name
        ),
        &mut created,
    )?;
    for d in ["Architecture", "Decisions", "Knowledge", "Runbooks", "Findings", "Inbox", "Archive"] {
        put(&pv.join(d).join(".gitkeep"), "", &mut created)?;
    }
    // Obsidian's own app state (workspace layout, plugin caches) is per person, not team knowledge.
    put(&pv.join(".gitignore"), "# Obsidian app state is per person\n.obsidian/\n", &mut created)?;
    put(&pv.join("_system/secret-allowlist.toml"), ALLOWLIST, &mut created)?;
    put(&pv.join("_system/secret-rules.toml"), RULES, &mut created)?;
    put(&pv.join("_system/Review.base"), REVIEW_BASE, &mut created)?;
    put(&pv.join("_system/Findings.base"), FINDINGS_BASE, &mut created)?;
    put(&pv.join(crate::audit::SUPPRESSIONS_FILE), crate::audit::SUPPRESSIONS_TEMPLATE, &mut created)?;
    put(
        &pv.join("Templates/decision.md"),
        &template("decision", "## Context\n\n## Decision\n\n## Alternatives considered\n\n## Consequences\n"),
        &mut created,
    )?;
    put(&pv.join("Templates/fact.md"), &template("fact", ""), &mut created)?;
    put(&pv.join("Templates/procedure.md"), &template("procedure", "## Steps\n1. \n"), &mut created)?;

    // Global vault (personal)
    for d in ["Global/Feedback", "Global/Preferences"] {
        put(&gv.join(d).join(".gitkeep"), "", &mut created)?;
    }
    put(&gv.join("_system/secret-allowlist.toml"), ALLOWLIST, &mut created)?;
    put(&gv.join("_system/Findings.base"), FINDINGS_BASE, &mut created)?;
    put(&gv.join(crate::audit::SUPPRESSIONS_FILE), crate::audit::SUPPRESSIONS_TEMPLATE, &mut created)?;
    put(&cfg.global_project_dir().join("Sessions/.gitkeep"), "", &mut created)?;
    Ok(created)
}

pub fn setup_snippets(bin: &str, cfg: &Config) -> String {
    let (pv, gv) = (cfg.project_vault.display(), cfg.global_vault.display());
    format!(
        r#"Next steps
──────────
1) Connect your agents (MCP server + lifecycle hooks, merged into the project's config files):
     {bin} setup claude            # .mcp.json, .claude/settings.json
     {bin} setup codex             # .codex/config.toml, .codex/hooks.json (trust the project in Codex)
     {bin} setup cursor            # .cursor/mcp.json, .cursor/hooks.json, AGENTS.md
     {bin} setup copilot           # .github/hooks/obmem.json, AGENTS.md (+ MCP in ~/.copilot)
     {bin} setup amp               # .amp/plugins/obmem.ts, AGENTS.md (+ amp mcp add --workspace)
   Any other MCP client: {{ "mcpServers": {{ "obmem": {{ "command": "obmem", "args": ["serve"] }} }} }}
2) Agents without hooks still get the usage guide as MCP server instructions; for agents
   that ignore those:  {bin} guide >> AGENTS.md

3) Required: obmem uses Obsidian itself (1.12+) for search, backlinks and link checks.
   - Obsidian → Settings → General → enable "Command line interface"
   - "Open folder as vault" for both folders (Obsidian cannot register vaults from the CLI):
       {pv}
       {gv}
   Check with `{bin} status`. If Obsidian is closed, obmem starts it in the background.
4) Optional: guard human commits to the vault too:
     git config core.hooksPath .githooks   # with a pre-commit running `{bin} scan --staged`
5) Review the vault when you want: `{bin} audit --write` (or ask the agent to run memory_audit);
   findings show up in _system/Findings.base and at session start.
"#
    )
}
