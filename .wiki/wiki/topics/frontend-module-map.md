---
title: "Code Map: React Frontend (src)"
category: topic
sources: [raw/repos/2026-09-29-agent-ide-source-scan.md]
created: 2026-09-29
updated: 2026-09-29
tags: [agent-ide, code-map, react, zustand, monaco, typescript]
aliases: [frontend map, src map, 前端代码地图]
confidence: high
volatility: warm
verified: 2026-09-29
summary: "Module-level map of src/: nine hand-persisted Zustand stores, the component tree from App down to each panel root, the normalizer layer that guards untrusted input, and the Monaco split between module-level and per-editor registration."
---

# Code Map: React Frontend (`src`)

> Entry `index.html:11` → `src/main.tsx:6` (React 18 `StrictMode`, single root,
> `styles/index.css`). Scripts at `package.json:8-11`: `dev`, `build` =
> `tsc && vite build`, `test` = `vitest run`.

## Directory responsibilities

- **`components/agent/`** — the Agent-facing UI: `ChatView.tsx`, `DiffView.tsx`,
  `SettingsPanel.tsx`, `ConfirmDialog.tsx`, `QuestionDialog.tsx`, sessions, MCP,
  plan/pipeline.
- **`components/editor/`** — Monaco hosting and satellites: `monacoGlobals.ts`
  (module-level registrations), `EditorContainer.tsx` (per-editor wiring),
  overlays `DiffOverlay` / `InlineSuggestion` / `IntentHint` / `QuickActions`,
  marker bridges.
- **`components/layout/`** — window chrome and shells: `TopBar`, `LeftPanel`,
  `AgentPanel`, `BottomPanel`, `StatusBar`, `ResizeHandle`, plus the pure helper
  `agentTabBadges.ts`.
- **`components/panels/`** — dockable panel contents: `Explorer`, `GitPanel`,
  `Terminal`, `TasksPanel`, `ProblemsPanel`, `LogView`, and pure helpers
  `explorerTree.ts`, `taskVerification.ts`.
- **`components/shared/`** — `CommandPalette`, `ShortcutsHelp`, `ModeSwitch`,
  `ErrorBoundary`, `PanelLoading`, `StatusDot`.
- **`hooks/`** — `useAgentBridge` (event → store), `useAppBootstrap`,
  `useTauriEvent`, `useShortcuts`, `useProjectTasks`, `useRunProjectTask`,
  `useLspDiagnostics`, `useFixWithAgent`, completion/rendering hooks.
- **`stores/`** — nine Zustand stores (the directory name is plural) plus the pure
  `llmConnection.ts` helper.
- **`types/`** — wire shapes *and their normalizers*: `agent.ts`, `browser.ts`,
  `editor.ts`, `project.ts`.
- **`utils/`** — pure helpers with explicit invariants: money, paths, token input,
  context budget, external actions, LSP client, completion.
- **`i18n/`** — `zh`/`en` message table and locale store (`index.ts`,
  `messages.ts`).
- **`styles/`** — single Tailwind entry `index.css`.

## Stores

Nine stores, all plain `create<T>()` — **no `persist` middleware**. Persistence is
hand-written per store, which is why the localStorage table below matters.

- `useAgentStore` — `src/stores/useAgentStore.ts:57` onwards, 2,141 lines. The big
  one: run state/mode/ideMode `:59-62`, `contextUsage`, `diffs` `:70-71`,
  external actions, SDD artifacts, steps, `pendingUndo` `:91`, `runUsage` `:93`,
  stream buffer `:94-95`, chat messages/turns `:100-109`, sessions `:116-122`,
  role/pipeline `:128-144`, LLM config + connection + profiles `:147-171`,
  permissions and `pendingConfirm`/`pendingQuestion` `:174-184`. Actions:
  `sendPrompt`, `stopAgent`, `applyAllDiffs`/`applyDiff`/`applyDiffHunk`,
  `rejectDiff*`, `undoLastApply` `:1329`, `refreshPendingUndo` `:1355`,
  `restoreDiffs`, `forgetSettledDiffs`, session CRUD, `fetchLlmConfig`.
- `useLayoutStore` — `useLayoutStore.ts:5-49`. Panel sizes/visibility,
  `focusMode`, tab selection, `performanceOverlay`, `workspacePath`; clamped
  setters `:223-225`, `toggleFocusMode` `:238`, `flushLayoutSave` `:265`.
