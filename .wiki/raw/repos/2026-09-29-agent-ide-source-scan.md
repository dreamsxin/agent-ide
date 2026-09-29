---
title: "agent-ide source scan @2b35623"
source: "d:\\work\\agent-ide (git 2b35623)"
type: repos
ingested: 2026-09-29
tags: [agent-ide, code-map, provenance, tauri, rust, react]
summary: "Provenance record for the module-level code map: which commit was read, how it was read, and what was deliberately not read."
collection: "agent-ide-repo"
adapter: git
upstream_id: "d:\\work\\agent-ide"
upstream_type: git-file
revision: "2b35623"
content_format: markdown
license: "see LICENSE in repo root"
---

# agent-ide source scan @2b35623

## What was scanned

- Repository: `d:\work\agent-ide`, branch `main`, commit `2b35623`
  ("Design: alert an unattended run, and where the seam should go"),
  authored 2026-09-28. Scanned 2026-09-29.
- Working tree was not clean-checked before scanning, so the map reflects the
  tree as it sat on disk, which may include uncommitted edits.
- Method: four parallel read-only exploration passes — Rust backend modules,
  React frontend modules, the command layer plus event names, and the remaining
  `services/` modules — each required to cite `path:line` from files actually
  opened. No code was modified.

## Sizes recorded at scan time

Rust, `src-tauri/src`, 48 `.rs` files, 47,908 lines total (counts include inline
`#[cfg(test)]` modules):

- `agent/` — 15 files, 19,363 lines
- `services/` — 20 files, 16,726 lines
- `commands/` — 9 files, 8,298 lines
- `cli/` — 1 file, 3,344 lines
- `bin/` — 1 file, 11 lines
- roots — `lib.rs` 160, `main.rs` 6

Largest single files: `agent/orchestrator.rs` 6,624 · `agent/workspace_tools.rs`
5,790 · `services/llm_client.rs` 4,391 · `commands/agent.rs` 3,840 ·
`cli/mod.rs` 3,344 · `agent/executor.rs` 2,614.

## Known gaps in this scan

- `services/` tail modules were only partially enumerated: `agent_runtime`,
  `context`, `credentials`, `images`, `problem_parser`, `project_memory`,
  `run_log`, `verification`, `command_text`, `web_fetch` are named from
  `src-tauri/src/services/mod.rs:1-23` but were not individually read. The
  backend article marks them as unread rather than guessing.
- `cli/mod.rs` (3,344 lines) was not mapped. `AGENTS.md` records that a
  fabricated CLI help block has already shipped once as documentation drift, so
  it is left blank here deliberately.
- Frontend `components/editor/` internals beyond `monacoGlobals.ts` /
  `EditorContainer.tsx`, and `src/utils/` file-by-file, were enumerated by name
  only.
- No verification gate was run as part of this scan (`cargo fmt --check`,
  `cargo clippy`, `cargo test --lib`, `npx tsc --noEmit`, `npm test`). Nothing in
  this wiki asserts that the tree builds.
