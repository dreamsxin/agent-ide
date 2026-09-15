# AGENTS.md

Project memory for Agent IDE. The backend injects this file into every Agent run
(`services/project_memory.rs`, bounded at 8 000 **bytes** — keep it under ~7 500 or
the tail is silently truncated; measure, do not guess).

Tauri v2 desktop Agent IDE: Rust in `src-tauri/`, React 18 + TS + Zustand in `src/`.
It exists so a user can **see and undo** everything the Agent did. That is the
top-level constraint, not a feature.

## Verify before claiming done

Five commands, all must pass. Run them; do not infer.

```
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy --all-targets -- -D warnings
cd src-tauri && cargo test --lib
npx tsc --noEmit
npm test
```

- `cargo fmt --check` is in CI but was missing here, which is how ~15 files drifted
  while every local check stayed green. It is first because it is the cheapest.
- `cargo test --lib` is the suite. `cargo test --bin agent_cli` runs **zero** tests.
- PowerShell: no `cat <<'EOF'` heredocs; chain with `;`. Multi-line commit message →
  temp file + `git commit -F`.
- `npm run tauri -- dev` is the only way to exercise the real backend; `npm run dev`
  has no Tauri bridge.

## Traps that have already cost time

- **Never `as SomeUnion` a value from `localStorage` or a Tauri event.** That silences
  the type checker instead of validating; a stale value from an older build reaches the
  store and the UI renders an impossible state. Normalize — see `normalizeAgentMode`.
- **Money is integer micro-USD, parsed as string.** `Number("0.29") * 1e6` is
  `289999.99999999994`. Use `usdToMicros`; it rounds **up**, so sub-cent is never free.
- **Vitest has no setup file.** Environment is node; DOM tests need a
  `// @vitest-environment jsdom` docblock, and with no setup file RTL auto-cleanup is
  absent — a file with two rendering tests needs its own `afterEach(cleanup)`.
- **`test.include` in `vite.config.ts` is pinned on purpose.** `artifacts/e2e/` holds
  frozen repo copies with `*.test.tsx`; the default include runs those. New test
  directories must be added explicitly or they never run.
- **`workspace::env_test_guard()` is only a mutex**, it does not set
  `AGENT_IDE_CONFIG_DIR`. A test needing config isolation sets it itself.
- **Windows path quirks are a security boundary.** `.git./hooks` and `name::$DATA`
  resolve like `.git/hooks` and `name`; deny-list comparison goes through
  `normalize_component_for_denial`.

## Architecture rules

- **`agent/orchestrator.rs` imports no Tauri types.** Emission goes through the
  `RunEvents` trait; `AppHandle` implements it, tests pass `RecordingEvents`. The rule
  covers anything that both decides and emits — `McpToolInvoker`, and the command
  layer's run-finish helpers (`finish_agent_run`, `publish_tool_writes`,
  `publish_external_actions`, both `emit_*_degradation_log`) take `&dyn RunEvents`.
  Every defect found in those for four cycles was a wording or counting defect, and a
  signature taking `AppHandle` cannot be tested for either.
- **A run is driven by `drive_run` / `drive_pipeline` / `drive_repair`**, free functions
  over `&Mutex<AgentOrchestrator>`. Every state mutation is a *synchronous* method with
  a doc comment naming its invariant. Two consequences:
  - Never hold the orchestrator lock across an `await`; there are no remaining
    exceptions. Holding it queues every other command behind a model call.
  - Do not split the orchestrator into per-field locks — four invariants span fields.
    See Known Issues 16 first.
- **The side-effect gate is one flag with one source.** The command layer mints an
  `Arc<AtomicBool>` per run and **moves** it into
  `WorkspaceToolPermissions::adopt_cancel`. Nothing else takes a flag: both
  `attach_mcp_tools` and `try_begin_run` take `&WorkspaceToolPermissions` and read
  `cancel_switch()` themselves, so no caller can put the lease, the built-in tools and
  the MCP tools on different flags. (`adopt_cancel` *after* a claim would still split
  them; adopt before claiming, as all four commands do.) After Stop, `invoke()` refuses
  calls that have not started and `run_project_command_cancellable` kills the process
  tree of one that has. One flag per run, never reset: a shared flag let a new prompt
  un-cancel a draining old run.
- **Stop must not need the lock held by the work being cancelled.** It pulls the
  switch from `CancelRegistry` *before* taking the orchestrator lock.
- **Every change the Agent lands must be visible and undoable.** Diffs go through the
  review area; direct tool writes are published back as `applied` diffs with their
  pre-write content plus an undo checkpoint (`record_tool_writes`) on **every** exit
  path including cancellation. Disk changing while the Diff view stays empty is the
  failure this product exists to prevent.
- **Agent writes resolve through `workspace::resolve_for_agent_write`.** No exceptions.
- **Permission gates are all `matches!(mode, AgentMode::Auto)`.** Finer authority
  belongs in `WorkspaceToolPermissions`.
- **Monaco's module-level registrations live in `components/editor/monacoGlobals.ts`.**
  `languages.register*` and `editor.registerCommand` belong to the monaco module, so
  they register once per module (`WeakSet` guard) and are never disposed. Only
  per-editor things (listeners, `addAction`) belong in `onMount` / `disposablesRef`.

## Conventions

- **Never ship a control that does nothing.** An inert toggle is worse than an absent
  one: it manufactures false confidence about what was authorized.
- **Comments are in Chinese and explain *why*** — the failure mode avoided or the
  invariant held. A comment restating the code is noise. Markdown docs are English.
- **Documentation claims must be verified against the code.** Drift here has included
  a fabricated CLI help block, an inert flag described as working, and stale test
  counts. When a write-up asserts a *consequence*, that consequence needs its own
  evidence: one fix was published with two symptoms the code could not produce.
- **Assertions sit on the property that matters**, not on incidental shape. Prefer
  "counted as a failure, earlier results survive" over an exact error string.
- Delete unused code rather than renaming it to `_unused` or leaving a `// removed`
  marker. No backwards-compatibility shims.

## Where decisions go

`ROADMAP.md` Known Issues is the design log: audited problems with the reasoning,
including rejected options and why. `SECURITY.md` documents what the backend actually
enforces. Record decisions there, not in new files.
