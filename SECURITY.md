# Agent IDE Security Policy

## Overview

Agent IDE follows a suggest-then-apply model: the Agent's proposed changes are visible and reviewable before they touch the disk, and the review flow is covered by backend tests rather than by manual clicking.

This document states what the backend actually enforces and what it does not. Claims here were verified by reading the enforcing code; where a protection is missing or partial it is listed as such rather than described aspirationally. The single largest gap is MCP tool exposure — see that section before enabling an MCP server.

## Workspace Boundary Model

All filesystem operations (read/write/delete) are scoped to the open workspace root. The centralized workspace service enforces this boundary:

- **`workspace::resolve_existing`** — resolves read paths after canonicalization and rejects any path outside the workspace root. Also rejects relative traversal (`../../etc/passwd`) by canonicalizing the candidate and checking the prefix.
- **`workspace::resolve_for_write`** — resolves write paths, including new files that do not yet exist, by validating the nearest existing ancestor is within the workspace. Rejects writes outside the workspace boundary.
- **`workspace::ensure_within_workspace`** — lower-level guard used by both resolve functions. Compares the canonical path prefix against the workspace root.
- **`workspace::shell_compatible_path`** — normalizes Windows verbatim `\\?\D:\...` and `\\?\UNC\...` prefixes into shell-compatible paths, preventing canonicalization mismatches on Windows.

Surfaces that enforce the workspace boundary:

- Filesystem commands (`read_file_content`, `write_file_content`, file tree operations)
- Agent-generated diffs (`apply_pending_diffs` resolves each target through `resolve_for_agent_write`, which adds the deny list below)
- Per-file Git path operations (stage, unstage, discard, conflict resolution, per-file commit)
- Terminal cwd (spawned PTY sessions start in the workspace root; Windows `\\?\` prefixes are stripped before passing to `cmd.exe`)
- Project task cwd (non-interactive command runner inherits workspace-scoped cwd)
- Agent CLI (shared workspace resolution and boundary checks)

Surfaces that do **not** fully enforce it:

- **Repository-wide Git operations.** Every Git command starts with `git2::Repository::discover`, which walks *upward*. If the workspace root is a subdirectory of a larger repository, the repository working directory is an ancestor of the workspace root, and worktree-wide operations — `checkout_head` (branch checkout, pull, discard) and `git_commit` with no explicit file list, which does `index.add_all(["*"])` — act on files outside the workspace. The git-diff *context* section is scoped back to the workspace with a pathspec, so it no longer leaks sibling directories to the model, but the write-side operations remain unscoped.
- **Recursive traversal.** `search_recursive` and `copy_dir_recursive` check the root once and then descend without re-checking each entry, so a symlinked directory inside the tree is followed.
- **Language servers.** `find_language_server` prefers `<workspace_root>/node_modules/.bin/...` before anything on `PATH`. Opening an untrusted repository therefore executes a binary that repository supplies. There is no signature check.
- **MCP tools.** See the MCP section — entirely unchecked.
- **`save_workspace_path`** accepts any canonicalizable directory, so the boundary itself is caller-defined. This is by design.
- Config files under `~/.agent-ide` (`workspace.json`, `config.json`, `mcp.json`) are outside the boundary by design.

Path traversal protection:

- Absolute paths are checked against the workspace root after canonicalization.
- Relative paths are joined against the workspace root first, then canonicalized and checked.
- Windows verbatim path prefixes (`\\?\`) are normalized centrally to prevent canonicalization bypasses.

Symlink handling: canonicalization defeats `../` traversal and normalizes Windows verbatim prefixes before comparison, both covered by tests. But recursive traversal helpers re-check nothing per entry, and `resolve_for_write` does not canonicalize the final component of a path that does not exist yet — see Known Limitations.

## Content Security Policy

The Tauri WebView enforces a CSP:

```
default-src 'self' ipc: http://ipc.localhost;
script-src 'self';
style-src 'self' 'unsafe-inline';
img-src 'self' asset: https://asset.localhost data:;
connect-src 'self' ipc: http://ipc.localhost http://localhost:* https://*
```

- Script loading is restricted to `self` — no inline scripts or external script sources.
- `connect-src` allows HTTPS connections (required for LLM API streaming) and `localhost` (required for Tauri IPC and Vite dev server).
- The CSP was restored after an earlier period where it was set to `null`; it is now enforced.

## Credential Storage

LLM API keys and Git HTTPS credentials are stored via the OS credential store using the `keyring` crate:

- **Windows**: Credential Manager
- **macOS**: Keychain
- **Linux**: Secret Service (libsecret)

Implementation details:

- Service name: `agent-ide`
- LLM credential references: `llm-profile:<profile_id>`
- Git credential references: `git-remote:<remote_url>`
- `~/.agent-ide/config.json` stores `credentialRef` strings. The `api_key` field on `LlmProfile` is `#[serde(default, skip_serializing)]`, so a config file written by this app never contains a plaintext key.
- Frontend responses use `api_key_masked` (`first4****last4`). `masked_api_key()` probes the credential store rather than trusting the presence of a `credentialRef`, so a reference whose entry cannot be read reports `not configured` instead of falsely claiming a key is stored.
- The only plaintext-over-IPC path is `reveal_llm_api_key`, used by the Settings eye toggle.
- No key is transmitted anywhere other than the configured LLM provider endpoint.

Known limitations:

- **A plaintext key can persist on disk.** `skip_serializing` prevents *writing* one, but not reading one. A legacy or hand-edited `config.json` containing `api_key` is read on load, and `migrate_profile_credentials` then tries to move it into the keyring. If that store operation fails the file is deliberately left unrewritten so the key is not lost — meaning the plaintext stays on disk and is preferred over the keyring entry in memory. There is no file-permission hardening on `~/.agent-ide`.
- **MCP tool arguments are logged with secret-looking keys redacted.** Values under keys containing `token`, `secret`, `password`, `passwd`, `apikey`, `api_key`, `authorization`, or `credential` are replaced with `[redacted]` before the argument JSON reaches the action log; arguments that are not valid JSON are not logged verbatim at all. Redaction keys on the field *name*, so a secret passed under an innocuous key is still logged.
- `reveal_llm_api_key` has no confirmation prompt, rate limit, or audit entry.
- Git HTTPS credentials are passed as plaintext over IPC by design, and persisted (when the user opts in) as `"{user}\n{pass}"` in the OS store. `GIT_USERNAME` / `GIT_PASSWORD` are accepted as an environment fallback.
- macOS Keychain and Linux Secret Service backends are enabled but have not been runtime-validated. Windows Credential Manager has been verified end to end, including across an app restart.

## Built-in Workspace Tools

The Agent has four built-in read-only tools — `workspace_read_file`, `workspace_search_text`, `workspace_list_files`, `workspace_read_image` — so it can decide what to read instead of relying only on the pre-assembled context bundle. Unlike MCP tools, these are constrained:

- Every path goes through `resolve_existing`, so reads are confined to the workspace.
- Credential files are refused outright, matching the context egress rule. Without that they would be a bypass: the model could simply call the read tool to get the `.env` contents the prompt builder withholds.
- Traversal skips `.git`, `node_modules`, `target`, `dist`, `build`, `.agent-ide`.
- Output is capped (64 KB per file read, 60 search hits, 200 directory entries).
- They cannot write, delete, move, or execute anything.
- Every call is written to the action log as a `workspace_tool_call` entry, so what the Agent read is auditable.
- `workspace_read_image` adds four limits of its own: the type must be png / jpg / gif / webp (an unknown extension is refused locally rather than guessed), the size is checked with `metadata` **before** the file is read (so a 2 GB `.png` is refused instead of loaded), one image is capped at 4 MiB, and **a whole run is capped at 16 MiB of image bytes**. The run cap is the one the per-image cap cannot cover: a round executes every tool call the model emitted, so without it the total is unbounded. It is checked before the read (pinned by a test: with the budget nearly spent, a non-image file fails with the *budget* message, not the media-type one) and charged only after a successful parse, so a refused or non-image read does not spend it; the refusal names the used, requested and limit bytes so the model can choose a smaller image instead of retrying the same one. A run that inherits its tool surface from a previous run — `continue_agent_pipeline`, `repair_workspace`, both of which clone the permissions object — calls `reset_image_budget()`, so the counter follows the run and not the clone. A **single request** is bounded separately at 12 MiB of image base64 (`fit_images_in_request`), because a round can contain several read calls and the run budget alone would let ~22 MB of base64 into one request, over the ~20 MB most providers accept. Images past that are **deferred to the next round**, not discarded — they were already charged against the run budget when they were read, so telling the model to read them again would walk it into the cap with advice it cannot follow. The message names what happened ("Attached the first N of M … The last K … will be attached in the next step — do not read them again"), and the only case an image is genuinely lost is the tool loop ending first, which is reported through the same `image_input_degraded` warning as every other image degradation. The denial check runs twice — once on the path the model asked for and once on the **resolved** path — because a committed symlink `docs/mockup.png -> ../.env` passes a name check and `resolve_existing` follows it. `read_file` now does the same. The image rides on a `user` message appended after the tool results (OpenAI Chat Completions accepts image blocks there, not on a `tool` message) and is sent **once**: the executor clears images from the transcript after each request, because a 4 MiB image is ~5.3 MB of base64 that would otherwise be re-sent every round of a 12-round tool loop. If the configured model is not a known vision model, or the endpoint is the local/mock text-only path, the images are removed and the text says so — and the removal is **also reported to the user** as an `image_input_degraded` warning in the action log, naming how many images and why. The note in the message text is only visible to the model; on its own it left the user with an answer that looked normal and no way to tell whether the model ever saw the picture.

Both degradation warnings — `image_input_degraded` and `tool_capability_degraded` — are emitted from `finish_agent_run`, so they appear on every exit path of a prompt, step or pipeline run including failure and Stop, and `repair_workspace` emits them explicitly because it assembles its own finish sequence. A degradation is a fact about a request that already went out; the run failing afterwards does not undo it, and the failed run is the one that most needs the clue. `agent_cli` has no action log, so it prints the same sentence to **stderr** — stdout carries the JSON/NDJSON a caller parses.

They are advertised when the profile's `toolCallMode` is `native_tools`, which is the default for cloud profiles. If the endpoint rejects a `tools` parameter the client drops it, retries once, and writes a `tool_capability_degraded` warning to the action log — so a run without tools is visible rather than silent. The tool loop is bounded at 12 rounds per stage, with the per-run token cap as the real cost limit.

## Desktop Observation

`workspace_computer_windows` lists the visible top-level desktop windows — title, app, size, and which one is in the foreground. This is the first slice of computer use and it is **read-only**: nothing on the desktop is clicked, typed into or captured.

- **Two independent gates, plus the platform.** `allows_computer()` requires the `allowComputerUse` grant, a non-empty app allowlist, *and* `cfg!(windows)`. Only Windows has an implementation; elsewhere the tool is not advertised at all, because a tool that always fails gets called repeatedly and its failure reads as "there are no windows".
- **The allowlist filters the results, not just the tool's existence.** This is the mistake `workspace_browser_tabs` made: there, allowlisting one origin still disclosed every open tab. Here, windows whose app is not on the list are removed from the output, and only the *count* of hidden ones is reported. Matching is on the **executable name only** — `normalize_app_name` strips path, case and `.exe`, so `Code.exe`, `code` and `C:\…\Code.exe` all mean the same thing. Pasting a full path does not scope the entry to that install: any process named `code.exe`, wherever it lives, matches.
- **Window titles are the thing being protected.** They carry document names, page titles and the other party's name in a chat window. That is why the authority is per-app and the disclosure is recorded, even though the tool changes nothing.
- **A window whose process cannot be read is never disclosed**, not even under `*`. `window_app` reports `unknown`, and `app_allowed` refuses that value outright: the usual reason the process handle cannot be opened is that it belongs to an elevated process, and those windows are disproportionately the sensitive ones. A window nobody can attribute to an approved app is not covered by any approval. (Cost: a real `unknown.exe` is also invisible.)
- **Recorded like an irreversible action.** A successful listing lands in the run's external action log as `computer_windows`, naming how many windows were disclosed, how many were hidden, and which apps; a call refused after Stop lands as `computer_windows_cancelled`; a transport-level failure as `computer_windows_failed`. The disclosure itself cannot be taken back, which is the same reason navigation is recorded.
- **An *unauthorized* call never reaches the tool at all.** `handles()` returns false, and both `select_external_calls` and `CompositeToolInvoker::invoke` filter on it, so the model's call is dropped before dispatch and leaves no external-action record — `computer_windows_refused` exists only as a defence-in-depth check for a direct caller. The same is true of the browser tools' in-tool gate.
- **Stop applies.** The tool is in the side-effect gate list, so a call that has not started is refused after Stop.

**Window capture** (`workspace_computer_capture`) is a second, **separate** grant: its own switch plus its own non-empty app list. It deliberately does not ride on Desktop Observation — a title says "Signal is open", a capture shows the messages — so allowing the Agent to see that a window exists never implies allowing it to see what is inside.

What it enforces:

- **One named window, never the screen.** `PrintWindow(PW_RENDERFULLCONTENT)` against a specific `HWND` from the same enumeration, so windows stacked on top are not copied in. A full-screen grab has no scope any per-app allowlist could describe, which is why there is no such tool.
- **Ambiguity captures nothing.** The model names the window with `app` and/or `title_contains`; zero matches, or more than one, is refused with the candidate list so it can narrow down. Guessing would be an unrecoverable disclosure of the wrong window.
- **Windows outside the allowlist are not even named** in the refusal — it reports only how many allowed windows exist.
- **Two size gates, in this order**: the pixel count (4 000 000, so a 2560×1440 window fits) is checked before any bitmap is copied, and the encoded PNG is checked against the same 4 MiB per-image cap `workspace_read_image` uses. It also charges the same 16 MiB per-run and 12 MiB per-request image budgets, so a capture and a file read compete for one allowance.
- **Every attempt is an external action record** — `computer_capture`, `computer_capture_refused`, `computer_capture_failed`, `computer_capture_cancelled` — naming the app, the title and the pixel size on success. A screenshot cannot be taken back, so it is logged like a navigation, not like a read.

Not implemented, deliberately: **input injection**. Clicking and typing are irreversible and can dismiss any confirmation dialog; that needs an authority model narrower than "allow the desktop" and a record that can be replayed, so the shape is being proven on the read-only and capture surfaces first.

## Browser Use

Two further tools — `workspace_browser_open` and `workspace_browser_tabs` — drive the user's own Chrome over the DevTools Protocol. They are the first Agent capability whose effects the product **cannot undo**, so the guarantee offered is authority plus a record, not reversibility.

What the backend enforces:

- **Two independent gates.** `WorkspaceToolPermissions::allows_browser()` requires *both* the `allowBrowserUse` grant and a non-empty origin allowlist. An empty list is not read as "unconfigured, so allow everything" — that reading is exactly what looks like user approval after an incident. Neither gate alone advertises the tools, and `handles()` returns false, so a model that names the tool anyway gets "unknown tool" rather than a tool that always fails.
- **The gate is re-checked inside the tool**, but that particular branch is defence in depth for a direct caller only: `handles()` already drops a call to an unadvertised tool before dispatch (`select_external_calls` and `CompositeToolInvoker::invoke` both filter on it), so "browser use was off entirely" produces no external-action record. The *reachable* refusals are the ones decided inside an advertised tool — a rejected scheme or an origin outside the allowlist — and those are recorded.
- **Origin-scoped, not URL-scoped.** `services::browser::origin_of` reduces a URL to `scheme://host[:port]` with the authority lowercased, and `origin_allowed` compares case-insensitively against the list — exact match, or `*` for any. The list is captured when the run starts (`browserOrigins` in the request); see the limitations below for what that means for a continued pipeline.
- **Scheme allowlist before any network call.** `normalize_target_url` accepts only `http` and `https`, and rejects control characters, a missing host, and `user:pass@` credentials. `javascript:` would run script in the *current* page's origin, `file:` reads local files outside the workspace boundary, and `chrome:` reaches the browser's own settings — all three are refused before a request is made.
- **Loopback only.** The CDP endpoint is `http://127.0.0.1:{port}` (`AGENT_IDE_CDP_PORT`, default 9222). CDP has no authentication whatsoever: anything that can reach the port controls the browser, so the port is never taken from remote input.
- **Stop refuses side-effecting calls that have not started yet, and kills a command that has.** The run's cancel flag is the same `Arc<AtomicBool>` the tool surface holds (`WorkspaceToolPermissions::fresh_cancel()` mints it, `try_begin_run` publishes it), so `WorkspaceToolInvoker::invoke()` refuses `workspace_run_command`, the write / delete / move tools and both browser tools once Stop has been pressed. `workspace_run_command` also carries the flag into `run_project_command_cancellable`, which polls it while the child runs and kills the process tree (`taskkill /T /F` on Windows) instead of waiting the command out; the tool then reports "Stopped: … was killed", not an exit code, so the repair loop cannot mistake it for a passing check. Read-only tools still run — they change nothing, and refusing them would only add noise to a transcript that is being abandoned. A browser attempt refused this way is recorded as `browser_open_cancelled` / `browser_tabs_cancelled` and counted as *not performed*. What this does **not** do: an in-flight CDP request is still bounded only by the timeout below, a Unix command that forks past its shell can outlive the kill, and an MCP call that is already in flight cannot be interrupted — MCP has no cancellation in the protocol as we use it, so the gate only prevents new calls.
- **Every CDP request has a 10-second timeout.** These calls run inside `ToolInvoker::invoke`, which is synchronous. Without a timeout a port that accepts the connection and never answers parks the run on a blocked worker thread: Stop marks the UI idle, the backend task never finishes, and the external action log is never published — an irreversible capability losing the one record that compensates for it.
- **Every attempt is recorded, including the refused ones.** Successes, refusals and transport failures land in the run's action log as `browser_open` / `browser_open_refused` / `browser_open_failed` and `browser_tabs` / `browser_tabs_refused` / `browser_tabs_failed`, with the text "These cannot be undone." A refusal that only appears in the tool's return value disappears with the conversation, and "the model tried to open a site it was not allowed to" — or tried at all while the switch was off — is precisely what the user wants to find afterwards.
- **The record lives on the orchestrator, next to the diffs.** `record_external_actions` appends to `AgentOrchestrator.external_actions` (bounded at 200, newest kept) and `get_agent_external_actions` reads it back, so a frontend reload still shows what the run did outside the workspace (when a workspace path is saved — that is the bootstrap path). The action-log entry is the second copy, not the only one — an emit-only record was lost whenever no window was listening, since `take_external_actions` drains the tool-side list.
- **Each record names the run that performed it**, taken from `WorkspaceToolPermissions.run_id`, which the command layer stamps when the run lease is granted. It is deliberately not re-read at publish time: a run that was stopped can drain its records after the *next* prompt has started, and reading the orchestrator then files the old navigation under the new prompt.
- **The review area shows them without an Undo button.** The Diff view lists every record newest-first in a scroll box, headed "N browser action(s) — cannot be undone", refusals counted separately, and records belonging to an earlier run tagged as such. The absence of a button is the point; a control that pretended to reverse a navigation would be the worst possible answer.

What it does **not** do: there is no page interaction — no clicking, typing, form submission or script evaluation.

External actions are deliberately a **separate list from writes**. A file write carries its previous content and an undo checkpoint; a navigation carries neither. One list would let the word "undo" mean two things in the same UI, one of them false.

Limitations worth stating plainly, because the record is the whole compensating control:

- **Nothing about a run is written to disk by the backend.** The external actions live on the orchestrator and are re-readable while the app runs. That is weaker than a `FileDiff`, which the frontend also mirrors into localStorage and re-displays after a restart with a "no longer known to the backend" warning; external actions are simply absent after a restart. Restoring them client-side is the open item — they happened, so forgetting them is a loss.
- **A run that only navigated does badge the Changes tab**, in a different colour from a pending change: the count is of actions that already happened and cannot be undone, not of things waiting for a decision. The action-log entry is the other notification path.
- **A redirect leaves the authorized origin.** The allowlist is checked against the URL the model supplied; Chrome then follows redirects with no further check, so an open redirect or a shortener on an allowed host can land the browser elsewhere. The success record names the requested URL, not the final one.
- **`workspace_browser_tabs` results are not origin-scoped.** The allowlist decides whether the tool exists, not what it returns: allowlisting only a local dev server still discloses the title and URL of *every* open page — internal sites, ticket URLs, magic links, OAuth callbacks with tokens in the query string. The record names the count and the distinct origins disclosed so the exposure can be reconstructed.
- **A continued pipeline uses the authority captured at its start.** `continue_agent_pipeline` clones the permissions stored on the orchestrator, so revoking Browser Use in Settings does not affect a paused run that is then continued. The same is true of command execution. Both it and the auto-repair loop mint a fresh side-effect switch and rebuild their tool surface, so Stop applies to them like any other run.

Browser use is deliberately **not** part of the permission preset ladder. All three presets (`read-only`, `create-files`, `run-commands`) set `allowBrowserUse: false` and an empty origin list; choosing `run-commands` to let the Agent run tests must not silently also approve outbound navigation.

## MCP Tool Exposure

Model Context Protocol servers are the largest privilege surface in the product, and the one with the fewest backend guarantees. This section states plainly what is and is not enforced.

What the backend enforces:

- **Tool-name gating only.** `McpToolPolicy` decides which discovered tools are advertised to the model and re-checks the name at call time: `Deny` exposes nothing, `AutoApprovedOnly` exposes only tools the user listed in that server's `autoApprove`, `AllowAll` exposes everything. An unrecognized policy string falls back to the restrictive `AutoApprovedOnly`, and `autoApprove` defaults to empty.
- Tool arguments must be a JSON object.
- The server process is spawned with its `cwd` resolved inside the workspace.
- **Qualified tool names are unique.** Tool names are sanitized into `mcp__{server}__{tool}`, which is lossy — `.` and spaces both become `_`, and the separator is itself `__`, so two distinct servers can sanitize to the same advertised name. Discovery now claims names first-come and drops later collisions, reporting them on that server's status. Without this, the call-time lookup took the first match and a tool call could silently land on a different server than the model named.
- **Tool results are capped at 64,000 characters** before they re-enter the conversation, with the truncation stated in the result text. Non-text content parts (image/audio) are replaced by a placeholder rather than inlined as base64.


What the backend does **not** enforce:

- **Nothing about what a tool actually does.** Arguments are forwarded opaquely. There is no path validation, no workspace-boundary check, no read/write distinction, and the Agent-write deny list below does not apply. An MCP filesystem tool can write `.git/hooks/pre-commit` or read `~/.ssh/id_rsa`, and those effects never appear in the diff-review UI. What *is* enforced since the side-effect switch landed: after Stop, `McpToolInvoker::invoke` refuses to issue further calls and logs the refusal. A call already in flight cannot be cancelled.
- The MCP server command, arguments, and environment come from `mcp.json` and are executed without validation. Configuring an MCP server is equivalent to granting arbitrary code execution.
- A `cwd` is not a sandbox.

Consequences for the operator: treat adding an MCP server as equivalent to installing a plugin with full user privileges. Prefer `AutoApprovedOnly` and list tools explicitly. Under the `run-commands` permission preset the policy resolves to `AllowAll`, so every discovered tool is callable without a human in the loop.

`call_mcp_tool` intentionally uses `AllowAll`: it is a user-initiated escape hatch, not an Agent-initiated call. Note that no UI currently invokes it — `McpPanel.tsx` only calls `get_mcp_config`, `get_mcp_tools`, `save_mcp_config` and `discover_mcp_tools` — so today the permissive policy is reachable only over IPC.


## Agent Write Deny List

Beyond the workspace boundary, Agent-generated diffs pass through `workspace::resolve_for_agent_write`, which rejects paths where a write would be equivalent to code execution or credential tampering:

- Directory components anywhere in the path: `.git`, `.agent-ide`, `node_modules`
- Credential filenames: `.env`, `.env.*`, `.npmrc`, `.netrc`, `id_rsa`, `id_ed25519`
- Credential extensions: `.pem`, `.key`, `.p12`, `.pfx`

Matching is case-insensitive and applies only to the path *relative* to the workspace root, so a workspace that happens to live under a directory named `node_modules` still works. Without this rule an Agent diff targeting `.git/hooks/pre-commit` would be applied — in `auto` mode with no human click — yielding arbitrary code execution on the next commit.

This rule deliberately constrains **only Agent-generated diffs**. Editing `.env` yourself from the file explorer goes through `resolve_for_write` and is allowed, because that is an explicit user action.

## Egress Constraints on Credential Files

The write deny list governs what lands on disk; it says nothing about what leaves the machine. A matching egress rule (`workspace::is_credential_path`, sharing the credential filename rules above but excluding `.git` / `node_modules`) applies to prompt construction:

- If the active file looks like a credential file, its contents and the current selection are withheld from the prompt. The path is still disclosed so the model knows what is open.
- `build_git_diff_summary` drops hunks belonging to credential files and names which files it withheld.

Not covered: a credential file passed explicitly as a context file, or read by an MCP tool, is not filtered.

## Per-Run Cost Controls

- `maxRunTokens` on the LLM profile caps total provider-reported tokens for one run. It is enforced in `send_chat_request`, the single choke point all provider requests pass through, so it cannot be bypassed by using a different entry point. A configured `0` is treated as unset.
- The meter is stored on the orchestrator, so resuming a paused pipeline continues against the same allowance instead of restarting the count.
- `usage_is_unknown()` distinguishes "the provider reported no usage" from "nothing was spent". Local runtimes and mock endpoints report no usage, so a cap cannot be enforced against them; this is surfaced in the run's action log rather than being reported as zero cost.
- `maxRunSpendMicros` caps the run in money rather than tokens, checked in the same `check_budget` choke point and evaluated **before** the token cap, so an expensive model stops on cost even when the token count looks modest. Amounts are integer micro-USD (1 USD = 1_000_000) and each call's cost is rounded up, so a run cannot accumulate spend that rounds to zero.
- Spend needs both `promptMicrosPerMillion` and `completionMicrosPerMillion` on the profile. With only one of them the estimate would systematically undercount, so pricing is treated as absent: the action log reports `not computable (no pricing configured)` and the spend cap is **not** enforced. An undercounting cap is worse than a missing one because the user believes they are protected.
- The three spend fields are editable in Settings → Per-Run Spend Cap. The UI takes dollars and converts to integer micro-USD by string parsing (`utils/money.ts`), because `Number("0.29") * 1e6` is `289999.99999999994` and this value decides when a run is cut off. Creating a new profile clears them rather than inheriting them, so a cap is never enforced using the previous model's prices.
- The tool loop is bounded at 12 rounds per stage (`MAX_TOOL_ITERATIONS`, `agent/executor.rs`).



## Agent Approval Model

Two independent settings control what an Agent run may do, and they are easy to
confuse because their values share names. Both are visible in the UI, in
different places.

- **Agent mode** (`AgentMode`: `suggest` | `auto`) — decides whether the
  Agent's diffs reach disk without a click, and whether the direct write tool is
  advertised. Set from the mode switch in the top bar.
- **Permission preset** (`AgentPermissionPreset`: `read-only` | `create-files` | `run-commands`) — a
  shortcut that sets the two fine-grained toggles (`allowFileCreate`,
  `allowCommandRun`). Set in
  Settings → Agent Permissions, where the toggles can also be changed one by one.

The preset values are deliberately named after what they grant. They used to be
`ask` / `suggest` / `auto`, which collided with the mode's values while meaning
something different — choosing the `suggest` preset did not put the run in
`suggest` mode. Two axes have to coexist, so the names now carry the distinction
instead of a paragraph explaining a trap.

The Agent mode is not persisted by the backend — it resets to `suggest` on every
launch. The frontend session snapshot in `localStorage` remembers the last
selection and normalizes anything it does not recognize back to `suggest`, so a
stale value cannot restore a privilege level the user cannot see.

The Agent operates in a "suggest-then-apply" pattern with two modes:

| Mode | Behavior |
|------|----------|
| `suggest` | Produces reviewable diffs only. User must explicitly apply. |
| `auto` | Applies pending diffs automatically after the pipeline run completes. |

Two modes, not three, because there is exactly one gate: every check in the
backend is written as `matches!(mode, AgentMode::Auto)`. An earlier `edit`
position sat between them and was byte-identical to `suggest` — the switch
offered three positions but granted two levels, and a user reasonably reads a
middle setting as "more than suggest, less than auto". It was removed rather
than given a meaning, because every candidate meaning ("may edit existing files
but not create", "may write during the run but not auto-apply") is already
expressed more precisely by the permission toggles below.

Which permission toggles are actually enforced in the backend:

- `allowFileCreate` — enforced twice. In `auto` mode, new-file diffs are held for review instead of being written when it is false, and `workspace_write_file` refuses to create a file that does not exist.
- `toolApproval` (derived from `allowCommandRun`) — enforced, as MCP tool-name gating only. This one follows the **preset**, not the mode: the `read-only` and `create-files` presets resolve to `AutoApprovedOnly`; the `run-commands` preset resolves to `AllowAll`.
- `allowCommandRun` — enforced. When false, the `workspace_run_command` tool is **not advertised to the model and not claimed by the invoker**, so there is no Agent path to process execution other than MCP tools. When true, the exposed allow-list is derived by the backend from the project's own declared tasks (`package.json` scripts, Cargo), never from model input, and long-running commands (dev servers, watch tasks) are refused regardless of the list. Every call is written to the action log.
- **Agent mode gates direct writes.** `workspace_write_file` is advertised only when the run is in `auto` mode. This is not a new privilege level: `auto` already applies pending diffs without a click, so writing during the run grants nothing it did not already have. In `suggest` the tool is absent and the model must emit reviewable diffs, which is what that mode promises. Writes still go through `workspace::resolve_for_agent_write`, so `.git/`, `.agent-ide/`, `node_modules/` and credential files are refused on this path too.
- **`workspace_delete_file` shares that gate and that boundary.** Advertised under the same `allow_write` condition, deliberately not a separate toggle: a whole-file overwrite can already destroy the contents, so a second switch would only imply that writing is safer than it is. Paths resolve through `resolve_for_agent_write`, so the deny list above applies identically. **Directories are refused** — recursive deletion has a different blast radius, and the undo record is a list of file→content pairs, which cannot rebuild a tree. The tool reads the file before removing it and fails if it cannot, because without those bytes the deletion would not be undoable. The review entry is labelled `delete` rather than shown as an emptied file, and undo recreates the file with its exact previous contents.
- **`workspace_move_file` needs both `allow_write` and `allow_create`, and never overwrites.** Moving makes a file disappear from one path (a write) and appear at a path that did not exist (a create), so it is advertised only when both grants are present and the invoker does not claim it otherwise. **Both** paths resolve through `resolve_for_agent_write`, so a credential file or anything under `.git/` can be neither the source nor the destination. An existing destination is always an error — no exception for names differing only in case, because case sensitivity is a per-directory property (Windows `setCaseSensitiveInfo`, case-sensitive APFS) and an exception there would silently replace a real file whose bytes no record holds. A case-only rename therefore needs an intermediate name, and the error message says so. Directories are refused, as with deletion. The move itself is a single `fs::rename`, so the file is never in two places or in neither; a destination directory created for the move is removed again if the rename then fails. The tool never reads the content, so binary files move as well as text. Undo is a move back rather than a content rewrite, so it restores the file itself and not just its bytes; if the file was edited after the move, undo also writes back the content as of the move. **Undo replays a batch in reverse order** — the inverse of a sequence of renames is the reversed sequence of inverse renames, and replaying forwards can destroy a file (move `A`→`B`, then create a new `A`: forwards, the move-back overwrites the new `A` and the create's undo then deletes it, losing the original).
- **The bounded repair loop (`repair_workspace`) is gated the same way, for the same reason.** Every iteration has to reach the workspace or the re-run checks the old code, so the command is refused outside `auto` mode instead of being downgraded to a single round. Its fixes go through the normal diff application path (`apply_all_diffs`), so each round keeps its base-hash staleness check and its undo point; the iteration budget is clamped to 3, and every iteration plus the stop reason is written to the action log.

- Every tool write is recorded as an `applied` diff entry in the review area with the pre-write content, and is covered by an undo checkpoint, so `Undo Apply` restores it. A write that would bypass the review area entirely is the failure mode this avoids: the file changes and the user has no way to see what changed.
- There used to be `allowFileDelete` and `allowGitActions` toggles here, documented as "not enforced", and they were removed from the UI because an unenforced permission toggle is worse than an absent one. Deletion **is** now a real Agent capability (`workspace_delete_file`, 2026-09-13), and it is gated by `allow_write` rather than by a toggle of its own — see the reasoning above. Git actions remain outside Agent runs, so nothing guards them because nothing needs guarding.
- Permissions are captured as a per-run snapshot when the run starts. Narrowing a permission mid-run does not revoke a tool that was already advertised for that run; stop the run instead.
- In the CLI, command execution is gated by `--allow-run` patterns instead. Both entry points now share one matcher (`services::verification::is_command_allowed`), so what counts as authorized cannot drift between them. **Read-only workspace tools are attached unconditionally**: reading files inside the workspace is not a privilege, it is the subject of the task, and without it the model has to guess the `original` block of any edit it proposes — a real DeepSeek run did exactly that and produced diffs that could not apply. `--allow-run` still decides whether `workspace_run_command` exists, and `--allow-edit` / `--allow-create` are deliberately *not* reinterpreted as write-tool grants — they govern whether produced diffs may be applied, and treating them as "the model may write files directly" would be a silent privilege escalation.

- The CLI's write tool has its own flag: `--allow-agent-write`, which requires `--apply`. That requirement mirrors the desktop's Auto-mode gate rather than repeating it: a preview run must leave the workspace untouched, and a tool that writes during a preview would break the only promise preview makes. `--allow-create` extends the grant to new files only when `--allow-agent-write` is also given, so neither flag alone can create anything. Every write lands in `tool-writes.json` under the run's artifact directory — the CLI has no review area, so that file is the only record of which files the model touched. It is written whenever the flag is on, even as an empty array, because "not authorized" and "authorized and never used" are different facts and a missing file conflates them.

- `McpToolPolicy::Deny` exists but no preset currently produces it, so there is no way to run with MCP tools fully disabled short of removing the servers from `mcp.json`.

Limits of the command and write tools, stated plainly:

- A permitted command is still arbitrary code execution by whatever the project declares. `npm test` runs the project's test script, which can do anything. The boundary is "commands this project already defines", not "commands that are safe".
- Commands run in the workspace root with the inherited environment. There is no network, filesystem, or environment isolation.
- Command output is truncated to 12,000 characters (tail kept, since failures land at the end) before reaching the model.
- `workspace_write_file` replaces the whole file. A model that writes without reading first can drop content it never saw. The tool description says so, and the pre-write content is kept for undo, but nothing prevents it.



- **Batch**: Apply All / Reject All
- **Per-file**: Apply or reject individual file diffs
- **Per-hunk**: Apply or reject individual hunks within a file diff
- **Undo**: `Undo Apply` restores every file touched by the most recent apply to its pre-apply content, up to 20 levels. Files that did not exist before are deleted rather than left empty. The affected diffs return to `pending` with a re-stamped `baseHash`, so they re-enter the review queue and can be applied again. Snapshots live only in backend memory — they are not persisted and do not survive an app restart, and they are never sent over IPC.

Safety mechanisms during diff application:

- Outside-workspace paths are rejected.
- Missing original content (empty hunks on edit diffs) is rejected.
- Ambiguous original matches (hunk text appears more than once) are rejected — the file is not modified.
- New-file hunks that would overwrite an existing file are rejected.
- Mixed new-file and edit hunks in the same diff are rejected.
- Optional `baseHash` validation rejects stale edit diffs if the file content hash no longer matches the hash recorded when the diff was generated.
- Partial-apply failures are reported structurally: `ApplyDiffsResult { applied, failed }` — each failed diff includes the diff ID, file path, and error message. The failed file content is not modified.
- Failed hunks within a multi-hunk diff prevent the entire file from being written (atomic per file).

Diff provenance tracks:

- Protocol (`agent-changes` or legacy markdown diff)
- Operation (edit or create)
- Schema version
- Change index within the model output
- Rationale for the change
- Source role and source stage (e.g., `coder` / `Coder`)
- Regeneration chain (`regeneratedFromDiffId`, `regeneratedFromHunkIndex`)

## Data Exposure Constraints

What is sent to the configured LLM provider, and only to it:

- `project_memory` — `AGENTS.md` verbatim, capped at 8000 characters
- `active_file_content` — the full file in `Full` mode, a 24 000 / 16 000 character excerpt in `Budgeted` / `Focused`, an outline in `Compact`
- `selection` — verbatim
- `git_diff` — working-tree patch text, capped at 24 000 characters
- `project_tree` — up to 160 entries, depth 4
- `open_files` — paths only
- `ide_runtime` — the IDE's current state as the user chose to attach it: up to 20 problems, 4 000 characters of terminal output, 8 warn/error log lines, and 4 000 characters from a failed check command. It used to be concatenated onto the prompt by the frontend, which meant it was sent but never counted; it is now a context section like the others, so it appears in the estimate panel and is subject to budget trimming.

**Every context source is on by default.** The per-run toggles let the user turn each off, but the shipped default sends all of them.

Filters that apply:

- Credential files are withheld as described in the egress section above.
- The tree listing skips `.git`, `node_modules`, `target`, `dist`, `.DS_Store`, `Cargo.lock`, `package-lock.json`. This filter applies to the listing only, not to file contents.

Other guarantees:

- No telemetry, analytics, crash reporting, or phone-home of any kind. `reqwest` is used only for LLM requests.
- The two hardcoded URLs (`https://api.openai.com/v1`, `https://api.deepseek.com/v1`) are overridable defaults, not fixed destinations. There is no scheme or host allow-list on the configured endpoint.

- Git remote URLs come from the repository's own config, not from the app.
- Agent output is never rendered as HTML: `ReactMarkdown skipHtml` plus `sanitizeMarkdown` before rendering.
- API keys are masked in IPC responses, action logs, and the UI. The exception is `reveal_llm_api_key`. MCP tool arguments are redacted by key name only, and MCP tool results are not redacted at all — see the credential section.

## Terminal Security

- Terminal cwd is scoped to the workspace root.
- Commands run with the user's local permissions (no elevation or privilege escalation).
- PTY lifecycle is managed by the Rust backend: `spawn_terminal`, `write_to_terminal`, `resize_terminal`, `kill_terminal`.
- Kill terminates the PTY cleanly by signaling the reader loop.
- Windows `\\?\` verbatim path prefixes are stripped before passing cwd to `cmd.exe` (which rejects UNC paths).
- Multi-session UI supports session tabs, new/close/restart, but all sessions are bounded by the workspace root.
- Browser preview mode shows a disabled-state message instead of attempting PTY access.

## Git Operation Safety

- All Git operations resolve paths through the workspace service and use `git2::Repository::discover` to locate the repository from the workspace path.
- Available operations: status, staged/worktree/all diff, stage/unstage/discard, commit, branch checkout/create, remote branch checkout/tracking, fetch, fast-forward-only pull, push, upstream/ahead/behind display, conflict detection, and conflict resolution (accept current/incoming/both).
- One-shot HTTPS credential inputs for remote actions: credentials are prompted once per operation and are not persisted by default.
- Optional OS-stored HTTPS credentials via `credentials::git_credential_ref` and `credentials::store_secret` — when the user opts in, the remote token is stored in the OS credential store and reused for future remote operations.
- Force-push and destructive operations (discard, revert, reset) are available but should require explicit confirmation (confirmation UX for destructive actions is still being improved).
- Conflict resolution is presented in the UI before any auto-resolution is applied.

## CLI Permission Model

The Agent CLI (`agent_cli`) is scoped as a headless automation runner. Security controls:

- Workspace boundary: shared `workspace::resolve_existing` and `workspace::resolve_for_write` checks.
- `--allow-run` authorization: repair loops require explicit `--allow-run <pattern>` for each command that will be re-executed. Patterns support:
  - Exact match: `npm test`
  - Prefix wildcard: `cargo *`
  - Trusted all: `*`
- `--max-iterations`: bounds the number of repair loop iterations.
- `--timeout-seconds`: bounds how long a single command or the overall run can take.
- `--max-output-bytes`: limits captured command output size.
- `--max-diff-files`: limits the number of files in a single Agent diff proposal.
- Stable exit codes: `0` success, `1` internal error, `2` invalid input, `3` changes proposed, `4` checks failed, `5` apply failed, `6` provider failed, `7` precondition failed, `8` cancelled.

Known gaps:

- The Agent write deny list is shared with the desktop app, because both go through `diff_apply`. On top of that the CLI enforces `--deny-path`: any generated diff whose path matches a pattern is refused with `ExitCode::PreconditionFailed` (`cli/mod.rs`), before anything is written.
- Operation-level flags exist for create, edit and delete (`--allow-create` / `--allow-edit` / `--allow-delete`), but only delete is independent. `--apply` itself sets create and edit (`allow_create: args.allow_create || args.apply`, same for edit), so **"edits but no new files" is not expressible** on the CLI. The whole permission check also only runs on the apply path, so without `--apply` these flags have no effect in either direction.
- `--allow-git` is accepted and logged but enforces nothing.

- MCP tools are not exposed to the CLI at all today.

## Known Limitations

Ordered by how much they would matter to an operator. Each was confirmed by reading the code, not inferred.

1. **MCP tools are unconstrained.** They bypass the workspace boundary, the Agent write deny list, and the diff-review UI entirely. Adding an MCP server is equivalent to granting arbitrary code execution. This is the single largest gap.
2. **A plaintext API key can persist in `~/.agent-ide/config.json`** if keyring migration ever fails, and it is then preferred over the keyring entry. No file-permission hardening.
3. **Repository-wide Git write operations escape the workspace boundary** when the workspace root is a subdirectory of a larger repository, because `Repository::discover` walks upward. Affects `checkout_head` and `git_commit` with no file list. The git-diff context section is now pathspec-scoped and no longer affected.
4. **Workspace-local language server binaries are executed in preference to `PATH`**, so opening an untrusted repository runs code it supplies.
5. **MCP argument redaction keys on field names**, so a secret passed under a key that does not look secret is still written to the action log. Tool *results* are logged without redaction.
6. **Broad `fs:allow-read` / `fs:allow-write` / `fs:allow-mkdir`** remain in `capabilities/default.json`. The unscoped `shell:allow-spawn` and `shell:allow-execute` have been removed — no first-party frontend code imports `plugin-shell`, so they were pure attack surface. `shell:allow-open` is retained for opening external links.
7. **`run_project_command` passes the command string to `cmd /C` or `sh -lc`** with no allow-list and no escaping. Two callers reach it: the GUI task runner and `verify_workspace`, both driven by commands the user supplies. The CLI additionally runs commands taken from the workspace's own `package.json` scripts autonomously.
8. **Recursive traversal follows symlinks.** `search_recursive` and `copy_dir_recursive` re-check nothing per entry.
9. **`resolve_for_write` does not canonicalize the final component** when the target does not exist, so a symlink created between check and write is not caught (TOCTOU). Not tested.
10. **Cancellation is cooperative** — a shared `AtomicBool` checked in the request and streaming paths. There is no transport-level abort.
11. **A token cap cannot be enforced against providers that report no usage** (local runtimes, mock endpoints). This is surfaced rather than silently treated as zero.
12. **Hunk matching is textual**, not AST-aware. Ambiguous matches are rejected rather than guessed, and `baseHash` catches stale edits, but line-offset tolerance is not implemented.
13. **macOS and Linux credential backends are unvalidated at runtime.** Windows is verified end to end. Linux and macOS CI jobs now attempt the round trip — Linux under `dbus-run-session` with `gnome-keyring`, macOS against the login Keychain — but **neither result has been confirmed yet**. Linux's first run failed before reaching the tests, on an unrelated RGBA icon problem that has since been fixed, so both stay listed as unvalidated until a run gets that far.
14. **`call_mcp_tool` has no UI caller.** It is registered and uses `AllowAll` by design, but nothing in the frontend invokes it, so the permissive policy is reachable only over IPC. Either wire it to a user-initiated action or drop it — a permissive command with no visible entry point is the kind of thing that gets forgotten.


## Vulnerability Reporting

If you discover a security vulnerability, please report it by opening a private issue or contacting the maintainers directly. Do not disclose vulnerabilities publicly before a fix is available.

Include as much of the following as possible:

- Description of the vulnerability and its impact
- Steps to reproduce
- Affected versions
- Any proposed mitigations

Security-related issues will be prioritized for review and resolution.