- `useEditorStore` — open tabs, `fileContents`, active file, selection/cursor,
  save errors, intent hints, `pendingRevealLocation` `:451-456`;
  `startWatching`/`stopWatching` `:357-373`, `setWorkspacePath` `:377`,
  `restoreEditorSession` `:387`.
- `useLogStore` — `useLogStore.ts:10-38`, ring buffer of ≤500 entries (`:8`);
  `addLog` also mirrors to disk.
- `useProblemStore` — `useProblemStore.ts:16-54`, problems keyed by `source`.
- `useGitStore` — status/diff/loading/error, all IPC `:52-232`.
- `useTaskStore` — `useTaskStore.ts:33-50`, task definitions, queued terminal
  commands/sessions, run state and history.
- `useLspStore` — `useLspStore.ts:12-37`; `message: null` means "no extra
  sentence" — wording is the view's job (`:14-21`).
- `useThemeStore` — `useThemeStore.ts:31-45`, applies `data-theme` on `<html>`
  (`:25`).
- `useLocaleStore` — `src/i18n/index.ts:69-85`.

### localStorage keys

- `agent-ide-layout` (`useLayoutStore.ts:51`) — written in `writeNow` `:194`,
  debounced 250 ms `:210-215`, subscribed once `:254`, force-flushed on
  `pagehide`/`visibilitychange` `:257-262`; read and field-by-field validated at
  `:146-159`. Deliberately excludes `performanceOverlay` and `workspacePath`
  (`:140-142`).
- `agent-ide-agent-diffs` (`useAgentStore.ts:1917`) — `persistDiffs` `:1953` keeps
  the last 200; `loadDiffs` `:1991` discards everything if `workspacePath`
  mismatches `:1995`.
- `agent-ide-agent-session` (`:1918`) — `persistAgentSession` `:2028`, cleared
  `:2080`, loaded `:2034`.
- `agent-ide-editor-session` (`useEditorStore.ts:465`) — written `:477` stripping
  `isDirty`/`loadError` `:472-474`, read `:483`.
- `agent-ide-logs` (`useLogStore.ts:7`) — written `:80`, read workspace-scoped
  `:86-90`.
- `agent-ide-workspace-path` — sole writer `useEditorStore.ts:379`; read by
  `useAgentStore.ts:2136`, `useLogStore.ts:106`, `chatContextOptions.ts:42,65`.
- `agent-ide-theme` (`useThemeStore.ts:23`/`:14`), `agent-ide-locale`
  (`i18n/index.ts:38`/`:25`), `agent-ide-chat-context-options`
  (`chatContextOptions.ts:62`/`:43`).

## The normalizer layer

Two input sources are untrusted: Tauri event payloads, and localStorage written by
an older build. Casting either with `as SomeUnion` silences the type checker
instead of validating, and a stale value then renders an impossible UI state.

- `normalizeAgentMode` — `src/types/agent.ts:29-31`, rationale at `:21-28` (a hard
  cast once let a bogus mode render a segmented control with nothing selected).
  Applied at `useAgentBridge.ts:57` and `useAgentStore.ts:2053`.
- `normalizeRunUsage` `types/agent.ts:78`, `normalizeContextUsage` `:69` — the
  latter returns `null` on total 0, because 0 means "never reported", not "empty
  context"; both applied at `useAgentBridge.ts:68,113`.
- `normalizeApprovalRequest` / `normalizeAgentQuestion` — an unreadable payload is
  **refused immediately** rather than dropped (`useAgentBridge.ts:157-189`);
  dropping it would leave the backend to wait out its two-minute timeout while the
  UI just looks stuck.
- `normalizeExternalActions` — `src/utils/externalActions.ts:38`; entries survive
  with empty strings, only a record missing both `kind` and `target` is dropped
  (`:32-37`).
- `normalizeLocale` — `src/i18n/index.ts:15-20`, same reasoning, cited explicitly.
- Restore-time: `normalizeRestoredAgentState` (in-flight → `waiting_user`,
  `useAgentStore.ts:2083`), `normalizeRestoredTask` `:2101`,
  `normalizeRestoredStep` (`doing` → `error` + "Interrupted by reload") `:2110`,
  `normalizeRestoredPipelineStage` `:2121`, and `markOrphanedDiffsStale` `:1978`
  which demotes diffs the backend no longer knows so no Apply button is inert.

