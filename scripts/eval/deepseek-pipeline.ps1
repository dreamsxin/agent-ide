# 用真实 provider（DeepSeek）跑一次流水线，把产出留下来供人工判断。
#
# 存在的理由：mock provider 能验证消息流对不对，但验证不了产出好不好。
# 提示词结构一改（9.0.11 就要改），"变差"这件事只有真实模型加人眼能看出来。
# 这个脚本把那次评测变成一条可重复的命令，而不是每次临时拼参数。
#
# 默认是 **preview**：不传 --apply，所以工作区不会被改动，一次运行只花一次
# 模型调用的钱。要评测修复循环之类需要落盘的行为时再显式加 -Apply。
#
# 用法：
#   $env:DEEPSEEK_API_KEY = "<key>"
#   pwsh scripts/eval/deepseek-pipeline.ps1
#   pwsh scripts/eval/deepseek-pipeline.ps1 -DryRun        # 只看要跑什么命令
#   pwsh scripts/eval/deepseek-pipeline.ps1 -Task "把 greet 改成返回大写"

[CmdletBinding()]
param(
    [string]$ApiKey = $env:DEEPSEEK_API_KEY,
    [string]$Endpoint = "https://api.deepseek.com",
    [string]$Model = "deepseek-chat",
    [string]$Task = "Rename the greet helper to greeting and update its callers.",
    [switch]$Apply,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

if (-not $DryRun -and [string]::IsNullOrWhiteSpace($ApiKey)) {
    # 不去别处翻找凭据：要用哪个 key 由调用者显式决定
    Write-Error "缺少 API key。设置 DEEPSEEK_API_KEY 或传 -ApiKey；只想看命令的话加 -DryRun。"
    exit 2
}

# 每次评测用一个干净的临时工作区：真实模型会改文件，不能让它碰这个仓库
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$workspace = Join-Path ([System.IO.Path]::GetTempPath()) "agent-ide-eval-$stamp"
$artifacts = Join-Path $workspace "artifacts"
New-Item -ItemType Directory -Path (Join-Path $workspace "src") -Force | Out-Null

# 一个小到能人工核对、又真的需要改两处的任务
Set-Content -Path (Join-Path $workspace "src/greet.ts") -Value @'
export function greet(name: string): string {
  return `Hello, ${name}`;
}
'@
Set-Content -Path (Join-Path $workspace "src/main.ts") -Value @'
import { greet } from "./greet";

console.log(greet("world"));
'@

$cliArgs = @(
    "run", "--quiet", "--bin", "agent_cli", "--",
    "run",
    "--workspace", $workspace,
    "--artifact-dir", $artifacts,
    "--endpoint", $Endpoint,
    "--model", $Model,
    "--output", "json"
)
if ($Apply) { $cliArgs += "--apply" }

# 选项一律排在任务前面。任务是可变长位置参数（`prompt: Vec<String>`，
# `num_args = 0..`），clap 目前仍会把它后面的 `--api-key` 认成选项，但这种顺序
# 依赖没必要留着 —— 一旦哪天加上 trailing_var_arg，key 就会被并进 prompt 发给模型。

if ($DryRun) {
    # 打印时用占位符：终端记录和 CI 日志里不该出现真值
    Write-Host "workspace: $workspace"
    Write-Host ("cargo " + (($cliArgs + @("--api-key", "<redacted>", $Task)) -join " "))
    exit 0
}
$cliArgs += @("--api-key", $ApiKey, $Task)

Push-Location (Join-Path $repoRoot "src-tauri")
try {
    & cargo @cliArgs
    $exit = $LASTEXITCODE
}
finally {
    Pop-Location
}


Write-Host ""
Write-Host "exit code: $exit"
Write-Host "artifacts: $artifacts"
Write-Host "看这几份来判断产出质量："
Write-Host "  prompt.txt    发出去的任务提示"
Write-Host "  context.txt   实际打包进请求的上下文"
Write-Host "  changes.json  模型提议的 diff（preview 模式下未落盘）"
Write-Host "  summary.json  状态、退出码、用量"
exit $exit
