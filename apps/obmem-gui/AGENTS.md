# obmem-gui

obmem's desktop control panel for macOS and Windows, built with Native Shell (NSH). It lists the projects obmem knows, shows each one's health and sets up agents. Notes are read and edited in Obsidian; the app deep-links there instead of rebuilding a note UI.

## Layout

- `data.rs`: everything the window reads or changes, through obmem-core and obmem-agents. All of it blocks.
- `state.rs`: the window's signals and the actions that change them. Views call actions; actions run `data` on a blocking thread and write the result back.
- `sidebar.rs`: the project list. It is resized by dragging its edge (`nsh_base::resize_handle`, as NSH's own dock does) and collapses to a rail of initials by its button or by dragging it narrower than `SNAP`; width and collapsed state are kept with `nsh_core::storage::Storage`. Below `widgets::NARROW` it stays a rail and the full list opens over the page as a drawer.
- `widgets.rs`: the shared pieces and the responsive rules: `page` (scrolls, caps the column width), `page_header` (actions wrap under the title), `tabs`, `choice`, `switch_control`, `checkbox_control`, `callout` (a card with an inset color bar, which notices use), `field` (label above the control when the page is narrow). Layout follows `window()`, which is a signal: a `style_when(move || …window()…)` restyles as the window resizes.
- `theme.json` (applied with `set_tokens` in `App`): the palette over NSH's shadcn default, plus the app's own tokens `ok`, `warn` and `highlight`, read with `widgets::tone`. `icons.rs`: inline SVG icons in `currentColor`.
- `project_page.rs` (header, tabs, Overview with the Claude Code import), `agent_tab.rs`, `sessions_tab.rs`, `settings_tab.rs`, `secrets_tab.rs`, `review_page.rs`, `personal_page.rs`: views only.
- `personal_page.rs` has the Sync card: `[sync]` settings (through `settings::save_sync`), connecting the device (the secret and passphrase fields are `masked`), the last sync status and Sync now, all through obmem-sync.
- The Agent view shows the session-start context and `memory_context` output as plain text, exactly as an agent receives them. Rendering them as Markdown would merge lines and show something no agent sees.
- `login_env.rs`: on macOS, the PATH from a login shell (see below).

Logic another front end could use belongs in obmem-core or obmem-agents, not here.

## NSH rules that bit

- After a failed save, do not reload the data the form was built from: the region rebuilds the form from the file and the user loses what they typed.
- A region re-renders whenever a signal it read changes. So each part of the state has its own signal, and a region that holds a form reads only what the form is built from. If it also read the notice, a failed save would rebuild the form and lose what the user typed. Seed a form's signals with untracked reads (`get_untracked`) for the same reason.

- A task started with `spawn` ends with its owner, and the button that starts one is usually re-rendered away when the task updates the page. Start tasks through `state::run_task` / `in_background`, which run them under the window's owner.
- `nsh_core::blocking` moves work off the engine thread. Every `data` call goes through it.
- Background loads must not change the screen. `state::select` picks a project quietly; only `state::open`, which the user triggers, switches to the project screen. Otherwise the project list, which loads after startup, pulls the user off Review or Personal settings.
- `ButtonElement` has no `test_id`. Wrap the button in a `view().test_id(..)` and select it with `id:… >> role:button`.
- Folder dialogs, revealing a path and opening URLs are NSH services (`pick_paths`, `reveal_path`, `open_url`); no extra crate is needed.
- `#[native_module] fn native` runs in the runner's `main` before the engine starts any thread. It is the only safe place for process setup such as changing the environment, and `main.rs` calls the same `prepare_process` first.
- Apps opened from Finder get launchd's short PATH, without `~/.cargo/bin` or `/usr/local/bin`, so they would not find `obmem` or the Obsidian CLI. `login_env` adopts a login shell's PATH on macOS.
- Prefer `nsh_base`'s controls over drawing one from views: `tabs`/`tab`, `toggle_group`/`toggle`, `switch`, `checkbox`, `button`, `input`, `resize_handle`. They bring the accessibility role, keyboard handling and native motion (the tab underline travels), and leave the look to the app: style their parts with the `*_class` props, which take Tailwind classes that name the theme's tokens (`bg-primary`, `checked:translate-x-4`). A slot prop is a builder method (`checkbox().indicator(..)`). Tests select them by role: `role:tab[…]`, `role:switch`, `role:checkbox`.
- `ButtonElement` has no `tooltip` either (only views do, through `Interactive`): `widgets::WithHint` wraps an element in a view with an `nsh_base::tooltip()`.
- The window has no system title bar (`set_title_bar(Hidden)` in `App`). Every column that reaches the top edge starts with `widgets::title_strip()`, which drags the window and leaves room for the window controls; the rail is at least as wide as `title_bar().left` so macOS's traffic lights fit over it.
- Headless tests default to a phone-sized viewport. Set `config.viewport` to a desktop size, or the page column collapses to zero width and its text vanishes from `screen()`.

## Commands

- `cargo test --workspace` runs the headless flow tests in `tests/gui/`. They point `OBMEM_CONFIG` (and, for the Claude Code import, `HOME`) at a temp folder, so the real project list, vaults and `~/.claude` are never touched. That environment is process-wide, so `common::sandbox` reruns each test alone in a child process of the same binary. Add a test as a module of `tests/gui/main.rs`, not as a new file in `tests/`: every test binary links all of Native Shell, about 200 MB.
- Prefer `--workspace` to `-p obmem-gui`. `rmcp` turns on serde and serde_json features only when obmem-mcp is in the build, so the two commands build Native Shell twice, under different hashes, and `target/` keeps both.
- `cargo run -p obmem-gui`: the window on this machine.
- `nsh run` / `nsh package --target macos|windows`: run these inside `apps/obmem-gui`. Output goes to `.nsh/` under the current directory, and runner builds use `apps/obmem-gui/target` (several GB).
- The nsh CLI builds against NSH crates of its own version. Upgrade it together with the `nsh-*` versions in the root `Cargo.toml`. NSH needs the latest stable Rust.
