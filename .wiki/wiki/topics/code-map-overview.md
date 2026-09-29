---
title: "Code Map: Repository Overview"
category: topic
sources: [raw/repos/2026-09-29-agent-ide-source-scan.md]
created: 2026-09-29
updated: 2026-09-29
tags: [agent-ide, code-map, architecture, tauri]
aliases: [agent-ide overview, repo map, 代码地图总览]
confidence: high
volatility: warm
verified: 2026-09-29
summary: "Top-level map of the agent-ide repository: the four Rust module groups, the frontend layers, where the seam between them sits, and which files hold authority over what."
---

# Code Map: Repository Overview

> A Tauri v2 desktop Agent IDE. Rust in `src-tauri/`, React 18 + TypeScript +
> Zustand in `src/`. The product exists so a user can **see and undo** everything
> the Agent did, which is why the map has an unusual shape: the write path, the
> cancel path and the undo path each have a single named owner, and almost every
> invariant in the codebase is about one of those three.

## The shape in one picture

```
src/ (React)                     src-tauri/src/ (Rust)
  hooks/useAgentBridge  <── 16 events ── agent/events.rs (RunEvents)
  stores/useAgentStore  ──105 invokes─>  commands/*      (Tauri surface)
  components/…                              │ owns AppHandle
                                            ▼
                                        agent/          (decides; no Tauri types)
                                          orchestrator ─ executor ─ workspace_tools
                                            │
                                            ▼
                                        services/       (platform + providers)
                                          workspace · llm_client · mcp · browser
                                          capture · computer · input
```

Three rules hold that picture together, all recorded in `AGENTS.md`:

1. **`agent/orchestrator.rs` imports no Tauri types** — verified: a `grep tauri`
   over the file returns nothing, and the intent is stated at
   `src-tauri/src/agent/orchestrator.rs:22`. Emission goes through the
   `RunEvents` trait (`src-tauri/src/agent/events.rs:17`); `AppHandle` implements
   it, tests pass `RecordingEvents` (`events.rs:48`).
2. **The command layer owns the seam.** `commands/` is a *private* module
   (`src-tauri/src/lib.rs:21`) and is the only place that holds `AppHandle`,
   mints the per-run cancel flag, and registers the 105 invokable commands
   (`lib.rs:43-157`).
3. **`services/` never decides.** It resolves paths, talks to providers and the
   OS, and enforces boundaries — but a run's control flow lives in `agent/`.

## Directory responsibilities

Rust (`src-tauri/src`, 48 files, 47,908 lines at scan time; counts include inline
test modules):

- **`agent/`** — 15 files, 19,363 lines. Decides what a run does: run state
  machine and lease, model/tool loop, the built-in tool surface, diff
  generation/application, approval gating, session and external-action ledgers.
- **`services/`** — 20 files, 16,726 lines. Platform and provider edges: path
  boundaries, the LLM HTTP client and its pricing/spend metering, MCP client,
  CDP browser, window enumeration/capture/input, project memory, run log.
- **`commands/`** — 9 files, 8,298 lines. The Tauri surface plus the run-finish
  helpers. Also the only module that wires `AppHandle` into anything.
- **`cli/`** — 1 file, 3,344 lines. A headless binary (`src/bin/`, 11 lines).
  Deliberately unmapped here — see the scan record's gap list.

Frontend (`src/`):

- **`components/`** — `agent/` (chat, diff review, plan, sessions, settings,
  approval dialogs), `editor/` (Monaco host and overlays), `layout/` (window
  chrome and panel shells), `panels/` (explorer, git, terminal, tasks, problems,
  logs), `shared/` (palette, mode switch, error boundary).
- **`stores/`** — nine Zustand stores, no `persist` middleware; every
  localStorage write is hand-rolled per store.
- **`hooks/`** — bridge and bootstrap behaviour, notably `useAgentBridge`
  (event → store) and `useAppBootstrap`.
- **`types/`** — wire shapes **and their normalizers**; this is where untrusted
  input from events and localStorage is made safe.
- **`utils/`**, **`i18n/`**, **`styles/`** — pure helpers with stated invariants,
  a zh/en message table, one Tailwind entry.

## Where authority is concentrated

Five files hold most of the enforcement. Read these before changing behaviour:

- `src-tauri/src/services/workspace.rs:128` — `resolve_for_agent_write`, the
  single funnel every Agent write passes through, with deny-listing at `:158`
  and Windows-quirk normalization at `:152`.
- `src-tauri/src/agent/workspace_tools.rs:214` — `WorkspaceToolPermissions`, ten
  switch+list authority pairs, one per capability.
- `src-tauri/src/agent/orchestrator.rs:220` — `CancelRegistry`, so Stop can pull
  the switch *without* taking the lock held by the work it cancels.
- `src-tauri/src/agent/events.rs:17` — `RunEvents::emit_json`, the only exit to
  the frontend.
- `src-tauri/src/types` equivalent on the frontend: `src/types/agent.ts:29`
  (`normalizeAgentMode`) — the only sanctioned way a union-typed value crosses in
  from an event or localStorage.

## Verification gates

Five commands, all must pass (`AGENTS.md`). Nothing in this wiki has run them; it
is a map, not a build report.

```
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy --all-targets -- -D warnings
cd src-tauri && cargo test --lib
npx tsc --noEmit
npm test
```

`cargo test --lib` is the whole Rust suite — `cargo test --bin agent_cli` runs
zero tests. `npm run tauri -- dev` is the only way to exercise the real backend.

## See Also

- [[backend-module-map|Backend Module Map]] ([Backend Module Map](backend-module-map.md)) — module-by-module detail for `src-tauri/`
- [[frontend-module-map|Frontend Module Map]] ([Frontend Module Map](frontend-module-map.md)) — stores, components, bridge
- [[ipc-contract|IPC Contract]] ([IPC Contract](../references/ipc-contract.md)) — the 105 commands and 16 events, cross-checked in both directions

## Sources

- [agent-ide source scan @2b35623](../../raw/repos/2026-09-29-agent-ide-source-scan.md) — commit, method, and the deliberate gaps
