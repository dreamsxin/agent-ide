# Agent IDE CLI Manual

The `agent_cli` binary is a headless Agent runner built from the Rust backend. It is useful for one-off scripted prompts, backend smoke checks, and validating diff parsing/apply behavior without launching the Tauri UI.

It is not currently a full command-line version of Agent IDE. The desktop app remains the complete daily-IDE workflow because it owns visual plan control, Problems, Terminal sessions, Git review, LSP operations, run history, and interactive diff review.

For the target CLI architecture that can integrate with external toolchains and fully automated workflows, see [agent_cli_design.md](agent_cli_design.md).

## Build and Help

```powershell
cd src-tauri
cargo build --bin agent_cli --release
target\release\agent_cli --help
```

Development build:

```powershell
cd src-tauri
cargo run --bin agent_cli -- --help
```

Current help output:

```text
Usage: agent_cli.exe [COMMAND]

Commands:
  doctor   Validate local CLI prerequisites
  context  Context utilities
  plan     Generate a plan only
  run      Run the Agent. This is also the default command when no subcommand is used
  smoke    Run deterministic backend smoke workflows for IDE integration paths
  help     Print help

Options:
  --endpoint <URL>    LLM API endpoint (or LLM_ENDPOINT env)
  --api-key <KEY>     API key (or LLM_API_KEY env)
  --model <NAME>      Model name (or LLM_MODEL env)
  --workspace <DIR>   Project workspace directory (default: current dir)
  --apply             Write generated files to disk
  --context-mode <full|focused|compact>
  --include <git-diff,project-tree>
  --output <text|json|ndjson>
  --artifact-dir <DIR>
  --run-id <ID>
  --prompt-file <FILE>
  --stdin
  --help, -h          Print help
```

`run --help` shows Agent execution options:

```text
Usage: agent_cli.exe run [OPTIONS] [PROMPT]...

Options:
  --endpoint <URL>
  --api-key <KEY>
  --model <NAME>
  --profile <ID>
  --workspace <DIR>
  --apply
  --context-mode <full|focused|compact>
  --include <git-diff,project-tree>
  --output <text|json|ndjson>
  --artifact-dir <DIR>
  --run-id <ID>
  --prompt-file <FILE>
  --stdin
  --run-command <COMMAND>
```

## Configuration

Pass provider values directly:

```powershell
cd src-tauri
target\release\agent_cli `
  run `
  --endpoint https://api.example.com/v1 `
  --api-key sk-... `
  --model example-model `
  --workspace D:\work\my-project `
  "Add unit tests for the parser"
```

Or use environment variables:

```powershell
$env:LLM_ENDPOINT = "https://api.example.com/v1"
$env:LLM_API_KEY = "sk-..."
$env:LLM_MODEL = "example-model"

cd D:\work\my-project
D:\work\agent-ide\src-tauri\target\release\agent_cli "Explain the project structure"
```

The CLI can use the same desktop provider profiles and OS credential-store references:

```powershell
target\release\agent_cli run `
  --profile default `
  --workspace D:\work\my-project `
  "Explain the project structure"
```

It also supports direct config through `--endpoint`, `--api-key`, `--model`, or environment variables `LLM_ENDPOINT`, `LLM_API_KEY`, and `LLM_MODEL`.

For compatibility, the older no-subcommand style still works and is normalized to `run`:

```powershell
target\release\agent_cli --workspace D:\work\my-project "Explain the project structure"
```

## Context Estimate

Use `context estimate` to inspect the same backend context sections that CLI runs will send to the Agent:

```powershell
target\release\agent_cli context estimate `
  --workspace D:\work\my-project `
  --context-mode focused `
  --include git-diff,project-tree `
  --output json
