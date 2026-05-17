# Agent IDE CLI Design

> Goal: turn `agent_cli` from a preview/apply helper into a stable automation surface that can be embedded in CI, scripts, other IDEs, task runners, and fully autonomous repair workflows.

---

## 1. Product Goal

CLI mode should support two use cases:

1. **Toolchain integration**
   - Run from CI, Git hooks, build scripts, package-manager scripts, external IDE tasks, or a future daemon/API wrapper.
   - Produce machine-readable output with stable exit codes and artifacts.
   - Avoid requiring a Tauri window or frontend state.

2. **Full automation**
   - Plan, edit, run checks, parse failures, ask the Agent to repair, and repeat within configured safety limits.
   - Preserve every action in a traceable run directory.
   - Make risky operations opt-in and policy-controlled.

The CLI should reuse backend primitives wherever possible: provider profiles, credential store, context section builder, planner/executor/orchestrator, diff parser/apply, task runner, problem parser, Git helpers, and action log models.

---

## 2. Current State

Current `agent_cli` supports:

- `--endpoint`, `--api-key`, `--model`
- env fallback: `LLM_ENDPOINT`, `LLM_API_KEY`, `LLM_MODEL`
- `--workspace`
- preview by default
- `--apply` for all generated diffs
- single prompt argument
- simple planning and step execution
- workspace boundary checks through shared backend behavior

Current limitations:

- No subcommands.
- No stable JSON/NDJSON protocol.
- No documented exit-code contract beyond process success/failure.
- No shared provider profile lookup or OS credential-store lookup.
- No shared context source flags.
- No context estimate/report artifacts.
- No command runner/test loop.
- No Problems integration.
- No Git workflow integration.
- No per-file/per-hunk review.
- No autonomous repair loop.
- No run-id/artifact persistence.
- No policy model for file, command, Git, and network operations.

---

## 3. CLI Command Model

Target binary name can remain `agent_cli` internally, but distribution should expose a stable command name such as `agent-ide`.

### 3.1 Top-Level Commands

```text
agent-ide --help
agent-ide doctor
agent-ide config list-profiles
agent-ide config show-profile <profile>
agent-ide context estimate [OPTIONS]
agent-ide plan [OPTIONS] <PROMPT>
agent-ide run [OPTIONS] <PROMPT>
agent-ide apply [OPTIONS] <RUN_ID|CHANGES_FILE>
agent-ide review [OPTIONS] <RUN_ID|CHANGES_FILE>
agent-ide fix [OPTIONS] --from failure.json
```

Recommended meanings:

- `doctor`: validate workspace, Git availability, credential-store access, provider profile availability, and optional language servers.
- `config`: inspect profile metadata without printing secrets.
- `context estimate`: print the exact context sections that would be used.
- `plan`: generate a task plan only.
- `run`: full Agent execution flow. Defaults to preview unless `--apply` or `--auto` is set.
- `apply`: apply a previously generated changes artifact.
- `review`: terminal review for per-file/per-hunk decisions.
- `fix`: start from a recorded failure artifact, such as test output or Problems JSON.

### 3.2 Common Options

```text
--workspace <DIR>
--profile <ID>
--endpoint <URL>
--api-key <KEY>
--model <NAME>
--context-mode full|focused|compact
--include active-file,selection,open-files,problems,failed-run,terminal,logs,git-diff,project-tree
--exclude <SOURCE_LIST>
--pin <FILE>
--prompt-file <FILE>
--stdin
--output text|json|ndjson
--artifact-dir <DIR>
--run-id <ID>
--dry-run
--apply
--review
--auto
--max-iterations <N>
--timeout <SECONDS>
--require-clean
--allow-dirty
```

### 3.3 Permission Options

```text
--permission suggest|edit|auto
--allow-create
--allow-edit
--allow-delete
--allow-run <COMMAND_PATTERN>
--allow-git status,diff,commit,push
--deny-path <GLOB>
--confirm-risky never|tty|fail
```

Recommended default:

