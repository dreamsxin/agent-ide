# Testing Strategy

## Overview

Agent IDE uses a multi-layer testing approach covering unit tests, integration tests, and end-to-end runtime validation. The project is currently in Phase 8 (IDE Workflow Completion) with automated unit tests on both frontend and backend, CLI smoke coverage, and a manual smoke checklist. Full E2E automation is planned for Phase 9.

## Test Layers

### Unit Tests

#### Frontend (Vitest)

| File | What it tests |
|------|---------------|
| `src/utils/paths.test.ts` | Windows/file-URI path normalization, `file:///` URI parsing, path-to-URI conversion |
| `src/hooks/useAppBootstrap.test.tsx` | Mount-time wiring: LLM config loaded on mount, loaded even with no workspace to restore, workspace restore in the Tauri runtime, and survival when the workspace lookup rejects. Runs under jsdom via a per-file docblock |
| `src/utils/terminalProblemParser.test.ts` | Terminal output parsing into Problems entries for TypeScript/lint/test-style `file:line:column` formats, Vitest/Jest-style `FAIL` summaries, and stack traces |
| `src/hooks/useLspDiagnostics.test.ts` | LSP diagnostics hook behavior — bridging LSP diagnostics into the Problems store |
| `src/stores/useProblemStore.test.ts` | Problem store behavior — adding, replacing, clearing, and deduplicating problems across diagnostic/lsp/test/agent/system sources |

Run command:

```bash
npm test
```

#### Backend (cargo test)

| Module | What it tests |
|--------|---------------|
| `services/context.rs` | Context compression modes (`full`, `focused`, `compact`, `budgeted`), token estimation, context section building |
| `agent/diff_apply.rs` | Diff apply with workspace boundary validation, new-file creation, edit replacement, ambiguous match rejection, new-file overwrite protection, partial-apply reporting, base hash stale-diff rejection, multi-hunk failure atomicity |
| `agent/orchestrator.rs` | Orchestrator state transitions (idle → thinking → planning → acting → reviewing → waiting_user → done), pipeline sequencing |
| `agent/multi_agent.rs` | Pipeline role execution, role prompt construction, stage status management |
| `services/workspace.rs` | Workspace path resolution (`resolve_existing`, `resolve_for_write`), boundary enforcement (`ensure_within_workspace`), Windows verbatim path normalization (`shell_compatible_path`), relative traversal rejection |
| `services/llm_profiles.rs` | Legacy config migration, profile serialization, API key masking, credential reference handling |
| `services/problem_parser.rs` | Backend command-output problem parsing for structured error extraction |
| `commands/git.rs` | Git status classification (added vs untracked), staged/worktree diff, repositories with no commits, branch checkout, remote branch tracking, conflict detection, conflict resolution, workspace boundary checks |
| `commands/agent.rs` | Context-compression precedence (request override vs stored default, unknown mode rejected). This file is the IPC boundary and most of it still needs a running app; logic is being pulled out into services rather than tested in place — see below |
| `services/verification.rs` | Repair-prompt construction, output truncation, `--allow-run` pattern matching, long-running command detection, verification candidate preparation (blank trimming, long-running partition, the two distinct failure messages), batch check execution when one command cannot run, action-log level/summary/detail rendering, bounded repair loop policy (iteration numbering, budget exhaustion, apply failure, repair-requires-apply) |

| `services/llm_client.rs` | Provider request shaping, mock provider tool calls, run token accounting, usage action-log wording for unknown / partially reported / fully reported usage |

| `commands/lsp.rs` | LSP file URI encoding/decoding, Windows verbatim path normalization, indexing-state detection |
| `cli/mod.rs` | CLI argument parsing, `--allow-run` pattern matching (exact, prefix wildcard, trusted all), repair permission requirements, `--allow-agent-write` requiring `--apply`, workspace resolution, `doctor --output json`, preview artifacts, apply artifacts, `repair-chain.json` for both a repair that succeeds and one that exhausts its budget, `tool-writes.json`, `smoke ide-backend` |


Run command:

```bash
cd src-tauri
cargo test
```

### Integration Tests (Planned — Phase 9)

