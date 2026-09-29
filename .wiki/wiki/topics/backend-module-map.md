---
title: "Code Map: Rust Backend (src-tauri)"
category: topic
sources: [raw/repos/2026-09-29-agent-ide-source-scan.md]
created: 2026-09-29
updated: 2026-09-29
tags: [agent-ide, code-map, rust, tauri, orchestrator, services]
aliases: [backend map, src-tauri map, 后端代码地图]
confidence: high
volatility: warm
verified: 2026-09-29
summary: "Module-level map of src-tauri: agent/ decides, services/ touches the platform, commands/ owns the Tauri seam. Entry-point symbols, dependency direction, and the invariants each module enforces."
---

# Code Map: Rust Backend (`src-tauri`)

> Four module groups declared at `src-tauri/src/lib.rs:19-22`: `agent`, `cli`,
> `commands` (private), `services`. There is **no** `tools/`, `workspace/`,
> `browser/`, `llm/` or `mcp/` directory — those concerns are single files, which
> is the first thing that surprises a reader coming from the names in `AGENTS.md`.

## Entry: `lib.rs` / `main.rs` (166 lines)

`pub fn run()` at `src-tauri/src/lib.rs:25` builds the Tauri app.

- Plugins — `lib.rs:27-30`: shell, fs, dialog, process.
- Managed state — `lib.rs:31-35`: `AgentGlobalState`, `TerminalManager`,
  `FileWatcherState`, `LspManager`, `McpState`.
- `invoke_handler(generate_handler![…])` — `lib.rs:43-157`, 105 commands. See
  [[ipc-contract|IPC Contract]] ([IPC Contract](../references/ipc-contract.md)).
- Two crate-wide clippy exceptions, each with a written rationale — `lib.rs:1-10`.

`commands` is private (`lib.rs:21`), so the seam cannot be reached from outside
the crate.

## `agent/` — decides (15 files, 19,363 lines)

### `orchestrator.rs` (6,624) — run state machine, lease, drivers

`pub struct AgentOrchestrator` at `:90-173` holds state manager, mode, steps,
diffs, external actions, SDD artifacts, tool invoker/policy/permissions, run
usage, conversation, session fields, undo stack, active claim, active cancel,
cancel registry.

A run is driven by three **free functions** over `&tokio::sync::Mutex<…>`:

- `drive_run` — `:549`
- `drive_pipeline` — `:601`
- `drive_repair` — `:679`

Every state mutation is a *synchronous* method, so the lock is never held across
an `await`. Holding it would queue every other command behind a model call.

Lease and cancellation:

- `RunClaim` `:183`, `RunLease` `:195` (`#[must_use]`).
- `CancelRegistry(Arc<std::sync::Mutex<Option<Arc<AtomicBool>>>>)` `:220` —
  `publish` `:223`, `clear` `:229`, `cancel_active_run` `:238`. Note the *std*
  mutex, not tokio's: reading the switch needs no `await`, which is what lets Stop
  read it *before* taking the orchestrator lock. Otherwise Stop would need the very
  lock held by the work it is cancelling.

- `try_begin_run(run_id, &WorkspaceToolPermissions)` `:1607` takes the switch via
  `permissions.cancel_switch()` at `:1632` — it does not accept a flag argument,
  which is what makes "one flag per run" unforgeable by a caller.
- `finish_run(claim)` `:1666` with an owner check at `:1667`; `abandon_run()`
  `:1685` sets cancel, then releases.

Write publication and undo — the product's reason to exist:

- `record_tool_writes` `:923`, `record_detected_writes` `:942`,
  `record_writes_from` `:963`, `ApplyCheckpoint` `:249`.
- External-action ledger: `ExternalActionRecord` `:49-68`,
  `MAX_EXTERNAL_ACTIONS = 200` `:72`, `forget_restored_external_actions` `:909`.

### `workspace_tools.rs` (5,790) — the built-in tool surface and per-run authority

Module doc at `:1-11` states why built-ins bypass MCP: every path goes through
`resolve_existing` and credential denial.

Tool name constants `:21-78` — read/search/glob/grep/list, `run_command`,
write/edit/delete/move, `browser_*`, `computer_*`, `ask_user_question`,
`web_fetch`, `delegate_task`.

`WorkspaceToolPermissions` `:214-328` is the authority object: ten switch+list
pairs — commands `:216`, write `:222`, create `:225`, browser `:228`/origins
`:233`, page read `:239`/origins `:243`, computer `:248`/apps `:254`, capture
`:259`/apps `:261`, input `:268`/apps `:270` — plus `run_id` `:277`, a private
`cancel` `:286`, and the collected `writes`/`detected_writes`/`external`/`images`
buffers `:289-327`. `adopt_cancel` `:679`, `cancelled` `:684`,
`pub(crate) cancel_switch` `:690`.

