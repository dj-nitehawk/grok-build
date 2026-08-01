---
type: Playbook
title: Fork Sync
description: Keep fork customizations on dev while pulling origin/main; always-keep prompts; regroup area commits after each sync.
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

## Always-keep fork prompt templates

**Hard policy:** never take upstream content for these two files. The fork’s versions on `dev` are authoritative. Do not merge, combine, or “update from monorepo” their text.

| Path | Policy |
| --- | --- |
| `crates/codegen/xai-grok-agent/templates/prompt.md` | Always keep pre-sync `dev` (backup) version |
| `crates/codegen/xai-grok-agent/templates/subagent_prompt.md` | Always keep pre-sync `dev` (backup) version |

Related prompt **Rust** sources (`template.rs`, `context.rs`, `prompt_encrypted.rs`, etc.) are **not** under this hard rule: resolve those for compile correctness while preserving customization intent. After intentional template edits on `dev`, regenerate encrypted bytes from the agent crate: `python3 scripts/encrypt_templates.py`.

### Why not only “take ours/theirs” at conflict time

Silent auto-merges can mix upstream lines into these templates without a conflict marker. So always **force-restore from `$BACKUP` after the rebase (or merge) finishes**, even when git reported no conflict on those paths.

### During a rebase conflict that touches either file

On a rebase, “theirs” is the commit being replayed (customization) and “ours” is the new base (`main`). Prefer the backup tip (full pre-sync custom tree):

```sh
git checkout "$BACKUP" -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md
git add \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md
# resolve any other paths, then:
git rebase --continue
```

Never `git checkout main --` or `git checkout --ours --` for these two paths during a rebase.

### During a merge conflict (`merge main` into `dev`)

On a merge while on `dev`, “ours” is `dev`. Still prefer `$BACKUP` (same content as pre-merge `dev` tip if backup was taken then):

```sh
git checkout "$BACKUP" -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md
git add \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md
```

### After rebase or merge completes (mandatory)

```sh
git checkout "$BACKUP" -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md

if ! git diff --quiet -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md
then
  git add \
    crates/codegen/xai-grok-agent/templates/prompt.md \
    crates/codegen/xai-grok-agent/templates/subagent_prompt.md
  git commit -m "$(cat <<'EOF'
keep custom system prompt templates after upstream sync

EOF
)"
fi
```

Verify identity with the backup (should print nothing):

```sh
git diff "$BACKUP" -- \
  crates/codegen/xai-grok-agent/templates/prompt.md \
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md
```

If the user later **intentionally** edits these templates on `dev`, that becomes the new authoritative content for the next sync’s `$BACKUP`. Do not re-introduce `main`’s versions when “fixing” prompts after a monorepo sync.

## Sync procedure (rebase preferred)

Solo-fork default: **ff/update `main`, then rebase `dev` onto `main`, then regroup** into the area series. Prefer rebase over merge so history stays “upstream tip + N area customization commits.”

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

