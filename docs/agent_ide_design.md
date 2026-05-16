# Agent IDE Detailed Design

> Current detailed design for the Tauri + React + Rust Agent IDE.
> `ROADMAP.md` remains the implementation state source of truth. This document explains the system design, workflows, context handling, Agent orchestration, and technical boundaries.

---

## 1. Document Sync Check

### `docs/agent_ide_plan.md`

Status: **partially synchronized**.

Still accurate:

- Core stack: Tauri v2, Rust backend, React 18, TypeScript, Tailwind, Monaco, xterm.js, Zustand.
- High-level architecture: frontend WebView invokes Rust commands and receives Tauri events.
- Main product direction: code-centric controllable Agent IDE.

Outdated or incomplete:

- Directory tree omits newer files such as `src-tauri/src/agent/diff_apply.rs`, `src/utils/tauri.ts`, `src/hooks/useAgentBridge.ts`, and several current components/stores.
- IPC examples use older names such as `send_prompt`, `apply_diff`, and `reject_diff`; current commands include `send_agent_prompt`, `apply_diffs`, and `reject_diffs`.
- Phase checklist still marks many Agent/Git/Terminal capabilities as incomplete even though parts are now implemented.
- Multi-Agent collaboration was described as planned; the backend now executes the configured pipeline as role-aware stages.
- Context compression and workspace boundary policy are missing from the old plan.

### `docs/agent_ide_ui_design.md`

Status: **directionally synchronized, product-target oriented**.

Still accurate:

- UI philosophy: editor first, AI visible but controllable, transparent task/diff review.
- Main layout: Explorer / Editor / Agent panel / Bottom execution panel.
- Agent states and task/diff visualization goals.
- Role-based Agent concept: Architect, Coder, Tester, Reviewer.

Outdated or aspirational:

- Split view, minimap, Ghost Mode, drag-driven AI, conflict resolution UI, and Agent action history are product goals, not fully implemented.
- Task pipeline is now wired to backend execution, but the UI design still describes it mostly as a conceptual collaboration view.
- Tests and Actions bottom tabs are not yet backed by full real workflows.

---

## 2. System Overview

Agent IDE is a local desktop IDE built around three control surfaces:

1. **Editor surface**: Monaco editor, tabs, file contents, selections, inline/diff overlays.
2. **Agent surface**: chat input, task plan, role pipeline, diff review, model settings.
3. **Execution surface**: terminal, logs, Git status/diff/commit, future tests/actions.

The frontend is responsible for interaction, state, and rendering. The Rust backend owns filesystem access, workspace boundary checks, terminal processes, Git operations, LLM streaming, Agent orchestration, and diff application.

```text
React UI
  -> Zustand stores
  -> Tauri invoke commands
  -> Rust command layer
  -> services / agent modules
  -> Tauri events
  -> useAgentBridge / UI refresh
```

The Agent path is intentionally split across small modules. UI components do not call the LLM directly. They dispatch through `useAgentStore`, cross the Tauri command boundary, and let the Rust orchestrator control planning, role execution, diff parsing, review, action logging, and optional apply behavior.

Important runtime distinction:

- `npm run dev`: browser/Vite preview only. Tauri IPC-dependent features are guarded or disabled.
- `npm run tauri -- dev`: real IDE runtime with filesystem, Git, terminal, and Agent backend.

---

## 3. Runtime Architecture

### Frontend

Key modules:

- `src/App.tsx`: main layout, workspace restore, shortcut help, Agent event bridge mount.
- `src/stores/useEditorStore.ts`: open files, active file, content cache, save/open operations.
- `src/stores/useAgentStore.ts`: Agent state, mode, messages, steps, diffs, pipeline, LLM config.
- `src/hooks/useAgentBridge.ts`: subscribes to backend Agent events and updates Zustand.
- `src/components/agent/*`: chat, tasks, diff review, role selector, pipeline editor, settings.
- `src/components/panels/*`: Explorer, Git, Terminal, Logs.
- `src/utils/tauri.ts`: detects Tauri runtime so browser preview does not crash.

