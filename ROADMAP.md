# Agent IDE - Implementation Roadmap

> This file is the canonical source of truth for project state.
> If you resume work after an interruption, start here.

---

## Quick Recovery

After any interruption, restore context in this order:

1. Read this file to understand current state and next work.
2. Read `README.md` for setup, runtime modes, and workflow overview.
3. Read `docs/agent_ide_design.md` for detailed current design.
4. Read `docs/agent_ide_ui_design.md` for UI design intent.
5. Read `docs/smoke_test.md` before changing LSP, Problems, Terminal, Git, or Agent diff application.
6. Check `git status --short` before editing. There may be user changes.
7. Run verification:

```powershell
npm run build
cd src-tauri
cargo fmt --check
cargo clippy --no-default-features --all-targets -- -D warnings
cargo test --no-default-features
```

These are the same commands CI runs (`.github/workflows/ci.yml`). If any of them fails locally, CI will fail too.

---

## Project Identity

| Field | Value |
|-------|-------|
| Project | Agent IDE |
| Description | Code-centric controllable AI Agent IDE |
| Stack | Tauri v2 + Rust backend + React 18 + TypeScript + Tailwind CSS |
| Editor | Monaco Editor |
| Terminal | xterm.js + Tauri PTY (`portable-pty`) |
| File Tree | react-arborist + Tauri commands |
| State | Zustand |
| Build | Vite |
| Root | `d:\work\agent-ide` |

---

## Current State

Status as of 2026-09-05: **Phase 8 daily IDE replacement hardening in progress; Phase 9.0 market-parity foundation mostly closed (native tool calling, AGENTS.md memory, and MCP client landed; only the permission model V2 remains)**.

Updated strategic direction (2026-09-03):

- Based on analysis of Zed Editor, Comate, open-source code models (StarCoder, CodeLlama, DeepSeek Coder, CodeGemma), and DeepSeek Harness architecture.
- Introducing Phase 9: Performance Optimization & Open-Source Model Integration.
- New focus on incremental rendering, intelligent code completion, local model support, and plugin-based extensibility inspired by DeepSeek Harness.
- Maintaining Phase 8 hardening work while preparing for Phase 9 architectural enhancements.

Updated strategic direction (2026-09-04, competitive review):

- Reviewed the 2026-09 market surface: Codex (subagents, MCP client/server, hooks, AGENTS.md, skills, agent dashboard, worktree isolation, cloud tasks), Claude Code (subagent tool scoping, hooks, SKILL.md open standard, budget caps, sandboxing), Cursor (parallel agents, deep codebase indexing, fast proprietary completion models), Windsurf/Devin Desktop (agent fleet, open Agent Client Protocol), and GitHub Copilot (cloud coding agent, agentic code review loop, enterprise governance).
- Three standards are now must-have parity features: provider-native tool calling, AGENTS.md project memory, and MCP connectivity. Agent IDE's differentiator remains the finest-grained controllable/auditable agent surface: per-hunk review, provenance, context budget engineering, and headless run artifacts.
- Phase 9 is restructured: new Phase 9.0 (market parity foundation) and Phase 9.5 (agent differentiation depth) bracket the existing Phase 9 work. Incremental rendering (9.1) is deferred: Monaco already virtualizes rendering, so the real performance levers are code splitting (Phase 10) and memory optimization (9.7).
- Local open-source model integration (9.3) is re-scoped to OpenAI-compatible local runtimes first (Ollama, LM Studio, vLLM) since the existing `llm_client.rs` already speaks that protocol; native inference-engine integration moves behind that.
- First Phase 9.0 item landed: workspace-root `AGENTS.md` project memory is loaded into `AgentContext` as a bounded section, included by default, and participates in every compression mode and budget packing.

- CLI work is now intentionally scoped and first-pass complete as a headless automation runner. Do not keep expanding it into a second interactive IDE unless that becomes an explicit product goal.
- The highest-value next work is back in the desktop IDE path: real Tauri runtime validation, Agent workflow interaction polish, LSP/workspace indexing validation, richer merge/diff review, and frontend/Tauri smoke coverage.
- Further CLI work should be limited to maintenance, smoke coverage, and policy hardening unless external toolchain requirements appear.

The app is no longer just a static UI prototype. It has a working Tauri/Rust backend, file commands, Git commands, LLM streaming, Agent planning/execution scaffolding, diff review UI, and settings for model configuration. Recent work focused on correcting safety and runtime assumptions:

- Added workspace path resolution and path-bound file operations.
- Added Agent context compression modes: `focused`, `compact`, `full`, and `budgeted`.
- Replaced unsafe Agent Markdown HTML injection with `ReactMarkdown skipHtml`.
- Restored a Tauri CSP instead of `csp: null`.
- Added browser/Tauri runtime guards so `npm run dev` can preview UI without crashing.
- Fixed Git untracked status classification.
- Fixed terminal kill path to signal the reader loop.
- Added tests for context compression behavior.
- Added a shared diff apply module with conflict detection for missing/ambiguous hunks, new-file overwrite protection, partial-apply reporting, and CLI/orchestrator reuse.
- Wired Agent cancellation checks through the LLM request and streaming read path.
- Scoped Git commands and terminal cwd to the saved workspace boundary.
- Wired the frontend terminal panel to Tauri PTY spawn/write/resize/output events.
- Added focused tests for workspace traversal, diff apply failures, auto-apply partial failure, and Git status/workspace boundaries.
- Surfaced structured diff apply failures inline on the affected diff cards in addition to the summary banner.
- Wired the configured Agent pipeline into backend execution as role-aware stages: planner -> architect -> coder -> tester -> reviewer.
- Added structured Agent action log events for prompt, planner, stage start/completion/error, diff readiness, and auto-apply.
- Surfaced Agent action logs in the Logs panel with expandable details, context summaries, diff summaries, stage, role, and phase.
- Fed actual pending diff summaries into the Reviewer stage so review is based on proposed file/hunk changes, not only prior text output.
- Added `docs/agent_ide_design.md` as the detailed design document for workflows, context handling, Agent orchestration, and technical boundaries.
- Added backend Agent context enrichment with bounded project tree summaries and Git working-tree diff excerpts.
- Added a compatible structured `agent-changes` JSON output protocol for model file changes while preserving legacy diff/new-file block parsing.
- Added a formal `agent-changes` version 1 schema document, validation diagnostics in action logs, optional reviewer findings, and hunk-level provenance.
- Fixed terminal PTY input handling by keeping a persistent writer per terminal instance instead of taking a new writer for each keystroke.
- Improved terminal startup feedback and guarded resize fitting when the panel has no measurable size.
- Added `README.md` with setup, runtime modes, verification, Agent workflow, protocol, and project status.
- Added optional `baseHash` metadata to structured Agent diffs and reject stale edit diffs when the file content hash no longer matches.
- Added `README.zh-CN.md` as the Chinese project README and linked it from the English README.
- Surfaced diff `baseHash` metadata in the Diff view and added stale-diff guidance when hash validation fails.
- Added Git file management commands and UI actions for stage, unstage, and discard through a file context menu.
- Added per-file diff apply/reject commands and Diff view controls so reviewers can accept or reject individual pending files instead of only applying or rejecting the full batch.
- Added Monaco local code completion provider for stable low-latency suggestions from language keywords, current-file symbols, snippets, and open file paths.
- Wired editor QuickActions to real Agent prompts with active file content, selection, and selected line range.
- Replaced static bottom-panel test/action samples with a Problems panel and store for diagnostics, Agent findings, and future test failures.
- Synced Monaco model markers into the Problems panel and made problem rows jump to the affected file location.
- Parsed terminal TypeScript/lint/test-style file-position errors into the Problems panel for click-through navigation.
- Added a Project Tasks panel for build, test, lint, run, and debug commands that queue into the integrated terminal.
- Added workspace task discovery from `package.json` scripts and Cargo manifests, with fallback default tasks when no project tasks are found.
- Moved common project Run/Debug/Build/Test commands into the TopBar and renamed the bottom task list to Commands to keep Agent Tasks distinct.
- Expanded terminal test-output parsing for Vitest/Jest-style failures, stack traces, and `FAIL` file summaries so `npm run test` failures can surface in Problems.
- Added non-interactive project task runner for build/test/lint/check commands with exit code, duration, Logs integration, and Problems parsing.
- Unified TopBar and Commands panel project command execution through a shared runner/terminal routing hook, so build/test/lint/check feed Logs and Problems consistently while run/debug stay interactive in Terminal.
- Fixed workspace switching for Commands and Terminal by passing the active frontend workspace path into task discovery, task execution, and terminal spawn instead of relying only on previously persisted backend workspace state.
- Normalized Windows verbatim workspace paths before spawning Terminal or project task shells so `cmd.exe` starts in `D:\...` paths instead of rejecting `\\?\D:\...` as a UNC path.
- Kept the integrated Terminal mounted across bottom-tab switches and bottom-panel hide/show so switching to Commands, Problems, or Logs does not kill and recreate the PTY session.
- Added IDE runtime failure context injection for Agent prompts, including the latest failed project command, parsed Problems, recent Terminal output, and recent warning/error Logs.
- Added one-click `Fix with Agent` actions in Problems and failed Commands, reusing the same IDE runtime failure context for structured repair prompts.
- Hardened Explorer context menu behavior so it closes on outside pointer interactions, Escape, scroll, and blur, and clamps menu placement inside the viewport.
- Replaced Explorer browser prompt flows for new file/folder/rename with an in-app dialog and changed Copy File to VS Code-style copy/paste with automatic `Copy`, `Copy 2`, etc. names.
- Added non-interactive project command run history with per-run status, exit code, duration, output details, rerun, clear history, and failed-run `Fix with Agent` actions.
- Added Terminal multi-session UI with session tabs, new session, close, restart, and active cwd/profile display while keeping inactive PTY views mounted.
- Routed Run/Test/Debug-style project commands into dedicated Terminal sessions so long-running or interactive commands do not overwrite the main shell.
- Added tracked Terminal task completion using per-session exit markers, so Test/Run-style commands opened in Terminal still update command status, run history, Problems, Logs, and Agent failure context.
- Added TypeScript/JavaScript semantic editor defaults through the Monaco TS worker, including worker-backed diagnostics, hover/completion behavior, F12 definition action, and stable file-backed Monaco models for open files.
- Added per-hunk diff review controls with backend `apply_diff_hunk` and `reject_diff_hunk` commands, hunk status tracking, and Diff view Apply/Reject hunk actions.
- Added Git staged/worktree/all diff modes plus Source Control multi-select batch Stage, Unstage, Discard, and workspace-path-aware status loading.
- Normalized Windows verbatim `\\?\D:\...` workspace paths across workspace resolution and Git repo path handling so Git status/diff no longer misreports active workspaces as outside the workspace.
- Added Git branch checkout/create, Fetch/Pull/Push actions, upstream/ahead/behind display, and conflict file detection in Source Control.
- Added one-shot Git credential inputs for remote actions, remote branch checkout/tracking, and conflict resolution controls for accept current, accept incoming, accept both, and conflict diff navigation.
- Added optional OS credential storage for HTTPS Git remote username/token inputs used by fetch, pull, and push.
- Added TypeScript LSP status details in the TopBar, including server path/source, workspace, install command, indexing mode, detected config files, opened document count, change count, diagnostics count, last error, and recent per-file diagnostics summaries.
- Added Go LSP first pass using `gopls`, with active Go-file startup, install guidance, go.mod/go.work indexing detection, and shared LSP operations.
- Routed Monaco Quick Fix/code actions through an explicit apply command that logs success/failure, syncs editor store state, and triggers LSP `didChange` so Problems and markers refresh after fixes.
- Enabled JavaScript semantic diagnostics in Monaco TS worker defaults instead of syntax-only JavaScript validation.
- Added frontend Vitest coverage for Windows/file-URI path normalization and terminal output problem parsing.
- Routed build/test/lint/check-style project commands through the non-interactive command runner so Test also records exit code, duration, output, Problems, Logs, and failed-run Agent repair context.
- Added `docs/smoke_test.md` as the real Tauri runtime regression checklist for LSP, Problems, Quick Fix, Commands/Run History, Terminal, Git, and Agent repair loops.
- Added LLM provider profiles with backward-compatible legacy config migration, Settings profile management, Chat-level profile selection, and per-run context compression mode selection.
- Added per-profile model budget metadata for max context tokens, reserved output tokens, and max output tokens, with Chat showing an estimated effective input budget.
- Wired max context and reserved output metadata into Agent context building as estimated token-to-character budget trimming, with action logs recording raw/final context character counts.
- Mapped per-profile max output tokens into OpenAI-compatible chat request bodies and added provider presets for default context/output budgets.
- Moved LLM API key persistence out of plain JSON config and into the OS credential store; profile JSON now stores credential references only.
- Added stricter structured Agent change protocol validation and first-class diff provenance metadata, including protocol, operation, schema version, change index, and rationale.
- Wired Agent pipeline role/stage metadata into generated diff provenance so proposed changes can be traced back to the stage that produced them.
- Added a first-pass Chat context preview with per-run source toggles for active file, selection, open files, Problems, failed runs, terminal output, and warning/error logs.
- Extended Chat context controls to include git diff and project tree toggles, passed those source choices through IPC, and made backend workspace enrichment respect them.
- Persisted frontend Logs/action-log entries per workspace in local storage and restore them when the workspace is reopened.
- Persisted Chat context source flags per workspace, included workspace context source flags in prompt action-log entries, and emit action-log events for apply/reject diff decisions.
- Persisted Agent pending diff/review metadata per workspace so the Diff tab can recover proposed changes and hunk statuses after reload.
- Added backend Agent context section estimates and replaced Chat's rough source estimate with real backend section sizes, budget trimming, and included/excluded source reasons.
- Added interactive Agent plan controls for editing step title/scope/mode, skipping steps, running one step only, and regenerating a step with broader context.
- Added regenerate-against-current-file actions for failed/stale Agent diffs and hunks, preserving the original failed diff while adding provenance on regenerated changes.
- Added a first-pass Command Palette with searchable workspace, panel, Agent mode, project command, theme, and focus-mode commands, reachable from `Ctrl+Shift+P` and the TopBar.
- Persisted Agent task session state per workspace, including current task, editable plan steps, pipeline stage state, mode, and interrupted state recovery after reload.
- Added frontend Agent run ids and restored-session metadata so recovered plans show whether they were interrupted and which run they came from.
- Added backend Agent run-id tracking and frontend reconciliation so restored sessions can show whether they match the backend or are frontend-recovered only.
- Added Agent plan step reorder controls and backend step-order persistence.
- Added pipeline `pauseBefore` controls so users can configure stage approval points before the Agent continues.
- Added paused pipeline snapshots and a Continue action so users can approve a paused stage and continue the backend pipeline from that stage.
- Added `docs/agent_cli_manual.md` and clarified that CLI mode is currently a headless preview/apply Agent runner, not a complete command-line IDE replacement.
- Added `docs/agent_cli_design.md` to plan CLI mode as a stable automation surface for CI, scripts, external IDE tasks, and bounded fully autonomous repair loops.
- Started CLI Phase 1 with `clap` parsing, `doctor/context estimate/plan/run` command shape, text/json/ndjson output modes, run ids, artifact directories, stable exit codes, prompt-file/stdin support, and parser-focused tests.
- Extracted LLM provider profile and credential handling into `services/llm_profiles.rs` so IDE commands and CLI can share one backend implementation; CLI now supports `--profile`.
- Extracted project task discovery and non-interactive command execution into `services/project_tasks.rs`; CLI can run shared check commands with `--run-command` and records results in artifacts.
- Added shared backend command-output problem parsing in `services/problem_parser.rs`; project command results and CLI artifacts now include parsed Problems.
- Extracted shared Agent single-step runtime helpers into `services/agent_runtime.rs`; CLI step execution and IDE single-step/regenerate flows now share step context enrichment and diff provenance attachment.
- Added first-pass CLI bounded repair iterations with `--max-iterations`, feeding failed `--run-command` output and parsed Problems back into an Agent repair prompt after applied changes.
- Added first-pass CLI repair-loop command authorization with `--allow-run`, including exact, prefix wildcard, and trusted all-command patterns.
- Added `repair-chain.json` artifacts for CLI repair loops, linking failed commands, parsed Problems, generated repair diffs, apply results, and rerun results.
- Added `agent_cli smoke ide-backend` to validate workspace resolution, package script discovery, project command execution, terminal-like Problems parsing, repair prompt construction, diff parsing, apply, rerun, and repair-chain artifacts without launching the desktop UI.
- CLI `problems.json` now preserves observed pre-repair Problems even when the final rerun passes, keeping failure -> repair -> rerun traceability intact.
- Added Pipeline stage source/output visualization from Agent action logs and clearer Diff hunk review/regeneration status in the desktop UI.
- Added a reusable Phase 8 real-runtime smoke run template and baseline notes to `docs/smoke_test.md`.
- Added Phase 9.0 AGENTS.md project memory: workspace-root `AGENTS.md` is loaded into `AgentContext` as a bounded section, included by default in every IDE and CLI run, respects the per-run `includeProjectMemory` source toggle, and is exposed to CLI automation via `--include project-memory`.
- Added Phase 9.0.3 MCP client: `services/mcp.rs` speaks MCP stdio (line-delimited JSON-RPC 2.0) with `initialize`, `tools/list`, and `tools/call`; servers are configured in `<config_dir>/mcp.json`; discovered tools are injected into the provider-native tool list as `mcp__{server}__{tool}`.
- Added a bounded tool-execution loop in `agent/executor.rs`: MCP tool calls are executed through a `ToolInvoker`, results are replayed as `role: "tool"` messages, and the loop stops after `MAX_TOOL_ITERATIONS` rounds. Built-in `emit_agent_changes` / `emit_sdd_draft` stay on the output-protocol path.
- Extended `ChatMessage` with optional `tool_calls` / `tool_call_id` so assistant tool-call turns and tool results can be replayed to OpenAI-compatible providers; requests without tool rounds serialize exactly as before.
- Added MCP Tauri commands (`get_mcp_config`, `save_mcp_config`, `discover_mcp_tools`, `get_mcp_tools`, `call_mcp_tool`, `disconnect_mcp_servers`) and a Settings-panel MCP section for server management, discovery status, and the exposed tool list.
- MCP tool calls and discovery emit `agent-action-log` entries (`mcp_tool_call`, `mcp_discovery`) so external tool use is auditable in the Logs panel alongside diffs and stages.
- Surfaced the AGENTS.md project-memory toggle in the ChatView context-source chips, wired to the backend `project_memory` estimate section.
- Fixed pre-existing `npm run build` type errors in `src/hooks/useIntelligentCompletion.ts` and `src/components/editor/EditorContainer.tsx` that were blocking frontend verification.
- Added `.github/workflows/ci.yml` (Phase 10.1): every push to `main` and every PR runs frontend typecheck/build/vitest plus Rust `cargo fmt --check`, `cargo clippy --no-default-features --all-targets -- -D warnings`, and `cargo test --no-default-features`. This is the gate that was missing when the frontend build silently broke on `main`.
- Made the clippy gate real: reformatted with `rustfmt` and fixed 35 lint findings (unused imports/variables, unreachable feature-gated code, redundant casts and clones, derivable `Default` impls, `sort_by` → `sort_by_key`, `ModelType::to_string` → `Display`). Two crate-level allows remain with written justification in `src-tauri/src/lib.rs`; `clippy::await_holding_lock` is allowed only on the two test modules that serialize env-var mutation.
- Added `.gitattributes` pinning `src-tauri/gen/schemas/*.json` to LF so a Windows build stops producing CRLF-only diffs on every `cargo check`.
- Fixed the ROADMAP phase numbering: Phase 10/11/12 task ids were off by one (Phase 10 items were numbered `9.x`, colliding with real Phase 9 items).
- Added MCP tool approval enforcement (first slice of Phase 9.0.4): `McpToolPolicy` (`deny` / `auto_approved_only` / `allow_all`) gates both which tools are injected into the model's tool list and every `tools/call` invocation. Per-server `autoApprove` lists live in `mcp.json` and are editable per tool in the Settings MCP panel. Unknown or missing policy values fall back to `auto_approved_only`, so a typo cannot open everything up.
- Wired the policy to the existing Agent permission preset: `allowCommandRun` maps to `allow_all`, otherwise only auto-approved tools are exposed. MCP tools are external process execution, so they follow the command-execution permission rather than getting their own implicit grant.
- Made diff staleness detection actually work (Phase 10.7 first slice). `baseHash` was previously supplied only by the model, which cannot know Agent IDE's hash function — so the check was effectively dead: either absent (no protection) or wrong (always rejected). The backend now stamps `baseHash` from the real file content at diff-generation time and discards the model-supplied value.
- Replaced `DefaultHasher` with a hand-written FNV-1a 64 for `content_hash`, pinned by standard test vectors. The standard library explicitly does not guarantee `DefaultHasher` stability across Rust releases, and these hashes are persisted per workspace and re-validated after restart — an unstable hash would have silently marked every saved diff as stale after a toolchain upgrade.
- Kept sequential review working alongside mandatory stamping: `apply_pending_diffs` tracks the files it wrote within one batch and skips the base-hash check for them (the change came from us, not from outside), and `restamp_applied_files` refreshes the stamp on still-pending/partial diffs for files that were just written. External edits between generation and apply are still rejected.
- Enforced `allowFileCreate` in the backend for Auto-mode auto-apply: new-file diffs are held as `pending` for human review instead of being written, the run ends in `waiting_user` rather than `done`, and the action log records which files were held. Editing existing files is unaffected. The permission arrives per run on `SendPromptRequest` and defaults to false when absent.
- File-create classification uses hunk shape (all hunks have empty `original`), matching what the apply path actually does, rather than `provenance.operation` — that field is `"unknown"` for every backend-generated diff, so keying a permission check on it would leave a bypass.
- Scoped the remaining permission toggles honestly after auditing every backend write path: `allowFileDelete`, `allowCommandRun`, and `allowGitActions` have no Agent-reachable path in the desktop app today. There is no delete code in the diff apply path at all (`executor::normalized_operation` maps `"delete"` to `"unknown"`), Agent runs never call `run_project_command`, and every mutating Git command is user-initiated UI. The Agent's only execution surface is MCP tools, already gated by `McpToolPolicy`. Adding backend checks for those three would be theater; they stay frontend UX hints until an Agent-reachable path exists.
- Ran the first real headless end-to-end verification (not just unit tests) with `agent_cli` against a throwaway git workspace and the `mock://` provider: `doctor` reports its preconditions correctly (missing LLM env, then missing Git repo, then OK), `run --apply` planned, generated a diff, and wrote the file, `changes.json` carried a backend-stamped `baseHash` where it used to be `null`, and `--deny-path` blocked the apply and left the file untouched.
- Fixed CLI policy classification: `validate_cli_policy` keyed create-vs-edit on `provenance.operation`, which is `"unknown"` for every backend-generated diff and falls back to `"edit"`, so a diff that actually creates a file was recorded as an edit in `policy.json`. It now uses `diff_apply::is_new_file_diff`, the same hunk-shape rule the apply path branches on. This is an auditability fix, not a privilege escalation fix — `effective_policy` already treats `--apply` as implying both create and edit.
- Added `.agent-ide/` to `.gitignore`. `agent_cli` writes run artifacts to `<workspace>/.agent-ide/runs/<run-id>/`, so any CLI run inside this repo left untracked directories behind.
- **Fixed silent credential loss (`keyring` had no platform backend).** `Cargo.toml` declared `keyring = "3"` with no features. keyring v3 requires an explicit store feature; without one it compiles the mock store, which returns `Ok` from writes and `NoEntry` from reads. Every API key on every platform was discarded, `Failed to store credential` could never fire, and the failure surfaced as `Credential not found or inaccessible` — blaming the OS. Now declares `apple-native` / `windows-native` / `sync-secret-service`. Verified on Windows: a real `LegacyGeneric:target=llm-profile:default.agent-ide` entry appears, survives an app restart, and the Agent reads it back.
- Added a credential round-trip test (`services::credentials`) covering store → read → overwrite → delete. This is the Phase 10.5 secret-storage validation and it fails loudly on a mock backend, so the regression cannot return silently.
- Stopped the UI from claiming a key is stored when it is not: `masked_api_key()` now probes the credential store instead of assuming `credentialRef.is_some()` means "saved", `save_profile` requires a key when no readable one exists (new `has_readable_api_key()`), and the frontend `(saved)` badge no longer treats the literal string `"not configured"` as truthy. Before this, the panel showed "stored in OS credential store", the user left the field blank, the save succeeded, and every run failed — with no way out through the UI.
- Added a masked "Stored:" readout with an eye toggle in Settings, backed by a dedicated `reveal_llm_api_key` command. Plaintext is never included in the routine profile-list response; it is fetched only on an explicit click.
- **Rendered the Agent error message.** `useAgentStore.error` was set on failure, persisted, and read by nothing — a failed run showed only a `Retry` button with no explanation. ChatView now shows the message above the input. This is what turned an opaque failure into the keyring root cause during the smoke run.
- Started separating IPC adapters from business logic so verification stops depending on the window. `apply_diff_hunk` is the first: the state work moved to `AgentOrchestrator::apply_diff_hunk`, `status_from_hunks` and the review-state rollup moved to `orchestrator.rs`, and the Tauri command is now only lock + emit + action log. Two Rust tests now cover what previously required clicking a hunk in the UI.
- **The very first extraction found a silent-failure bug**: `apply_diff_hunk` built its synthetic single-hunk diff with `..diff.clone()`, inheriting `status`. After the first hunk landed the diff became `partial`, and `apply_pending_diffs` skips anything that is not `pending` — so applying a second hunk did nothing, returned `Ok` with both `applied` and `failed` empty, and the UI showed neither a change nor an error. The synthetic diff is now forced to `pending`. Manual testing could not have caught this: the bundled mock provider only ever emits a single-hunk diff.
- Extracted `apply_diff` the same way, which fixed a second review-flow gap: it only accepted `pending`, so once any hunk had been reviewed individually the whole-file Apply button failed with "not pending" and the user had to click every remaining hunk one at a time. It now accepts `partial` / `failed`, applies only the undecided hunks, and leaves already-rejected hunks untouched instead of writing them back.
- Extracted the reject path (`reject_diff` / `reject_diff_hunk`) into the orchestrator, and it had the predicted symmetric bugs. `reject_diff` also required `pending`, so whole-file Reject failed after any per-hunk review — the exact mirror of the Apply gap. Worse, it hard-set `diff.status = "rejected"` without touching per-hunk statuses, so a file with one applied hunk could report `rejected` while that hunk still read `applied`; anything downstream that trusts either field saw a contradiction. Both now accept `pending` / `partial` / `failed`, skip already-decided hunks, and derive the file status from `status_from_hunks` instead of writing it directly. Three tests cover rejecting the remainder of a partially applied file, per-hunk status convergence to `Done`, and refusing to reject an applied hunk.
- Extracted the batch commands (`apply_diffs` / `reject_diffs`) last, and the same bug class was present one level up. Apply-all handed `self.diffs` straight to `apply_pending_diffs`, which skips anything that is not `pending`, so a file with even one individually reviewed hunk was dropped from the batch entirely — no write, no error, and the "N applied, M failed" summary did not account for it. It also only ever set the file-level status, leaving per-hunk statuses empty so the UI's hunk badges went blank after Apply all. Reject-all had the `pending`-only restriction plus a reporting bug: it returned every diff whose status was `rejected`, including ones rejected in earlier rounds, so the action log over-counted. Both now iterate the reviewable diffs and reuse the per-file `apply_diff` / `reject_diff`, which gets the undecided-hunk filtering, `restamp_applied_files`, and status derivation for free. `commands/agent.rs` no longer imports `apply_pending_diffs` at all — the IPC layer has no direct path into the diff engine.
- Added store-level frontend tests against a mocked `invoke` (no new dependencies; `@testing-library/react` and jsdom are not installed). They lock the error-capture path, the `toolApproval` / `allowFileCreate` derivation from the permission preset, and `api_key_masked` passthrough including the literal `"not configured"`.