- Non-interactive CLI defaults to `suggest`.
- `--apply` implies file edit permission but not delete, command execution, or Git mutation.
- `--auto` must explicitly opt into command execution and maximum iterations.
- If no TTY is present, confirmation prompts should fail unless `--confirm-risky never` and the permission policy allows the operation.

---

## 4. Automation Modes

### 4.1 Preview Mode

```powershell
agent-ide run --workspace D:\work\repo --profile default "Refactor parser error handling"
```

Behavior:

- Build context.
- Generate plan.
- Execute steps.
- Parse changes.
- Write artifacts.
- Do not modify workspace.
- Exit with a code that tells automation whether changes were proposed.

### 4.2 Apply Mode

```powershell
agent-ide run --apply --workspace D:\work\repo --profile default "Fix failing tests"
```

Behavior:

- Same as preview mode.
- Apply all accepted generated changes according to policy.
- Record applied/failed files and hunks.
- Return non-zero if any required apply fails.

### 4.3 Review Mode

```powershell
agent-ide run --review --workspace D:\work\repo "Update README examples"
```

Behavior:

- Generate changes.
- Present a terminal review loop.
- Support accept/reject/edit per file and hunk.
- Persist the review decisions.

### 4.4 Autonomous Repair Mode

```powershell
agent-ide run `
  --auto `
  --apply `
  --run "npm test" `
  --max-iterations 3 `
  --allow-run "npm test" `
  --workspace D:\work\repo `
  "Make the test suite pass"
```

Behavior:

```text
build context
  -> plan
  -> generate changes
  -> apply
  -> run configured checks
  -> parse failures into Problems
  -> if failed and iteration budget remains, repair with failure context
  -> write final summary and exit code
```

Full automation should be bounded by:

- iteration limit
- command allow-list
- file permission policy
- workspace boundary checks
- timeout
- maximum generated diff size
- optional clean-worktree requirement

---

## 5. Machine-Readable Interfaces

### 5.1 JSON Summary

`--output json` should print one final JSON object to stdout:

```json
{
  "schemaVersion": 1,
  "runId": "run-20260516-001",
  "status": "changes_proposed",
  "workspace": "D:/work/repo",
  "profileId": "default",
  "context": {
    "mode": "focused",
    "estimatedTokens": 12345,
    "sections": []
  },
  "plan": {
    "steps": []
  },
  "diffs": [],
  "commands": [],
  "problems": [],
  "artifacts": {
    "dir": ".agent-ide/runs/run-20260516-001",
    "events": "events.ndjson",
    "summary": "summary.json",
    "changes": "changes.json"
  }
}
```

### 5.2 NDJSON Event Stream

`--output ndjson` should stream one event per line:

```json
{"type":"run_started","runId":"..."}
{"type":"context_section","id":"git_diff","estimatedTokens":1200,"included":true}
{"type":"plan_ready","steps":[...]}
{"type":"step_started","stepId":"..."}
{"type":"diff_ready","diffId":"...","file":"src/app.ts"}
{"type":"apply_result","file":"src/app.ts","status":"applied"}
{"type":"command_finished","command":"npm test","exitCode":1}
{"type":"problem_detected","file":"src/app.ts","line":10,"severity":"error"}
{"type":"run_finished","status":"failed_checks","exitCode":4}
```

NDJSON is the preferred integration format for tools that want progress updates without parsing human-readable text.

### 5.3 Artifact Directory

Default location:

```text
<workspace>/.agent-ide/runs/<run-id>/
```

Recommended files:

```text
summary.json
events.ndjson
prompt.txt
context.json
context.txt
plan.json
changes.json
changes.patch
apply-result.json
commands.json
problems.json
repair-chain.json
repair-summary.json
action-log.json
```

Artifacts should contain enough data to reproduce or audit the run without relying on frontend state.

---

## 6. Exit-Code Contract

Proposed stable exit codes:

| Code | Meaning |
|------|---------|
| 0 | Completed successfully; no required checks failed. |
| 1 | Unexpected internal error. |
| 2 | Invalid arguments, invalid config, missing profile, or policy violation. |
| 3 | Preview succeeded and changes were proposed but not applied. |
| 4 | Checks failed after applying or after max repair iterations. |
| 5 | Diff apply failed or became stale. |
| 6 | Provider/LLM request failed. |
| 7 | Workspace/Git precondition failed, such as `--require-clean`. |
| 8 | User or policy cancelled the run. |

Automation should be able to use only the exit code and `summary.json` to decide next steps.

---

## 7. Shared Backend Architecture

The CLI should not grow a separate Agent implementation. It should reuse the same backend layers as Tauri commands.

Target extraction:

```text
agent_cli.rs
  -> cli argument parser
  -> cli runner/service
    -> provider profile loader
    -> context builder
    -> orchestrator/executor
    -> diff parser/apply
    -> command runner
    -> problem parser
    -> artifact writer
