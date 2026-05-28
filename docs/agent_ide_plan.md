# Agent IDE — Tauri v2 + React Project Plan

> **NOTE**: This document reflects the original implementation plan. For the current project state and roadmap, see **ROADMAP.md** (canonical source of truth).

> **Goal**: Code-centric controllable AI Agent IDE, prioritizing performance and interaction
> **Stack**: Rust (Tauri v2) + React 18 + TypeScript + Tailwind CSS + Monaco Editor

---

## 1. Tech Stack Selection

| Layer | Technology | Rationale |
|-------|------------|-----------|
| Shell Framework | **Tauri v2** | Native performance, tiny bundle (~5MB), Rust backend, cross-platform |
| Frontend | **React 18** + TypeScript | Mature ecosystem, existing skeleton code reuse |
| Styling | **Tailwind CSS** | Atomic CSS, seamless alignment with existing designs |
| Editor | **Monaco Editor** | VS Code kernel, native Diff/Syntax/IntelliSense |
| Terminal | **xterm.js** + Tauri PTY | Real terminal, ANSI sequence support |
| State Mgmt | **Zustand** | Lightweight, no boilerplate, subscribe + selector |
| Build Tool | **Vite** | Sub-second HMR, native ESM |
| IPC | Tauri Commands (IPC) + Tauri Events (push) | Bidirectional real-time |
| AI Backend | Rust sidecar / HTTP streaming | Key security (not exposed to frontend) |
| Packaging | Tauri Bundle (MSI/DMG/AppImage) | Full cross-platform coverage |

---

## 2. Project Directory Structure

> The directory tree below reflects the **current** file structure as of Phase 8. Items from the original plan that were renamed or removed during implementation are not shown.

