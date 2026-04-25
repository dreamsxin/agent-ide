# Agent IDE — Implementation Roadmap

> **This file is the canonical source of truth for project state.**
> If you resume work after an interruption, start here.

---

## Quick Recovery

After any interruption, restore context in this order:

1. **Read this file** — understand what's done and what's next
2. **Read `docs/agent_ide_plan.md`** — full technical plan
3. **Read `docs/agent_ide_ui_design.md`** — UI design specs
4. **Check `.workbuddy/memory/`** — recent work logs
5. **Run `cargo check && npx tsc --noEmit`** — verify code compiles

---

## Project Identity

| Field | Value |
|-------|-------|
| **Project** | Agent IDE |
| **Description** | Code-centric controllable AI Agent IDE |
| **Stack** | Tauri v2 (Rust) + React 18 + TypeScript + Tailwind CSS |
| **Editor** | Monaco Editor (`@monaco-editor/react`) |
| **Terminal** | xterm.js + Tauri PTY (`portable-pty`) |
| **File Tree** | react-arborist + Tauri FS |
| **State** | Zustand |
| **Build** | Vite |
| **Root** | `d:\work\agent-ide` |

---

## Current State: Phase 5 — COMPLETE + UI Polish

All 5 phases done as of 2026-04-24. Recent additions: custom titlebar, workspace open, enhanced file tools, Agent roles & LLM config, UI clarity improvements.

### What's Built

```
d:\work\agent-ide\
├── ROADMAP.md                          <-- You are here
├── docs/
│   ├── agent_ide_plan.md               <-- Full technical plan (English)
│   └── agent_ide_ui_design.md          <-- UI design specification (English)
│
├── src/                                # React Frontend
│   ├── App.tsx                         # Layout, animated panels, shortcuts, theme support
│   ├── main.tsx                        # Entry point
│   ├── styles/index.css                # Tailwind + Light/Dark theme + animations
│   │
│   ├── stores/
│   │   ├── useLayoutStore.ts           # Panel sizes, visibility, focus mode, tabs
│   │   ├── useEditorStore.ts           # Files, contents, dirty state, save
│   │   ├── useAgentStore.ts            # Agent state + IPC actions + streaming support
│   │   ├── useGitStore.ts              # Git status, diff, commit actions
│   │   ├── useLogStore.ts              # Log entries with source/level tracking
│   │   └── useThemeStore.ts            # Dark/Light theme with localStorage persistence
│   │
│   ├── hooks/
│   │   ├── useAgentBridge.ts           # Tauri event -> Zustand store sync
│   │   ├── useTauriEvent.ts            # Generic Tauri event listener hook
│   │   └── useShortcuts.ts             # Centralized keyboard shortcut system
│   │
│   ├── components/
│   │   ├── layout/
│   │   │   ├── TopBar.tsx              # Mode switch, Run/Stop, panel toggles, theme, help
│   │   │   ├── LeftPanel.tsx           # Explorer/Git tab switching
│   │   │   ├── AgentPanel.tsx          # Chat/Tasks/Diff/Pipeline tabs
│   │   │   ├── BottomPanel.tsx         # Terminal/Logs/Tests/Actions tabs
│   │   │   └── ResizeHandle.tsx        # Drag-to-resize panels
│   │   │
│   │   ├── editor/
│   │   │   ├── EditorContainer.tsx     # Monaco + Ctrl+S + onMount context
│   │   │   ├── EditorTabs.tsx          # File tab bar
│   │   │   ├── MonacoContext.tsx       # Shared editor instance + monaco ns
│   │   │   ├── InlineSuggestion.tsx    # Ghost text decoration
│   │   │   ├── DiffOverlay.tsx         # Diff line highlight (green/red)
│   │   │   ├── IntentHint.tsx          # AI hint content widgets
│   │   │   └── QuickActions.tsx        # Selection floating toolbar
│   │   │
│   │   ├── panels/
│   │   │   ├── Explorer.tsx            # react-arborist + Tauri FS lazy load
│   │   │   ├── Terminal.tsx            # xterm.js + FitAddon + WebLinksAddon
│   │   │   ├── GitPanel.tsx            # Source control: status, diff viewer, commit
│   │   │   └── LogView.tsx             # Log timeline with source icons, auto-scroll
│   │   │
│   │   ├── agent/
│   │   │   ├── ChatView.tsx            # Multi-turn chat + streaming display + IPC send
│   │   │   ├── TaskView.tsx            # Step visualization from agent store
│   │   │   ├── DiffView.tsx            # Diff list + Apply All / Reject All bulk actions
│   │   │   ├── TaskPipeline.tsx        # Pipeline timeline with status indicators
│   │   │   └── AgentSelector.tsx       # Agent role selector with descriptions
│   │   │
│   │   └── shared/
│   │       ├── StatusDot.tsx
│   │       ├── ModeSwitch.tsx
│   │       ├── ShortcutsHelp.tsx       # F1 shortcut reference modal
│   │       ├── Button.tsx
│   │       ├── Badge.tsx
│   │       └── Spinner.tsx
│   │
│   └── types/
│       ├── agent.ts                    # AgentState, Step, DiffEntry, Task, ChatMessage, PipelineStage
│       ├── editor.ts                   # FileTab, FileNode, DiffOverlay types
│       └── project.ts                  # ProjectInfo, GitStatus, GitStatusEntry, LogEntry
│
├── src-tauri/                          # Rust Backend
│   ├── Cargo.toml                      # deps: portable-pty, tokio, serde, reqwest, git2, etc.
│   └── src/
│       ├── main.rs
│       ├── lib.rs                      # Plugin reg + command handler reg
│       ├── commands/
│       │   ├── mod.rs
│       │   ├── fs.rs                   # read_file, write_file, list_dir, file_exists
│       │   ├── terminal.rs             # spawn/write/resize/kill PTY + TerminalManager
│       │   ├── git.rs                  # git_status, git_diff, git_commit (discover)
│       │   └── agent.rs                # Agent IPC: prompt/stop/mode/apply/reject + LLM config
│       ├── agent/
│       │   ├── mod.rs
│       │   ├── state_machine.rs        # AgentState enum + AgentStateManager transitions
│       │   ├── orchestrator.rs         # Main flow: prompt -> plan -> execute -> review
│       │   ├── planner.rs              # LLM task decomposition + plan parsing
│       │   ├── executor.rs             # Step execution + diff parsing from LLM output
│       │   ├── diff_gen.rs             # Text diff utilities (similar crate)
│       │   └── multi_agent.rs          # AgentRole, PipelineStage, default_pipeline
│       └── services/
│           ├── mod.rs
│           ├── llm_client.rs           # OpenAI-compatible HTTP streaming client
│           └── context.rs              # AgentContext builder (file/selection/project)
│
├── .workbuddy/memory/                  # Cross-session memory
│   ├── 2026-04-24.md                   # Daily log
│   └── MEMORY.md                       # Long-term facts
│
├── package.json                        # All npm deps installed
├── tsconfig.json
├── vite.config.ts
├── tailwind.config.js
└── postcss.config.js
```

