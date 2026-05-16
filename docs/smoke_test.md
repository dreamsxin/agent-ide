# Agent IDE Smoke Test Checklist

> This checklist is for validating that Agent IDE can keep replacing daily IDE workflows in the real Tauri runtime.
> Run it after changes to LSP, Problems, Terminal, Git, file operations, or Agent diff application.

## 1. Baseline Verification

Automated checks:

```powershell
npm run build
npm test
cd src-tauri
cargo check
cargo test
```

Expected:

- Frontend build succeeds.
- Vitest parser/path tests pass.
- Rust checks and unit tests pass.
- Vite may warn about a large chunk; this is not a smoke failure.

## 2. Runtime Mode

Manual check in the real desktop runtime:

```powershell
npm run tauri -- dev
```

Expected:

- App boots without console crashes.
- Previously opened workspace is restored, or Open Folder can select a workspace.
- Tauri-only panels are enabled in desktop runtime.
- `npm run dev` remains browser preview only and does not expose backend-only features.

## 3. TypeScript LSP Status

Manual check:

1. Open a workspace that contains `package.json`.
2. Open a `.ts`, `.tsx`, `.js`, or `.jsx` file.
3. Click the `TS ready/unavailable/error` status in the TopBar.

Expected:

- Status becomes `ready` when `typescript-language-server` is available.
- Details show server path, workspace root, opened documents, changes, diagnostics count, and last error when present.
- Recent diagnostics show per-file `error/warning/info` counts after diagnostics are published.
- If the server is unavailable, the status explains how to install it.

Automated coverage:

- Rust tests cover LSP file URI encoding/decoding and Windows verbatim path normalization.
- Frontend tests cover file URI/path normalization.

## 4. Diagnostics to Problems to Editor Markers

Manual check:

1. Open a TypeScript or JavaScript file.
2. Introduce a syntax or semantic error.
3. Wait for diagnostics.
4. Open the Problems panel.
5. Click the problem row.

Expected:

- Problems shows the issue with `filepath (row:col)`.
- Source and severity are visible.
- Clicking the row opens the file and reveals the line/column.
- Editor line background, gutter decoration, minimap, and overview ruler show severity-colored markers.
- Clearing or fixing the problem removes the marker after diagnostics refresh.

Automated coverage:

- Monaco diagnostics are not currently covered by automated UI tests.
- Terminal/test failure parsing into Problems is covered by Vitest.

## 5. Quick Fix and Code Actions

Manual check:

1. Create a TypeScript/JavaScript problem with a language-server quick fix, such as a missing import or removable unused import.
2. Open Monaco Quick Fix / code action.
3. Apply the fix.
4. Open Logs and Problems.

Expected:

- Code action applies to the editor without corrupting file content.
- Logs records success or failure for the code action.
- Editor store content is updated.
- LSP receives `didChange` after the edit.
- Problems and editor markers refresh after diagnostics are republished.
- If a workspace edit targets a file that is not open, failure is logged instead of silently doing nothing.

Automated coverage:

- Code action UI application requires real Monaco/Tauri runtime validation.
- Rust tests cover LSP URI handling used by code action workspace edits.

## 6. Commands, Run History, and Problems

Manual check:

1. Open a workspace with `package.json` scripts.
2. Confirm TopBar Run/Debug/Build/Test and bottom Commands discover workspace commands.
3. Run Build/Test/Lint/Check-style commands.
4. Run an intentionally failing test command.

Expected:

- Build/Test/Lint/Check run through the non-interactive command runner.
- Run History records status, exit code, duration, command, and output.
- Failed output with file/line/column is parsed into Problems.
- Failed run exposes `Fix with Agent`.
- Run/Debug/dev-server style commands open dedicated Terminal sessions.

Automated coverage:

- Rust tests cover package/Cargo task discovery.
- Vitest covers common terminal failure parsing and file URI stack traces.

## 7. Terminal Runtime

Manual check:

1. Open Terminal.
2. Verify cwd/profile display.
3. Create a new terminal session.
4. Restart one session.
5. Switch between Terminal, Commands, Problems, and Logs.
6. Hide/show the bottom panel.
7. Switch workspace and open Terminal again.

Expected:

- Terminal starts in the active workspace, not `C:\Windows`.
- Windows `\\?\D:\...` paths are not passed to `cmd.exe` as unsupported UNC paths.
- Session tabs remain mounted across bottom-tab switches.
- Restart and close affect only the target session.
- Terminal output remains available for Agent runtime failure context.

Automated coverage:

- Rust tests cover workspace path normalization.
- PTY lifecycle currently requires real Tauri runtime validation.

## 8. Git Workflow

Manual check:

1. Open a Git workspace.
2. Modify, add, and delete files.
3. Test staged/worktree/all diff modes.
4. Multi-select files and stage/unstage.
5. Create or checkout a branch.
6. Run fetch/pull/push if remote credentials are available.
7. Open a conflict workspace and test accept current/incoming/both.

Expected:

- Git status is scoped to the active workspace.
- Staged and worktree diffs show the correct content.
- Multi-select batch actions update status without losing selection unexpectedly.
- Remote operations surface credential errors clearly.
- Conflict files are listed and resolution actions update the file.

Automated coverage:

- Rust tests cover Git status, staged/worktree diff, branch checkout, remote branch tracking, conflict detection, conflict resolution, and workspace boundaries.
- Credential UX and full remote interaction require manual validation.

## 9. Agent Repair Loop

Manual check:

1. Produce a failing command or Problem.
2. Click `Fix with Agent` from Problems or failed Run History.
3. Review Agent plan, pipeline stages, Logs, and proposed diffs.
4. Apply one hunk, reject another, and apply a file-level diff.

Expected:

- Agent prompt includes failed command output, Problems, recent Terminal output, and warning/error Logs.
- Pipeline stages and action logs are visible.
- Reviewer receives actual pending diff summaries.
- Per-hunk and per-file apply/reject state is clear.
- Stale `baseHash` diffs are rejected with actionable guidance.

Automated coverage:

- Rust tests cover context compression, pending diff summaries, diff application failures, base hash validation, and hunk operations.
- Full Agent repair flow requires real runtime and LLM configuration.

## 10. Release Smoke Notes

Record each manual smoke run with:

- Date and commit hash.
- OS and shell.
- Workspace used.
- `typescript-language-server` version or unavailable state.
- Failed checklist items with reproduction steps.
- Whether failures are blocking daily IDE usage.

Recommended first target:

- Complete sections 2 through 6 before expanding Rust/Python LSP adapters.