### Backend

Key modules:

- `src-tauri/src/lib.rs`: Tauri builder, plugin setup, command registration.
- `src-tauri/src/commands/fs.rs`: workspace-scoped file operations.
- `src-tauri/src/commands/git.rs`: Git status, diff, commit, workspace path validation.
- `src-tauri/src/commands/terminal.rs`: PTY spawn/write/resize/output/kill lifecycle.
- `src-tauri/src/commands/agent.rs`: Agent command API, LLM config, mode, pipeline, diff apply.
- `src-tauri/src/services/workspace.rs`: saved workspace, config directory, path resolution.
- `src-tauri/src/services/context.rs`: `AgentContext` and compression modes.
- `src-tauri/src/services/llm_client.rs`: OpenAI-compatible streaming chat client.
- `src-tauri/src/agent/orchestrator.rs`: Agent state machine integration and pipeline execution.
- `src-tauri/src/agent/multi_agent.rs`: roles, role prompts, pipeline stages.
- `src-tauri/src/agent/diff_apply.rs`: structured diff application and failure reporting.

---

## 4. Core Workflows

### 4.1 Open Workspace

```text
App boot
  -> invoke("get_workspace_path")
  -> workspace path restored into layout/editor stores
  -> Explorer lists files through list_directory
  -> filesystem paths are resolved through workspace service
```

The backend treats the saved workspace as the allowed root for filesystem, Git, terminal cwd, and Agent diff writes. Backend commands should use `workspace::resolve_existing` or `workspace::resolve_for_write` before touching paths.

### 4.2 Open and Save File

```text
Explorer selects file
  -> useEditorStore.openFile()
  -> invoke("read_file_content", { path })
  -> fs command validates path inside workspace
  -> content cached in Zustand
  -> Monaco renders active tab

Ctrl+S / save action
  -> useEditorStore.saveCurrentFile()
  -> invoke("write_file_content", { path, content })
  -> backend validates write target
  -> file is written
```

### 4.3 Agent Prompt

```text
ChatView.handleSend()
  -> collect active file, active content, selected text, context file list
  -> useAgentStore.sendPrompt()
  -> invoke("send_agent_prompt", { request })
  -> AgentGlobalState resolves selected LLM profile and reads its API key from the OS credential store
  -> AgentGlobalState clones LLM client, context compression, and current pipeline
  -> AgentContext is enriched with workspace project tree and Git diff
  -> ContextCompressionMode formats the context as full/focused/compact
  -> AgentOrchestrator.run()
```

The Agent emits events while running:

| Event | Payload | Frontend Consumer |
|-------|---------|-------------------|
| `agent-state-changed` | state/mode | `useAgentBridge` -> Agent state |
| `agent-stream-token` | string token | stream content |
| `agent-plan-ready` | `TaskStep[]` | task view |
| `agent-step-update` | `TaskStep` | step status/logs |
| `agent-pipeline-update` | `PipelineStage[]` | pipeline timeline |
| `agent-diff-ready` | `FileDiff[]` | diff review |
| `agent-action-log` | `ActionLogEntry` | logs/audit trail |

Frontend scheduling responsibilities:

| Module | Role |
|--------|------|
| `ChatView` | Captures the user prompt and active editor context. |
| `QuickActions` | Creates focused prompts from the current editor selection. |
| `useAgentStore` | Holds Agent state and invokes backend commands. |
| `useAgentBridge` | Listens to Agent events and updates messages, task steps, diffs, pipeline stages, and logs. |
| `DiffView` | Lets the user apply/reject all diffs, individual files, or individual hunks. |

Backend scheduling responsibilities:

