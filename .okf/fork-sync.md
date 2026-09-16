---
type: Playbook
title: Fork Sync
description: Keep fork customizations on dev while pulling origin/main; always-keep prompts and README; regroup area commits after each sync.
tags: [ops, maintain]
---

# Fork Sync

Playbook for this **private fork**: absorb upstream product updates from `origin/main` without dropping local customizations on `dev`.

## Branch model

| Branch | Role | Tracking |
| --- | --- | --- |
| `main` | Pure upstream mirror. No local custom commits. | `origin/main` |
| `dev` | Daily work branch: `main` + local customization commits. | `origin/dev` |

| Remote | URL role |
| --- | --- |
| `origin` | This fork (`dj-nitehawk/grok-build`). Source of new monorepo syncs on `main`. |

Invariant: `git merge-base dev main` should equal `main` after a successful sync (dev is a linear patch series on top of main).

Do **not** commit customizations on `main`. Do all local product work on `dev` (or short topic branches merged into `dev`).

## When to run

- User asks to pull/sync/update from main / upstream / latest monorepo sync.
- `git fetch origin` shows `origin/main` ahead of local `main`.
- `main..origin/main` is non-empty after fetch.

Skip if `main` already matches `origin/main` and `dev` is already rebased on that `main`.

## Preflight (read-only)

```sh
git status -sb
git fetch origin
git log --oneline main..origin/main
git log --oneline main..dev
git merge-base --is-ancestor main dev && echo "dev is based on main" || echo "dev diverged; inspect before rebase"
```

Stop and ask the user if:

- Working tree is dirty (`git status` not clean).
- `dev` is not a descendant of `main` (unexpected divergence).
- `main` has local commits not on `origin/main` (breaks the pure-mirror invariant).

## Safety before rewrite

Create a disposable backup of `dev` (local only is enough). **Record the branch name**; later steps restore always-keep files from it.

```sh
BACKUP="dev-backup-$(date +%Y%m%d-%H%M%S)"
git branch "$BACKUP" dev
echo "BACKUP=$BACKUP"
```

Confirm with the user before:

- Hard-resetting `main` (rewrites local `main` to match remote).
- Force-pushing `dev` (`--force-with-lease` to `origin/dev`).

Do not force-push `main`.

## Always-keep fork files

**Hard policy:** never take upstream content for these paths. The fork’s versions on `dev` are authoritative. Do not merge, combine, or “update from monorepo” their text.

| Path | Policy |
| --- | --- |
| `crates/codegen/xai-grok-agent/templates/prompt.md` | Always keep pre-sync `dev` (backup) version |
| `crates/codegen/xai-grok-agent/templates/subagent_prompt.md` | Always keep pre-sync `dev` (backup) version |
| `README.md` | Always keep pre-sync `dev` (backup) version (fork product README) |

