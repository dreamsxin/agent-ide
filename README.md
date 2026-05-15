# Agent IDE

中文文档见 [README.zh-CN.md](README.zh-CN.md).

Code-centric controllable AI Agent IDE built with Tauri v2, Rust, React, TypeScript, Tailwind CSS, Monaco Editor, and xterm.js.

Agent IDE is not intended to be a chat-only coding tool. The product direction is an IDE where the Agent is visible, auditable, and user-controlled through task plans, role pipelines, diff review, logs, Git state, and terminal workflows.

![Agent IDE screenshot](docs/screen-01.png)

## Current Status

Current phase: **Phase 7 - Agent execution quality and auditability**.

Implemented core capabilities:

- Tauri desktop shell with React/Vite frontend and Rust backend.
- Monaco-based editor, file tabs, file tree, Git panel, terminal panel, logs, and Agent panel.
- Workspace-scoped filesystem operations with path boundary checks.
- Git status/diff/stage/unstage/discard/commit/branch/fetch/pull/push commands through `git2`, with staged/worktree/all diff modes and multi-select batch actions in Source Control.
- PTY terminal backend using `portable-pty` and xterm.js frontend integration.
- OpenAI-compatible streaming LLM client.
- Role-aware Agent pipeline: planner -> architect -> coder -> tester -> reviewer.
- Agent context compression modes: `full`, `focused`, `compact`.
- Agent context enrichment with project tree summary and Git working-tree diff.
- Structured action log events shown in the Logs panel.
- Diff review and apply flow with structured apply failures.
- Compatible structured `agent-changes` JSON protocol plus legacy diff/new-file block parsing.
- TypeScript/JavaScript semantic bridge with Monaco fallback plus `typescript-language-server` support for hover, completion, definition, document symbols, rename, code actions, and diagnostics.
- Problems integration for static diagnostics and terminal/test failures, including severity-colored editor line decorations, minimap markers, and runtime failure markers for file/line/column locations.
- Explorer quality-of-life actions including reveal in file explorer, copy file, copy absolute file path, and copy relative file path.

Important remaining gaps:

- Git workflow still needs persistent credential storage, better SSH/passphrase UX, and richer merge editor controls.
- LSP support still needs workspace-wide indexing validation, install/configuration UX, and broader language coverage beyond TypeScript/JavaScript.
- Agent change protocol still needs stricter schema validation and richer provenance.
- API keys are still persisted in local JSON config.
- Terminal still needs deeper interactive runtime testing across panel hide/show, workspace switching, and long-running processes.
- Frontend test coverage and Tauri smoke tests are still thin.

See [ROADMAP.md](ROADMAP.md) for the implementation source of truth and [docs/agent_ide_design.md](docs/agent_ide_design.md) for detailed design.

## Runtime Modes

There are two different development modes:

```powershell
npm run dev
```

Runs Vite web preview only. Tauri IPC, filesystem, terminal, Git, and Agent backend features are disabled or guarded.

```powershell
npm run tauri -- dev
```

Runs the real desktop IDE with the Rust backend and Tauri APIs.

## Setup

Prerequisites:

- Node.js and npm
- Rust toolchain
- Tauri v2 prerequisites for your OS

Install frontend dependencies:

```powershell
npm install
```

Run the web preview:

```powershell
npm run dev
```

Run the desktop app:

```powershell
npm run tauri -- dev
```

## Verification

Run these checks before committing substantial changes:

```powershell
npm run build
cd src-tauri
cargo check
cargo test
```

Known build note: Vite currently warns about a large chunk because Monaco, Markdown, xterm, and syntax tooling are bundled together. This is not a correctness failure.

## Project Structure

```text
src/
  components/
    agent/       Agent chat, task, diff, pipeline, settings UI
    editor/      Monaco editor, tabs, overlays, quick actions
    layout/      top/left/right/bottom layout panels
    panels/      Explorer, Git, Terminal, Logs
  hooks/         Tauri event bridge and shortcuts
  stores/        Zustand stores
  types/         frontend DTOs
  utils/         Tauri runtime helpers

src-tauri/
  src/
    agent/       planner, executor, orchestrator, diff apply, roles
    commands/    Tauri IPC commands for fs/git/terminal/agent
    services/    workspace, context, LLM client
    bin/         agent_cli

docs/
  agent_ide_design.md      detailed current design
  agent_ide_plan.md        original technical plan
  agent_ide_ui_design.md   product UI design target
```

