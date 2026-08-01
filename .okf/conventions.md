---
type: Reference
title: Conventions
description: Coding, path, config, and design conventions for this Rust workspace.
tags: [conventions]
---

# Conventions

## Naming

- Crates: `xai-grok-*` for product family; `xai-*` for shared utilities; `prod-mc-*` path for proxy types package.
- Binary ship name: `grok` externally; cargo bin `xai-grok-pager`.
- Prefer explicit module names matching domain (`session`, `leader`, `registry`, `implementations`).

## Style

- Edition: workspace `2024` (`Cargo.toml`).
- Format: `rustfmt.toml` + `cargo fmt --all`.
- Lint: root `clippy.toml` (codegen crates note nearest-file clippy behavior; disallowed canonicalize methods).
- Field init shorthand enabled in rustfmt.
- Prefer small, focused changes that match existing module structure over large reorganizations.

## Errors and validation

- Widespread `anyhow` / `thiserror` usage; follow local crate patterns.
- Config TOML parse errors must not echo offending source lines (may contain secrets); use span-based detail helpers in `xai-grok-config`.
- Fail closed on path/trust/sandbox containment checks when paths cannot be safely compared (especially long Windows verbatim paths).

## Paths and filesystem

- **Always prefer `dunce::canonicalize`** over `std::fs::canonicalize`, `Path::canonicalize`, or `tokio::fs::canonicalize`.
- In async tools code, use blessed helpers under `xai_grok_tools::util::fs` when present.
- Clippy disallows raw canonicalize methods for this reason (Windows `\\?\` verbatim paths).

## APIs and data

- Config layering: user guide documents runtime precedence (CLI → env → `config.toml` → managed/requirements → defaults). File merge and requirements enforcement live in `xai-grok-config` / validation modules.
- Env expansion: `$VAR` in config TOML via loader.
- Auth: implement against `xai-grok-auth` traits; do not hardcode credential storage in unrelated crates.
- Tool surface: register through tools crate taxonomy/registry; headless allow/deny lists apply to built-ins.

## Config and DI

- User home: `$GROK_HOME` or `~/.grok` via `xai_grok_config::paths::grok_home`.
- Do not confuse project-local `.grok/` trees with user-global grok home (`user_grok_home` vs project dirs).
- Prefer workspace dependency versions from `[workspace.dependencies]`; per-crate `path` deps for local crates.
- Feature flags: follow existing `default-features = false` patterns (e.g. sandbox, voice) when adding optional heavy deps.

## YAGNI / simplicity

- This tree is a published slice of a larger monorepo; avoid inventing parallel build systems or rewriting the workspace graph.
- Do not re-vendor third_party crates without following each crate's vendoring notes.

## Fork customizations (`dev` only)

This fork rebases onto `origin/main` regularly. **Minimize the surface of shared upstream files** so future syncs stay cheap. Full branch/sync procedure: [Fork Sync](fork-sync.md).

A “minimal diff” in a hot upstream file is still a bad merge surface if it rewrites assembly main owns. Prefer leaving main’s construction alone and applying fork policy in fork-owned code.

When adding or changing product customizations on `dev`:

1. **New files for feature bodies.** Prefer a dedicated module (for example `dispatch/session/handoff.rs`, `effects/purge.rs`, `session/purge.rs`, `slash/commands/*.rs`, `extensions/*.rs`). Do not grow large inline blocks inside hot upstream modules.
2. **Thin switchboards only.** Shared “catalog / router” files get the smallest possible registration: one `mod` line, one catalog entry, one match arm, one import. Avoid rewriting surrounding upstream logic in the same edit.
3. **Do not fork-extend shared structs.** Extra fields on types constructed in many upstream sites (for example `PromptInfo`) force fan-out and recurring conflicts. Prefer existing extension points (`PromptFlag`, helper modules, app-owned state) or a private wrapper in a fork-owned file.
4. **Own the behavior, call from 1–2 sites.** Keep billing/quota/UI helpers in fork-owned modules (`views/credit_bar.rs`, dispatch helpers); leave `event_loop`, `auth`, `queue`, etc. with comment-only or single-line changes when possible.
5. **Override by filter/transform, not by deleting main’s assembly.** If main builds a value (mode flags, chips, labels, effects list), keep that construction matching main. Suppress, reorder, or restyle in fork-owned helpers so syncs can take main’s side cleanly.
   - **Bad:** delete the `always-approve` push in `agent_view/render.rs` (or rewrite its mode-flag ladder) to quiet the prompt info line.
   - **Good:** leave the push; drop it in `credit_bar` via `keep_info_line_mode_flag` / `PromptBorderChips::with_mode_flags`. Minimal mode may still *build* the same candidate set as main, then apply the shared filter.
6. **Always-keep prompts stay binary-custom.** Never “refresh” `templates/prompt.md` / `subagent_prompt.md` from `main` to shrink the diff. See [Fork Sync always-keep](fork-sync.md#always-keep-fork-prompt-templates).
7. **Topical commits.** One customization intent per commit so rebase stops stay reviewable; do not mix unrelated switchboard churn into feature bodies.
8. **No plugin framework just for isolation.** Prefer the patterns above over inventing a generalization layer unless product needs it.

### Finish gate (required on `dev` product work)

1. Skim `git diff main --stat` (or the customization commit’s own files). Hotspots: `agent_view/render.rs`, `prompt_widget`, `event_loop`, large `dispatch/*`, `effects/mod.rs`, `persistence.rs`.
2. For each shared-file hunk: is it registration/wiring only, or did you change policy/UX/control flow main will keep editing? Policy belongs in a fork-owned module; re-home before finishing.
3. If a shared file grew more than registration noise, extract. Do not ship “quick” deletions of upstream branches that a filter could have handled.

## Sources
- `rustfmt.toml`, `clippy.toml`, `Cargo.toml`
- `crates/codegen/xai-grok-config/src/{loader,paths}.rs`
- `README.md` Development section
- Pager user guide `05-configuration.md`
- [Fork Sync](fork-sync.md) (branch model, always-keep prompts, sync checklist)