### Key Data Flows

**Open a File:**
```
Explorer click file
  -> useEditorStore.openFile({ path, name, language })
    -> invoke("read_file_content", { path })
      -> store fileContents[path] = content
        -> EditorContainer reads fileContents[activeFile]
          -> Monaco renders with key={activeFile}
```

**Save a File:**
```
Ctrl+S -> EditorContainer handler -> saveCurrentFile()
  -> invoke("write_file_content", { path, content })
    -> markDirty(path, false)
```

**Agent System:**
```
ChatView.handleSend()
  -> useAgentStore.sendPrompt({ prompt, activeFile, selection ... })
    -> invoke("send_agent_prompt", { request })
      -> Rust AgentOrchestrator.run()
        -> State: Thinking -> emit "agent-state-changed"
        -> LLM plan_task() via streaming -> emit "agent-stream-token"
        -> State: Planning -> emit "agent-plan-ready" (steps[])
        -> For each step:
            -> State: Acting -> emit "agent-step-update"
            -> LLM execute_step() via streaming
            -> Parse diffs from LLM output
        -> State: Reviewing -> emit "agent-diff-ready" (diffs[])
        -> State: WaitingUser -> emit "agent-state-changed"

Frontend Bridge (useAgentBridge):
  -> agent-state-changed  -> setState() / setMode()
  -> agent-plan-ready     -> setSteps()
  -> agent-step-update    -> updateStep()
  -> agent-diff-ready     -> setDiffs()
  -> agent-stream-token   -> appendStreamContent() -> ChatView real-time render

User applies/rejects diffs:
  DiffView -> applyAllDiffs() / rejectAllDiffs()
    -> invoke("apply_diffs") / invoke("reject_diffs")
      -> State: Done -> emit "agent-state-changed"
```

### Verification

```
npx tsc --noEmit    # TypeScript: 0 errors
cargo check         # Rust: 0 errors (15 warnings, benign)
```

---

## All Phases Summary

