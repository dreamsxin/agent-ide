# Agent IDE - Implementation Roadmap

> This file is the canonical source of truth for project state.
> If you resume work after an interruption, start here.

---

## Quick Recovery

After any interruption, restore context in this order:

1. Read this file to understand current state and next work.
2. Read `docs/agent_ide_plan.md` for the broader technical plan.
3. Read `docs/agent_ide_ui_design.md` for UI design intent.
4. Check `git status --short` before editing. There may be user changes.
5. Run verification:

```powershell
npm run build
cd src-tauri
cargo check
cargo test
```

---

## Project Identity

| Field | Value |
|-------|-------|
| Project | Agent IDE |
| Description | Code-centric controllable AI Agent IDE |
| Stack | Tauri v2 + Rust backend + React 18 + TypeScript + Tailwind CSS |
| Editor | Monaco Editor |
| Terminal | xterm.js + Tauri PTY (`portable-pty`) |
| File Tree | react-arborist + Tauri commands |
| State | Zustand |
| Build | Vite |
| Root | `d:\work\agent-ide` |

---

## Current State

Status as of 2026-04-30: **Phase 6 in progress - stabilization and Agent hardening**.

The app is no longer just a static UI prototype. It has a working Tauri/Rust backend, file commands, Git commands, LLM streaming, Agent planning/execution scaffolding, diff review UI, and settings for model configuration. Recent work focused on correcting safety and runtime assumptions:

- Added workspace path resolution and path-bound file operations.
- Added Agent context compression modes: `focused`, `compact`, `full`.
- Replaced unsafe Agent Markdown HTML injection with `ReactMarkdown skipHtml`.
- Restored a Tauri CSP instead of `csp: null`.
- Added browser/Tauri runtime guards so `npm run dev` can preview UI without crashing.
- Fixed Git untracked status classification.
- Fixed terminal kill path to signal the reader loop.
- Added tests for context compression behavior.

Important distinction:

- `npm run dev`: Vite web preview only. Tauri IPC, filesystem, terminal, Git, and Agent backend are disabled or stubbed.
- `npm run tauri -- dev`: real IDE runtime with Rust backend and Tauri APIs.

---

## Current Verification

Last verified locally:

```powershell
npm run build     # passes; Vite still warns about a large Monaco/Markdown chunk
cargo check       # passes
cargo test        # passes; includes context compression tests
```

Known local worktree note:

- `demo/hello.js` may contain unrelated user/demo changes. Do not revert it unless explicitly requested.

---

## Implemented Architecture

### Frontend

- `src/App.tsx`: layout, panel visibility, shortcut help, workspace restore.
- `src/components/layout/`: titlebar, left/right/bottom panels, resize handles.
- `src/components/editor/`: Monaco editor, tabs, inline suggestions, diff overlays, quick actions.
- `src/components/panels/`: Explorer, Git panel, terminal, logs.
- `src/components/agent/`: chat, tasks, diff review, role selector, pipeline, settings.
- `src/stores/`: Zustand state for layout, editor, Agent, Git, logs, theme.
- `src/hooks/`: Tauri event bridge, shortcuts, event helpers.
- `src/utils/tauri.ts`: runtime detection for Tauri-only APIs.

### Backend

- `src-tauri/src/lib.rs`: Tauri plugin setup and command registration.
- `src-tauri/src/commands/fs.rs`: workspace-scoped file operations and watcher.
- `src-tauri/src/commands/git.rs`: Git status, diff, commit.
- `src-tauri/src/commands/terminal.rs`: PTY lifecycle.
- `src-tauri/src/commands/agent.rs`: Agent commands, LLM config, context compression config.
- `src-tauri/src/agent/`: state machine, planner, executor, orchestrator, diff helpers, roles/pipeline models.
- `src-tauri/src/services/llm_client.rs`: OpenAI-compatible streaming client.
- `src-tauri/src/services/context.rs`: AgentContext and context compression.
- `src-tauri/src/services/workspace.rs`: config dir, workspace persistence, path resolution and workspace boundary checks.

---

## Key Data Flows

### Open File

```text
Explorer click
  -> useEditorStore.openFile()
    -> invoke("read_file_content")
      -> Rust fs command resolves path inside workspace
        -> editor store caches content
          -> Monaco renders active file
```

### Agent Prompt

```text
ChatView.handleSend()
  -> useAgentStore.sendPrompt()
    -> invoke("send_agent_prompt")
      -> AgentContext built from active file, selection, open files
      -> context compressed by selected mode
      -> AgentOrchestrator.run()
        -> planner LLM streaming
        -> executor LLM streaming
        -> parse model diff blocks
        -> emit plan, step, token, diff, state events
```

### Apply Diff

```text
DiffView.applyAllDiffs()
  -> invoke("apply_diffs")
    -> resolve each diff file inside workspace
    -> string-match original hunk
    -> write updated content
    -> mark applied and emit state
```

Current limitation: diff application still uses textual `find` replacement. It needs stronger conflict detection.

---

## Known Issues

### High Priority

1. **Diff application is fragile**
   - Current behavior depends on exact or trimmed string match.
   - Missing file hash/version checks.
   - Partial apply errors are collected but not fully surfaced to UI.
   - No per-hunk apply/reject.

2. **Agent cancellation is not real cancellation**
   - `stop_agent` changes state and clears data.
   - In-flight LLM requests are not cancelled with a cancellation token.

3. **Pipeline is mostly UI/state, not execution orchestration**
   - Roles exist.
   - Pipeline stages can be configured.
   - Orchestrator does not yet execute architect/coder/tester/reviewer as distinct passes.

4. **Secret storage is weak**
   - LLM API key is persisted in `~/.agent-ide/config.json`.
   - Should move to OS keychain or a permission-hardened credential store.