Related prompt **Rust** sources (`template.rs`, `context.rs`, `prompt_encrypted.rs`, etc.) are **not** under this hard rule: resolve those for compile correctness while preserving customization intent. Intentional template edits on `dev` use the full flow (edit → encrypt → fix tests → fold into `customize system prompts`): [Workflows → System prompt templates](workflows.md#system-prompt-templates).

Intentional root `README.md` edits on `dev` become the new authoritative content for the next sync’s `$BACKUP`. Do not re-introduce `main`’s upstream product README when “refreshing” docs after a monorepo sync.

### Why not only “take ours/theirs” at conflict time

Silent auto-merges can mix upstream lines into these files without a conflict marker. So always **force-restore from `$BACKUP` after the rebase (or merge) finishes**, even when git reported no conflict on those paths.

### During a rebase conflict that touches any always-keep path

On a rebase, “theirs” is the commit being replayed (customization) and “ours” is the new base (`main`). Prefer the backup tip (full pre-sync custom tree):

```sh
git checkout "$BACKUP" -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md \
  README.md
git add \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md \
  README.md
# resolve any other paths, then:
git rebase --continue
```

Never `git checkout main --` or `git checkout --ours --` for always-keep paths during a rebase.

### During a merge conflict (`merge main` into `dev`)

On a merge while on `dev`, “ours” is `dev`. Still prefer `$BACKUP` (same content as pre-merge `dev` tip if backup was taken then):

```sh
git checkout "$BACKUP" -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md \
  README.md
git add \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md \
  README.md
```

### After rebase or merge completes (mandatory)

```sh
git checkout "$BACKUP" -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md \
  README.md

if ! git diff --quiet -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md \
  README.md
then
  git add \
    crates/codegen/xai-grok-agent/templates/prompt.md \
    crates/codegen/xai-grok-agent/templates/subagent_prompt.md \
    README.md
  git commit -m "$(cat <<'EOF'
keep always-keep fork files after upstream sync

EOF
)"
fi
```

Verify identity with the backup (should print nothing):

```sh
git diff "$BACKUP" -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md \
  README.md
```

If the user later **intentionally** edits always-keep files on `dev` (prompts via [the prompt template flow](workflows.md#system-prompt-templates); README as a normal `dev` edit), that becomes the new authoritative content for the next sync’s `$BACKUP`. Do not re-introduce `main`’s versions when “fixing” those paths after a monorepo sync.

## Sync procedure (rebase preferred)

Solo-fork default: **ff/update `main`, then rebase `dev` onto `main`, then regroup** into the area series, then **clear compiler warnings**. Prefer rebase over merge so history stays "upstream tip + N area customization commits."

### 1. Fast-forward local `main` to `origin/main`

```sh
git checkout main
git merge --ff-only origin/main
# If main is a pure mirror and ff-only fails because of accidental local commits:
# stop and report; do not rewrite main without explicit user approval.
git status -sb
```

`main` should now equal `origin/main` (typically commits titled like `Synced from monorepo`).

### 2. Rebase `dev` onto updated `main`

```sh
git checkout dev
git rebase main
```

On conflict for each stop:

1. **If any always-keep path is involved** (prompt templates or root `README.md`): restore all always-keep paths from `$BACKUP` (see above), `git add` them, do not use upstream text.
2. For all other paths: inspect both sides; prefer **preserving customization intent** on the **new upstream structure**.
3. `git add <resolved-paths>`
4. `git rebase --continue`
5. To abort entirely: `git rebase --abort` (returns `dev` to pre-rebase tip; backup branch remains).

After the rebase command finishes successfully:

1. Run the **mandatory** always-keep restore (and commit if dirty).
2. Confirm:

```sh
git log --oneline main..dev    # customization series still present
git merge-base --is-ancestor main dev && echo ok
git diff "$BACKUP" -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md \
  README.md
```

### 3. Verify build after rebase

Smallest useful checks (prefer package-scoped; see [Workflows](workflows.md) and [Testing](testing.md)):

```sh
cargo check -p xai-grok-pager-bin
cargo check -p xai-grok-pager-bin --no-default-features --features sandbox-enforce   # or: cargo grok-slim-check
# If conflicts touched tests or a specific crate:
cargo test -p <crate>
# Defaults are slim; capability tests are cfg-gated (see Testing). New missing-feature
# failures usually mean a new upstream test needs the same feature cfg.
```

Report any check **errors**; do not push a broken tip without saying so. Rustc **warnings** are cleaned in [step 5](#5-fix-compiler-warnings-final-step) after regroup so fixes fold into the area series instead of a trailing tip commit.

If prompt **Rust** glue no longer matches the kept templates after an upstream template-engine change, fix `template.rs` / `context.rs` (and related) on `dev` without replacing the two markdown templates from `main`.

### 4. Regroup customization series (after every sync)

**Default after a successful rebase + always-keep restore + build verify:** rewrite `main..dev` into the canonical **area series** (see [Customization series](#customization-series-what-to-preserve)) so the next monorepo sync has a short, stable patch stack.

Why: rebase and conflict resolution often leave extra fixups (`re-apply slim gates`, split perf commits, docs-only follow-ups). A long noisy series is harder to rebase next time. Regrouping does **not** change product intent; it only rewrites commit boundaries.

#### When to skip

Skip regroup only if **all** of:

- `git log --oneline main..dev` already matches the series list (one commit per area, oldest first).
- No extra fixup/re-apply/WIP commits.
- Series list in this file still matches intent (no new fork feature without a new area entry).

If unsure, regroup.

#### Target shape

One commit per series area (currently 15, oldest first in [Customization series](#customization-series-what-to-preserve)). Fold within an area:


| Area | Fold into it |
| --- | --- |
| setup okf | All `.okf/**` (and OKF-related root `AGENTS.md`) edits; never leave OKF hunks in feature area commits |
| Slim strip | All tools/shell/pager/telemetry/workspace/build/policy-docs commits; post-sync `re-apply slim gates` / feature-cfg fixups |
| TUI startup TTFP | Config-reuse + nonblocking auth/prefetch + frozen welcome paint before connect (and similar startup-only follow-ups) |
| gix 0.86 sunset | Pin `0.87` + lock + shims only (`0.86` lock is unresolvable: yanked `bisync`). **Drop the whole area** when `main` `gix >= 0.86` (not a permanent customization) |
| sandbox settings persist | Privileged `config.toml` writer. **Drop the whole area** when `main` persists user settings under sandbox write-deny (not a permanent customization) |
| Other areas | Same-intent fixups only; do not merge unrelated product features |

**Do not** merge distinct product features (prompts, border line, handoff, purge, OKF, MCP promote, redo fix, spawn_subagent, …) into one commit. Keep areas separate so conflicts stay localized. Feature commits must not touch `.okf/**`; document elsewhere then fold into `setup okf`.

New fork work that is not already in the series: add a new area commit (and refresh the series list below) rather than stuffing it into slim.

#### Procedure

Work on the **post-rebase tip** (tree already includes upstream + customizations + always-keep files). Preserve that tip tree exactly.

```sh
# Still on dev, clean tree, after steps 2–3
PRE_REORG=$(git rev-parse dev)
OLD_TREE=$(git rev-parse dev^{tree})
REORG_BACKUP="dev-reorg-backup-$(date +%Y%m%d-%H%M%S)"
git branch "$REORG_BACKUP" dev
echo "PRE_REORG=$PRE_REORG REORG_BACKUP=$REORG_BACKUP OLD_TREE=$OLD_TREE"

git log --oneline main..dev   # classify each commit into a series area

# Rebuild linear series from main
git checkout -B dev-reorg main

# For each series area (oldest first): cherry-pick the post-rebase
# commit(s) that belong to that area.
# Single commit:
#   git cherry-pick <sha>
# Multiple commits in one area (e.g. slim, TTFP):
#   git cherry-pick -n <sha1> <sha2> ...
#   git commit -m "$(cat <<'EOF'
#   <area subject>
#
#   <optional body>
#   EOF
#   )"
#
# Example order (adjust SHAs to the post-rebase log):
#   1 prompts  2 border  3 handoff  4 purge  5 okf  6 mcp promote
#   7 slim (all strip + re-apply)  8 redo  9 TTFP  10 spawn_subagent
#   11 release-local  12 github release workflow  13 gix 0.86 sunset
#   14 ChatGPT Codex  15 sandbox settings persist
#   + any new area commits after that


# Mandatory: tip tree must match pre-reorg tip
NEW_TREE=$(git rev-parse HEAD^{tree})
test "$OLD_TREE" = "$NEW_TREE" || {
  echo "TREE MISMATCH after reorg; aborting"
  git diff "$REORG_BACKUP" --stat
  exit 1
}

git checkout dev
git reset --hard dev-reorg
git branch -D dev-reorg

git log --oneline main..dev
git diff --quiet "$REORG_BACKUP" && echo "reorg tree OK"
```

If a post-rebase commit mixes two areas (rare), split with `git cherry-pick -n` plus path-limited `git restore --staged/--worktree` before committing, or fix with a follow-up path-based commit and squash. Prefer not inventing content; only re-bucket the existing tip tree.

If regroup would drop customization intent or the tree cannot be matched, **stop**, leave `dev` at `$PRE_REORG` / `$REORG_BACKUP`, and report.

After a successful reorg, refresh the [Customization series](#customization-series-what-to-preserve) list if areas were added, removed, or renamed.

#### Relation to `$BACKUP`

`$BACKUP` is the **pre-sync** tip (always-keep file source: prompts + `README.md`). `$REORG_BACKUP` / `$PRE_REORG` is the **post-sync** tip before history rewrite. Do not use `$BACKUP` as the tree-identity target for regroup (upstream files differ after rebase).

### 5. Fix compiler warnings (final step)

Last content step before publish. Both pager-bin checks must be **warning-free**, not merely successful. A green `Finished` with `warning:` from workspace crates is not done.

```sh
cargo check -p xai-grok-pager-bin --message-format=short
cargo check -p xai-grok-pager-bin --no-default-features --features sandbox-enforce --message-format=short
# or: cargo grok-slim-check
```

Treat any rustc `warning:` whose path is under `crates/` as a defect (ignore third-party/crates.io noise). Typical leftovers after a monorepo sync plus slim re-apply:

- `unexpected cfg condition value` because a slim gate landed in source without a matching `[features]` entry on that crate
- unused imports / `pub(crate)` consts on the feature-off path

Fix them, then **fold into the matching series area** (usually slim for feature-cfg leftovers; OKF copy into `setup okf`). Do not leave a trailing "fix warnings" commit on `main..dev`.

```sh
# after the working tree has the warning fixes:
git add <paths>
git commit --fixup=<area-sha>   # e.g. the slim commit
GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash main
```

If the fold rewrites an area commit, re-run both checks and confirm zero workspace warnings.

### 6. Publish updated branches

```sh
git push origin main
git push --force-with-lease origin dev
```

Use `--force-with-lease` only for `dev` after rebase/reorg. Ask first if the user has not authorized force-push in this task.


## Merge alternative

Use only if the user rejects force-push or `dev` is shared by multiple writers:

```sh
git checkout dev
git merge main
# resolve conflicts (always-keep files from $BACKUP), commit merge
# run mandatory always-keep restore + commit if dirty
git push origin dev   # no force
```

Trade-off: keeps non-linear history and recurring merge commits. **Series regroup (step 4) assumes a linear rebase series**; after a merge, either leave history as-is or rebuild from `main` with the cherry-pick procedure using path/intent classification of the merged tip (harder; prefer rebase sync).

## Slim / strip policy (compile-out capabilities)

Ship a **lighter** `xai-grok-pager` on `dev` by compiling out optional product surfaces, without hard-deleting upstream module trees (those reappear on every monorepo sync and maximize rebase pain). Policy lives only on `dev`; `main` stays pure upstream.

### Principles

1. **Compile out at the leaf** (optional deps + positive capability features).
2. **Enable at the composition root** (`xai-grok-pager-bin` feature forwarding / cargo alias).
3. **Select at registration** (tool registry, slash catalog, session setup). Do not cfg-spray the agent run loop.
4. **Keep upstream module paths and bodies** so `git rebase main` still applies. Fork owns feature flags, thin filters, and defaults.
5. **Never** drop generated root workspace members or delete mid-layer modules solely for size.

### Hard non-goals (v1)

| Surface | Why it stays |
| --- | --- |
| Computer hub `LocalRegistry` | Tool execution bus inside `xai-grok-tools` registry; not a leaf feature |
| Full ACP unwire | Session/TUI/headless are ACP-shaped; removing the protocol is a different product |
| All SQLite | Shell/workspace session search can still use `rusqlite` after product memory is off |
| Hard-delete of gated trees | Conflicts with every `Synced from monorepo` rebase |

### Capability inventory (target on `dev` slim)

| Capability | Leaf gate (feature) | Slim default | Sync rule |
| --- | --- | --- | --- |
| jemalloc | `xai-grok-pager-bin/jemalloc` | **off** | keep fork default; take main bodies |
| PDF (`pdf_oxide`) | `xai-grok-tools/pdf` | **off** | take main `read_file/pdf` body; keep feature off |
| Cloud S3/GCS | `xai-file-utils/cloud-upload` | **off** | never drop `events`/`queue`; only gate SDK backends |
| Mermaid | `xai-grok-pager/mermaid` | **off** | keep fence hooks; optional engine dep |
| Voice | `xai-grok-pager/voice` (+ voice `audio`) | **off** | take main UI; keep feature off |
| LSP | `xai-grok-tools/lsp` | **off** | register only when on |
| Workflows / Rhai | `xai-grok-shell/workflows` | **off** | take main `session/workflow` bodies under cfg |
| Product memory | `xai-grok-shell/memory` | **off** | Noop backend / omit tools; residual SQLite OK |
| Telemetry export | `export-sentry` / `export-otel` on telemetry (+ bin forwards) | **off** | keep facade + `log_event` / stubs. W3C `traceparent` stays on via a non-exporting tracer layer (`xai-grok-otel`); `opentelemetry-otlp` stays off |
| Hub OTLP donation | `xai-grok-shell/hub-telemetry` → workspace + daemon + hub-sdk `metrics`/`telemetry-donate` + `xai-tracing/otlp` + file-utils `otel-context` | **off** | LocalRegistry stays; only donation/propagator stack is optional; daemon preview-scraper donate is cfg-gated |
| Crash reports | bin `crash-report` (install path only) | **off** | **keep TTY restore** always (`xai-crash-handler`) |
| Sandbox enforce | `sandbox-enforce` | **on** (unless user changes) | security default. `grok-slim` / `grok-slim-check` pass `--no-default-features --features sandbox-enforce` so the alias cannot drop enforce |
| Computer hub / ACP | — | linked | non-goal: LocalRegistry + ACP spine stay; OTLP donate is gated above |
| MCP | — | linked | keep (product need); not stripped |
| Codebase graph | shell/workspace `codebase-graph` | **off** | take main code-nav bodies; keep feature off. Queries error (`not compiled`); `x.ai/code/status` reason is `notCompiled`, not an empty hit list or `notStarted`. Bash permission `tree-sitter-bash` stays |
| Workspace Prometheus | workspace `prometheus-metrics` → daemon `prometheus-metrics` | **off** | facade no-ops (`prometheus_facade` in workspace **and** daemon; daemon cannot depend on workspace) |
| Image codecs | png+jpeg only; optional `image-extra` / product-full | **png+jpeg** | never strip `image` crate; drop gif/webp/tiff/bmp/ico on slim. `xai-grok-image` must not enable those `image` features by default (Cargo unifies them into the slim bin). `image-extra` forwards `xai-grok-image/image-extra` |
| web_fetch | tools `web-fetch` (shell/agent forward) | **off** | keep types; omit registration + HTML deps |
| PPTX extract | tools `pptx` (shell forward) | **off** | `read_file` arm only; PDF is separate |
| System power | shell `system-power` | **off** | auth sleep-gate no-op; accept rare re-auth after suspend |
| Product TLS | workspace `rustls` ring-only + tonic `tls-ring` + MCP `rustls-no-provider` + OTLP `tls-ring` | **ring** | `xai-grok-extra-ca` is the process TLS policy crate (OS + Mozilla + extra CA). Take main bodies; pin `ring`, not `aws-lc-rs`. Residual `aws-lc-sys` via nono/sigstore (sandbox) is accepted |
| System theme | pager-render `system-theme` | **off** | optional `dark-light`; no `ashpd`/`zbus` when power also off |
| Syntax highlight | workspace syntect `default-fancy` and `two-face` `syntect-fancy` | **on (lighter)** | keep highlighting; drop Oniguruma (`onig`/`onig_sys`); pure-Rust fancy-regex. Main may pin syntect `default-onig` and two-face `syntect-onig`. Re-apply both fancy pins. If both regex engines unify, syntect uses onig (`parsing/regex.rs`) and the native crate comes back |
| Pager minimal | bin `pager-minimal` | **off** | optional `xai-grok-pager-minimal`; fork does not use minimal mode |
| Auto-update | bin `auto-update` | **off** | optional `xai-grok-update` |
| Plugin marketplace | pager/shell `marketplace` | **off** | optional `xai-grok-plugin-marketplace` |
| Foreign sessions + Claude import | pager/shell `foreign-sessions` | **off** | optional `xai-grok-foreign-sessions`; stub types + no-op scan/marker; hide `/import-claude` at slash catalog |
| Workspace daemon + diag-server | workspace `workspace-daemon` / `diag-server` | **off** | optional deps; `required-features` on `xai-workspace-server` bin; stub `DiagHandle` in handle/hub |

### Always-keep (strip layer)

| Path / artifact | Policy |
| --- | --- |
| Slim capability features + composition defaults on `dev` | Keep fork policy after sync |
| Cargo alias `grok-slim` / `grok-slim-check` in `.cargo/config.toml` | Keep fork aliases |
| Thin registration filters (cfg blocks next to `b.register`, slash visibility) | Prefer minimal; re-apply if main rewrites the block |
| This section of `fork-sync.md` | Keep / refresh inventory |

### Always-take main

- Bodies of gated modules (`pdf.rs`, `implementations/lsp/**`, `session/workflow/**`, mermaid worker, voice UI, memory engine, telemetry event types).
- New upstream call sites: wire them behind the existing feature or a no-op registration; do not delete the upstream addition.

### Build entry (slim)

```sh
cargo grok-slim
# equivalent: cargo build -p xai-grok-pager-bin --profile release-dist --no-default-features --features sandbox-enforce
cargo check -p xai-grok-pager-bin --no-default-features --features sandbox-enforce   # or: cargo grok-slim-check
```

After sync, spot-check that heavy crates are absent under slim:

```sh
for c in pdf_oxide rhai aws-sdk-s3 gcloud-storage sentry opentelemetry-otlp fastrace-opentelemetry \
  xai-codebase-graph scraper htmd xai-system-power zbus prometheus dark-light onig onig_sys \
  xai-grok-pager-minimal xai-grok-update xai-grok-plugin-marketplace \
  xai-grok-foreign-sessions xai-grok-diag-server xai-grok-workspace-daemon
do
  cargo tree -p xai-grok-pager-bin --no-default-features --features sandbox-enforce -i "$c" || true
done
# product TLS is ring-only; residual aws-lc via nono/sigstore (sandbox) is accepted
cargo tree -p xai-grok-pager-bin --no-default-features --features sandbox-enforce -i ring || true
cargo tree -p xai-grok-pager-bin --no-default-features --features sandbox-enforce -i aws-lc-sys || true
# `opentelemetry` + `opentelemetry_sdk` stay on slim for non-exporting W3C traceparent.
# `opentelemetry-otlp` must be absent (export only).
# brotli only when web-fetch / product-full enables tools compression
cargo tree -p xai-grok-pager-bin --no-default-features --features sandbox-enforce -i brotli || true
```

### Conflict hotspots (strip)

- Per-crate `Cargo.toml` `[features]` and `optional = true` deps
- Markdown `syntax.rs`: take main Swift patched grammar (`patched_swift`, `OUT_DIR/syntaxes.bin`) and keep fork `resolve_syntax_token` csharp/fsharp aliases plus their tests. Workspace `two-face` feature must stay `syntect-fancy` (main is `syntect-onig`); do not "fix" it back or onig wins feature unification. Fancy dumps omit `JavaScript (Babel)` and PowerShell; `fallback_syntax_for_missing_grammar` maps `jsx` to plain JavaScript and `ps1` to the shell grammar. Keep that fallback when re-applying main's syntax file
- `xai-grok-tools/src/registry/types.rs` registration block
- Pager settings registry (voice / mermaid constants). `PagerLocalSnapshot::default` keeps `crate::voice_rt::STT_LANGUAGE_DEFAULT` and upstream `subagent_model_inheritance`
- `xai-file-utils` if upstream merges events with cloud SDK types
- Bin `main.rs` sentry / crash / jemalloc / subprocess intercepts
- Extracted `xai-grok-workspace-daemon`: own `prometheus_facade` + `prometheus-metrics` / `hub-telemetry` features; workspace forwards both weakly (`?/`) so TUI hub-telemetry does not pull the daemon crate
- Workspace `diag-server` / `workspace-daemon`: optional; `xai-workspace-server` bin `required-features`; stub `DiagHandle` in `diag.rs` (handle/hub). When main grows methods (`revive_connected`, `clear_terminal_close`), add no-ops on the stub
- `xai-grok-extra-ca`: take main TLS-policy rewrite (native certs, webpki, `SSL_CERT_FILE`); keep `rustls::crypto::ring` in `ensure_default_crypto_provider` / `client_config_with_shared_roots` and handshake tests
- Auth `refresh_chain.rs` (extracted from `manager.rs`): gate `xai_system_power::hold_awake` with `system-power`; no-op path when off. **Declare** `system-power = ["dep:xai-system-power"]` on `xai-grok-login` and forward it from shell (`xai-grok-login/system-power`); a source-only cfg without the crate feature is an `unexpected cfg` warning
- Bin `main.rs` process identity (`set_identity` / `process_identity`): take main; keep `update_facade` for `auto-update` off. Route `channel_name()` through the facade, never `xai_grok_update::` from uncfg'd code. Also take main's `test_seam` module (`signed_policy`); do not drop it when keeping the facade import
- Voice auth: main's `VoiceAuthProvider::bearer` returns `Result<String, VoiceAuthError>` (`ForeignSession` / `NotSignedIn`). The `voice_rt` stub must match. Pager `voice/auth.rs` imports those types from `voice_rt`, not `xai_grok_voice`. Gate `auth_tests` with `#[cfg(all(test, feature = "voice"))]`
- MCP promote vs Messages file forms: `prepare_tool_definitions_inner` uses `tool_definitions_builtins_only_inline_mcp` when `mcp_file_forms_hidden`, else builtins-only, then `append_promoted_mcp_definitions`. Keep both registry methods (`tool_definitions_builtins_only_inline_mcp` and `tool_definitions_with_promoted_mcp`); do not let the two additions delete each other
- `read_file` project instructions: `INSTRUCTION_FILENAMES` is unconditional. Keep `raw_text_to_file_content` / `run_document_extraction` behind `#[cfg(feature = "pptx")]`
- Plugin CTA: take main custom marketplace (`cta_marketplace` + `source_url_or_path`). Route official-source checks through `marketplace_info`, not `xai_grok_plugin_marketplace`, so slim still compiles. Marketplace CLI stays in `plugin_cmd_marketplace.rs` behind `marketplace`
- Foreign sessions: optional `xai-grok-foreign-sessions` on pager + shell; facade `foreign_sessions_api`; Claude import body behind the same feature with marker stubs
- New codebase-graph call sites (`fs_notify.rs` `confine_to_root` is take-main, `handle.rs` `get_covering`, `workspace_ops.rs` `resolve_index_for_file`): use `CodebaseIndexHandle` + cfg stubs, never `xai_codebase_graph::` in uncfg'd signatures. Re-export both `CodebaseIndexHandle` and `get_index_cache_path`. Main dropped eager `ensure_all` / `ensure_codebase_indexes`; do not restore. Cfg-split `spawn_codebase_index_event_forwarder` (no-op when off)
- Memory observation (`xai-grok-memory` `observation.rs`, shell `session/memory_observation.rs`): stub `MemorySearchSource` / `MemoryObservationSink` / related types in `session/memory/disabled.rs`; re-export from `session/memory/mod.rs`; never `use xai_grok_memory` from uncfg'd files. Keep `MemoryBackendParams` field-compatible (`search_source`, `observation_sink`). `V2Manifest` needs `included_entries` (memory-context injection counts).
- Memory flush: `xai-grok-memory` owns `flush.rs` (moved out of shell `helpers/memory_flush`). Re-export `flush` from `session/memory/mod.rs`; stub `should_flush` / `process_flush_response` / `is_semantically_duplicate` / prompt consts in `disabled.rs`. Call sites (`memory_dream.rs`, `compaction.rs`) must use `crate::session::memory::flush`, never `xai_grok_memory::flush`
- Memory dream stubs (`session/memory/disabled.rs`): keep `DreamLock::acquire` -> `Option<DreamLockGuard>` with `commit() -> bool`; `execute_dream(storage, response, sessions_eligible)` (3 args); `clean_processed_sessions`; `DreamStatus` has no `Skipped` (use `Failed` on the no-op path). `IdentityAttrs` in `external_stub.rs` needs `email` even when export is off
- Memory v2 capture/dream/forget/control (new on main): `acp_session_impl/{memory_capture,memory_carryover,memory_control,memory_forget,v2_memory_dream}.rs` and `session/memory/v2_capture.rs` use `xai_grok_memory` directly. Gate those `mod`s behind `memory`. Main deleted `memory_status.rs`; do not restore. Slim stubs in `memory_v2_disabled.rs` (named `memory_capture` when off): `memory_listing` / `memory_toggle_and_list` / `memory_flush_command` / `memory_dream_command` / `run_v2_dream_slash_command` (returns `MemoryDreamResponse`, not `()`). `v2_capture_stub.rs` still owns `FlushResult`. `spawn.rs` tombstone is now `memory_control::initialize_v2_memory`; cfg that call and keep main's `.instrument(memory_init_span.clone())`, do not replay the old inline `V2MaintenanceStore` block. `notification_drain` prompt-sync (`sync_v2_memory_prompt`) is `#[cfg(feature = "memory")]`.
- Workspace `permission/hub_gate.rs` (new on main): take the gate body; import `prometheus_facade` not `prometheus` (same pattern as `recovery.rs`)
- Telemetry `external/schema.rs` now `pub use super::metrics::MetricIncrement`. The export-off stub must `#[path]` include `external/metrics.rs` as well as schema/config/truncate. Cfg `Instruments` / otel imports **and** wire-name consts in `metrics.rs` so the enum still compiles without unused-const warnings when `export-otel` is off. Cfg the `xai_grok_otel::otlp` re-export on `export-otel`. Do not copy telemetry's `export-otel` feature name onto `xai-grok-otel` (`redact_common.rs`); that crate's gate is `otel-context` and it only enables the OTLP exporter (`opentelemetry-otlp`). Slim still mints W3C `traceparent` through `non_exporting_tracer_layer` (no export). `export-otel` must forward `xai-grok-otel/otel-context`.

## Customization series (what to preserve)

`main..dev` intent (oldest first). After every sync, **regroup** so history is one commit per area below (see step 4). The same *intent* should remain even if SHAs change. Refresh this list when the set changes (new local feature, drop, or rename).

1. Customize system prompts (templates are **always-keep**; see above)
2. Custom bottom border info line (`PromptBorderChips` + `keep_info_line_mode_flag` in `credit_bar`; thin `prompt_widget` + `agent_view/render` wiring only; no fork-only `PromptInfo` fields; always-approve filtered in `credit_bar`, not removed from render). `AppRenderParams` keeps main's `status_line` plus fork `billing_fetch_in_flight`
3. Handoff feature (bodies in `dispatch/session/handoff.rs`, `effects/handoff.rs`, `acp_session_impl/handoff.rs`, …)
4. `/purge` command for cleaning history (bodies in `effects/purge.rs`, `session/purge.rs`, …)
5. Setup OKF
6. Ability to promote MCP tools
7. Slim strip (single commit: tools, shell, pager, telemetry, workspace, build composition, policy docs). Inventory above; `product-full` forwards leaf features.
8. Unrelated product fix: Ctrl+Shift+Z redo in textarea
9. TUI startup TTFP: reuse effective config on connect; nonblocking auth/prefetch; docs extract skip-if-unchanged; frozen welcome paint before connect (not interactive until event loop) (`app/startup.rs`; join after terminal; thin reorders in `app::run` / `event_loop` / `acp::connect`; `docs.rs` stamp)
10. Concise parent `spawn_subagent` description (upstream short form; parent appends a `16-subagents.md` pointer plus `task_model_guidance`; no type roster, `subagent_type` stays off the model schema)
11. Build: `release-local` profile for fast local installs
12. CI: GitHub release workflow (includes fork root `README.md`, which is **always-keep**)
13. **Sunset:** bump `gix` to `0.87` (gix-odb [#2723](https://github.com/GitoxideLabs/gitoxide/issues/2723) cleared-slot panic). Drop this area when `main`'s workspace `gix` is `>= 0.86` (see [Sunset: gix 0.86](#sunset-gix-086-gix-odb-2723))
14. ChatGPT Codex backend (`agent/chatgpt/`, thin `default_model_entries` / `sampling_config_for_model` / CLI / slash registration)
15. **Sunset:** persist user settings under sandbox (privileged `config.toml` writer). Drop this area when `main` lands the same (or equivalent) persist path (see [Sunset: sandbox settings persist](#sunset-sandbox-settings-persist))

### Sunset: gix 0.86 (gix-odb #2723)

Temporary fork pin. `gix 0.83` pulls `gix-odb 0.80`, which panics in `load_pack` when a pack slot is cleared under parallel status workers (`BUG: must set this handle to be stable`). Fixed in `gix-odb 0.83` / `gix 0.86`. Cannot patch `gix-odb` alone (`gix 0.83` is `gix-odb ^0.80`).

`gix 0.86` is no longer resolvable: `gix-protocol 0.64` depends on yanked `bisync 0.3`. Pin `gix = "0.87"` (currently `0.87.1` / `gix-odb 0.84`) plus `gix-status = "0.34"` in `xai-fast-worktree`. Drop when `main` is `>= 0.86` (including a main pin of `0.86` or `0.87`).

**Area commit** (last in the series; lockfile last): workspace `gix = "0.87"` in root `Cargo.toml`, `gix-status = "0.34"` in `xai-fast-worktree`, `Cargo.lock`, and remaining API shims (today: `from_bstr(storage)` in `git/safety/git_dir.rs`). Do not fold into slim.

After rebase onto updated `main`, before regroup:

```sh
git show main:Cargo.toml | rg '^gix = '
```

| `main` workspace `gix` | Action |
| --- | --- |
| `< 0.86` (including `0.84` / `0.85`) | Keep the area. Re-apply pin `0.87` + lock + remaining shims (not `0.86`: yanked `bisync`). Lockfile conflicts on this commit are expected. |
| `>= 0.86` | **Drop** the area (do not cherry-pick). Take `main`'s pin, lock, and call sites. Remove this section and series item 13 in `setup okf`. |

### Sunset: sandbox settings persist

Temporary fork fix. Under workspace/read-only/strict, kernel write-deny of `~/.grok/config.toml` (H1-3969489) blocked TUI `/settings` and `/model` saves. The area starts a privileged writer before bwrap/Seatbelt (re-exec of the current binary, then detach from the terminal process group; not `libc::fork` on the tokio runtime) so user-initiated persist works; agent path writes stay denied. `xai-grok-pager-bin` `main` calls `run_user_config_helper_if_requested` first.

**Area commit:** `fix: persist user settings under sandbox`. Fork-owned body: `xai-grok-sandbox/src/user_config_writer.rs`. Thin wiring: sandbox `lib.rs` exports, shell `apply_sandbox` + `persist.rs`, user-guide `18-sandbox.md`, sandbox `Cargo.toml` `tempfile` dev-dep. Do not fold into slim. Do not lift `config.toml` off the kernel write-deny list to "fix" settings. Keep the request token (magic, then 32 bytes, then dest and content) and the "followed target must stay inside grok home" check. CLOEXEC is not the auth boundary. The token crosses re-exec only in a pipe (`GROK_USER_CONFIG_WRITE_TOKEN_FD`), not in the environment (`/proc/<pid>/environ` would keep it). `adopt_from_env` reads and closes that pipe.

After rebase onto updated `main`, before regroup:

```sh
git grep -n 'user_config_writer\|GROK_USER_CONFIG_WRITE_FD\|privileged writer' main -- crates/codegen/xai-grok-sandbox crates/codegen/xai-grok-shell
git show main:crates/codegen/xai-grok-pager/docs/user-guide/18-sandbox.md | rg -n 'session only|privileged|edit.*config.toml'
```

| `main` | Action |
| --- | --- |
| Settings under sandbox still session-only (no user-initiated `config.toml` persist channel) | Keep the area. Re-apply our writer on conflicts. |
| User-initiated `config.toml` persist works under write-deny (their helper, an equivalent channel, or docs no longer tell the user to edit the file by hand) | **Drop** the area (do not cherry-pick). Take `main`'s sandbox, shell persist/`apply_sandbox`, user-guide, and lock. Delete fork-only `user_config_writer.rs` if `main` has no such file. Do not merge our helper with theirs. Remove this section, series item 15, and the matching gotchas bullet in `setup okf`. |

### Conflict hotspots (product)

- **Always-keep:** `templates/prompt.md`, `templates/subagent_prompt.md`, root `README.md` (never take `main`). `prompt_encrypted.rs` is not always-keep, but do not line-merge the ciphertext: take the fork file wholesale (it already has `CODEX_PROMPT_ENC` for the kept templates)
- Prompt Rust loaders/renderers (`template.rs`, `context.rs`, …) when template variable sets change
- **spawn_subagent description:** Upstream `build_task_description(naming)` is the short form and must not mention `subagent_type` (`#[schemars(skip)]`, not on the model-facing schema). Do not restore `TaskDescriptionDetail` or a type roster. The parent override appends `PARENT_TASK_DOCS_POINTER` (user-guide `16-subagents.md`) and then `task_model_guidance(selection, slugs)`. Do not append model guidance inside `build_task_description`
- **ChatGPT quota vs image-notice flush:** outer `dispatch` calls `chatgpt_quota.sync_authentication(false)`, then main's `dispatch_depth` / `dispatch_inner` / `flush_image_notices` wrapper. Keep both
- `prompt_widget` info-line layout; chips live as `PromptFlag`s, not extra `PromptInfo` fields
- `credit_bar.rs` (`PromptBorderChips`, `keep_info_line_mode_flag`, quota/context helpers) and Alt+Q / billing cache paths in `dispatch/billing.rs` + `status.rs`
- **Do not “fix” info-line policy in `agent_view/render.rs`.** Main may still push `always-approve`; fork drops it in `credit_bar`. Prefer taking main’s render assembly on sync. Keep both `AppRenderParams.status_line` (main) and `billing_fetch_in_flight` (fork); pass both from `app_view.rs`.
- **Billing auto-fetch removals** (quota is Alt+Q only): `FetchBilling` / `FetchAppBilling` deletions in `event_loop.rs`, `dispatch/auth.rs`, `dispatch/prompt.rs`, `dispatch/session/{lifecycle,load}.rs`, poll arm in `event_loop`, plus tests in `queue` / `billing`. `handle_credit_limit_recheck_complete` keeps main's `maybe_drain_queue(agent, &mut app.pending_image_notices)` and does not push `FetchBilling`
- `TaskResult::BtwResponse` now passes `skipped_image_numbers` into `handle_btw_response`. Keep that argument and the `HandoffReady` / `HandoffFailed` arms beside it
- Thin registration only: `slash/commands/mod.rs`, `extensions/mod.rs`, `helpers/mod.rs`, one arm each in `router` / `effects` / `task_result` / `acp_agent`; `acp_session.rs` `mod handoff` + `run_loop` arm
- Feature bodies (prefer these over switchboards): `dispatch/session/handoff.rs`, `effects/{handoff,purge}.rs`, `session/purge.rs`, `slash/commands/{handoff,purge}.rs`, `extensions/handoff.rs`, `acp_session_impl/handoff.rs`, `session/helpers/session_handoff.rs`. `CreateSession` / similar effects: pass new fields (`permission_mode_override: None`) rather than inheriting parent mode unless a live override is in scope
- Slash `builtin_commands()`: take main's ordered menu. Insert fork commands into that order (`handoff` next to `fork`, `purge` next to `delete`). Do not replay the pre-order list tail. Slim: `#[cfg(feature = "foreign-sessions")]` on `ImportClaudeCommand` registration
- `acp_agent` method match: keep main's new feedback arms (`upload-trace`) and add the `x.ai/handoff` arm beside them
- `/purge` calls `delete_session_history`; when that signature grows (e.g. `search_index`), pass `None` unless a live `SearchIndexManager` is in scope. Persistence already evicts FTS; purge then sweeps `sessions/`. `clear_directory_contents` must refuse a symlink directory and unlink symlink entries without following them
- Anything under `.okf/` if upstream ever adds the same paths (rare in public tree)
- **Startup TTFP:** `app/mod.rs` `run` spine (auth/prefetch order, paint-before-connect + discard-pending-input), `app/startup.rs` (fork-owned helpers: prefetch kick, config snapshot, frozen welcome / minimal skeleton paint), `app/event_loop.rs` AppInit preloaded-config arg, `acp/mod.rs` `connect` signature, `docs.rs` extract stamp; take main's new startup steps when possible and re-apply join-after-terminal + preloaded-config + paint-before-connect. Do **not** build `ConnectFlags` before prefetch join (`remote_settings` is not available yet). After join, set `status_line` from `ui_config_from_effective` and use main's `effective_auto_for_launch`. `ConnectFlags` field is `no_subagents` (not `subagents`). Take main's `connect_timeout` / `GROK_CONNECT_UI_TIMEOUT_SECS` resolver instead of a hardcoded 30s. Event loop: preloaded config for `launch_effective_config` (plugin-CTA marketplace needs the full root, not only `[ui]`) **and** status-line `report_config`. Carry `preloaded_layers` from `load_effective_config_with_layers` (first load, and `reload_config_after_remote` when remote settings arrived) so `subagent_model_inheritance` does not force a second merge. New crate `xai-grok-status-line` is take-main (not a slim gate). Main replaced `EarlyPrefetchHandle` with process-global `startup_prefetch::begin` / `wait_settings`; `kick_auth_and_prefetch` must call `begin` without awaiting auth, and `join_early_prefetch` must call `wait_settings(EARLY_PREFETCH_WAIT)`. Apply `cache_remote_prompt_suggestions` at join time with the other remote caches.
- **gix 0.86 sunset (until dropped):** root `Cargo.toml` workspace `gix` line (`0.87`), `xai-fast-worktree` `gix-status` pin (`0.34`), `Cargo.lock`. On every sync, compare `main`'s `gix` pin and drop the area at `>= 0.86` (do not keep a no-op bump).
- **sandbox settings persist (until dropped):** `user_config_writer.rs`, sandbox `lib.rs` exports, shell `apply_sandbox` / `persist.rs`, user-guide `18-sandbox.md`. On every sync, if `main` already persists user `config.toml` under write-deny, drop the area and take `main` (do not merge helpers).

## Reducing future conflicts (fork conventions)

**Normative day-to-day rules** (feature work on `dev`): [Conventions → Fork customizations](conventions.md#fork-customizations-dev-only). That section includes the finish gate and the **filter, do not delete main’s assembly** rule.

Sync-time reminders:

- Prefer **new files** for feature bodies; touch shared switchboards only for registration.
- Prefer **filter/transform in fork modules** over deleting or rewriting main-owned assembly in hot files.
- Do **not** “refresh” always-keep files (`prompt.md`, `subagent_prompt.md`, root `README.md`) from `main` to shrink the diff.
- After extracting or adding customizations, refresh the series list and hotspot bullets above if touch points changed.

## Agent checklist

When the user asks to sync after `origin/main` updates:

1. [ ] Working tree clean; fetch `origin`
2. [ ] Show what will land: `main..origin/main` and current `main..dev`
3. [ ] Create `$BACKUP` branch on `dev` and keep the name
4. [ ] FF `main` to `origin/main`
5. [ ] Rebase `dev` onto `main`; on conflicts preserve custom intent
6. [ ] For always-keep files (prompt templates + root `README.md`): never use `main` text; restore from `$BACKUP` on conflict
7. [ ] **After rebase:** force-restore all always-keep paths from `$BACKUP`; commit if dirty
8. [ ] Confirm `git diff "$BACKUP" --` on both prompt templates and root `README.md` is empty
9. [ ] `cargo check -p xai-grok-pager-bin` (and targeted tests if needed)
10. [ ] Slim check: `cargo check -p xai-grok-pager-bin --no-default-features --features sandbox-enforce` (or `cargo grok-slim-check`)
11. [ ] If running tests: defaults = slim; capability tests cfg-gated ([Testing](testing.md)); chase non-capability failures
12. [ ] **Regroup** `main..dev` into the canonical area series (step 4); verify tip tree matches pre-reorg tip
13. [ ] Refresh [Customization series](#customization-series-what-to-preserve) if areas changed
14. [ ] **Warnings (final):** both pager-bin checks warning-free; fold any fixes into the matching area (no extra tip commit)
15. [ ] Confirm before `git push --force-with-lease origin dev`
16. [ ] Report: new `main` tip, `main..dev` area commits, conflicts, always-keep restore, reorg, warnings, verify result
17. [ ] If this playbook's customization list or procedure changed, update this file
18. [ ] If strip features conflicted: re-apply fork defaults/alias; take main module bodies
19. [ ] **gix sunset:** `git show main:Cargo.toml | rg '^gix = '`. If `>= 0.86`, drop series area 13 and this checklist item (fold the OKF delete into `setup okf`). If `< 0.86`, keep the bump commit at `0.87` (not `0.86`: yanked `bisync`; expect `Cargo.lock` conflicts)
20. [ ] **sandbox settings persist:** grep `main` for `user_config_writer` / `GROK_USER_CONFIG_WRITE_FD` and check user-guide `18-sandbox.md`. If `main` persists user `config.toml` under write-deny, drop series area 15 (take `main`, do not merge helpers) and this checklist item (fold the OKF delete into `setup okf`). If still session-only, keep our writer.

## Do not

- Commit product customizations on `main`.
- `git push --force` to `main` / `origin/main`.
- Drop customization **intent** to “make rebase easy” without explicit user approval (regrouping commit boundaries is required; deleting features is not). Exceptions: drop the gix 0.86 sunset area when `main` is `>= 0.86`; drop the sandbox settings persist area when `main` already persists user `config.toml` under write-deny.
- Skip post-sync series regroup when the log is noisy (fixups, split slim/TTFP, re-apply gates) unless the series is already clean per step 4.
- Leave rustc warnings on the post-sync tip, or land them as a trailing "fix warnings" commit. Fold into the matching area (step 5).
- Merge unrelated product features into one commit during regroup.
- Change tip tree content during regroup (only commit boundaries / messages).
- Assume an `upstream` remote exists (this fork may only use `origin`; monorepo syncs already appear on `origin/main`).
- Accept upstream (or auto-merged) content for `prompt.md` / `subagent_prompt.md` / root `README.md`.
- Delete `$BACKUP` before the always-keep restore and empty-diff check have succeeded.


## Sources
- Local branch layout: `main` + `dev` on `origin` (`dj-nitehawk/grok-build`)
- Always-keep paths: `crates/codegen/xai-grok-agent/templates/{prompt,subagent_prompt}.md`, `README.md`
- [Workflows](workflows.md) (build/check commands)
- [Testing](testing.md) (slim default tests vs feature-on coverage)
- [Gotchas](gotchas.md)
