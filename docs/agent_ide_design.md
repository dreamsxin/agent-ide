# Agent IDE Detailed Design

> Current detailed design for the Tauri + React + Rust Agent IDE.
> `ROADMAP.md` remains the implementation state source of truth. This document explains the system design, workflows, context handling, Agent orchestration, and technical boundaries.

---

## 1. Document Sync Check

### `docs/agent_ide_plan.md`

Status: **partially synchronized**.

Still accurate:

- Core stack: Tauri v2, Rust backend, React 18, TypeScript, Tailwind, Monaco, xterm.js, Zustand.
- High-level architecture: frontend WebView invokes Rust commands and receives Tauri events.
- Main product direction: code-centric controllable Agent IDE.

Outdated or incomplete:

- Directory tree omits newer files such as `src-tauri/src/agent/diff_apply.rs`, `src/utils/tauri.ts`, `src/hooks/useAgentBridge.ts`, and several current components/stores.
- IPC examples use older names such as `send_prompt`, `apply_diff`, and `reject_diff`; current commands include `send_agent_prompt`, `apply_diffs`, and `reject_diffs`.
- Phase checklist still marks many Agent/Git/Terminal capabilities as incomplete even though parts are now implemented.
- Multi-Agent collaboration was described as planned; the backend now executes the configured pipeline as role-aware stages.
- Context compression and workspace boundary policy are missing from the old plan.

### `docs/agent_ide_ui_design.md`

Status: **directionally synchronized, product-target oriented**.

Still accurate:

- UI philosophy: editor first, AI visible but controllable, transparent task/diff review.
- Main layout: Explorer / Editor / Agent panel / Bottom execution panel.
- Agent states and task/diff visualization goals.
- Role-based Agent concept: Architect, Coder, Tester, Reviewer.

Outdated or aspirational:

- Split view, minimap, Ghost Mode, drag-driven AI, conflict resolution UI, and Agent action history are product goals, not fully implemented.
- Task pipeline is now wired to backend execution, but the UI design still describes it mostly as a conceptual collaboration view.
- Tests and Actions bottom tabs are not yet backed by full real workflows.

---

## 2. System Overview

Agent IDE is a local desktop IDE built around three control surfaces:

1. **Editor surface**: Monaco editor, tabs, file contents, selections, inline/diff overlays.
2. **Agent surface**: chat input, task plan, role pipeline, diff review, model settings.
3. **Execution surface**: terminal, logs, Git status/diff/commit, future tests/actions.

The frontend is responsible for interaction, state, and rendering. The Rust backend owns filesystem access, workspace boundary checks, terminal processes, Git operations, LLM streaming, Agent orchestration, and diff application.

```text
React UI
  -> Zustand stores
  -> Tauri invoke commands
  -> Rust command layer
  -> services / agent modules
  -> Tauri events
  -> useAgentBridge / UI refresh
```

The Agent path is intentionally split across small modules. UI components do not call the LLM directly. They dispatch through `useAgentStore`, cross the Tauri command boundary, and let the Rust orchestrator control planning, role execution, diff parsing, review, action logging, and optional apply behavior.

Important runtime distinction:

- `npm run dev`: browser/Vite preview only. Tauri IPC-dependent features are guarded or disabled.
- `npm run tauri -- dev`: real IDE runtime with filesystem, Git, terminal, and Agent backend.

---

## 3. Runtime Architecture

### Frontend

Key modules:

- `src/App.tsx`: main layout, workspace restore, shortcut help, Agent event bridge mount.
- `src/stores/useEditorStore.ts`: open files, active file, content cache, save/open operations.
- `src/stores/useAgentStore.ts`: Agent state, mode, messages, steps, diffs, pipeline, LLM config.
- `src/hooks/useAgentBridge.ts`: subscribes to backend Agent events and updates Zustand.
- `src/components/agent/*`: chat, tasks, diff review, role selector, pipeline editor, settings.
- `src/components/panels/*`: Explorer, Git, Terminal, Logs.
- `src/utils/tauri.ts`: detects Tauri runtime so browser preview does not crash.

### Backend

Key modules:

