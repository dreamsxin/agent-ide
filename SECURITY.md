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

The Agent has three built-in read-only tools — `workspace_read_file`, `workspace_search_text`, `workspace_list_files` — so it can decide what to read instead of relying only on the pre-assembled context bundle. Unlike MCP tools, these are constrained:

- Every path goes through `resolve_existing`, so reads are confined to the workspace.
- Credential files are refused outright, matching the context egress rule. Without that they would be a bypass: the model could simply call the read tool to get the `.env` contents the prompt builder withholds.
- Traversal skips `.git`, `node_modules`, `target`, `dist`, `build`, `.agent-ide`.
- Output is capped (64 KB per file read, 60 search hits, 200 directory entries).
- They cannot write, delete, move, or execute anything.

They are advertised when the profile's `toolCallMode` is `native_tools`, which is the default for cloud profiles. If the endpoint rejects a `tools` parameter the client drops it, retries once, and writes a `tool_capability_degraded` warning to the action log — so a run without tools is visible rather than silent. The tool loop is bounded at 12 rounds per stage, with the per-run token cap as the real cost limit.

## MCP Tool Exposure

Model Context Protocol servers are the largest privilege surface in the product, and the one with the fewest backend guarantees. This section states plainly what is and is not enforced.

What the backend enforces:

- **Tool-name gating only.** `McpToolPolicy` decides which discovered tools are advertised to the model and re-checks the name at call time: `Deny` exposes nothing, `AutoApprovedOnly` exposes only tools the user listed in that server's `autoApprove`, `AllowAll` exposes everything. An unrecognized policy string falls back to the restrictive `AutoApprovedOnly`, and `autoApprove` defaults to empty.
- Tool arguments must be a JSON object.
- The server process is spawned with its `cwd` resolved inside the workspace.

What the backend does **not** enforce:

- **Nothing about what a tool actually does.** Arguments are forwarded opaquely. There is no path validation, no workspace-boundary check, no read/write distinction, and the Agent-write deny list below does not apply. An MCP filesystem tool can write `.git/hooks/pre-commit` or read `~/.ssh/id_rsa`, and those effects never appear in the diff-review UI.
- The MCP server command, arguments, and environment come from `mcp.json` and are executed without validation. Configuring an MCP server is equivalent to granting arbitrary code execution.
- A `cwd` is not a sandbox.

Consequences for the operator: treat adding an MCP server as equivalent to installing a plugin with full user privileges. Prefer `AutoApprovedOnly` and list tools explicitly. In `auto` mode the permission preset resolves to `AllowAll`, so every discovered tool is callable without a human in the loop.

`call_mcp_tool` intentionally uses `AllowAll`: it backs the Settings "test this server" button and is user-initiated, not Agent-initiated.

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
- The tool loop is bounded at 5 rounds per stage (`MAX_TOOL_ITERATIONS`).


## Agent Approval Model

The Agent operates in a "suggest-then-apply" pattern with three modes:

| Mode | Behavior |
|------|----------|
| `suggest` | Produces reviewable diffs only. User must explicitly apply. |
| `edit` | Produces reviewable diffs. User must explicitly apply. |
| `auto` | Applies pending diffs automatically after the pipeline run completes. |

Which permission toggles are actually enforced in the backend:

- `allowFileCreate` — enforced. In `auto` mode, new-file diffs are held for review instead of being written when it is false.
- `toolApproval` (derived from `allowCommandRun`) — enforced, as MCP tool-name gating only. `ask` / `suggest` resolve to `AutoApprovedOnly`; `auto` resolves to `AllowAll`.
- `allowFileDelete`, `allowGitActions` — **not enforced, because no Agent-reachable backend path performs those operations today.** Adding checks for them would be theatre until such a path exists. Agent runs never invoke Git commands, and diff application never deletes files.
- `allowCommandRun` has no effect on process execution in the desktop app; the Agent cannot spawn processes except through MCP tools. In the CLI, command execution is instead gated by `--allow-run` patterns.
- `McpToolPolicy::Deny` exists but no preset currently produces it, so there is no way to run with MCP tools fully disabled short of removing the servers from `mcp.json`.



- **Batch**: Apply All / Reject All
- **Per-file**: Apply or reject individual file diffs
- **Per-hunk**: Apply or reject individual hunks within a file diff

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

