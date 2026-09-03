---
type: Reference
title: Gotchas
description: Non-obvious traps agents should not rediscover the hard way.
tags: [gotcha]
---

# Gotchas

- **Root `Cargo.toml` is generated.** Treat it as read-only; edit per-crate `Cargo.toml` files instead (`README.md`).
- **Always `-p <crate>`.** Full-workspace `cargo build` / `test` is slow and discouraged for routine work.
- **Binary name mismatch:** cargo produces `xai-grok-pager`; product/install name is `grok`.
- **DotSlash required for hermetic protoc.** Without `dotslash` on `PATH`, `bin/protoc` cannot download; builds needing protoc fail obscurely.
- **No raw canonicalize.** Use `dunce::canonicalize` (or tools `util::fs` helpers). Std/tokio canonicalize yields Windows `\\?\` paths that break git and path equality; clippy bans the raw APIs.
- **User home vs project `.grok`.** `$GROK_HOME`/`~/.grok` is user-global; project-local `.grok` is not a fallback for user home resolution.
- **Config secrets in parse errors.** Never log full TOML `Display` errors; use redacting helpers in `xai-grok-config`.
- **MCP reqwest skew.** MCP crate intentionally uses reqwest 0.13; do not “fix” by unifying versions without understanding the quarantine.
- **MCP tools are hidden from sampling by default.** Schemas stay behind `search_tool` / `use_tool` for KV-cache stability. Opt in with per-server `promote_tools` (bare or `server__tool` names); do not re-enable full MCP tool lists in the prepare path without a config allowlist.
- **third_party is vendored upstream source**, not app code. Re-apply `VENDORING NOTES` patches on upgrade; British `LICENCE` filenames are intentional.
- **External PRs are not accepted** (`CONTRIBUTING.md`). Do not design workflows around community contribution.
- **Sandbox default is off.** Tests or demos that assume confinement must set `--sandbox` / config explicitly.
- **Clippy config does not merge.** Nearest `clippy.toml` wins; codegen-oriented bans live at repo root for this tree.
- **release vs release-dist vs release-local.** Local `--release` is not the hardened dist profile; shipping uses `release-dist`. Local install script uses `release-local` (faster: no LTO, CGU=8, no debug).
- **SOURCE_REV** identifies monorepo provenance; it is not a crates.io version by itself.
- **Fork branch model:** `main` is a pure upstream mirror (`origin/main`); local customizations live only on `dev`. When `origin/main` advances, follow [Fork Sync](fork-sync.md) (ff `main`, rebase `dev`, force-with-lease push `dev` only after confirm). Do not put custom commits on `main`.
- **Always-keep fork files:** never take upstream (or auto-merged) content for `crates/codegen/xai-grok-agent/templates/prompt.md`, `subagent_prompt.md`, or root `README.md`. On every fork sync, restore all three from the pre-sync `dev` backup branch even if git reported no conflict. Details: [Fork Sync](fork-sync.md#always-keep-fork-files).
- **Prompt template update flow:** edit `templates/*.md` → `python3 scripts/encrypt_templates.py` (from `xai-grok-agent`) → fix `prompt::template` / `prompt::context` tests → fold into the existing `customize system prompts` commit. Do not hand-edit `src/prompt/prompt_encrypted.rs`. Full steps: [Workflows → System prompt templates](workflows.md#system-prompt-templates).
- **OKF ownership:** only the `setup okf` area commit may change `.okf/**` (and root `AGENTS.md` when it is the OKF gate). Feature commits stay product/code only; fold any docs updates into `setup okf` (fixup + autosquash). Mixing OKF into later areas causes fold/rebase conflicts on `workflows.md` and friends.
- **Fork features: thin switchboards.** New `dev` work should live in fork-owned modules with 1–2 registration touch points in shared upstream files. Do not add fields to multi-site structs or grow `effects/mod.rs` / `persistence.rs` with large bodies. Rules: [Conventions](conventions.md#fork-customizations-dev-only).
- **Fork overrides: filter, do not delete main’s assembly.** For UX/policy on values main still builds (e.g. hide `always-approve` on the prompt info line), leave hot paths like `agent_view/render.rs` matching main and filter in fork-owned code (`credit_bar::keep_info_line_mode_flag`, `PromptBorderChips`). Deleting upstream pushes/branches is a recurring rebase tax.
- **Sunset gix 0.86:** fork pins workspace `gix` above `main` until `main` is `>= 0.86` (gix-odb cleared-slot panic). Dedicated series area, last in the stack. On sync, drop the area when `main` catches up. Do not fold into slim. Details: [Fork Sync → Sunset: gix 0.86](fork-sync.md#sunset-gix-086-gix-odb-2723).

## Sources
- `README.md`
- `clippy.toml`
- `CONTRIBUTING.md`
- `crates/codegen/xai-grok-config/src/{loader,paths}.rs`
- `third_party/README.md`
- `Cargo.toml` profiles
- `.okf/fork-sync.md`
