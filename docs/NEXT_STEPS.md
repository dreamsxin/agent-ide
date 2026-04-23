# Next Steps

## Current Target

Build the V3 foundation into an industrial-grade desktop AI Agent IDE using Tauri for the shell, Rust for the trusted runtime, and a thin client workbench.

The product-level roadmap now lives in `AI_AGENT_IDE_PLAN.md`.

## Phase 1: Repository Skeleton

Deliver:

- `apps/desktop` Tauri application
- `apps/client` workbench frontend
- `apps/runtime` Rust runtime crate
- `packages/protocol` shared contract package

Tasks:

- initialize a Rust workspace at the repository root
- scaffold Tauri v2 in `apps/desktop`
- scaffold React + TypeScript in `apps/client`
- add `apps/runtime` as a Rust crate for trusted local behavior
- decide whether `packages/protocol` stores generated bindings, schema docs, or both

Definition of done:

- the repository builds
- the desktop shell launches
- the client is mounted inside Tauri
- the runtime crate is linked into the application

## Phase 2: Minimal IDE Loop

Deliver:

- open folder
- hierarchical file tree
- syntax-highlighted editor
- tabbed editing
- dirty file state
- save file
- Git status

Tasks:

- add native folder picker in desktop shell
- implement workspace descriptor protocol models
- implement runtime file listing and file read APIs
- implement runtime file write and save APIs
- render a hierarchical explorer and Monaco editor
- add syntax highlighting and language-aware editor modes
- add editor tabs and dirty-state feedback
- show Git branch and dirty state in the workbench

Definition of done:

- a local folder can be opened from the UI
- clicking a file opens it in the editor
- edits can be saved through the Rust runtime
- Git state is visible in the workbench
- code is syntax-highlighted in the editor
- tabs and dirty-state feedback are visible and reliable

## Phase 3: Command And Logs Surface

Deliver:

- command execution
- test execution
- streaming logs

Tasks:

- add runtime command execution module
- add protocol event models for progress and logs
- upgrade the current log panel into runtime-backed logs
- add basic test runner action

Definition of done:

- a configured command can be run from the UI
- output streams back into the workbench
- test execution can be triggered and observed

## Phase 4: Agent Loop

Deliver:

- task submission
- execution status
- review and diff presentation

Tasks:

- define Agent task request and result contracts
- implement a minimal orchestrator inside `apps/runtime`
- stream task status to the client
- show plan, logs, and diff output in review panels

Definition of done:

- a user can submit an Agent task
- the runtime executes the task locally
- output is visible in the review UI

## Recommended Early Technical Decisions

- use a Rust workspace from day one
- keep Tauri command handlers thin
- make runtime modules independently testable
- keep frontend state shaped by protocol contracts
- avoid putting filesystem or Git logic in TypeScript
- treat editor polish as part of the core workbench milestone
- optimize layout density and visual hierarchy alongside feature development

## Immediate Next Task

Current focus after the workspace loop:

1. complete explorer move operation and polish file-operation prompts beyond the current prompt-based create, rename, delete, and refresh flow
2. handle malformed persisted Agent data as a non-blocking workspace warning while keeping the new schema strict
3. extract protocol contracts from duplicated Rust and TypeScript shapes
4. improve execution registry semantics with history and clearer cancel/finalize states
5. implement the real provider adapter inside the Rust runtime only
6. add provider request/response logging that redacts prompt-sensitive and secret material
7. add diff and patch review surfaces for Agent-applied changes
