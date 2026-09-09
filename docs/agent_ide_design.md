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
- `src-tauri/src/services/llm_client.rs`: OpenAI-compatible streaming chat client with native tool calling.
- `src-tauri/src/services/mcp.rs`: MCP stdio client, tool discovery, and tool approval policy.
- `src-tauri/src/agent/orchestrator.rs`: Agent state machine integration and pipeline execution.
- `src-tauri/src/agent/executor.rs`: role execution and the bounded tool-call loop.
- `src-tauri/src/agent/workspace_tools.rs`: built-in read-only workspace tools.
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

### 4.3.1 Tool Call Loop

Each pipeline stage runs a bounded tool loop in `agent/executor.rs::stream_with_tool_loop`, not a single prompt:

```text
stream_chat_with_tools
  -> model returns tool_calls
  -> ToolInvoker executes each call
  -> results are appended as role: "tool" messages
  -> next round
  -> repeat until the model stops calling tools, or 12 rounds
```

Transport: `services/llm_client.rs` sends native OpenAI `tools` + `tool_choice: "auto"` when the profile uses `native_tools`. Streaming `delta.tool_calls` fragments are reassembled by `ToolCallAccumulator`. If a provider rejects the tool parameters (400/404/422/501 naming `tools`/`tool_choice`), the client retries without them and flags `tools_rejected`, and the command layer emits a warning action log.

Two tool families are exposed:

| Family | Source | Scope |
|--------|--------|-------|
| Output protocol | `emit_agent_changes`, `emit_sdd_draft` | Not side effects. Their arguments are synthesized back into `agent-changes` blocks so the diff parser stays transport-agnostic. |
| Workspace read | `workspace_read_file`, `workspace_search_text`, `workspace_list_files` | Read-only, resolved through `workspace::resolve_existing`, credential files refused, 64 KB read cap, 60 search hits, 200 listed entries. |
| Workspace verify | `workspace_run_command` | Only advertised when the run grants `allowCommandRun`. The allow-list is derived by the backend from the project's declared tasks, never from model input. Long-running commands are refused regardless of the list. Output tail-truncated to 12,000 chars. |
| Workspace write | `workspace_write_file` | Only advertised in `auto` mode. Whole-file replacement through `resolve_for_agent_write`; creating a file needs `allowFileCreate`. Each write is recorded as an `applied` diff with its pre-write content and covered by an undo checkpoint. |
| MCP | `mcp__{server}__{tool}` | External stdio servers, gated by `McpToolPolicy`. |

Notes:

- `workspace_run_command` is the only Agent path to process execution outside MCP. When the permission is absent the tool is neither advertised nor claimed by the invoker — a tool that would always fail is worse than an absent one, because the model spends a round discovering that.
- The long-running-command refusal is a safety invariant, not a preference: verification runs a command to completion, and a dev server never exits. It is checked before the allow-list, so listing `npm run dev` does not enable it.
- Gating writes on `auto` mode is deliberate rather than a new permission flag. `auto` already applies pending diffs without a click, so writing mid-run grants nothing it did not already have; `suggest` / `edit` promise "review before it lands", so the tool is absent there and the model emits diffs.
- Tool writes are published back into the review area (`AgentOrchestrator::record_tool_writes`) on every exit path — success, failure, and cancellation — because a write that happened before a failure is still on disk. Without that, the file changes and the Diff view shows nothing, which is the auditability the product exists for.
- Tool failures do not abort the stage; the error text is returned to the model so it can adapt.
- Cancellation is checked before each tool call.
- Tool definitions and the executing `ToolInvoker` are always attached together. `send_agent_prompt`, `run_agent_step`, and `continue_agent_pipeline` all build both; the MCP policy and tool permissions used by a run are remembered on the orchestrator (`tool_policy`, `tool_permissions`) so a resumed pipeline rebuilds the same tool surface. The write log is shared through the same `Arc`, so a resumed run's writes are still published.
- `agent_cli` attaches the read-only workspace tools unconditionally (`cli/mod.rs` calls `attach_workspace_tools` on every agent command), so headless runs have the same read surface as the desktop app. Write and command tools still require the matching permission flags.




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
- Prior stages' real `assistant` / `tool` messages, tool results included.
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