```
tests/integration/
  lsp.spec.ts           - LSP server spawn, diagnostics, completions, hover, definition, rename, code actions
  problems.spec.ts      - All 4 diagnostic sources (diagnostic, lsp, test, agent) feed unified Problems panel
  agent_pipeline.spec.ts - Full pipeline execution with mock LLM: planner → architect → coder → tester → reviewer
  git_workflow.spec.ts   - Status, stage, unstage, commit, branch, fetch, pull, push cycle with workspace scoping
```

These tests will exercise the Tauri IPC boundary end-to-end using a mock LLM provider, validating that frontend state updates correctly when backend commands complete.

### End-to-End Tests (Planned — Phase 8/9)

```
tests/e2e/
  boot.spec.ts          - App launches, main window renders, workspace restores
  workspace.spec.ts     - Open folder, file tree populates correctly
  editor.spec.ts        - Open file, edit, save, undo, LSP diagnostics appear in Problems
  terminal.spec.ts      - Spawn shell, run command, see output, kill terminal
  git.spec.ts           - Status, stage, commit, branch, conflict resolution cycle
  agent.spec.ts         - Send prompt, receive streaming output, review diff, apply hunk
```

**Framework:** tauri-driver + WebDriver protocol

- Windows: native display
- Linux: Xvfb virtual display
- macOS: native display

See `docs/smoke_test.md` sections 2–10 for the full manual checklist that these E2E tests will automate.

### Windows Desktop Workflow E2E

`npm run e2e:workflow` is the Phase 10.0 local golden-path gate on Windows. It uses a small PowerShell/Win32/UI Automation controller instead of browser preview automation, launches the real Tauri debug application, and drives the daily IDE loop through the visible desktop window. The test prepares an isolated clone of this repository, seeds a failing `workflow` command and `smoke.txt`, configures `mock://workflow`, then verifies failure → Problems → Fix with Agent → Diff hunk apply → rerun success → Git commit.

`npm run verify:workflow` runs the baseline build/tests/checks and then this E2E workflow.

## CLI Smoke Coverage

The Agent CLI has automated smoke coverage independent of the desktop UI:

- `agent_cli doctor --output json` — validates workspace resolution, profile lookup, and capability reporting
- Preview artifacts — context estimation and plan generation with mock provider
- Apply artifacts — diff parsing, apply, and structured error reporting
- `repair-chain.json` — full repair loop with mock provider: failed command → parsed Problems → repair prompt → diff → apply → rerun → repair-chain traceability
- `smoke ide-backend` — validates workspace resolution, package script discovery, command runner, terminal-like Problems parsing, repair prompt construction, diff parsing, apply, rerun, and repair-chain artifacts without launching the desktop UI

Run command:

```bash
cd src-tauri
cargo test --bin agent_cli
```

## CI Pipeline (Target — Phase 9)

```yaml
jobs:
  check:
    - npm run lint
    - npx tsc --noEmit
    - npm test (Vitest)
    - cargo check
    - cargo test
    - cargo clippy
  smoke:
    - npm run tauri -- build
    - Run tauri-driver E2E suite
  package:
    - Windows MSI/NSIS (scripts/package-windows.ps1)
    - macOS .dmg (planned)
    - Linux AppImage (planned)
```

Current CI status: GitHub Actions workflow (`windows-package.yml`) handles Windows packaging only. Full check/smoke/package pipeline is a Phase 9 deliverable.

## Current Coverage Status

