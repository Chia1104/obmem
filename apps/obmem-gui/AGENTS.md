# obmem-gui

obmem's desktop control panel for macOS and Windows, built with Native Shell (NSH). It lists the projects obmem knows, shows each one's health and sets up agents. Notes are read and edited in Obsidian; the app deep-links there instead of rebuilding a note UI.

## Layout

- `data.rs`: everything the window reads or changes, through obmem-core and obmem-agents. All of it blocks.
- `state.rs`: the window's signals and the actions that change them. Views call actions; actions run `data` on a blocking thread and write the result back.
- `sidebar.rs`, `project_page.rs` (header, tabs, Overview), `settings_tab.rs`, `secrets_tab.rs`, `personal_page.rs`, `widgets.rs`: views only.
- `login_env.rs`: on macOS, the PATH from a login shell (see below).

Logic another front end could use belongs in obmem-core or obmem-agents, not here.

## NSH rules that bit

- A region re-renders whenever a signal it read changes. So each part of the state has its own signal, and a region that holds a form reads only what the form is built from. If it also read the notice, a failed save would rebuild the form and lose what the user typed. Seed a form's signals with untracked reads (`get_untracked`) for the same reason.

- A task started with `spawn` ends with its owner, and the button that starts one is usually re-rendered away when the task updates the page. Start tasks through `state::run_task` / `in_background`, which run them under the window's owner.
- `nsh_core::blocking` moves work off the engine thread. Every `data` call goes through it.
- Folder dialogs, revealing a path and opening URLs are NSH services (`pick_paths`, `reveal_path`, `open_url`); no extra crate is needed.
- `#[native_module] fn native` runs in the runner's `main` before the engine starts any thread. It is the only safe place for process setup such as changing the environment, and `main.rs` calls the same `prepare_process` first.
- Apps opened from Finder get launchd's short PATH, without `~/.cargo/bin` or `/usr/local/bin`, so they would not find `obmem` or the Obsidian CLI. `login_env` adopts a login shell's PATH on macOS.
- Headless tests default to a phone-sized viewport. Set `config.viewport` to a desktop size, or the page column collapses to zero width and its text vanishes from `screen()`.

## Commands

- `cargo test -p obmem-gui`: headless flow tests. They point `OBMEM_CONFIG` at a temp folder, so the real project list and vaults are never touched.
- `cargo run -p obmem-gui`: the window on this machine.
- `nsh run` / `nsh package --target macos|windows`: run these inside `apps/obmem-gui`. Output goes to `.nsh/` under the current directory, and runner builds use `apps/obmem-gui/target` (several GB).
- The nsh CLI builds against NSH crates of its own version. Upgrade it together with the `nsh-*` versions in the root `Cargo.toml`. NSH needs the latest stable Rust.