```

The command writes run artifacts under `<workspace>\.agent-ide\runs\<run-id>` unless `--artifact-dir` is provided.

## Preview Mode

By default the CLI prints the generated plan and proposed diffs without writing files:

```powershell
target\release\agent_cli --workspace D:\work\my-project "Create hello.ts"
```

Equivalent explicit command:

```powershell
target\release\agent_cli run --workspace D:\work\my-project "Create hello.ts"
```

Use preview mode first for prompts that may touch multiple files.

## Apply Mode

`--apply` writes generated diffs to the workspace:

```powershell
target\release\agent_cli --apply --workspace D:\work\my-project "Create a React login component"
```

Apply mode reuses the backend diff-apply path and workspace boundary checks. It still lacks the desktop Diff tab's per-file and per-hunk accept/reject controls, stale diff guidance, and regenerate actions.

## Workspace Behavior

- `--workspace <DIR>` sets the project root.
- Without `--workspace`, the current shell directory is used.
- The workspace must exist and be a directory.
- CLI file writes are constrained to the selected workspace.
- The CLI stores temporary Agent IDE config under `<workspace>\.agent-ide` for this headless run.

## Current Workflow

```text
Prompt
  -> build workspace context
  -> planning phase
  -> execute each generated step
  -> parse model output into diffs
  -> print preview
  -> optionally apply all generated diffs
```

The CLI uses the shared planner, executor, context, diff parser, and diff-apply modules. This makes it valuable for backend validation, but it does not run the full desktop orchestrator state machine or UI event loop.

## Machine-Readable Output

Phase 1 supports:

- `--output text`: human-readable progress and summary.
- `--output json`: one final JSON summary object.
- `--output ndjson`: progress events as newline-delimited JSON.

Each run writes artifacts by default:

```text
<workspace>\.agent-ide\runs\<run-id>\
  summary.json
  events.json
  events.ndjson
  prompt.txt
  context.json
  context.txt
  plan.json
  changes.json
  apply-result.json
  commands.json
  problems.json
  repair-chain.json
  repair-summary.json
```

`changes.json` and `apply-result.json` are only written when that data exists.
`commands.json` is written when one or more `--run-command` checks are executed.
`problems.json` is written when command output can be parsed into file/line/column Problems.
`repair-chain.json` and `repair-summary.json` are written when bounded repair iterations run.

## Command Checks

CLI mode can run one or more non-interactive checks after an Agent run by using the shared backend project command runner:

```powershell
target\release\agent_cli run `
  --profile default `
  --workspace D:\work\my-project `
  --run-command "npm test" `
  "Fix failing tests"
```

The check results are included in `summary.json` and `commands.json`; parsed file/line/column failures are included in `summary.json` and `problems.json`. A non-zero check exit code returns CLI exit code `4`.

For bounded repair, combine `--apply`, `--run-command`, and `--max-iterations`:

```powershell
target\release\agent_cli run `
  --profile default `
  --workspace D:\work\my-project `
  --apply `
  --run-command "npm test" `
  --allow-run "npm test" `
  --max-iterations 2 `
  --timeout-seconds 120 `
  --max-output-bytes 60000 `
  --max-diff-files 20 `
  "Fix failing tests"
```

When checks fail after an applied change, the CLI builds a repair prompt from the failed command output and parsed Problems, generates another diff, applies it, and reruns the checks until they pass or the iteration budget is exhausted. Repair mode requires each check command to be authorized with `--allow-run`. Use an exact command, a prefix pattern such as `cargo *`, or `*` for all commands in trusted workspaces.

Each repair iteration is recorded in `repair-chain.json` with the failed commands before repair, parsed Problems, generated diffs, apply result, rerun command results, and final failed/pass state for that iteration. `repair-summary.json` keeps the same chain compact for CI dashboards and logs.

`problems.json` records all observed Problems for the run, including Problems seen before a successful repair. This keeps the failure -> repair -> rerun chain traceable even when the final command result is clean.

Hardening flags:

- `--timeout-seconds <N>` bounds Agent execution and command checks.
- `--max-output-bytes <N>` trims command stdout/stderr in artifacts after Problems are parsed.
- `--max-diff-files <N>` rejects generated changes that touch too many files.

## IDE Backend Smoke

Use `smoke ide-backend` when you want CLI automation to validate backend pieces that the desktop UI depends on:

```powershell
target\release\agent_cli smoke ide-backend `
  --profile default `
  --workspace D:\work\my-project `
  --output json `
  --artifact-dir .agent-ide\smoke\ide-backend `
  "Fix failing backend smoke"
```