- `src-tauri/src/lib.rs`: Tauri builder, plugin setup, command registration.
- `src-tauri/src/commands/fs.rs`: workspace-scoped file operations.
- `src-tauri/src/commands/git.rs`: Git status, diff, commit, workspace path validation.
- `src-tauri/src/commands/terminal.rs`: PTY spawn/write/resize/output/kill lifecycle.
- `src-tauri/src/commands/agent.rs`: Agent command API, LLM config, mode, pipeline, diff apply.
- `src-tauri/src/services/workspace.rs`: saved workspace, config directory, path resolution.
- `src-tauri/src/services/context.rs`: `AgentContext` and compression modes.
- `src-tauri/src/services/llm_client.rs`: OpenAI-compatible streaming chat client.
- `src-tauri/src/agent/orchestrator.rs`: Agent state machine integration and pipeline execution.
- `src-tauri/src/agent/multi_agent.rs`: roles, role prompts, pipeline stages.
- `src-tauri/src/agent/diff_apply.rs`: structured diff application and failure reporting.

---

## 4. Core Workflows

### 4.1 Open Workspace

```text
App boot
  -> invoke("get_workspace_path")
  -> workspace path restored into layout/editor stores
  -> Explorer lists files through list_directory
  -> filesystem paths are resolved through workspace service
```

The backend treats the saved workspace as the allowed root for filesystem, Git, terminal cwd, and Agent diff writes. Backend commands should use `workspace::resolve_existing` or `workspace::resolve_for_write` before touching paths.

### 4.2 Open and Save File

```text
Explorer selects file
  -> useEditorStore.openFile()
  -> invoke("read_file_content", { path })
  -> fs command validates path inside workspace
  -> content cached in Zustand
  -> Monaco renders active tab

Ctrl+S / save action
  -> useEditorStore.saveCurrentFile()
  -> invoke("write_file_content", { path, content })
  -> backend validates write target
  -> file is written
```

### 4.3 Agent Prompt

```text
ChatView.handleSend()
  -> collect active file, active content, selected text, context file list
  -> useAgentStore.sendPrompt()
  -> invoke("send_agent_prompt", { request })
  -> AgentGlobalState resolves selected LLM profile and reads its API key from the OS credential store
  -> AgentGlobalState clones LLM client, context compression, and current pipeline
  -> AgentContext is enriched with workspace project tree and Git diff
  -> ContextCompressionMode formats the context as full/focused/compact
  -> AgentOrchestrator.run()
```

The Agent emits events while running:

| Event | Payload | Frontend Consumer |
|-------|---------|-------------------|
| `agent-state-changed` | state/mode | `useAgentBridge` -> Agent state |
| `agent-stream-token` | string token | stream content |
| `agent-plan-ready` | `TaskStep[]` | task view |
| `agent-step-update` | `TaskStep` | step status/logs |
| `agent-pipeline-update` | `PipelineStage[]` | pipeline timeline |
| `agent-diff-ready` | `FileDiff[]` | diff review |
| `agent-action-log` | `ActionLogEntry` | logs/audit trail |

Frontend scheduling responsibilities:

| Module | Role |
|--------|------|
| `ChatView` | Captures the user prompt and active editor context. |
| `QuickActions` | Creates focused prompts from the current editor selection. |
| `useAgentStore` | Holds Agent state and invokes backend commands. |
| `useAgentBridge` | Listens to Agent events and updates messages, task steps, diffs, pipeline stages, and logs. |
| `DiffView` | Lets the user apply/reject all diffs, individual files, or individual hunks. |

Backend scheduling responsibilities:

| Module | Role |
|--------|------|
| `commands/agent.rs` | IPC boundary, request validation, context construction, pipeline/config lookup. |
| `services/context.rs` | Workspace context enrichment and compression. |
| `services/credentials.rs` | OS credential store access for LLM profile secrets. |
| `agent/orchestrator.rs` | State transitions, planner call, pipeline sequencing, reviewer context, action logs. |
| `agent/planner.rs` | Converts the user prompt and context into task steps. |
| `agent/executor.rs` | Runs role-specific model calls and streams output. |
| `agent/multi_agent.rs` | Defines role prompts and pipeline stage semantics. |
| `agent/diff_apply.rs` | Applies validated pending diffs inside the workspace. |

