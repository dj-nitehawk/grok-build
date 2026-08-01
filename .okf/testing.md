---
type: Playbook
title: Testing
description: How tests are organized and how to run them per crate under fork slim defaults.
tags: [test]
---

# Testing

## Frameworks and layout

- Standard Rust tests: unit tests in modules, integration tests under crate `tests/`.
- Async tests via tokio where crates already use them.
- Snapshot testing: `insta` (e.g. pager).
- Support crates: `xai-grok-test-support`, `xai-test-utils`, pager PTY harness (`xai-grok-pager-pty-harness`).
- Heavy product coverage concentrates in `xai-grok-shell`, `xai-grok-pager`, `xai-grok-tools`, `xai-grok-config`.

Examples of integration surfaces:

| Crate | Examples |
| --- | --- |
| `xai-grok-pager` | `tests/pty_e2e_*.rs`, `doctor_early_dispatch`, render/search bins |
| `xai-grok-shell` | session load/fork, hooks e2e, subagent, vendor compat, trace replay |
| `xai-grok-tools` | path suggestions, cgroup memory, etc. |
| `xai-grok-sandbox` | `sandbox_smoke_test` |

## Slim defaults (fork `dev`)

On this fork, **package defaults are slim**. Plain `cargo test -p <crate>` enables that crate’s default features only, not a full upstream product matrix. Capability inventory and build aliases: [Fork Sync → Slim / strip policy](fork-sync.md#slim--strip-policy-compile-out-capabilities).

| Package (typical) | Default features (fork intent) | Off unless requested |
| --- | --- | --- |
| `xai-grok-shell` | empty | `workflows`, `memory`, `pdf`, `lsp`, `cloud-upload`, `hub-telemetry`, `image-extra`, `web-fetch`, `pptx`, `system-power`, `prometheus-metrics`, `codebase-graph` |
| `xai-grok-pager` | mainly `sandbox-enforce` | `mermaid`, `voice`, `image-extra` |
| `xai-grok-tools` | minimal (`serde` etc.) | `pdf`, `lsp`, `pptx`, `web-fetch`, `workflows`, `memory`, `image-extra` |
| `xai-grok-pager-bin` | slim (`sandbox-enforce`); use `product-full` for fat | jemalloc, mermaid, voice, shell leaf forwards, telemetry export, crash-report |

`cargo test -p xai-grok-pager-bin` is uncommon; most unit coverage lives in the leaf crates above. Composition-root check/build: `cargo check -p xai-grok-pager-bin`, `cargo grok-slim-check`.

### Feature-specific unit tests (gated)

Capability tests that need a stripped leaf are behind `#[cfg(feature = "…")]` (or `#[cfg(all(test, feature = "…"))]` on the test module) so **default slim** `cargo test -p …` does not run them. Integration targets may use `required-features` in `Cargo.toml` (existing pattern for shell `test-support`).

| Gate | Typical unit-test homes |
| --- | --- |
| `mermaid` | `xai-grok-pager` `app/mermaid_worker` engine/render tests |
| `workflows` | `xai-grok-shell` `session/workflow/{manager,registry}` (+ related restore) |
| `memory` | shell `session/memory/hooks` write paths, `memory_flush` semantic dedup, first-turn injection, `auth/credential_provider` embedding scope |
| `pdf` | tools `read_file` PDF size-gate / render paths |
| `image-extra` | tools/shell gif/ico/webp/tiff/bmp paths |

If a default-feature failure still says “not compiled” / `Unsupported` for a gated codec, a new upstream test likely needs the same cfg. Prefer gating over re-enabling defaults.

### Known reds on monorepo tip (not slim)

Verified against pure `origin/main` (same monorepo SHA as after last sync) and/or zero fork commits on the path. Do **not** chase these as slim fallout; fix upstream or wait for monorepo, unless a fork customization clearly re-breaks them.

| Crate | Tests (filter / area) | Notes |
| --- | --- | --- |
| `xai-grok-pager` | `scrollback::blocks::user` skill-token teal spans (6) | Fail on pure main; product vs test expectation drift |
| `xai-grok-pager` | command palette + picker search-bar cursor focus (2) | Fail on pure main |
| `xai-grok-pager` | `settings_modal::…picker_highlights_current_choice` | Fail on pure main (`bg_visual` vs `DarkGray`) |
| `xai-grok-shell` | `agent::models::from_config_without_prefetch…` | No fork commits on path; treat as upstream |
| `xai-grok-shell` | `mvp_agent` post-auth settings gate / writeback (4) | Likely upstream (fork only registers handoff/purge/code-nav) |
| `xai-grok-shell` | `auto_wake_suppression` cancel-barrier / admit (2) | No fork commits on path; treat as upstream |

Fork-fixed / forked concerns (not in the table): Ctrl+Shift+Z redo is handled in `xai-ratatui-textarea` (`is_redo_input` / `is_undo_input`); product-memory embedding scope is cfg-gated.

### Full capability coverage

Enable the leaf features that match the code under test:

```sh
# Examples (adjust to the surface you touched)
cargo test -p xai-grok-pager --features mermaid
cargo test -p xai-grok-shell --features workflows,memory
cargo test -p xai-grok-tools --features pdf,image-extra,pptx,web-fetch

# Fat composition-root build/check (not a substitute for leaf unit tests)
cargo check -p xai-grok-pager-bin --features product-full
```

Prefer the smallest feature set that covers the change. Use `product-full` when verifying the bin’s feature graph or an install-shaped binary, not as the default unit-test entry.

## Commands

```sh
# Always scope by package when possible (defaults = slim on this fork)
cargo test -p xai-grok-config
cargo test -p xai-grok-tools
cargo test -p xai-grok-shell
cargo test -p xai-grok-pager

# Single test filter
cargo test -p xai-grok-config <filter>

# Avoid default full-workspace test unless intentional (slow)
```

Shell’s lib suite is large. If you hit stack overflow mid-run, raise the stack and re-run:

```sh
RUST_MIN_STACK=16777216 cargo test -p xai-grok-shell --lib
```

Clippy/check as pre-submit style validation:

```sh
cargo check -p <crate>
cargo clippy -p <crate>
# Slim composition root
cargo check -p xai-grok-pager-bin --no-default-features
# or: cargo grok-slim-check
```

## Integration and data

- PTY e2e tests drive the TUI through a harness; may be slower and environment-sensitive.
- Sandbox tests depend on OS support (Landlock/Seatbelt); behavior differs by platform.
- Config/path tests often use tempdirs; prefer existing tempfile patterns.
- Some shell tests exercise network/auth seams with mocks (e.g. mockito in workspace deps) where already present.
- Do not assume Docker is required for the default unit surface; follow the crate under test.

## Expectations

For new behavior:

1. Prefer unit tests next to the module for pure logic.
2. Add/adjust integration tests in the owning crate when cross-module contracts change (session, tools, config layers, pager flows).
3. Keep tests hermetic: no real secrets, no production endpoints unless explicitly gated.
4. When changing canonicalize/path logic, cover Windows-sensitive cases if the crate already tests them; always use `dunce` helpers.
5. Run the smallest `cargo test -p …` that covers the change; state the blocker if not run.
6. **Feature-specific tests:** gate with `#[cfg(feature = "…")]` (unit) or `required-features` on `[[test]]` (integration) so slim defaults stay free of capability-off noise. When touching a gated capability, run both default (off) and feature-on tests when practical.
7. After monorepo sync, if a new test fails only under defaults with a missing-feature / codec message, add the same cfg as sibling tests; otherwise treat as a real regression. See [Fork Sync](fork-sync.md) verify steps.

## Sources
- `README.md` Development section
- `crates/codegen/xai-grok-pager/Cargo.toml` `[[test]]` entries and `[features]`
- `crates/codegen/xai-grok-shell/Cargo.toml` `[features]`
- `crates/codegen/xai-grok-tools/Cargo.toml` `[features]`
- `crates/codegen/xai-grok-pager-bin/Cargo.toml` `product-full`
- `.cargo/config.toml` aliases `grok-slim`, `grok-slim-check`
- [Fork Sync](fork-sync.md) slim policy
- `crates/codegen/xai-grok-shell/tests/`
- `crates/codegen/xai-grok-tools/tests/`
