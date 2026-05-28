param(
  [string]$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path,
  [string]$OutDir = (Join-Path $Root "artifacts\e2e\workflow")
)

$ErrorActionPreference = "Stop"

function Write-JsonFile($Path, $Value) {
  $dir = Split-Path -Parent $Path
  if ($dir) {
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
  }
  $Value | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $Path -Encoding UTF8
}

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$runDir = Join-Path $OutDir $timestamp
$workspace = Join-Path $runDir "workspace"
$config = Join-Path $runDir "config"
New-Item -ItemType Directory -Force -Path $runDir,$config | Out-Null

git -C $Root clone --quiet --local --no-hardlinks $Root $workspace
git -C $workspace config user.email "agent-ide-e2e@example.test"
git -C $workspace config user.name "Agent IDE E2E"

Set-Content -LiteralPath (Join-Path $workspace "smoke.txt") -Encoding UTF8 -Value "broken"
@'
const fs = require("fs");
const value = fs.readFileSync("smoke.txt", "utf8").trim();
if (value !== "fixed") {
  console.error("smoke.txt:1:1: error: workflow smoke expected fixed");
  process.exit(1);
}
console.log("workflow smoke passed");
'@ | Set-Content -LiteralPath (Join-Path $workspace "workflow-smoke.cjs") -Encoding UTF8

$packagePath = Join-Path $workspace "package.json"
$packageJson = Get-Content -Raw -LiteralPath $packagePath | ConvertFrom-Json
if (-not $packageJson.scripts) {
  $packageJson | Add-Member -MemberType NoteProperty -Name scripts -Value ([pscustomobject]@{})
}
$packageJson.scripts | Add-Member -MemberType NoteProperty -Name workflow -Value "node workflow-smoke.cjs" -Force
Write-JsonFile $packagePath $packageJson

Write-JsonFile (Join-Path $config "workspace.json") @{ path = $workspace }
Write-JsonFile (Join-Path $config "config.json") @{
  profiles = @(
    @{
      id = "default"
      name = "E2E Mock"
      provider = "custom"
      endpoint = "mock://workflow"
      api_key = "sk-e2e"
      model = "mock-e2e"
      toolCallMode = "text_protocol"
    }
  )
  active_profile_id = "default"
  context_compression = "focused"
}

$editorSession = @{
  workspacePath = $workspace
  openFiles = @(
    @{
      path = (Join-Path $workspace "smoke.txt")
      name = "smoke.txt"
      language = "plaintext"
      isDirty = $false
    }
  )
  activeFile = (Join-Path $workspace "smoke.txt")
}
$bootstrap = @"
localStorage.setItem("agent-ide-workspace-path", $(ConvertTo-Json $workspace));
localStorage.setItem("agent-ide-editor-session", $(ConvertTo-Json ($editorSession | ConvertTo-Json -Depth 20)));
"@
$bootstrap | Set-Content -LiteralPath (Join-Path $runDir "browser-bootstrap.js") -Encoding UTF8

$metadata = @{
  runDir = $runDir
  workspace = $workspace
  configDir = $config
  bootstrap = (Join-Path $runDir "browser-bootstrap.js")
}
Write-JsonFile (Join-Path $runDir "metadata.json") $metadata
$metadata | ConvertTo-Json -Depth 10 -Compress