```
agent-ide/
├── src-tauri/                    # Rust Backend
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── capabilities/
│   │   └── default.json
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs                # Plugin registration
│   │   ├── agent/
│   │   │   ├── mod.rs
│   │   │   ├── state_machine.rs  # State machine
│   │   │   ├── planner.rs        # Task decomposition
│   │   │   ├── executor.rs       # Code execution
│   │   │   ├── orchestrator.rs   # Pipeline orchestration
│   │   │   ├── diff_gen.rs       # Diff generation
│   │   │   ├── diff_apply.rs     # Diff application with conflict detection
│   │   │   └── multi_agent.rs    # Multi-agent collaboration
│   │   ├── bin/
│   │   │   └── agent_cli.rs      # Headless CLI runner
│   │   ├── cli/
│   │   │   └── mod.rs            # CLI argument parsing
│   │   ├── commands/
│   │   │   ├── mod.rs
│   │   │   ├── fs.rs             # File system operations
│   │   │   ├── terminal.rs       # PTY terminal
│   │   │   ├── git.rs            # Git operations
│   │   │   ├── lsp.rs            # LSP integration
│   │   │   ├── agent.rs          # Agent dispatch
│   │   │   └── tasks.rs          # Project task discovery
│   │   └── services/
│   │       ├── mod.rs
│   │       ├── llm_client.rs     # LLM API (streaming)
│   │       ├── llm_profiles.rs   # LLM provider profiles & credentials
│   │       ├── context.rs        # Context builder
│   │       ├── credentials.rs    # OS credential store
│   │       ├── workspace.rs      # Workspace resolution & boundary checks
│   │       ├── problem_parser.rs # Terminal output → Problems parsing
│   │       ├── project_tasks.rs  # Shared project task runner
│   │       └── agent_runtime.rs  # Shared Agent step runtime
│   └── icons/
│
├── src/                          # React Frontend
│   ├── main.tsx
│   ├── App.tsx                   # Root layout
│   ├── styles/
│   │   └── index.css             # Tailwind + global styles
│   ├── stores/
│   │   ├── useAgentStore.ts      # Agent state, mode, tasks, diffs
│   │   ├── useEditorStore.ts     # Open files, content cache, suggestions
│   │   ├── useLayoutStore.ts     # Panel sizes, visibility, focus mode
│   │   ├── useGitStore.ts        # Git status, diff, branches
│   │   ├── useLogStore.ts        # Action log entries
│   │   ├── useLspStore.ts        # LSP status & diagnostics
│   │   ├── useProblemStore.ts    # Unified Problems (diagnostics, test, Agent)
│   │   ├── useTaskStore.ts       # Project task sessions & run history
│   │   └── useThemeStore.ts      # Theme state
│   ├── components/
│   │   ├── layout/
│   │   │   ├── TopBar.tsx
│   │   │   ├── LeftPanel.tsx
│   │   │   ├── AgentPanel.tsx
│   │   │   ├── BottomPanel.tsx
│   │   │   └── ResizeHandle.tsx
│   │   ├── editor/
│   │   │   ├── EditorContainer.tsx
│   │   │   ├── EditorTabs.tsx
│   │   │   ├── InlineSuggestion.tsx
│   │   │   ├── DiffOverlay.tsx
│   │   │   ├── IntentHint.tsx
│   │   │   ├── QuickActions.tsx
│   │   │   ├── DiagnosticsBridge.tsx      # Monaco diagnostics → Problems
│   │   │   ├── ProblemsMarkerBridge.tsx   # Problems → Monaco markers
│   │   │   └── MonacoContext.tsx           # Monaco instance provider
│   │   ├── agent/
│   │   │   ├── ChatView.tsx
│   │   │   ├── TaskView.tsx
│   │   │   ├── DiffView.tsx
│   │   │   ├── TaskPipeline.tsx
│   │   │   ├── PipelineEditor.tsx         # Pipeline stage editor
│   │   │   ├── AgentSelector.tsx
│   │   │   └── SettingsPanel.tsx           # LLM profiles & settings
│   │   ├── panels/
│   │   │   ├── Explorer.tsx
│   │   │   ├── GitPanel.tsx
│   │   │   ├── Terminal.tsx
│   │   │   ├── LogView.tsx
│   │   │   ├── ProblemsPanel.tsx           # Unified Problems view
│   │   │   └── TasksPanel.tsx              # Project commands
│   │   └── shared/
│   │       ├── CommandPalette.tsx           # Unified command entry
│   │       ├── ErrorBoundary.tsx
│   │       ├── ModeSwitch.tsx
│   │       ├── ShortcutsHelp.tsx
│   │       └── StatusDot.tsx
│   ├── hooks/
│   │   ├── useAgentBridge.ts
│   │   ├── useFixWithAgent.ts             # Fix with Agent from Problems/Commands
│   │   ├── useLspDiagnostics.ts
│   │   ├── useProjectTasks.ts             # Shared task discovery hook
│   │   ├── useRunProjectTask.ts           # Shared task runner hook
│   │   ├── useShortcuts.ts
│   │   └── useTauriEvent.ts
│   ├── types/
│   │   ├── agent.ts
│   │   ├── editor.ts
│   │   └── project.ts
│   └── utils/
│       ├── agentRuntimeContext.ts          # IDE failure context injection
│       ├── codeCompletion.ts              # Local Monaco completion provider
│       ├── lspClient.ts                   # Frontend LSP bridge
│       ├── paths.ts                       # Path normalization
│       ├── tauri.ts                       # Runtime detection
│       ├── terminalProblemParser.ts       # Terminal output → Problems
│       └── typescriptSemantic.ts          # Monaco TS worker defaults
│
├── docs/
│   ├── agent_ide_plan.md           # This file (original plan)
│   ├── agent_ide_design.md         # Detailed current design
│   ├── agent_ide_ui_design.md      # UI design specs
│   ├── agent_cli_design.md         # CLI design document
│   ├── agent_cli_manual.md         # CLI manual
│   ├── agent_changes_schema.md     # Agent changes protocol schema
│   ├── smoke_test.md               # Runtime regression checklist
│   └── skeleton.jsx                # Original skeleton reference
├── scripts/
│   └── package-windows.ps1
├── demo/
│   ├── hello.js
│   ├── hello.js.backup
│   └── e2e_test.ps1
├── index.html
├── package.json
├── package-lock.json
├── tsconfig.json
├── tsconfig.node.json
├── vite.config.ts
├── tailwind.config.js
├── postcss.config.js
└── ROADMAP.md                      # Implementation roadmap (canonical)
```

---

## 3. Component Tree