### Medium Priority

5. **Terminal frontend is not fully wired to PTY commands**
   - Backend PTY commands exist.
   - Frontend terminal currently behaves mostly as local xterm UI.
   - Needs spawn/write/resize/output event integration.

6. **Workspace boundary should cover every backend surface**
   - FS and Agent diff paths are guarded.
   - Git and terminal working directory policy still needs a focused review.
   - Agent CLI should align with the same policy.

7. **Runtime modes need clearer UI messaging**
   - Browser preview now avoids crashes.
   - Some panels still need explicit disabled states for web preview mode.

8. **Encoding cleanup is incomplete**
   - Many files had historical mojibake comments/text.
   - User-visible text should be cleaned progressively.

### Lower Priority

9. **Large frontend bundle**
   - Monaco, Markdown, xterm and syntax tooling create a large chunk.
   - Add dynamic imports/manual chunks later.

10. **Test coverage is thin**
   - Rust context compression has tests.
   - Need tests for workspace guard, diff apply, Git status, Agent state transitions, and frontend store behavior.

---

## Roadmap

### Phase 6 - Stabilization and Safety

Goal: make the IDE safe enough for regular local development.

Deliverables:

- Workspace boundary applied consistently across FS, Agent, Git, terminal cwd, and CLI.
- LLM key storage moved out of plain JSON or protected with strict permissions as an interim step.
- Diff application returns structured errors to UI.
- Agent cancellation token wired through orchestrator and LLM client.
- Browser preview mode has clear disabled states.
- Roadmap and docs reflect actual project state.

Acceptance checks:

```powershell
npm run build
cd src-tauri
cargo check
cargo test
```

Add focused tests:

- `workspace::resolve_existing` rejects outside paths.
- `workspace::resolve_for_write` rejects outside parents.
- `apply_diffs` reports unmatched hunks.
- `git_status` distinguishes added vs untracked.

### Phase 7 - Agent Execution Quality

Goal: turn Agent scaffolding into a reliable controllable coding loop.

Deliverables:

- Role-aware orchestration: architect -> coder -> tester -> reviewer.
- Pipeline stages influence prompts and state transitions.
- Context sources: selected files, open files, git diff, project tree summary, terminal/log excerpts.
- Context compression strategy interface:
  - `full`: complete active context.
  - `focused`: selected code and active-file excerpt.
  - `compact`: outline and metadata.
  - `budgeted`: token-budget-aware file packing.
- Structured model protocol instead of free-form markdown-only diff parsing.
- Agent action log with prompt/context/diff provenance.

Acceptance checks:

- A prompt produces visible plan stages.
- Each stage emits state and logs.
- Diff suggestions include source context metadata.
- Stop cancels active LLM streaming.

### Phase 8 - IDE Workflow Completion

Goal: make core IDE workflows practical.

Deliverables:

- Terminal fully wired to backend PTY:
  - spawn terminal
  - write input
  - resize
  - receive `terminal-output`
  - kill terminal
- QuickActions sends real Agent prompts.
- DiffView supports per-file and per-hunk apply/reject.
- Git panel supports stage, unstage, discard with confirmation.
- Tests tab reflects real test commands instead of static sample data.
- Logs panel consumes backend and Agent event streams.

Acceptance checks:

- Open workspace, edit file, save, view Git diff.
- Ask Agent for a small change, review diff, apply one hunk.
- Run terminal command and see output.

### Phase 9 - Release Readiness

Goal: make the app packageable and maintainable.

Deliverables:

- CI for TypeScript, Rust, tests, formatting.
- Tauri smoke tests for app boot, workspace open, file read/write, settings load.
- Packaging validation for Windows first.
- Security model documentation:
  - workspace policy
  - terminal permissions
  - Agent auto-edit policy
  - LLM data exposure
  - secret storage
- Troubleshooting guide for Vite vs Tauri dev modes.

---

## Technical Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-24 | Monaco `key={activeFile}` for file switching | Simple and reliable remount behavior |
| 2026-04-24 | `portable-pty` for terminal | Cross-platform PTY support |
| 2026-04-24 | react-arborist for explorer | Virtualized file tree |
| 2026-04-24 | Zustand stores | Lightweight local state model |
| 2026-04-24 | `tokio::sync::Mutex` for Agent orchestrator | Allows async lock usage |
| 2026-04-24 | reqwest SSE streaming | OpenAI-compatible LLM API support |
| 2026-04-24 | `similar` crate for diff utilities | Existing text diff support |
| 2026-04-24 | `git2::Repository::discover` | Locate Git repo from workspace paths |
| 2026-04-30 | Tauri runtime guard in frontend | Vite web preview should not crash without Tauri APIs |
| 2026-04-30 | Workspace path service | Centralized path resolution and workspace boundary checks |
| 2026-04-30 | `ReactMarkdown skipHtml` for Agent output | Avoid rendering arbitrary LLM HTML |
| 2026-04-30 | Context compression modes | Let users choose prompt context size/detail |

---

## Command Cheat Sheet

```powershell
# Web UI preview only. Backend features are disabled/stubbed.
npm run dev

# Real IDE runtime with Rust backend.
npm run tauri -- dev

# Frontend build/type check.
npm run build

# Rust verification.
cd src-tauri
cargo check
cargo test

# Agent CLI.
cd src-tauri
cargo build --bin agent_cli --release
target\release\agent_cli --help
```

---

## Next Immediate Tasks

1. Add workspace guard tests and fix any uncovered path edge cases.
2. Wire frontend terminal to backend PTY commands.
3. Replace current diff apply with structured conflict-aware apply results.
4. Add real cancellation token through Agent orchestrator and LLM client.
5. Move LLM API key storage to a safer credential path.

---

*Last updated: 2026-04-30 - Phase 6 in progress.*
