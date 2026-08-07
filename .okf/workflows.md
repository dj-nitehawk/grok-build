---
type: Playbook
title: Workflows
description: Setup, build, run, lint/format, and codegen workflows for the Cargo workspace.
tags: [build]
resource: README.md
---

# Workflows

## Setup

Requirements:

1. **Rust** via `rustup` — pinned in `rust-toolchain.toml` (channel `1.92.0`, rustfmt + clippy).
2. **DotSlash** on `PATH` before first build so `bin/protoc` can fetch hermetic protoc:
   ```sh
   cargo install dotslash
   /usr/bin/env dotslash --help
   ```
3. **protoc**: resolved via `bin/protoc` (DotSlash) or `PATH` / `$PROTOC`.

Supported build hosts: macOS, Linux. Windows is best-effort from this tree.

## Build and run

```sh
# Prefer package-scoped commands (full workspace is slow)
cargo run -p xai-grok-pager-bin              # build + launch TUI (crate defaults = slim on fork)
cargo build -p xai-grok-pager-bin --release  # → target/release/xai-grok-pager
cargo check -p xai-grok-pager-bin            # fast validation
cargo check -p <crate>
# Slim composition root (no-default-features) or fat opt-in
cargo grok-slim-check
cargo check -p xai-grok-pager-bin --features product-full
```

Distribution-oriented profile (slower, more optimized):

```sh
cargo build -p xai-grok-pager-bin --profile release-dist
cargo grok-slim   # release-dist + --no-default-features (fork alias)
```

Fast local install profile (no LTO, CGU=8, debug=0; used by `/home/SOFTWARE/grok/build-release.sh`):

```sh
cargo build -p xai-grok-pager-bin --profile release-local
```

Profiles of note (root `Cargo.toml`): `release`, `release-dist`, `release-local`, `release-dist-jemalloc`, `x-prod`, `dev` (panic=abort on release/dev product profiles as configured).

First launch of the TUI typically opens a browser for authentication (see user guide auth page).

## Lint and format

```sh
cargo fmt --all
cargo clippy -p <crate>
```

Root `clippy.toml` bans raw canonicalize APIs. Nearest `clippy.toml` wins for clippy (no merge).

Toolchain bump procedure is documented in `rust-toolchain.toml` (bump carefully; then workspace check/clippy).

## Codegen and migrations

- Protobuf: crate `build.rs` + `xai-proto-build`; ensure DotSlash/protoc available.
- Proto source example: `crates/codegen/xai-grok-tools-api/proto/grok-tools.proto`.
- System prompt templates: edit `crates/codegen/xai-grok-agent/templates/*.md`, then from that crate run `python3 scripts/encrypt_templates.py` (updates `src/prompt/prompt_encrypted.rs`).
- No separate open-source “migrate DB” CLI documented as a primary workflow; session utilities include shell bin `chat-history-downgrade`.
- After dependency or workspace member changes, expect lockfile updates via normal cargo resolution (root `Cargo.toml` itself is generated upstream). Commit the resulting `Cargo.lock` with that same change (see [Dependencies](dependencies.md)).

## Common agent workflows

| Goal | Command / path |
| --- | --- |
| Run product | `cargo run -p xai-grok-pager-bin` |
| Unit/integration for one crate | `cargo test -p <crate>` (defaults = slim; see [Testing](testing.md)) |
| Capability / full-feature tests | [Testing → Full capability coverage](testing.md#full-capability-coverage) |
| Format | `cargo fmt --all` |
| Lint one crate | `cargo clippy -p <crate>` |
| Slim check / fat bin check | `cargo grok-slim-check` / `--features product-full` |
| Pull monorepo updates into this fork | [Fork Sync](fork-sync.md) (`origin/main` → rebase `dev`) |

## Env vars (names only)

| Name | Role |
| --- | --- |
| `GROK_HOME` | Override user grok data directory |
| `PROTOC` | Override protoc binary location |
| `XAI_API_KEY` | API auth (user runtime) |
| `GROK_MEMORY` | Memory-related override (user guide) |

Do not store values in OKF.

## Sources
- `README.md`
- `rust-toolchain.toml`
- `Cargo.toml` profiles
- `.cargo/config.toml` (`grok-slim`, `grok-slim-check`)
- [Testing](testing.md) (slim vs feature-on tests)
- `bin/protoc`
- `crates/codegen/xai-grok-pager/docs/user-guide/05-configuration.md`