```

Suggested modules:

```text
src-tauri/src/cli/
  args.rs
  runner.rs
  output.rs
  artifacts.rs
  policy.rs
  exit_codes.rs
  review.rs
```

Shared service candidates:

- provider profile loading from `commands/agent.rs` should move into a reusable `services/llm_profiles.rs`.
- context estimation/building is already in `services/context.rs`; CLI should call the same section builder.
- task execution should reuse the non-interactive command runner logic behind Commands/Run History.
- terminal output problem parsing should move to a backend-shared parser if CLI needs test/lint failure extraction.

---

## 8. Security and Trust Model

CLI automation is higher risk than the desktop UI because it can run unattended.

Required guardrails:

- Workspace-boundary checks for every file write.
- Explicit permission policy for create/edit/delete/run/git/network operations.
- Optional `--require-clean` before apply/auto mode.
- Deny deletion by default.
- Deny command execution by default.
- Deny Git mutation by default.
- Record every automatic operation in `action-log.json`.
- Redact API keys and credential values from logs and artifacts.
- Keep provider secrets in the OS credential store when `--profile` is used.
- Support `--dry-run` for any mutating flow.

---

## 9. Integration Examples

### 9.1 Package Script

```json
{
  "scripts": {
    "agent:fix-tests": "agent-ide run --auto --apply --run \"npm test\" --allow-run \"npm test\" --max-iterations 2 \"Fix failing tests\""
  }
}
```

### 9.2 CI Preview

```powershell
agent-ide run `
  --output json `
  --artifact-dir artifacts\agent `
  --require-clean `
  --workspace . `
  "Review this branch and propose minimal fixes"
```

### 9.3 External IDE Task

```powershell
agent-ide context estimate --output json --workspace .
agent-ide run --output ndjson --workspace . --prompt-file .agent-prompt.txt
```

### 9.4 Headless Repair

```powershell
agent-ide run `
  --auto `
  --apply `
  --run "cargo test" `
  --allow-run "cargo test" `
  --permission auto `
  --max-iterations 3 `
  "Make cargo test pass without changing public APIs"