Important distinction:

- `npm run dev`: Vite web preview only. Tauri IPC, filesystem, terminal, Git, and Agent backend are disabled or stubbed.
- `npm run tauri -- dev`: real IDE runtime with Rust backend and Tauri APIs.

---

## Current Verification

Last verified locally:

```powershell
npm run build     # passes; Vite still warns about a large Monaco/Markdown chunk
npm test          # passes; covers path normalization and terminal problem parsing
cargo check       # passes
cargo test        # passes; includes context, workspace, diff apply, orchestrator, pipeline, action-log support, and Git tests
```

2026-09-06 verification note: `cargo fmt --check`, `cargo clippy --no-default-features --all-targets -- -D warnings`, and `cargo test --no-default-features` all pass on Windows (164 tests, 0 failed, 1 ignored), including the AGENTS.md project-memory tests, the tool-call accumulator / synthesis tests, the MCP config/qualified-name/content-flattening, tool-policy and tool-loop selection tests, the base-hash stamping / batch-write / restamp tests with pinned FNV-1a vectors, the auto-apply file-creation gating tests, the per-hunk apply/reject review-flow tests extracted out of the IPC layer, and the CLI create-classification regression test. `npm ci`, `npm run build`, and `npm test` (24 tests) also pass. All five commands now run in CI on every push and PR. The default `llama-cpp` feature additionally requires LLVM/libclang plus a full llama.cpp native build, which is pending the re-scoped Phase 9.3 (OpenAI-compatible local runtimes first). The default `llama-cpp` feature additionally requires LLVM/libclang plus a full llama.cpp native build, which is pending the re-scoped Phase 9.3 (OpenAI-compatible local runtimes first).

