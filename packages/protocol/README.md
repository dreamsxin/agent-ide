# Protocol

This package holds shared protocol documents and future generated client bindings.

The Rust runtime is the source of truth for trusted-boundary models. TypeScript bindings can be generated or mirrored from those models as the protocol stabilizes.

## Current Contract Surface

The V3 foundation currently exposes these protocol families:

- `workspace`: open workspace, list entries, read files, save files, and inspect Git state
- `command`: run a workspace-scoped command and return final output
- `execution`: track command and test run lifecycle state
- `runtime-event`: stream command output and runtime logs into the workbench
- `task`: discover common workspace tasks such as check and test commands

## Command Protocol

`CommandRequest`

- `command`: shell command string executed relative to the trusted workspace root

`CommandResult`

- `command`: command string that was executed
- `success`: whether the process exited successfully
- `exit_code`: numeric exit code when available
- `stdout`: collected standard output
- `stderr`: collected standard error

`CommandStreamEvent`

- `stream`: `stdout` or `stderr`
- `line`: one emitted output line

The current streaming event is intentionally line-oriented. This keeps the first runtime event model simple while preserving enough structure for logs, tests, and future Agent task output.

## Task Protocol

`WorkspaceTask`

- `id`: stable task identifier, for example `rust.check`
- `label`: user-facing action label
- `command`: command that should run through the command protocol

Task discovery is owned by the Rust runtime because it inspects trusted workspace state. The client renders the discovered task list and sends selected task commands back through the command protocol.

## Runtime Event Protocol

`RuntimeLogEvent`

- `level`: `info`, `success`, or `error`
- `message`: human-readable runtime activity message

Runtime events are presentation-safe summaries. They should not become the source of truth for execution state; a dedicated execution model should be introduced before cancellation, parallel tasks, or Agent orchestration.

## Execution Protocol

`ExecutionState`

- `id`: stable execution identifier for the current run
- `command`: command associated with the execution
- `status`: `idle`, `running`, `succeeded`, or `failed`
- `startedAt`: presentation timestamp for when the run started
- `finishedAt`: presentation timestamp for when the run finished, or `null`
- `exitCode`: process exit code when available, or `null`
- `outputCount`: count of streamed output lines observed by the workbench

The current implementation tracks a single foreground execution. The next runtime iteration should move execution identity into Rust so cancellation and parallel execution can target runtime-owned processes.

`ExecutionEvent`

- `started`: emitted when the runtime-owned execution id is available
- `finished`: emitted when the process exits with success state and exit code

`CommandStreamEvent` now carries `execution_id` so output can be associated with the active runtime-owned execution.

## Next Protocol Step

Before adding deeper Agent orchestration, complete the `Execution` protocol family:

- `ExecutionOutput`
- `ExecutionCancelled`

This will let command runs, tests, and Agent tasks share the same lifecycle model instead of each inventing separate status handling.
