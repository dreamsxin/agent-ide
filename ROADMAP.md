# Agent IDE - Implementation Roadmap

> This file is the canonical source of truth for project state.
> If you resume work after an interruption, start here.

---

## Quick Recovery

After any interruption, restore context in this order:

1. Read this file to understand current state and next work.
2. Read `README.md` for setup, runtime modes, and workflow overview.
3. Read `docs/agent_ide_design.md` for detailed current design.
4. Read `docs/agent_ide_ui_design.md` for UI design intent.
5. Read `docs/smoke_test.md` before changing LSP, Problems, Terminal, Git, or Agent diff application.
6. Check `git status --short` before editing. There may be user changes.
7. Run verification:

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

Status as of 2026-05-15: **Phase 7 in progress - Agent execution quality and auditability**.

The app is no longer just a static UI prototype. It has a working Tauri/Rust backend, file commands, Git commands, LLM streaming, Agent planning/execution scaffolding, diff review UI, and settings for model configuration. Recent work focused on correcting safety and runtime assumptions:

- Added workspace path resolution and path-bound file operations.
- Added Agent context compression modes: `focused`, `compact`, `full`.
- Replaced unsafe Agent Markdown HTML injection with `ReactMarkdown skipHtml`.
- Restored a Tauri CSP instead of `csp: null`.
- Added browser/Tauri runtime guards so `npm run dev` can preview UI without crashing.
- Fixed Git untracked status classification.
- Fixed terminal kill path to signal the reader loop.
- Added tests for context compression behavior.
- Added a shared diff apply module with conflict detection for missing/ambiguous hunks, new-file overwrite protection, partial-apply reporting, and CLI/orchestrator reuse.
- Wired Agent cancellation checks through the LLM request and streaming read path.
- Scoped Git commands and terminal cwd to the saved workspace boundary.
- Wired the frontend terminal panel to Tauri PTY spawn/write/resize/output events.
- Added focused tests for workspace traversal, diff apply failures, auto-apply partial failure, and Git status/workspace boundaries.
- Surfaced structured diff apply failures inline on the affected diff cards in addition to the summary banner.
- Wired the configured Agent pipeline into backend execution as role-aware stages: planner -> architect -> coder -> tester -> reviewer.
- Added structured Agent action log events for prompt, planner, stage start/completion/error, diff readiness, and auto-apply.
- Surfaced Agent action logs in the Logs panel with expandable details, context summaries, diff summaries, stage, role, and phase.
- Fed actual pending diff summaries into the Reviewer stage so review is based on proposed file/hunk changes, not only prior text output.
- Added `docs/agent_ide_design.md` as the detailed design document for workflows, context handling, Agent orchestration, and technical boundaries.
- Added backend Agent context enrichment with bounded project tree summaries and Git working-tree diff excerpts.
- Added a compatible structured `agent-changes` JSON output protocol for model file changes while preserving legacy diff/new-file block parsing.
- Fixed terminal PTY input handling by keeping a persistent writer per terminal instance instead of taking a new writer for each keystroke.
- Improved terminal startup feedback and guarded resize fitting when the panel has no measurable size.
- Added `README.md` with setup, runtime modes, verification, Agent workflow, protocol, and project status.
- Added optional `baseHash` metadata to structured Agent diffs and reject stale edit diffs when the file content hash no longer matches.
- Added `README.zh-CN.md` as the Chinese project README and linked it from the English README.
- Surfaced diff `baseHash` metadata in the Diff view and added stale-diff guidance when hash validation fails.
- Added Git file management commands and UI actions for stage, unstage, and discard through a file context menu.
- Added per-file diff apply/reject commands and Diff view controls so reviewers can accept or reject individual pending files instead of only applying or rejecting the full batch.
- Added Monaco local code completion provider for stable low-latency suggestions from language keywords, current-file symbols, snippets, and open file paths.
- Wired editor QuickActions to real Agent prompts with active file content, selection, and selected line range.
- Replaced static bottom-panel test/action samples with a Problems panel and store for diagnostics, Agent findings, and future test failures.
- Synced Monaco model markers into the Problems panel and made problem rows jump to the affected file location.
- Parsed terminal TypeScript/lint/test-style file-position errors into the Problems panel for click-through navigation.
- Added a Project Tasks panel for build, test, lint, run, and debug commands that queue into the integrated terminal.
- Added workspace task discovery from `package.json` scripts and Cargo manifests, with fallback default tasks when no project tasks are found.
- Moved common project Run/Debug/Build/Test commands into the TopBar and renamed the bottom task list to Commands to keep Agent Tasks distinct.
- Expanded terminal test-output parsing for Vitest/Jest-style failures, stack traces, and `FAIL` file summaries so `npm run test` failures can surface in Problems.
- Added non-interactive project task runner for build/test/lint/check commands with exit code, duration, Logs integration, and Problems parsing.
- Unified TopBar and Commands panel project command execution through a shared runner/terminal routing hook, so build/test/lint/check feed Logs and Problems consistently while run/debug stay interactive in Terminal.
- Fixed workspace switching for Commands and Terminal by passing the active frontend workspace path into task discovery, task execution, and terminal spawn instead of relying only on previously persisted backend workspace state.
- Normalized Windows verbatim workspace paths before spawning Terminal or project task shells so `cmd.exe` starts in `D:\...` paths instead of rejecting `\\?\D:\...` as a UNC path.
- Kept the integrated Terminal mounted across bottom-tab switches and bottom-panel hide/show so switching to Commands, Problems, or Logs does not kill and recreate the PTY session.
- Added IDE runtime failure context injection for Agent prompts, including the latest failed project command, parsed Problems, recent Terminal output, and recent warning/error Logs.
- Added one-click `Fix with Agent` actions in Problems and failed Commands, reusing the same IDE runtime failure context for structured repair prompts.
- Hardened Explorer context menu behavior so it closes on outside pointer interactions, Escape, scroll, and blur, and clamps menu placement inside the viewport.
- Added non-interactive project command run history with per-run status, exit code, duration, output details, rerun, clear history, and failed-run `Fix with Agent` actions.
- Added Terminal multi-session UI with session tabs, new session, close, restart, and active cwd/profile display while keeping inactive PTY views mounted.
- Routed Run/Test/Debug-style project commands into dedicated Terminal sessions so long-running or interactive commands do not overwrite the main shell.
- Added tracked Terminal task completion using per-session exit markers, so Test/Run-style commands opened in Terminal still update command status, run history, Problems, Logs, and Agent failure context.
- Added TypeScript/JavaScript semantic editor defaults through the Monaco TS worker, including worker-backed diagnostics, hover/completion behavior, F12 definition action, and stable file-backed Monaco models for open files.
- Added per-hunk diff review controls with backend `apply_diff_hunk` and `reject_diff_hunk` commands, hunk status tracking, and Diff view Apply/Reject hunk actions.
- Added Git staged/worktree/all diff modes plus Source Control multi-select batch Stage, Unstage, Discard, and workspace-path-aware status loading.
- Normalized Windows verbatim `\\?\D:\...` workspace paths across workspace resolution and Git repo path handling so Git status/diff no longer misreports active workspaces as outside the workspace.
- Added Git branch checkout/create, Fetch/Pull/Push actions, upstream/ahead/behind display, and conflict file detection in Source Control.
- Added one-shot Git credential inputs for remote actions, remote branch checkout/tracking, and conflict resolution controls for accept current, accept incoming, accept both, and conflict diff navigation.
- Added TypeScript LSP status details in the TopBar, including server path, workspace, opened document count, change count, diagnostics count, last error, and recent per-file diagnostics summaries.
- Routed Monaco Quick Fix/code actions through an explicit apply command that logs success/failure, syncs editor store state, and triggers LSP `didChange` so Problems and markers refresh after fixes.
- Enabled JavaScript semantic diagnostics in Monaco TS worker defaults instead of syntax-only JavaScript validation.
- Added frontend Vitest coverage for Windows/file-URI path normalization and terminal output problem parsing.
- Routed build/test/lint/check-style project commands through the non-interactive command runner so Test also records exit code, duration, output, Problems, Logs, and failed-run Agent repair context.
- Added `docs/smoke_test.md` as the real Tauri runtime regression checklist for LSP, Problems, Quick Fix, Commands/Run History, Terminal, Git, and Agent repair loops.
- Added LLM provider profiles with backward-compatible legacy config migration, Settings profile management, Chat-level profile selection, and per-run context compression mode selection.
- Added per-profile model budget metadata for max context tokens, reserved output tokens, and max output tokens, with Chat showing an estimated effective input budget.

