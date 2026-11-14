---
type: decision
title: The desktop app's window stands on the desktop or on one picture of the user's, and the page's opacity is a setting
status: active
author: agent
confidence: high
created: 2026-11-14
updated: 2026-11-14
verified_commit: 9627e8b
code_refs:
- apps/obmem-gui/src/lib.rs#behind
- apps/obmem-gui/src/appearance.rs#keep_background
- apps/obmem-gui/src/appearance.rs#PICTURES
- apps/obmem-gui/src/state.rs#panel_cover
tags:
- gui
- appearance
approved_at: 2026-11-14
approved_by: auto-approve
---
# The desktop app's window stands on the desktop or on one picture of the user's, and the page's opacity is a setting

Decided with the user on 2026-11-14, in `apps/obmem-gui`. Related: [[the-desktop-app-s-themes-are-native-shell-theme-files-built-in-or-in-the-user-s-]], [[obmem-gui-scope-control-panel-for-macos-and-windows-notes-stay-in-obsidian]].

## Context

The user wanted the sidebar to look like macOS's own: a bit see-through, with the desktop behind it. Then they wanted to put a picture of their own behind the window's content.

## Decision

- The window is the theme's sidebar color as a tint (50%) over what is behind it. Behind it is the picture the user chose, on macOS and Windows. Without a picture it is macOS's sidebar material, so the desktop. On Windows without a picture the color is plain.
- **One picture for both appearances.** The user chose this over one per appearance. The tint comes from the theme, which already differs by appearance.
- **The picture stays on the device.** The user chose not to sync it with the personal vault: the look is this device's, like the themes and `appearance.toml`.
- **How much a page covers of what is behind it is the user's setting** (`panel`, in percent, never under 50). I had proposed a page that is always opaque. The user asked for a setting instead, and then for a blur of what the page lets through: under 100% the page blurs it, by a fixed amount.
- **A page may have no panel at all** (`show_panel`, a switch). The user asked for it after trying 50%. A page's cards then stand right on what is behind them, and the opacity setting waits.
- obmem keeps its own copy of the picture, in `backgrounds/` beside the themes folder, one at a time. `appearance.toml` names it. A theme file does not: it stays in the format tweakcn exports.
- **Pictures are PNG, JPEG and AVIF.** PNG and JPEG are drawn by macOS and Windows themselves. The user asked for AVIF: macOS 13 and later draws it (seen on macOS 27.3 on 2026-11-14), and Windows draws it only where its AV1 extension is installed, which was not checked.

## Alternatives not taken

- A choice of three (none, desktop, picture). There is only "a picture or not": macOS's "Reduce transparency" already serves a user who wants no see-through.
- A setting for the tint's strength, and a blur of the picture behind the sidebar. Not asked for. The picture shows as it is there.

## Consequences

- The macOS material is the root view's (`.macos_backdrop`), not the window's (`WindowAttributes::backdrop`). In Native Shell 0.5.0 the window's material keeps the computer's appearance, so a dark obmem on a light computer stood on a light material. Checked on 2026-11-14 with NSH 0.5.0 on macOS 27.3.
- The material stays under the picture, so a picture that does not load leaves the material, not a hole in the window.
- Without a panel, a page's title and the line under it stand on the tinted picture, and a busy picture makes them hard to read. That is the user's choice to make.
- Not checked on Windows as of 2026-11-14.
