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

## Current State: Phase 2 — COMPLETE ✅

Phase 1 + Phase 2 done as of 2026-04-24.

### What's Built

```
d:\work\agent-ide\
├── ROADMAP.md                          ◄── You are here
├── docs/
│   ├── agent_ide_plan.md               ◄── Full technical plan (English)
│   └── agent_ide_ui_design.md          ◄── UI design specification (English)
│
├── src/                                # React Frontend
│   ├── App.tsx                         # CSS Grid layout, resizable panels
│   ├── main.tsx                        # Entry point
│   ├── styles/index.css                # Tailwind + scrollbar + terminal styles
│   │
│   ├── stores/
│   │   ├── useLayoutStore.ts           # Panel sizes, visibility, focus mode
│   │   ├── useEditorStore.ts           # Files, contents, dirty state, save
│   │   └── useAgentStore.ts            # Agent state placeholder
│   │
│   ├── components/
│   │   ├── layout/
│   │   │   ├── TopBar.tsx              # Mode switch + status + settings
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
│   │   │   ├── ChatView.tsx            # Placeholder
│   │   │   ├── TaskView.tsx            # Placeholder
│   │   │   └── DiffView.tsx            # Placeholder
│   │   │
│   │   └── shared/
│   │       ├── StatusDot.tsx
│   │       ├── ModeSwitch.tsx
│   │       ├── Button.tsx
│   │       ├── Badge.tsx
│   │       └── Spinner.tsx
│   │
│   └── types/
│       └── editor.ts                   # FileTab, FileNode, DiffOverlay types
│
├── src-tauri/                          # Rust Backend
│   ├── Cargo.toml                      # deps: portable-pty, tokio, serde, etc.
│   └── src/
│       ├── main.rs
│       ├── lib.rs                      # Plugin reg + command handler reg
│       ├── commands/
│       │   ├── mod.rs
│       │   ├── fs.rs                   # read_file, write_file, list_dir, file_exists
│       │   ├── terminal.rs             # spawn/write/resize/kill PTY + TerminalManager
│       │   └── agent.rs                # Agent state placeholder
│       ├── agent/
│       │   └── state_machine.rs        # AgentState enum (Idle/Thinking/...)
│       └── services/
│           └── mod.rs
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

### Key Data Flow: Open a File

```
Explorer click file
  -> useEditorStore.openFile({ path, name, language })
    -> invoke("read_file_content", { path })    // Rust fs.rs
      -> store fileContents[path] = content
        -> EditorContainer reads fileContents[activeFile]
          -> Monaco renders with key={activeFile}
```

### Key Data Flow: Save a File

```
User presses Ctrl+S
  -> EditorContainer handler triggers saveCurrentFile()
    -> invoke("write_file_content", { path, content })  // Rust fs.rs
      -> markDirty(path, false)
```

### Key Data Flow: File Tree

```
Explorer mount
  -> invoke("list_directory", { path: "." })
    -> filter EXCLUDE_DIRS
      -> react-arborist <Tree> renders

User expands directory
  -> onToggle(id)
    -> invoke("list_directory", { path: id })
      -> setRootData(update node children)
```

### Verification

```
npx tsc --noEmit    # TypeScript: 0 errors
cargo check         # Rust: 0 errors (4 warnings for unused AgentState variants)
```

---

## Next: Phase 2 — COMPLETE ✅

### What was built:
1. **MonacoContext** — React Context sharing editor instance + monaco namespace
2. **InlineSuggestion** — Ghost text via `editor.deltaDecorations` with `after: { content }`
3. **DiffOverlay** — Green/red line backgrounds + glyph margin indicators
4. **IntentHint** — Content widgets below target lines with type-specific styling
5. **QuickActions** — Floating toolbar positioned at selection top via `getTopForLineNumber`

### Data Flow: AI Layer

```
EditorContainer.onMount
  → set editor/monaco ref in state
    → MonacoContext.Provider wraps AI-layer children
      → InlineSuggestion reads inlineSuggestion from store → deltaDecorations
      → DiffOverlay reads diffOverlays from store → deltaDecorations (wholeLine)
      → IntentHint reads intentHints from store → addContentWidget
      → QuickActions reads selectedText/selectedRange → getTopForLineNumber positioning

EditorContainer.onDidChangeCursorSelection
  → model.getValueInRange(selection) → setSelectedText / setSelectedRange
    → QuickActions position re-calculated via useMemo
```

## Next: Phase 3 — Agent System

---

## Future Phases Summary

| Phase | Name | Key Deliverables |
|-------|------|------------------|
| **1** ✅ | Skeleton | Layout, Monaco, Terminal, File Tree |
| **2** ✅ | Editor Enhancements | InlineSuggestion, DiffOverlay, IntentHint, QuickActions |
| **3** ⏳ | Agent System | Rust state machine, LLM streaming, ChatView, TaskView |
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
│  │Left  │Editor│Agent     │   │  PTY Terminal           │
│  │(FS)  │Monaco│Chat/Task │◄──┼── File System            │
│  └──────┴──────┴──────────┘   │  LLM Client (future)    │
│  ┌──────────────────────────┐ │  Git (git2, future)     │
│  │  Terminal | Logs         │◄┼──                         │
│  └──────────────────────────┘ │                         │
│                                │                         │
│  Zustand Stores ──invoke─────►│                         │
│  ◄── Tauri Event (listen) ────│                         │
└──────────────────────────────────────────────────────────┘
```

IPC Commands registered:
- `read_file_content`, `write_file_content`, `list_directory`, `file_exists`
- `spawn_terminal`, `write_to_terminal`, `resize_terminal`, `kill_terminal`
- `get_agent_state`, `send_agent_prompt`, `stop_agent` (placeholder)

Tauri Events emitted:
- `terminal-output` — PTY output to frontend

---

## Technical Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-24 | `useEditorStore.fileContents` as Record<string, string> | Simple cache, sufficient for Phase 1. May need LRU later |
| 2026-04-24 | Monaco `key={activeFile}` for file switching | Forces remount, reliable for different files |
| 2026-04-24 | `portable-pty` over `termion` | Cross-platform (Windows/macOS/Linux) |
| 2026-04-24 | react-arborist over custom tree | Virtual scrolling for large directories |
| 2026-04-24 | `try_clone_reader()` for PTY read | MasterPty doesn't implement Read directly |
| 2026-04-24 | English docs | Avoid encoding issues, better cross-platform |
| 2026-04-24 | React Context for Monaco sharing | Editor instance + monaco namespace shared across AI-layer components |
| 2026-04-24 | `deltaDecorations` for AI layers | Performant bulk decoration updates, no DOM manipulation |
| 2026-04-24 | Content widget for IntentHint | Widgets render inline below lines, styled per hint type |

---

## Commands Cheat Sheet

```bash
# Development
cd d:\work\agent-ide
npm run tauri dev          # Start Tauri + Vite dev server
npm run dev                # Vite only (web)
npx tsc --noEmit          # TypeScript check
cargo check               # Rust check (from src-tauri/)

# Documentation
cat ROADMAP.md             # This file
cat docs/agent_ide_plan.md # Full plan
cat docs/agent_ide_ui_design.md # UI design
```

---

*Last updated: 2026-04-24 — Phase 2 complete, Phase 3 pending.*
