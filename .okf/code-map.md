---
type: Reference
title: Code Map
description: Top-level layout and where to add TUI, agent, tool, and workspace behavior.
tags: [layout]
resource: crates/codegen/
---

# Code Map

## Layout

| Path | Contents |
| --- | --- |
| `crates/codegen/` | Primary product crates (~60): pager, shell, tools, workspace, config, MCP, etc. |
| `crates/common/` | Shared leaf crates (tool protocol/runtime, tracing, compaction, computer-hub) |
| `crates/build/xai-proto-build` | protoc location + protobuf build helpers |
| `prod/mc/cli-chat-proxy-types` | Types shared with cli-chat-proxy (config/session/metrics contracts) |
| `third_party/` | Vendored Mermaid layout/SVG stack |
| `bin/protoc` | DotSlash hermetic protoc launcher |
| `.cargo/config.toml` | Per-target rustflags, jemalloc page env |
| `target/` | Cargo build output (gitignored) |
| `SOURCE_REV` | Upstream monorepo commit SHA for this tree |
| `Cargo.toml` / `Cargo.lock` | Generated workspace root + lockfile |

## Modules (product path)

| Concern | Crate / path |
| --- | --- |
| Binary entry | `crates/codegen/xai-grok-pager-bin/src/main.rs` |
| TUI app, scrollback, settings UI | `crates/codegen/xai-grok-pager/src/` (`app`, `scrollback`, `settings`, `input`, …) |
| Pager render | `crates/codegen/xai-grok-pager-render` |
| Agent runtime entry modes | `crates/codegen/xai-grok-shell/src/agent/`, `leader/`, `session/` |
| Tool implementations | `crates/codegen/xai-grok-tools/src/implementations/` |
| Tool registry / taxonomy | `crates/codegen/xai-grok-tools/src/{registry,tool_taxonomy,types}` |
| Host workspace | `crates/codegen/xai-grok-workspace/src/` |
| Agent definitions / prompts | `crates/codegen/xai-grok-agent/src/` (editable templates: `templates/{prompt,subagent_prompt,apply_patch_prompt}.md`) |
| Parent `spawn_subagent` tool text | Shared builder: `xai-tool-types::build_task_description_with_detail` (`Full` default; parent CLI uses `Concise`). On-demand depth: user-guide `16-subagents.md` |
| Config load + paths | `crates/codegen/xai-grok-config/src/` |
| Config value types | `crates/codegen/xai-grok-config-types` |
| Sampling / inference | `crates/codegen/xai-grok-sampler`, `xai-grok-sampling-types` |
| Chat state | `crates/codegen/xai-chat-state` |
| Workflows (Rhai) | `crates/codegen/xai-workflow` |
| MCP | `crates/codegen/xai-grok-mcp` |
| Sandbox | `crates/codegen/xai-grok-sandbox` |
| ACP | `crates/codegen/xai-acp-lib` |
| Hooks | `crates/codegen/xai-grok-hooks` |
| Memory | `crates/codegen/xai-grok-memory` |
| Markdown TUI render | `crates/codegen/xai-grok-markdown` |
| Mermaid | `crates/codegen/xai-grok-mermaid` → `third_party/mermaid-to-svg` |
| Models defaults | `crates/codegen/xai-grok-models` (embedded `default_models.json`) |
| User guide (shipped docs) | `crates/codegen/xai-grok-pager/docs/user-guide/` |
| Proto for tools API | `crates/codegen/xai-grok-tools-api/proto/grok-tools.proto` |

## Entry points

| Binary / mode | Package | Notes |
| --- | --- | --- |
| `xai-grok-pager` | `xai-grok-pager-bin` | Main product binary |
| Headless / leader / stdio agent | via pager-bin → `xai_grok_shell::agent::app` | `run_headless`, `run_leader`, `run_stdio_agent` |
| `xai-workspace-server` | `xai-grok-workspace` | Optional workspace host server |
| `workspace-server-probe` | `xai-grok-workspace` | Probe helper |
| `chat-history-downgrade` | `xai-grok-shell` | Session migration utility |
| `ptyctl` / `ptyctl-cli` | codegen | Headless PTY controller (tests/tooling) |

## Generated / do-not-hand-edit

| Artifact | Guidance |
| --- | --- |
| Root `Cargo.toml` | Generated workspace; edit per-crate manifests |
| `target/**` | Build output |
| Proto-generated Rust (via build.rs / tonic) | Regenerate through cargo build of owning crate; use `bin/protoc` / DotSlash |
| `crates/codegen/xai-grok-agent/src/prompt/prompt_encrypted.rs` | Generated; full edit flow (edit → encrypt → tests → fold): [Workflows → System prompt templates](workflows.md#system-prompt-templates) |
| `THIRD-PARTY-NOTICES` / crate notices | Attribution; update when deps change, not for feature work |

## Ownership notes

- New tools: prefer `xai-grok-tools` implementations + registry wiring; keep shell thin.
- New TUI surfaces: `xai-grok-pager` (and render crate when presentation-only).
- New host capabilities: `xai-grok-workspace` (+ types crate for cross-boundary data).
- Cross-cutting types for proxy/signed config: `prod/mc/cli-chat-proxy-types`.

## Sources
- `README.md`
- Workspace `Cargo.toml` `[workspace].members`
- Crate `src/` trees under `crates/codegen/`