A sub-agent inherits the *same* switch: `child.adopt_cancel(self.cancel_switch())`
at `:505`. Every write resolves through `workspace::resolve_for_agent_write`
(`:2504`, `:2596`, `:2652`, `:2730`, `:2731`).

### The rest of `agent/`

- `events.rs` (93) — the only exit to the frontend. `trait RunEvents` `:17`,
  `impl for tauri::AppHandle` `:21` (mirrors each event into the run log *before*
  emitting, `:30-32`), `SilentEvents` `:37`, `RecordingEvents` `:48`.
- `executor.rs` (2,614) — runs one stage against the model and drives the tool
  loop. `trait ToolInvoker` `:31-49`, `MAX_TOOL_ITERATIONS = 12` `:57`,
  `MAX_OUTPUT_CONTINUATIONS = 2` `:64`, `execute_stage` `:703`.
- `approval.rs` (696) — human approval and model questions.
  `ApprovalRegistry` `:195` (`resolve` `:227`, `answer` `:245`, `refuse_all`
  `:257`), `ApprovalGate` `:290` constructed with `Arc<dyn RunEvents>` `:306`,
  `ask` `:325`, `ask_question` `:368`. Default timeout 120 s `:42`.
- `diff_apply.rs` (1,348) — `apply_pending_diffs` `:211`,
  `apply_pending_diffs_with_snapshots` `:219`, write gate `:249`.
- `diff_gen.rs` (37) — unified diff text.
- `state_machine.rs` (333) — the data model: `AgentState` `:6`, `AgentMode` `:39`,
  `IdeMode` `:46`, `AgentEvent` `:98`, `TaskStep` `:126`, `FileDiff` `:141`,
  `DiffProvenance` `:153`, `DiffHunk` `:194`, `AgentStateManager` `:231`.
- `multi_agent.rs` (239) — roles and pipeline stages.
- `planner.rs` (183) — `plan_task` `:163`.
- `task_shape.rs` (177) — decides whether the full pipeline is warranted.
- `subagent.rs` (291) — read-only sub-agent; recursion depth is structurally 1.
- `session_store.rs` (427) — session history persistence.
- `external_log.rs` (493) — on-disk ledger of actions that cannot be undone:
  `load_for_current_workspace` `:104`, `append_for_current_workspace` `:127`.

## `services/` — touches the platform (20 files, 16,726 lines)

The authoritative list is `src-tauri/src/services/mod.rs:1-23`. No re-exports, so
every caller writes `crate::services::<mod>::…`. Four modules carry a one-line
Chinese doc pointer at the `mod` declaration (`:4`, `:10`, `:18`, `:21`).

### `workspace.rs` (560) — every path boundary

`config_dir` `:3` (the test build never falls back to a real home, `:14-16`),
`save/load_workspace_path` `:21`/`:32`, `current_workspace_key` `:50`,
`workspace_root` `:58`, `shell_compatible_path` `:75`, `resolve_existing` `:90`,
`resolve_for_write` `:100`, **`resolve_for_agent_write` `:128-138`**,
`normalize_component_for_denial` `:152-155`, `agent_write_denial` `:158`
(`DENIED_DIRS = [".git", ".agent-ide", "node_modules"]` `:160`),
`is_credential_file_name` `:199`.

The normalization step is a security boundary, not tidiness: on Windows
`.git./hooks` and `name::$DATA` resolve like `.git/hooks` and `name`, so
deny-list comparison must go through `normalize_component_for_denial`.

### `llm_client.rs` (4,391) — provider HTTP, streaming, metering

`LlmClient` `:1233`, `CONNECT_TIMEOUT = 15s` `:1273` with no total/read timeout by
design (`:1266-1272`), `new` `:1276` forcing HTTP/1.1 for DeepSeek `:1279-1283`,
`tools_were_rejected` `:1308`, `image_drops` `:1316`, micro-USD pricing doc `:608`.

This client keeps the system proxy deliberately — it is the mirror image of the
CDP client below.

### `llm_profiles.rs` (1,250) — profiles, pricing, spend caps

`prompt_micros_per_million` `:35`, `completion_micros_per_million` `:37`,
`max_run_spend_micros` `:41`, cap normalization `:671-681`. Money is integer
micro-USD everywhere; the frontend has the matching rule.

### `browser.rs` (1,219) — CDP over loopback