```
<App>                                          # CSS Grid main layout
├── <TopBar>                                   # grid-row:1 / col-span:3
│   ├── Logo + ProjectName
│   ├── Project Run/Build/Test/Debug buttons
│   ├── <ModeSwitch />                         # Suggest | Edit | Auto
│   ├── LSP Status
│   └── <StatusDot /> + Run/Stop/Settings
│
├── <LeftPanel>                                # grid-row:2 / col-start:1
│   ├── <Explorer />                           # File tree (react-arborist)
│   └── <GitPanel />                           # Source Control
│
├── <EditorContainer>                          # grid-row:2 / col-start:2
│   ├── <EditorTabs />
│   ├── <MonacoContext />                       # Monaco instance provider
│   ├── Monaco Editor (core)
│   ├── <InlineSuggestion />                   # Ghost Text layer
│   ├── <DiffOverlay />                        # Diff highlight layer
│   ├── <IntentHint />                         # AI bubble layer
│   ├── <QuickActions />                       # Selection floating bar
│   ├── <DiagnosticsBridge />                  # Monaco diagnostics → Problems
│   └── <ProblemsMarkerBridge />               # Problems → Monaco markers
│
├── <AgentPanel>                               # grid-row:2 / col-start:3
│   ├── TabHeader: [Chat | Tasks | Diff]
│   ├── <AgentSelector />
│   ├── <SettingsPanel />                      # LLM profiles & settings
│   ├── <ChatView />
│   │   └── Context source toggles & budget
│   ├── <TaskView />
│   │   └── <TaskPipeline /> / <PipelineEditor />
│   └── <DiffView />                           # Per-file & per-hunk review
│
├── <BottomPanel>                              # grid-row:3 / col-span:3
│   ├── <Terminal />                           # xterm.js + PTY (multi-session)
│   ├── <TasksPanel />                         # Project commands & run history
│   ├── <ProblemsPanel />                      # Unified diagnostics/problems
│   └── <LogView />                            # Agent & system action logs
│
├── <CommandPalette />                         # Ctrl+Shift+P overlay
├── <ShortcutsHelp />
└── <ResizeHandle /> x3                        # Panel resize handles
```

---

## 4. Rust Backend Module Architecture

### 4.1 IPC Commands

> **Note**: The command signatures below are from the original plan and may not reflect current naming or parameter shapes. See `src-tauri/src/commands/` for actual implementations.

```rust
// commands/fs.rs
#[tauri::command] async fn read_file(path: String) -> Result<String, String>
#[tauri::command] async fn write_file(path: String, content: String) -> Result<(), String>
#[tauri::command] async fn list_dir(path: String) -> Result<Vec<FileEntry>, String>
#[tauri::command] async fn watch_files(paths: Vec<String>) -> ...

// commands/terminal.rs
#[tauri::command] async fn spawn_terminal(id: String) -> Result<(), String>
#[tauri::command] async fn write_to_terminal(id: String, data: String) -> Result<(), String>
#[tauri::command] async fn resize_terminal(id: String, cols: u16, rows: u16) -> Result<(), String>
#[tauri::command] async fn kill_terminal(id: String) -> Result<(), String>

// commands/git.rs
#[tauri::command] async fn git_status(path: String) -> Result<GitStatus, String>
#[tauri::command] async fn git_diff(path: String) -> Result<String, String>
#[tauri::command] async fn git_commit(path: String, msg: String) -> Result<(), String>

// commands/agent.rs
#[tauri::command] async fn send_prompt(prompt: String, context: AgentContext) -> ...
#[tauri::command] async fn stop_agent() -> ...
#[tauri::command] async fn apply_diff(diff_id: String) -> ...
#[tauri::command] async fn reject_diff(diff_id: String) -> ...

// Additional commands added since original plan:
// commands/lsp.rs — LSP status, hover, completion, definition, symbols, rename, code actions
// commands/tasks.rs — Project task discovery from package.json/Cargo.toml
// Per-file and per-hunk apply/reject diff commands
// LLM profile management and context compression commands
```

### 4.2 Agent State Machine

