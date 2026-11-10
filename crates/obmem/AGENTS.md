# obmem (binary)

The `obmem` CLI. It parses arguments with clap, calls obmem-core, obmem-mcp and obmem-agents, and prints the results. Any logic another front end could need (the MCP server, a future GUI) belongs in a library crate. This crate keeps only argument parsing, the calls and the output.

- This crate turns on the `clap` feature of obmem-core and obmem-agents.
- `init` and the session-start hook record the project in the machine's project list (`obmem projects`). A failure to record never fails the hook.
- `obmem scan --staged` reads staged blobs from the git index, not from the working tree, so it sees exactly what the commit will contain. It fails rather than skipping a file it cannot read.
- After the session-start hook in an obmem project, and after a stop hook that lets the turn end (not one that asks the agent to log first), `spawn_background_sync` starts `obmem sync --background` as a detached process with null stdio (its own process group on Unix). On Windows it first marks the standard handles non-inheritable, so the agent does not wait on a pipe the sync still holds. The hook's own output never depends on the sync.
- With `[sync] ephemeral = true` the hooks call `sync_and_wait` instead: the session-start hook before it opens the store, so the context is read from the fetched vault, and the stop hook after printing its answer. A failed sync goes to stderr and the sync status, and never fails the hook.