2026-09-05 headless end-to-end run (real binary, not unit tests): built `agent_cli --no-default-features`, then ran it against a throwaway git workspace with `LLM_ENDPOINT=mock://smoke`:

- `doctor` reported each precondition in turn — missing LLM env (exit 7), then "Git repository was not found", then `Ok` (exit 0) once the workspace was a repo.
- `run --apply "Update smoke.txt"` planned one step, produced a `legacy-diff-block` diff, and rewrote `smoke.txt` from `initial` to `changed`.
- `changes.json` carried `"baseHash": "7f5f002d3284b6c3"`. This field was `null` on every CLI-generated diff before the backend stamping change, so the fix is confirmed on the real path and not only in tests.
- `run --apply --deny-path smoke.txt` failed with `Policy denied generated change for smoke.txt by --deny-path smoke.txt` and left the file at `initial`.

Still unverified: everything that needs the desktop UI — per-hunk review, the Auto-mode file-creation hold, MCP discovery against a real server, and the Terminal/LSP/Git panels. Those need `npm run tauri -- dev` and a human at the window.

2026-09-05 desktop smoke run (real app window, `npm run tauri -- dev -- --no-default-features`, `mock://` provider, throwaway workspace `D:\work\agent-ide-smoke` with a hand-written zero-dependency MCP stdio server):

- **MCP discovery**: `Discover Tools` listed 2 tools with only the `autoApprove`-listed one checked. Verified from the backend that the `node` child process was spawned and held open, so the hand-rolled `initialize` / `tools/list` round-trip works against a real server.
- **MCP auto-approve toggle**: ticking a tool wrote `autoApprove: ["echo","danger_wipe"]` to `mcp.json` at 01:46:40.045 and respawned the server 20ms later, confirming the save → re-discover path.
- **`shutdown_all` reclaims children**: a second `Discover Tools` replaced PID 15068 with 36368 and left exactly one process, so connections are not leaked.
- **AGENTS.md context chip**: showed 63 tokens, which matches exactly — the section wrapper is 37 chars, the file is 214 chars, and `estimate_tokens(251) = (251+3)/4 = 63`.
- **Agent run + per-hunk apply**: after the credential fix, a prompt produced a diff and clicking a single hunk's apply rewrote `smoke.txt` from `initial` to `changed` with no errors. This is the first end-to-end verification of the desktop diff-apply path, including the backend-stamped `baseHash` and post-apply restamping.

Still unverified after this run: Auto-mode file-creation hold (the bundled mock only emits an edit diff, never a create), Terminal/LSP/Git panels, and the Commands → Problems parsing path.

MCP runtime note: the stdio transport, discovery, and tool loop are unit-covered and type-checked, but a live round-trip against a real MCP server (e.g. `npx -y @modelcontextprotocol/server-filesystem .`) has not been run yet. That belongs in the Tauri smoke loop.

Known local worktree note:

- `demo/hello.js` may contain unrelated user/demo changes. Do not revert it unless explicitly requested.

---

## Implemented Architecture

### Frontend

- `src/App.tsx`: layout, panel visibility, shortcut help, workspace restore.
- `src/components/layout/`: titlebar, left/right/bottom panels, resize handles.
- `src/components/editor/`: Monaco editor, tabs, inline suggestions, diff overlays, quick actions.
- `src/components/editor/DiagnosticsBridge.tsx`: syncs Monaco diagnostics into Problems.
- `src/components/editor/ProblemsMarkerBridge.tsx`: mirrors runtime/test/Agent/system Problems into Monaco markers and active-line decorations.
- `src/utils/codeCompletion.ts`: local completion candidate extraction for Monaco suggestions.
- `src/utils/lspClient.ts`: frontend bridge for TypeScript/JavaScript and Go LSP hover, completion, definition, symbols, rename, code actions, diagnostics, and status snapshots.
- `src/components/panels/`: Explorer, Git panel, terminal, logs.
- `src/components/panels/ProblemsPanel.tsx`: unified Problems view for diagnostics, test failures, and Agent findings.
- `src/components/panels/TasksPanel.tsx`: project command list for discovered build/test/lint/run/debug commands.
- `src/hooks/useProjectTasks.ts`: shared frontend task discovery hook for TopBar and Commands.
- `src/hooks/useRunProjectTask.ts`: shared project command executor that routes build/lint/check/typecheck through the non-interactive runner and run/test/debug-style commands through dedicated Terminal sessions.
- `src/stores/useTaskStore.ts`: queued terminal command/session state, latest task state, run history, and recent terminal output for project tasks.
- `src/utils/terminalProblemParser.ts`: parses terminal output into Problems entries for common file:line:column formats.
- `src/components/agent/`: chat, tasks, diff review, role selector, pipeline, settings.
- `src/stores/`: Zustand state for layout, editor, Agent, Git, logs, theme.
- `src/hooks/`: Tauri event bridge, shortcuts, event helpers.
- `src/utils/tauri.ts`: runtime detection for Tauri-only APIs.

### Backend

- `src-tauri/src/lib.rs`: Tauri plugin setup and command registration.
- `src-tauri/src/commands/fs.rs`: workspace-scoped file operations and watcher.
- `src-tauri/src/commands/git.rs`: Git status, diff, commit.
- `src-tauri/src/commands/tasks.rs`: project task discovery from workspace configuration.
- `src-tauri/src/commands/terminal.rs`: PTY lifecycle.
- `src-tauri/src/commands/agent.rs`: Agent commands, LLM config, context compression config.
- `src-tauri/src/agent/`: state machine, planner, executor, orchestrator, diff helpers, roles/pipeline models.
- `src-tauri/src/services/llm_client.rs`: OpenAI-compatible streaming client.
- `src-tauri/src/services/context.rs`: AgentContext and context compression.
- `src-tauri/src/services/workspace.rs`: config dir, workspace persistence, path resolution and workspace boundary checks.
- `src-tauri/src/services/mcp.rs`: MCP stdio JSON-RPC client, server config persistence, and the discovered-tool registry.
- `src-tauri/src/commands/mcp.rs`: MCP config/discovery/invocation commands and the `ToolInvoker` that runs MCP tools during an Agent run.
- `README.md`: setup, runtime modes, verification, Agent workflow, and current limitations.
- `README.zh-CN.md`: Chinese setup, workflow, protocol, and project status overview.
- `docs/smoke_test.md`: manual and automated smoke checklist for daily-IDE replacement workflows.

---

## Key Data Flows

### Open File

```text
Explorer click
  -> useEditorStore.openFile()
    -> invoke("read_file_content")
      -> Rust fs command resolves path inside workspace
        -> editor store caches content
          -> Monaco renders active file
```

### Code Completion

```text
Monaco completion trigger
  -> EditorContainer registered completion provider
    -> buildLocalCompletionCandidates()
    -> Monaco TypeScript worker handles TypeScript/JavaScript semantic suggestions
    -> local provider handles non-TS languages with language keywords + snippets
      -> current model identifiers
      -> open file paths for path-like prefixes
    -> Monaco suggestions list
```

### Quick Actions

```text
Editor selection
  -> QuickActions Explain/Fix/Refactor/Optimize
    -> build Agent prompt with active file, selection, and line range
    -> open Agent panel
    -> send_agent_prompt()
      -> normal Agent planning/review/diff flow
```

### Problems

```text
Monaco markers or Agent error/diff failure events
  -> DiagnosticsBridge or useAgentBridge
    -> useProblemStore
      -> ProblemsPanel
        -> file click opens and reveals the affected location

Terminal command output
  -> terminalProblemParser
    -> useProblemStore.replaceProblems("test")
      -> ProblemsPanel
        -> Fix with Agent sends a focused repair prompt with current runtime context

Project Tasks
  -> discover_project_tasks(active workspace path)
    -> package.json scripts + Cargo manifests
    -> fallback defaults when no tasks are discovered
  -> TopBar common Run/Debug/Build/Test buttons or Commands panel
    -> shared useRunProjectTask routing
    -> non-interactive runner for build/test/lint/check/typecheck
      -> Logs + Problems + task status
      -> failed command card exposes Fix with Agent
      -> run history stores exit code, duration, and output details
    -> dedicated Terminal sessions for run/test/debug-style tasks
      -> Terminal output is parsed into Problems and retained for Agent runtime context
      -> task exit marker records exit code, duration, output, history, and failed-run Fix with Agent context
```

### Agent Prompt

