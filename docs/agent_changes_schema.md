# Agent Changes Schema

`agent-changes` is the structured model-output protocol used by Agent IDE when an Agent proposes file changes.

The protocol is intentionally JSON-in-Markdown so it works with providers that do not support tool calls yet:

````markdown
```agent-changes
{
  "version": 1,
  "changes": [
    {
      "type": "edit",
      "file": "src/app.ts",
      "baseHash": "optional-current-file-hash",
      "rationale": "why this edit is needed",
      "hunks": [
        {
          "original": "exact existing code",
          "updated": "replacement code"
        }
      ]
    },
    {
      "type": "create",
      "file": "src/new-file.ts",
      "rationale": "why this file is needed",
      "content": "complete file content"
    }
  ],
  "findings": [
    {
      "severity": "warning",
      "file": "src/app.ts",
      "hunkIndex": 0,
      "message": "reviewer finding tied to this hunk"
    }
  ]
}
```
````

## Top-Level Fields

| Field | Required | Description |
| --- | --- | --- |
| `version` | yes | Must be `1`. Other versions are rejected and logged. |
| `changes` | yes | Non-empty array of file changes. |
| `findings` | no | Reviewer or validation findings that can be tied to a file/hunk. |

## Change Fields

| Field | Required | Description |
| --- | --- | --- |
| `type` | yes | `edit`, `modify`, `create`, or `new`. `modify` normalizes to `edit`; `new` normalizes to `create`. |
| `file` | yes | Workspace-relative file path. Absolute paths, traversal (`..`), empty segments, URLs, and NUL bytes are rejected. |
| `baseHash` | no | Current file hash used for stale-diff rejection. |
| `rationale` | no | Human-readable reason stored in diff and hunk provenance. |
| `hunks` | edit only | Array of exact replacement hunks. |
| `content` | create only | Complete new file content. |

## Hunk Fields

| Field | Required | Description |
| --- | --- | --- |
| `original` | yes | Exact existing code to replace. Empty `original` is rejected for edits. |
| `updated` | yes | Replacement code. Identical `original`/`updated` hunks are rejected. |

Each accepted hunk receives provenance:

- `changeIndex`
- `hunkIndex`
- `sourceRole`
- `sourceStage`
- `promptContext`
- `rationale`

## Finding Fields

| Field | Required | Description |
| --- | --- | --- |
| `severity` | yes | Free-form severity label such as `error`, `warning`, or `info`. |
| `file` | yes | Must reference a file changed by this block. |
| `hunkIndex` | no | Hunk index in the referenced file. Defaults to `0`. |
| `message` | yes | Non-empty finding text. |

Findings are attached to matching hunk provenance. Invalid findings are not fatal, but they are emitted as validation diagnostics.

## Validation Behavior

Validation failures are emitted into Agent action logs with phase `agent_changes_validation`.

Fatal block-level failures:

- invalid JSON
- missing or unsupported `version`
- empty `changes`

Per-change failures:

- invalid file path
- unsupported change type
- edit change containing `content`
- edit change missing `hunks`
- create change missing non-empty `content`
- create change also containing `hunks`
- hunk with empty `original`
- hunk with identical `original` and `updated`
- hunk containing NUL bytes

Legacy `diff:` and `new:` Markdown blocks are still accepted for compatibility, but new Agent prompts should prefer this schema.
