# Testing Strategy

## Overview

Agent IDE uses a multi-layer testing approach covering unit tests, integration tests, and end-to-end runtime validation. The project is currently in Phase 8 (IDE Workflow Completion) with automated unit tests on both frontend and backend, CLI smoke coverage, and a manual smoke checklist. Full E2E automation is planned for Phase 9.

## Test Layers

### Unit Tests

#### Frontend (Vitest)

| File | What it tests |
|------|---------------|
| `src/utils/paths.test.ts` | Windows/file-URI path normalization, `file:///` URI parsing, path-to-URI conversion |
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
| `commands/git.rs` | Git status classification (added vs untracked), staged/worktree diff, branch checkout, remote branch tracking, conflict detection, conflict resolution, workspace boundary checks |
| `commands/lsp.rs` | LSP file URI encoding/decoding, Windows verbatim path normalization, indexing-state detection |
| `cli/mod.rs` | CLI argument parsing, `--allow-run` pattern matching (exact, prefix wildcard, trusted all), repair permission requirements, workspace resolution, `doctor --output json`, preview artifacts, apply artifacts, `repair-chain.json`, `smoke ide-backend` |

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
| LSP server integration | None | LSP URI/indexing helpers are tested; actual server spawn requires runtime validation |
| Tauri runtime (E2E) | Manual only | Smoke checklist in `docs/smoke_test.md` |
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

# CLI binary tests
cd src-tauri
cargo test --bin agent_cli
```

## Test Environment Notes

- **`npm run dev`**: Vite web preview only. Tauri IPC, filesystem, terminal, Git, and Agent backend are disabled or stubbed. Do not rely on this for testing backend functionality.
- **`npm run tauri -- dev`**: Real IDE runtime with Rust backend and Tauri APIs. Required for all smoke and E2E validation.
- **Rust tests** use temporary directories with UUID-based names and a mutex guard (`env_test_guard`) to prevent concurrent workspace config mutation across test threads.
- **Frontend tests** run in a JSDOM environment via Vitest and do not require a Tauri runtime.