```text
ChatView.handleSend()
  -> useAgentStore.sendPrompt()
    -> append IDE runtime context from failed tasks, Problems, Terminal output, and Logs
    -> invoke("send_agent_prompt")
      -> AgentContext built from active file, selection, open files
      -> context enriched with project tree and Git working-tree diff when available
      -> context compressed by selected mode
      -> AgentOrchestrator.run()
        -> planner LLM streaming
        -> role-aware pipeline execution: architect -> coder -> tester -> reviewer
        -> emit structured action-log events
        -> reviewer receives pending diff summary
        -> parse model diff blocks
        -> emit plan, step, token, diff, pipeline, state events
```

### Apply Diff

```text
DiffView.applyAllDiffs() or per-file Apply
  -> invoke("apply_diffs") or invoke("apply_diff", diffId)
    -> resolve target diff files inside workspace
    -> validate optional baseHash for stale edits
    -> string-match original hunks
    -> write updated content
    -> mark applied/failed and emit state
```

Current limitation: diff application still uses textual `find` replacement. It needs stronger conflict recovery and clearer mixed hunk status semantics.

---

## Known Issues

### High Priority

1. **Diff application still lacks version-aware hunks**
   - Current behavior still depends on exact or trimmed textual hunk content; no line-offset tolerance yet.
   - Now rejects ambiguous matches and refuses to overwrite existing files for new-file hunks.
   - `baseHash` is now stamped by the backend at generation time from real file content (stable FNV-1a), so stale detection actually works. It used to be model-supplied and therefore either absent or wrong.
   - Sequential review still works: writes within one apply batch skip their own base-hash check, and still-pending diffs on a just-written file are restamped.
   - Remaining gap: an edit diff that carries no stamp at all (e.g. restored from a pre-2026-09-05 session) is still applied without a staleness check. Making that a hard rejection needs the real Tauri smoke loop first.
   - Partial apply errors are returned structurally and shown inline on failed diff cards.
   - Per-file and per-hunk apply/reject are now wired in the backend and Diff view.
   - Mixed applied/rejected hunk state currently closes the file diff; add clearer partial status next.

2. **Agent protocol still needs stronger schema and persistence**
   - Pipeline stages now drive backend execution.
   - Reviewer receives pending diff summaries.
   - Model outputs can now use structured `agent-changes` JSON blocks.
   - Legacy free-form markdown diff blocks are still supported.
   - Optional `baseHash` validation now rejects stale edit diffs.
   - Structured changes now reject unsafe paths, mixed create/edit payloads, empty/no-op hunks, and NUL-containing hunks.
   - Diff provenance now records protocol, operation, schema version, change index, rationale, source role, and source stage.
   - Still needs persisted action logs and a formal versioned schema document.

3. **Secret storage is weak**
   - LLM API keys are stored through the OS credential store.
   - Config JSON stores profile metadata and credential references only.
   - Needs real-runtime validation across supported OS credential backends.

4. **Cancellation is cooperative, not transport-abort based**
   - `stop_agent` now reaches the LLM request/stream loop quickly through a shared flag.
   - The underlying HTTP request is dropped by `tokio::select!`, but there is no explicit provider-side cancellation API.

### Medium Priority