### 4.4 Agent Pipeline

Current backend execution is role-aware:

```text
Planner
  -> produces task steps
Pipeline reset to pending
  -> Architect stage
      -> architecture/design output
  -> Coder stage
      -> implementation diff/new-file blocks
  -> Tester stage
      -> test diff/new-file blocks or test findings
  -> Reviewer stage
      -> review findings and optional required fix diffs
Diff parser
  -> extracts pending FileDiff entries
Review state
  -> user applies/rejects diffs, or Auto mode applies directly
```

Each stage receives:

- Original user prompt.
- Compressed project context.
- Prior stage outputs.
- Role-specific system prompt and output rules.

The configured pipeline lives in `AgentGlobalState.pipeline_stages` and can be changed through `get_pipeline`, `update_pipeline`, and `reset_pipeline`.

Reviewer behavior is tied to actual proposed changes. After earlier stages produce model output, the orchestrator parses pending diffs and sends a summary of those concrete file/hunk changes into the reviewer stage. That prevents review from relying only on previous prose.

Action logs are emitted for prompt receipt, planner completion, stage start/completion/failure, diff readiness, review context, and apply results. The frontend displays these logs so users can audit what the Agent did and what context summary was used.

### 4.5 Diff Review and Apply

Model responses prefer a structured protocol:

````text
```agent-changes
{
  "changes": [
    {
      "type": "edit",
      "file": "path/to/file",
      "baseHash": "optional current file hash when known",
      "rationale": "why this change is needed",
      "hunks": [
        { "original": "exact existing code", "updated": "replacement code" }
      ]
    },
    {
      "type": "create",
      "file": "path/to/new-file",
      "rationale": "why this file is needed",
      "content": "complete file content"
    }
  ]
}
```
````

Legacy markdown diff blocks are still supported for compatibility:

````text
```diff:path/to/file
<<<<<<< ORIGINAL
existing code
=======
updated code
>>>>>>> UPDATED
```
````

New files use:

````text
```new:path/to/file
file content
```
````

Apply flow:

```text
DiffView.applyAllDiffs()
  -> invoke("apply_diffs")
  -> apply_pending_diffs()
  -> resolve each target path inside workspace
  -> apply each pending diff
  -> return ApplyDiffsResult { applied, failed }
  -> frontend marks applied/failed cards
```

Current conflict behavior:

- Rejects outside-workspace paths.
- Rejects missing original content.
- Rejects ambiguous original matches.
- Rejects new-file overwrite.
- Reports partial failures structurally.

Known limitation:

- Hunks are still text-match based and do not include file version/hash metadata.
- Per-hunk apply/reject is implemented in the backend and Diff view; mixed hunk states still need clearer partial-status semantics.

### 4.6 Terminal

```text
Terminal component mounts in Tauri runtime
  -> invoke("spawn_terminal", { id })
  -> listen("terminal-output")
  -> xterm writes user input
  -> invoke("write_to_terminal", { id, data })
  -> ResizeObserver invokes resize_terminal
  -> unmount invokes kill_terminal
```

Terminal cwd is scoped to the saved workspace. Browser preview shows a disabled-state message instead of attempting PTY access.

Terminal/test failures are parsed into structured Problems when output includes file, line, and column information. Those Problems are mirrored back into Monaco markers so runtime/test failures can be highlighted in the editor instead of only appearing in the Problems panel.

### 4.6.1 Problems and Diagnostics

Problems currently aggregate multiple sources:

- `diagnostic`: Monaco built-in language diagnostics.
- `lsp`: diagnostics published by the TypeScript language server.
- `test`: terminal/task/test failures parsed from command output.
- `agent` and `system`: Agent/runtime issues surfaced by the IDE.

The editor has three marker bridges:

- Monaco diagnostics are read into Problems through `DiagnosticsBridge`.
- TypeScript LSP diagnostics are written to Problems and Monaco markers through `useLspDiagnostics`.
- Runtime Problems from terminal/test/Agent/system sources are written back to Monaco markers through `ProblemsMarkerBridge`.
- All Problems sources are mirrored into severity-colored editor decorations for the active model, including whole-line background, line-decoration gutter, minimap, and overview ruler indicators.