```rust
enum AgentState {
    Idle,
    Thinking,       // Understanding requirements
    Planning,       // Task decomposition -> emit PlanReady
    Acting,         // Execute code/commands -> emit StepStart/StepDone
    Reviewing,      // Generate Diff -> emit DiffReady
    WaitingUser,    // Awaiting confirmation
    Done,
    Error(String),
}

enum AgentEvent {
    UserPrompt(String),
    PlanReady(Vec<Step>),
    StepStart(String),
    StepDone(StepResult),
    DiffReady(Diff),
    UserApply,
    UserReject,
    Error(String),
}

// State transitions + Tauri Event push to frontend
// Frontend uses useTauriEvent("agent-state-changed", ...) for real-time UI updates
```

### 4.3 Multi-Agent Collaboration

```rust
enum AgentRole {
    Architect,    // Architecture design
    Coder,        // Code implementation
    Tester,       // Testing
    Reviewer,     // Code review
}

struct MultiAgentPipeline {
    stages: Vec<PipelineStage>,  // Design -> Implement -> Test -> Review
    current: usize,
}
```

---

## 5. Frontend State Management (Zustand)

```typescript
// stores/useAgentStore.ts
interface AgentStore {
  state: AgentState;
  mode: 'suggest' | 'edit' | 'auto';
  currentTask: Task | null;
  steps: Step[];
  diffs: Diff[];
  activeAgents: AgentRole[];
  sendPrompt: (text: string) => void;
  applyDiff: (id: string) => void;
  rejectDiff: (id: string) => void;
  retryStep: (id: string) => void;
}

// stores/useEditorStore.ts
interface EditorStore {
  openFiles: FileTab[];
  activeFile: string | null;
  fileContents: Record<string, string>;  // path -> content cache
  inlineSuggestions: InlineSuggestion[];
  diffOverlays: DiffOverlay[];
  intentHints: IntentHint[];
  openFile: (tab: FileTab) => Promise<void>;  // loads from Tauri FS
  closeFile: (path: string) => void;
  saveCurrentFile: () => Promise<void>;       // saves to Tauri FS
  updateFileContent: (path: string, content: string) => void;
}

// stores/useLayoutStore.ts
interface LayoutStore {
  leftWidth: number;         // default 240
  rightWidth: number;        // default 360
  bottomHeight: number;      // default 240
  leftVisible: boolean;
  rightVisible: boolean;
  bottomVisible: boolean;
  focusMode: boolean;        // editor only
}

// Additional stores added since original plan:
// stores/useGitStore.ts      — Git status, diff modes, branches, conflicts
// stores/useLogStore.ts      — Action log entries per workspace
// stores/useLspStore.ts      — LSP server status & diagnostics
// stores/useProblemStore.ts  — Unified Problems (diagnostics, test, Agent findings)
// stores/useTaskStore.ts     — Project task sessions, run history, terminal output
// stores/useThemeStore.ts    — Theme state
```

---

## 6. Core Interaction Flows

### 6.1 User Sends Prompt

```
User types in Chat "implement login feature"
  -> Zustand: sendPrompt("implement login feature")
    -> Tauri invoke("send_agent_prompt", { prompt, context, sources })
      -> Rust Agent state machine starts
        -> State: Thinking (emit event -> frontend StatusDot turns purple)
        -> State: Planning (emit PlanReady -> TaskView renders steps)
        -> State: Acting  (emit StepStart/StepDone -> TaskView real-time update)
        -> State: Reviewing (emit DiffReady -> DiffView + DiffOverlay display)
        -> State: WaitingUser
  -> User clicks Apply
    -> Tauri invoke("apply_diffs") or per-hunk apply
      -> Rust writes files
      -> Frontend Monaco updates content
```

### 6.2 Selection Quick Actions

```
User selects code block in Editor
  -> QuickActions floats: [Explain | Fix | Refactor | Optimize]
    -> Click "Explain"
      -> Auto-build context (file path + selection content + line range)
      -> Chat sends: "Explain the following code: [selection]"
      -> Normal Agent planning/review/diff flow
```

### 6.3 Problems → Fix with Agent

```
Monaco markers, terminal test/lint output, or Agent error events
  -> useProblemStore
    -> ProblemsPanel displays unified list
      -> Click "Fix with Agent" on a problem
        -> Build IDE runtime failure context (failed command, Problems, Terminal output, Logs)
        -> Send focused repair prompt through Agent
          -> Normal Agent planning/review/diff flow
```

### 6.4 Drag Context (Original Plan)