### 4.6.2 Language Server Semantic Bridge

Current semantic support uses two layers:

- Monaco TypeScript/JavaScript worker fallback for open-file syntax and semantic diagnostics.
- Optional `typescript-language-server` backend for hover, completion, definition, document symbols, rename, code actions, and diagnostics.
- Optional `gopls` backend for Go hover, completion, definition, document symbols, rename, code actions, and diagnostics.

The Rust backend chooses a language server from the active file language. TypeScript/JavaScript use `typescript-language-server`; Go uses `gopls`. TopBar shows `TS checking/ready/unavailable` or `Go checking/ready/unavailable`; the details popover includes startup errors, install command, server source, detected config files, and inferred indexing mode.

Remaining semantic work:

- Validate workspace-wide indexing across larger TypeScript projects.
- Add Rust/Python LSP adapters.
- Runtime-validate indexing behavior on larger monorepos and project-reference workspaces.
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
- `git_fetch(path, remote?, credentials?)`
- `git_pull(path, remote?, credentials?)`
- `git_push(path, remote?, credentials?)`

Current Git scope covers status, staged/worktree/all diff views, file and multi-file stage/unstage/discard, commit, local branch checkout/create, remote branch checkout/tracking, fetch, fast-forward-only pull, push, upstream/ahead/behind display, one-shot credential inputs for remote actions, optional OS-stored HTTPS remote credentials, conflict file detection, and basic conflict resolution controls.

Remaining Git roadmap work:

- Better SSH/passphrase failure recovery.
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
- `project_memory`

`project_memory` is loaded from a workspace-root `AGENTS.md` when present (`services/project_memory.rs`). It is trimmed to 8,000 characters, included by default in every run, and rendered as the first content section after the project header so it survives budget trimming. Per-run source toggles can exclude it via `includeProjectMemory`; the CLI selects it with `--include project-memory`.

This context is built in `send_agent_prompt` from the frontend request and the saved workspace root.
The backend enriches it with a bounded project tree summary and, when the workspace is a Git repository, a bounded working tree diff.
Runtime failure prompts can also include recent Problems, failed command output, terminal excerpts, and warning/error logs before reaching the backend.

### 5.2 Compression Modes

Context compression is implemented in `src-tauri/src/services/context.rs`.

| Mode | Intent |
|------|-------|
| `full` | Include complete active context. Best fidelity, largest prompt. |
| `focused` | Include selection and active-file excerpt. Default practical mode. |
| `compact` | Include outline/metadata-style summary. Lowest token use. |
| `budgeted` | Token-budget-aware packing using the active provider profile budget or a safe default budget. |

Budget packing is priority-quota based, not sequential. Each section has a priority and a share of the input budget (`section_budget_rule`): project header and active-file path first, then project memory and selection, then conversation digest, then active-file content, Git diff, project tree, and open-file list. Allocation runs in two passes — quota first, then unused allowance is redistributed to sections that were truncated. A section granted less than 240 bytes is excluded rather than filled with a truncation marker.

This replaced sequential greedy trimming, where the first oversized section consumed the remaining budget and every later section was dropped as "budget exhausted" purely because of its position in the list.

Token estimation counts non-ASCII characters as one token each and ASCII as roughly four characters per token (`estimate_tokens_for_text`). The token budget is converted to a byte budget using the measured byte-per-token ratio of the actual context, so Chinese-heavy prompts are not systematically under-estimated.

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
- File diffs include protocol/operation/schema/source stage provenance.
- Hunks include change/hunk index, role/stage, prompt context, and rationale when generated by structured `agent-changes`.
- Full persistent action-log history and exact context source manifests are still future work.

---

## 6. Agent Modes and Safety

| Mode | Intended Behavior | Current Behavior |
|------|-------------------|------------------|
| `suggest` | Suggest changes only | Produces reviewable diffs |
| `edit` | Can prepare edits for user confirmation | Produces reviewable diffs |
| `auto` | Can apply accepted Agent diffs automatically | Applies pending diffs after pipeline run |

Safety rules:

- Filesystem writes must go through workspace path resolution (`workspace::resolve_for_agent_write` for Agent writes, which also enforces path deny rules and refuses credential files).
- Credential-looking files are withheld from prompt context and from the Git diff section, because egress is irreversible.
- Agent-generated HTML is not rendered directly; markdown rendering skips HTML.
- Diff application returns structured failures and preserves failed file content.
- `ApplyCheckpoint` snapshots files before an apply (20 levels), and `undo_last_apply` restores them, returns hunks to pending, and restamps `baseHash`.
- Cancellation is cooperative through a shared atomic flag and streaming checks.
- MCP tools are gated by `McpToolPolicy` (`deny` / `auto_approved_only` / `allow_all`); unrecognized values fall back to the most conservative usable policy. MCP arguments have no schema constraint, which is why the built-in workspace tools are not routed through MCP.
- `RunUsageMeter` enforces the per-run token cap before every provider request; retries are not double-counted.
- LLM API keys are stored through the OS credential store; local JSON profile config stores credential references only. Validated on Windows; macOS/Linux still unverified.

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

1. **Autonomous repair loop** (largest remaining gap)
   - The model can read the workspace, run the project's check commands, and — in `auto` mode — write files, so a full observe/change/verify cycle is now possible within one stage's tool loop.
   - The orchestrator does drive that cycle now: `repair_until_checks_pass` (`orchestrator.rs`) runs verify → repair → re-verify under a shared `RepairPolicy`, exposed as the `repair_workspace` command and reachable from `Auto Repair` in the Commands panel. What remains is that a *stage* failure still aborts the pipeline (`orchestrator.rs` returns `Err`) rather than being fed back as a repair round.
   - `agent_cli` has a bounded repair loop (`--max-iterations`, default 0 = off); the desktop app has `verify_workspace` + `agent_repair_prompt` and a `Verify All` / `Fix with Agent` path, but each is a single user-triggered round.
   - Target: an orchestrator-level bounded repair loop reusing `services/verification.rs`.

2. **Version-aware diff application**
   - `baseHash` is stamped by the backend from real file content, so stale detection works.
   - Per-file and per-hunk apply/reject are wired.
   - Remaining: line-offset tolerance, and rejecting edit diffs that carry no stamp at all.

3. **Context retrieval**
   - No symbol index, embedding, or relevance ranking. Context is the active file, selection, open-file list, a bounded project tree (160 entries, 4 levels), and a bounded Git diff.
   - The project tree cap does not scale to large workspaces; a tree-sitter symbol index feeding `budgeted` packing is the planned replacement.

4. **Session memory**
   - Cross-prompt memory is a 6-turn digest (prompt trimmed to 400 chars, outcome to 300) carried in `context.conversation`.
   - Stage prompts are rebuilt per stage as system + the stage's task user message + the prior stages' real `assistant` / `tool` messages + a short "run this stage now" user message. Tool results therefore survive the stage that produced them, including across a pause and resume. The thread is bounded by `executor::bound_transcript`, which drops whole tool-call/tool-result groups rather than splitting a pair, and states in-band how much it omitted. The thread is in-memory only — a process restart loses it.

5. **Action log persistence**
   - Action logs are emitted as events and rendered in the UI, but not persisted. Run history and replay are still missing.

6. **Cost accounting**
   - `RunUsageMeter` enforces a per-run token cap (`maxRunTokens`) before every provider request, and reports under-counting honestly when providers omit usage.
   - A monetary cap exists as well (`maxRunSpendMicros` plus per-million prices, integer micro-USD), checked ahead of the token cap. It only takes effect when both the prompt and completion price are configured; otherwise spend is reported as "not computable" rather than as zero. Editable in Settings; the UI works in dollars and converts to micro-USD by string parsing.

7. **Runtime hardening**
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

## 10. Model Access and Performance Decisions

This section records what is implemented and which earlier proposals were rejected. `ROADMAP.md` holds the task-level status; the notes here exist so the design document stops describing abandoned designs.

### 10.1 Model Access

There is one provider path: an OpenAI-compatible HTTP client in `services/llm_client.rs`.

