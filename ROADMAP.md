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

## Current State: Phase 3 — COMPLETE

Phase 1 + Phase 2 + Phase 3 done as of 2026-04-24.

### What's Built

```
d:\work\agent-ide\
├── ROADMAP.md                          <-- You are here
├── docs/
│   ├── agent_ide_plan.md               <-- Full technical plan (English)
│   └── agent_ide_ui_design.md          <-- UI design specification (English)
│
├── src/                                # React Frontend
│   ├── App.tsx                         # CSS Grid layout, resizable panels, useAgentBridge mount
│   ├── main.tsx                        # Entry point
│   ├── styles/index.css                # Tailwind + scrollbar + terminal styles
│   │
│   ├── stores/
│   │   ├── useLayoutStore.ts           # Panel sizes, visibility, focus mode
│   │   ├── useEditorStore.ts           # Files, contents, dirty state, save
│   │   └── useAgentStore.ts            # Agent state + IPC actions + streaming support
│   │
│   ├── hooks/
│   │   ├── useAgentBridge.ts           # Tauri event -> Zustand store sync
│   │   └── useTauriEvent.ts            # Generic Tauri event listener hook
│   │
│   ├── components/
│   │   ├── layout/
│   │   │   ├── TopBar.tsx              # Mode switch + Run/Stop + panel toggles
│   │   │   ├── LeftPanel.tsx           # Wraps Explorer
│   │   │   ├── AgentPanel.tsx          # Chat/Tasks/Diff tabs
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
│   │   │   └── Terminal.tsx            # xterm.js + FitAddon + WebLinksAddon
│   │   │
│   │   ├── agent/
│   │   │   ├── ChatView.tsx            # Multi-turn chat + streaming display + IPC send
│   │   │   ├── TaskView.tsx            # Step visualization from agent store
│   │   │   └── DiffView.tsx            # Diff list + Apply All / Reject All bulk actions
│   │   │
│   │   └── shared/
│   │       ├── StatusDot.tsx
│   │       ├── ModeSwitch.tsx
│   │       ├── Button.tsx
│   │       ├── Badge.tsx
│   │       └── Spinner.tsx
│   │
│   └── types/
│       ├── agent.ts                    # AgentState, Step, DiffEntry, Task, ChatMessage
│       └── editor.ts                   # FileTab, FileNode, DiffOverlay types
│
├── src-tauri/                          # Rust Backend
│   ├── Cargo.toml                      # deps: portable-pty, tokio, serde, reqwest, etc.
│   └── src/
│       ├── main.rs
│       ├── lib.rs                      # Plugin reg + command handler reg
│       ├── commands/
│       │   ├── mod.rs
│       │   ├── fs.rs                   # read_file, write_file, list_dir, file_exists
│       │   ├── terminal.rs             # spawn/write/resize/kill PTY + TerminalManager
│       │   └── agent.rs                # Agent IPC: prompt/stop/mode/apply/reject + LLM config
│       ├── agent/
│       │   ├── mod.rs
│       │   ├── state_machine.rs        # AgentState enum + AgentStateManager transitions
│       │   ├── orchestrator.rs         # Main flow: prompt -> plan -> execute -> review
│       │   ├── planner.rs              # LLM task decomposition + plan parsing
│       │   ├── executor.rs             # Step execution + diff parsing from LLM output
│       │   └── diff_gen.rs             # Text diff utilities (similar crate)
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
cargo check         # Rust: 0 errors (10 warnings, benign)
```

---

## Future Phases Summary

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
┌──────────────────────────────────────────────────────────┐
│                    Tauri v2 Shell                        │
├──────────────────────────────────────────────────────────┤
│  WebView (React 18)            │  Rust Backend           │
│                                │                         │
│  ┌──────┬──────┬──────────┐   │  Agent State Machine    │
│  │Left  │Editor│Agent     │   │  Agent Orchestrator     │
│  │(FS)  │Monaco│Chat/Task │<--+-- File System            │
│  └──────┴──────┴──────────┘   │  LLM Client (reqwest)   │
│  ┌──────────────────────────┐ │  PTY Terminal           │
│  │  Terminal | Logs         │<--+-- Planner / Executor     │
│  └──────────────────────────┘ │  Diff Generator         │
│                                │                         │
│  Zustand Stores --invoke----->│                         │
│  <-- Tauri Event (listen) ----│                         │
└──────────────────────────────────────────────────────────┘
```

**IPC Commands registered:**
- `read_file_content`, `write_file_content`, `list_directory`, `file_exists`
- `spawn_terminal`, `write_to_terminal`, `resize_terminal`, `kill_terminal`
- `get_agent_state`, `send_agent_prompt`, `stop_agent`
- `set_agent_mode`, `apply_diffs`, `reject_diffs`
- `get_agent_steps`, `get_agent_diffs`, `update_llm_config`

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

*Last updated: 2026-04-24 — Phase 3 complete, Phase 4 pending.*