```
User drags file to Agent panel
  -> DragEvent carries file path
    -> Chat auto-attaches context: "Current file: auth.js"
User drags Error Log to Chat
  -> Chat auto-attaches: "Fix the following error: [log content]"
```

---

## 7. Performance Optimization

| Strategy | Implementation |
|----------|----------------|
| Monaco lazy load | `React.lazy(() => import('@monaco-editor/react'))` |
| Virtual file tree | react-arborist virtual scrolling |
| Streaming AI response | Rust backend SSE/Stream -> Tauri Event -> React setState batch |
| Diff incremental | Rust side diffs library on-demand, frontend renders visible area only |
| Terminal buffer limit | xterm.js scrollback limit 5000 lines |
| Code splitting | Vite code-split: editor chunk / agent chunk / panels chunk |
| Web Worker | Large file parsing offload to Worker |
| Tauri multithreading | Rust tokio async runtime, PTY/git/fs concurrent |

---

## 8. Key Dependencies

### Rust (Cargo.toml)
```toml
[dependencies]
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-shell = "2"
tauri-plugin-fs = "2"
tauri-plugin-dialog = "2"
tauri-plugin-process = "2"
portable-pty = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
reqwest = { version = "0.12", features = ["stream"] }  # LLM HTTP
similar = "2"                                           # Diff algorithm
git2 = "0.19"                                           # Git operations
uuid = "1"
```

### Frontend (package.json)
```json
{
  "dependencies": {
    "react": "^18.3",
    "react-dom": "^18.3",
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-shell": "^2",
    "@tauri-apps/plugin-fs": "^2",
    "@monaco-editor/react": "^4.6",
    "monaco-editor": "^0.50",
    "@xterm/xterm": "^5.4",
    "@xterm/addon-fit": "^0.9",
    "@xterm/addon-web-links": "^0.9",
    "react-arborist": "^3.4",
    "zustand": "^4.5",
    "react-markdown": "^9",
    "react-syntax-highlighter": "^15"
  }
}
```

---

## 9. Phased Implementation Plan

> Phase numbers in this section correspond to the original plan. The ROADMAP.md uses an expanded phase numbering that reflects the actual implementation sequence. Cross-reference: Original Phase 1 ≈ ROADMAP Phase 1; Original Phases 2–5 were expanded into ROADMAP Phases 2–8+.

### Phase 1 — Skeleton ✅ COMPLETE
- [x] Tauri v2 project init + window config
- [x] React + Vite + Tailwind integration
- [x] CSS Grid main layout + resizable panels
- [x] TopBar / LeftPanel / Editor / AgentPanel / BottomPanel placeholders
- [x] Monaco Editor basic integration (real file loading + Ctrl+S)
- [x] xterm.js + Tauri PTY terminal
- [x] File tree (react-arborist + Tauri FS lazy loading)

### Phase 2 — Editor Enhancements ✅ COMPLETE
- [x] EditorTabs multi-file management
- [x] InlineSuggestion Ghost Text layer
- [x] DiffOverlay rendering (Monaco diff mode)
- [x] IntentHint bubbles
- [x] QuickActions selection floating bar
- [x] File save/sync watcher
- [x] DiagnosticsBridge (Monaco diagnostics → Problems)
- [x] ProblemsMarkerBridge (Problems → Monaco markers)
- [x] MonacoContext provider
- [x] Local code completion (keywords, symbols, snippets, paths)
- [x] TypeScript/JavaScript semantic completion via Monaco TS worker
- [x] TypeScript LSP integration (hover, completion, definition, symbols, rename, code actions, diagnostics)
- [x] Go LSP integration (gopls detection/startup, completion, hover, diagnostics)
- [x] Quick Fix / code action application with state sync

### Phase 3 — Agent System ✅ COMPLETE
- [x] Rust side Agent state machine
- [x] LLM HTTP streaming call + Tauri Event push
- [x] ChatView multi-turn conversation
- [x] TaskView step visualization
- [x] DiffView change confirmation (per-file and per-hunk apply/reject)
- [x] Suggest/Edit/Auto mode switch
- [x] Agent action log with prompt/context/diff provenance
- [x] Role-aware pipeline execution: architect → coder → tester → reviewer
- [x] Structured `agent-changes` JSON protocol with validation
- [x] Hunk-level provenance for structured changes
- [x] Context compression modes (full, focused, compact, budgeted)
- [x] Context source toggles and token budget estimates
- [x] Interactive plan controls (edit, reorder, skip, run single step, regenerate)
- [x] Pipeline pause-before-stage controls and paused snapshots
- [x] Agent state restoration after reload
- [x] SettingsPanel for LLM provider profiles and credential management
- [x] Failed/stale diff regeneration against current file