Paths are normalized before tab matching, marker matching, and problem navigation. This avoids duplicate tabs and broken paths such as URL-encoded Windows drive paths.

### 4.6.2 Language Server Semantic Bridge

Current semantic support uses two layers:

- Monaco TypeScript/JavaScript worker fallback for open-file syntax and semantic diagnostics.
- Optional `typescript-language-server` backend for hover, completion, definition, document symbols, rename, code actions, and diagnostics.
- Optional `gopls` backend for Go hover, completion, definition, document symbols, rename, code actions, and diagnostics.

The Rust backend chooses a language server from the active file language. TypeScript/JavaScript use `typescript-language-server`; Go uses `gopls`. TopBar shows `TS checking/ready/unavailable` or `Go checking/ready/unavailable`; the details popover includes startup errors, install command, server source, detected config files, and inferred indexing mode.

Remaining semantic work:

- Validate workspace-wide indexing across larger TypeScript projects.
- Add Rust/Python LSP adapters.
- Runtime-validate indexing behavior on larger monorepos and project-reference workspaces.
- Feed code actions with actual diagnostics context for richer quick fixes.

### 4.7 Git

Git commands resolve paths through the workspace service and then use `git2`:

- `git_status(path)`
- `git_diff(path, file?, kind?)`, where `kind` is `worktree`, `staged`, or `all`
- `git_stage_files(path, files)`
- `git_unstage_files(path, files)`
- `git_discard_files(path, files)`
- `git_commit(path, message)`
- `git_checkout_branch(path, branch, create)`
- `git_fetch(path, remote?, credentials?)`
- `git_pull(path, remote?, credentials?)`
- `git_push(path, remote?, credentials?)`

Current Git scope covers status, staged/worktree/all diff views, file and multi-file stage/unstage/discard, commit, local branch checkout/create, remote branch checkout/tracking, fetch, fast-forward-only pull, push, upstream/ahead/behind display, one-shot credential inputs for remote actions, optional OS-stored HTTPS remote credentials, conflict file detection, and basic conflict resolution controls.

Remaining Git roadmap work:

- Better SSH/passphrase failure recovery.
- Rich merge editor UI for conflict blocks.
- Safer destructive-action UX for discard/revert/reset workflows.

---

## 5. Context Model

### 5.1 AgentContext

The current Agent prompt context includes:

- `active_file`
- `active_file_content`
- `selection`
- `open_files`
- `project_path`
- `project_tree`
- `git_diff`
- `project_memory`

`project_memory` is loaded from a workspace-root `AGENTS.md` when present (`services/project_memory.rs`). It is trimmed to 8,000 characters, included by default in every run, and rendered as the first content section after the project header so it survives budget trimming. Per-run source toggles can exclude it via `includeProjectMemory`; the CLI selects it with `--include project-memory`.

This context is built in `send_agent_prompt` from the frontend request and the saved workspace root.
The backend enriches it with a bounded project tree summary and, when the workspace is a Git repository, a bounded working tree diff.
Runtime failure prompts can also include recent Problems, failed command output, terminal excerpts, and warning/error logs before reaching the backend.

### 5.2 Compression Modes

Context compression is implemented in `src-tauri/src/services/context.rs`.

| Mode | Intent |
|------|--------|
| `full` | Include complete active context. Best fidelity, largest prompt. |
| `focused` | Include selection and active-file excerpt. Default practical mode. |
| `compact` | Include outline/metadata-style summary. Lowest token use. |
| `budgeted` | Token-budget-aware packing using the active provider profile budget or a safe default budget. |

### 5.3 Context Boundaries

The Agent should not receive more context than needed. Preferred priority:

1. User selection.
2. Active file content or excerpt.
3. Explicitly attached/open files.
4. Git diff and relevant project tree summary.
5. Terminal/log excerpts when the task is about runtime errors.

Context should carry provenance in future action logs so users can inspect what was sent to the model.

Current provenance level:

- Action logs include prompt phase, role/stage, context summary, diff summary, and details.
- Reviewer receives pending diff summaries generated from actual proposed changes.
- File diffs include protocol/operation/schema/source stage provenance.
- Hunks include change/hunk index, role/stage, prompt context, and rationale when generated by structured `agent-changes`.
- Full persistent action-log history and exact context source manifests are still future work.

