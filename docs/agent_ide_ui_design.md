# Agent IDE — Product-Level UI Design Specification

---

## 1. Overall Layout

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

### Area 1: Top Control Bar (Global Control)

**Functions:**
- Agent mode switch: Suggest / Edit / Auto
- Current task status (Idle / Thinking / Acting)
- One-click Run (Run Task / Stop)
- Scope control (Current File / Project / Multi-file)
- Git status

**Design points:**
- Always visible
- Clear status (color indicates whether AI is executing)

---

### Area 2: Left Side — File Area (Stable Zone)

**Contents:**
- File tree (Explorer)
- Search
- Git

**Design principle:**
- Fully maintain VS Code habits
- No AI elements introduced
- Purpose: reduce learning curve

---

### Area 3: Center — Code Editor (Main Stage)

**Core principle:** User always has control

**Feature layers:**

#### (A) Base Editing
- Multi-file tabs
- Split view
- Minimap

#### (B) AI Enhancement (Non-intrusive)

**Inline Suggestion:**
- Gray ghost text (Copilot-like)

**Diff Overlay (Key Feature):**
- AI modifications displayed as overlay
- Not directly written to file

Example:
```
- old code
+ new code (AI suggestion)
```

**Intent Layer (Innovation):**
- Inline AI intent hints

Example:
```
// Optimize this loop for better performance
```

**Design value:**
- AI "visible but not disruptive"

---

### Area 4: Right Side — Agent Panel (Intelligence Core)

**Three-layer structure:**

#### Chat Layer (Conversation)
- Input tasks
- Multi-turn dialogue
- Context binding (file / selection)

#### Task Layer (Execution Visualization)

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

#### Diff Layer (Trust Core)

Show:
- File changes
- Code diff

**Characteristic:**
- User confirms before applying

---

### Area 5: Bottom — Execution Panel

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

### 3.1 Selection as Context
- Select code -> Ask Agent
- Auto-attaches context

### 3.2 Drag-Driven AI
- Drag file to Agent panel
- Drag error log to Chat

### 3.3 Quick Action Layer (Important)

Floating on selection:
```
Explain | Fix | Refactor | Optimize
```

### 3.4 AI Control Level Toggle

- Suggest (suggestions only)
- Edit (can modify code)
- Auto (automatic execution)

### 3.5 Ghost Mode (Background AI)

**Behavior:**
- Pre-analyze project
- Generate potential optimizations
- Don't disturb user

---

## 4. State System

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

## 7. Figma Component Breakdown

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
- **Toggle / Segment** (Suggest | Edit | Auto)
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

## 8. Agent State Machine + Data Flow

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

## 9. Multi-Agent Collaboration UI

### 9.1 Core Concept

> Not just one Agent, but "role-based Agents"

- Architect
- Coder
- Tester
- Reviewer

### 9.2 Agent List (Right Panel Top)

```
[Architect] [Coder] [Tester] [Reviewer]
```

Status: Active / Idle / Busy

### 9.3 Collaboration View

**Task Pipeline:**
```
Design -> Implement -> Test -> Review -> Merge
```

Each stage handled by a different Agent.

### 9.4 Conflict Resolution UI

When multiple Agents modify the same file:

```
Agent A vs Agent B

[Accept A] [Accept B] [Merge]
```

### 9.5 Advanced Capabilities (Future)

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
