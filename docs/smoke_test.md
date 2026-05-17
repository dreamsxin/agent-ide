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

## 3. Language Server Status

Manual check:

1. Open a workspace that contains `package.json`.
2. Open a `.ts`, `.tsx`, `.js`, or `.jsx` file.
3. Click the `TS ready/unavailable/error` status in the TopBar.
4. Open a Go workspace with `go.mod` or `go.work`, then open a `.go` file.
5. Click the `Go ready/unavailable/error` status in the TopBar.

Expected:

- Status becomes `ready` when `typescript-language-server` is available.
- Go status becomes `ready` when `gopls` is available.
- Details show server path, workspace root, opened documents, changes, diagnostics count, and last error when present.
- Details show server source, install command, detected config files, and indexing mode.
- Missing TypeScript server shows `npm install -D typescript typescript-language-server`.
- Missing Go server shows `go install golang.org/x/tools/gopls@latest`.
- Recent diagnostics show per-file `error/warning/info` counts after diagnostics are published.
- If the server is unavailable, the status explains how to install it.

Automated coverage:

- Rust tests cover LSP file URI encoding/decoding, Windows verbatim path normalization, and indexing-state detection.
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
6. Run fetch/pull/push with one-shot HTTPS credentials.
7. Repeat with `Remember HTTPS credential in OS store` enabled, then run a remote action without re-entering the token.
8. Open a conflict workspace and test accept current/incoming/both.

Expected:

- Git status is scoped to the active workspace.
- Staged and worktree diffs show the correct content.
- Multi-select batch actions update status without losing selection unexpectedly.
- Remote operations surface credential errors clearly.
- Remembered HTTPS credentials are reused from the OS credential store.
- Conflict files are listed and resolution actions update the file.

Automated coverage:

- Rust tests cover Git status, staged/worktree diff, branch checkout, remote branch tracking, conflict detection, conflict resolution, and workspace boundaries.
- SSH/passphrase UX and full remote interaction require manual validation.

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
- Applying or rejecting only part of a diff leaves the file card in `Partial` state and shows pending/applied/rejected/failed hunk counts.
- Problems or Agent findings on the same file/line are shown inside the matching hunk when reviewing a diff.
- Stale `baseHash` diffs are rejected with actionable guidance.

Automated coverage:

- Rust tests cover context compression, pending diff summaries, diff application failures, base hash validation, and hunk operations.
- Full Agent repair flow requires real runtime and LLM configuration.

## 10. End-to-End Daily IDE Loop

Manual check:

1. Open a Git workspace with TypeScript/JavaScript and package scripts.
2. Open a source file and confirm LSP status is ready.
3. Introduce a TypeScript error and confirm Problems plus editor markers update.
4. Run `npm test` or the project test command from TopBar/Commands.
5. Confirm the run appears in Run History with exit code, duration, and output.
6. Click `Fix with Agent` from the failed run or Problem.
7. Review the Agent plan, pipeline, action logs, proposed diff, hunk findings, and partial hunk state.
8. Apply one hunk, reject another, then commit through Git UI.

Expected:

- Terminal/Commands/Problems/LSP/Git/Agent repair loop can be completed without leaving the app.
- Failures remain traceable from Run History or Problems into Agent prompt context and proposed diffs.
- Git status updates after Agent edits and after partial hunk application.
- Any manual failure is recorded with commit hash, workspace path, and reproduction steps.

Automated coverage:

- This full loop still needs Playwright/Tauri-driver coverage. Until then, it is a required manual smoke for changes touching any involved panel.

## 11. LLM Profiles and Budget Metadata

Manual check:

1. Open Agent Settings.
2. Create a new provider profile with endpoint, model, API key, max context, reserved output, and max output tokens.
3. Save it and set it as default.
4. Switch to Chat and select the profile.
5. Change the context compression mode for the next run.

Expected:

- Settings persists multiple profiles without requiring the API key again when editing an existing profile.
- Profile config JSON stores credential references, not plaintext API keys.
- Chat lists all configured profiles and shows the selected profile/model.
- Chat displays an estimated input budget when max context metadata is present.
- `focused`, `compact`, `budgeted`, and `full` are shown as compression modes, not fixed context sizes.
- Existing legacy single-provider config is migrated into a default profile.

Automated coverage:

- Rust tests cover legacy config migration, API key masking, and profile serialization without plaintext API keys.
- Max context and reserved output fields are used for estimated context trimming. Max output is mapped into OpenAI-compatible chat request bodies and should be runtime-verified per provider endpoint.

## 12. Large Workspace LSP Indexing

Manual check:

1. Open a TypeScript/JavaScript workspace with at least hundreds of files and a real `tsconfig.json` or `jsconfig.json`.
2. Open files across multiple folders and watch the TopBar TS status details.
3. Open a Go workspace with `go.mod` or `go.work` and at least several packages.
4. Open files from different packages and watch the TopBar Go status details.

Expected:

- Status details show workspace root, detected config files, indexing mode, opened document count, change count, diagnostics count, and last error.
- Diagnostics refresh after edits without freezing the UI.
- Missing `typescript-language-server` or `gopls` shows install guidance instead of a silent unavailable state.
- Large workspace behavior is recorded in the release smoke notes, including server versions.

Automated coverage:

- Rust tests cover indexing-state detection and path/URI handling.
- Performance and UI responsiveness require real runtime validation on representative workspaces.

## 13. Release Smoke Notes

Record each manual smoke run with:

- Date and commit hash.
- OS and shell.
- Workspace used.
- `typescript-language-server` version or unavailable state.
- Failed checklist items with reproduction steps.
- Whether failures are blocking daily IDE usage.

Recommended first target:

- Complete sections 2 through 9 before expanding Rust/Python LSP adapters.

### Smoke Run Template

Copy this block into the release notes section for each real-runtime validation pass:

```text
Date:
Commit:
OS / shell:
Workspace:
Node / npm:
Rust:
typescript-language-server:
gopls:

Checklist result:
- Runtime mode:
- Language server status:
- Diagnostics -> Problems -> editor markers:
- Quick Fix / code actions:
- Commands / Run History / Problems:
- Terminal runtime:
- Git workflow:
- Agent repair loop:
- End-to-end daily IDE loop:
- Large workspace LSP indexing:

Blocking daily-IDE issues:
- None / list reproduction steps

Follow-up fixes:
- None / link issue or commit
```

## 14. Current Phase 8 Baseline

Status as of 2026-05-16:

- Automated CLI smoke coverage exists for `doctor --output json`, preview artifacts, apply artifacts, `repair-chain.json`, and `smoke ide-backend`.
- Frontend unit coverage exists for file URI/path normalization, terminal failure parsing, problem store behavior, and LSP diagnostics bridging.
- Rust unit coverage exists for Git conflict/status operations, LSP path/indexing helpers, Agent context/diff behavior, and CLI repair artifacts.
- Full Tauri desktop runtime smoke remains required before calling Phase 8 complete, especially for PTY lifecycle, Monaco marker rendering, real LSP server behavior, OS credential storage, and remote Git operations.