---

## 6. Agent Modes and Safety

| Mode | Intended Behavior | Current Behavior |
|------|-------------------|------------------|
| `suggest` | Suggest changes only | Produces reviewable diffs |
| `edit` | Can prepare edits for user confirmation | Produces reviewable diffs |
| `auto` | Can apply accepted Agent diffs automatically | Applies pending diffs after pipeline run |

Safety rules:

- Filesystem writes must go through workspace path resolution.
- Agent-generated HTML is not rendered directly; markdown rendering skips HTML.
- Diff application returns structured failures and preserves failed file content.
- Cancellation is cooperative through a shared atomic flag and streaming checks.
- LLM API keys are stored through the OS credential store; local JSON profile config stores credential references only. This still needs cross-OS runtime validation and recovery UX for inaccessible credentials.

---

## 7. State and Data Structures

### AgentState

```text
idle
thinking
planning
acting
reviewing
waiting_user
done
error
```

### TaskStep

```typescript
{
  id: string;
  title: string;
  type: "create" | "edit" | "run" | "test" | string;
  status: "todo" | "doing" | "done" | "error";
  logs: string[];
}
```

### PipelineStage

```typescript
{
  role: "architect" | "coder" | "tester" | "reviewer";
  name: string;
  status: "pending" | "active" | "completed" | "failed";
}
```

### FileDiff

```typescript
{
  id: string;
  file: string;
  hunks: DiffHunk[];
  status: "pending" | "applied" | "rejected" | "failed";
  applyError?: string;
}
```

---

## 8. Technical Gaps Before Daily IDE Replacement

Highest-impact gaps:

1. **Structured Agent protocol**
   - `agent-changes` JSON blocks are supported with a versioned schema and validation diagnostics.
   - Schema details live in `docs/agent_changes_schema.md`.
   - Future work is a provider-native tool-call transport, not basic schema support.

2. **Version-aware diff application**
   - Optional `baseHash` metadata is now supported and checked before edit diffs are applied.
   - Support per-file and per-hunk apply/reject.
   - Show conflicts with clear recovery options.

3. **Context expansion**
   - Git diff and project tree summary are now included.
   - Add terminal/log excerpts and selected file packing.
   - Add token budget packing.

4. **Action log**
   - Persist prompt, compressed context summary, stage outputs, diffs, apply results, and errors.
   - Make Agent actions auditable in the UI.

5. **Secret storage**
   - Runtime-validate OS credential storage and add recovery UX for inaccessible or missing LLM credentials.

6. **Runtime hardening**
   - Interactive Tauri smoke tests for boot, workspace open, file read/write, terminal, Agent prompt, diff apply.
   - Frontend store/component tests for Agent events and diff status updates.

---

## 9. Verification

Baseline checks before considering Agent workflow changes complete:

```powershell
npm run build
cd src-tauri
cargo check
cargo test
```

Current known build note:

- Vite warns about a large frontend chunk due to Monaco/Markdown/xterm/syntax tooling. This is not a correctness failure, but code splitting should be added before release readiness.

---

## 10. Performance Optimization & Open-Source Model Integration (Phase 9)

### 10.1 Incremental Rendering Architecture

Inspired by **Zed Editor's** high-performance rendering approach, Agent IDE implements incremental rendering to handle large files efficiently:

```text
Monaco Editor (Web Worker)
  -> IncrementalRenderer
    -> Viewport tracking
      -> Dirty line detection
        -> Frame-budget rendering
          -> Multi-threaded text operations
```

**Key Components:**

- **Viewport Management**: Track visible lines and prioritize rendering
- **Dirty Line Detection**: Only re-render changed lines
- **Frame Budget**: Allocate time per frame (16ms for 60fps)
- **Multi-threading**: Offload heavy operations to worker threads

