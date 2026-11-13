---
type: decision
title: The desktop app's themes are Native Shell theme files, built in or in the user's themes folder
status: active
author: agent
confidence: high
created: 2026-11-11
updated: 2026-11-11
verified_commit: 2952fc7
code_refs:
- apps/obmem-gui/src/appearance.rs#tokens
- apps/obmem-gui/src/appearance.rs#BUILT_IN
- apps/obmem-gui/src/state.rs#start_appearance
tags:
- gui
- theme
approved_at: 2026-11-11
approved_by: auto-approve
---
# The desktop app's themes are Native Shell theme files, built in or in the user's themes folder

Decided with the user on 2026-11-11, when solyx's theme customization was ported to `apps/obmem-gui`.

**Decision.**
- A theme is a Native Shell theme file: shadcn's `cssVars` with a `light` and a `dark` section, plus the app's own tokens `ok`, `warn`, `highlight`. obmem adds no token model of its own.
- The built-in themes are files too: `apps/obmem-gui/src/themes/*.json`, built into the binary. `ink` is the default (the former `theme.json`). Eleven others are solyx's palettes, converted once. `motoko` was drawn for obmem from a picture the user gave, and solyx does not have it.
- The user's themes are `.json` files in `~/.obmem/themes/`, same format. `~/.obmem/appearance.toml` keeps the mode (`system`, `light`, `dark`) and one theme id for each appearance.
- The window's tokens merge the two picks: each color has the light theme's light value and the dark theme's dark value. Radius and fonts have one value, so they come from the theme that shows.
- The app has no color editor. Duplicate copies a theme to the user's folder, and the user edits the file. Theme files are read again when the Appearance screen opens and when the window becomes active.

**Why.** The user asked to keep NSH's theme architecture, to port every solyx palette, and to use files, the built-in themes included. Files in shadcn's format mean a theme from tweakcn works unchanged.

**Alternatives considered.**
- solyx's own model (14 HeroUI tokens, a custom palette as `extends` plus overrides) mapped to shadcn tokens at run time. Rejected: a second token model beside NSH's.
- Keeping the choice in NSH `Storage`, as the sidebar width is. Rejected by the user in favor of files.
- Writing the built-in themes into the user's folder at first run. Rejected: a copy on disk goes stale when a release changes the theme.

**Consequences.**
- solyx's tokens were mapped by a one-off script; the files are the source of truth now. `card`=surface, `popover`=overlay, `primary`/`ring`=accent, `secondary`=default, `muted`=surfaceSecondary, `muted-foreground`=muted, `border`=separator, `input`=border. The sidebar differs by appearance: light uses surfaceTertiary with separator for `sidebar-accent` and `accent`; dark uses surface with surfaceTertiary. `fieldBackground` and `segment` have no place. The status colors are ink's in every theme.
- `appearance.toml` is read once, at start. A hand edit to it while the app runs is overwritten by the next pick.
- A duplicated file has its keys in alphabetical order: `serde_json` runs without `preserve_order`, and turning it on would change the CLI's JSON output in workspace builds.
- NSH has no API to read "the appearance shown" other than `dark()`, which the token effect reads.