| Area | Coverage | Notes |
|------|----------|-------|
| Path normalization | Good | Frontend Vitest + Rust workspace tests; Windows verbatim path handling covered |
| Terminal problem parsing | Good | Frontend Vitest + backend `problem_parser.rs` tests |
| Context compression | Good | Rust unit tests for `full`, `focused`, `compact`, `budgeted` modes |
| Diff apply / boundary | Good | Rust unit tests for apply, reject, partial failure, base hash, workspace boundary |
| Orchestrator / pipeline | Good | Rust unit tests for state transitions and role execution |
| Workspace boundary | Good | Rust unit tests for `resolve_existing`, `resolve_for_write`, traversal rejection |
| Git operations | Good | Rust unit tests for status, diff, branch, conflict, boundary checks |
| LLM profile handling | Good | Rust unit tests for migration, masking, serialization |
| CLI smoke coverage | Good | Automated `doctor`, preview, apply, repair-chain, `ide-backend` tests |
| Agent state transitions | Thin | Needs more coverage for edge cases (error recovery, cancelled states, interrupted sessions) |
| Frontend store behavior | Thin | Needs more coverage for Agent event bridging, diff status updates, Problem deduplication |
| Monaco diagnostics bridge | None | Requires real Monaco/Tauri runtime; currently manual smoke only |
| Mount-time wiring | Thin | `useAppBootstrap` is covered (jsdom + React Testing Library). Every other component's mount behaviour is still unobserved — this is the gap that let the startup "LLM Not Configured" bug ship |
| IDE panel backends | Good | `agent_cli smoke ide-surface` probes workspace, project tasks, verification candidates, Git status/diff, and context packing through the same functions the desktop calls; runs in CI as both an in-process test and the real binary |
| LSP server integration | None | LSP URI/indexing helpers are tested; actual server spawn requires runtime validation |
| Tauri runtime (E2E) | Manual only | Smoke checklist in `docs/smoke_test.md`, plus `npm run e2e:workflow` — which **requires an exclusive interactive desktop**, see below |
| LSP indexing at scale | None | Pending Phase 8.5/9 runtime validation on large TypeScript/Go workspaces |
| Windows credentials | Manual only | Pending cross-OS runtime validation of `keyring` crate behavior |
| SSH Git remote operations | Manual only | SSH/passphrase UX requires manual validation |

## Manual Smoke Test

See `docs/smoke_test.md` for the current manual verification checklist covering 13 sections:

1. Baseline verification (automated)
2. Runtime mode
3. Language server status
4. Diagnostics → Problems → editor markers
5. Quick Fix and code actions
6. Commands, Run History, and Problems
7. Terminal runtime
8. Git workflow
9. Agent repair loop
10. End-to-end daily IDE loop
11. LLM profiles and budget metadata
12. Large workspace LSP indexing
13. Release smoke notes template

This will be automated in Phase 9 using tauri-driver.

## Test Commands

```bash
# Frontend unit tests
npm test

# Backend unit tests
cd src-tauri
cargo test

# Frontend build verification
npm run build

# Rust type and compilation check
cd src-tauri
cargo check

# Full dev runtime verification
npm run tauri -- dev

# Windows desktop golden-path E2E
npm run e2e:workflow

# Baseline + Windows desktop golden-path E2E
npm run verify:workflow

# CLI binary tests
cd src-tauri
cargo test --bin agent_cli
```

## Test Environment Notes

- **`npm run dev`**: Vite web preview only. Tauri IPC, filesystem, terminal, Git, and Agent backend are disabled or stubbed. Do not rely on this for testing backend functionality.
- **`npm run tauri -- dev`**: Real IDE runtime with Rust backend and Tauri APIs. Required for all smoke and E2E validation.
- **Rust tests** use temporary directories with UUID-based names and a mutex guard (`env_test_guard`) to prevent concurrent workspace config mutation across test threads.
- **Frontend tests** run in Vitest's default node environment and do not require a Tauri runtime. `src/hooks/useAppBootstrap.test.tsx` is the exception: it opts into jsdom with a `// @vitest-environment jsdom` docblock because it renders a hook. The global environment is deliberately left as node — only files that need a DOM pay for one.

## Desktop workflow E2E (`npm run e2e:workflow`)

`scripts/e2e/` builds the frontend and the debug Tauri binary, launches the real app against the
`mock://workflow` provider, and drives it through UI Automation: run the failing project command,
open Problems, click Fix, apply the Agent's hunk, rerun the command, then commit through the Git
panel. It asserts file content and Git status, and captures screenshots at each step.

**It requires an exclusive interactive desktop.** The controller must bring the app window to the
foreground; Windows silently refuses `SetForegroundWindow` when another process owns the
foreground, and the harness then has no trustworthy path forward: screenshots capture the primary
screen rather than the app window, and the `Click-Element` fallback sends keystrokes to whichever
window does hold focus. So:

- It cannot run in CI. GitHub runners have no interactive desktop session.
- It cannot run on a machine someone is using. It takes over the foreground window, the mouse
  position and the clipboard, and a stray `{ENTER}` can land in an unrelated application.
- A run under RDP, on a locked session, or alongside another focused window is invalid, not failed.

The harness now detects this and fails immediately at an `environment` step with that explanation.
It previously discarded the `SetForegroundWindow` result, clicked into the void, and reported
`Element 'workflow' not found` 30 seconds later — an environment problem disguised as a product
defect.

Treat this as a **manually triggered check on an idle machine**, not a regression net. The
automated regression net for backend behaviour is `agent_cli` (`smoke ide-backend`,
`smoke ide-surface`), which needs no desktop and does run in CI.

The native tool loop is covered headlessly instead. `mock://` providers can now emit a tool call on
demand (`AGENT_IDE_MOCK_TOOL` / `AGENT_IDE_MOCK_TOOL_ARGS`), and `agent_cli` exposes the workspace
verification tool when `--allow-run` is given, so `cli::tests::smoke_tool_loop_*` drive the whole
chain — model emits a tool call, the executor runs the command, the result returns as a `tool`
message, the next round produces the diff. The two tests are a differential pair: identical runs
that differ only in `--allow-run`, asserting the command's side effect appears in one and not the
other. That distinguishes "the tool ran" from "the round completed", which a diff-only assertion
cannot.

The write tool has the same shape of coverage under `--allow-agent-write`
(`cli::tests::smoke_write_tool_*`): one run asserts the file on disk holds what the model wrote and
that `tool-writes.json` records it, its pair asserts the file is untouched without the flag, and a
third asserts the flag is rejected without `--apply`. Asserting the file contents rather than the
tool's return string is the point — a tool can report success and still not have written anything.

Still not covered by the desktop harness: its profile uses `toolCallMode = "text_protocol"`, so
`npm run e2e:workflow` continues to exercise the text protocol rather than the tool loop. The
desktop's own write path (tool writes becoming applied diff cards with an undo checkpoint) is
covered at the orchestrator level, not end to end through the UI.

One trap worth knowing about, since it produced a false green for a while: `npm run e2e:workflow`
copies the repository into `artifacts/e2e/workflow/<timestamp>/workspace/`, test files included.
Vitest's default `include` picked those copies up, so `npm test` was running frozen snapshots of old
code alongside the real suite — 43 files instead of 10 — and a failure there would have pointed at
history rather than at the working tree. `vite.config.ts` now pins `test.include` to `src/**`.

## The command layer

`commands/agent.rs` is the largest untested file in the backend. It cannot be tested in place: every
interesting function takes `AppHandle` or `tauri::State`, which only exist inside a running app. The
approach is therefore not "write tests for the command layer" but **move the decisions out of it**,
leaving adapters that are too thin to be worth testing.

Two rules that come out of doing this:

- A signature that mentions `State<T>` when the body only reads one field is a testability bug, not a
  style question. `resolve_context_compression` took `State<AgentGlobalState>` to read a single
  mutex; taking that mutex directly made the precedence rule (request override beats stored default,
  unknown mode is an error) testable with no fixture at all.
- Wording that encodes a judgement belongs with the data it describes, not with the emitter. The
  three usage-log branches (usage unknown / partially reported / fully reported) exist because
  partial reporting makes the per-run cap undercount — that distinction is the whole point, and it
  lived inside a function that needed `AppHandle` to call. It is now
  `RunUsageSnapshot::action_log_summary`.

What remains behind the Tauri dependency in that file, deliberately for now: the resumed-pipeline
meter rule in `continue_agent_pipeline`, and event emission order within each branch. The one
invariant that used to live only in a comment — "register tool writes on every exit path" — is now
structural instead: all nine exit branches across the three run commands call a single
`finish_agent_run`, which does finish-run, publish-writes and usage accounting in that order. Writing
that helper is what surfaced the drift it was meant to prevent: `run_agent_step`'s cancel and error
branches had never emitted the usage log, so a step cancelled mid-run left no record of the tokens it
had already spent.



