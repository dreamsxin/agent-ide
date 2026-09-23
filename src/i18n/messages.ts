/**
 * 界面文案表。英文是基准，中文必须一条不缺 —— 缺一条是**编译错误**，不是悄悄退回英文。
 *
 * 为什么不用 i18next / react-intl：这里要的是两种语言、一层命名空间、零运行时依赖。
 * 那两个库带来的是插件体系、复数规则引擎和一个 provider 树，而代价是把"缺翻译"从
 * 类型错误变成运行时回退 —— 半中半英的界面就是这么来的（这个项目已经有过一次：
 * `ModeSwitch` 的按钮是英文、tooltip 是中文）。
 *
 * 键名按界面区域分段。值里的 `{name}` 由 `translate` 替换。
 */
export const EN = {
  "language.toggle": "Switch to Chinese",
  "language.short": "中",

  "mode.suggest": "Suggest",
  "mode.suggest.desc": "Changes wait in the review area",
  "mode.auto": "Auto",
  "mode.auto.desc": "Changes are written to disk, with undo",
  "mode.group": "Agent mode",

  "topbar.openFolder": "Open Folder (Ctrl+O)",
  "topbar.noFolder": "No folder opened",
  "topbar.run": "Run",
  "topbar.debug": "Debug",
  "topbar.build": "Build",
  "topbar.building": "Building...",
  "topbar.test": "Test",
  "topbar.testing": "Testing...",
  "topbar.noTask": "No {kind} task discovered",
  "topbar.ideMode.code": "Code",
  "topbar.ideMode.plan": "Plan",
  "topbar.ideMode.code.title": "Code mode: edit files and talk to the Agent",
  "topbar.ideMode.plan.title": "Plan mode: turn a goal into steps before any file changes",
  "topbar.stop": "Stop",
  "topbar.stop.title": "Stop the Agent",
  "topbar.toggleExplorer": "Toggle Explorer (Ctrl+Shift+E)",
  "topbar.toggleAgent": "Toggle Agent Panel (Ctrl+Shift+X)",
  "topbar.toggleTerminal": "Toggle Terminal (Ctrl+`)",
  "topbar.focusMode": "Focus Mode (Ctrl+Shift+F)",
  "topbar.theme.toLight": "Switch to Light Theme",
  "topbar.theme.toDark": "Switch to Dark Theme",
  "topbar.commandPalette": "Command Palette (Ctrl+Shift+P)",
  "topbar.shortcuts": "Keyboard Shortcuts (F1)",
  "topbar.minimize": "Minimize",
  "topbar.maximize": "Maximize",
  "topbar.restore": "Restore",
  "topbar.close": "Close",
  "topbar.openWorkspaceDialog": "Open Workspace Folder",

  "lsp.title": "Language Server",
  "lsp.close": "Close",
  "lsp.closeDetails": "Close language server details",
  "lsp.status": "Status",
  "lsp.message": "Message",
  "lsp.server": "Server",
  "lsp.source": "Source",
  "lsp.workspace": "Workspace",
  "lsp.indexing": "Indexing",
  "lsp.config": "Config",
  "lsp.noConfig": "No tsconfig/jsconfig/package.json detected",
  "lsp.noIndexingDetails": "No indexing details.",
  "lsp.unknown": "unknown",
  "lsp.opened": "Opened",
  "lsp.changes": "Changes",
  "lsp.diagnostics": "Diagnostics",
  "lsp.install": "Install",
  "lsp.recentDiagnostics": "Recent diagnostics",
  "lsp.noDiagnostics": "No diagnostics received yet.",
} as const;

export type MessageKey = keyof typeof EN;

/**
 * 中文文案。类型是 `Record<MessageKey, string>` 而不是另一张自由的表：漏一条键
 * `tsc` 就红，于是"半中半英"这个状态在类型层面不存在。
 */
export const ZH: Record<MessageKey, string> = {
  "language.toggle": "切换到英文",
  "language.short": "EN",

  "mode.suggest": "待审查",
  "mode.suggest.desc": "改动先进审查区，你点了才落盘",
  "mode.auto": "自动",
  "mode.auto.desc": "改动直接落盘，可以一键撤销",
  "mode.group": "Agent 模式",

  "topbar.openFolder": "打开文件夹（Ctrl+O）",
  "topbar.noFolder": "还没打开文件夹",
  "topbar.run": "运行",
  "topbar.debug": "调试",
  "topbar.build": "构建",
  "topbar.building": "构建中…",
  "topbar.test": "测试",
  "topbar.testing": "测试中…",
  "topbar.noTask": "项目里没找到{kind}命令",
  "topbar.ideMode.code": "写代码",
  "topbar.ideMode.plan": "先做计划",
  "topbar.ideMode.code.title": "写代码模式：直接改文件、和 Agent 对话",
  "topbar.ideMode.plan.title": "计划模式：先把目标拆成步骤，不动文件",
  "topbar.stop": "停止",
  "topbar.stop.title": "停止 Agent",
  "topbar.toggleExplorer": "显示/隐藏文件树（Ctrl+Shift+E）",
  "topbar.toggleAgent": "显示/隐藏 Agent 面板（Ctrl+Shift+X）",
  "topbar.toggleTerminal": "显示/隐藏终端（Ctrl+`）",
  "topbar.focusMode": "专注模式（Ctrl+Shift+F）",
  "topbar.theme.toLight": "切换到浅色主题",
  "topbar.theme.toDark": "切换到深色主题",
  "topbar.commandPalette": "命令面板（Ctrl+Shift+P）",
  "topbar.shortcuts": "快捷键（F1）",
  "topbar.minimize": "最小化",
  "topbar.maximize": "最大化",
  "topbar.restore": "还原",
  "topbar.close": "关闭",
  "topbar.openWorkspaceDialog": "选择工作区文件夹",

  "lsp.title": "语言服务",
  "lsp.close": "关闭",
  "lsp.closeDetails": "关闭语言服务详情",
  "lsp.status": "状态",
  "lsp.message": "说明",
  "lsp.server": "服务程序",
  "lsp.source": "来源",
  "lsp.workspace": "工作区",
  "lsp.indexing": "索引",
  "lsp.config": "配置",
  "lsp.noConfig": "没有找到 tsconfig / jsconfig / package.json",
  "lsp.noIndexingDetails": "没有索引信息。",
  "lsp.unknown": "未知",
  "lsp.opened": "已打开",
  "lsp.changes": "改动数",
  "lsp.diagnostics": "诊断数",
  "lsp.install": "安装",
  "lsp.recentDiagnostics": "最近的诊断",
  "lsp.noDiagnostics": "还没有收到诊断信息。",
};

