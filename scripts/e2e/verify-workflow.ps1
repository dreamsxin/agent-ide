param(
  [switch]$SkipE2E
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path

Push-Location $Root
try {
  npm run build
  npm test
  Push-Location (Join-Path $Root "src-tauri")
  try {
    cargo check
    cargo test
  } finally {
    Pop-Location
  }
  if (-not $SkipE2E) {
    npm run e2e:workflow
  }
} finally {
  Pop-Location
}
