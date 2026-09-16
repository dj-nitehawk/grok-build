---
type: Playbook
title: Testing
description: How tests are organized and how to run them per crate with current package defaults.
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
| `xai-grok-pager` | settings, home paths, Mermaid subprocess, public API integration |
| `xai-grok-pager-pty-harness` | `pty_e2e_*.rs`, `doctor_early_dispatch`, scripted scenarios |
| `xai-grok-shell` | session load/fork, hooks e2e, subagent, vendor compat, trace replay |
| `xai-grok-tools` | path suggestions, cgroup memory, etc. |
| `xai-grok-sandbox` | `sandbox_smoke_test` |

## Package defaults and test support

Broad slimming is retired. Plain `cargo test -p <crate>` uses the current
manifest defaults; an empty `default` list does not imply stripped capabilities
when dependencies are unconditional.

| Package | `default` features |
| --- | --- |
| `xai-grok-shell` | none (`default = []`) |
| `xai-grok-pager` | `jemalloc`, `sandbox-enforce`, shell/update defaults |
| `xai-grok-tools` | `serde` |
| `xai-grok-pager-bin` | `stock`, `jemalloc`, `sandbox-enforce` |

`stock` forwards pager, minimal-pager, shell, and updater defaults. Most unit
coverage lives in leaf crates; check the composition root with
`cargo check -p xai-grok-pager-bin`.

Use only features declared in the owning manifest. Shell integration targets
with `required-features = ["test-support"]` need
`cargo test -p xai-grok-shell --features test-support`. Pager exposes
`test-support` for `xai-grok-pager-render` and `xai-grok-gboom` support;
workspace test seams may separately need `xai-grok-workspace/test-support`.
Do not add retired capability cfg gates to hide failing tests.

### Failure triage

Do not keep a stale "known red" list. Investigate failures as regressions or
setup issues, not presumed slimming fallout. Compare the failing path with
`git log main..dev -- <path>` and the upstream baseline to establish ownership.
Fork-owned prompts, handoff/purge, MCP promotion, startup, ChatGPT, and sandbox
settings persistence remain this fork's responsibility.

## Commands

```sh
# Always scope by package when possible (current manifest defaults)
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
# Composition root with normal defaults
cargo check -p xai-grok-pager-bin
```

## ChatGPT regression checks

- Auth, storage races, callback HTTP and mock device exchange: `cargo test -p xai-grok-shell --lib agent::chatgpt`.
- Auxiliary routing: same shell command with filter `prompt_suggest`.
- Pager quota cache, RPC timeout, and reset presentation: `cargo test -p xai-grok-pager --lib <filter> --features xai-grok-workspace/test-support`, using `provider_quota`, `chatgpt_quota`, and `credit_bar` filters.
- Streaming reconciliation: `cargo test -p xai-grok-sampler --lib`; conversation conversion: `cargo test -p xai-grok-sampling-types --lib conversation`.
- Manual callback browser fixture: shell test filter `manual_browser_callback_fixture` with `-- --ignored --nocapture`. It listens on loopback port 18765 for at most 180 seconds, uses dummy state/code printed by the test, and never contacts OAuth services.

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
6. Use existing `test-support` seams and manifest `required-features` where needed. Do not assume retired capability gates exist.
7. After monorepo sync, investigate default-feature failures rather than hiding them behind new cfg gates. See [Fork Sync](fork-sync.md) verify steps.

## Sources
- `README.md`
- `crates/codegen/xai-grok-pager-bin/Cargo.toml`
- `crates/codegen/xai-grok-shell/Cargo.toml`
- `crates/codegen/xai-grok-tools/Cargo.toml`
- `.cargo/config.toml`
