# Agent IDE

代码优先、可控、可审计的 AI Agent IDE。项目基于 Tauri v2、Rust、React、TypeScript、Tailwind CSS、Monaco Editor 和 xterm.js 构建。

Agent IDE 的目标不是做一个聊天式代码工具，而是把 Agent 放进 IDE 的核心工作流中：用户可以看到计划、角色流水线、Diff 审查、日志、Git 状态和终端执行过程，并始终保留控制权。

![Agent IDE screenshot](docs/screen-01.png)

## 当前状态

Phase 7 已完成功能收口。Phase 8 正在推进“可稳定替代 IDE 日常使用”的运行时打磨。

能力快照：

- 桌面 IDE 壳：Monaco 编辑器、Explorer、Git、Terminal、Problems、Logs、Commands 和 Agent 面板。
- Agent 闭环：角色流水线、可编辑 Plan、上下文预览/预算、结构化 action log、`agent-changes` 协议、Diff 审查/应用/重新生成。
- 语义与运行闭环：TypeScript/JavaScript 和 Go LSP 第一版、diagnostics 到 Problems/editor markers、项目命令 Run History、terminal failure 上下文进入 Agent repair。
- 自动化与发布：headless `agent_cli` 第一版和 Windows packaging workflow。

详细实现状态、缺口和下一步任务以 [ROADMAP.md](ROADMAP.md) 为准。设计和协议文档见 [docs/agent_ide_design.md](docs/agent_ide_design.md)、[docs/agent_changes_schema.md](docs/agent_changes_schema.md) 和 [docs/smoke_test.md](docs/smoke_test.md)。

## 运行模式

项目有两种开发运行方式。

```powershell
npm run dev
```

只运行 Vite Web 预览。Tauri IPC、文件系统、终端、Git 和 Agent 后端功能会被禁用或通过 runtime guard 保护。

```powershell
npm run tauri -- dev
```

运行真实桌面 IDE，包含 Rust 后端和 Tauri API。

## 环境准备

需要：

- Node.js 和 npm
- Rust toolchain
- 当前系统所需的 Tauri v2 依赖

安装依赖：

```powershell
npm install
```

运行 Web 预览：

```powershell
npm run dev
```

运行桌面应用：

```powershell
npm run tauri -- dev
```

## 验证命令

提交较大改动前运行：

```powershell
npm run build
npm test
cd src-tauri
cargo check
cargo test
```

已知情况：Vite 目前会提示前端 chunk 较大，因为 Monaco、Markdown、xterm 和语法高亮工具打在一起。这不是正确性失败，后续需要做 code splitting。

如果改动涉及 LSP、Problems、Terminal、Git 或 Agent diff application，还需要按 [docs/smoke_test.md](docs/smoke_test.md) 执行真实 Tauri runtime 回归。

## Windows 打包

生成 Windows 安装包：

```powershell
npm run package:windows
```

脚本会依次执行前端 build/tests、`cargo check`、`cargo test` 和 `tauri build --bundles nsis,msi`，然后把安装包复制到 `release/windows/<version>/`，并生成 `SHA256SUMS.txt` 和 `manifest.json`。

如果本地已经跑过检查，只想快速验证打包：

```powershell
npm run package:windows:fast
```

只生成某一种安装包格式：

```powershell
npm run package:windows:nsis
npm run package:windows:msi
```

第一次 Windows bundle 可能需要通过 Tauri 下载 NSIS、`nsis_tauri_utils.dll` 和/或 WiX 工具链。如果本地下载工具时超时，可以在工具缓存完成后重跑命令，或者运行 `Windows Package` GitHub Actions workflow；该 workflow 会在 `windows-latest` 上构建并上传 artifacts。

生成的 release artifacts 已加入 `.gitignore`，不会进入 Git。

## 项目结构

```text
src/
  components/
    agent/       Agent chat、task、diff、pipeline、settings UI
    editor/      Monaco editor、tabs、overlays、quick actions
    layout/      顶部、左右、底部布局面板
    panels/      Explorer、Git、Terminal、Logs
  hooks/         Tauri event bridge 和快捷键
  stores/        Zustand stores
  types/         前端 DTO 类型
  utils/         Tauri runtime helper

src-tauri/
  src/
    agent/       planner、executor、orchestrator、diff apply、roles
    commands/    fs/git/terminal/agent 的 Tauri IPC 命令
    services/    workspace、context、LLM client
    bin/         agent_cli

docs/
  agent_ide_design.md      当前详细设计
  agent_cli_manual.md      CLI 模式使用说明和限制
  agent_cli_design.md      CLI 自动化和工具链集成目标设计
  agent_ide_plan.md        原始技术计划
  agent_ide_ui_design.md   产品 UI 目标设计
```

## Agent 工作流

Agent IDE 以 Chat 作为用户入口，但 Agent 不是单轮自由聊天，而是由 IDE runtime 调度的可审计执行链路。

