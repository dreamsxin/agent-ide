# Architecture V3

## Canonical Shape

```text
apps/
  desktop/
  client/
  runtime/
packages/
  protocol/
docs/
  ARCHITECTURE.md
  AI_WORKFLOW.md
  DESKTOP_IDE.md
  NEXT_STEPS.md
```

This is now the canonical architecture for the project.

## Module Responsibilities

### apps/desktop

Owns:

- Tauri shell
- native dialogs
- application window lifecycle
- menus and shortcuts
- packaging and updater integration

Does not own:

- complex Agent orchestration logic
- direct business logic unrelated to desktop integration
- protocol ownership

### apps/client

Owns:

- React workbench
- Monaco editor integration
- explorer and panel layout
- review and task presentation
- runtime capability presentation
- user-facing IDE interactions

Does not own:

- unrestricted filesystem access
- secrets
- command execution trust boundary
- Git authority

### apps/runtime

Owns:

- workspace operations
- command execution
- test execution
- Git operations
- provider configuration
- Agent orchestration
- review and verification data preparation
- future retrieval and indexing

This is the trusted local execution boundary and should be implemented in Rust.

### packages/protocol

Owns:

- shared types and contracts
- tool definitions
- request and response models
- runtime status and event models
- review and verification models

No client-runtime behavior should drift without protocol updates first.

## Product Boundary

The product is not:

- a web IDE with desktop support added later

The product is:

- a desktop-first Agent IDE

The browser run path may exist for isolated UI development, but it is not the primary product identity.

## Naming Policy

Target names:

- `apps/client`
- `apps/runtime`
- `packages/protocol`

## Runtime Policy

The runtime is a local IDE runtime, not an HTTP server pretending to be one.

Primary runtime capabilities:

- workspace open
- workspace read
- workspace write
- workspace command run
- workspace save
- workspace search
- test execution
- Git status
- Agent orchestration
- provider and secret handling

## Rust Policy

Architecture V3 uses Rust as the primary implementation language for the runtime and desktop integration.

Rules:

- new trusted-boundary logic should default to Rust
- Tauri commands should stay thin where possible and delegate to runtime modules
- TypeScript should remain concentrated in the client UI
- protocol definitions should be kept explicit and close to the Rust source models

## UI Reference Policy

Use VS Code as a reference for:

- spatial layout
- panel density
- workbench navigation
- editor-centered composition

Do not use VS Code as:

- a source dependency
- a codebase to fork
- the implementation base for this repository

## Migration Principle

During the transition:

- documents follow V3 first
- new work should favor the V3 structure
- legacy code may appear only as migration residue
- runtime ownership should move into Rust early instead of being deferred indefinitely
