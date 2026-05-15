# Agent IDE

代码优先、可控、可审计的 AI Agent IDE。项目基于 Tauri v2、Rust、React、TypeScript、Tailwind CSS、Monaco Editor 和 xterm.js 构建。

Agent IDE 的目标不是做一个聊天式代码工具，而是把 Agent 放进 IDE 的核心工作流中：用户可以看到计划、角色流水线、Diff 审查、日志、Git 状态和终端执行过程，并始终保留控制权。

## 当前状态

当前阶段：**Phase 7 - Agent execution quality and auditability**。

已实现的核心能力：

- Tauri 桌面壳，React/Vite 前端，Rust 后端。
- Monaco 编辑器、文件标签、文件树、Git 面板、Terminal 面板、Logs 面板和 Agent 面板。
- 工作区范围内的文件系统操作，并带路径边界检查。
- 基于 `git2` 的 Git status/diff/commit 命令。
- 基于 `portable-pty` 的 PTY 后端和 xterm.js 前端终端。
- OpenAI 兼容的流式 LLM 客户端。
- 角色化 Agent 流水线：planner -> architect -> coder -> tester -> reviewer。
- Agent 上下文压缩模式：`full`、`focused`、`compact`。
- Agent 上下文增强：项目树摘要和 Git working-tree diff。
- Logs 面板中可查看结构化 Agent action log。
- Diff 审查和应用流程，支持结构化失败信息。
- 兼容式 `agent-changes` JSON 协议，同时保留旧的 diff/new-file 代码块解析。
- Agent diff 支持可选 `baseHash`，用于拒绝基于过期文件内容的编辑。

重要缺口：

- Diff 应用还没有完整的 per-hunk apply/reject。
- `baseHash` 已支持，但 UI 还需要更完整地展示和操作。
- API key 仍保存在本地 JSON 配置中。
- Terminal 还需要在真实 Tauri runtime 中做更多交互测试。
- 前端测试和 Tauri smoke tests 仍然不足。

实现状态以 [ROADMAP.md](ROADMAP.md) 为准，详细设计见 [docs/agent_ide_design.md](docs/agent_ide_design.md)。

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
cd src-tauri
cargo check
cargo test
```

已知情况：Vite 目前会提示前端 chunk 较大，因为 Monaco、Markdown、xterm 和语法高亮工具打在一起。这不是正确性失败，后续需要做 code splitting。

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
  agent_ide_plan.md        原始技术计划
  agent_ide_ui_design.md   产品 UI 目标设计
```

## Agent 工作流

```text
Chat prompt
  -> 前端收集 active file、selection、open files
  -> 后端补充 project tree 和 Git diff
  -> planner 生成任务步骤
  -> pipeline 执行配置好的角色
     -> architect
     -> coder
     -> tester
     -> reviewer
  -> 模型输出被解析为 pending diffs
  -> reviewer 接收实际 pending diff 摘要进行审查
  -> 用户 apply 或 reject diffs
```

Agent 事件会流式回传给前端：

- `agent-state-changed`
- `agent-stream-token`
- `agent-plan-ready`
- `agent-step-update`
- `agent-pipeline-update`
- `agent-diff-ready`
- `agent-action-log`

## Agent Change Protocol

推荐的结构化输出：

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

旧的 `diff:path` 和 `new:path` 代码块仍然兼容。

## 配置

LLM 配置可以通过 UI 或环境变量提供：

```powershell
$env:LLM_ENDPOINT = "https://api.openai.com/v1"
$env:LLM_API_KEY = "..."
$env:LLM_MODEL = "..."
```

当前本地配置默认保存在 `~/.agent-ide`，除非设置了 `AGENT_IDE_CONFIG_DIR`。

## CLI

Rust 侧包含一个预览/应用模式的 CLI：

```powershell
cd src-tauri
cargo build --bin agent_cli --release
target\release\agent_cli --help
```

## Git 注意事项

仓库中可能存在本地 demo 改动。提交前先检查：

```powershell
git status --short
```

不要把无关 demo/workspace 改动带进功能提交。

