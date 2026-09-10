# Agent IDE — Product-Level UI Design Specification

**NOTE:** This document describes the target UI design vision. Features are at various stages of implementation. Status badges indicate current state — see ROADMAP.md for timeline.

---

## 1. Overall Layout `[Implemented]`

```
┌──────────────────────────────────────────────────────────────┐
│ TopBar: Mode Switch | Agent Status | Run | Scope | Git | Settings │
├──────────────┬──────────────────────────────┬────────────────┤
│              │                              │                │
│ File Area    │     Code Editor (Core)       │ Agent Panel    │
│ Explorer     │                              │                │
│ Search       │  ┌────────────────────────┐  │ Chat Layer     │
│ Git          │  │ Code + Inline Suggest  │  │ Task Layer     │
│              │  │ Diff Overlay           │  │ Diff Layer     │
│              │  │ Intent Layer (AI hints)│  │                │
│              │  └────────────────────────┘  │                │
│              │                              │                │
├──────────────┴──────────────────────────────┴────────────────┤
│ Execution Panel: Terminal | Logs | Tests | Agent Actions      │
└──────────────────────────────────────────────────────────────┘
```

---

## 2. Core Areas

### Area 1: Top Control Bar (Global Control) `[Implemented]`

The top bar carries **actions**. Passive status moved to the status bar (Area 1b)
so that one 40px row is not doing both jobs.

**Functions:**
- Agent mode switch: Suggest / Auto
- IDE mode switch: Code / Plan
- Run / Debug / Build / Test, from the project's own declared commands
- Stop, while a run is in flight
- Panel toggles (Explorer / Agent / Terminal), focus mode, theme, command
  palette, shortcuts help
- LSP status entry point — a badge that opens a details popover. It is the one
  status-shaped thing that stays, because it is also the way in to that popover.
- Window controls (minimize / maximize / close)

Not present despite earlier drafts claiming otherwise: scope control
(Current File / Project / Multi-file) and a Git status segment. Scope is chosen
per prompt in the Agent panel; Git state lives in the Source Control panel.

### Area 1b: Status Bar (Passive Status) `[Implemented]`

A 24px row at the bottom edge, outside the bottom panel — it is not a panel, and
focus mode collapses panels.

- Problem counts by severity, clickable to open the Problems panel. Previously
  these were only visible once that panel was already open.
- Cursor line and column. Tracked separately from the selection, because the
  selection is cleared the moment you deselect and "which line am I on" is a
  fact that always holds.
- Active file's language.
- Whether an LLM profile is configured, **with text**. This used to be an
  unlabelled coloured dot in the top bar: the single most consequential piece of
  state in the app — whether the Agent can run at all — required hovering to read.
- Agent state (Idle / Thinking / Planning / Acting / Reviewing / Waiting / Done /
  Error).

Deliberately absent until the data exists: encoding and line-ending, Git branch,
and per-run token spend. An empty segment is worse than no segment.


**Design points:**
- Always visible
- Clear status (color indicates whether AI is executing)

---

### Area 2: Left Side — File Area (Stable Zone) `[Implemented]`

**Contents:**
- File tree (Explorer)
- Search
- Git

**Design principle:**
- Fully maintain VS Code habits
- No AI elements introduced
- Purpose: reduce learning curve

---

### Area 3: Center — Code Editor (Main Stage) `[Implemented]`

**Core principle:** User always has control

**Feature layers:**

#### (A) Base Editing `[Implemented]`
- Multi-file tabs `[Implemented]`
- Split view `[Future - Phase 11]`
- Minimap `[Implemented]`

#### (B) AI Enhancement (Non-intrusive) `[Implemented]`

**Inline Suggestion:** `[Implemented]`
- Gray ghost text (Copilot-like)

**Diff Overlay (Key Feature):** `[Implemented]`
- AI modifications displayed as overlay
- Not directly written to file

Example:
```
- old code
+ new code (AI suggestion)
```

**Intent Layer (Innovation):** `[Implemented]`
- Inline AI intent hints

Example:
```
// Optimize this loop for better performance
```

**Design value:**
- AI "visible but not disruptive"

---

### Area 4: Right Side — Agent Panel (Intelligence Core) `[Implemented]`

**Three-layer structure:**

#### Chat Layer (Conversation) `[Implemented]`
- Input tasks
- Multi-turn dialogue
- Context binding (file / selection)

#### Task Layer (Execution Visualization) `[Implemented]`

Example:
```
Task: Login System

[✓] Create auth.js
[→] Add JWT
[ ] Write tests
[ ] Fix errors
```

**Capabilities:**
- Clickable
- Rollback
- Re-run

#### Diff Layer (Trust Core) `[Implemented]`

Show:
- File changes
- Code diff

**Characteristic:**
- User confirms before applying

---

### Area 5: Bottom — Execution Panel `[Implemented]`

**Contents:**
- Terminal
- Logs
- Tests
- Agent Actions

Example:
```
> npm test
❌ failed

Agent: Fixed test/login.test.js
```

**Design focus:**
- AI operations fully transparent
- Supports traceability

---

## 3. Core Interaction Design

### 3.1 Selection as Context `[Implemented]`
- Select code -> Ask Agent
- Auto-attaches context

### 3.2 Drag-Driven AI `[Future - Phase 10]`
- Drag file to Agent panel
- Drag error log to Chat

### 3.3 Quick Action Layer (Important) `[Implemented]`

Floating on selection:
```
Explain | Fix | Refactor | Optimize
```

### 3.4 AI Control Level Toggle `[Implemented]`

- Suggest (changes wait in the review area)
- Auto (changes are applied when the run finishes)

