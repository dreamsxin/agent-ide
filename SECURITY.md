# Agent IDE Security Policy

## Overview

Agent IDE prioritizes user control and data safety. The Agent is visible, auditable, and requires user approval before applying changes. All filesystem, Git, terminal, and Agent operations are bounded by the workspace root.

## Workspace Boundary Model

All filesystem operations (read/write/delete) are scoped to the open workspace root. The centralized workspace service enforces this boundary:

- **`workspace::resolve_existing`** — resolves read paths after canonicalization and rejects any path outside the workspace root. Also rejects relative traversal (`../../etc/passwd`) by canonicalizing the candidate and checking the prefix.
- **`workspace::resolve_for_write`** — resolves write paths, including new files that do not yet exist, by validating the nearest existing ancestor is within the workspace. Rejects writes outside the workspace boundary.
- **`workspace::ensure_within_workspace`** — lower-level guard used by both resolve functions. Compares the canonical path prefix against the workspace root.
- **`workspace::shell_compatible_path`** — normalizes Windows verbatim `\\?\D:\...` and `\\?\UNC\...` prefixes into shell-compatible paths, preventing canonicalization mismatches on Windows.

Surfaces that enforce the workspace boundary:

- Filesystem commands (`read_file_content`, `write_file_content`, file tree operations)
- Agent-generated diffs (`apply_pending_diffs` resolves each target through `resolve_for_write`)
- Git operations (status, diff, commit, branch, fetch, pull, push — all resolve through workspace service)
- Terminal cwd (spawned PTY sessions start in the workspace root; Windows `\\?\` prefixes are stripped before passing to `cmd.exe`)
- Project task cwd (non-interactive command runner inherits workspace-scoped cwd)
- Agent CLI (shared workspace resolution and boundary checks)

Path traversal protection:

- Absolute paths are checked against the workspace root after canonicalization.
- Relative paths are joined against the workspace root first, then canonicalized and checked.
- Windows verbatim path prefixes (`\\?\`) are normalized centrally to prevent canonicalization bypasses.

Known gap: symlink traversal is not explicitly checked. Symlinks pointing outside the workspace would pass the canonical-path prefix check if the symlink target resolves inside the workspace, and would be rejected if it resolves outside. This has not been runtime-validated across all platforms.

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
- Local profile configuration JSON (`~/.agent-ide/llm-profiles.json`) stores `credentialRef` strings, not plaintext API keys.
- The `api_key` field in `LlmProfile` is marked `skip_serializing` so it is never written to disk through the profile config.
- Frontend responses use `api_key_masked` (e.g., `sk-...abc`) to avoid exposing keys in IPC responses.
- No keys are transmitted to any service other than the configured LLM provider endpoint.
- `delete_secret` silently succeeds even if the credential is missing, to support cleanup flows.

Known limitations:

- Cross-OS runtime validation is pending. The `keyring` crate behavior has not been verified on all supported platforms.
- Recovery UX for inaccessible or missing credentials is minimal — the error message indicates the credential store is unavailable, but there is no guided recovery flow yet.

## Agent Approval Model

The Agent operates in a "suggest-then-apply" pattern with three modes:

| Mode | Behavior |
|------|----------|
| `suggest` | Produces reviewable diffs only. User must explicitly apply. |
| `edit` | Produces reviewable diffs. User must explicitly apply. |
| `auto` | Applies pending diffs automatically after the pipeline run completes. |

Diff review controls:

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

- Agent context includes only workspace files, user-selected sources, and local IDE state (open files, Problems, terminal output, logs, git diff, project tree summary).
- No telemetry or data collection is performed.
- Agent-generated HTML is never rendered directly: `ReactMarkdown skipHtml` is used in `ChatView`, and markdown rendering skips HTML tags.
- The `sanitizeMarkdown` function is applied before rendering Agent output.
- Action logs capture provenance (prompt, context summary, stage outputs, diff summaries, apply results) but do not leak credentials.
- LLM API key masking ensures keys never appear in IPC responses, action logs, or the UI.
- Context source choices are explicit per run: users can toggle active file, selection, open files, Problems, failed runs, terminal output, logs, git diff, and project tree independently.
- Backend context enrichment respects the user's per-run source toggles.

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

- The deny-path model (explicit path exclusions) is not yet implemented.
- Operation-level restrictions (e.g., "allow edits but not file creation") are partially implemented.
- CLI permission model should be broadened only if the CLI scope is intentionally widened beyond headless automation.

## Known Limitations (to be addressed)

1. **Cancellation is cooperative** — `stop_agent` sets a shared `AtomicBool` flag checked in the LLM request and streaming read path via `tokio::select!`. There is no explicit provider-side cancellation API or transport-level abort.
2. **Symlink traversal** — symlink targets are not explicitly validated. The canonical-path prefix check provides implicit protection, but this has not been runtime-validated across platforms.
3. **Secret storage cross-OS validation** — the `keyring` crate behavior has not been verified on all supported OS credential backends. Recovery UX for inaccessible credentials is minimal.
4. **Workspace boundary enforcement** — requires ongoing review as new command surfaces are added. All current entry points (FS, Agent diffs, Git, terminal cwd, task cwd, CLI) are guarded.
5. **CLI permission model** — deny-path exclusions and granular operation restrictions are partially implemented.
6. **Capabilities configuration** — the current `default.json` grants `fs:allow-read`, `fs:allow-write`, `fs:allow-mkdir`, `shell:allow-spawn`, and `shell:allow-execute` broadly. Fine-grained scope restrictions would limit the impact of any WebView compromise.
7. **Diff hunk text matching** — hunk application depends on exact or trimmed textual matching. Ambiguous matches are rejected, but there is no structural/AST-aware matching yet. Mixed applied/rejected hunk state within a single file needs clearer partial-status semantics.
8. **Tauri capabilities** — `shell:allow-execute` and `shell:allow-spawn` are broadly permitted in the capabilities config. Restricting these to specific commands would reduce the attack surface.

## Vulnerability Reporting

If you discover a security vulnerability, please report it by opening a private issue or contacting the maintainers directly. Do not disclose vulnerabilities publicly before a fix is available.

Include as much of the following as possible:

- Description of the vulnerability and its impact
- Steps to reproduce
- Affected versions
- Any proposed mitigations

Security-related issues will be prioritized for review and resolution.
