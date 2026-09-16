---
type: Playbook
title: Fork Sync
description: Keep fork customizations on dev while pulling origin/main; always-keep prompts and README; regroup area commits after each sync.
tags: [ops, maintain]
---

# Fork Sync

Playbook for this **private fork**: absorb upstream product updates from `origin/main` without dropping local customizations on `dev`.

## Branch model and safety

- `origin` is this fork (`dj-nitehawk/grok-build`); monorepo syncs arrive on `origin/main`.
- `main` is the pure upstream mirror; `dev` holds the linear customization series.
- After sync, `git merge-base main dev` must equal `main`.
- Start with a clean tree. Stop for unexpected divergence or local-only commits on `main`.
- Create and retain a backup before rewriting. Ask before hard-resetting `main` or force-pushing `dev`; use `--force-with-lease` for `dev` only.
- Preserve customization intent. Regroup changes commit boundaries only, with exact tip-tree identity.
- Day-to-day isolation and OKF commit ownership: [Conventions](conventions.md#fork-customizations-dev-only).

## Always-keep fork files

The pre-sync `dev` versions are authoritative:

```sh
ALWAYS_KEEP=(
  crates/codegen/xai-grok-agent/templates/prompt.md
  crates/codegen/xai-grok-agent/templates/subagent_prompt.md
  README.md
)
```

Commands below use Bash arrays. Keep `$BACKUP` and `ALWAYS_KEEP` in the same shell throughout the sync.

Restore these paths from `$BACKUP` on conflicts and again after the rebase or merge, including conflict-free auto-merges. During rebase, `ours` means the new upstream base; use the backup explicitly. Prompt Rust loaders are resolved for compatibility with the kept templates. Regenerate encrypted prompts through [Workflows](workflows.md#system-prompt-templates) when intentionally changing templates.

Intentional prompt or README edits on `dev` become authoritative for the next sync. Keep the backup until restoration and identity checks succeed.

## Sync procedure (rebase preferred)

### 1. Preflight and backup

```sh
git status -sb
git fetch origin
git log --oneline main..origin/main
git log --oneline origin/main..main
git log --oneline main..dev
git merge-base --is-ancestor main dev
```

Proceed when the tree is clean, `main` has no local-only commits, and `dev` descends from `main`. If upstream and the customization series already match the desired state, skip the rewrite.

```sh
BACKUP="dev-backup-$(date +%Y%m%d-%H%M%S)"
git branch "$BACKUP" dev
echo "BACKUP=$BACKUP"
git checkout main
git merge --ff-only origin/main
git checkout dev
git rebase main
```

If fast-forward fails, stop and inspect. At each rebase conflict:

1. Restore any affected always-keep paths with `git restore --source="$BACKUP" --staged --worktree -- "${ALWAYS_KEEP[@]}"`.
2. Resolve other files on upstream's current structure, preserving fork intent. Use the hotspots below.
3. Stage resolutions and run `git rebase --continue`. `git rebase --abort` restores the pre-rebase branch.

### 2. Restore always-keep files and verify

```sh
git restore --source="$BACKUP" --staged --worktree -- "${ALWAYS_KEEP[@]}"
if ! git diff --cached --quiet -- "${ALWAYS_KEEP[@]}"; then
  git commit -m "keep always-keep fork files after upstream sync"
fi
git diff --exit-code "$BACKUP" -- "${ALWAYS_KEEP[@]}"
git merge-base --is-ancestor main dev
cargo check -p xai-grok-pager-bin
```

Investigate errors and run targeted tests for changed/conflicted surfaces using [Testing](testing.md). Prompt glue repairs belong in the prompt area; markdown stays fork-owned.

### 3. Review sunset areas and regroup

Check the three temporary areas below against current `main`. Retire an area when its fix is upstream, then refresh its series entry and cross-references in `setup okf`. Retirement is an intentional content change performed before the tree-preserving regroup.

Skip regroup only when the log already matches the canonical series in order, with one commit per area and no fixups or undocumented new areas.

- Fold same-intent follow-ups into their area. Keep distinct features separate.
- All `.okf/**` and OKF-related `AGENTS.md` changes belong in `setup okf`.
- Lockfile changes stay with the dependency/feature area that produced them.
- Split mixed-area commits by path or hunk while preserving existing content.

Capture the verified post-sync tree, then rebuild the series on a temporary branch:

```sh
PRE_REORG=$(git rev-parse dev)
OLD_TREE=$(git rev-parse 'dev^{tree}')
REORG_BACKUP="dev-reorg-backup-$(date +%Y%m%d-%H%M%S)"
git branch "$REORG_BACKUP" dev
echo "PRE_REORG=$PRE_REORG REORG_BACKUP=$REORG_BACKUP OLD_TREE=$OLD_TREE"
git checkout -b dev-reorg main
```

Replay areas in the order below. For one existing area commit, use `git cherry-pick <sha>`. For multiple same-area commits, use `git cherry-pick -n <sha1> <sha2> ...` followed by `git commit -m '<area subject>'`. Fold restore/fixup hunks into their owning areas.

```sh
NEW_TREE=$(git rev-parse 'HEAD^{tree}')
if [ "$OLD_TREE" = "$NEW_TREE" ]; then
  git checkout dev
  git reset --hard dev-reorg
  git branch -D dev-reorg
  git diff --exit-code "$REORG_BACKUP" dev --
else
  git diff "$REORG_BACKUP" HEAD --stat
  git checkout dev
  echo "Tree mismatch: dev preserved at $PRE_REORG; inspect dev-reorg"
fi
```

On mismatch, stop without publishing. `$BACKUP` is the pre-sync always-keep source; `$REORG_BACKUP` is the post-sync tree-identity target.

### 4. Clear warnings and publish

```sh
cargo check -p xai-grok-pager-bin --message-format=short
```

Resolve all rustc warnings from workspace crates. Fold fixes into the owning area (`git commit --fixup=<area-sha>`, then `GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash main`) and rerun the check. Fold documentation into `setup okf`. Retain a backup before any additional rewrite.

Final gates: clean tree, `main` ancestry, always-keep identity, canonical area series, exact regroup tree, successful checks, and zero workspace warnings. Report the main tip, areas, conflicts, restoration, retirement/regroup results, and any verification limits.

After explicit force-push approval:

```sh
git push origin main
git push --force-with-lease origin dev
```

### Merge alternative

For shared `dev` or when the user chooses to preserve published history, merge `main` into `dev`. Use the same backup, always-keep restore, tests, and warning gates; publish with a normal `git push origin dev`. Linear-series regroup assumes a rebase and requires a separate history-rewrite decision after a merge.

## Capability defaults after retirement

Use current per-crate manifests and normal upstream capabilities. Broad slimming, strip inventories, stubs, capability gates, and `product-full` forwarding are retired. Updater and telemetry support follow upstream runtime controls; see [Operations](operations.md#config-and-observability).

## Customization series (what to preserve)

`main..dev` intent (oldest first). After every sync, **regroup** so history is one commit per area below (see step 4). The same *intent* should remain even if SHAs change. Refresh this list when the set changes (new local feature, drop, or rename).

1. Customize system prompts (templates are **always-keep**; see above)
2. Custom bottom border info line (`PromptBorderChips` + `keep_info_line_mode_flag` in `credit_bar`; thin `prompt_widget` + `agent_view/render` wiring only; no fork-only `PromptInfo` fields; always-approve filtered in `credit_bar`, not removed from render). `AppRenderParams` keeps main's `status_line` plus fork `billing_fetch_in_flight`
3. Handoff feature (bodies in `dispatch/session/handoff.rs`, `effects/handoff.rs`, `acp_session_impl/handoff.rs`, …)
4. `/purge` command for cleaning history (bodies in `effects/purge.rs`, `session/purge.rs`, …)
5. Setup OKF
6. Ability to promote MCP tools
7. Unrelated product fix: Ctrl+Shift+Z redo in textarea
8. TUI startup TTFP: reuse effective config on connect; nonblocking auth/prefetch; docs extract skip-if-unchanged; frozen welcome paint before connect (not interactive until event loop) (`app/startup.rs`; join after terminal; thin reorders in `app::run` / `event_loop` / `acp::connect`; `docs.rs` stamp)
9. Concise parent `spawn_subagent` description (upstream short form; parent appends a `16-subagents.md` pointer plus `task_model_guidance`; no type roster, `subagent_type` stays off the model schema)
10. Build: `release-local` profile for fast local installs
11. CI: GitHub release workflow (includes fork root `README.md`, which is **always-keep**)
12. **Sunset:** bump `gix` to `0.87` (gix-odb [#2723](https://github.com/GitoxideLabs/gitoxide/issues/2723) cleared-slot panic). Drop this area when `main`'s workspace `gix` is `>= 0.86` (see [Sunset: gix 0.86](#sunset-gix-086-gix-odb-2723))
13. ChatGPT Codex backend (`agent/chatgpt/`, thin `default_model_entries` / `sampling_config_for_model` / CLI / slash registration)
14. **Sunset:** persist user settings under sandbox (privileged `config.toml` writer). Drop this area when `main` lands the same (or equivalent) persist path (see [Sunset: sandbox settings persist](#sunset-sandbox-settings-persist))
15. **Sunset:** tool-layer image bridge test import (`base64::Engine`). This compile fix was previously bundled with slimming; drop it when `main` imports the trait in `vision_ok_png_b64`.

### Sunset: gix 0.86 (gix-odb #2723)

Temporary fix for the cleared-pack-slot panic in `gix-odb 0.80`, fixed in `gix-odb 0.83` / `gix 0.86`. Pin `gix = "0.87"` and fast-worktree `gix-status = "0.34"`: `gix 0.86` resolution depends on yanked `bisync 0.3`.

```sh
git show main:Cargo.toml | rg '^gix = '
```

Keep while upstream is below `0.86`; drop when upstream is `>= 0.86`. The area owns root pins, `Cargo.lock`, fast-worktree manifest, and API shims (`from_bstr(storage)` in `git/safety/git_dir.rs`). On retirement, take upstream's versions and remove the series entry, this section, and related dependency/gotcha notes.

### Sunset: sandbox settings persist

Temporary privileged writer for user-initiated `config.toml` saves under kernel write-deny (H1-3969489). Body: `xai-grok-sandbox/src/user_config_writer.rs`. Wiring: pager-bin `main`, sandbox exports/dev-dependency, shell sandbox apply and `util/config/persist.rs`, sandbox user guide, and lockfile.

Preserve these security constraints:

- Run `run_user_config_helper_if_requested` before threads. Re-exec and detach the helper before bwrap/Seatbelt; avoid multithreaded `fork`.
- Keep `config.toml` kernel write-denied to agent tools.
- Authenticate requests with magic and a 32-byte token before destination/content. Transfer the token across re-exec through `GROK_USER_CONFIG_WRITE_TOKEN_FD`; adoption reads and closes the pipe. Environment values remain visible in `/proc/<pid>/environ` even after removal.
- CLOEXEC prevents normal inheritance; the token is the auth boundary because same-uid children can duplicate descriptors through `/proc/<pid>/fd`.
- Reject config symlinks whose followed target escapes grok home. Folder trust persistence remains session-only under restrictive profiles.

```sh
git grep -n 'user_config_writer\|GROK_USER_CONFIG_WRITE_FD\|privileged writer' main -- crates/codegen/xai-grok-sandbox crates/codegen/xai-grok-shell
git show main:crates/codegen/xai-grok-pager/docs/user-guide/18-sandbox.md | rg -n 'session only|privileged|edit.*config.toml'
```

Keep while upstream saves are session-only. Drop once verified upstream behavior persists user settings under write-deny through an equivalent channel. Take upstream's implementation and docs; remove fork-only helper code, this section, the series entry, and related gotcha notes.

### Sunset: tool-layer image bridge test import

The isolated compile fix adds `use base64::Engine` in `vision_ok_png_b64` at `xai-grok-shell/src/session/acp_session_tests/tool_layer_images_bridge_tests.rs`. Drop the area once upstream brings the trait into scope or removes the need for it. Refresh the series entry in `setup okf`.

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
- Slash `builtin_commands()`: take main's ordered menu. Insert fork commands into that order (`handoff` next to `fork`, `purge` next to `delete`). Do not replay the pre-order list tail.
- `acp_agent` method match: keep main's new feedback arms (`upload-trace`) and add the `x.ai/handoff` arm beside them
- `/purge` calls `delete_session_history`; when that signature grows (e.g. `search_index`), pass `None` unless a live `SearchIndexManager` is in scope. Persistence already evicts FTS; purge then sweeps `sessions/`. `clear_directory_contents` must refuse a symlink directory and unlink symlink entries without following them
- Anything under `.okf/` if upstream ever adds the same paths (rare in public tree)
- **Startup TTFP:** `app/mod.rs` `run` spine (auth/prefetch order, paint-before-connect + discard-pending-input), `app/startup.rs` (fork-owned helpers: prefetch kick, config snapshot, frozen welcome / minimal skeleton paint), `app/event_loop.rs` AppInit preloaded-config arg, `acp/mod.rs` `connect` signature, `docs.rs` extract stamp; take main's new startup steps when possible and re-apply join-after-terminal + preloaded-config + paint-before-connect. Do **not** build `ConnectFlags` before prefetch join (`remote_settings` is not available yet). After join, set `status_line` from `ui_config_from_effective` and use main's `effective_auto_for_launch`. `ConnectFlags` field is `no_subagents` (not `subagents`). Take main's `connect_timeout` / `GROK_CONNECT_UI_TIMEOUT_SECS` resolver instead of a hardcoded 30s. Event loop: preloaded config for `launch_effective_config` (plugin-CTA marketplace needs the full root, not only `[ui]`) **and** status-line `report_config`. Carry `preloaded_layers` from `load_effective_config_with_layers` (first load, and `reload_config_after_remote` when remote settings arrived) so `subagent_model_inheritance` does not force a second merge. That function returns `EffectiveConfigLayers { layers, active_campaigns, effective }`, not a `(layers, value)` tuple. `app::run` clones `.effective` into `raw_config` and passes the struct through. AppInit's `effective_config` is already `Option<&toml::Value>`; do not call `.as_ref()` again before `load_initial_config_session_bools`. New crate `xai-grok-status-line` is take-main. Main replaced `EarlyPrefetchHandle` with process-global `startup_prefetch::begin` / `wait_settings`; `kick_auth_and_prefetch` must call `begin` without awaiting auth, and `join_early_prefetch` must call `wait_settings(EARLY_PREFETCH_WAIT)`. Apply `cache_remote_prompt_suggestions` at join time with the other remote caches.
- **MCP promote:** `McpServerConfig` lives in `xai-grok-config` `mcp_server_config.rs` (config-types `mcp.rs` is a re-export shim). Port `promote_tools`, `promoted_qualified_names`, and `collect_promoted_mcp_tool_names` onto that struct, and re-export the free function from `xai-grok-config` `lib.rs` so the shim's `pub use` resolves. Do not treat the shim as the struct definition.
- **ChatGPT model meta:** keep `acp_model_meta` and call `chatgpt::quota::stamp_provider` on that map. Do not restore the inlined meta block.
- **gix 0.86 sunset (until dropped):** root `Cargo.toml` workspace `gix` line (`0.87`), `xai-fast-worktree` `gix-status` pin (`0.34`), `Cargo.lock`. On every sync, compare `main`'s `gix` pin and drop the area at `>= 0.86` (do not keep a no-op bump).
- **sandbox settings persist (until dropped):** `user_config_writer.rs`, sandbox `lib.rs` exports, shell `apply_sandbox` / `persist.rs`, user-guide `18-sandbox.md`. On every sync, if `main` already persists user `config.toml` under write-deny, drop the area and take `main` (do not merge helpers).

## Sources

- `crates/codegen/xai-grok-agent/templates/{prompt,subagent_prompt}.md`, root `README.md` (always-keep)
- Local `main..dev` history and current upstream manifests
- [Conventions](conventions.md) (feature isolation and commit ownership)
- [Workflows](workflows.md), [Testing](testing.md) (generation and validation)