Two positions, not three. A middle `Edit` position existed and was byte-identical
to Suggest — every backend gate tests for Auto only — so it was removed rather
than given a meaning it would have duplicated from the permission toggles.

This is `AgentMode`, and it is separate from the permission preset in
Settings → Agent Permissions (`read-only` / `create-files` / `run-commands`),
which sets the fine-grained toggles instead. The preset decides what the Agent may
do during a run; the mode decides what happens to its changes afterwards.

### 3.5 Ghost Mode (Background AI) `[Future - Phase 10]`

**Behavior:**
- Pre-analyze project
- Generate potential optimizations
- Don't disturb user

---

## 4. State System `[Implemented]`

**Agent States:**

```
Idle -> Thinking -> Planning -> Acting -> Reviewing
```

**UI Representation:**
- Idle: gray
- Thinking: animated dot
- Acting: progress bar

---

## 5. Differences from Existing IDEs

| Dimension | VS Code | Agent IDE |
|-----------|---------|-----------|
| Center | Editor | Editor + Agent |
| AI Position | Plugin | Core structure |
| Operation | Manual | Command + operation |
| Visualization | None | Task + Diff |

---

## 6. Design Summary

**Core Principles:**

1. Editor First
2. AI Always Available
3. Actions Transparent
4. Control in User

**One-line definition:**

> A "code-centric controllable AI Agent IDE," not a chat tool.

---

## 7. Figma Component Breakdown `[Implemented]`

### 7.1 Design Tokens

**Color:**
- Background: #0D1117
- Panel: #161B22
- Border: #30363D
- Primary: #3B82F6 (blue)
- AI: #8B5CF6 (purple)
- Diff Add: #238636
- Diff Remove: #DA3633
- Diff Modify: #D29922

**Typography:**
- Code / UI / Caption

**Spacing:** 4 / 8 / 12 / 16 / 24
**Radius:** 6 / 10 / 16
**Elevation:** Panel / Modal / Overlay

### 7.2 Atomic Components

- **Button**: Primary / Secondary / Ghost / Danger
  - Size: S / M / L
  - State: Default / Hover / Active / Disabled / Loading
- **Icon Button** (Run / Stop / Diff / Apply)
- **Tag / Chip** (Scope / Mode / Agent)
- **Input**: Chat Input (multiline + attachments), Command Input (single line)
- **Toggle / Segment** (Suggest | Auto)
- **Status Dot** (Idle / Thinking / Acting)

### 7.3 Composite Components

- **Chat Message**: User / Agent / System
  - Supports: code blocks / file references / Diff cards
- **Task Item**: Todo / Doing / Done / Error
  - Actions: Run / Retry / Rollback
- **Diff Card**: File-level / Snippet-level
  - Actions: Apply / Reject / Open in Editor
- **Inline Suggest**: Ghost Text + Accept / Next
- **Intent Hint**: Inline bubble hint

### 7.4 Container Components

- **Editor Container**: Tabs / Split / Minimap / Overlay layers
- **Agent Panel**: Tabs: Chat | Tasks | Diff
- **Bottom Panel**: Terminal / Logs / Tests / Actions
- **Explorer Panel**

### 7.5 Layout Templates

- 3-Column Layout (Explorer / Editor / Agent)
- 2-Column (Editor / Agent)
- Focus Mode (Editor only)

---

## 8. Agent State Machine + Data Flow `[Implemented]`

### 8.1 State Machine

```
Idle
  ↓
Thinking (understanding requirements)
  ↓
Planning (task decomposition)
  ↓
Acting (execute code/commands)
  ↓
Reviewing (generate diff / verify)
  ↓
Waiting User (awaiting confirmation)
  ↓
Done / Error
```

### 8.2 State Events
- USER_PROMPT
- PLAN_READY
- STEP_START / STEP_DONE
- DIFF_READY
- APPLY / REJECT
- ERROR

### 8.3 Data Flow

```
User Input
   ↓
Context Builder (file/selection/project)
   ↓
Planner (task decomposition)
   ↓
Executor (code generation/command execution)
   ↓
Diff Generator
   ↓
UI (Task + Diff)
   ↓
User Confirm
   ↓
Apply Patch -> Editor
```

### 8.4 Key Data Structures

**Task:**
```typescript
{ id, title, status, steps: [], affectedFiles: [] }
```

**Step:**
```typescript
{ id, type: create|edit|run|test, status, logs, diff }
```

**Diff:**
```typescript
{ file, hunks: [], status: pending|applied|rejected }
```

---

## 9. Multi-Agent Collaboration UI `[In-Progress]`

### 9.1 Core Concept

> Not just one Agent, but "role-based Agents"

- Architect
- Coder
- Tester
- Reviewer

### 9.2 Agent List (Right Panel Top) `[In-Progress]`

```
[Architect] [Coder] [Tester] [Reviewer]
```

Status: Active / Idle / Busy

### 9.3 Collaboration View `[In-Progress]`

**Task Pipeline:**
```
Design -> Implement -> Test -> Review -> Merge
```

Each stage handled by a different Agent.

### 9.4 Conflict Resolution UI `[Future - Phase 9]`

When multiple Agents modify the same file:

```
Agent A vs Agent B

[Accept A] [Accept B] [Merge]
```

### 9.5 Advanced Capabilities (Future) `[Future - Phase 10]`

- Agent parallel execution
- Automatic task assignment
- Long-running tasks
- Project-level memory

---

## 10. Final Product Summary

**This is:**

> Editor + Multi-Agent System + Transparent Execution UI

Three things fused into one development environment.

**This is NOT:**
- A chat tool
- An automatic code generator

---

*Design spec complete. Refer to agent_ide_plan.md for technical details, ROADMAP.md for implementation status.*