| Module | Role |
|--------|------|
| `commands/agent.rs` | IPC boundary, request validation, context construction, pipeline/config lookup. |
| `services/context.rs` | Workspace context enrichment and compression. |
| `services/credentials.rs` | OS credential store access for LLM profile secrets. |
| `agent/orchestrator.rs` | State transitions, planner call, pipeline sequencing, reviewer context, action logs. |
| `agent/planner.rs` | Converts the user prompt and context into task steps. |
| `agent/executor.rs` | Runs role-specific model calls and streams output. |
| `agent/multi_agent.rs` | Defines role prompts and pipeline stage semantics. |
| `agent/diff_apply.rs` | Applies validated pending diffs inside the workspace. |

### 4.4 Agent Pipeline

Current backend execution is role-aware:

```text
Planner
  -> produces task steps
Pipeline reset to pending
  -> Architect stage
      -> architecture/design output
  -> Coder stage
      -> implementation diff/new-file blocks
  -> Tester stage
      -> test diff/new-file blocks or test findings
  -> Reviewer stage
      -> review findings and optional required fix diffs
Diff parser
  -> extracts pending FileDiff entries
Review state
  -> user applies/rejects diffs, or Auto mode applies directly
```

Each stage receives:

- Original user prompt.
- Compressed project context.
- Prior stage outputs.
- Role-specific system prompt and output rules.

The configured pipeline lives in `AgentGlobalState.pipeline_stages` and can be changed through `get_pipeline`, `update_pipeline`, and `reset_pipeline`.

Reviewer behavior is tied to actual proposed changes. After earlier stages produce model output, the orchestrator parses pending diffs and sends a summary of those concrete file/hunk changes into the reviewer stage. That prevents review from relying only on previous prose.

Action logs are emitted for prompt receipt, planner completion, stage start/completion/failure, diff readiness, review context, and apply results. The frontend displays these logs so users can audit what the Agent did and what context summary was used.

### 4.5 Diff Review and Apply

Model responses prefer a structured protocol:

````text
```agent-changes
{
  "changes": [
    {
      "type": "edit",
      "file": "path/to/file",
      "baseHash": "optional current file hash when known",
      "rationale": "why this change is needed",
      "hunks": [
        { "original": "exact existing code", "updated": "replacement code" }
      ]
    },
    {
      "type": "create",
      "file": "path/to/new-file",
      "rationale": "why this file is needed",
      "content": "complete file content"
    }
  ]
}
```
````

Legacy markdown diff blocks are still supported for compatibility:

````text
```diff:path/to/file
<<<<<<< ORIGINAL
existing code
=======
updated code
>>>>>>> UPDATED
```
````

New files use:

````text
```new:path/to/file
file content
```
````

Apply flow:

```text
DiffView.applyAllDiffs()
  -> invoke("apply_diffs")
  -> apply_pending_diffs()
  -> resolve each target path inside workspace
  -> apply each pending diff
  -> return ApplyDiffsResult { applied, failed }
  -> frontend marks applied/failed cards
```

Current conflict behavior:

- Rejects outside-workspace paths.
- Rejects missing original content.
- Rejects ambiguous original matches.
- Rejects new-file overwrite.
- Reports partial failures structurally.

Known limitation:

- Hunks are still text-match based and do not include file version/hash metadata.
- Per-hunk apply/reject is implemented in the backend and Diff view; mixed hunk states still need clearer partial-status semantics.

### 4.6 Terminal

```text
Terminal component mounts in Tauri runtime
  -> invoke("spawn_terminal", { id })
  -> listen("terminal-output")
  -> xterm writes user input
  -> invoke("write_to_terminal", { id, data })
  -> ResizeObserver invokes resize_terminal
  -> unmount invokes kill_terminal
```

Terminal cwd is scoped to the saved workspace. Browser preview shows a disabled-state message instead of attempting PTY access.

Terminal/test failures are parsed into structured Problems when output includes file, line, and column information. Those Problems are mirrored back into Monaco markers so runtime/test failures can be highlighted in the editor instead of only appearing in the Problems panel.

### 4.6.1 Problems and Diagnostics

