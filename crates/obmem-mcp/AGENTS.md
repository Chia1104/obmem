# obmem-mcp

The stdio MCP server (`obmem serve`): tool definitions and finding the project. The logic behind each tool belongs in obmem-core; this is the only crate that depends on rmcp.

- The server instructions are `obmem_core::guide::GUIDE`, the same text that the session-start context and the AGENTS.md block carry. Edit the guide there.
- Agents have fewer powers than the CLI. Through MCP they cannot reject findings and cannot verify human-authored notes.

## Finding the project

Some hosts start the server outside the project. For example, T3 Code's Cursor driver starts it in the home directory, does not interpolate `${workspaceFolder}`, and offers no MCP roots. The server tries these sources in order:
1. The cwd or `--project`. An uninterpolated `--project ${…}` falls back to the cwd.
2. MCP roots, asked once, when the client supports them.
3. The `project` argument that every tool accepts.

Rules:
- If the resolved project has no `.obmem.toml`, refuse project-vault writes and say why. Never write to a guessed vault such as `~/docs/kb`.
- `project` binds a server only when it has no project yet, and the path must contain `.obmem.toml`. Once bound, a server never switches projects (paths are canonicalized before comparing), so an agent that has been misled cannot redirect writes into another project's vault.