### Phase 4 — Multi-Agent + Advanced ✅ COMPLETE
- [x] Multi-Agent role selection and collaboration
- [x] TaskPipeline visualization
- [x] Git panel (status/diff/commit, stage/unstage/discard, branch ops, remote actions, conflict resolution)
- [x] LogView timeline (Agent action logs with expandable details)
- [x] Project-level context memory (workspace persistence)
- [x] Problems panel (diagnostics, Agent findings, terminal test/lint parsing)
- [x] Project Tasks panel with discovered commands and run history
- [x] Fix with Agent from Problems and failed commands
- [x] Command Palette (Ctrl+Shift+P)
- [x] LLM provider profiles with backward-compatible config migration
- [x] Per-profile model budget metadata
- [x] OS credential store for LLM API keys
- [x] Terminal multi-session UI with task exit tracking
- [x] Git credential inputs for remote actions
- [x] Per-workspace state persistence (logs, context flags, diffs, task sessions)
- [x] Agent CLI headless automation runner (doctor, context estimate, plan, run, repair loops)

### Phase 5 — Polish & Release ✅ COMPLETE
- [x] Keyboard shortcut system
- [x] Theme customization
- [x] Animation polish (state transitions, Diff fade-in)
- [x] Windows packaging (NSIS/MSI)
- [x] Performance benchmarks (build/lint/type checks pass)
- [x] Workspace boundary enforcement (FS, Git, Agent, Terminal, CLI)
- [x] Browser/Tauri runtime guards for Vite preview mode
- [x] Context compression test coverage
- [x] Frontend test coverage (path normalization, terminal problem parsing)

### Phase 6 — Stabilization and Safety ✅ COMPLETE
- [x] Workspace boundary applied consistently across FS, Agent, Git, terminal cwd, and CLI
- [x] LLM key storage moved to OS credential store
- [x] Diff application returns structured errors to UI
- [x] Agent cancellation token wired through orchestrator and LLM client
- [x] Browser preview mode has clear disabled states
- [x] Roadmap and docs reflect actual project state

### Phase 7 — Agent Execution Quality ✅ COMPLETE
- [x] Role-aware orchestration: architect → coder → tester → reviewer
- [x] Pipeline stages influence prompts and state transitions
- [x] Agent action log with prompt/context/diff provenance
- [x] Reviewer uses actual pending diff summaries for structured review
- [x] Context sources: selected files, open files, git diff, project tree summary, terminal/log excerpts
- [x] Context compression strategy interface (full, focused, compact, budgeted)
- [x] Structured model protocol with `agent-changes` version 1 schema
- [x] Hunk-level provenance for structured changes and reviewer findings
- [x] Formal `agent-changes` schema validation and diagnostics

### Phase 8 — IDE Workflow Completion 🔄 IN PROGRESS
- [ ] Terminal fully wired to backend PTY (spawn, write, resize, output, kill) — **mostly complete, needs runtime validation**
- [ ] TopBar exposes common Run/Debug/Build/Test commands — **wired, needs polish**
- [ ] QuickActions sends real Agent prompts — **implemented, needs runtime testing**
- [ ] DiffView supports per-file and per-hunk apply/reject — **wired, needs clearer mixed-hunk status**
- [ ] Git panel supports stage, unstage, discard with confirmation — **implemented, needs SSH/passphrase UX and richer merge editor**
- [ ] Editor has local code completion for common languages and current-file symbols — **implemented, needs LLM inline completion**
- [ ] Problems panel replaces static samples and accepts Monaco diagnostics, Agent findings, parsed terminal failures — **implemented, needs richer test-runner protocol integration**
- [ ] Logs panel consumes backend and Agent event streams — **implemented, needs persisted action logs**
- [ ] Runtime validation across all daily IDE workflows in `npm run tauri -- dev`
- [ ] TypeScript/Go LSP large-workspace indexing validation
- [ ] Frontend/Tauri smoke tests for daily workflows

