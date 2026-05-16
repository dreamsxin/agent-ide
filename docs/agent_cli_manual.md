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
  --workspace <DIR>
  --apply
  --context-mode <full|focused|compact>
  --include <git-diff,project-tree>
  --output <text|json|ndjson>
  --artifact-dir <DIR>
  --run-id <ID>
  --prompt-file <FILE>
  --stdin
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

The CLI currently reads `LLM_ENDPOINT`, `LLM_API_KEY`, and `LLM_MODEL`. It does not yet read desktop UI provider profiles or OS credential-store references.

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
```

`changes.json` and `apply-result.json` are only written when that data exists.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Completed successfully. |
| 1 | Internal error. |
| 2 | Invalid arguments or missing configuration. |
| 3 | Preview succeeded and changes were proposed but not applied. |
| 4 | Checks failed. Reserved for future automated repair loops. |
| 5 | Diff apply failed. |
| 6 | Provider or LLM request failed. |
| 7 | Workspace or precondition failed. |
| 8 | Cancelled. Reserved for future cancellation support. |

## Completeness as an Agent IDE CLI

Implemented:

- Single-prompt Agent execution.
- Workspace selection.
- Direct provider configuration through flags or environment variables.
- Planning output.
- Step execution output.
- Generated diff preview.
- Optional all-diff apply.
- `doctor`, `context estimate`, `plan`, and `run` command shape.
- `--output text|json|ndjson`.
- run-id and artifact directory output.
- stable exit-code contract.
- Workspace-boundary protection.
- Shared backend diff-apply behavior.

Missing for daily IDE replacement:

- No interactive Agent Plan editing, reorder, skip, pause-before-stage, or continue controls.
- No Problems panel, diagnostics bridge, terminal failure parsing, or Fix with Agent loop.
- No Terminal sessions, run history, exit status history, or command rerun flow.
- No LSP-backed completion, hover, diagnostics, rename, references, or code actions.
- No Git status/diff/stage/commit/branch/fetch/pull/push workflow.
- No visual Diff tab, per-hunk review, stale diff guidance, or regenerate-against-current-file UI.
- No context preview, source toggles, pin files, or provider-profile budget display.
- No persisted Agent task recovery, action-log view, or frontend/backend run-id reconciliation.
- No OS credential-store integration for CLI provider profiles.

## Recommended Next CLI Work

The detailed implementation plan is tracked in [agent_cli_design.md](agent_cli_design.md). The short version:

1. Add a real `--profile <name>` path that reads the same provider profile metadata as the desktop app, including OS credential-store references.
2. Add `--context-mode full|focused|compact` and `--include git-diff,project-tree,problems` flags using the same context section builder as the UI.
3. Add a machine-readable `--json` output mode for plans, steps, diffs, failures, and applied files.
4. Add `--review` or `--interactive` mode for per-file and per-hunk apply/reject in the terminal.
5. Add `--run test|build|lint` integration so command failures can feed the same Agent repair context used by the desktop IDE.
6. Add smoke tests that run `agent_cli --help`, preview mode with a mocked LLM response, and apply mode against a temporary workspace.

## Safety Notes

- Prefer preview mode before `--apply`.
- Run from a clean Git worktree or inspect `git diff` after apply mode.
- Do not pass secrets in shell history on shared machines; environment variables are better than command-line flags, but profile-backed OS credential storage is the desired future path.
- Keep prompts scoped. The CLI has fewer interactive guardrails than the desktop IDE.
