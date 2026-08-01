---
type: Reference
title: Project Overview
description: Purpose, scope, and consumers of the Grok Build CLI/TUI workspace.
tags: [overview]
resource: README.md
---

# Project Overview

## Purpose

**Grok Build** (`grok`) is SpaceXAI's terminal-based AI coding agent. It runs as a full-screen TUI that understands a codebase, edits files, runs shell commands, searches the web, and manages long-running tasks. It also supports headless/scripted use and IDE embedding via the Agent Client Protocol (ACP).

This repository holds the Rust source for the CLI/TUI and agent runtime. It is synced periodically from the SpaceXAI monorepo. `SOURCE_REV` at the repo root records the monorepo commit SHA for the published tree.

## Scope

| In scope | Out of scope (here) |
| --- | --- |
| `xai-grok-pager` binary and agent/tool/workspace crates | External PR/contribution workflow (not accepted) |
| Local build/test of the Cargo workspace | Server-side product services beyond types in `prod/mc/` |
| User-facing docs under the pager crate user guide | crates.io publishing of first-party crates as a library product |

## Capabilities

- Interactive TUI (scrollback, prompt, modals, theming, slash commands)
- Headless one-shot / scripting (`grok -p`, JSON output formats)
- ACP agent mode (stdio or WebSocket) for IDEs and SDKs
- Tool runtime (filesystem, shell, search, MCP, skills, subagents, workflows)
- Workspace host layer (FS, VCS, worktrees, permissions, optional workspace server)
- Config layers (user `~/.grok`, managed/requirements, CLI/env overrides)
- OS sandbox profiles (Landlock / Seatbelt via `xai-grok-sandbox`)
- Mermaid render path (vendored under `third_party/`)

## Status

- Public tree for source transparency and local builds (Apache-2.0).
- External contributions are not accepted (`CONTRIBUTING.md`).
- Prebuilt binaries install as `grok`; cargo artifact is `xai-grok-pager`.
- macOS and Linux are supported build hosts; Windows from this tree is best-effort.

## Non-goals

- Accepting unsolicited external patches.
- Treating root `Cargo.toml` as hand-edited source of truth (it is generated).
- Replacing online product docs; prefer [docs.x.ai/build](https://docs.x.ai/build/overview) and the in-tree user guide for end-user behavior.

## Glossary

| Term | Meaning |
| --- | --- |
| pager | TUI / product UI crate family (`xai-grok-pager*`) |
| shell | Agent runtime (`xai-grok-shell`) |
| leader | Long-lived agent process mode used with clients |
| workspace | Host FS/VCS/exec abstraction (`xai-grok-workspace`) |
| grok home | Per-user data dir: `$GROK_HOME` or `~/.grok` |
| ACP | Agent Client Protocol (JSON-RPC agent embedding) |

## Sources
- `README.md`
- `CONTRIBUTING.md`
- `SOURCE_REV`
- `crates/codegen/xai-grok-pager/docs/user-guide/`