Important distinction:

- `npm run dev`: Vite web preview only. Tauri IPC, filesystem, terminal, Git, and Agent backend are disabled or stubbed.
- `npm run tauri -- dev`: real IDE runtime with Rust backend and Tauri APIs.

---

## Current Verification

Last verified locally:

```powershell
npm run build     # passes; Vite still warns about a large Monaco/Markdown chunk
npm test          # passes; covers path normalization and terminal problem parsing
cargo check       # passes
cargo test        # passes; includes context, workspace, diff apply, orchestrator, pipeline, action-log support, and Git tests
```

Known local worktree note:

- `demo/hello.js` may contain unrelated user/demo changes. Do not revert it unless explicitly requested.

---

## Implemented Architecture

### Frontend

- `src/App.tsx`: layout, panel visibility, shortcut help, workspace restore.
- `src/components/layout/`: titlebar, left/right/bottom panels, resize handles.
- `src/components/editor/`: Monaco editor, tabs, inline suggestions, diff overlays, quick actions.
- `src/components/editor/DiagnosticsBridge.tsx`: syncs Monaco diagnostics into Problems.
- `src/components/editor/ProblemsMarkerBridge.tsx`: mirrors runtime/test/Agent/system Problems into Monaco markers and active-line decorations.
- `src/utils/codeCompletion.ts`: local completion candidate extraction for Monaco suggestions.
- `src/utils/lspClient.ts`: frontend bridge for TypeScript LSP hover, completion, definition, symbols, rename, code actions, diagnostics, and status snapshots.
- `src/components/panels/`: Explorer, Git panel, terminal, logs.
- `src/components/panels/ProblemsPanel.tsx`: unified Problems view for diagnostics, test failures, and Agent findings.
- `src/components/panels/TasksPanel.tsx`: project command list for discovered build/test/lint/run/debug commands.
- `src/hooks/useProjectTasks.ts`: shared frontend task discovery hook for TopBar and Commands.
- `src/hooks/useRunProjectTask.ts`: shared project command executor that routes build/lint/check/typecheck through the non-interactive runner and run/test/debug-style commands through dedicated Terminal sessions.
- `src/stores/useTaskStore.ts`: queued terminal command/session state, latest task state, run history, and recent terminal output for project tasks.
- `src/utils/terminalProblemParser.ts`: parses terminal output into Problems entries for common file:line:column formats.
- `src/components/agent/`: chat, tasks, diff review, role selector, pipeline, settings.
- `src/stores/`: Zustand state for layout, editor, Agent, Git, logs, theme.
- `src/hooks/`: Tauri event bridge, shortcuts, event helpers.
- `src/utils/tauri.ts`: runtime detection for Tauri-only APIs.

