# Architecture V3 Plan

## 1. Goal

Rebuild the project as a desktop-first Agent IDE with:

- `Rust` as the primary implementation language
- `Tauri` as the desktop shell and application host
- a thin `client` UI focused on IDE interaction surfaces
- a local `runtime` implemented in Rust as the trusted execution boundary
- a shared `protocol` that is defined from the runtime outward

This reset removes ambiguity in:

- product shape
- runtime ownership
- trust boundaries
- migration sequencing
- AI implementation handoff

## 2. Product Definition

The target product has four clear layers:

1. `desktop`
2. `client`
3. `runtime`
4. `protocol`

### 2.1 Desktop

Owns:

- Tauri application shell
- native folder dialogs
- native menus and shortcuts
- window lifecycle
- desktop packaging
- future OS integration points

### 2.2 Client

Owns:

- IDE workbench UI
- Monaco editor integration
- explorer, tabs, panels, and status presentation
- Agent planning, logs, diffs, and verification presentation
- runtime capability presentation

The client should stay thin. It presents state and sends intent. It should not become the trust boundary.

### 2.3 Runtime

Owns:

- workspace open, read, write, save, and search
- command execution
- test execution
- Git integration
- Agent orchestration
- provider configuration and secret handling
- future indexing, retrieval, verifier loops, and patch application

The runtime is the trusted local execution boundary and should be implemented in Rust from the start of V3.

### 2.4 Protocol

Owns:

- request and response contracts
- runtime event models
- workspace descriptors
- tool definitions
- review and verification models
- capability and status models

The protocol should be defined explicitly and versioned alongside the Rust runtime.

## 3. Naming

The canonical structure is:

- `apps/desktop`
- `apps/client`
- `apps/runtime`
- `packages/protocol`

## 4. Technical Direction

### 4.1 Language Strategy

Primary language:

- Rust

Secondary language:

- TypeScript for the Tauri frontend workbench where it improves UI development speed

Rule:

- new runtime behavior should default to Rust
- protocol contracts should originate from Rust domain models
- frontend logic should remain presentation-oriented

### 4.2 Desktop Stack

Recommended stack:

- Tauri v2
- Rust for shell commands and runtime hosting
- React + TypeScript for the workbench UI
- Monaco Editor in the client

### 4.3 Runtime Strategy

The runtime should be a Rust crate or Rust workspace that is callable from the Tauri layer and remains separable from the UI shell.

Preferred internal Rust modules:

- `workspace`
- `command`
- `git`
- `agent`
- `protocol`
- `indexing`
- `verification`

### 4.4 Protocol Strategy

Protocol-first rules:

- define request and response models before expanding features
- use stable Rust types as the source of truth
- generate or mirror TypeScript client bindings from protocol definitions when practical
- keep UI and runtime behavior aligned through protocol updates

## 5. UI Direction

The workbench should move toward a modern IDE spatial model:

- title or command area
- activity rail
- left explorer or navigation panel
- center editor
- right review or inspection panel
- bottom logs or terminal surface
- status bar
- tab strip
- dirty-state and save feedback surfaces

The UI should feel inspired by VS Code in layout clarity, but it should present:

- stronger Agent review flow
- explicit workspace trust state
- explicit runtime capability state
- clear syntax highlighting and language-aware editing
- a polished, visually intentional desktop workbench

## 6. Runtime Capabilities For Foundation

The first V3 runtime foundation should support:

- open workspace
- list files
- read file
- write file
- save file
- search workspace
- run command
- run tests
- inspect Git status
- expose runtime capabilities
- execute a minimal Agent task loop

## 7. Non-goals

This reset does not mean:

- copying the VS Code codebase into this repository
- building a browser-first product
- putting all logic into the frontend
- implementing advanced AI orchestration before the IDE loop works
- polishing every platform integration before the local loop is stable

## 8. Delivery Phases

### Phase V3-0: Documentation Reset

- align `PLAN.md` and all docs with a Rust-first architecture
- create `docs/NEXT_STEPS.md`
- declare Rust as the primary language for runtime development

### Phase V3-1: Workspace Skeleton

- create `apps/desktop`
- create `apps/client`
- create `apps/runtime`
- create `packages/protocol`
- create root workspace configuration for Rust and frontend tooling
- define minimal dev commands

### Phase V3-2: Minimal Closed Loop

- launch Tauri shell
- open local workspace through native dialog
- render hierarchical file explorer
- open file in syntax-highlighted editor
- edit and save file through Rust runtime
- show tabs and dirty file state
- show Git status

### Phase V3-3: Runtime Service Layer

- add command execution APIs
- add test execution APIs
- add runtime event stream for logs and task status
- establish protocol-driven error handling

### Phase V3-4: Agent Loop

- submit Agent task from UI
- run orchestrated local task in Rust runtime
- stream logs and status back to client
- show review artifacts and diffs

### Phase V3-5: IDE Hardening

- indexing and workspace search optimization
- patch and diff application improvements
- verification helpers
- settings, secrets, and provider management
- packaging and platform QA
- layout polish, panel balance, and responsive workbench tuning
- visual refinement of explorer, tabs, logs, and review surfaces

## 9. Recommended Repository Shape

```text
apps/
  client/         # React + Monaco workbench
  desktop/        # Tauri application shell
  runtime/        # Rust runtime crate(s)
packages/
  protocol/       # shared schema, generated bindings, docs
docs/
  ARCHITECTURE.md
  AI_WORKFLOW.md
  DESKTOP_IDE.md
  NEXT_STEPS.md
```

## 10. Definition Of Done For Foundation

The foundation is complete only when:

- the V3 directory structure exists
- the docs describe the Rust-first architecture consistently
- the Tauri shell can open and display a workspace
- file read and write flow goes through the Rust runtime
- the UI can show runtime capability and Git state
- the next milestone after foundation is explicit in `docs/NEXT_STEPS.md`