5. **Terminal PTY integration needs runtime polish**
   - Frontend now spawns, writes, resizes, and listens for PTY output through Tauri.
   - Persistent PTY writer is now used for terminal input.
   - Project tasks can open run/test/debug-style commands in dedicated terminal sessions.
   - Task terminal sessions now record exit markers into Commands history with status, duration, exit code, output, Problems parsing, Logs, and Agent failure context.
   - Build/lint/check/typecheck tasks can run through a non-interactive command runner with exit code and duration.
   - TopBar and Commands panel now use the same project command execution path.
   - Terminal spawn now receives the active frontend workspace path, so newly opened terminals start in the currently opened workspace.
   - Windows shell startup strips `\\?\` verbatim prefixes before passing cwd to `cmd.exe`.
   - Bottom tab switches no longer unmount Terminal or reset the PTY session.
   - Multi-session UI, restart, close, and cwd/profile display are now present.
   - Needs interactive runtime testing in `npm run tauri -- dev` across shell startup, panel hide/show, workspace switching, and long-running commands.

6. **Git workflow needs continued polish**
   - File-level stage, unstage, discard, diff, and commit are wired.
   - Staged-vs-worktree/all diff views and multi-select batch actions are wired.
   - Branch checkout/create, fetch, fast-forward-only pull, push, and conflict file detection are wired.
   - Optional OS-stored HTTPS credentials, remote branch checkout/tracking, and basic conflict resolution controls are wired.
   - Needs better SSH/passphrase UX, richer merge editor UI, and safer destructive-action UX.

7. **Workspace boundary coverage needs continued review**
   - FS, Agent diff paths, Git entry points, terminal cwd, task cwd, and Agent CLI are now guarded or aligned.
   - Windows verbatim path prefixes are normalized centrally in the workspace service and Git repository relative-path helpers.
   - Continue reviewing any new backend command surfaces as they are added.

8. **Problems panel is only partially populated**
   - The UI/store foundation is now present.
   - Monaco diagnostics, Agent errors, and failed diff findings can be surfaced.
   - Terminal TypeScript/lint/test-style file-position errors, Vitest/Jest-style stack traces, and failed test file summaries are parsed into Problems.
   - Frontend smoke tests now cover file URI parsing and terminal problem extraction.
   - Rich test-runner protocol integration is still pending.
   - TypeScript/JavaScript open-file diagnostics now come from Monaco TS worker markers.
   - TypeScript LSP diagnostics are surfaced through Problems, Monaco markers, and TopBar diagnostics summaries.
   - Whole-workspace diagnostics still need indexing/runtime validation beyond opened files.

9. **Runtime modes need clearer UI messaging**
   - Browser preview now avoids crashes.
   - Some panels still need explicit disabled states for web preview mode.

10. **Encoding cleanup is incomplete**
   - Many files had historical mojibake comments/text.
   - User-visible text should be cleaned progressively.

### Lower Priority

11. **Code completion is partially semantic**
   - Monaco now has stable local keyword/symbol/snippet/path suggestions for non-TS languages.
   - TypeScript/JavaScript now use Monaco TS worker semantic completion/hover/diagnostics for open models.
   - TypeScript LSP-backed hover, completion, definition, document symbols, rename, code actions, diagnostics, and status snapshots are wired.
   - Go LSP first pass is wired through `gopls` detection/startup, shared operations, and Go module/workspace indexing status.
   - Code actions now log apply success/failure and trigger LSP diagnostics refresh after edits.
   - Workspace indexing/install UX is wired through TopBar probe details; large-workspace runtime validation still remains.
   - No LLM inline completion request path yet.

12. **Large frontend bundle**
   - Monaco, Markdown, xterm and syntax tooling create a large chunk.
   - Add dynamic imports/manual chunks later.

13. **Test coverage is thin**
   - Rust context compression has tests.
   - Rust diff apply, workspace boundaries, pipeline helpers, and pending diff summaries have tests.
   - Need more tests for Agent state transitions and frontend store behavior.

14. **CLI mode is first-pass complete for headless automation**
   - `agent_cli` is scoped as a headless automation runner, not a full terminal IDE replacement.
   - It supports `doctor`, `context estimate`, `plan`, and `run`, plus text/JSON/NDJSON output, artifacts, run ids, prompt-file/stdin input, stable exit codes, profile lookup, and shared workspace-boundary checks.
   - It uses the shared project command runner for `--run-command`, records parsed backend Problems, can feed failed command output into bounded repair iterations with `--max-iterations` guarded by `--allow-run`, and writes `repair-chain.json` for iteration traceability.
   - `doctor --output json` exposes a machine-readable capability contract for external tools.
   - CLI automation runs now support `--timeout-seconds`, `--max-output-bytes`, `--max-diff-files`, compact text summaries, and `repair-summary.json` with command/problem/repair counts.
   - CLI smoke tests cover `doctor --output json`, preview artifacts, apply artifacts, `repair-chain.json`, and `smoke ide-backend` using a mock provider.
   - Interactive plan controls, Problems/Terminal/Git/LSP integration, run history, per-hunk review, context preview/source toggles, action-log view, and task recovery remain desktop IDE workflows unless a separate terminal UI is intentionally planned.
   - CLI hardening is now mostly closed; only broaden permissions if the CLI scope is intentionally widened.

---

## Roadmap

### Phase 8.5 - Agent-First Interaction

Goal: make the IDE feel like a controllable Agent workspace, not a chat box bolted onto an editor.

Planned interaction TODO:

Current priority after reassessment:

1. Runtime validation in `npm run tauri -- dev` for Terminal sessions, Commands/Run History, Problems markers, LSP diagnostics, Git remote/conflict flows, and Agent repair loops.
2. Desktop Agent interaction polish: reviewer/Problem findings now bind into matching diff hunks, partial hunk review state is explicit, and workflow input/output provenance still needs richer stage-level UI.
3. IDE semantic reliability: large TypeScript/Go workspace indexing validation and next language adapters only after runtime behavior is stable.
4. Frontend/Tauri smoke tests that exercise daily IDE workflows end to end.

1. Agent workflow visualization
   - Show current pipeline phase, input sources, output state, retry/skip controls, and stage-level provenance.
   - Bind Reviewer findings to specific files/hunks where possible. First pass: same-file Problems/Agent findings are displayed on matching diff hunks.
2. Interactive Agent plan
   - Let users edit, reorder, skip, and run individual plan steps.
   - Add step scope controls: selection, active file, selected files, workspace.
3. Stronger diff review
   - Show prompt/stage/context provenance per hunk.
   - Add inline edit-before-apply and regenerate-against-current-file controls.
4. Problems / Terminal / Agent repair loop
   - Add Explain/Fix/Ignore/Open related tests actions on Problems.
   - Preserve failed run -> Agent fix -> diff -> rerun trace.
5. Context selection UX
   - Show context preview near Chat.
   - Let users include/exclude active file, selection, open files, Problems, terminal output, failed runs, logs, git diff, and project tree per run.
   - Show estimated token impact and excluded-over-budget sources.
6. Agent permission UX
   - Add Ask/Suggest/Auto permission presets with granular toggles for file creation, file deletion, command execution, and git actions.
   - Confirm destructive or broad operations.
7. Unified command entry
   - Keep TopBar for high-frequency Run/Test/Build/Debug.
   - Add command palette for all IDE commands.
   - Keep Agent Tasks separate from project commands.
8. Editor-native Agent actions
   - Add right-click and lightbulb Agent actions for Explain/Fix/Refactor/Generate Tests.
   - Add inline Agent suggestion acceptance.
9. State restoration
   - Persist Agent task state, action logs, pending diffs, run history, Problems, open files, active tab, cursor, and workspace-specific settings.

Current first implementation:

- Chat now exposes a compact context preview and lets users include/exclude active file, selection, open files, Problems, failed run, terminal output, and warning/error logs per Agent run.
- Git diff and project tree are also per-run context toggles, with backend section estimates, token budget visibility, and trimmed/excluded source reasons.
- Agent Tasks can now edit step metadata, skip a step, run only one step, or regenerate a step with broader workspace context.
- Failed/stale Agent diffs and hunks can be regenerated against the current file while preserving the original failed review item.
- Command Palette now provides a unified searchable entry for workspace open, panel navigation, Agent mode changes, project commands, focus mode, and theme toggling.
- Agent task state now restores after reload for current task, steps, pipeline, and waiting-review state. In-flight stages are downgraded to a recoverable waiting state so users can review diffs or rerun a step.
- Restored Agent tasks now show run id, restore time, and interrupted-vs-restored status in the Tasks tab.
- On workspace restore, the frontend checks backend `currentRunId`/`lastRunId`; unmatched restored sessions are explicitly marked `Frontend recovered only`.
- Agent Tasks can reorder plan steps with Up/Down controls, and Pipeline Editor can mark stages to pause before execution.
- Paused pipelines now retain prompt/context/stage-output snapshots in the backend, and the Pipeline view exposes a Continue button for paused stages.

---

### Phase 6 - Stabilization and Safety

Goal: make the IDE safe enough for regular local development.

Deliverables:

- Workspace boundary applied consistently across FS, Agent, Git, terminal cwd, and CLI.
- LLM key storage moved out of plain JSON or protected with strict permissions as an interim step. **Current:** LLM keys use the OS credential store; cross-OS runtime validation remains.
- Diff application returns structured errors to UI.
- Agent cancellation token wired through orchestrator and LLM client.
- Browser preview mode has clear disabled states.
- Roadmap and docs reflect actual project state.

Acceptance checks:

```powershell
npm run build
cd src-tauri
cargo check
cargo test
```

Add focused tests:

- `workspace::resolve_existing` rejects outside paths.
- `workspace::resolve_for_write` rejects outside parents.
- `apply_diffs` reports unmatched hunks.
- `git_status` distinguishes added vs untracked.

### Phase 7 - Agent Execution Quality

Goal: turn Agent scaffolding into a reliable controllable coding loop.

Status: **feature-complete as of 2026-05-16**. Remaining validation is covered by the Phase 8 real-runtime smoke loop.

Deliverables:

- Role-aware orchestration: architect -> coder -> tester -> reviewer.
- Pipeline stages influence prompts and state transitions.
- Agent action log with prompt/context/diff provenance.
- Reviewer uses actual pending diff summaries for structured review.
- Context sources: selected files, open files, git diff, project tree summary, terminal/log excerpts.
- Context compression strategy interface:
  - `full`: complete active context.
  - `focused`: selected code and active-file excerpt.
  - `compact`: outline and metadata.
  - `budgeted`: token-budget-aware file packing.
- Structured model protocol instead of free-form markdown-only diff parsing.
- Formal `agent-changes` version 1 schema and validation diagnostics in Agent action logs.
- Hunk-level provenance for structured changes and reviewer findings.

Acceptance checks:

- A prompt produces visible plan stages.
- Each stage emits state and logs.
- Diff suggestions include source context metadata.
- Stop cancels active LLM streaming.

Implementation status:

- Plan stages, role-aware execution, and action logs are wired.
- Diff suggestions include file and hunk provenance.
- `budgeted` context mode is available in UI, CLI, backend parsing, and context estimation.
- Invalid structured model output emits `agent_changes_validation` warnings.
- Stop/cancel is wired in backend; final provider/runtime confirmation remains in Phase 8 smoke notes.

### Phase 8 - IDE Workflow Completion

Goal: make core IDE workflows practical.

Deliverables:

- Terminal fully wired to backend PTY:
  - spawn terminal
  - write input
  - resize
  - receive `terminal-output`
  - kill terminal
- TopBar exposes common Run/Debug/Build/Test commands, while the bottom Commands panel lists all discovered workspace commands and task status.
- QuickActions sends real Agent prompts.
- DiffView supports per-file and per-hunk apply/reject.
- Git panel supports stage, unstage, discard with confirmation.
- Editor has local code completion for common languages and current-file symbols.
- Problems panel replaces static test/action samples and accepts Monaco diagnostics, Agent findings, and parsed terminal test/lint failures.
- Logs panel consumes backend and Agent event streams.

Acceptance checks:

- Open workspace, edit file, save, view Git diff.
- Ask Agent for a small change, review diff, apply one hunk.
- Run terminal command and see output.

### Phase 9.0 - Market Parity Foundation (Target: 4-6 weeks, ahead of Phase 9)

Goal: close the gap to the 2026 agentic-coding standard feature surface (Codex / Claude Code / Cursor parity) without changing the product direction of a controllable, auditable agent IDE.

| Task | Deliverable | Priority | Status |
|------|-------------|----------|--------|
| 9.0.1 Provider-Native Tool Calling | Native function-call request and streaming tool-call parsing in `services/llm_client.rs` + `agent/executor.rs`; `agent-changes` text protocol remains the fallback for local/small models | Critical | **Done (2026-09-05)** |
| 9.0.2 AGENTS.md Project Memory | Load workspace-root `AGENTS.md` into `AgentContext`; bounded size; participates in all compression modes and budget packing; CLI `--include project-memory` | Critical | **Done (2026-09-04)** |
| 9.0.3 MCP Client | MCP stdio (line-delimited JSON-RPC 2.0) transport in `services/mcp.rs` with tool discovery/invocation, `mcp__{server}__{tool}` injection into the native tool list, a bounded tool-execution loop in `agent/executor.rs`, and `agent-action-log` entries | High | **Done (2026-09-05)** |
| 9.0.4 Desktop Permission Model V2 | Ask/Suggest/Auto presets plus granular file/command/git toggles, path deny rules, and per-run cost caps, all enforced in the backend | High | **In progress**: MCP tool approval (`McpToolPolicy` + per-server `autoApprove`) and `allowFileCreate` for Auto-mode apply are enforced backend-side. `allowFileDelete` / `allowCommandRun` / `allowGitActions` have no Agent-reachable backend path today (see Current State). Path deny rules and per-run cost caps are still missing |

Exit criteria: an Agent run uses native tool calls with at least one OpenAI-compatible provider; AGENTS.md content is visible in context estimate sections; MCP tools appear in the agent tool surface; desktop runs enforce permission presets.

---

### Phase 9: Performance Optimization & Open-Source Model Integration (Target: 4 weeks after Phase 8)

**Goal:** Enhance editor performance, intelligent code completion, and integrate open-source code models (StarCoder, CodeLlama, DeepSeek Coder, CodeGemma) while learning from Zed, Comate, and DeepSeek Harness architectures.

| Task | Deliverable | Priority |
|------|-------------|----------|
| 9.1 Incremental Rendering Engine | Inspired by Zed: viewport-based dirty line rendering, frame-budget rendering, multi-threaded editor operations | **Deferred (2026-09-04)**: Monaco already virtualizes rendering; performance leverage is code splitting (Phase 10) and 9.7 memory optimization |
| 9.2 Intelligent Code Completion System | Simplified scope (2026-09-04): inline LLM completion channel over the existing provider client plus per-profile language presets; no standalone completion framework | High |
| 9.3 Open-Source Model Integration | Re-scoped (2026-09-04): first integrate OpenAI-compatible local runtimes (Ollama, LM Studio, vLLM) through `llm_client.rs` model discovery; native engines (llama-cpp-rs, candle-core) come later | Critical |
| 9.4 Hybrid Model Strategy | Smart model selection: simple tasks → local models, complex tasks → cloud models | High |
| 9.5 Plugin Architecture Foundation | DeepSeek Harness-inspired: model adapters, tool registry, session logs, and agent loops as replaceable plugins | Medium |
| 9.6 Performance Profiling Tools | Flamegraph integration, real-time frame time monitoring, memory usage tracking | Medium |
| 9.7 Editor Memory Optimization | Lazy file loading, efficient tab management, optimized Monaco model caching | High |
| 9.8 Chinese-Optimized Prompts | Merged into 9.2 simplified scope: per-profile language presets for Chinese naming/comment conventions instead of a separate prompt-engineering layer | Medium |

**Technical Implementation:**

**9.1 Incremental Rendering (Zed-inspired):**
```rust
pub struct IncrementalRenderer {
    viewport: Viewport,
    dirty_lines: HashSet<LineNumber>,
    frame_budget: Duration,
}