### Phase 9 — Release Readiness ⏳ UPCOMING
- [ ] CI for TypeScript, Rust, tests, formatting
- [ ] Tauri smoke tests for app boot, workspace open, file read/write, settings load
- [ ] Packaging validation for Windows first
- [ ] Security model documentation
- [ ] Troubleshooting guide for Vite vs Tauri dev modes

### Future — Phase 10/11
- [ ] Ghost Mode (background analysis without user-initiated prompts)
- [ ] Split View (side-by-side editor panes)
- [ ] Advanced inline suggestions from LLM (real-time code completion via LLM streaming)
- [ ] Full workspace diagnostics indexing beyond opened files
- [ ] Richer test-runner protocol integration
- [ ] SSH/passphrase UX for Git
- [ ] Richer merge editor UI for conflict blocks
- [ ] Web Worker offload for large file parsing
- [ ] Dynamic imports / manual chunks for bundle size reduction

---

## 10. Architecture Diagram

```
┌──────────────────────────────────────────────────────────┐
│                    Tauri v2 Shell                        │
├──────────────────────────────────────────────────────────┤
│  WebView (React 18)            │  Rust Backend           │
│                                │                         │
│  ┌──────┬──────┬──────────┐   │  ┌───────────────────┐  │
│  │Left  │Editor│Agent     │   │  │ Agent State Mach. │  │
│  │240px │Flex  │360px     │◄──┼──┤ Planner/Executor  │  │
│  │      │      │          │   │  │ Orchestrator      │  │
│  │Expl..│Monaco│Chat/Task │   │  │ Diff Gen/Apply    │  │
│  │      │      │          │   │  │ Multi-Agent       │  │
│  │Git   │      │Settings  │   │  └───────────────────┘  │
│  └──────┴──────┴──────────┘   │  ┌───────────────────┐  │
│  ┌──────────────────────────┐ │  │ File System       │  │
│  │  Bottom 240px            │◄┼──┤ (read/write/watch)│  │
│  │  Terminal | Commands |   │ │  └───────────────────┘  │
│  │  Problems | Logs         │ │  ┌───────────────────┐  │
│  └──────────────────────────┘ │  │ PTY Terminal      │  │
│                                │  │ (portable-pty)    │  │
│  Zustand Stores ───invoke────►│  └───────────────────┘  │
│  ◄────── listen(event) ────────│  ┌───────────────────┐  │
│                                │  │ LLM Client        │  │
│  Command Palette               │  │ (reqwest stream)  │  │
│  ShortcutsHelp                 │  │ LLM Profiles      │  │
│  ErrorBoundary                 │  └───────────────────┘  │
│                                │  ┌───────────────────┐  │
│                                │  │ Git (git2)        │  │
│                                │  └───────────────────┘  │
│                                │  ┌───────────────────┐  │
│                                │  │ LSP Bridge        │  │
│                                │  │ (TS/JS + Go)      │  │
│                                │  └───────────────────┘  │
│                                │  ┌───────────────────┐  │
│                                │  │ Agent CLI         │  │
│                                │  │ (headless runner) │  │
│                                │  └───────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

---

## 11. Comparison with Original Skeleton

| Dimension | Original JSX Skeleton | Tauri Enhancement |
|-----------|----------------------|-------------------|
| File tree | Static mock | react-arborist + Tauri FS real files |
| Editor | `<pre>` tag | Monaco Editor (syntax/IntelliSense/Diff) |
| Terminal | Static text | xterm.js + PTY real terminal |
| State mgmt | React state | Zustand global + Tauri Event real-time sync |
| AI calls | None | Rust backend streaming SSE -> frontend Event push |
| Panels | Fixed grid | Resizable ResizeHandle |
| Git | None | Rust git2 integration |
| Multi-Agent | None | Rust side role scheduling |
| File writes | None | Tauri FS safe writes + confirmation flow |
| LSP | None | TypeScript/JS + Go LSP with diagnostics and code actions |
| Problems | None | Unified panel for Monaco diagnostics, test failures, Agent findings |
| CLI | None | Headless Agent runner with repair loops |

---

*This document preserves the original design intent and architecture decisions. For current project state, see ROADMAP.md.*
