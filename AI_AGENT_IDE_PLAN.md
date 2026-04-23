# AI Agent IDE Product Plan

## 1. Product Vision

Build an industrial-grade desktop AI Agent IDE for professional software work.

The product should combine:

- a modern local IDE workbench
- a trusted Rust runtime boundary
- explicit protocol contracts
- AI Agent planning, editing, verification, and review loops
- safe workspace operations
- reproducible command and test execution
- enterprise-ready security, observability, and configuration

The IDE is not a chat window beside a code editor. It is an agentic development environment where the Agent can inspect, plan, edit, run, verify, and explain changes through controlled local capabilities.

## 2. Product Principles

### 2.1 Desktop First

The primary product is a desktop application.

The desktop shell owns:

- native workspace selection
- native menus and shortcuts
- window lifecycle
- local trust and permission surfaces
- packaging and update flow
- platform integration

Browser development mode may exist for UI iteration, but it is not the product boundary.

### 2.2 Runtime Trusted Boundary

All trusted local behavior belongs in Rust runtime modules.

The runtime owns:

- workspace open, list, read, write, delete, and search
- command and test execution
- Git state and diff operations
- Agent orchestration
- provider configuration and secret handling
- indexing and retrieval
- patch application
- verification workflows
- audit and redaction logic

The client presents state and sends user intent. It must not become the filesystem, Git, shell, provider, or secret authority.

### 2.3 Protocol First

Runtime-client behavior should be represented by explicit contracts.

The protocol layer owns:

- request and response models
- runtime event streams
- workspace descriptors
- file operation models
- command execution models
- Agent task, plan, diff, review, and verification models
- provider status and capability models
- structured error types

Protocol changes should happen before major behavior expansion.

### 2.4 Agent As Reviewer And Operator

The Agent should support an accountable workflow:

1. understand workspace context
2. propose a plan
3. request or use scoped capabilities
4. edit through runtime-controlled tools
5. run verification
6. present diffs and risks
7. preserve user control

The Agent should never silently perform high-impact actions without visible state and review affordances.

## 3. Canonical Repository Shape

```text
apps/
  desktop/        # Tauri shell and native integration
  client/         # React + Monaco workbench
  runtime/        # Rust trusted local runtime
packages/
  protocol/       # shared contracts, schemas, generated bindings
docs/
  ARCHITECTURE.md
  AI_WORKFLOW.md
  DESKTOP_IDE.md
  NEXT_STEPS.md
AI_AGENT_IDE_PLAN.md
```

## 4. Core Workbench

The IDE workbench should include:

- frameless desktop title bar with native window controls
- command center
- activity rail
- explorer with context menu operations
- Monaco editor with tabs, dirty state, save, and syntax modes
- Agent composer
- Agent plan and review board
- Git and runtime context inspector
- command/test logs panel
- status bar with workspace, branch, language, cursor, encoding, and runtime state

The UI should feel dense, durable, and operational. It should favor repeatable developer workflows over marketing-style presentation.

## 5. Runtime Capability Roadmap

### 5.1 Workspace Operations

- open workspace
- list hierarchical files
- read text files
- write and save files
- delete files and directories with workspace-root protection
- rename and move files
- create files and folders
- reveal path in file manager
- workspace search
- binary/large-file detection
- ignore rules and traversal limits

### 5.2 Git Operations

- current branch
- dirty state
- file-level changes
- diff read
- staged/unstaged separation
- apply and revert selected hunks
- create branch
- commit preparation
- conflict visibility

### 5.3 Command And Test Execution

- run configured workspace commands
- stream stdout/stderr
- cancel process trees
- maintain execution history
- classify exit states
- detect common test frameworks
- attach verification commands to Agent plans

### 5.4 Agent Orchestration

- decompose user task into plan steps
- bind plan steps to runtime capabilities
- persist plan history
- execute scoped tool calls
- produce patches and diffs
- run verification
- summarize risks and residual work
- support human approval gates

### 5.5 Provider Layer

- runtime-owned provider configuration
- runtime-owned secret storage
- OpenAI-compatible provider adapter
- request and response logging with redaction
- model capability descriptors
- retry and timeout policies
- offline fallback planning

### 5.6 Indexing And Retrieval

- workspace symbol and text index
- incremental indexing
- ignore generated/vendor folders
- retrieval for Agent context
- file relevance scoring
- cache invalidation on file changes

## 6. Industrial-Grade Requirements

### 6.1 Safety

- every filesystem mutation goes through runtime validation
- paths must be canonicalized and constrained to workspace root
- destructive operations require visible confirmation
- secrets are never stored in frontend state beyond form entry
- provider logs redact secrets and prompt-sensitive material
- Agent tool actions are auditable

### 6.2 Reliability

- runtime modules are independently testable
- command execution handles cancellation and finalization clearly
- old or malformed persisted runtime data should not prevent workspace open
- UI state should recover from command, provider, and file operation failures
- logs should preserve enough context to debug failures