impl IncrementalRenderer {
    pub fn render_with_budget(&mut self, changes: Vec<TextChange>) -> RenderResult {
        let start = Instant::now();
        for change in changes {
            self.dirty_lines.extend(change.affected_lines());
        }
        // Render only dirty regions within frame budget
        self.render_dirty_regions(start.elapsed() < self.frame_budget)
    }
}
```

**9.2 Intelligent Code Completion (Comate-inspired):**
```typescript
export class IntelligentCodeCompletion {
    private contextAnalyzer: ContextAnalyzer;
    private suggestionCache: Map<string, Suggestion[]>;

    async getSuggestions(file: string, position: Position, surroundingCode: string): Promise<Suggestion[]> {
        const context = await this.contextAnalyzer.analyze({
            file, position, surroundingCode,
            projectStructure: await this.getProjectContext(),
            recentEdits: this.getRecentEdits()
        });
        const prompt = this.buildChineseOptimizedPrompt(context);
        return this.generateSuggestions(prompt);
    }
}
```

**9.3 Open-Source Model Integration:**
```rust
pub struct EnhancedLlmClient {
    openai_client: Option<LlmClient>,
    local_models: Vec<LocalModel>,
}

pub struct LocalModel {
    name: String,
    model_type: ModelType, // StarCoder, CodeLlama, DeepSeekCoder, CodeGemma
    engine: Box<dyn ModelEngine>,
}

pub enum ModelType {
    StarCoder,      // Hugging Face open-source
    CodeLlama,      // Meta open-source
    DeepSeekCoder,  // DeepSeek open-source
    CodeGemma,      // Google open-source
}
```

**9.5 Plugin Architecture (DeepSeek Harness-inspired):**
```typescript
export interface Plugin {
    name: string;
    version: string;
    onActivate?(context: PluginContext): void;
    onDeactivate?(): void;
    registerCommands?(registry: CommandRegistry): void;
    registerLanguageSupport?(provider: LanguageProvider): void;
    enhanceAgentPipeline?(pipeline: Pipeline): void;
}

