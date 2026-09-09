# Getting Started

For someone who has the app open and wants to get a task done. Building from
source is covered in [README.md](../README.md#setup); this document assumes the
app is already running.

## Before the Agent can do anything

Two things are required, and the app tells you when either is missing:

- **A workspace folder.** The title bar shows `No folder opened` until one is set.
  Open one with `Ctrl+O`, or from the command palette (`Ctrl+Shift+P` →
  "Open Workspace Folder"). Every Agent write, command, and file read is confined
  to this folder.
- **A model profile.** The LLM indicator at the right of the top bar turns red
  until one exists. It is a small dot with no visible text — hover it to read
  "LLM Not Configured". Without a profile the Agent panel accepts input but every
  run fails at the first request.

The Run / Debug / Build / Test buttons are disabled unless the project declares
tasks the app can discover (`package.json` scripts, Cargo targets) **and** you are
running the desktop app. In browser preview (`npm run dev`) they are always
disabled, because they need the Tauri runtime.

## 1. Configure a model

Open the Agent panel (`Ctrl+Shift+X`), then reach Settings from the command
palette: `Ctrl+Shift+P` → "Open Agent Settings". It is also the gear icon at the
right end of the Agent panel's tab row.

Fill in, top to bottom. Field labels are quoted as they appear:

- **Profile** — the selector at the top, with a **New** button beside it. Use New
  for a first profile.
- **Profile Name** — free text, e.g. `Work OpenAI`.
- **AI Provider** — picking one fills in a default base URL and model.
- **API Base URL** — e.g. `https://api.openai.com/v1`. Any OpenAI-compatible base
  URL works, including a local server.
- **Secret Key** — stored in the OS credential store, not in the config file. The
  field shows a masked value once saved; the eye icon fetches the real value on
  demand.
- **Model Name** — e.g. `gpt-4o`, `deepseek-chat`.
- **Context Budget Estimate** — model metadata used for budgeting, plus **Per-run
  cap**, which stops a run once provider-reported tokens reach it. Empty means no
  limit.
- **Per-Run Spend Cap** — input and output price per million tokens, in dollars,
  plus a cap. It only takes effect when **both** prices are filled in; with one
  missing, the estimate would undercount and the panel says the cap is not
  enforced rather than showing one that does nothing.
- **Tool Call Mode** — leave on "Provider-native tools". The Agent needs it to read
  your files during a run; without it the model only sees the context bundle
  assembled at the start and has to guess file contents. If your endpoint rejects
  the `tools` parameter, the request is retried without it and the run is flagged
  in the action log.

Then **Save Profile**. Feedback appears directly under that button.

Below it, still in the same panel:

- **Agent Permissions** — see [How much freedom to give it](#4-how-much-freedom-to-give-it).
- **Set Default** / **Delete** for the selected profile.
- **⚡ Test Connection** — worth doing before spending a real run on finding out
  that the URL or key is wrong.
- **MCP** — at the very bottom of the panel. `Ctrl+Shift+P` → search "mcp" is the
  fastest way there.

## 2. Run your first task

In the Agent panel's **Task** tab, describe what you want in your own words, then
press Enter (`Shift+Enter` inserts a newline).

What happens next:

- The Agent plans the work, then runs the configured pipeline of roles. The
  **Plan** tab shows the steps and the stage timeline as they progress.
- The send button changes with state: red **Stop** while working, green
  **Continue** when done, red **Retry** after an error.
- Proposed changes appear inline under the reply as a pending-changes card, and
  also in the **Changes** tab.

## 3. Review and apply changes

You can act on changes at three levels of granularity:

- From the card in the conversation: **Apply** / **Reject** per file, or
  **Apply all**.
- In the **Changes** tab: **Apply All** / **Reject All**, or per file.
- Inside a file's diff: **Apply hunk** / **Reject hunk**, and **Regenerate against
  current file** when the file moved on since the diff was produced.

**Undo Apply** appears whenever there is something to revert, including after
you have applied everything, and its tooltip names how many files it will
restore. Applying is not one-way. Up to 20 apply points are kept, but they live
in memory only — restarting the app discards them, and Git is the backstop after
that.

A diff can also fail to apply because the file changed underneath it. That is
reported per file rather than silently skipped; **Regenerate** is the intended
response.

## 4. How much freedom to give it

Two separate settings, in two different places. They share value names, which is
worth knowing before you rely on either.

- **Agent mode** — the switch in the top bar: `suggest`, `auto`. Controls whether
  changes reach disk without a click. `auto` applies pending diffs when the run
  finishes and lets the Agent write files during the run.
- **Permission preset** — Settings → Agent Permissions: `ask`, `suggest`, `auto`.
  Sets two toggles (create files, run commands),
  which you can also flip individually.

Choosing the `suggest` preset does not put the run in `suggest` mode — the two
settings are independent despite sharing value names.
[SECURITY.md](../SECURITY.md#agent-approval-model) documents which toggles the
backend actually enforces — several are deliberately inert because no
Agent-reachable code path performs those operations yet.

Permissions are captured when a run starts. Narrowing one mid-run does not revoke
a tool already granted for that run; stop the run instead.

## 5. Keyboard shortcuts

Press **F1** in the app for this list.

- `Ctrl+Shift+P` — Command palette
- `Ctrl+O` — Open folder
- `Ctrl+Shift+E` — Toggle the left panel
- `Ctrl+Shift+X` — Toggle the Agent panel
- `` Ctrl+` `` — Toggle the bottom panel
- `Ctrl+Shift+F` — Focus mode (hide all three panels)
- `Ctrl+Shift+D` — Explorer
- `Ctrl+Shift+G` — Source control
- `Ctrl+Shift+T` — Terminal
- `Ctrl+Shift+B` — Commands
- `Ctrl+Shift+M` — Problems
- `Ctrl+Shift+L` — Logs
- `F1` — This shortcut list

Local to a panel: `Enter` / `Shift+Enter` to send or newline in the Agent input,
`Ctrl+Enter` to commit in the Git panel.

There is no shortcut for Settings, the pipeline editor, or applying changes — use
the command palette.

## 6. Where things are

- **Top bar** — project name, LSP status, run/build/test buttons, the code/plan
  mode switch, the Agent mode switch, the LLM indicator, and the panel toggles.
  There is no separate status bar; this row carries all of it.
- **Left panel** — Explorer and Source Control.
- **Editor** — tabs, inline suggestions, and a floating action bar on selected
  text.
- **Right panel** — the Agent: Task, Plan, Changes, plus Pipeline and Settings as
  icons at the end of the row.
- **Bottom panel** — Terminal, Commands, Problems, Logs.

Panel sizes, visibility, and the selected tabs are remembered between launches.

## Anything else

- [README.md](../README.md) — what the product is, and how to build it
- [SECURITY.md](../SECURITY.md) — what the Agent can and cannot touch, and where
  credentials live
- [docs/agent_cli_manual.md](agent_cli_manual.md) — the headless CLI for
  automation and CI
- [ROADMAP.md](../ROADMAP.md) — current state and known issues, including the
  rough edges this document points at