If no `--run-command` is supplied, the smoke command discovers project tasks from `package.json` and Cargo manifests, then prefers `test`, `check`, `lint`, `typecheck`, or `build`. It forces apply mode and enables one bounded repair iteration by default. Discovered tasks are written to `project-tasks.json`.

This smoke covers:

- workspace resolution and boundary setup;
- package scripts / project command discovery;
- shared command runner execution;
- terminal-like output parsing into Problems;
- `problems.json`, including pre-repair failures;
- repair prompt construction from failed command output and Problems;
- diff parsing;
- apply result;
- rerun result;
- `repair-chain.json` and `repair-summary.json`.

It still does not test Monaco markers, xterm rendering, panel state, Tauri IPC event timing, or interactive Git/LSP UI behavior. Those remain desktop runtime smoke items.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Completed successfully. |
| 1 | Internal error. |
| 2 | Invalid arguments or missing configuration. |
| 3 | Preview succeeded and changes were proposed but not applied. |
| 4 | Checks failed after configured command checks or after repair iterations are exhausted. |
| 5 | Diff apply failed. |
| 6 | Provider or LLM request failed. |
| 7 | Workspace or precondition failed. |
| 8 | Cancelled. Reserved for future cancellation support. |

## Completeness as an Agent IDE CLI

CLI mode is now scoped as a **headless automation runner**, not a full terminal IDE replacement. It is suitable for scripts, CI-style checks, external toolchain integration, and bounded repair attempts. Interactive IDE workflows remain in the desktop UI.

Implemented:

- Single-prompt Agent execution.
- Workspace selection.
- Direct provider configuration through flags or environment variables.
- Planning output.
- Step execution output.
- Generated diff preview.
- Optional all-diff apply.
- `doctor`, `context estimate`, `plan`, and `run` command shape.
- `smoke ide-backend` for IDE backend integration smoke.
- `--output text|json|ndjson`.
- run-id and artifact directory output.
- stable exit-code contract.
- shared backend command runner checks through `--run-command`.
- shared backend terminal/test problem parsing for command output.
- bounded repair iterations with `--max-iterations` after applied command-check failures.
- repair-loop command authorization through `--allow-run`.
- repair-chain artifacts that link failures, generated repair diffs, apply results, and rerun results.
- timeout, command-output, and generated-diff file-count limits for automation runs.
- compact text and JSON summaries with command/problem/repair counts for CI logs.
- smoke coverage for `doctor --output json`, preview artifacts, apply artifacts, and `repair-chain.json`.
- smoke coverage for workspace parsing, package script discovery, command runner, Problems artifact, repair prompt, diff parsing, apply, rerun, and repair-chain through `smoke ide-backend`.
- Workspace-boundary protection.
- Shared backend diff-apply behavior.

Intentionally outside the current CLI scope:

- No interactive Agent Plan editing, reorder, skip, pause-before-stage, or continue controls.
- No Problems panel, diagnostics bridge, terminal failure parsing, or Fix with Agent loop.
- No Terminal sessions, run history, exit status history, or command rerun flow.
- No LSP-backed completion, hover, diagnostics, rename, references, or code actions.
- No Git status/diff/stage/commit/branch/fetch/pull/push workflow.
- No visual Diff tab, per-hunk review, stale diff guidance, or regenerate-against-current-file UI.
- No context preview, source toggles, pin files, or provider-profile budget display.
- No persisted Agent task recovery, action-log view, or frontend/backend run-id reconciliation.

## Recommended Next CLI Work

The detailed implementation plan is tracked in [agent_cli_design.md](agent_cli_design.md). The CLI surface can now be treated as Phase 1-4 first-pass complete. Further work should focus on hardening the automation contract rather than adding desktop-IDE features:

1. Expand permission policy beyond command checks to file create/delete and Git mutations only if the CLI scope is intentionally widened.

## Safety Notes

- Prefer preview mode before `--apply`.
- Run from a clean Git worktree or inspect `git diff` after apply mode.
- Do not pass secrets in shell history on shared machines; environment variables are better than command-line flags, but profile-backed OS credential storage is the desired future path.
- Keep prompts scoped. The CLI has fewer interactive guardrails than the desktop IDE.