```rust
pub struct IncrementalRenderer {
    viewport: Viewport,
    dirty_lines: HashSet<LineNumber>,
    frame_budget: Duration,
    target_fps: u32,
}

impl IncrementalRenderer {
    pub fn render_with_budget(&mut self, changes: Vec<TextChange>) -> RenderResult {
        let start = Instant::now();

        // Mark affected lines as dirty
        for change in changes {
            self.dirty_lines.extend(change.affected_lines());
        }

        // Render critical content first
        let critical = self.render_critical_elements();

        // Render secondary content if time allows
        if start.elapsed() < self.frame_budget {
            self.render_secondary_elements();
        }

        RenderResult::new(critical)
    }

    pub fn render_dirty_regions(&mut self, budget: Duration) {
        let start = Instant::now();
        for line in &self.dirty_lines {
            if start.elapsed() > budget { break; }
            self.render_line(line);
        }
    }
}
```

### 10.2 Intelligent Code Completion System

Inspired by **Comate's** intelligent code completion, Agent IDE provides context-aware suggestions with Chinese optimization:

```text
Code Completion Trigger
  -> Context Analyzer
    -> Surrounding Code Analysis
      -> Project Structure Awareness
        -> Recent Edits Context
          -> Suggestion Generation
            -> Chinese-Optimized Prompts
              -> Local/Cloud Model Selection
```

**Components:**

```typescript
export class IntelligentCodeCompletion {
    private contextAnalyzer: ContextAnalyzer;
    private suggestionCache: Map<string, Suggestion[]>;
    private modelSelector: ModelSelector;

    async getSuggestions(
        file: string,
        position: Position,
        surroundingCode: string
    ): Promise<Suggestion[]> {
        const context = await this.contextAnalyzer.analyze({
            file,
            position,
            surroundingCode,
            projectStructure: await this.getProjectContext(),
            recentEdits: this.getRecentEdits(),
            language: this.detectLanguage(file)
        });

        // Build Chinese-optimized prompts
        const prompt = this.buildChineseOptimizedPrompt(context);

        // Select appropriate model based on task complexity
        const model = this.modelSelector.selectBestModel(context.complexity);

        return this.generateSuggestions(model, prompt);
    }

    private buildChineseOptimizedPrompt(context: CodeContext): string {
        // Comate-style Chinese prompt engineering
        return `
分析以下代码上下文，提供智能代码补全建议：

文件: ${context.file}
位置: ${context.position.line}:${context.position.character}
语言: ${context.language}

周围代码:
${context.surroundingCode}

项目结构:
${context.projectStructure.summary}

请基于中文编程习惯提供以下建议：
1. 当前上下文最可能的代码补全
2. 考虑项目中的相似代码模式
3. 提供符合中文开发者习惯的命名建议
`;
    }
}
```

### 10.3 Open-Source Model Integration

Agent IDE supports multiple open-source code models to provide cost-effective and privacy-preserving AI assistance:

```text
EnhancedLlmClient
  -> Cloud Models (OpenAI, DeepSeek, etc.)
  -> Local Models
    -> StarCoder (Hugging Face)
    -> CodeLlama (Meta)
    -> DeepSeek Coder (DeepSeek)
    -> CodeGemma (Google)
```

**Architecture:**

```rust
pub struct EnhancedLlmClient {
    openai_client: Option<LlmClient>,
    local_models: Vec<LocalModel>,
    model_selector: ModelSelector,
}

pub struct LocalModel {
    name: String,
    model_type: ModelType,
    engine: Box<dyn ModelEngine>,
    capabilities: ModelCapabilities,
}

pub enum ModelType {
    StarCoder,      // Hugging Face open-source, good for Python/JS
    CodeLlama,      // Meta open-source, strong multi-language support
    DeepSeekCoder,  // DeepSeek open-source, optimized for code completion
    CodeGemma,      // Google open-source, efficient inference
}

pub struct ModelCapabilities {
    max_context_tokens: u32,
    supported_languages: Vec<String>,
    inference_speed_ms: u32,
    memory_requirement_mb: u32,
}

impl EnhancedLlmClient {
    // Local model inference
    pub async fn generate_code_local(
        &self,
        prompt: &str,
        model: &LocalModel
    ) -> Result<String> {
        model.engine.generate(prompt).await
    }

    // Smart model selection
    pub async fn smart_generate(&self, prompt: &str, context: &TaskContext) -> Result<String> {
        let model = self.model_selector.select_best_model(context);

        match model {
            ModelSource::Local(local_model) => {
                self.generate_code_local(prompt, local_model).await
            }
            ModelSource::Cloud(cloud_client) => {
                cloud_client.stream_chat(/* ... */).await
            }
        }
    }
}

pub struct ModelSelector;

impl ModelSelector {
    pub fn select_best_model(&self, context: &TaskContext) -> ModelSource {
        match context.complexity {
            Complexity::Low => self.find_fastest_local_model(),
            Complexity::Medium => self.find_balanced_model(),
            Complexity::High => self.find_most_capable_cloud_model(),
        }
    }
}
```