`configured_port` `:34`, `normalize_target_url` `:49`, `parse_page_targets` `:117`,
`origin_allowed` `:193`, `list_tabs` `:264`, `open_url` `:277` (PUT, because
Chrome ≥111 rejects GET on `/json/new`, `:275-276`), `MAX_PAGE_TEXT_CHARS =
20_000` `:322`, `validate_page_ws_url` `:498`, `verify_read_origin` `:518`,
`read_page_text` `:589`, `test_support::FakeCdp` `:663`.

Three invariants worth knowing:

- `cdp_client()` `:239-246` sets **`.no_proxy()`**, reasoned at `:233-238`:
  reqwest honours `HTTP_PROXY`/`ALL_PROXY`, so a 127.0.0.1 request would be
  routed to a corporate proxy — failing unrecognisably *and* disclosing the URL
  the Agent is opening to a third party.
- `validate_page_ws_url` `:498-507` requires the prefix `ws://127.0.0.1:<port>/`.
  The trailing slash *is* the check: without it `ws://127.0.0.1:9222@evil.example/x`
  passes, because userinfo `@` swallows the authority (`:494-497`).
- `verify_read_origin` `:518-526` re-checks origin after the read and deliberately
  does not name the origin it landed on — naming it would complete the disclosure
  it is refusing.

### `capture.rs` (802) — screenshot one approved window

`MAX_CAPTURE_PIXELS = 4_000_000` `:16`, `select_capture_target` `:63`,
`check_capture_pixels` `:108` (`saturating_mul` `:109`), `encode_png` `:126`,
`ApprovedWindow` `:196` with `verify` `:261`.

`ApprovedWindow::verify` `:261-299` is the four checks: window still alive `:268`,
same app after `normalize_app_name` `:272-279`, same pid when both known
`:280-288`, same window class when both known `:289-297`. Title is deliberately
allowed to change (`:254-257`) to avoid approval fatigue, and the doc admits
sibling windows of the same class remain indistinguishable (`:250-252`). Identity
travels as the `isize` handle value, never as a title lookup — titles are
attacker-controlled (`:191-194`). Zero filters, zero matches and *multiple*
matches all refuse rather than guess (`:63-104`).

### `computer.rs` (751) — read-only window enumeration

`DesktopWindow` `:21`, `normalize_app_name` `:37`, `app_allowed` `:62`,
`filter_windows` `:77`, `format_windows` `:93`, `disclosed_apps` `:127`, Win32
`list_windows` `:163` / `window_pid` `:231` / `window_class` `:320`, non-Windows
stubs `:349-368`. Module doc `:1-15` records that the allow list filters
**results**, not merely tool existence — "browser tabs made exactly the latter
mistake" — because the window titles are themselves the sensitive payload.

### `input.rs` (925) — inject one gesture into an approved window

`Gesture` `:30`, `check_scroll_notches` `:91`, `check_click_inside` `:111`,
`check_same_size` `:129`, `normalized_absolute` `:147`, `send_gesture` `:550`.

Module doc `:1-23` is the authority record: coordinates are only valid against a
frame id from a prior `computer_capture`; a resized window is refused rather than
approximated; `SendInput` is chosen over posted `WM_LBUTTONDOWN` because posted
messages silently do nothing in Chrome/Electron/WPF and "clicked but nothing
happened" makes the model retry. Scroll is *not* coordinate-routed —
`WM_MOUSEWHEEL` follows focus (`:20-23`). `normalized_absolute` `:140-172`
divides by `virtual_width - 1` so the rightmost column (often the close button) is
reachable, and subtracts the virtual-desktop origin, which is negative on
multi-monitor. Order is foreground → confirm foreground → *then* measure
(`:204-205`), polling 20 × 25 ms because `SetForegroundWindow` is asynchronous
(`:190-197`).

### `mcp.rs` (1,283) — hand-written MCP client

`MCP_TOOL_PREFIX = "mcp__"` `:20`, `REQUEST_TIMEOUT_SECS = 30` `:22`,
`MAX_TOOL_RESULT_CHARS = 64_000` `:32`, `McpServerConfig` `:41`,
`McpToolPolicy` `:64`, `McpConfig` `:95`, `McpToolDescriptor::to_tool_definition`
`:152`, `qualify_tool_name` `:197`, `McpConnection::spawn` `:220`,
`ArgumentPaths` `:502`, `McpRegistry` `:726` (`discover` `:738`, `call` `:808`,
`shutdown_all` `:843`, `retain_configured` `:867`).

The rmcp SDK was rejected on purpose (`:1-6`): three methods are needed and an
SDK would import tokio/serde version constraints. Server stderr is `Stdio::null()`
so a chatty server cannot block the pipe (`:226-227`), `kill_on_drop(true)`
`:228`, and a configured `cwd` still passes `workspace::resolve_existing`
(`:232-235`). `ArgumentPaths` is documented as **description, not authorization**
(`:496-500`): the schema belongs to an external server, so calls are *recorded*
(outside-the-workspace targets listed first, `:521-527`) rather than refused, and
the split is purely lexical because the call already happened (`:536-538`).