That last one is the convention in `AGENTS.md` made mechanical: never ship a
control that does nothing.

## Component tree

`App` — `src/App.tsx:60`. Mounts `useAgentBridge()` `:88`, `useAppBootstrap()`
`:120`, `useShortcuts` `:90`, then the chrome:

```
App (App.tsx:170, data-testid="app-root")
├── ShortcutsHelp / CommandPalette            :171, :176
├── ConfirmDialog   (approval)                :186 → components/agent/ConfirmDialog.tsx
├── QuestionDialog  (model asks user)         :191 → components/agent/QuestionDialog.tsx
├── TopBar                                    :194 → layout/TopBar.tsx:35
├── row: LeftPanel | EditorContainer | AgentPanel
│   ├── AnimatedPanel(leftVisible)            :197-204 → layout/LeftPanel.tsx:15
│   ├── ErrorBoundary + Suspense              :207-211 → editor/EditorContainer.tsx:79 (lazy, :21)
│   └── AnimatedPanel(rightVisible)           :214-221 → layout/AgentPanel.tsx:52
├── AnimatedPanel(bottomVisible, keepMounted) :224-231 → layout/BottomPanel.tsx:21
└── StatusBar (outside the panels on purpose) :234 → layout/StatusBar.tsx:45
```

- **Agent panel** `layout/AgentPanel.tsx:52` — three primary tabs Chat/Plan/Changes
  (`:39-49`) inside `PrimaryView` (`AgentRunSummary` + body, `:227-242`), plus four
  utility buttons: new task, history, pipeline, settings (`:134-164`). Chat is
  eager (`:13`); `DiffView`, `SessionHistory`, `SettingsPanel`, `TaskPipeline`,
  `TaskView`, `AgentSelector` are lazy (`:23-28`). The Changes badge counts only
  non-restored external actions (`:61-67`).
- **Chat** `agent/ChatView.tsx:258` — root `data-testid="agent-chat"` `:505`,
  `PendingChangesCard` inline `:520`, lazy `MarkdownMessage` `:24,86`, input and
  send `:907-972`, error banner `:883`, calls `sendPrompt` `:465`.
- **Diff / review area** `agent/DiffView.tsx:242` — root
  `data-testid="diff-view"` `:327`, external-action list `:349`, per-diff cards
  `:497` with hunk-level apply/reject `:60,67`. This is the surface the whole
  product is built to keep honest.

## Monaco

Module-level registrations live in `components/editor/monacoGlobals.ts`:
`languages.register*` and `editor.registerCommand` belong to the monaco module, so
they register once behind a `WeakSet` guard and are never disposed. Only
per-editor things — listeners, `addAction` — belong in `onMount` /
`disposablesRef` in `EditorContainer.tsx:79`.

## Money

Integer micro-USD, parsed from a string. `Number("0.29") * 1e6` is
`289999.99999999994`, so `usdToMicros` is mandatory; it rounds **up**, which means
a sub-cent charge is never free.

## Tests

- `npm test` = `vitest run` (`package.json:8-11`). Environment is node, and there
  is **no setup file**: a DOM test needs a `// @vitest-environment jsdom`
  docblock, and with no setup file RTL auto-cleanup is absent — a file with two
  rendering tests needs its own `afterEach(cleanup)`.
- `test.include` in `vite.config.ts` is pinned deliberately. `artifacts/e2e/`
  holds frozen repo copies containing `*.test.tsx`, which the default include
  would run. A new test directory must be added explicitly or it never runs.

## Dead code found during the scan

`src/hooks/useTauriEvent.ts` exports `useTauriEvent` `:9` and `useTauriEvents`
`:43`, but a repo-wide search finds no call site — only the definition file and
its own `console.warn` strings (`:26`, `:61`). Every real listener uses
`@tauri-apps/api` `listen` directly. `AGENTS.md` says to delete unused code rather
than keep it, so this is a live finding, not a note.

## See Also

- [[code-map-overview|Repository Overview]] ([Repository Overview](code-map-overview.md)) — where this half sits
- [[ipc-contract|IPC Contract]] ([IPC Contract](../references/ipc-contract.md)) — the events these stores consume
- [[backend-module-map|Backend Module Map]] ([Backend Module Map](backend-module-map.md)) — the producer side

## Sources

- [agent-ide source scan @2b35623](../../raw/repos/2026-09-29-agent-ide-source-scan.md) — commit, method, and gaps
