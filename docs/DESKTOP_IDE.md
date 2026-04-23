# Desktop IDE V3

The product-level roadmap is maintained in `AI_AGENT_IDE_PLAN.md`.

## Role In Architecture V3

`apps/desktop` is the primary product shell.

It should host:

- the IDE window
- native folder dialogs
- menu and shortcut behavior
- desktop packaging concerns
- desktop-native integration points

It should not replace:

- `apps/client`
- `apps/runtime`
- `packages/protocol`

## Target Relationship

```text
apps/desktop
  -> apps/client
  -> apps/runtime
  -> packages/protocol
```

In practice:

- `desktop` hosts the Tauri app
- `client` renders the workbench
- `runtime` provides trusted local behavior in Rust
- `protocol` defines the contracts between them

## Product Direction

The desktop experience should feel closer to a modern IDE than to a dashboard.

Reference points:

- VS Code for layout clarity
- Codex-style review flow for Agent output
- explicit workspace and safety state
- Agent chat and task composition as first-class workbench surfaces

## Desktop Workbench Expectations

The shell should support:

- title or command area
- native menu bar with core workspace actions
- custom frameless title bar with integrated workspace, menu, and window controls
- activity rail
- explorer or contextual side panels
- editor center
- review or inspection panel
- terminal or logs surface
- status bar

## Layout Plan

The intended workbench layout should stabilize around:

- top title and command strip for current workspace identity and quick actions
- top Agent composer for task planning and decomposition
- native menu bar for `File`, `View`, `Debug`, and `Help`
- left activity rail for major workbench modes
- left side panel for explorer, editor list, review queue, or recent logs
- center editor region with tabs, breadcrumbs, Monaco, or Agent plan workbench
- right context panel for Git, runtime capabilities, and review context
- bottom logs surface for runtime and command output
- status bar for branch, encoding, runtime state, and task status

The menu bar should own desktop-native commands such as:

- open folder
- save active file
- switch major workbench views
- reload window
- devtools access in debug mode

The window surface should keep contextual controls such as:

- quick-access open workspace button for first-run discoverability
- editor-local save affordance
- visible activity rail for mode switching
- status bar for branch, encoding, and runtime state

For text and icon rendering:

- source files should be stored as UTF-8
- UI should avoid fragile glyph choices until font fallback is standardized
- desktop-critical actions should not rely on decorative icons alone
- labels should remain legible even when the host font stack changes

Current workbench direction also includes:

- SVG-based activity and explorer icons instead of fragile text glyphs
- explorer context actions for creating files and folders, renaming entries, deleting entries, and refreshing workspace state through the Rust runtime
- breadcrumb context above the editor for workspace-to-file navigation clarity
- a bottom status bar that surfaces branch, encoding, language, EOL, runtime, and cursor status
- a top command input for workspace-scoped commands with results shown in the logs surface
- live command output streaming into the logs surface while workspace commands run
- workspace task buttons for common verification commands such as Rust check and tests
- a visible Agent chat composer and review workbench for task decomposition
- production IDE chrome with compact panels, clearer contrast, and Agent-first composition
- frameless Tauri window chrome where the React workbench owns the visible title bar, common menu actions, and Windows controls while native menu events remain available for shortcuts and platform integration
- a compact command center rather than a large dashboard-style command form, keeping workspace identity, command execution, task shortcuts, and Agent planning visually balanced like an IDE workbench
- a compact inspector where provider configuration is opened from a small status affordance and runtime capabilities collapse to a summary, leaving the right rail focused on Agent review, Git state, and changed files
- fixed viewport workbench sizing where the application shell does not show a browser-style page scrollbar; sidebars, inspector, editor lists, and bottom panels own their own IDE-style scrollbars
- non-wrapping title/menu actions so desktop commands such as `Open Folder` and `Run Command` remain single-line menu affordances

## Native Boundary

Desktop-native capabilities should be explicit:

- open folder
- reveal in file manager
- menu actions
- keyboard shortcuts
- future system integrations where justified

Those capabilities should remain thin wrappers around the runtime boundary where possible.

## Rust Role

`Tauri + Rust` is the primary desktop technology direction.

Rules:

- desktop-native actions can live in the Tauri layer
- trusted workspace logic should live in runtime modules, not ad hoc frontend code
- command handlers should avoid absorbing unrelated business logic
- runtime modules should remain testable outside the UI shell where possible

## Immediate Architecture Rule

Do not design the desktop shell around the old `web/server` model.

Design it around:

- desktop shell
- thin client workbench
- Rust runtime boundary
- explicit protocol contract