```text
Chat prompt
  -> ChatView 收集 prompt、active file、selection 和附加上下文文件
  -> useAgentStore.sendPrompt() 通过 Tauri IPC 调用 send_agent_prompt
  -> commands/agent.rs 构建 AgentContext 并读取当前 pipeline 配置
  -> services/context.rs 补充并压缩上下文
  -> agent/orchestrator.rs 运行 Agent 状态机
  -> planner 生成任务步骤
  -> role pipeline 按配置执行阶段
     -> architect
     -> coder
     -> tester
     -> reviewer
  -> executor 通过 services/llm_client.rs 流式调用模型
  -> diff parser 将模型输出转换为 pending diffs
  -> reviewer 接收实际 pending diff 摘要进行审查
  -> useAgentBridge 接收后端事件并刷新 Chat/Tasks/Pipeline/Diff/Logs
  -> 用户通过 commands/agent.rs 和 agent/diff_apply.rs apply/reject diffs
```

主要调度模块：

| 层级 | 模块 | 职责 |
|------|------|------|
| UI | `src/components/agent/*` | Chat 输入、任务视图、流水线视图、Diff 审查、设置。 |
| 前端状态 | `src/stores/useAgentStore.ts` | Agent 状态、IPC 调用、消息、步骤、diff、pipeline 配置。 |
| 事件桥接 | `src/hooks/useAgentBridge.ts` | 监听后端事件并写入 Zustand stores。 |
| IPC 边界 | `src-tauri/src/commands/agent.rs` | 校验请求、构建上下文、启动/停止 Agent、应用/拒绝 diff。 |
| 上下文 | `src-tauri/src/services/context.rs` | 组合 active file、selection、open files、project tree、Git diff 和压缩模式。 |
| 编排 | `src-tauri/src/agent/orchestrator.rs` | 运行 planner、角色阶段、reviewer、action log 和状态转换。 |
| 角色执行 | `src-tauri/src/agent/executor.rs` | 构造角色提示词，调用 LLM 并流式返回。 |
| LLM | `src-tauri/src/services/llm_client.rs` | OpenAI 兼容的流式 chat client。 |
| Diff 应用 | `src-tauri/src/agent/diff_apply.rs` | 在 workspace 边界内应用可审查文件改动。 |

上下文压缩模式在 Chat 中按本次运行选择：

| 模式 | 用途 |
|------|------|
| `focused` | 默认实用模式：selection、active-file excerpt、project summary、Git diff。 |
| `compact` | 低 token 模式：用 outline 和 metadata 表达更宽的上下文。 |
| `budgeted` | 按 provider profile 的上下文预算做 token-aware packing；没有模型预算时使用安全默认值。 |
| `full` | 高保真模式：尽量包含完整 active context，适合准确性优先的任务。 |

Agent 事件会流式回传给前端和 action log：

- `agent-state-changed`
- `agent-stream-token`
- `agent-plan-ready`
- `agent-step-update`
- `agent-pipeline-update`
- `agent-diff-ready`
- `agent-action-log`

完整设计见 [docs/agent_ide_design.md](docs/agent_ide_design.md)，重点阅读 4.3 Agent Prompt、4.4 Agent Pipeline、5 Context Model、6 Agent Modes and Safety。结构化变更协议见 [docs/agent_changes_schema.md](docs/agent_changes_schema.md)。

## Agent Change Protocol

推荐的结构化输出：

````text
```agent-changes
{
  "version": 1,
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
  ],
  "findings": [
    {
      "severity": "warning",
      "file": "path/to/file",
      "hunkIndex": 0,
      "message": "optional reviewer finding tied to this hunk"
    }
  ]
}
```
````

旧的 `diff:path` 和 `new:path` 代码块仍然兼容。Schema 细节和校验行为见 [docs/agent_changes_schema.md](docs/agent_changes_schema.md)。

## 配置

LLM 配置可以通过 UI 或环境变量提供：

```powershell
$env:LLM_ENDPOINT = "https://api.openai.com/v1"
$env:LLM_API_KEY = "..."
$env:LLM_MODEL = "..."
```

当前本地配置默认保存在 `~/.agent-ide`，除非设置了 `AGENT_IDE_CONFIG_DIR`。

## CLI

Rust 侧包含一个 headless automation CLI：

```powershell
cd src-tauri
cargo build --bin agent_cli --release
target\release\agent_cli --help
```

CLI 模式已经完成 headless automation 第一版。它支持 `doctor`、`context estimate`、`plan`、`run` 和 `smoke ide-backend`，支持 text/JSON/NDJSON 输出、运行 artifacts、可选 apply、项目命令检查、有边界的 repair iterations、命令 allow-list、timeout/output/diff 限制，以及经过 smoke test 覆盖的 `project-tasks.json`、`problems.json`、`repair-chain.json` 和 `repair-summary.json` artifacts。

它有意不作为完整命令行 IDE 替代方案。可视化 Agent Plan 控制、Problems/Terminal/Git 闭环、LSP 视图、Run History 和 per-hunk review UI 仍然属于桌面 IDE 工作流。

使用方式、安全注意事项和当前完成度见 [docs/agent_cli_manual.md](docs/agent_cli_manual.md)。面向工具链集成和全自动执行的目标架构见 [docs/agent_cli_design.md](docs/agent_cli_design.md)。

## Git 注意事项

仓库中可能存在本地 demo 改动。提交前先检查：

```powershell
git status --short
```

不要把无关 demo/workspace 改动带进功能提交。