Problems currently aggregate multiple sources:

- `diagnostic`: Monaco built-in language diagnostics.
- `lsp`: diagnostics published by the TypeScript language server.
- `test`: terminal/task/test failures parsed from command output.
- `agent` and `system`: Agent/runtime issues surfaced by the IDE.

The editor has three marker bridges:

- Monaco diagnostics are read into Problems through `DiagnosticsBridge`.
- TypeScript LSP diagnostics are written to Problems and Monaco markers through `useLspDiagnostics`.
- Runtime Problems from terminal/test/Agent/system sources are written back to Monaco markers through `ProblemsMarkerBridge`.
- All Problems sources are mirrored into severity-colored editor decorations for the active model, including whole-line background, line-decoration gutter, minimap, and overview ruler indicators.

Paths are normalized before tab matching, marker matching, and problem navigation. This avoids duplicate tabs and broken paths such as URL-encoded Windows drive paths.

### 4.6.2 TypeScript/JavaScript Semantic Bridge

Current TypeScript/JavaScript semantic support uses two layers:

- Monaco TypeScript/JavaScript worker fallback for open-file syntax and semantic diagnostics.
- Optional `typescript-language-server` backend for hover, completion, definition, document symbols, rename, code actions, and diagnostics.

The Rust backend searches for `typescript-language-server` in workspace `node_modules/.bin`, `%APPDATA%\npm` on Windows, and `PATH`. TopBar shows `TS checking`, `TS ready`, or `TS unavailable`; the unavailable tooltip includes the backend startup error.

Remaining semantic work:

- Validate workspace-wide indexing across larger TypeScript projects.
- Add installation/configuration UX for missing language servers.
- Add Rust/Python LSP adapters.
- Feed code actions with actual diagnostics context for richer quick fixes.

### 4.7 Git

Git commands resolve paths through the workspace service and then use `git2`:

- `git_status(path)`
- `git_diff(path, file?, kind?)`, where `kind` is `worktree`, `staged`, or `all`
- `git_stage_files(path, files)`
- `git_unstage_files(path, files)`
- `git_discard_files(path, files)`
- `git_commit(path, message)`
- `git_checkout_branch(path, branch, create)`
- `git_fetch(path, remote?)`
- `git_pull(path, remote?)`
- `git_push(path, remote?)`

Current Git scope covers status, staged/worktree/all diff views, file and multi-file stage/unstage/discard, commit, local branch checkout/create, remote branch checkout/tracking, fetch, fast-forward-only pull, push, upstream/ahead/behind display, one-shot credential inputs for remote actions, conflict file detection, and basic conflict resolution controls.

Remaining Git roadmap work:

- Persistent credential storage and better HTTPS/SSH/passphrase failure recovery.
- Rich merge editor UI for conflict blocks.
- Safer destructive-action UX for discard/revert/reset workflows.

---

## 5. Context Model

### 5.1 AgentContext

The current Agent prompt context includes:

- `active_file`
- `active_file_content`
- `selection`
- `open_files`
- `project_path`
- `project_tree`
- `git_diff`

This context is built in `send_agent_prompt` from the frontend request and the saved workspace root.
The backend enriches it with a bounded project tree summary and, when the workspace is a Git repository, a bounded working tree diff.
Runtime failure prompts can also include recent Problems, failed command output, terminal excerpts, and warning/error logs before reaching the backend.

### 5.2 Compression Modes

Context compression is implemented in `src-tauri/src/services/context.rs`.

| Mode | Intent |
|------|--------|
| `full` | Include complete active context. Best fidelity, largest prompt. |
| `focused` | Include selection and active-file excerpt. Default practical mode. |
| `compact` | Include outline/metadata-style summary. Lowest token use. |

Future target:

- `budgeted`: token-budget-aware packing across selected files, open files, Git diff, project tree, logs, and terminal output.

### 5.3 Context Boundaries

The Agent should not receive more context than needed. Preferred priority:

1. User selection.
2. Active file content or excerpt.
3. Explicitly attached/open files.
4. Git diff and relevant project tree summary.
5. Terminal/log excerpts when the task is about runtime errors.

Context should carry provenance in future action logs so users can inspect what was sent to the model.

Current provenance level:

- Action logs include prompt phase, role/stage, context summary, diff summary, and details.
- Reviewer receives pending diff summaries generated from actual proposed changes.
- Full persistent action-log history and exact context source manifests are still future work.

---

## 6. Agent Modes and Safety

| Mode | Intended Behavior | Current Behavior |
|------|-------------------|------------------|
| `suggest` | Suggest changes only | Produces reviewable diffs |
| `edit` | Can prepare edits for user confirmation | Produces reviewable diffs |
| `auto` | Can apply accepted Agent diffs automatically | Applies pending diffs after pipeline run |

Safety rules:

- Filesystem writes must go through workspace path resolution.
- Agent-generated HTML is not rendered directly; markdown rendering skips HTML.
- Diff application returns structured failures and preserves failed file content.
- Cancellation is cooperative through a shared atomic flag and streaming checks.
- LLM API keys are stored through the OS credential store; local JSON profile config stores credential references only. This still needs cross-OS runtime validation and recovery UX for inaccessible credentials.

---

## 7. State and Data Structures

### AgentState

```text
idle
thinking
planning
acting
reviewing
waiting_user
done
error
```

### TaskStep

```typescript
{
  id: string;
  title: string;
  type: "create" | "edit" | "run" | "test" | string;
  status: "todo" | "doing" | "done" | "error";
  logs: string[];
}
```

### PipelineStage

```typescript
{
  role: "architect" | "coder" | "tester" | "reviewer";
  name: string;
  status: "pending" | "active" | "completed" | "failed";
}
```

### FileDiff

```typescript
{
  id: string;
  file: string;
  hunks: DiffHunk[];
  status: "pending" | "applied" | "rejected" | "failed";
  applyError?: string;
}
```

---

## 8. Technical Gaps Before Daily IDE Replacement

Highest-impact gaps:

1. **Structured Agent protocol**
   - `agent-changes` JSON blocks are supported as a compatibility step.
   - Still need file version/hash, stronger validation, and eventually tool-call style protocol.
   - Include operation type, file path, file version/hash, hunks, rationale, and provenance.

2. **Version-aware diff application**
   - Optional `baseHash` metadata is now supported and checked before edit diffs are applied.
   - Support per-file and per-hunk apply/reject.
   - Show conflicts with clear recovery options.

3. **Context expansion**
   - Git diff and project tree summary are now included.
   - Add terminal/log excerpts and selected file packing.
   - Add token budget packing.

4. **Action log**
   - Persist prompt, compressed context summary, stage outputs, diffs, apply results, and errors.
   - Make Agent actions auditable in the UI.

5. **Secret storage**
   - Runtime-validate OS credential storage and add recovery UX for inaccessible or missing LLM credentials.

6. **Runtime hardening**
   - Interactive Tauri smoke tests for boot, workspace open, file read/write, terminal, Agent prompt, diff apply.
   - Frontend store/component tests for Agent events and diff status updates.

---

## 9. Verification

Baseline checks before considering Agent workflow changes complete:

```powershell
npm run build
cd src-tauri
cargo check
cargo test
```

Current known build note:

- Vite warns about a large frontend chunk due to Monaco/Markdown/xterm/syntax tooling. This is not a correctness failure, but code splitting should be added before release readiness.

---

## 10. Source of Truth Policy

Use the documents as follows:

- `ROADMAP.md`: current implementation state, known issues, next tasks.
- `docs/agent_ide_design.md`: detailed technical design and current workflow explanation.
- `docs/agent_ide_ui_design.md`: product/UI target and design intent.
- `docs/agent_ide_plan.md`: original technical plan; useful historically, but should be refreshed when major implementation milestones land.