### Backend

- `src-tauri/src/lib.rs`: Tauri plugin setup and command registration.
- `src-tauri/src/commands/fs.rs`: workspace-scoped file operations and watcher.
- `src-tauri/src/commands/git.rs`: Git status, diff, commit.
- `src-tauri/src/commands/tasks.rs`: project task discovery from workspace configuration.
- `src-tauri/src/commands/terminal.rs`: PTY lifecycle.
- `src-tauri/src/commands/agent.rs`: Agent commands, LLM config, context compression config.
- `src-tauri/src/agent/`: state machine, planner, executor, orchestrator, diff helpers, roles/pipeline models.
- `src-tauri/src/services/llm_client.rs`: OpenAI-compatible streaming client.
- `src-tauri/src/services/context.rs`: AgentContext and context compression.
- `src-tauri/src/services/workspace.rs`: config dir, workspace persistence, path resolution and workspace boundary checks.
- `README.md`: setup, runtime modes, verification, Agent workflow, and current limitations.
- `README.zh-CN.md`: Chinese setup, workflow, protocol, and project status overview.
- `docs/smoke_test.md`: manual and automated smoke checklist for daily-IDE replacement workflows.

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

### Code Completion

```text
Monaco completion trigger
  -> EditorContainer registered completion provider
    -> buildLocalCompletionCandidates()
    -> Monaco TypeScript worker handles TypeScript/JavaScript semantic suggestions
    -> local provider handles non-TS languages with language keywords + snippets
      -> current model identifiers
      -> open file paths for path-like prefixes
    -> Monaco suggestions list
```

### Quick Actions

```text
Editor selection
  -> QuickActions Explain/Fix/Refactor/Optimize
    -> build Agent prompt with active file, selection, and line range
    -> open Agent panel
    -> send_agent_prompt()
      -> normal Agent planning/review/diff flow
```

### Problems

