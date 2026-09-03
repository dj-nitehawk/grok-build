---
type: Playbook
title: Operations
description: Runtime modes, user data paths, env/config surfaces, and local process notes.
tags: [ops]
---

# Operations

## Deploy model

- **This fork:** prebuilt `grok` for linux/amd64 from [GitHub Releases](https://github.com/dj-nitehawk/grok-build/releases) (`grok-<tag>-linux-amd64.zip`); install steps in root `README.md`. Or build `xai-grok-pager` from source on `dev`.
- Stock SpaceXAI install scripts at x.ai/cli remain available for multi-platform upstream binaries (no fork customizations).
- This repository is not a full production deploy system for cloud services.
- Distribution builds may use profile `release-dist` (thin LTO, single CGU); musl targets enable RELRO/NX stack via `.cargo/config.toml`.
- **Fork CI release:** push a `v*` tag whose commit is on `dev` → `.github/workflows/release-linux-amd64.yml` builds `release-dist` for `x86_64-unknown-linux-gnu`, zips as `grok`, and attaches it to a GitHub Release (see [Workflows](workflows.md#github-release-linuxamd64)).

## Services and ports

| Process | How | Notes |
| --- | --- | --- |
| Interactive TUI | `grok` / `cargo run -p xai-grok-pager-bin` | Fullscreen/minimal UI modes |
| Headless agent | `grok -p …` | Exits after prompt; stdout formats plain/json/streaming-json |
| ACP stdio | `grok agent … stdio` | IDE/SDK embedding |
| ACP WebSocket | `grok agent … serve --bind <addr>` | Example bind `127.0.0.1:2419` in user guide; secret token for serve |
| Leader process | shell leader connect/spawn | Socket path derived from workspace URL helpers |
| Workspace server | `xai-workspace-server` | Host-local workspace RPC |

Sandbox profiles (`off`, `workspace`, `read-only`, `strict`, `devbox`, …) change FS/network rights for the agent process; default is off.

## Data stores

| Location | Contents |
| --- | --- |
| `$GROK_HOME` or `~/.grok` | User config, sessions, caches, marketplace cache |
| `~/.grok/config.toml` | Primary user config |
| `managed_config.toml` / `requirements.toml` | Managed and requirements layers (user and/or system dirs e.g. `/etc/grok`) |
| Session dirs | Under `grok_home/sessions/<encoded-cwd>/` |
| Project trees | Project rules, hooks, skills, local `.grok` content (distinct from user home) |

## Config and observability

Runtime setting precedence (user guide):

1. CLI flags  
2. Environment variables  
3. `config.toml`  
4. Managed / requirements config  
5. Built-in defaults  

Notable env var **names** (not values): `GROK_HOME`, `XAI_API_KEY`, `GROK_MEMORY`, `PROTOC`.

**MCP sampling tool list:** registered MCP tools are hidden from the model tool list by default (`search_tool` / `use_tool`). Per-server `promote_tools = ["bare_or_server__tool", ...]` on `[mcp_servers.*]` adds matching registered tools as first-class sampling defs (`prepare_tool_definitions_inner` → `session/mcp_promote.rs`). Empty/default keeps builtins-only.

Telemetry and feedback are user-configurable feature flags in config (`[features] telemetry`, `feedback`, etc.).

Tracing/OpenTelemetry crates exist in-tree for instrumentation; follow existing `xai-grok-telemetry` / fastrace usage when adding spans.

Auth: browser-based login on first launch for interactive use; security reports via HackerOne only.

## Caveats

- Org-managed config and macOS MDM preferences (`ai.x.grok`) can constrain user settings.
- Air-gapped use may disable remote fetch features; managed-config sync has separate switches in config docs.
- Windows path verbatim prefixes break tools if raw canonicalize is used.

## Sources
- `crates/codegen/xai-grok-pager/docs/user-guide/{05-configuration,07-mcp-servers,14-headless-mode,15-agent-mode,18-sandbox}.md`
- `crates/codegen/xai-grok-config/src/paths.rs`
- `crates/codegen/xai-grok-config-types/src/mcp.rs` (`promote_tools`)
- `crates/codegen/xai-grok-shell/src/session/mcp_promote.rs`
- `README.md`
- `.cargo/config.toml`
- `.github/workflows/release-linux-amd64.yml`
