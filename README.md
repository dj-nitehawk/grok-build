<div align="center">

# Grok Build (`grok`): enhanced fork

**A leaner, faster, more controllable build of [SpaceXAI's terminal AI coding agent](https://x.ai/cli).**

Same product core. Tuned for people who live in the TUI: less binary weight, less context noise, cleaner sessions, and sharper agent defaults.

[Download the binary](#download-the-binary) ·
[Why this build](#why-this-build) ·
[What's customized](#whats-customized) ·
[Build from source](#build-from-source) ·
[Docs](#documentation)

</div>

---

## Download the binary

1. Go to **[Releases](https://github.com/dj-nitehawk/grok-build/releases)**
2. Download `grok-v*-linux-amd64.zip`
3. Unzip and put `grok` on your `PATH`

```sh
unzip grok-v1.0.0-linux-amd64.zip
chmod +x grok
sudo mv grok /usr/local/bin/   # or: mv grok ~/.local/bin/
```

> **Platform:** CI currently publishes **linux/amd64** only. Other hosts: build from source below.
> Official SpaceXAI installers (macOS / multi-platform) remain at [x.ai/cli](https://x.ai/cli); they do **not** include this fork's customizations.

On first launch, authenticate as usual (browser login or API key). See the
[authentication guide](crates/codegen/xai-grok-pager/docs/user-guide/02-authentication.md).

---

## Why this build

| | Stock Grok Build | **This fork** |
|---|---|---|
| **Binary** | Full product matrix (voice, mermaid, PDF, cloud SDKs, telemetry export) | **Slim by default**: optional surfaces compile out; sandbox enforcement stays on |
| **Startup** | Standard connect path | **Faster time-to-first-paint**: welcome paints before connect (input after backend ready); config reuse + nonblocking auth/prefetch |
| **Agent prompts** | Upstream defaults | **Custom system prompts** tuned for careful, high-signal coding work |
| **Context** | Verbose parent `spawn_subagent` tool text | **Concise hybrid description** so subagent tools cost less context every turn |
| **Sessions** | Fork copies full history | **`/handoff`** seeds a clean session with task-relevant notes only |
| **Cleanup** | Manual session hygiene | **`/purge`** (and `grok sessions purge`) wipes local session history + logs |
| **MCP** | Tools stay behind discovery helpers | **`promote_tools`** lifts chosen MCP tools into first-class model tools |
| **TUI polish** | Stock status / info line | **Custom bottom border** (quota/context chips; always-approve noise filtered) |
| **Editing** | Ctrl+Shift+Z mishandled as undo | **Redo works** in the prompt textarea |
| **Shipping** | x.ai install channel | **GitHub Releases** from tags on `dev` |

Upstream product docs and the monorepo tree still apply. This fork tracks
`origin/main` syncs and layers a short, deliberate customization series on
`dev`.

---

## What's customized

### Leaner product binary

Optional capabilities are feature-gated and **off by default** on this branch:
PDF, cloud upload SDKs, Mermaid, voice, product memory engine, workflow Rhai,
heavy telemetry export, plugin marketplace auto-update, extra image codecs, and
more. Core agent, MCP, tools, and **sandbox enforcement** stay.

```sh
cargo run -p xai-grok-pager-bin              # slim defaults
cargo check -p xai-grok-pager-bin --features product-full   # full matrix when you need it
cargo grok-slim                              # release-dist slim binary
```

### Agent behavior

- **System prompts** (`prompt.md` / `subagent_prompt.md`) rewritten for safer
  defaults, less fluff, and coding-agent workflows that match how power users
  actually work.
- **Concise parent `spawn_subagent` description**: roster + short policy in
  context; deep guide stays on disk until needed.

### Session control

- **`/handoff <task>`**: transfer *task-relevant* context into a new empty
  session (not a full history fork). Ideal when the transcript is noisy and you
  want a clean agent focused on the next goal.
- **`/purge`**: clear local session dirs and logs (config/auth/skills untouched).

### MCP control plane

Per-server allowlist to promote tools into the sampling tool list:

```toml
# ~/.grok/config.toml
[mcp_servers.github]
# …
promote_tools = ["create_issue", "list_pull_requests"]
```

Default remains discovery-only (`search_tool` / `use_tool`) for KV-cache stability.

### TUI & input

- Custom **bottom border info line** (context + mode flags + quota chips; always-approve noise filtered).
- **Alt+Q** refreshes Grok usage quota onto that border (1-minute cache; further presses reuse it).
- **Ctrl+Shift+Z redo** fixed in the prompt textarea.
- **Startup TTFP**: frozen welcome paints before connect so startup feels instant; input is live after the backend is ready.

Chip order on the prompt bottom border: **context · modes · quota**.

```text
  47K / 500K (9%)  ·  plan  ·  10% (reset: 4d5h)
  ^^^^^^^^^^^^^^^^    ^^^^     ^^^^^^^^^^^^^^^^^^
  context window      flags    usage % + time to reset
```

| State | What you see |
|---|---|
| Idle, no fetch yet | Context (and modes) only; no quota chip |
| **Alt+Q** in flight | `refreshing...` in the quota slot |
| Cached balance | `10% (reset: 4d5h)`, or just `10%` if period end is unknown |
| After cache (≤1 min) | Same chip; no network round-trip until it expires |

Quota is on-demand in this fork (no background billing poll on every prompt). Press **Alt+Q** when you want a fresh reading.

### Engineering on this tree

- **OKF** (`.okf/`) operational knowledge for agents working in-repo.
- **`release-local`** Cargo profile for fast personal installs (no thin LTO).
- **GitHub Actions** release: tag `v*` on a commit reachable from `dev` →
  linux/amd64 zip on GitHub Releases.

---

## Build from source

**Requirements**

- **Rust**: pinned by [`rust-toolchain.toml`](rust-toolchain.toml) (`rustup` installs it).
- **[DotSlash](https://dotslash-cli.com)** on `PATH` before the first build
  (hermetic `bin/protoc`):

  ```sh
  cargo install dotslash
  /usr/bin/env dotslash --help
  ```

- **protoc** via `bin/protoc` (DotSlash), or `$PROTOC` / `PATH`.

```sh
git clone https://github.com/dj-nitehawk/grok-build.git
cd grok-build
git checkout dev

cargo run -p xai-grok-pager-bin                 # build + launch TUI
cargo build -p xai-grok-pager-bin --release     # → target/release/xai-grok-pager
cargo build -p xai-grok-pager-bin --profile release-dist   # distribution-class
cargo build -p xai-grok-pager-bin --profile release-local  # fast local install
```

Cargo artifact name: `xai-grok-pager`. Release zips rename it to **`grok`**.

macOS and Linux are supported build hosts. Windows from this tree is best-effort.

## Documentation

| Resource | Location |
|---|---|
| In-tree user guide | [`crates/codegen/xai-grok-pager/docs/user-guide/`](crates/codegen/xai-grok-pager/docs/user-guide/) |
| Official product docs | [docs.x.ai/build](https://docs.x.ai/build/overview) |
| Upstream product page | [x.ai/cli](https://x.ai/cli) |
| Fork ops / slim policy | [`.okf/`](.okf/) (agents & maintainers) |

Useful guide pages for fork features: slash commands (`/handoff`, `/purge`),
MCP servers (`promote_tools`), sessions, subagents.

---

## Development

```sh
cargo check -p <crate>         # package-scoped; full workspace is slow
cargo test -p xai-grok-config  # defaults = slim on this fork
cargo clippy -p <crate>
cargo fmt --all
```

Branch model for this fork: **`main`** mirrors upstream; **`dev`** holds
customizations. See [`.okf/fork-sync.md`](.okf/fork-sync.md) before rebasing
onto new monorepo syncs.

## License

First-party code is **Apache License 2.0**. See [`LICENSE`](LICENSE).

Third-party and vendored code keeps original licenses:

- [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES)
- [`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`](crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md)
- [`third_party/NOTICE`](third_party/NOTICE)