| Phase | Name | Key Deliverables |
|-------|------|------------------|
| **1** | Skeleton | Layout, Monaco, Terminal, File Tree |
| **2** | Editor Enhancements | InlineSuggestion, DiffOverlay, IntentHint, QuickActions |
| **3** | Agent System | Rust state machine, LLM streaming, ChatView, TaskView, DiffView |
| **4** | Multi-Agent | Agent roles, TaskPipeline, Git panel, LogView |
| **5** | Polish & Release | Shortcuts, themes, animations, cross-platform packaging |

---

## Architecture at a Glance

```
+----------------------------------------------------------+
|                    Tauri v2 Shell                        |
+----------------------------------------------------------+
|  WebView (React 18)            |  Rust Backend           |
|                                |                         |
|  +------+------+----------+   |  Agent State Machine    |
|  |Left  |Editor|Agent     |   |  Agent Orchestrator     |
|  |(FS)  |Monaco|Chat/Task |<--+-- File System            |
|  +------+------+----------+   |  LLM Client (reqwest)   |
|  +--------------------------+ |  PTY Terminal           |
|  |  Terminal | Logs         |<--+-- Planner / Executor     |
|  +--------------------------+ |  Diff Generator         |
|                                |  Git (git2)            |
|  Zustand Stores --invoke----->|                         |
|  <-- Tauri Event (listen) ----|                         |
+----------------------------------------------------------+
```

**IPC Commands registered:**
- FS: `read_file_content`, `write_file_content`, `list_directory`, `file_exists`
- FS: `delete_path`, `create_file`, `create_directory`, `rename_path`
- FS: `copy_path`, `get_file_metadata`, `search_files` (new)
- FS: `watch_start`, `watch_stop`
- Terminal: `spawn_terminal`, `write_to_terminal`, `resize_terminal`, `kill_terminal`
- Agent: `get_agent_state`, `send_agent_prompt`, `stop_agent`, `set_agent_mode`
- Agent: `apply_diffs`, `reject_diffs`, `get_agent_steps`, `get_agent_diffs`
- Agent: `update_llm_config`, `get_llm_config` (new)
- Agent: `set_active_role`, `get_active_role` (new)
- Agent: `get_pipeline`, `update_pipeline`, `reset_pipeline` (new)
- Git: `git_status`, `git_diff`, `git_commit`

**Tauri Events emitted:**
- `terminal-output` — PTY output to frontend
- `agent-state-changed` — Agent state transitions
- `agent-plan-ready` — Task steps after LLM planning
- `agent-step-update` — Step status changes
- `agent-diff-ready` — Diff entries after code generation
- `agent-stream-token` — Real-time LLM streaming tokens

---

## Technical Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-24 | `fileContents` as Record<string, string> | Simple cache, sufficient for Phase 1 |
| 2026-04-24 | Monaco `key={activeFile}` for file switching | Forces remount, reliable |
| 2026-04-24 | `portable-pty` over `termion` | Cross-platform |
| 2026-04-24 | react-arborist over custom tree | Virtual scrolling |
| 2026-04-24 | `try_clone_reader()` for PTY read | MasterPty doesn't implement Read |
| 2026-04-24 | English docs | Avoid encoding issues |
| 2026-04-24 | React Context for Monaco sharing | Shared across AI-layer components |
| 2026-04-24 | `deltaDecorations` for AI layers | Performant batch decoration |
| 2026-04-24 | Content widget for IntentHint | Inline rendering below lines |
| 2026-04-24 | `tokio::sync::Mutex` for orchestrator | Safe lock across .await |
| 2026-04-24 | reqwest SSE streaming for LLM | Standard OpenAI-compatible API |
| 2026-04-24 | `useAgentBridge` hook pattern | Single mount for event->store sync |
| 2026-04-24 | `similar` crate for diff | Fast text diff utilities |
| 2026-04-24 | `git2::Repository::discover` | Walk up to find .git in Tauri |
| 2026-04-24 | Theme via `data-theme` + CSS vars | Runtime switching, no rebuild |
| 2026-04-24 | `useShortcuts` hook | Centralized, group-aware shortcut registry |
| 2026-04-24 | Custom titlebar (`decorations: false`) | Native-like IDE experience |
| 2026-04-24 | `copy_path` + `search_files` + `get_file_metadata` | Complete file mgmt for AI agent |
| 2026-04-24 | Structured config status card | Replaced cryptic "Connected: url · model(key)" display |

---

## Commands Cheat Sheet

```bash
# Development
cd d:\work\agent-ide
npm run tauri dev          # Start Tauri + Vite dev server
npm run dev                # Vite only (web)
npx tsc --noEmit          # TypeScript check
cargo check               # Rust check (from src-tauri/)
```

---

*Last updated: 2026-04-24 — All phases complete. Custom titlebar, workspace, agent roles/LLM config, file tools, UI polish done.*