### 6.3 Performance

- file walking must skip heavy directories such as `.git`, `node_modules`, `target`, and `dist`
- large workspaces need depth limits, pagination, or indexing
- command output should stream without blocking the UI
- editor operations should avoid unnecessary full-workspace refreshes
- indexing must become incremental before large-project support is declared complete

### 6.4 Security

- no trusted shell, filesystem, Git, provider, or secret behavior in the frontend
- no silent command execution by Agent without policy and UI state
- provider secrets should move toward OS keychain or encrypted local storage
- audit logs should avoid leaking credentials, full prompts, or private file content by default
- plugin and extension systems must be sandboxed or explicitly permissioned

### 6.5 Enterprise Readiness

- deterministic settings model
- import/export of workspace-safe settings
- policy layer for provider and command permissions
- proxy and custom endpoint support
- telemetry toggle and local-only mode
- signed builds and update channel strategy
- crash reporting strategy with privacy controls

## 7. Delivery Phases

### Phase 0: V3 Foundation

Status: mostly established.

Deliver:

- Rust workspace
- Tauri desktop shell
- React workbench
- Rust runtime crate
- protocol package placeholder
- runtime-backed workspace open/read/write/delete
- Git status
- command execution
- basic Agent plan loop

### Phase 1: IDE File Operations

Deliver:

- create file/folder
- rename entries
- move entries
- delete with context menu and keyboard support
- refresh and filesystem state reconciliation
- safer handling of malformed plan history
- file operation protocol models

Current status:

- create, rename, delete, and refresh are implemented through the Rust runtime
- move and richer non-prompt file operation UI remain next

Definition of done:

- common explorer operations work without shell commands
- all mutations go through Rust runtime
- open tabs and Git state reconcile after file operations

### Phase 2: Protocol Hardening

Deliver:

- shared protocol package with generated or mirrored TypeScript types
- structured runtime errors
- versioned event models
- capability descriptors
- file operation models
- command execution lifecycle models

Definition of done:

- UI-runtime contracts are explicit and reviewed before behavior expands
- frontend no longer hand-maintains broad duplicate protocol shapes

### Phase 3: Execution System

Deliver:

- execution registry with history
- cancellation finalization semantics
- command templates
- test discovery
- log retention controls
- structured stdout/stderr events

Definition of done:

- users can run, cancel, inspect, and rerun commands reliably
- Agent plans can attach verification runs to steps

### Phase 4: Real Provider Adapter

Deliver:

- provider adapter inside Rust runtime
- OpenAI-compatible request path
- model configuration
- timeout/retry handling
- redacted request/response logs
- local fallback when provider is unavailable

Definition of done:

- provider-backed planning works through runtime only
- secrets never become durable frontend state

### Phase 5: Agent Editing Loop

Deliver:

- scoped file read/search/write tools
- patch generation
- diff preview
- approval gates
- verification run binding
- review summary

Definition of done:

- Agent can propose and apply a small code change with user review and verification

### Phase 6: Industrial Hardening

Deliver:

- indexing and retrieval
- settings and policy layer
- OS keychain or encrypted secret store
- signed packaging
- update channels
- crash/log privacy controls
- large workspace performance work

Definition of done:

- the product is suitable for daily professional use on real repositories

## 8. Current Priorities

The current near-term priority is to finish the local IDE loop before expanding advanced Agent behavior:

1. complete explorer file operations: create, rename, move, delete, refresh
2. improve malformed persisted data handling without weakening new schema requirements
3. extract protocol contracts from runtime/client duplication
4. improve execution registry history and cancel/finalize states
5. implement provider adapter inside Rust runtime
6. add redacted provider logging
7. add diff and patch review surfaces

## 9. Non-Goals

This project should not:

- fork or embed VS Code source
- become a browser-first web IDE
- move trusted local operations into TypeScript
- hide destructive Agent actions
- implement provider-specific logic directly in the UI
- build advanced orchestration before the local workspace loop is reliable

## 10. Foundation Definition Of Done

The foundation is complete when:

- the V3 directory structure exists
- desktop launches through Tauri
- workspace open/read/write/delete goes through Rust runtime
- Monaco editing works with tabs and dirty state
- command execution streams logs
- Git status is visible
- Agent plan creation and persistence work
- provider configuration is runtime-owned
- docs and next steps reflect the current architecture

## 11. Product Definition Of Done

The industrial-grade AI Agent IDE is complete when:

- it can safely operate on large real-world repositories
- it can plan, edit, verify, and review code changes through controlled runtime capabilities
- it provides reliable command/test execution and history
- it protects secrets and sensitive prompts
- it exposes clear user approval gates for high-impact actions
- it packages and updates like a professional desktop product
- it remains understandable and recoverable when things fail