## Agent Workflow

Agent IDE uses the chat UI as the user entry point, but the Agent is scheduled by the IDE runtime rather than by a single free-form chat loop.

```text
Chat prompt
  -> ChatView collects prompt, active file, selection, and attached context files
  -> useAgentStore.sendPrompt() invokes send_agent_prompt over Tauri IPC
  -> commands/agent.rs builds AgentContext and reads the configured pipeline
  -> services/context.rs enriches and compresses context
  -> agent/orchestrator.rs runs the Agent state machine
  -> planner produces task steps
  -> role pipeline executes configured stages
     -> architect
     -> coder
     -> tester
     -> reviewer
  -> executor streams LLM output through services/llm_client.rs
  -> diff parser converts model output into pending diffs
  -> reviewer receives actual pending diff summaries
  -> useAgentBridge receives backend events and refreshes Chat/Tasks/Pipeline/Diff/Logs
  -> user applies/rejects diffs through commands/agent.rs and agent/diff_apply.rs
```

The main scheduling modules are:

| Layer | Module | Responsibility |
|-------|--------|----------------|
| UI | `src/components/agent/*` | Chat input, task view, pipeline view, diff review, settings. |
| Frontend state | `src/stores/useAgentStore.ts` | Agent state, IPC calls, messages, steps, diffs, pipeline config. |
| Event bridge | `src/hooks/useAgentBridge.ts` | Subscribes to backend events and updates Zustand stores. |
| IPC boundary | `src-tauri/src/commands/agent.rs` | Validates requests, builds context, starts/stops Agent runs, applies/rejects diffs. |
| Context | `src-tauri/src/services/context.rs` | Adds active file, selection, open files, project tree, Git diff, and compression mode. |
| Orchestration | `src-tauri/src/agent/orchestrator.rs` | Runs planner, role stages, reviewer, action logs, and Agent state transitions. |
| Role execution | `src-tauri/src/agent/executor.rs` | Sends role-specific prompts to the LLM and streams responses. |
| LLM | `src-tauri/src/services/llm_client.rs` | OpenAI-compatible streaming chat client. |
| Diff apply | `src-tauri/src/agent/diff_apply.rs` | Applies reviewable file changes inside the workspace boundary. |

Context compression is configured in the Agent panel's Settings tab:

| Mode | Use |
|------|-----|
| `focused` | Default practical mode: selection, active-file excerpt, project summary, Git diff. |
| `compact` | Lower-token mode: outline and metadata for broad context. |
| `full` | Maximum-fidelity mode: complete active context when accuracy matters more than token use. |

Agent events are streamed back to the UI and action log:

- `agent-state-changed`
- `agent-stream-token`
- `agent-plan-ready`
- `agent-step-update`
- `agent-pipeline-update`
- `agent-diff-ready`
- `agent-action-log`

For the full design, read [docs/agent_ide_design.md](docs/agent_ide_design.md), especially sections 4.3 Agent Prompt, 4.4 Agent Pipeline, 5 Context Model, and 6 Agent Modes and Safety.

## Agent Change Protocol

Preferred structured output:

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

Legacy `diff:path` and `new:path` code blocks are still supported.

## Configuration

LLM config can be provided through the UI or environment variables:

```powershell
$env:LLM_ENDPOINT = "https://api.openai.com/v1"
$env:LLM_API_KEY = "..."
$env:LLM_MODEL = "..."
```

Current local config files are stored under `~/.agent-ide` unless `AGENT_IDE_CONFIG_DIR` is set.

## CLI

The Rust side includes a preview/apply CLI:

```powershell
cd src-tauri
cargo build --bin agent_cli --release
target\release\agent_cli --help
```

## Git Notes

This repo may have local demo changes. Check status before staging:

```powershell
git status --short
```

Do not include unrelated demo/workspace changes in feature commits.
