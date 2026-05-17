# Agent IDE

中文文档见 [README.zh-CN.md](README.zh-CN.md).

Code-centric controllable AI Agent IDE built with Tauri v2, Rust, React, TypeScript, Tailwind CSS, Monaco Editor, and xterm.js.

Agent IDE is not intended to be a chat-only coding tool. The product direction is an IDE where the Agent is visible, auditable, and user-controlled through task plans, role pipelines, diff review, logs, Git state, and terminal workflows.

![Agent IDE screenshot](docs/screen-01.png)

## Current Status

Phase 7 is feature-complete. Phase 8 is focused on daily IDE replacement hardening.

Capability snapshot:

- Desktop IDE shell: Monaco editor, Explorer, Git, Terminal, Problems, Logs, Commands, and Agent panels.
- Agent loop: role pipeline, editable plan, context preview/budgeting, structured action logs, `agent-changes` protocol, and diff review/apply/regenerate.
- Semantic/runtime loop: TypeScript/JavaScript and Go LSP first pass, diagnostics to Problems/editor markers, project command run history, and terminal failure context for Agent repair.
- Automation/release: headless `agent_cli` first pass and Windows packaging workflow.

The detailed implementation state, remaining gaps, and next tasks live in [ROADMAP.md](ROADMAP.md). Design and protocol docs: [docs/agent_ide_design.md](docs/agent_ide_design.md), [docs/agent_changes_schema.md](docs/agent_changes_schema.md), and [docs/smoke_test.md](docs/smoke_test.md).

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
npm test
cd src-tauri
cargo check
cargo test
```

Known build note: Vite currently warns about a large chunk because Monaco, Markdown, xterm, and syntax tooling are bundled together. This is not a correctness failure.

For changes to LSP, Problems, Terminal, Git, or Agent diff application, also run the real Tauri runtime checklist in [docs/smoke_test.md](docs/smoke_test.md).

## Windows Packaging

Build a Windows installer package:

```powershell
npm run package:windows
```

The script runs frontend build/tests, `cargo check`, `cargo test`, and `tauri build --bundles nsis,msi`, then copies installers to `release/windows/<version>/` with `SHA256SUMS.txt` and `manifest.json`.

For a local packaging smoke after checks have already passed:

```powershell
npm run package:windows:fast
```

To build one installer format:

```powershell
npm run package:windows:nsis
npm run package:windows:msi
```

The first Windows bundle may download NSIS, `nsis_tauri_utils.dll`, and/or WiX tooling through Tauri. If local bundling times out while downloading those tools, rerun the command after the tool cache is populated or run the `Windows Package` GitHub Actions workflow, which builds on `windows-latest` and uploads the generated artifacts.

Generated release artifacts are intentionally ignored by Git.

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
  agent_cli_manual.md      CLI mode usage and limitations
  agent_cli_design.md      CLI automation and integration target design
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

Context compression is selected per Chat run:

| Mode | Use |
|------|-----|
| `focused` | Default practical mode: selection, active-file excerpt, project summary, Git diff. |
| `compact` | Lower-token mode: outline and metadata for broad context. |
| `budgeted` | Token-budget-aware packing that uses provider profile budget metadata or a safe default budget. |
| `full` | Maximum-fidelity mode: complete active context when accuracy matters more than token use. |

Agent events are streamed back to the UI and action log:

- `agent-state-changed`
- `agent-stream-token`
- `agent-plan-ready`
- `agent-step-update`
- `agent-pipeline-update`
- `agent-diff-ready`
- `agent-action-log`

For the full design, read [docs/agent_ide_design.md](docs/agent_ide_design.md), especially sections 4.3 Agent Prompt, 4.4 Agent Pipeline, 5 Context Model, and 6 Agent Modes and Safety. The structured change protocol is documented in [docs/agent_changes_schema.md](docs/agent_changes_schema.md).

## Agent Change Protocol

Preferred structured output:

````text
```agent-changes
{
  "version": 1,
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
  ],
  "findings": [
    {
      "severity": "warning",
      "file": "path/to/file",
      "hunkIndex": 0,
      "message": "optional reviewer finding tied to this hunk"
    }
  ]
}
```
````

Legacy `diff:path` and `new:path` code blocks are still supported. Schema details and validation behavior are documented in [docs/agent_changes_schema.md](docs/agent_changes_schema.md).

## Configuration

LLM config can be provided through the UI or environment variables:

```powershell
$env:LLM_ENDPOINT = "https://api.openai.com/v1"
$env:LLM_API_KEY = "..."
$env:LLM_MODEL = "..."
```

Current local config files are stored under `~/.agent-ide` unless `AGENT_IDE_CONFIG_DIR` is set.

## CLI

The Rust side includes a headless automation CLI:

```powershell
cd src-tauri
cargo build --bin agent_cli --release
target\release\agent_cli --help
```

CLI mode is first-pass complete for headless automation. It supports `doctor`, `context estimate`, `plan`, `run`, and `smoke ide-backend`; text/JSON/NDJSON output; run artifacts; optional apply; project command checks; bounded repair iterations; command allow-listing; timeout/output/diff limits; and smoke-tested `project-tasks.json`, `problems.json`, `repair-chain.json`, and `repair-summary.json` artifacts.

It is intentionally not a full command-line IDE replacement. Visual Agent plan controls, Problems/Terminal/Git integration, LSP views, run history, and per-hunk review UI remain desktop IDE workflows.

See [docs/agent_cli_manual.md](docs/agent_cli_manual.md) for usage, safety notes, and the current completeness assessment. See [docs/agent_cli_design.md](docs/agent_cli_design.md) for the planned toolchain-integration and full-automation architecture.

## Git Notes

This repo may have local demo changes. Check status before staging:

```powershell
git status --short
```

Do not include unrelated demo/workspace changes in feature commits.
