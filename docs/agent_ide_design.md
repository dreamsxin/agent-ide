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
  -> AgentGlobalState clones LLM client, context compression, and current pipeline
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

### 4.5 Diff Review and Apply

Model responses are parsed from code blocks:

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
- Per-hunk apply/reject is not implemented yet.

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

### 4.7 Git

Git commands resolve paths through the workspace service and then use `git2`:

- `git_status(path)`
- `git_diff(path, file?)`
- `git_commit(path, message)`

Current Git scope is status/diff/commit. Stage, unstage, and discard are still product roadmap work.

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
- API keys are currently persisted in local JSON and should move to OS keychain or stricter credential storage.

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
   - Replace free-form markdown diff parsing with a schema or tool-call style protocol.
   - Include operation type, file path, file version/hash, hunks, rationale, and provenance.

2. **Version-aware diff application**
   - Add file hash or revision metadata.
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
   - Move LLM API key from JSON config to OS credential storage or a permission-hardened interim store.

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