1. **If either always-keep prompt template is involved:** restore both from `$BACKUP` (see above), `git add` them, do not use upstream text.
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
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md
```

### 3. Verify build after rebase

Smallest useful checks (prefer package-scoped; see [Workflows](workflows.md) and [Testing](testing.md)):

```sh
cargo check -p xai-grok-pager-bin
cargo check -p xai-grok-pager-bin --no-default-features   # or: cargo grok-slim-check
# If conflicts touched tests or a specific crate:
cargo test -p <crate>
# Defaults are slim; capability tests are cfg-gated (see Testing). New missing-feature
# failures usually mean a new upstream test needs the same feature cfg.
```

Report any check failures; do not push a broken tip without saying so.

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

One commit per series area (currently ~12). Fold within an area:


| Area | Fold into it |
| --- | --- |
| Slim strip | All tools/shell/pager/telemetry/workspace/build/policy-docs commits; post-sync `re-apply slim gates` / feature-cfg fixups |
| TUI startup TTFP | Config-reuse + nonblocking auth/prefetch + frozen welcome paint before connect (and similar startup-only follow-ups) |
| Other areas | Same-intent fixups only; do not merge unrelated product features |

**Do not** merge distinct product features (prompts, border line, handoff, purge, OKF, MCP promote, redo fix, spawn_subagent, …) into one commit. Keep areas separate so conflicts stay localized.

New fork work that is not already in the series: add a new area commit (and refresh the series list below) rather than stuffing it into slim.

#### Procedure

Work on the **post-rebase tip** (tree already includes upstream + customizations + always-keep prompts). Preserve that tip tree exactly.

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
#   11 release-local  12 github release workflow
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

`$BACKUP` is the **pre-sync** tip (always-keep prompt source). `$REORG_BACKUP` / `$PRE_REORG` is the **post-sync** tip before history rewrite. Do not use `$BACKUP` as the tree-identity target for regroup (upstream files differ after rebase).

### 5. Publish updated branches

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
# resolve conflicts (always-keep prompts from $BACKUP), commit merge
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
| Telemetry export | `export-sentry` / `export-otel` on telemetry (+ bin forwards) | **off** | keep facade + `log_event` / stubs |
| Hub OTLP donation | `xai-grok-shell/hub-telemetry` → workspace + hub-sdk `metrics`/`telemetry-donate` + `xai-tracing/otlp` + file-utils `otel-context` | **off** | LocalRegistry stays; only donation/propagator stack is optional |
| Crash reports | bin `crash-report` (install path only) | **off** | **keep TTY restore** always (`xai-crash-handler`) |
| Sandbox enforce | `sandbox-enforce` | **on** (unless user changes) | security default |
| Computer hub / ACP | — | linked | non-goal: LocalRegistry + ACP spine stay; OTLP donate is gated above |
| MCP | — | linked | keep (product need); not stripped |
| Codebase graph | shell/workspace `codebase-graph` | **off** | take main code-nav bodies; keep feature off; bash permission `tree-sitter-bash` stays |
| Workspace Prometheus | workspace `prometheus-metrics` | **off** | facade no-ops (`prometheus_facade`); product paths unchanged |
| Image codecs | png+jpeg only; optional `image-extra` / product-full | **png+jpeg** | never strip `image` crate; drop gif/webp/tiff/bmp/ico on slim |
| web_fetch | tools `web-fetch` (shell/agent forward) | **off** | keep types; omit registration + HTML deps |
| PPTX extract | tools `pptx` (shell forward) | **off** | `read_file` arm only; PDF is separate |
| System power | shell `system-power` | **off** | auth sleep-gate no-op; accept rare re-auth after suspend |
| Product TLS | workspace `rustls` ring-only + tonic `tls-ring` + MCP `rustls-no-provider` + OTLP `tls-ring` | **ring** | residual `aws-lc-sys` via nono/sigstore (sandbox); may feature-unify `rustls-webpki/aws-lc-rs` but product `rustls` stays ring-only |
| System theme | pager-render `system-theme` | **off** | optional `dark-light`; no `ashpd`/`zbus` when power also off |
| Syntax highlight | workspace syntect `default-fancy` | **on (lighter)** | keep highlighting; drop Oniguruma (`onig`/`onig_sys`); pure-Rust fancy-regex |
| Pager minimal | bin `pager-minimal` | **off** | optional `xai-grok-pager-minimal`; fork does not use minimal mode |
| Auto-update | bin `auto-update` | **off** | optional `xai-grok-update` |
| Plugin marketplace | pager/shell `marketplace` | **off** | optional `xai-grok-plugin-marketplace` |

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
# equivalent: cargo build -p xai-grok-pager-bin --profile release-dist --no-default-features
cargo check -p xai-grok-pager-bin --no-default-features   # or: cargo grok-slim-check
```

After sync, spot-check that heavy crates are absent under slim:

```sh
for c in pdf_oxide rhai aws-sdk-s3 gcloud-storage sentry opentelemetry fastrace-opentelemetry \
  xai-codebase-graph scraper htmd xai-system-power zbus prometheus dark-light onig onig_sys \
  xai-grok-pager-minimal xai-grok-update xai-grok-plugin-marketplace
do
  cargo tree -p xai-grok-pager-bin --no-default-features -i "$c" || true
done
# product TLS is ring-only; residual aws-lc via nono/sigstore (sandbox) is accepted
cargo tree -p xai-grok-pager-bin --no-default-features -i ring || true
cargo tree -p xai-grok-pager-bin --no-default-features -i aws-lc-sys || true
# brotli only when web-fetch / product-full enables tools compression
cargo tree -p xai-grok-pager-bin --no-default-features -i brotli || true
```

### Conflict hotspots (strip)

- Per-crate `Cargo.toml` `[features]` and `optional = true` deps
- `xai-grok-tools/src/registry/types.rs` registration block
- Pager settings registry (voice / mermaid constants)
- `xai-file-utils` if upstream merges events with cloud SDK types
- Bin `main.rs` sentry / crash / jemalloc / subprocess intercepts

## Customization series (what to preserve)

`main..dev` intent (oldest first). After every sync, **regroup** so history is one commit per area below (see step 4). The same *intent* should remain even if SHAs change. Refresh this list when the set changes (new local feature, drop, or rename).

1. Customize system prompts (templates are **always-keep**; see above)
2. Custom bottom border info line (`PromptBorderChips` + `keep_info_line_mode_flag` in `credit_bar`; thin `prompt_widget` + `agent_view/render` wiring only; no fork-only `PromptInfo` fields; always-approve filtered in `credit_bar`, not removed from render)
3. Handoff feature (bodies in `dispatch/session/handoff.rs`, `effects/handoff.rs`, `acp_session_impl/handoff.rs`, …)
4. `/purge` command for cleaning history (bodies in `effects/purge.rs`, `session/purge.rs`, …)
5. Setup OKF
6. Ability to promote MCP tools
7. Slim strip (single commit: tools, shell, pager, telemetry, workspace, build composition, policy docs). Inventory above; `product-full` forwards leaf features.
8. Unrelated product fix: Ctrl+Shift+Z redo in textarea
9. TUI startup TTFP: reuse effective config on connect; nonblocking auth/prefetch; docs extract skip-if-unchanged; frozen welcome paint before connect (not interactive until event loop) (`app/startup.rs`; join after terminal; thin reorders in `app::run` / `event_loop` / `acp::connect`; `docs.rs` stamp)
10. Concise parent `spawn_subagent` description (hybrid; `xai-tool-types` task schema + agent builder)
11. Build: `release-local` profile for fast local installs
12. CI: GitHub release workflow


### Conflict hotspots (product)

- **Always-keep:** `templates/prompt.md`, `templates/subagent_prompt.md` (never take `main`)
- Prompt Rust loaders/renderers (`template.rs`, `context.rs`, …) when template variable sets change
- `prompt_widget` info-line layout; chips live as `PromptFlag`s, not extra `PromptInfo` fields
- `credit_bar.rs` (`PromptBorderChips`, `keep_info_line_mode_flag`, quota/context helpers) and Alt+Q / billing cache paths in `dispatch/billing.rs` + `status.rs`
- **Do not “fix” info-line policy in `agent_view/render.rs`.** Main may still push `always-approve`; fork drops it in `credit_bar`. Prefer taking main’s render assembly on sync.
- **Billing auto-fetch removals** (quota is Alt+Q only): `FetchBilling` / `FetchAppBilling` deletions in `event_loop.rs`, `dispatch/auth.rs`, `dispatch/prompt.rs`, `dispatch/session/{lifecycle,load}.rs`, poll arm in `event_loop`, plus tests in `queue` / `billing`
- Thin registration only: `slash/commands/mod.rs`, `extensions/mod.rs`, `helpers/mod.rs`, one arm each in `router` / `effects` / `task_result` / `acp_agent`; `acp_session.rs` `mod handoff` + `run_loop` arm
- Feature bodies (prefer these over switchboards): `dispatch/session/handoff.rs`, `effects/{handoff,purge}.rs`, `session/purge.rs`, `slash/commands/{handoff,purge}.rs`, `extensions/handoff.rs`, `acp_session_impl/handoff.rs`, `session/helpers/session_handoff.rs`
- Anything under `.okf/` if upstream ever adds the same paths (rare in public tree)
- **Startup TTFP:** `app/mod.rs` `run` spine (auth/prefetch order, paint-before-connect + discard-pending-input), `app/startup.rs` (fork-owned helpers: prefetch kick, config snapshot, frozen welcome / minimal skeleton paint), `app/event_loop.rs` AppInit preloaded-config arg, `acp/mod.rs` `connect` signature, `docs.rs` extract stamp; take main’s new startup steps when possible and re-apply join-after-terminal + preloaded-config + paint-before-connect

