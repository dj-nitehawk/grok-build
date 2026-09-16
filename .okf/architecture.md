---
type: Architecture
title: Architecture
description: Layering, crate boundaries, and invariants for the Grok Build agent stack.
tags: [architecture]
---

# Architecture

## Style

Multi-crate Rust workspace for a single product binary. Composition root is thin; domain logic lives in libraries. Async runtime (tokio), actor-style session/chat/sampling components, and DI-style trait seams for auth and host capabilities.

## Components

```text
xai-grok-pager-bin          # composition root → binary `xai-grok-pager` (shipped as `grok`)
        │
        ├── xai-grok-pager  # TUI: scrollback, prompt, modals, render, headless CLI surface
        │
        └── xai-grok-shell  # agent runtime: sessions, leader/stdio/headless, tools wiring
                │
                ├── xai-grok-agent       # agent defs, system prompt assembly
                ├── xai-grok-tools       # tool implementations + registry
                ├── xai-grok-workspace   # host FS, VCS, exec, trust, workspace server
                ├── xai-grok-sampler     # inference/streaming (no shell coupling)
                ├── xai-chat-state       # chat state actor
                ├── xai-workflow         # Rhai workflow orchestration
                ├── xai-grok-mcp         # MCP + OAuth credential isolation
                ├── xai-grok-sandbox     # OS sandbox (Landlock/Seatbelt)
                ├── xai-acp-lib          # ACP protocol helpers
                └── xai-grok-config      # grok_home, TOML layers, requirements
```

Supporting groups:

| Area | Location | Role |
| --- | --- | --- |
| Shared leaves | `crates/common/*` | tool protocol/runtime, tracing, compaction, computer-hub |
| Build helpers | `crates/build/xai-proto-build` | protoc discovery and tonic/prost build helpers |
| Proxy types | `prod/mc/cli-chat-proxy-types` | shared signed config / session type contracts with proxy |
| Vendored | `third_party/*` | Mermaid → SVG stack (untrusted model output path) |

## Dependency rules

- **Prefer leaf → mid → product**: types/config/http/auth under specialized crates; shell/pager compose them.
- **Do not edit root `Cargo.toml` by hand** for normal work; it is generated. Change per-crate `Cargo.toml` (and any monorepo generator upstream of this tree).
- **MCP isolation**: `xai-grok-mcp` quarantines `rmcp` + reqwest 0.13 while the rest of the workspace uses reqwest 0.12.
- **Auth**: use `xai-grok-auth` traits (`HttpAuth`, `AuthCredentialProvider`); avoid ad-hoc credential plumbing in tools.
- **Workspace types**: tools and remote paths should depend on `xai-grok-workspace-types` / client seams rather than pulling full host implementation when possible.
- **third_party/**: not first-party app code; re-vendor only with crate-local `VENDORING NOTES`.

## Communication

| Mode | Path |
| --- | --- |
| Interactive TUI | pager app loop + shell session |
| Headless | `run_headless` via shell agent app (`-p` / prompt flags) |
| ACP stdio / serve | shell agent + `xai-acp-lib` |
| Leader / multi-client | shell `leader` module; socket paths derived from workspace URL |
| Workspace server | `xai-workspace-server` binary in workspace crate |
| MCP | stdio/HTTP servers configured by user; credentials in MCP crate store |
| Inference | `xai-grok-sampler` + sampling types over HTTP streaming |

## Persistence

- User state under `$GROK_HOME` or `~/.grok` (config, sessions, caches, marketplace).
- Session directories keyed by encoded CWD under `sessions/`.
- Config files: `config.toml`, `managed_config.toml`, `requirements.toml` (user and system tiers such as `/etc/grok` where applicable).
- SQLite journal usage via `xai-sqlite-journal` for durable agent-side state where wired.
- Project rules / hooks / skills: project and user filesystem trees (see user guide); not all under this repo.

## Dual inference identity

| Family | Creds | Host |
| --- | --- | --- |
| `xai` (default) | `grok login` / `XAI_API_KEY` | cli-chat-proxy / api.x.ai |
| `openai` (`gpt-6-astra`, `gpt-6-sol`) | `grok chatgpt-login` → `~/.grok/chatgpt-auth.json` | `https://chatgpt.com/backend-api/codex` |

Codex is unofficial. `SamplingClient` stays URL-agnostic; shell `agent/chatgpt` owns bearer, headers, and Responses extras. Do not send a Grok session token to chatgpt.com. Codex rejects `role: system` in `input`; that text lifts into top-level `instructions`.

- Credentials are disk-authoritative (sidecar lock, atomic replace), not sampler-cached. Hard-expired access tokens fail closed when refresh is unavailable. Logout is retryable-busy while refresh holds the lock, not a false success.
- Explicit `auth_provider = "chatgpt"` requires HTTPS. Do not nest a second ChatGPT wrapper; stamp through `auth_provider_name`. Session and subagent reconstruction must pass that provider name into `stamp_reconstructed_sampler_config`, and only from a catalog row whose `base_url` is the live one (`matching_route_auth_provider`). A wire id that collides with a catalog key must not inherit that row's provider. A collided `NotByok` row is treated as `Unknown`, so the Grok session token stays off a different host. Bearer resolvers are not stored on the chat-state sampling config, and URL-only stamping drops them for any non-Codex host (including an HTTP proxy, which would otherwise keep a cached token).
- `no_inline_citations` stays on trusted xAI routes only.

Catalog merge, reasoning levels, OAuth port, and streaming reconcile: [Gotchas](gotchas.md). Paths: [Code Map](code-map.md).

## Provider-aware quota

Alt+Q selects via ACP `quotaProvider` from resolved backend/auth, not model names. Shell `agent/chatgpt/quota.rs` calls unofficial `wham/usage` with ChatGPT OAuth only; `x.ai/chatgpt/quota` is the adapter. Grok billing and `/usage` stay separate. Pager cache is app-owned (`app/provider_quota.rs`). Unknown providers must not fall back to Grok.

## Security and auth (durable boundaries)

- Auth flows and credential providers live behind `xai-grok-auth`; do not log secret values (config TOML parse errors deliberately omit source snippets that may contain secrets). ChatGPT tokens are a separate file (`chatgpt-auth.json`), never mixed into `auth.json`.
- Sandbox profiles restrict child FS/network via kernel primitives; default is off. Trust-boundary files (`config.toml`, hooks, `sandbox.toml`, ...) stay kernel write-denied for the agent. User-initiated `~/.grok/config.toml` persist uses a helper re-exec'd and detached before apply (socketpair, CLOEXEC so bash does not inherit it; not a multithreaded `fork`).
- Security reports: HackerOne only (`SECURITY.md`), not public issues.
- Config TOML may expand `$VAR` references; never commit real secrets into repo or OKF.

## Invariants

1. Product binary name in cargo is `xai-grok-pager`; installs rename/ship as `grok`.
2. Root workspace manifest is generated; treat as read-only in this tree.
3. Target packages with `-p <crate>`; full-workspace builds are intentionally slow.
4. Prefer `dunce::canonicalize` over raw std/tokio canonicalize (Windows verbatim path trap).
5. Vendored Mermaid stack is the only path for diagram SVG from model output; keep audit surface in `third_party/`.
6. External contributions are not part of the development model.

## Sources
- `crates/codegen/xai-grok-pager-bin/src/main.rs`
- `crates/codegen/xai-grok-shell/src/lib.rs`
- `crates/codegen/xai-grok-config/src/{loader,paths}.rs`
- `crates/codegen/xai-grok-mcp/Cargo.toml`
- `third_party/README.md`