**Model Integration Benefits:**

- **Cost Reduction**: Use local models for simple tasks
- **Privacy**: Keep code on local machine
- **Latency**: Faster responses for local inference
- **Reliability**: Work offline with local models
- **Hybrid Strategy**: Smart selection based on task needs

### 10.4 Plugin Architecture (DeepSeek Harness Inspired)

Inspired by **DeepSeek Harness's** "everything is a plugin" philosophy, Agent IDE introduces a modular plugin system:

```text
PluginManager
  -> Model Plugins (OpenAI, Local Models)
  -> Tool Plugins (File operations, Git, Terminal)
  -> Skill Plugins (Code generation, Debugging, Testing)
  -> UI Plugins (Custom panels, Commands)
  -> Pipeline Plugins (Custom Agent roles)
```

**Plugin Interface:**

```typescript
export interface Plugin {
    name: string;
    version: string;
    description?: string;

    // Lifecycle hooks
    onActivate?(context: PluginContext): void;
    onDeactivate?(): void;

    // Extension points
    registerCommands?(registry: CommandRegistry): void;
    registerLanguageSupport?(provider: LanguageProvider): void;
    enhanceAgentPipeline?(pipeline: Pipeline): void;
    registerModelProvider?(provider: ModelProvider): void;
    registerTool?(tool: Tool): void;
}

export interface PluginContext {
    workspace: Workspace;
    editor: Editor;
    agentStore: AgentStore;
    logger: Logger;
}

export class PluginManager {
    private plugins: Map<string, Plugin> = new Map();
    private commandRegistry: CommandRegistry;
    private modelProviders: ModelProviderRegistry;

    async loadPlugin(pluginPath: string): Promise<void> {
        const plugin = await import(pluginPath);
        this.plugins.set(plugin.name, plugin);

        const context = this.createContext();
        plugin.onActivate?.(context);

        plugin.registerCommands?.(this.commandRegistry);
        plugin.registerModelProvider?.(this.modelProviders);
    }

    unloadPlugin(name: string): void {
        const plugin = this.plugins.get(name);
        if (plugin) {
            plugin.onDeactivate?.();
            this.plugins.delete(name);
        }
    }
}
```

**Example Plugin: Local Model Provider**

```typescript
export class StarCoderPlugin implements Plugin {
    name = 'starcoder-provider';
    version = '1.0.0';

    onActivate(context: PluginContext): void {
        const provider = new StarCoderProvider({
            modelPath: '~/.agent-ide/models/starcoder',
            maxTokens: 4096,
        });
        context.modelProviders.register(provider);
    }

    registerModelProvider(registry: ModelProviderRegistry): void {
        registry.register(new StarCoderProvider());
    }
}
```

### 10.5 Performance Profiling Tools

Agent IDE includes comprehensive performance monitoring inspired by Zed's profiling capabilities:

```rust
pub struct PerformanceProfiler {
    flamegraph: FlamegraphRecorder,
    frame_times: Vec<Duration>,
    memory_tracker: MemoryTracker,
}

impl PerformanceProfiler {
    pub fn start_frame(&mut self) {
        let start = Instant::now();
        // Record frame start
    }

    pub fn end_frame(&mut self) {
        let duration = Instant::now() - self.frame_start;
        self.frame_times.push(duration);
        self.check_performance_regression();
    }

    pub fn profile_function<F, R>(&mut self, name: &str, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let start = Instant::now();
        let result = f();
        let duration = start.elapsed();
        self.flamegraph.record(name, duration);
        result
    }
}
```

**Performance Targets:**

