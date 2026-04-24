# Agent IDE — Tauri v2 + React Project Plan

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

```
agent-ide/
├── src-tauri/                    # Rust Backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs                # Plugin registration
│   │   ├── commands/
│   │   │   ├── mod.rs
│   │   │   ├── fs.rs             # File system operations
│   │   │   ├── terminal.rs       # PTY terminal
│   │   │   ├── git.rs            # Git operations
│   │   │   └── agent.rs          # Agent dispatch
│   │   ├── agent/
│   │   │   ├── mod.rs
│   │   │   ├── state_machine.rs  # State machine
│   │   │   ├── planner.rs        # Task decomposition
│   │   │   ├── executor.rs       # Code execution
│   │   │   ├── diff_gen.rs       # Diff generation
│   │   │   └── multi_agent.rs    # Multi-agent collaboration
│   │   ├── services/
│   │   │   ├── mod.rs
│   │   │   ├── llm_client.rs     # LLM API (streaming)
│   │   │   ├── context.rs        # Context builder
│   │   │   └── project.rs        # Project analysis
│   │   └── utils/
│   │       ├── mod.rs
│   │       └── diff.rs           # Diff algorithm
│   └── icons/
│
├── src/                          # React Frontend
│   ├── main.tsx
│   ├── App.tsx                   # Root layout
│   ├── styles/
│   │   └── index.css             # Tailwind + global styles
│   ├── stores/
│   │   ├── useAgentStore.ts
│   │   ├── useEditorStore.ts
│   │   ├── useLayoutStore.ts
│   │   └── useProjectStore.ts
│   ├── components/
│   │   ├── layout/
│   │   │   ├── TopBar.tsx
│   │   │   ├── LeftPanel.tsx
│   │   │   ├── AgentPanel.tsx
│   │   │   ├── BottomPanel.tsx
│   │   │   └── ResizeHandle.tsx
│   │   ├── editor/
│   │   │   ├── EditorContainer.tsx
│   │   │   ├── CodeLayer.tsx
│   │   │   ├── InlineSuggestion.tsx
│   │   │   ├── DiffOverlay.tsx
│   │   │   ├── IntentHint.tsx
│   │   │   ├── QuickActions.tsx
│   │   │   └── EditorTabs.tsx
│   │   ├── agent/
│   │   │   ├── ChatView.tsx
│   │   │   ├── TaskView.tsx
│   │   │   ├── DiffView.tsx
│   │   │   ├── TaskPipeline.tsx
│   │   │   └── AgentSelector.tsx
│   │   ├── panels/
│   │   │   ├── Explorer.tsx
│   │   │   ├── SearchPanel.tsx
│   │   │   ├── GitPanel.tsx
│   │   │   ├── Terminal.tsx
│   │   │   ├── LogView.tsx
│   │   │   └── TestView.tsx
│   │   └── shared/
│   │       ├── StatusDot.tsx
│   │       ├── ModeSwitch.tsx
│   │       ├── Button.tsx
│   │       ├── Badge.tsx
│   │       └── Spinner.tsx
│   ├── hooks/
│   │   ├── useTauriCommand.ts
│   │   ├── useTauriEvent.ts
│   │   ├── useDragDrop.ts
│   │   └── useKeyboard.ts
│   └── types/
│       ├── agent.ts
│       ├── editor.ts
│       └── project.ts
│
├── docs/
│   ├── agent_ide_plan.md           # This file
│   ├── agent_ide_ui_design.md      # UI design specs
│   └── skeleton.jsx                # Original skeleton reference
├── index.html
├── package.json
├── tsconfig.json
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
│   ├── <ModeSwitch />                         # Suggest | Edit | Auto
│   └── <StatusDot /> + Run/Stop/Settings
│
├── <LeftPanel>                                # grid-row:2 / col-start:1
│   ├── <Explorer />                           # File tree (react-arborist)
│   ├── <SearchPanel />
│   └── <GitPanel />
│
├── <EditorContainer>                          # grid-row:2 / col-start:2
│   ├── <EditorTabs />
│   ├── <CodeLayer />                          # Monaco Editor (core)
│   ├── <InlineSuggestion />                   # Ghost Text layer
│   ├── <DiffOverlay />                        # Diff highlight layer
│   ├── <IntentHint />                         # AI bubble layer
│   └── <QuickActions />                       # Selection floating bar
│
├── <AgentPanel>                               # grid-row:2 / col-start:3
│   ├── TabHeader: [Chat | Tasks | Diff]
│   ├── <AgentSelector />
│   ├── <ChatView />
│   ├── <TaskView />
│   │   └── <TaskPipeline />
│   └── <DiffView />
│
├── <BottomPanel>                              # grid-row:3 / col-span:3
│   ├── <Terminal />                           # xterm.js + PTY
│   ├── <LogView />
│   ├── <TestView />
│   └── <ActionHistory />
│
└── <ResizeHandle /> x3                        # Panel resize handles
```

