---
title: "IPC Contract: Commands and Events"
category: reference
sources: [raw/repos/2026-09-29-agent-ide-source-scan.md]
created: 2026-09-29
updated: 2026-09-29
tags: [agent-ide, code-map, tauri, ipc, events, commands]
aliases: [invoke handler list, event list, 前后端契约]
confidence: high
volatility: hot
verified: 2026-09-29
summary: "The full seam between Rust and React: 105 commands registered in lib.rs grouped by theme, all 16 events emitted to the frontend with their emission sites, and a both-directions cross-check showing an exact 16/16 match."
---

# IPC Contract: Commands and Events

> Everything crossing the seam does so in one of two ways: a command the frontend
> invokes, or an event the backend emits through `RunEvents::emit_json`
> (`src-tauri/src/agent/events.rs:17`). This article is the table for both.
> Marked `volatility: hot` — the lists change whenever a feature lands.

## Commands — 105, registered at `src-tauri/src/lib.rs:43-157`

Grouped by theme, source order preserved within each group.

**Files / editor** (`lib.rs:45-57`) — `read_file_content`, `write_file_content`,
`list_directory`, `delete_path`, `create_file`, `create_directory`, `rename_path`,
`reveal_in_file_explorer`, `copy_path`, `get_file_metadata`, `search_files`,
`watch_start`, `watch_stop`

**Agent run control** (`lib.rs:59-70`, `110-112`) — `get_agent_state`,
`estimate_agent_context`, `send_agent_prompt`, `stop_agent`, `update_agent_step`,
`update_agent_steps`, `skip_agent_step`, `run_agent_step`,
`continue_agent_pipeline`, `set_agent_mode`, `get_agent_steps`,
`verify_workspace`, `repair_workspace`, `agent_repair_prompt`

**Approval / question** (`lib.rs:63-64`) — `resolve_agent_approval`,
`answer_agent_question`

**Diffs / undo** (`lib.rs:71-80`, `107-109`) — `apply_diffs`, `apply_diff`,
`apply_diff_hunk`, `reject_diffs`, `reject_diff`, `reject_diff_hunk`,
`get_agent_diffs`, `adopt_restored_diffs`, `forget_settled_diffs`,
`revert_turn_changes`, `undo_last_apply`, `pending_undo`

**External-action ledger** (`lib.rs:81-82`) — `get_agent_external_actions`,
`forget_earlier_external_actions`

**Sessions / conversation** (`lib.rs:96-106`) — `start_new_agent_session`,
`list_agent_sessions`, `resume_agent_session`, `rename_agent_session`,
`fork_agent_session`, `delete_agent_session`, `get_agent_conversation`,
`truncate_agent_conversation`

**LLM config / profiles** (`lib.rs:85-90`, `113`) — `get_llm_config`,
`save_llm_profile`, `set_active_llm_profile`, `reveal_llm_api_key`,
`delete_llm_profile`, `set_context_compression`, `test_llm_connection`

**Roles / pipeline** (`lib.rs:91-95`) — `set_active_role`, `get_active_role`,
`get_pipeline`, `update_pipeline`, `reset_pipeline`

**SDD artifacts** (`lib.rs:83-84`) — `get_agent_sdd_artifacts`,
`save_sdd_artifact`

**MCP** (`lib.rs:117-120`) — `get_mcp_config`, `save_mcp_config`,
`discover_mcp_tools`, `get_mcp_tools`

**Git** (`lib.rs:122-133`) — `git_status`, `git_diff`, `git_commit`,
`git_stage_files`, `git_unstage_files`, `git_discard_files`, `git_checkout_branch`,
`git_checkout_remote_branch`, `git_fetch`, `git_pull`, `git_push`,
`git_resolve_conflict`

**Tasks** (`lib.rs:135-136`) — `discover_project_tasks`, `run_project_task`

**LSP** (`lib.rs:138-148`) — `lsp_initialize`, `lsp_open_file`, `lsp_change_file`,
`lsp_hover`, `lsp_definition`, `lsp_completion`, `lsp_document_symbols`,
`lsp_rename`, `lsp_code_actions`, `lsp_status`, `lsp_probe`

**Terminal** (`lib.rs:150-153`) — `spawn_terminal`, `write_to_terminal`,
`resize_terminal`, `kill_terminal`

**Browser** (`lib.rs:155-156`) — `browser_list_tabs`, `browser_open_url`

**Misc** (`lib.rs:103-105`, `114-115`) — `get_project_memory`, `append_ui_log`,
`run_log_path`, `save_workspace_path`, `get_workspace_path`

