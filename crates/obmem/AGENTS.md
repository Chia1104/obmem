# obmem (binary)

The `obmem` CLI. It parses arguments with clap, calls obmem-core, obmem-mcp and obmem-agents, and prints the results. Any logic another front end could need (the MCP server, a future GUI) belongs in a library crate. This crate keeps only argument parsing, the calls and the output.

- This crate turns on the `clap` feature of obmem-core and obmem-agents.
- `init` and the session-start hook record the project in the machine's project list (`obmem projects`). A failure to record never fails the hook.
- `obmem scan --staged` reads staged blobs from the git index, not from the working tree, so it sees exactly what the commit will contain. It fails rather than skipping a file it cannot read.
