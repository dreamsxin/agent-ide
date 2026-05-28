param(
  [int]$TimeoutSeconds = 180,
  [switch]$KeepApp
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$prep = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "prepare-workflow.ps1") -Root $Root
$metadata = $prep | Select-Object -Last 1 | ConvertFrom-Json
$runDir = [string]$metadata.runDir
$workspace = [string]$metadata.workspace
$configDir = [string]$metadata.configDir
$logDir = Join-Path $runDir "logs"
$shotDir = Join-Path $runDir "screenshots"
New-Item -ItemType Directory -Force -Path $logDir,$shotDir | Out-Null

Write-Host "Building frontend and Tauri debug app for workflow E2E..."
Push-Location $Root
try {
  npm run build
  Push-Location (Join-Path $Root "src-tauri")
  try {
    cargo build
  } finally {
    Pop-Location
  }
} finally {
  Pop-Location
}

$appPath = Join-Path $Root "src-tauri\target\debug\agent-ide.exe"
if (-not (Test-Path -LiteralPath $appPath)) {
  throw "Missing debug app at $appPath"
}

$env:AGENT_IDE_CONFIG_DIR = $configDir
$env:AGENT_IDE_E2E = "1"
$env:LLM_ENDPOINT = "mock://workflow"
$env:LLM_API_KEY = "sk-e2e"
$env:LLM_MODEL = "mock-e2e"

$stdout = Join-Path $logDir "app.stdout.log"
$stderr = Join-Path $logDir "app.stderr.log"
$app = Start-Process -FilePath $appPath -WorkingDirectory (Join-Path $Root "src-tauri") -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr

try {
  & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "workflow-controller.ps1") `
    -ProcessId $app.Id `
    -Workspace $workspace `
    -RunDir $runDir `
    -TimeoutSeconds $TimeoutSeconds
  if ($LASTEXITCODE -ne 0) {
    throw "Workflow controller failed with exit code $LASTEXITCODE"
  }
} finally {
  if (-not $KeepApp -and -not $app.HasExited) {
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
  }
}

Write-Host "Workflow E2E artifacts: $runDir"