## Reducing future conflicts (fork conventions)

**Normative day-to-day rules** (feature work on `dev`): [Conventions → Fork customizations](conventions.md#fork-customizations-dev-only). That section includes the finish gate and the **filter, do not delete main’s assembly** rule.

Sync-time reminders:

- Prefer **new files** for feature bodies; touch shared switchboards only for registration.
- Prefer **filter/transform in fork modules** over deleting or rewriting main-owned assembly in hot files.
- Do **not** “refresh” always-keep prompt markdown from `main` to shrink the diff.
- After extracting or adding customizations, refresh the series list and hotspot bullets above if touch points changed.

## Agent checklist

When the user asks to sync after `origin/main` updates:

1. [ ] Working tree clean; fetch `origin`
2. [ ] Show what will land: `main..origin/main` and current `main..dev`
3. [ ] Create `$BACKUP` branch on `dev` and keep the name
4. [ ] FF `main` to `origin/main`
5. [ ] Rebase `dev` onto `main`; on conflicts preserve custom intent
6. [ ] For always-keep prompt templates: never use `main` text; restore from `$BACKUP` on conflict
7. [ ] **After rebase:** force-restore both prompt templates from `$BACKUP`; commit if dirty
8. [ ] Confirm `git diff "$BACKUP" --` on both template paths is empty
9. [ ] `cargo check -p xai-grok-pager-bin` (and targeted tests if needed)
10. [ ] Slim check: `cargo check -p xai-grok-pager-bin --no-default-features` (or `cargo grok-slim-check`)
11. [ ] If running tests: defaults = slim; capability tests cfg-gated ([Testing](testing.md)); chase non-capability failures
12. [ ] **Regroup** `main..dev` into the canonical area series (step 4); verify tip tree matches pre-reorg tip
13. [ ] Refresh [Customization series](#customization-series-what-to-preserve) if areas changed
14. [ ] Confirm before `git push --force-with-lease origin dev`
15. [ ] Report: new `main` tip, `main..dev` area commits, conflicts, prompt-template restore, reorg, verify result
16. [ ] If this playbook’s customization list or procedure changed, update this file
17. [ ] If strip features conflicted: re-apply fork defaults/alias; take main module bodies

## Do not

- Commit product customizations on `main`.
- `git push --force` to `main` / `origin/main`.
- Drop customization **intent** to “make rebase easy” without explicit user approval (regrouping commit boundaries is required; deleting features is not).
- Skip post-sync series regroup when the log is noisy (fixups, split slim/TTFP, re-apply gates) unless the series is already clean per step 4.
- Merge unrelated product features into one commit during regroup.
- Change tip tree content during regroup (only commit boundaries / messages).
- Assume an `upstream` remote exists (this fork may only use `origin`; monorepo syncs already appear on `origin/main`).
- Accept upstream (or auto-merged) content for `prompt.md` / `subagent_prompt.md`.
- Delete `$BACKUP` before the always-keep restore and empty-diff check have succeeded.


## Sources
- Local branch layout: `main` + `dev` on `origin` (`dj-nitehawk/grok-build`)
- Always-keep paths: `crates/codegen/xai-grok-agent/templates/{prompt,subagent_prompt}.md`
- [Workflows](workflows.md) (build/check commands)
- [Testing](testing.md) (slim default tests vs feature-on coverage)
- [Gotchas](gotchas.md)
