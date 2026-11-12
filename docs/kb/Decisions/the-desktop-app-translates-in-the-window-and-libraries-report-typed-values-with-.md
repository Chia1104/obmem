---
type: decision
title: The desktop app translates in the window, and libraries report typed values with English as Display
status: active
author: agent
confidence: high
created: 2026-11-12
updated: 2026-11-12
verified_commit: aa873c6
code_refs:
- apps/obmem-gui/src/words.rs#tr
- apps/obmem-gui/src/appearance.rs#Language
- apps/obmem-gui/build.rs#main
- crates/obmem-core/src/settings.rs#Rule
- crates/obmem-core/src/store.rs#Decided
- crates/obmem-agents/src/setup.rs#ManualStep
- crates/obmem-sync/src/run.rs#Outcome
tags:
- gui
- i18n
approved_at: 2026-11-12
approved_by: auto-approve
---
# The desktop app translates in the window, and libraries report typed values with English as Display

# The desktop app translates in the window, and libraries report typed values with English as Display

Decided on 2026-11-12, when `apps/obmem-gui` got English and Traditional Chinese (CHI-66) on Native Shell 0.5.0. Scope is in [[obmem-gui-scope-control-panel-for-macos-and-windows-notes-stay-in-obsidian]].

## Context

Native Shell 0.5.0 has rust-i18n translations (`locales/`, `t!`, `set_locale`). The window showed three kinds of text: its own, sentences obmem's libraries made as `String`s (a broken settings rule, what approving a note did, a manual setup step, the last sync), and errors.

## Decisions

**Libraries stay in English and report typed values.** What a library tells a person about work that went through is a value: `settings::Rule`, `secrets::Ignored`, `store::Decided`, `import::Skip`, `setup::ManualStep`, obmem-sync's `Outcome`, `Summary`, `Cleanup`. `Display` is the English the CLI and the MCP server print. `apps/obmem-gui/src/words.rs` says the same value in the window's language.

- Rejected: translations in the libraries. The CLI and the MCP server are English only, and agents read their output.
- Rejected: the window matching on English sentences. It breaks when a sentence changes.
- Consequence: the sync status file keeps numbers, not a sentence, so an obmem from before this cannot read a status written after it.

**Errors are not translated.** The reason an action failed is the library's `anyhow` text. Only the title over it is translated. Also left as they are: values that are data (a note's type and status, a scanner rule's description, paths) and the Agent view, which shows what an agent receives.

**Chinese is under the tag `zh`.** Native Shell finds a language by cutting a tag's last part off. macOS says `zh-Hant-TW`, Windows says `zh-TW`: `zh-Hant` misses Windows, `zh-TW` misses macOS, and only `zh` is found by both. Simplified Chinese systems get it too, which is what "Chinese or English" asks. `nsh.json`'s `locales` names `zh-Hant`, which is what a package declares to macOS, and picking Chinese in the app sets the locale `zh-Hant`.

**The choice is in `appearance.toml`** (`language = "system" | "en" | "zh"`), next to the mode and themes, and is picked on the Appearance screen. Rejected: Native Shell's `Storage`, which the sidebar uses, because a file under `OBMEM_CONFIG` is what the tests' sandbox and a person can both reach.

**Text on screen is a closure** (`tr!`), so it follows the language while it shows. Text said once (a toast, a notice) is translated when it is said. Rejected: letting each region rebuild on a language change, which works by accident of which screen is open and loses a form's typed text.

**A build by Cargo reads `locales/` itself.** `nsh run` and `nsh package` hand the translations to the root; `cargo run` and the tests do not. `build.rs` reads the folder with `nsh-i18n`'s `build` feature, the same reader the nsh CLI uses, and `obmem_gui::root()` gives them to the engine.

## Not checked on 2026-11-12

Windows, and `nsh run` / `nsh package` with the 0.5.0 CLI reading `locales/` and `nsh.json`'s `locales`.
