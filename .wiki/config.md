---
title: "Agent IDE Code Map"
description: "Local wiki holding a module-level code map of the agent-ide repository"
created: 2026-09-29
freshness_threshold: 70
---

# Wiki Configuration

## Scope

A module-level map of this repository only: what each directory and module is
responsible for, its entry-point symbols, the dependency direction between
modules, and the IPC contract between the Rust backend and the React frontend.

Out of scope: product decisions (they live in `ROADMAP.md` Known Issues), the
enforced security surface (`SECURITY.md`), and per-line behaviour. This wiki
points *at* code; it does not restate it.

## Conventions

- Articles are English, matching the repo rule in `AGENTS.md` ("Markdown docs are
  English"); only code comments are Chinese.
- Every claim carries a `path:line` citation. Line numbers are accurate as of the
  scan commit recorded in `raw/repos/2026-09-29-agent-ide-source-scan.md`; they
  drift with every edit, so trust the symbol name over the number.
- Structure is deliberately reduced: only `raw/repos/`, `wiki/topics/` and
  `wiki/references/` exist. There is no `inbox/`, `inventory/`, `datasets/` or
  `output/` — nothing here needs them yet, and empty scaffolding would lint as
  noise.
- Line counts include inline `#[cfg(test)]` modules, which are a large share of
  the big Rust files. They measure file size, not production size.
