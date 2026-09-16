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

1. **Rust** via `rustup`. Pinned in `rust-toolchain.toml` (channel `1.94.0`, rustfmt + clippy).
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
cargo run -p xai-grok-pager-bin              # build + launch TUI (normal upstream capability defaults)
cargo build -p xai-grok-pager-bin --release  # → target/release/xai-grok-pager
cargo check -p xai-grok-pager-bin            # fast validation
cargo check -p <crate>
```

Distribution-oriented profile (slower, more optimized):

```sh
cargo build -p xai-grok-pager-bin --profile release-dist
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
- System prompt templates: follow [System prompt templates](#system-prompt-templates) below (edit → encrypt → tests → fold).
- No separate open-source “migrate DB” CLI documented as a primary workflow; session utilities include shell bin `chat-history-downgrade`.
- After dependency or workspace member changes, expect lockfile updates via normal cargo resolution (root `Cargo.toml` itself is generated upstream). Commit the resulting `Cargo.lock` with that same change (see [Dependencies](dependencies.md)).

## System prompt templates

Fork-owned always-keep templates (`prompt.md`, `subagent_prompt.md`) plus optional `apply_patch_prompt.md`. Runtime loads XOR-obfuscated bytes from `src/prompt/prompt_encrypted.rs` (do not hand-edit that file).

**Flow (always in this order):**

1. **Edit** the markdown under `crates/codegen/xai-grok-agent/templates/*.md`.
2. **Encrypt** from that crate (regenerates `src/prompt/prompt_encrypted.rs`):
   ```sh
   cd crates/codegen/xai-grok-agent
   python3 scripts/encrypt_templates.py
   ```
3. **Update / fix tests** that assert template shape or rendered content (typically `src/prompt/template.rs` and `src/prompt/context.rs`). Align asserts with the fork-owned sections (`work_policy`, optional `background_tasks` / `user_guide`); remove asserts for dropped sections. Run:
   ```sh
   cargo test -p xai-grok-agent --lib 'prompt::template::tests'
   cargo test -p xai-grok-agent --lib 'prompt::context::tests'
   ```
4. **Fold into the existing customization commit** (do not leave a new tip commit for routine prompt tweaks). On `dev`, the area commit subject is `customize system prompts` (SHA rewrites after regroup/rebase). Capture the target SHA **before** the fixup (a later `fixup!` subject also matches a naive grep):
   ```sh
   PROMPT_COMMIT=$(git log --grep='^customize system prompts$' -1 --format=%H)
   git add \
     crates/codegen/xai-grok-agent/templates/ \
     crates/codegen/xai-grok-agent/src/prompt/prompt_encrypted.rs \
     crates/codegen/xai-grok-agent/src/prompt/template.rs \
     crates/codegen/xai-grok-agent/src/prompt/context.rs
   git commit --fixup="$PROMPT_COMMIT"
   GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash "${PROMPT_COMMIT}^"
   # force-with-lease push dev only after explicit user confirm
   ```

Related Rust loaders (`template.rs`, `context.rs`) may need edits when placeholders or section expectations change; fold those with the same commit. Fork-sync always-keep policy for the two main prompt markdown files (and root `README.md`): [Fork Sync](fork-sync.md#always-keep-fork-files).

## GitHub Release (linux/amd64)

Workflow: `.github/workflows/release-linux-amd64.yml`

- **Trigger:** push of a tag matching `v*` (lowercase `v`; GitHub globs are case-sensitive).
- **Branch policy:** the tagged commit must be an ancestor of `origin/dev` (tags are not branch-scoped in git; the job enforces this).
- **Build:** `cargo build -p xai-grok-pager-bin --profile release-dist --target x86_64-unknown-linux-gnu` (default features: `stock`, `jemalloc`, `sandbox-enforce`).
- **Artifact:** zip containing a `grok` binary, attached to a GitHub Release for that tag (`grok-<tag>-linux-amd64.zip`).
- **Release body:** the H1 in `crates/codegen/xai-grok-shell/CHANGELOG.md` whose version equals the tag with the leading `v` removed. The job fails if that section is missing. Auto PR/commit notes are off.
- **CI needs:** DotSlash on PATH (prebuilt install in the workflow) so `bin/protoc` resolves. Rust channel in the workflow must match `rust-toolchain.toml` (`dtolnay/rust-toolchain` requires an explicit `toolchain` input).

Tag from a `dev` tip (example):

```sh
git checkout dev
git pull
VERSION=$(sed -n 's/^# \([0-9][^ ]*\).*/\1/p' crates/codegen/xai-grok-shell/CHANGELOG.md | head -1)
test -n "$VERSION" && git tag "v$VERSION"
git push origin "v$VERSION"
```

## Common agent workflows

| Goal | Command / path |
| --- | --- |
| Run product | `cargo run -p xai-grok-pager-bin` |
| Unit/integration for one crate | `cargo test -p <crate>` (see [Testing](testing.md)) |
| Test support and defaults | [Testing](testing.md#package-defaults-and-test-support) |
| Format | `cargo fmt --all` |
| Lint one crate | `cargo clippy -p <crate>` |
| Composition-root check | `cargo check -p xai-grok-pager-bin` |
| Pull monorepo updates into this fork | [Fork Sync](fork-sync.md) (`origin/main` → rebase `dev`) |
| Edit system prompt templates | [System prompt templates](#system-prompt-templates) (edit → encrypt → tests → fold) |
| Ship linux/amd64 zip via GitHub Release | push tag `v*` on a commit reachable from `dev` |

## Env vars (names only)

| Name | Role |
| --- | --- |
| `GROK_HOME` | Override user grok data directory |
| `PROTOC` | Override protoc binary location |
| `XAI_API_KEY` | API auth (user runtime) |
| `GROK_MEMORY` | Memory-related override (user guide) |
| `GROK_CONFIG` | Inline JSON config overlay (not a direct setting like `XAI_API_KEY`) |
| `GROK_CONFIG_PATH` | Extra JSON/TOML file overlay. `GROK_CONFIG` wins if both are set |

Do not store values in OKF.

## Sources
- `README.md`
- `rust-toolchain.toml`
- `.cargo/config.toml`
- `.github/workflows/release-linux-amd64.yml`
- `crates/codegen/xai-grok-agent/scripts/encrypt_templates.py`