export class PluginManager {
    private plugins: Map<string, Plugin> = new Map();
    async loadPlugin(pluginPath: string) {
        const plugin = await import(pluginPath);
        this.plugins.set(plugin.name, plugin);
        plugin.onActivate?.(this.createContext());
    }
}
```

**Exit Criteria:** Editor renders >60fps with 1000+ line files; intelligent completion works for TypeScript/JavaScript/Go/Python; at least 2 open-source models integrated; plugin foundation tested.

---

### Phase 9.5 - Agent Differentiation Depth (Target: 3-4 weeks after Phase 9.0)

Goal: deepen the controllable/auditable agent moat and adopt the multi-agent and hooks direction the market has standardized on.

| Task | Deliverable | Priority | Status |
|------|-------------|----------|--------|
| 9.5.1 Parallel Subagents + Worktree Isolation | Upgrade the serial role pipeline to a DAG: coder steps can fan out in parallel, each in an isolated `git worktree`, with reviewer-stage merge and summary | Critical | Planned |
| 9.5.2 Hooks Engine | User-configurable shell/MCP hooks at stage start/complete, pre-apply, and on-failure points, reusing CLI command authorization patterns | High | Planned |
| 9.5.3 SKILL.md Skills | Lazy-loaded, bounded project skill packages compatible with the SKILL.md open standard; treat skills as code: path and command constraints apply | High | Planned |
| 9.5.4 Semantic Index / Retrieval | tree-sitter symbol index plus local embedding retrieval feeding `budgeted` context packing | High | Planned |
| 9.5.5 Run Artifacts Unification | IDE agent runs persist the same artifact model as `agent_cli` (run log, diffs, repair chain); replay/compare in the UI | Medium | Planned |

Exit criteria: one coder fan-out runs two steps in isolated worktrees with merged review; a hook can veto a diff apply; a skill loads on demand without exceeding the context budget.

---

### Phase 10: Release Readiness (Target: 3 weeks after Phase 9)

**Goal:** Production-quality packaging, security hardening, and CI coverage for public beta.

| Task | Deliverable | Priority | Status |
|------|-------------|----------|--------|
| 10.1 CI Pipeline | GitHub Actions `ci.yml`: frontend typecheck + build + vitest, and Rust `cargo fmt --check` / `cargo clippy -D warnings` / `cargo test` on every push and PR | Critical | **Done (2026-09-05)** |
| 10.2 Code Splitting & Bundle Optimization | Monaco/xterm/markdown lazy-loaded; initial bundle < 500KB | High | Planned |
| 10.3 Security Policy Document | Unified doc covering: workspace boundaries, credential storage, Agent approval model, MCP tool exposure, data exposure limits | Critical | Planned |
| 10.4 Cross-Platform Packaging | Windows MSI validated; macOS .dmg script; Linux AppImage/deb; add Linux/macOS CI jobs | High | Planned |
| 10.5 Secret Storage Validation | Keyring tested on Windows Credential Manager, macOS Keychain, Linux Secret Service | High | **Windows done (2026-09-05)**: platform backends enabled, round-trip test added, verified in the running app including across restart. macOS/Linux still unverified |
| 10.6 Performance Baselines | Startup < 3s, memory < 300MB idle, editor input latency < 50ms; regression tests in CI | Medium | Planned |
| 10.7 Diff Application Hardening | Version-aware hunk matching using file hash + line offset tolerance; stale rejection mandatory | High | **In progress**: backend-stamped `baseHash` (stable FNV-1a) now makes stale detection real, with intra-batch write tracking and post-apply restamping. Still open: line-offset tolerance, and rejecting edit diffs that carry no stamp at all |
| 10.8 Tauri Smoke Tests in CI | App boot, workspace open, file read/write, settings load, driven headlessly | High | Planned |
| 10.9 Agent Entry-Point Argument Structs | Collapse the 8-11 parameter orchestration entry points into request structs and drop the crate-level `clippy::too_many_arguments` allow | Low | Planned |

**Exit Criteria:** Clean CI green on Windows + macOS; installer produces working app from scratch; security doc reviewed.

---

### Phase 11: Agent Intelligence, Plan/SDD Mode & Language Expansion (Target: 4 weeks after Phase 10)

**Goal:** Expand Agent capabilities with structured design document generation, IDE planning mode, and broader language support.

| Task | Deliverable | Priority |
|------|-------------|----------|
| 11.1 Plan/SDD IDE Mode | IDE-level "Plan Mode" toggle: Agent produces design docs instead of code changes | Critical |
| 11.2 SDD Pipeline Stage | New Agent pipeline role "Designer" that outputs structured SDD Markdown | Critical |
| 11.3 SDD Template System | Markdown templates with frontmatter schema for SDD docs, stored in `docs/` | High |
| 11.4 Python LSP Adapter | pylsp/pyright integration with diagnostics, completions, hover | High |
| 11.5 Rust LSP Adapter | rust-analyzer integration | Medium |
| 11.6 Provider-Specific Tool Extensions | Shared native tool transport is done in Phase 9.0.1; this covers provider-specific extensions beyond it (parallel tool calls, strict schemas, prompt caching) | High |
| 11.7 Agent Context Token Budget UI | Real-time token meter showing budget usage per source; warn on overflow | Medium |
| 11.8 Ghost Mode (Background Analysis) | Lightweight background indexing producing proactive suggestions; user-dismissable | Medium |
| 11.9 CLI Permission Model V2 | `--deny-path`, `--allow-create/edit/delete`, `--allow-git`, plus MCP tool allowlists | Medium | **Mostly done, was mis-tracked**: `--deny-path` (with `*` / `/**` globs), `--allow-create/edit/delete`, `--allow-git` and a `policy.json` decision record already ship and are verified end to end. Open: `--apply` implies create+edit so those two flags add no granularity in apply mode, and MCP tools are not exposed to the CLI yet |
| 11.10 Workspace Indexing Scalability | Validate on 10k+ file workspaces; implement incremental indexing if needed | High |

**Plan/SDD Mode Technical Design:**

The Plan/SDD Mode is a dual-layer feature:

1. **IDE Mode Layer** - A mode toggle in `ModeSwitch` component (`src/components/shared/ModeSwitch.tsx`) switches between `code` and `plan` modes via `useAgentStore`. In plan mode, the Agent pipeline skips Coder/Tester stages and outputs documents instead of diffs.

2. **Designer Pipeline Stage** - New pipeline: `Planner -> Designer -> Reviewer`. The Designer role receives the original prompt, project context, file tree, and existing docs. It outputs structured SDD Markdown following a template schema with frontmatter (type, title, version, date, author, status, module). Output is saved to `docs/design/`.

3. **SDD Template Structure:**
   - Overview (Purpose, Scope, Definitions)
   - System Context (Architecture Position, External Interfaces, Dependencies)
   - Design Details (Component Architecture, Data Models, Interface Definitions, State Management, Error Handling)
   - Implementation Plan (Task Breakdown, File Changes, Migration Notes)
   - Quality Assurance (Test Strategy, Acceptance Criteria, Performance)
   - Risks & Mitigations
   - Open Questions

4. **Frontend Flow:** User toggles Plan Mode → describes feature → Agent runs Designer pipeline → ChatView renders SDD preview → User can edit inline, save to docs/, iterate, or "Proceed to Code" which feeds task breakdown into Coder pipeline.

5. **CLI Support:** `agent-cli plan --output docs/design/feature.md` generates SDD in headless mode.

**Exit Criteria:** Plan Mode produces valid SDD documents; Python + Rust projects get full LSP; native tool calling is covered by the Phase 9.0 exit criteria.

---

### Phase 12: Production Polish & Ecosystem (Target: 6 weeks after Phase 11)

**Goal:** Polish for public release; address accessibility, extensibility, and community onboarding.

| Task | Deliverable | Priority |
|------|-------------|----------|
| 12.1 Command Palette Enhancement | Recent commands, symbol search, workspace file search, template commands | Medium |
| 12.2 Accessibility Audit | Keyboard navigation for all panels; ARIA labels; high-contrast theme support | Medium |
| 12.3 Plugin/Extension API Design | Document extension points for: language adapters, Agent roles, UI panels | Low |
| 12.4 Troubleshooting Guide | Structured guide covering common failures with solutions | Medium |
| 12.5 Cancellation at Provider Level | Transport-abort for streaming LLM calls (where provider supports it) | Low |
| 12.6 Agent Run History & Replay | Persist Agent runs with full action logs; replay/compare past runs | Low |
| 12.7 Split View & Advanced Editor | Side-by-side file editing; enhanced minimap with Agent change indicators | Low |
| 12.8 Extended Document Types | Add HLD/LLD, RFC/ADR, Test Plan templates to Plan Mode; template marketplace | Low |
| 12.9 SDD-to-Code Pipeline | One-click "Implement from SDD" that feeds approved design tasks into Coder pipeline | Medium |

---

## Technical Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-24 | Monaco `key={activeFile}` for file switching | Simple and reliable remount behavior |
| 2026-04-24 | `portable-pty` for terminal | Cross-platform PTY support |
| 2026-04-24 | react-arborist for explorer | Virtualized file tree |
| 2026-04-24 | Zustand stores | Lightweight local state model |
| 2026-04-24 | `tokio::sync::Mutex` for Agent orchestrator | Allows async lock usage |
| 2026-04-24 | reqwest SSE streaming | OpenAI-compatible LLM API support |
| 2026-04-24 | `similar` crate for diff utilities | Existing text diff support |
| 2026-04-24 | `git2::Repository::discover` | Locate Git repo from workspace paths |
| 2026-04-30 | Tauri runtime guard in frontend | Vite web preview should not crash without Tauri APIs |
| 2026-04-30 | Workspace path service | Centralized path resolution and workspace boundary checks |
| 2026-04-30 | `ReactMarkdown skipHtml` for Agent output | Avoid rendering arbitrary LLM HTML |
| 2026-04-30 | Context compression modes | Let users choose prompt context compression strategy |
| 2026-09-05 | Hand-written MCP stdio JSON-RPC instead of the `rmcp` SDK | Only `initialize` / `tools/list` / `tools/call` are needed; ~300 lines with zero new dependencies avoids SDK version constraints against the pinned tokio/serde. Revisit if resources/prompts/sampling are needed. |
| 2026-09-05 | MCP tools namespaced as `mcp__{server}__{tool}` | Matches the Codex/Claude Code convention, keeps built-in output-protocol tools distinguishable, and makes `ToolInvoker` dispatch a prefix check |
| 2026-09-05 | MCP server `cwd` resolved through `workspace::resolve_existing` | Keeps spawned server processes inside the workspace boundary that already guards FS, Git, and terminal |
| 2026-09-05 | CI clippy gate uses `-D warnings` with two written-down crate-level allows | A gate that is warning-only gets ignored. Denying everything except `too_many_arguments` and `should_implement_trait` (both style debates on existing entry points) makes the gate real today and leaves an explicit ratchet item (10.9) |
| 2026-09-05 | CI Rust job verifies `--no-default-features` only | The default `llama-cpp` feature needs LLVM/libclang plus a full llama.cpp native build; gating on it would make CI slow and fragile while covering no code path that ships today |
| 2026-09-05 | MCP tool exposure defaults to `auto_approved_only` | "The user enabled this server" is not consent to run every tool in it. Defaulting to an explicit per-tool allowlist, and treating unknown policy strings as the restrictive case, keeps a config typo from silently opening all external tools |
| 2026-09-05 | MCP policy re-checked inside `McpRegistry::call`, not only at injection time | Not injecting a tool is not an enforcement boundary: the model can name a tool from conversation history or by guessing |
| 2026-09-05 | `baseHash` is computed by the backend, never taken from the model | The model cannot know Agent IDE's hash function, so a model-supplied value is a guess. Trusting it made the staleness check dead code in both directions |
| 2026-09-05 | Hand-written FNV-1a instead of `DefaultHasher` for `content_hash` | `DefaultHasher` output is explicitly not stable across Rust releases, and these hashes are persisted per workspace and re-validated after restart. Pinned test vectors keep it honest |
| 2026-09-05 | Base-hash check skipped for files already written in the same apply batch | Otherwise applying hunk 2 of a diff would always fail after hunk 1 landed. External edits are still caught because they happen before the batch starts |
| 2026-09-05 | `allowFileCreate` gates auto-apply only, not explicit Apply clicks | A user clicking Apply on a reviewed diff *is* the confirmation. The gap worth closing is Auto mode writing files with no confirmation at all |
| 2026-09-05 | Blocked new-file diffs stay `pending`, and the Auto run ends in `waiting_user` | Marking them `failed` would push the user toward "regenerate" for something that was never broken; ending in `done` would hide that changes still need review |
| 2026-09-05 | `keyring` must declare platform store features explicitly | v3 silently compiles a mock store when no store feature is enabled: writes return `Ok`, reads return `NoEntry`. A dependency line that looks complete had disabled all secret storage. Any future keyring upgrade must re-check this |
| 2026-09-05 | "Is a secret stored?" is answered by reading the store, never by the presence of a reference | The old `credentialRef.is_some()` check made the UI assert a key was saved when nothing was, producing a save/fail loop with no UI escape |
| 2026-09-05 | Plaintext secrets are exposed only through a dedicated reveal command | Keeps the routine profile-list response free of secrets while still allowing the standard eye-toggle affordance in settings |

---

## Command Cheat Sheet

```powershell
# Web UI preview only. Backend features are disabled/stubbed.
npm run dev

# Real IDE runtime with Rust backend.
npm run tauri -- dev

# Frontend build/type check.
npm run build

# Rust verification.
cd src-tauri
cargo check
cargo test

# Agent CLI.
cd src-tauri
cargo build --bin agent_cli --release
target\release\agent_cli --help
```

---

## Next Immediate Tasks

1. ~~Implement Phase 9.0.1 provider-native tool calling in `services/llm_client.rs` + `agent/executor.rs` with `agent-changes` kept as fallback~~ **Done (2026-09-05)**: `stream_chat_with_tools` parses OpenAI-compatible `delta.tool_calls` streams and non-streaming `tool_calls`, and `merge_tool_call_output` synthesizes `agent-changes` blocks so the parse pipeline is transport-agnostic. Live provider round-trip still pending a real API key.
2. ~~Implement Phase 9.0.3 MCP client (stdio transport, tool discovery/invocation) behind the Phase 9.0.1 tool surface~~ **Done (2026-09-05)**: `services/mcp.rs` + `commands/mcp.rs` + the `ToolInvoker` loop in `agent/executor.rs`. Live server round-trip still pending the Tauri smoke loop.
3. ~~Surface the AGENTS.md project-memory toggle in the ChatView context-source chips~~ **Done (2026-09-05)**.
4. ~~Add a CI pipeline (Phase 10.1)~~ **Done (2026-09-05)**: `.github/workflows/ci.yml` gates frontend typecheck/build/vitest and Rust fmt/clippy/tests on every push and PR.
5. Finish Phase 9.0.4 desktop permission model V2. Done so far: MCP tool approval (`McpToolPolicy` + per-server `autoApprove`) and `allowFileCreate` for Auto-mode apply, both enforced in `src-tauri`. Still open: path deny rules, a per-run cost cap, and exposing MCP tools to the CLI behind the same model. `allowFileDelete` / `allowCommandRun` / `allowGitActions` need an Agent-reachable backend path before they can be enforced at all — see the Current State note.
6. Finish Phase 10.7 diff hardening: add line-offset tolerance to hunk matching, and reject edit diffs that carry no `baseHash` at all (only safe once the Tauri smoke loop confirms per-hunk review still works end to end).
7. Run the real Tauri smoke loop for Terminal / Commands / Problems / LSP / Git / Agent repair / MCP discovery and record the commit/workspace results in `docs/smoke_test.md` release notes.
8. Runtime-verify TypeScript and Go LSP indexing in `npm run tauri -- dev`, including install/config UX, large workspace behavior, diagnostics refresh, and Quick Fix application.
9. Add frontend and Tauri smoke tests for daily workflows: open workspace, edit/save, LSP diagnostics, run test, Problems jump, Agent Fix, review/apply hunk, Git commit/push. Wire them into CI (Phase 10.8).
10. Write the unified security policy document (Phase 10.3) covering workspace boundaries, credential storage, the Agent approval model, and MCP tool exposure.
11. Add richer merge editor UI for conflict blocks, including conflict-region navigation, accept current/incoming/both per block, and post-resolution status refresh.
12. Expand Agent workflow UI with stage input/output source panels and explicit per-stage approve/skip controls.
13. Expand Command Palette with recent commands, file/symbol search, command keybinding hints, and Agent prompt templates.
14. Keep Agent CLI scoped as headless automation; broaden file/Git permissions only if CLI scope is intentionally widened.
15. Continue shared backend refactor by moving Agent run artifacts behind reusable services used by both Tauri commands and CLI without widening CLI into a second interactive IDE by default.

---

*Last updated: 2026-09-05 - Phase 9.0: native tool calling (9.0.1), AGENTS.md memory (9.0.2), and the MCP client (9.0.3) are done; permission model V2 (9.0.4) enforces MCP tool approval and Auto-mode file creation backend-side. Phase 10.1 CI is live; Phase 10.7 diff staleness detection is now real and confirmed by a headless `agent_cli` end-to-end run.*