### Named but not individually read

`agent_runtime`, `command_text`, `context`, `credentials`, `images`,
`problem_parser`, `project_memory`, `project_tasks` (partially), `run_log`,
`verification`, `web_fetch` — all declared in `services/mod.rs`. Two are worth
flagging from `AGENTS.md` rather than from a read: `project_memory.rs` injects
this repo's `AGENTS.md` into every run bounded at **8,000 bytes** (the tail is
silently truncated past it), and `project_tasks.rs` runs a workspace shell
command cancellably, killing the *process tree* on Stop. Treat both as unverified
here until read.

## `commands/` — the Tauri seam (9 files, 8,298 lines)

`commands/mod.rs:1-8` declares the eight submodules.

- **`agent.rs` (3,840)** — 59 commands plus the shared run-finish helpers. Entry
  points: `send_agent_prompt` `:391`, `stop_agent` `:1014`, `run_agent_step`
  `:1180`, `continue_agent_pipeline` `:1473`, `repair_workspace` `:1887`,
  `apply_diffs` `:1648`, `undo_last_apply` `:1748`, `revert_turn_changes` `:3553`,
  `forget_settled_diffs` `:3149`, `get_agent_external_actions` `:3162`.
- **`fs.rs` (628)** — workspace-scoped file CRUD, search, and the watcher:
  `read_file_content` `:38`, `write_file_content` `:45`, `list_directory` `:55`,
  `search_files` `:321`, `watch_start` `:410`, `watch_stop` `:488`.
- **`git.rs` (1,358)** — git2-backed: `git_status` `:43`, `git_diff` `:385`,
  `git_commit` `:538`, stage/unstage/discard `:443`/`:466`/`:481`, branch and
  remote ops `:149-298`, `git_resolve_conflict` `:324`.
- **`lsp.rs` (1,596)** — single-server lifecycle and request forwarding:
  `lsp_probe` `:109`, `lsp_initialize` `:241`, hover/definition/completion/
  symbols/rename/code-actions `:409-511`, `lsp_status` `:536`.
- **`mcp.rs` (619)** — config and discovery commands `:387-458`, plus
  `McpToolInvoker`, which both decides and emits and therefore takes
  `&dyn RunEvents`.
- **`terminal.rs` (215)** — PTY `spawn_terminal` `:32`, `write_to_terminal` `:146`,
  `resize_terminal` `:173`, `kill_terminal` `:202`.
- **`browser.rs` (19)** — `browser_list_tabs` `:11`, `browser_open_url` `:17`.
- **`tasks.rs` (15)** — `discover_project_tasks` `:6`, `run_project_task` `:11`.

There is no `computer_*` invokable command. Computer use reaches the user only
through the Agent tool surface, never as a direct invoke — a deliberate asymmetry
with `browser_*`.

### The cancel flag, concretely

The command layer mints one `Arc<AtomicBool>` per run and **moves** it into
`WorkspaceToolPermissions::adopt_cancel`. Nothing else accepts a flag:
`attach_mcp_tools` and `try_begin_run` both take `&WorkspaceToolPermissions` and
read `cancel_switch()` themselves, so no caller can put the lease, the built-in
tools and the MCP tools on different flags. Adopt must happen *before* the claim
(all four run-starting commands do). The flag is never reset — a shared flag once
let a new prompt un-cancel a draining old run. After Stop, `invoke()` refuses
calls that have not started, and `run_project_command_cancellable` kills the
process tree of one that has.

## `cli/` — headless binary (3,344 + 11 lines)

`src-tauri/src/cli/mod.rs` with the binary in `src-tauri/src/bin/`. Not mapped
here. `AGENTS.md` records that a fabricated CLI help block has already shipped as
documentation drift once, so this section stays blank until the file is read
rather than being filled from inference. Note that `cargo test --bin agent_cli`
runs **zero** tests; the suite is `cargo test --lib`.

## See Also

- [[code-map-overview|Repository Overview]] ([Repository Overview](code-map-overview.md)) — how the four groups fit together
- [[ipc-contract|IPC Contract]] ([IPC Contract](../references/ipc-contract.md)) — the command and event tables this module group exposes
- [[frontend-module-map|Frontend Module Map]] ([Frontend Module Map](frontend-module-map.md)) — the consumer on the other side of `RunEvents`

## Sources

- [agent-ide source scan @2b35623](../../raw/repos/2026-09-29-agent-ide-source-scan.md) — commit and method; also lists which `services/` modules went unread