- **Startup Time**: < 3 seconds
- **Memory Usage**: < 300MB idle, < 1GB with 10+ files
- **Editor Latency**: < 50ms input response
- **Frame Rate**: > 60fps during scrolling
- **Code Completion**: < 200ms response time

### 10.6 Chinese Optimization Strategy

Inspired by **Comate's** Chinese developer experience, Agent IDE includes Chinese-specific optimizations:

```typescript
export class ChinesePromptOptimizer {
    optimizeCodeCompletion(context: CodeContext): string {
        return `
基于以下中文编程上下文提供代码补全：

文件: ${context.file}
语言: ${context.language}

当前代码:
${context.surroundingCode}

项目结构:
${context.projectStructure.summary}

请提供符合以下要求的补全:
1. 遵循中文变量命名习惯 (拼音 vs 英文)
2. 考虑中文注释风格
3. 适配中文开发者的常用模式
4. 提供中英文双语注释建议
`;
    }

    optimizeErrorMessage(error: Error): string {
        return `
错误信息: ${error.message}
位置: ${error.location}

请用中文解释:
1. 错误的具体原因
2. 可能的解决方案
3. 预防类似错误的建议
`;
    }
}
```

### 10.7 Hybrid Model Strategy

Agent IDE implements intelligent model selection to balance cost, performance, and quality:

```rust
pub struct HybridModelStrategy {
    local_models: Vec<LocalModel>,
    cloud_models: Vec<CloudModel>,
    cost_tracker: CostTracker,
}

impl HybridModelStrategy {
    pub fn select_model_for_task(&self, task: &Task) -> ModelSelection {
        match task.task_type {
            TaskType::SimpleCompletion => {
                // Use fastest local model
                self.select_fastest_local_model(&task.language)
            }
            TaskType::CodeGeneration => {
                // Use balanced model
                self.select_balanced_model(&task.complexity)
            }
            TaskType::ComplexRefactoring => {
                // Use most capable cloud model
                self.select_cloud_model(&task.language, ModelCapability::High)
            }
            TaskType::OfflineTask => {
                // Must use local model
                self.select_best_local_model(&task.language)
            }
        }
    }

    pub fn estimate_cost(&self, task: &Task, model: &Model) -> CostEstimate {
        let tokens = self.estimate_tokens(task);
        let cost_per_token = model.cost_per_token;
        CostEstimate {
            estimated_tokens: tokens,
            estimated_cost: tokens as f64 * cost_per_token,
            latency: model.estimated_latency,
        }
    }
}
```

### 10.8 Implementation Benefits

These Phase 9 improvements provide:

1. **Performance**: Zed-inspired incremental rendering enables handling large files smoothly
2. **Intelligence**: Comate-style context-aware completion improves developer productivity
3. **Cost**: Open-source models reduce dependency on expensive cloud APIs
4. **Privacy**: Local model support keeps code on developer machines
5. **Extensibility**: Plugin architecture allows community contributions
6. **Localization**: Chinese optimization improves experience for Chinese developers
7. **Reliability**: Hybrid strategy provides fallback options when cloud services fail

---

## 11. Future Enhancements Beyond Phase 9

### 11.1 Real-time Collaboration (Zed-inspired)

- Multi-user editing sessions
- Conflict resolution UI
- Shareable workspace states
- Agent collaboration between users

### 11.2 Advanced Code Analysis

- Static analysis integration
- Security vulnerability scanning
- Performance bottleneck detection
- Code smell identification

### 11.3 Ecosystem Integration

- Package manager integration (npm, cargo, pip)
- CI/CD pipeline visualization
- Project template marketplace
- Community plugin repository

### 11.4 Cross-Platform Mobile Support

- Mobile-optimized interface
- Touch gestures for code editing
- Cloud sync for workspace state
- Offline mode with local models

---

## 12. Source of Truth Policy

Use the documents as follows:

- `ROADMAP.md`: current implementation state, known issues, next tasks, and strategic direction including performance optimization and open-source model integration.
- `docs/agent_ide_design.md`: detailed technical design including incremental rendering, intelligent completion, and plugin architecture.
- `docs/agent_ide_ui_design.md`: product/UI target and design intent.
- `docs/agent_ide_plan.md`: original technical plan; useful historically, but should be refreshed when major implementation milestones land.
