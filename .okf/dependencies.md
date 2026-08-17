---
type: Reference
title: Dependencies
description: Runtime, package management, and key libraries for the Grok Build workspace.
tags: [deps]
---

# Dependencies

## Runtime

| Item | Detail |
| --- | --- |
| Language | Rust, edition `2024` |
| Toolchain | `rust-toolchain.toml` channel `1.92.0` (+ rustfmt, clippy) |
| Hosts | macOS, Linux primary; Windows best-effort in this tree |
| Package manager | Cargo workspace, resolver `"2"` |
| Hermetic tools | DotSlash + `bin/protoc` (protoc 29.3 pins in DotSlash manifest) |

## Packages

| File | Role |
| --- | --- |
| Root `Cargo.toml` | Generated workspace members, `[workspace.dependencies]`, lints, profiles |
| `Cargo.lock` | Locked transitive graph |
| Per-crate `Cargo.toml` | Package metadata, path deps, features (edit here) |
| `.cargo/config.toml` | Target rustflags, jemalloc page-size env |

Workspace members include `crates/codegen/*`, `crates/common/*`, `crates/build/xai-proto-build`, `prod/mc/cli-chat-proxy-types`, and `third_party/*` Mermaid crates.

## Key libraries

| Area | Libraries / crates |
| --- | --- |
| CLI / TUI | clap, crossterm, ratatui-related (`xai-ratatui-*`), alacritty_terminal / ptyctl |
| Async | tokio, futures, async-trait |
| HTTP / API | reqwest (workspace 0.12 family), async-openai (git patch), axum (where used) |
| MCP | `rmcp` isolated in `xai-grok-mcp` with reqwest 0.13 |
| Proto | prost, tonic, pbjson-build, `xai-proto-build` |
| Config | toml, serde, shellexpand |
| Git | gix **0.86** (fork pin until `main` `>= 0.86`; gix-odb [#2723](https://github.com/GitoxideLabs/gitoxide/issues/2723)), `xai-gix-status`, `xai-fast-worktree` (`gix-status` 0.33) |
| Sandbox | `xai-grok-sandbox` / nono kernel primitives |
| Observability | tracing, fastrace; product Sentry/OTLP export via `xai-grok-telemetry` features `export-sentry` / `export-otel` (slim off); residual OTel may remain via computer-hub / `xai-tracing` |
| Diagrams | `xai-grok-mermaid` → vendored `mermaid-to-svg` stack |
| Workflows | Rhai via `xai-workflow` |
| Auth | oauth2, ring, `xai-grok-auth` |

`[patch.crates-io]` pins `async-openai` to a git fork rev (see root `Cargo.toml`).

## Constraints

- Prefer versions from `[workspace.dependencies]`; avoid silent version skew.
- Application workspace: always commit `Cargo.lock` (reproducible builds/CI).
- Commit lockfile updates in the **same commit** as the `Cargo.toml`, feature, or workspace-member change that produced them. Do not land a standalone "refresh lockfile" commit unless you intentionally ran `cargo update`.
- Full-workspace builds/tests are slow; day-to-day work uses `-p <crate>`.
- Root `Cargo.toml` is generated: do not treat hand-edits as durable in this public tree.
- Re-vendoring third_party requires preserving local patches listed in each crate's Cargo.toml header.
- License: first-party Apache-2.0; third-party attribution in `THIRD-PARTY-NOTICES` and crate-local notices.
- External crates.io publish of the full product graph is not the contribution model.

## Sources
- `Cargo.toml`
- `rust-toolchain.toml`
- `third_party/README.md`
- `crates/codegen/xai-grok-mcp/Cargo.toml`
