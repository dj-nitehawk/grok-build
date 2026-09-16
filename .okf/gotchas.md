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
- **Config precedence:** CLI, direct env, requirements/MDM clamps, config overlay, user config, managed config, defaults. File-tier merge and overlay details: [Operations](operations.md#config-and-observability).
- **Responses SSE heartbeats:** `xai-grok-sampler/src/client.rs` filters `keepalive` (SSE event name or JSON `type`) before async-openai decoding. Heartbeats must not terminate the stream or count as model output; unrelated unknown events and malformed response payloads remain errors. Regression: `cargo test -p xai-grok-sampler --lib responses_stream_skips_keepalive_and_preserves_errors`.
- **Credential telemetry:** `SamplingClient::post` and subagent model-resolution logs report key presence only. Never log bearer or API-key prefixes, including for short tokens; `post_never_logs_credential_fragments` captures emitted tracing fields with synthetic credentials. A pinned Codex subagent must retain the ChatGPT bearer resolver attached by `sampling_config_for_model`; the session-token resolver applies only when no provider resolver exists. ChatGPT provider refresh and 401 recovery match the live route as well as the model ID to avoid minting a ChatGPT token for a colliding wire ID.
- **Responses tool-call reconciliation:** Match argument frames to output items by item ID when available. On `response.incomplete`, a newly recovered call requires a terminal item or arguments-done frame before tool execution, while snapshot calls preserve existing length-policy behavior. Regressions: `cargo test -p xai-grok-sampler --lib stream::responses::tests`. Recovered SSE maps are bounded by model output and frame count, except content-index gaps (bounded to 64).
- **MCP reqwest skew.** MCP crate intentionally uses reqwest 0.13; do not “fix” by unifying versions without understanding the quarantine.
- **MCP tools are hidden from sampling by default.** Schemas stay behind `search_tool` / `use_tool` for KV-cache stability. Opt in with per-server `promote_tools` (bare or `server__tool` names). Project `.grok/config.toml` lists count only when folder trust allows project scope (`project_scope_allowed`); an untrusted repo must not promote a user-scoped MCP tool. Do not re-enable full MCP tool lists in the prepare path without a config allowlist.
- **third_party is vendored upstream source**, not app code. Re-apply `VENDORING NOTES` patches on upgrade; British `LICENCE` filenames are intentional.
- **External PRs are not accepted** (`CONTRIBUTING.md`). Do not design workflows around community contribution.
- **Sandbox persistence:** default sandbox is off. Under write-deny profiles, user settings persist through the privileged helper while agent config writes stay denied; folder trust stays session-only. Preserve startup ordering, pipe-transferred token authentication, and symlink containment. Security constraints and retirement gate: [Fork Sync](fork-sync.md#sunset-sandbox-settings-persist).
- **`/purge` does not follow symlinks.** `clear_directory_contents` refuses a symlinked `sessions/` or `logs/` directory and unlinks symlink entries without following them. Do not switch that sweep back to `exists` / `is_dir` / `remove_dir_all` on the raw path.
- **Clippy config does not merge.** Nearest `clippy.toml` wins; codegen-oriented bans live at repo root for this tree.
- **release vs release-dist vs release-local.** Local `--release` is not the hardened dist profile; shipping uses `release-dist`. Local install script uses `release-local` (faster: no LTO, CGU=8, no debug).
- **SOURCE_REV** identifies monorepo provenance; it is not a crates.io version by itself.
- **Fork branch model:** `main` is a pure upstream mirror (`origin/main`); local customizations live only on `dev`. When `origin/main` advances, follow [Fork Sync](fork-sync.md) (ff `main`, rebase `dev`, force-with-lease push `dev` only after confirm). Do not put custom commits on `main`.
- **Always-keep fork files:** never take upstream (or auto-merged) content for `crates/codegen/xai-grok-agent/templates/prompt.md`, `subagent_prompt.md`, or root `README.md`. On every fork sync, restore all three from the pre-sync `dev` backup branch even if git reported no conflict. Details: [Fork Sync](fork-sync.md#always-keep-fork-files).
- **Prompt template update flow:** edit `templates/*.md` → `python3 scripts/encrypt_templates.py` (from `xai-grok-agent`) → fix `prompt::template` / `prompt::context` tests → fold into the existing `customize system prompts` commit. Do not hand-edit `src/prompt/prompt_encrypted.rs`. Full steps: [Workflows → System prompt templates](workflows.md#system-prompt-templates).
- **Fork isolation and OKF ownership:** feature bodies live in fork-owned modules with thin upstream registration. Fold `.okf/**` and OKF-related `AGENTS.md` updates into `setup okf`. Full rules: [Conventions](conventions.md#fork-customizations-dev-only).
- **ChatGPT Codex:** credentials and system-message conversion follow [Architecture](architecture.md#dual-inference-identity). Codex rejects unsupported `include` values and reasoning `none`; keep allowlists in `agent/chatgpt/extras.rs` and `catalog.rs`. OAuth callback port 1455 can conflict with Codex/OpenCode (`--device` fallback). Run `chatgpt::merge_catalog` after remote prefetch replaces defaults so Codex models remain in `/model`. Seeded config overrides use bare catalog keys (`[model.gpt-6-astra]`, `[model.gpt-6-sol]`).
- **ChatGPT / quota tests:** use current manifest defaults and declared test seams, not retired capability features. Filters and commands: [Testing](testing.md#chatgpt-regression-checks).
- **ChatGPT quota:** `wham/usage` is an unofficial subscription endpoint, not OpenAI API billing. Never reuse Grok auth or log response bodies. Optional windows do not imply zero usage. Alt+Q and prompt rendering both use `quotaProvider` metadata; unknown providers must not fall back to Grok. ChatGPT cache is app-owned and identity/request-scoped, separate from agent-local Grok balance and `/usage`. Account identity is read from the local ChatGPT credential store, matching the local-shell login flow.
- **ChatGPT streaming:** completed snapshots can be partial or empty after streamed text/tool frames. Reconcile item identities, content indexes, and call IDs before flattening; keep `MAX_RECONCILED_CONTENT_PARTS` bounds. Global concatenation or all-or-nothing tool fallback loses calls or duplicates text. Session/subagent reconstruction retains route-matched provider identity and credentials; see [Architecture](architecture.md#dual-inference-identity) and [Testing](testing.md#chatgpt-regression-checks).
- **Sunset gix 0.86:** fork pins workspace `gix` above `main` until `main` is `>= 0.86` (gix-odb cleared-slot panic). Dedicated series area. On sync, drop the area when `main` catches up. Keep it separate from unrelated areas. Details: [Fork Sync → Sunset: gix 0.86](fork-sync.md#sunset-gix-086-gix-odb-2723).

## Sources
- [Fork Sync](fork-sync.md), [Architecture](architecture.md), [Testing](testing.md) (detailed boundaries and checks)
- `crates/codegen/xai-grok-config/src/{loader,paths}.rs`
- `crates/codegen/xai-grok-shell/src/agent/chatgpt/`
- `crates/codegen/xai-grok-sampler/src/{client,stream/responses}.rs`
- `clippy.toml`, `third_party/README.md`