---

## 4. Rust Backend Module Architecture

### 4.1 IPC Commands

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
```

---

## 6. Core Interaction Flows

### 6.1 User Sends Prompt

```
User types in Chat "implement login feature"
  -> Zustand: sendPrompt("implement login feature")
    -> Tauri invoke("send_prompt", { prompt, context })
      -> Rust Agent state machine starts
        -> State: Thinking (emit event -> frontend StatusDot turns purple)
        -> State: Planning (emit PlanReady -> TaskView renders steps)
        -> State: Acting  (emit StepStart/StepDone -> TaskView real-time update)
        -> State: Reviewing (emit DiffReady -> DiffView + DiffOverlay display)
        -> State: WaitingUser
  -> User clicks Apply
    -> Tauri invoke("apply_diff")
      -> Rust writes files
      -> Frontend Monaco updates content
```

### 6.2 Selection Quick Actions

```
User selects code block in Editor
  -> QuickActions floats: [Explain | Fix | Refactor | Optimize]
    -> Click "Explain"
      -> Auto-build context (file path + selection content)
      -> Chat sends: "Explain the following code: [selection]"
```

### 6.3 Drag Context

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

### Phase 1 — Skeleton (2 weeks) ✅ COMPLETE
- [x] Tauri v2 project init + window config
- [x] React + Vite + Tailwind integration
- [x] CSS Grid main layout + resizable panels
- [x] TopBar / LeftPanel / Editor / AgentPanel / BottomPanel placeholders
- [x] Monaco Editor basic integration (real file loading + Ctrl+S)
- [x] xterm.js + Tauri PTY terminal
- [x] File tree (react-arborist + Tauri FS lazy loading)

### Phase 2 — Editor Enhancements (2 weeks)
- [ ] EditorTabs multi-file management (implemented as placeholder)
- [ ] InlineSuggestion Ghost Text layer
- [ ] DiffOverlay rendering (Monaco diff mode)
- [ ] IntentHint bubbles
- [ ] QuickActions selection floating bar
- [ ] File save/sync watcher

### Phase 3 — Agent System (3 weeks)
- [ ] Rust side Agent state machine
- [ ] LLM HTTP streaming call + Tauri Event push
- [ ] ChatView multi-turn conversation
- [ ] TaskView step visualization
- [ ] DiffView change confirmation
- [ ] Suggest/Edit/Auto mode switch

### Phase 4 — Multi-Agent + Advanced (2 weeks)
- [ ] Multi-Agent role selection and collaboration
- [ ] TaskPipeline visualization
- [ ] Git panel (status/diff/commit)
- [ ] LogView timeline
- [ ] Ghost Mode background analysis
- [ ] Project-level context memory

### Phase 5 — Polish & Release (1 week)
- [ ] Keyboard shortcut system
- [ ] Theme customization
- [ ] Animation polish (state transitions, Diff fade-in)
- [ ] Cross-platform packaging (Windows/MSI + macOS/DMG + Linux/AppImage)
- [ ] Performance benchmarks

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
│  │      │      │          │   │  │ Diff Generator    │  │
│  │Expl..│Monaco│Chat/Task │   │  └───────────────────┘  │
│  │      │      │          │   │  ┌───────────────────┐  │
│  └──────┴──────┴──────────┘   │  │ File System       │  │
│  ┌──────────────────────────┐ │  │ (read/write/watch)│  │
│  │  Bottom 240px            │◄┼──┤                   │  │
│  │  Terminal | Logs | Tests │ │  └───────────────────┘  │
│  └──────────────────────────┘ │  ┌───────────────────┐  │
│                                │  │ PTY Terminal      │  │
│  Zustand Stores ───invoke────►│  │ (portable-pty)    │  │
│  ◄────── listen(event) ────────│  └───────────────────┘  │
│                                │  ┌───────────────────┐  │
│                                │  │ LLM Client        │  │
│                                │  │ (reqwest stream)  │  │
│                                │  └───────────────────┘  │
│                                │  ┌───────────────────┐  │
│                                │  │ Git (git2)        │  │
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

---

*Plan complete. See ROADMAP.md for implementation status and context recovery.*