No `computer_*` command is registered. Computer use — window listing, capture,
click — reaches the user only through the Agent tool surface
(`services/computer.rs`, `capture.rs`, `input.rs`), never as a direct invoke. The
asymmetry with `browser_*` is deliberate: the browser commands exist for the
command palette.

## Events — 16, all through `RunEvents::emit_json`

Only three have named constants; the other thirteen are inline literals. The
`AppHandle` implementation mirrors every event into the run log **before** calling
`emit` (`agent/events.rs:30-32`), which is what makes a post-mortem possible after
the UI log panel is gone.

- `agent-approval-requested` — const `APPROVAL_REQUESTED_EVENT`
  `agent/approval.rs:25`, emitted `:340-341`
- `agent-question-requested` — const `QUESTION_REQUESTED_EVENT` `approval.rs:31`,
  emitted `:381-382`
- `agent-approval-closed` — const `APPROVAL_CLOSED_EVENT` `approval.rs:36`,
  emitted `:356` and `:396` (shared by the approval and question paths)
- `agent-state-changed` — `agent/orchestrator.rs:2852` (`emit_state_to`);
  `commands/agent.rs:808, 1664, 1695, 1725, 1761, 2098, 2126, 2150, 3584`
- `agent-diff-ready` — `commands/agent.rs:802, 1360, 1757, 3580`;
  `orchestrator.rs:2173, 2608, 2666`
- `agent-plan-ready` — `commands/agent.rs:1131`; `orchestrator.rs:1834, 3268`
- `agent-step-update` — `commands/agent.rs:1166, 1270, 1356, 1457`;
  `orchestrator.rs:2870`
- `agent-stream-token` — `commands/agent.rs:1255`; `orchestrator.rs:577, 633, 722`
- `agent-action-log` — `commands/agent.rs:595`; `commands/mcp.rs:99`
  (`McpToolInvoker`) and `:452` (discovery); `orchestrator.rs:2903`
- `agent-context-usage` — `commands/agent.rs:1006` (`emit_usage_action_log`)
- `agent-sdd-ready` — `orchestrator.rs:2146, 2769, 2791`
- `agent-pipeline-update` — `orchestrator.rs:2862`
- `lsp-status` — `commands/lsp.rs:630, 641`
- `lsp-diagnostics` — `commands/lsp.rs:677`
- `terminal-output` — `commands/terminal.rs:114`
- `file-changed` — `commands/fs.rs:467`

Strings that look like events and are not: `lsp-indexing-*` are temp-dir names in
tests (`commands/lsp.rs:1503-1583`), `agent-changes` is a CLI patch-protocol tag
(`cli/mod.rs:2541`), `agent-ide*` are service/config names, and `agent-step` is a
diff `source_role`.

## Cross-check: both directions

Frontend subscriptions, all via `@tauri-apps/api` `listen`:

- `src/hooks/useAgentBridge.ts` — `agent-state-changed` `:53`,
  `agent-plan-ready` `:71`, `agent-step-update` `:76`, `agent-diff-ready` `:81`,
  `agent-sdd-ready` `:101`, `agent-pipeline-update` `:106`,
  `agent-context-usage` `:112`, `agent-action-log` `:117`,
  `agent-stream-token` `:151`, `agent-approval-requested` `:157`,
  `agent-question-requested` `:176`, `agent-approval-closed` `:194`
- `src/hooks/useLspDiagnostics.ts` — `lsp-diagnostics` `:23`, `lsp-status` `:55`
- `src/components/panels/Terminal.tsx` — `terminal-output` `:400`
- `src/components/panels/Explorer.tsx` — `file-changed` `:304`

**Result: exact 16/16.** No backend event goes unlistened, and no frontend
listener waits on an event the backend never emits.

## Run-finish helpers

`finish_agent_run`, `publish_tool_writes`, `publish_external_actions`, and both
`emit_*_degradation_log` live in `commands/agent.rs` and take `&dyn RunEvents`
rather than `AppHandle`. The reason is recorded in `AGENTS.md`: every defect found
in those helpers across four review cycles was a wording or counting defect, and a
signature taking `AppHandle` cannot be tested for either. `McpToolInvoker`
(`commands/mcp.rs`) is under the same rule, because it both decides and emits.

## See Also

- [[backend-module-map|Backend Module Map]] ([Backend Module Map](../topics/backend-module-map.md)) — which module each command lives in
- [[frontend-module-map|Frontend Module Map]] ([Frontend Module Map](../topics/frontend-module-map.md)) — the normalizers every event payload passes through
- [[code-map-overview|Repository Overview]] ([Repository Overview](../topics/code-map-overview.md))

## Sources

- [agent-ide source scan @2b35623](../../raw/repos/2026-09-29-agent-ide-source-scan.md) — commit and method