```text
Monaco markers or Agent error/diff failure events
  -> DiagnosticsBridge or useAgentBridge
    -> useProblemStore
      -> ProblemsPanel
        -> file click opens and reveals the affected location

Terminal command output
  -> terminalProblemParser
    -> useProblemStore.replaceProblems("test")
      -> ProblemsPanel
        -> Fix with Agent sends a focused repair prompt with current runtime context

Project Tasks
  -> discover_project_tasks(active workspace path)
    -> package.json scripts + Cargo manifests
    -> fallback defaults when no tasks are discovered
  -> TopBar common Run/Debug/Build/Test buttons or Commands panel
    -> shared useRunProjectTask routing
    -> non-interactive runner for build/test/lint/check/typecheck
      -> Logs + Problems + task status
      -> failed command card exposes Fix with Agent
      -> run history stores exit code, duration, and output details
    -> dedicated Terminal sessions for run/test/debug-style tasks
      -> Terminal output is parsed into Problems and retained for Agent runtime context
      -> task exit marker records exit code, duration, output, history, and failed-run Fix with Agent context
```

### Agent Prompt

```text
ChatView.handleSend()
  -> useAgentStore.sendPrompt()
    -> append IDE runtime context from failed tasks, Problems, Terminal output, and Logs
    -> invoke("send_agent_prompt")
      -> AgentContext built from active file, selection, open files
      -> context enriched with project tree and Git working-tree diff when available
      -> context compressed by selected mode
      -> AgentOrchestrator.run()
        -> planner LLM streaming
        -> role-aware pipeline execution: architect -> coder -> tester -> reviewer
        -> emit structured action-log events
        -> reviewer receives pending diff summary
        -> parse model diff blocks
        -> emit plan, step, token, diff, pipeline, state events
```

### Apply Diff

```text
DiffView.applyAllDiffs() or per-file Apply
  -> invoke("apply_diffs") or invoke("apply_diff", diffId)
    -> resolve target diff files inside workspace
    -> validate optional baseHash for stale edits
    -> string-match original hunks
    -> write updated content
    -> mark applied/failed and emit state
```

Current limitation: diff application still uses textual `find` replacement. It needs stronger conflict recovery and clearer mixed hunk status semantics.

---

## Known Issues

### High Priority

1. **Diff application still lacks version-aware hunks**
   - Current behavior still depends on exact or trimmed textual hunk content.
   - Now rejects ambiguous matches and refuses to overwrite existing files for new-file hunks.
   - Missing file hash/version checks.
   - Partial apply errors are returned structurally and shown inline on failed diff cards.
   - Per-file and per-hunk apply/reject are now wired in the backend and Diff view.
   - Mixed applied/rejected hunk state currently closes the file diff; add clearer partial status next.

2. **Agent protocol is still markdown/diff-block based**
   - Pipeline stages now drive backend execution.
   - Reviewer receives pending diff summaries.
   - Model outputs can now use structured `agent-changes` JSON blocks.
   - Legacy free-form markdown diff blocks are still supported.
   - Optional `baseHash` validation now rejects stale edit diffs.
   - Need stricter schema enforcement, operation metadata, and richer provenance.

3. **Secret storage is weak**
   - LLM API key is persisted in `~/.agent-ide/config.json`.
   - Should move to OS keychain or a permission-hardened credential store.

4. **Cancellation is cooperative, not transport-abort based**
   - `stop_agent` now reaches the LLM request/stream loop quickly through a shared flag.
   - The underlying HTTP request is dropped by `tokio::select!`, but there is no explicit provider-side cancellation API.

### Medium Priority

