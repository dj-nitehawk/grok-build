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
| `openai` (`gpt-6-astra`, `gpt-5.6-sol`) | `grok chatgpt-login` → `~/.grok/chatgpt-auth.json` | `https://chatgpt.com/backend-api/codex` (POST `…/responses`) |

Codex is unofficial (Codex CLI OAuth client). `SamplingClient` stays URL-agnostic; shell `agent/chatgpt` owns extras, bearer, and headers. Do not send a Grok session token to chatgpt.com (`has_own_credentials` via synthetic `[auth_provider.chatgpt]`). Codex rejects `role: system` in Responses `input`; stamped configs lift that text into `instructions`.

ChatGPT stamping identifies existing resolver/header wrappers through default `auth_provider_name` trait hooks, preserving inner injectors without nesting repeated ChatGPT wrappers. Explicit `auth_provider = "chatgpt"` custom endpoints remain supported, but require HTTPS; insecure destinations receive no ChatGPT or seed bearer. ChatGPT credentials are disk-authoritative, not sampler-cached. Refresh, login publication, and logout coordinate through a stable sidecar file lock; token refresh reloads after locking and publishes only changed credentials through an owner-only temporary file and atomic replacement. Synchronous logout reports a retryable busy error while refresh holds the lock, rather than reporting a successful deletion. Hard-expired access tokens fail closed when refresh is unavailable. Token-helper JSON omits unknown expiry and never maps expired credentials to a synthetic one-hour lifetime. OAuth callbacks validate state before handling success or denial, and unrelated callbacks leave login pending. Device authorization polls for a code/verifier pair, then exchanges it at `/oauth/token`.

Auxiliary suggestions resolve the complete selected model configuration (endpoint, credentials, transport, capabilities), then copy session-local fields only. An unavailable default AI-suggest route falls back to the complete active configuration; unresolved explicit overrides are skipped. Custom provider capabilities are preserved by the Codex extras policy, while xAI-specific `no_inline_citations` remains restricted to trusted xAI routes.

## Provider-aware quota

Alt+Q selects via ACP `quotaProvider` metadata stamped from resolved backend/auth fields, not model names or the OpenAI family. Shell `agent/chatgpt/quota.rs` fetches unofficial GET `https://chatgpt.com/backend-api/wham/usage` with isolated ChatGPT OAuth/account headers and a 15-second total timeout; `x.ai/chatgpt/quota` is its extension adapter. Grok payment/billing types and `/usage` remain unchanged.

Pager `app/provider_quota.rs` owns ChatGPT snapshots, account identity and request generations separately from Grok billing. Both use the 60-second success TTL with coalescing, immediate error retry and no quota polling. ChatGPT ACP calls are bounded; failed refreshes retain snapshots with a visible stale/error label. Credential identity synchronization runs on dispatched actions and focus events, not rendering. Rendering selects the current provider; late completions update only their originating cache. Account changes invalidate ChatGPT snapshots and pending request identities. A single usable ChatGPT quota window omits the duration label, matching Grok; multiple windows retain duration labels.

## Security and auth (durable boundaries)

- Auth flows and credential providers live behind `xai-grok-auth`; do not log secret values (config TOML parse errors deliberately omit source snippets that may contain secrets). ChatGPT tokens are a separate file (`chatgpt-auth.json`), never mixed into `auth.json`.
- Sandbox profiles restrict child FS/network via kernel primitives; default is off. Trust-boundary files (`config.toml`, hooks, `sandbox.toml`, ...) stay kernel write-denied for the agent. User-initiated `~/.grok/config.toml` persist uses a helper forked before apply (socketpair, CLOEXEC so bash does not inherit it).
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
- `README.md`
- `crates/codegen/xai-grok-pager-bin/src/main.rs`
- `crates/codegen/xai-grok-shell/src/lib.rs`
- `crates/codegen/xai-grok-config/src/{loader,paths}.rs`
- `crates/codegen/xai-grok-mcp/Cargo.toml`
- `third_party/README.md`