```

---

## 10. Implementation Plan

### Phase CLI-1: Stable Automation Surface

Deliverables:

- Replace manual argument parsing with a real parser such as `clap`. **Current: done.**
- Add subcommands: `doctor`, `context estimate`, `plan`, `run`. **Current: first pass done.**
- Add `--profile`, `--context-mode`, `--include`, `--prompt-file`, `--stdin`. **Current: done for first-pass shared provider profile loading and CLI-local context sources.**
- Add `--run-command` check execution through the shared project command runner. **Current: first pass done.**
- Add `--output text|json|ndjson`. **Current: done.**
- Add stable exit codes. **Current: done.**
- Add run-id and artifact directory. **Current: done.**
- Add tests for help output, invalid args, exit-code mapping, and JSON serialization. **Current: parser and exit-code tests are in place; more smoke tests are still needed.**

Acceptance:

- CI can call CLI without scraping human text.
- `agent-ide run --output json` writes `summary.json`, `events.ndjson`, `context.json`, and `changes.json`.

### Phase CLI-2: Shared Profiles and Context

Deliverables:

- Move LLM profile loading into reusable service code. **Current: first pass done.**
- Load API keys from OS credential store when `--profile` is used. **Current: first pass done.**
- Use the same backend context section builder as Chat Context Preview.
- Support context include/exclude flags and budget reporting.

Acceptance:

- CLI and desktop produce matching context estimate sections for the same workspace/options.
- CLI can run without passing API keys on the command line.

### Phase CLI-3: Review and Apply Control

Deliverables:

- Add `review` command and `--review` mode.
- Add terminal per-file/per-hunk accept/reject.
- Add `apply` command for saved `changes.json`.
- Add stale-diff detection and regenerate-against-current-file from CLI.

Acceptance:

- A user can generate changes in CI, download artifacts, and apply/review them locally.
- Apply failures are machine-readable and preserve original diffs.

### Phase CLI-4: Fully Automated Repair Loop

Deliverables:

- Add `--run <COMMAND>` checks. **Current: first pass implemented as `--run-command`.**
- Parse command output into Problems.
- Feed failed checks back into Agent repair context.
- Add `--max-iterations`.
- Add command allow-list policy. **Current: first pass implemented as `--allow-run` for repair loops.**

Acceptance:

- `agent-ide run --auto --apply --run "npm test" --max-iterations 3` can iterate until tests pass or budget is exhausted.
- Artifacts show failure -> diff -> apply -> rerun chain.

Current status:

- Shared project task discovery and non-interactive command execution live in `services/project_tasks.rs`.
- CLI can execute checks with `--run-command` and writes command results to artifacts.
- Shared command-output problem parsing lives in `services/problem_parser.rs`.
- Shared Agent single-step runtime helpers live in `services/agent_runtime.rs`, including step-context enrichment and diff provenance used by both CLI execution and IDE single-step/regenerate flows.
- `--max-iterations` provides a first bounded repair loop after `--apply` and failed `--run-command` checks: failed command output and parsed Problems are injected into a repair prompt, new diffs are applied, and checks rerun.
- `--allow-run` is required when `--max-iterations` is enabled. Exact commands, prefix patterns such as `cargo *`, and trusted wildcard `*` are supported.
- `repair-chain.json` records each bounded repair iteration as failure context -> repair prompt -> generated diffs -> apply result -> rerun result.
- `--timeout-seconds`, `--max-output-bytes`, and `--max-diff-files` provide first-pass automation limits.
- Compact text summaries and `repair-summary.json` include command/problem/repair counts for CI logs.
- CLI smoke tests cover `doctor --output json`, preview artifacts, apply artifacts, `repair-chain.json`, and `smoke ide-backend` using a mock provider.
- `smoke ide-backend` discovers package/Cargo tasks, runs the selected project command, parses terminal-like output into Problems, preserves pre-repair Problems in `problems.json`, verifies repair prompt construction, applies generated diffs, reruns checks, and writes `project-tasks.json`, `commands.json`, `apply-result.json`, `repair-chain.json`, and `repair-summary.json`.
- Broader permission policy is still pending.

### Phase CLI-5: Toolchain Packaging

Deliverables:

- Publish/package the CLI with the desktop app or as a separate binary artifact.
- Add shell completion generation.
- Add GitHub Actions examples.
- Add `agent-ide doctor` checks for CI environments.
- Add smoke tests in release validation.

Acceptance:

- CLI can be installed and used by external repositories without launching the desktop app.
- Release smoke tests cover CLI preview, JSON output, and apply mode against a temporary workspace.

---

## 11. Recommended Next Coding Task

CLI Phase 1-4 are now first-pass complete for the intended scope: a headless automation runner that can plan, generate diffs, optionally apply them, run checks, parse Problems, perform bounded repair iterations, run IDE-backend smoke workflows, and write machine-readable artifacts.

Do not keep expanding this into a second interactive IDE by default. Interactive plan editing, per-hunk review, Git workflows, terminal sessions, LSP views, and Problems UI should remain desktop features unless there is a separate product decision to build a full terminal UI.

Recommended next hardening tasks:

1. Keep `doctor --output json` as the machine-readable capability contract for external tools.
2. Expand permission policy only if the CLI scope is intentionally widened.