5. **Terminal PTY integration needs runtime polish**
   - Frontend now spawns, writes, resizes, and listens for PTY output through Tauri.
   - Persistent PTY writer is now used for terminal input.
   - Project tasks can open run/test/debug-style commands in dedicated terminal sessions.
   - Task terminal sessions now record exit markers into Commands history with status, duration, exit code, output, Problems parsing, Logs, and Agent failure context.
   - Build/lint/check/typecheck tasks can run through a non-interactive command runner with exit code and duration.
   - TopBar and Commands panel now use the same project command execution path.
   - Terminal spawn now receives the active frontend workspace path, so newly opened terminals start in the currently opened workspace.
   - Windows shell startup strips `\\?\` verbatim prefixes before passing cwd to `cmd.exe`.
   - Bottom tab switches no longer unmount Terminal or reset the PTY session.
   - Multi-session UI, restart, close, and cwd/profile display are now present.
   - Needs interactive runtime testing in `npm run tauri -- dev` across shell startup, panel hide/show, workspace switching, and long-running commands.

6. **Git workflow needs continued polish**
   - File-level stage, unstage, discard, diff, and commit are wired.
   - Staged-vs-worktree/all diff views and multi-select batch actions are wired.
   - Branch checkout/create, fetch, fast-forward-only pull, push, and conflict file detection are wired.
   - One-shot credential input, remote branch checkout/tracking, and basic conflict resolution controls are wired.
   - Needs persistent credential storage, better SSH/passphrase UX, richer merge editor UI, and safer destructive-action UX.

7. **Workspace boundary coverage needs continued review**
   - FS, Agent diff paths, Git entry points, terminal cwd, task cwd, and Agent CLI are now guarded or aligned.
   - Windows verbatim path prefixes are normalized centrally in the workspace service and Git repository relative-path helpers.
   - Continue reviewing any new backend command surfaces as they are added.

8. **Problems panel is only partially populated**
   - The UI/store foundation is now present.
   - Monaco diagnostics, Agent errors, and failed diff findings can be surfaced.
   - Terminal TypeScript/lint/test-style file-position errors, Vitest/Jest-style stack traces, and failed test file summaries are parsed into Problems.
   - Frontend smoke tests now cover file URI parsing and terminal problem extraction.
   - Rich test-runner protocol integration is still pending.
   - TypeScript/JavaScript open-file diagnostics now come from Monaco TS worker markers.
   - TypeScript LSP diagnostics are surfaced through Problems, Monaco markers, and TopBar diagnostics summaries.
   - Whole-workspace diagnostics still need indexing/runtime validation beyond opened files.

9. **Runtime modes need clearer UI messaging**
   - Browser preview now avoids crashes.
   - Some panels still need explicit disabled states for web preview mode.

10. **Encoding cleanup is incomplete**
   - Many files had historical mojibake comments/text.
   - User-visible text should be cleaned progressively.

### Lower Priority

11. **Code completion is partially semantic**
   - Monaco now has stable local keyword/symbol/snippet/path suggestions for non-TS languages.
   - TypeScript/JavaScript now use Monaco TS worker semantic completion/hover/diagnostics for open models.
   - TypeScript LSP-backed hover, completion, definition, document symbols, rename, code actions, diagnostics, and status snapshots are wired.
   - Code actions now log apply success/failure and trigger LSP diagnostics refresh after edits.
   - Workspace-wide LSP indexing still needs runtime validation.
   - No LLM inline completion request path yet.

12. **Large frontend bundle**
   - Monaco, Markdown, xterm and syntax tooling create a large chunk.
   - Add dynamic imports/manual chunks later.

13. **Test coverage is thin**
   - Rust context compression has tests.
   - Rust diff apply, workspace boundaries, pipeline helpers, and pending diff summaries have tests.
   - Need more tests for Agent state transitions and frontend store behavior.

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
- Agent action log with prompt/context/diff provenance.
- Reviewer uses actual pending diff summaries for structured review.
- Context sources: selected files, open files, git diff, project tree summary, terminal/log excerpts.
- Context compression strategy interface:
  - `full`: complete active context.
  - `focused`: selected code and active-file excerpt.
  - `compact`: outline and metadata.
  - `budgeted`: token-budget-aware file packing.
- Structured model protocol instead of free-form markdown-only diff parsing.

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
- TopBar exposes common Run/Debug/Build/Test commands, while the bottom Commands panel lists all discovered workspace commands and task status.
- QuickActions sends real Agent prompts.
- DiffView supports per-file and per-hunk apply/reject.
- Git panel supports stage, unstage, discard with confirmation.
- Editor has local code completion for common languages and current-file symbols.
- Problems panel replaces static test/action samples and accepts Monaco diagnostics, Agent findings, and parsed terminal test/lint failures.
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
| 2026-04-30 | Context compression modes | Let users choose prompt context compression strategy |

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

1. Runtime-verify LLM provider profiles in `npm run tauri -- dev`, including legacy config migration, profile create/edit/delete/default, Chat profile selection, and per-run context mode.
2. Add persistent credential storage and better SSH/passphrase UX for Git remote operations.
3. Add richer merge editor UI for conflict blocks and safer destructive-action UX.
4. Runtime-verify TypeScript LSP completion/diagnostics/code actions in `npm run tauri -- dev`, including Quick Fix refresh and workspace indexing behavior.
5. Add Rust/Python LSP adapters after TypeScript runtime validation.
6. Add stricter validation to the structured Agent protocol.
7. Persist Agent action logs with prompt/context/diff provenance.
8. Move LLM API key storage to a safer credential path.
9. Wire profile budget metadata into Agent context building with clearly labeled estimated token/character budgeting, then map max output tokens into provider request bodies where supported.

---

*Last updated: 2026-05-16 - Phase 7 in progress; per-profile model budget metadata is configurable and visible in Chat; actual budget-aware context trimming remains next.*