**Every context source is on by default.** The per-run toggles let the user turn each off, but the shipped default sends all of them.

Filters that apply:

- Credential files are withheld as described in the egress section above.
- The tree listing skips `.git`, `node_modules`, `target`, `dist`, `.DS_Store`, `Cargo.lock`, `package-lock.json`. This filter applies to the listing only, not to file contents.

Other guarantees:

- No telemetry, analytics, crash reporting, or phone-home of any kind. `reqwest` is used only for LLM requests.
- The three hardcoded URLs (`https://api.openai.com/v1`, `https://api.deepseek.com/v1`) are overridable defaults, not fixed destinations. There is no scheme or host allow-list on the configured endpoint.
- Git remote URLs come from the repository's own config, not from the app.
- Agent output is never rendered as HTML: `ReactMarkdown skipHtml` plus `sanitizeMarkdown` before rendering.
- API keys are masked in IPC responses, action logs, and the UI. The exception is `reveal_llm_api_key`, and MCP tool arguments, which are logged unredacted — see the credential section.

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

- The Agent write deny list is shared with the desktop app, because both go through `diff_apply`. A CLI-specific deny-path option is not implemented.
- Operation-level restrictions (e.g., "allow edits but not file creation") are partially implemented.
- MCP tools are not exposed to the CLI at all today.

## Known Limitations

Ordered by how much they would matter to an operator. Each was confirmed by reading the code, not inferred.

1. **MCP tools are unconstrained.** They bypass the workspace boundary, the Agent write deny list, and the diff-review UI entirely. Adding an MCP server is equivalent to granting arbitrary code execution. This is the single largest gap.
2. **A plaintext API key can persist in `~/.agent-ide/config.json`** if keyring migration ever fails, and it is then preferred over the keyring entry. No file-permission hardening.
3. **Repository-wide Git write operations escape the workspace boundary** when the workspace root is a subdirectory of a larger repository, because `Repository::discover` walks upward. Affects `checkout_head` and `git_commit` with no file list. The git-diff context section is now pathspec-scoped and no longer affected.
4. **Workspace-local language server binaries are executed in preference to `PATH`**, so opening an untrusted repository runs code it supplies.
5. **MCP argument redaction keys on field names**, so a secret passed under a key that does not look secret is still written to the action log. Tool *results* are logged without redaction.
6. **Broad `fs:allow-read` / `fs:allow-write` / `fs:allow-mkdir`** remain in `capabilities/default.json`. The unscoped `shell:allow-spawn` and `shell:allow-execute` have been removed — no first-party frontend code imports `plugin-shell`, so they were pure attack surface. `shell:allow-open` is retained for opening external links.
7. **`run_project_command` passes the command string to `cmd /C` or `sh -lc`** with no allow-list and no escaping. It is user-initiated in the GUI, but the CLI runs commands from the workspace's own `package.json` scripts autonomously.
8. **Recursive traversal follows symlinks.** `search_recursive` and `copy_dir_recursive` re-check nothing per entry.
9. **`resolve_for_write` does not canonicalize the final component** when the target does not exist, so a symlink created between check and write is not caught (TOCTOU). Not tested.
10. **Cancellation is cooperative** — a shared `AtomicBool` checked in the request and streaming paths. There is no transport-level abort.
11. **A token cap cannot be enforced against providers that report no usage** (local runtimes, mock endpoints). This is surfaced rather than silently treated as zero.
12. **Hunk matching is textual**, not AST-aware. Ambiguous matches are rejected rather than guessed, and `baseHash` catches stale edits, but line-offset tolerance is not implemented.
13. **macOS and Linux credential backends are unvalidated at runtime.** Windows is verified end to end.
14. **The CLI deny-path model is not implemented**; the Agent write deny list covers the desktop diff path only, and both share it via `diff_apply`.

## Vulnerability Reporting

If you discover a security vulnerability, please report it by opening a private issue or contacting the maintainers directly. Do not disclose vulnerabilities publicly before a fix is available.

Include as much of the following as possible:

- Description of the vulnerability and its impact
- Steps to reproduce
- Affected versions
- Any proposed mitigations

Security-related issues will be prioritized for review and resolution.
