# AGENTS.md

Project memory for Agent IDE. The backend loads the workspace-root `AGENTS.md`
into every Agent run's context (`services/project_memory.rs`, bounded at 8 000
characters — keep this file well under that or the tail is silently truncated).

Agent IDE is a Tauri v2 desktop Agent IDE: Rust backend in `src-tauri/`, React 18
+ TypeScript + Zustand frontend in `src/`. Its reason to exist is that a user can
**see and undo** everything the Agent did. Treat that as the top-level constraint,
not a feature.

## Verify before claiming done

Four commands, all of which must pass. Run them; do not infer.

```
cd src-tauri && cargo clippy --all-targets -- -D warnings
cd src-tauri && cargo test --lib
npx tsc --noEmit
npm test
```

- `cargo test --lib` is the suite. `cargo test --bin agent_cli` runs **zero**
  tests — the CLI's tests live in the lib target.
- The shell is PowerShell. `cat <<'EOF'` heredocs do not work; chain with `;`.
  For a multi-line commit message, write a temp file and use `git commit -F`.
- `npm run tauri -- dev` is the only way to exercise the real backend. `npm run
  dev` is a browser preview with the Tauri bridge absent.

## Traps that have already cost time here

- **Never `as SomeUnion` a value from `localStorage` or a Tauri event.** That is
  not validation, it only silences the type checker; a stale value from an older
  build reaches the store and the UI renders an impossible state. Normalize
  instead — see `normalizeAgentMode` in `src/types/agent.ts`.
- **Money is integer micro-USD, and parsing is string-based.**
  `Number("0.29") * 1e6` is `289999.99999999994`. Use `usdToMicros` in
  `src/utils/money.ts`. Rounding is *up*, so a sub-cent cost is never free.
- **Vitest has no setup file.** The global environment is node; the four files
  that need a DOM opt in with a `// @vitest-environment jsdom` docblock. Because
  there is no setup file, React Testing Library's auto-cleanup is not wired — a
  file with more than one rendering test needs its own `afterEach(cleanup)`, as
  `src/components/shared/CommandPalette.test.tsx` does.
- **`test.include` in `vite.config.ts` is pinned on purpose.** `artifacts/e2e/`
  holds whole repo copies including `*.test.tsx`; the default `include` runs those
  frozen snapshots, so failures point at old code. Add new test directories to
  that list explicitly or they never execute.
- **`workspace::env_test_guard()` is only a mutex.** It does not set
  `AGENT_IDE_CONFIG_DIR`. A Rust test that needs config isolation sets it itself;
  `config_dir()` additionally refuses to fall back to the real home under
  `cfg!(test)` so a forgetful test cannot overwrite the developer's saved API key.
- **Windows path quirks are a security boundary, not cosmetics.** `.git./hooks`
  and `name::$DATA` resolve to the same files as `.git/hooks` and `name`. Deny-list
  comparison goes through `normalize_component_for_denial`.

## Architecture rules

- **`agent/orchestrator.rs` imports no Tauri types.** Emission goes through the
  `RunEvents` trait (`agent/events.rs`); `AppHandle` implements it and tests pass
  `RecordingEvents`. Reintroducing `AppHandle` there makes the pipeline
  untestable, which is how it used to be.
- **A pipeline run is driven by `drive_run` / `drive_pipeline` / `drive_repair`**,
  free functions taking `&Mutex<AgentOrchestrator>`. Every state mutation lives in a
  *synchronous* method (`begin_planning`, `record_plan`, `prepare_stage`,
  `record_stage_outcome`, `finish_pipeline`, `prepare_repair_iteration`,
  `record_repair_apply`, `record_repair_iteration`), each one critical section, each
  with a doc comment naming the invariant it lands. Two rules follow:
  - Do not hold the orchestrator lock across an `await`. **There are no remaining
    exceptions** — the last one, the repair loop, was converted for this reason.
    Holding it makes every other command queue behind a multi-minute model call,
    including `get_agent_state`, so the UI cannot even tell it is busy.
  - Do not split the orchestrator into per-field locks. Four invariants span
    fields; see Known Issues 16 in ROADMAP.md before proposing it again.
- **Cancelling must not need the lock held by the work being cancelled.** `Stop`
  pulls the current run's switch through `CancelRegistry` *before* it takes the
  orchestrator lock. A run's cancel flag and its exclusivity claim both come from
  `try_begin_run` as a `RunLease`, one fresh pair per run — a shared flag let a new
  prompt un-cancel a still-draining old run.
- **Every change the Agent lands must be visible and undoable.** Diffs go through
  the review area; direct tool writes are published back as `applied` diffs with
  their pre-write content plus an undo checkpoint (`record_tool_writes`), on every
  exit path including cancellation. A write that changes disk while the Diff view
  stays empty is the failure this product exists to prevent.
- **Agent writes resolve through `workspace::resolve_for_agent_write`.** No
  exceptions, on any path.
- **Permission gates are all `matches!(mode, AgentMode::Auto)`.** The mode has two
  values because there is one gate. Finer authority belongs in
  `WorkspaceToolPermissions`.

## Conventions

- **Never ship a control that does nothing.** Two were deleted in one day: an
  `Edit` mode position identical to `Suggest`, and two permission toggles with no
  backend reader. An inert toggle is worse than an absent one — it manufactures
  false confidence about what was authorized.
- **Comments are in Chinese and explain *why*.** Name the failure mode avoided or
  the invariant held. A comment that restates the code is noise; delete it.
  Markdown docs are in English.
- **Documentation claims must be verified against the code.** Repeated doc drift
  here has included a fabricated CLI help block, an inert flag described as
  working, and stale test counts. If you change behaviour, grep the docs for the
  old claim.
- **Assertions sit on the property that matters**, not on incidental shape. A
  command that cannot launch reports a missing executable differently on Windows
  than elsewhere; assert "counted as a failure, earlier results survive".
- Delete unused code rather than renaming it to `_unused` or leaving a
  `// removed` marker. No backwards-compatibility shims.

## Where decisions go

`ROADMAP.md` is the design log: Known Issues carries audited problems with the
reasoning, including options that were rejected and why. `SECURITY.md` documents
what the backend actually enforces. Record a decision there rather than in a new
file; both are read far more often than they are written.