- Cloud providers and local runtimes (Ollama, LM Studio, vLLM) are the same code path, differing only in profile endpoint and model.
- No native in-process inference engine is linked. This was removed, not deferred: linking an inference engine would pull its license and build toolchain into the binary for a capability an OpenAI-compatible local server already provides.
- Profiles carry the endpoint, model, tool-call mode, context budget, `maxRunTokens`, and the spend-cap trio (`promptMicrosPerMillion`, `completionMicrosPerMillion`, `maxRunSpendMicros`). API keys live in the OS credential store; the JSON profile file stores references only.
- Local engine profiles report `supports_tool_calls: false`; for them the message list is flattened into a single prompt and the `agent-changes` text protocol is the transport.

Hybrid routing (route simple tasks to a cheap local model, complex tasks to a cloud model) is not implemented. It stays a roadmap item because it needs a task-complexity signal the pipeline does not currently produce.

### 10.2 Rejected: Custom Rendering Engine

A viewport/dirty-line incremental renderer was designed earlier and dropped. Monaco already virtualizes rendering, so a second renderer would duplicate it without measurable gain. The remaining editor performance work is bundle code splitting and Monaco model/tab memory management.

### 10.3 Rejected: Separate Completion Framework and Prompt-Optimizer Layer

A standalone completion framework and a separate Chinese prompt-optimizer class were designed earlier and dropped in favor of:

- an inline completion channel over the existing provider client, and
- per-profile language presets for naming and comment conventions.

Chinese-language handling that does exist and is load-bearing: `estimate_tokens_for_text` counts non-ASCII characters as one token each, so Chinese context is not silently under-budgeted.

### 10.4 Extensibility

MCP is the extension mechanism that ships. External stdio servers contribute tools into the same native tool surface the model already uses, gated by `McpToolPolicy`.

A general in-process plugin API (model adapters, UI panels, custom agent roles as loadable plugins) is not implemented. Pipeline stages and roles are configurable data (`get_pipeline` / `update_pipeline`), not plugins.

### 10.5 Performance Targets

These are targets, not verified measurements. Baseline tests are a Phase 10 item.

- Startup: < 3 s
- Memory: < 300 MB idle
- Editor input latency: < 50 ms

---

## 11. Known Direction Beyond the Current Loop

Ordered by dependency, not by appeal:

1. **Write tool.** Done: `workspace_write_file`, advertised only in `auto` mode, recorded as an applied+undoable diff.
2. **Autonomous bounded repair loop.** Landed. The prompt builder and check runner are shared (`services/verification.rs`, `verify_workspace`, `agent_repair_prompt`), and `repair_until_checks_pass` runs verify → repair → re-verify bounded by `RepairPolicy`, reachable as `Auto Repair` in the Commands panel and as the CLI's `--repair-iterations`. Still open: a failed *stage* aborts the pipeline instead of becoming a repair round.
3. **Persistent message thread per run.** Done: stages exchange real `assistant` / `tool` messages (`executor::StageOutcome`), so tool results survive across stages and a pause/resume. Prompt caching is still not implemented — no cache-control markers are sent — and the thread is not persisted across a process restart.
4. **Symbol index and retrieval.** tree-sitter symbol index plus local retrieval feeding `budgeted` packing. Prerequisite for large workspaces, where the 160-entry project tree is not a usable map.
5. **Parallel subagents with worktree isolation.** Depends on (1): parallel agents that cannot write have nothing to isolate.
6. **Hooks and skills.** User-configurable hooks at stage and pre-apply points, and lazily loaded `SKILL.md` packages.
7. **Persisted run artifacts.** IDE runs should produce the same artifact model as `agent_cli`, enabling replay and comparison.

---

## 12. Source of Truth Policy

Use the documents as follows:

- `ROADMAP.md`: current implementation state, known issues, and next tasks. Task-level status lives there, not here.
- `docs/agent_ide_design.md`: detailed technical design of what is implemented, including the tool-call loop, context budgeting, permissions, and recorded design rejections.
- `docs/agent_ide_ui_design.md`: product/UI target and design intent.
- `docs/agent_ide_plan.md`: original technical plan; useful historically, but should be refreshed when major implementation milestones land.
