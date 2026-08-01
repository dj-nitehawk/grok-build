---
type: Reference
title: Maintenance
description: OKF v0.1 conformance, update triggers, and conflict handling for this repository.
tags: [maintain]
---

# Maintenance

Normative day-to-day OKF finish gates live in `AGENTS.md`. This file is the detailed inventory and conformance reminder.

## Conformance

- Reserved: `index.md` (listing/router); optional `log.md` (not used unless requested).
- Non-reserved `.okf/*.md` need YAML frontmatter with non-empty `type` (closed list), `title`, and `description`.
- Bundle-root `index.md` may only use frontmatter for `okf_version: "0.1"`.
- Allowed types: `Reference`, `Architecture`, `Playbook`, `API Endpoint`, `Database`, `Service`, `Event`, `Security`, `Deployment`, `Generated`, `ADR`.
- Prefer `## Sources` (1-5 paths) for multi-source or non-obvious claims. At most one `resource` per file.
- Compact operational summaries only; no secrets, roadmaps, or full API dumps.
- Soft target ~50-150 lines per concept file; split by topic when scanability suffers.

## Update triggers

Sync `.okf/` when changes affect:

- Architecture / crate boundaries / dependency direction
- Public product surfaces (CLI flags, ACP, headless contracts, tool surface)
- Persistence / session layout / config file names or merge rules
- Dependencies / Rust toolchain / package management / workspace members
- Build, run, test, lint, format, codegen, dist profiles
- Testing strategy or layout
- Security / auth / sandbox
- Config / env var names / ports / ops assumptions
- Conventions / repository layout
- Gotchas / do-not-edit generated paths

If unaffected, say so explicitly before finishing. Pure comment/typo/format: `OKF unaffected (non-behavioral edit)`.

## Conflicts

1. Prefer verified source, tests, generated artifacts, and manifests over OKF prose.
2. Fix OKF to match verified truth.
3. Mention the correction in the final response.

## Sources
- OKF v0.1 setup skill (format)
- `AGENTS.md` (normative gates)
