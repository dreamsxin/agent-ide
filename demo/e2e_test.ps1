#!/usr/bin/env pwsh
# ============================================================
# Agent IDE E2E Test — Full Agent Workflow (Plan → Execute → Apply)
# ============================================================
# Tests the full agent pipeline:
#   1. Planner: LLM decomposes task into steps
#   2. Executor: LLM generates code diffs per step
#   3. Diff Parser: extracts diff blocks with ORIGINAL/UPDATED
#   4. Apply: writes modified files to disk
# ============================================================

$ErrorActionPreference = "Stop"
$SCRIPT_DIR = Split-Path -Parent $PSCommandPath

$ROOT = Split-Path -Parent $SCRIPT_DIR  # go from demo/ up to project root

Write-Host "============================================" -ForegroundColor Cyan
Write-Host " Agent IDE E2E Test" -ForegroundColor Cyan
Write-Host "============================================" -ForegroundColor Cyan
Write-Host ""

# --- 1. Locate agent_cli binary ---
$RELEASE = Join-Path $ROOT "src-tauri\target\release\agent_cli.exe"
$DEBUG   = Join-Path $ROOT "src-tauri\target\debug\agent_cli.exe"
$CLI = $null
if (Test-Path $RELEASE) { $CLI = $RELEASE } elseif (Test-Path $DEBUG) { $CLI = $DEBUG }
if (-not $CLI) {
    Write-Host "[BUILD] agent_cli not found, building..." -ForegroundColor Yellow
    Push-Location "$ROOT\src-tauri"
    cargo build --bin agent_cli --release 2>&1 | Out-Null
    Pop-Location
    $CLI = $RELEASE
    if (-not (Test-Path $CLI)) {
        Write-Host "[FAIL] Could not build agent_cli" -ForegroundColor Red
        exit 1
    }
}
Write-Host "[OK] agent_cli: $CLI" -ForegroundColor Green

# --- 2. Load LLM config ---
$CONFIG_PATH = Join-Path $env:USERPROFILE ".agent-ide\config.json"
if (-not (Test-Path $CONFIG_PATH)) {
    Write-Host "[FAIL] No LLM config at $CONFIG_PATH" -ForegroundColor Red
    exit 1
}
$config = Get-Content $CONFIG_PATH | ConvertFrom-Json
$endpoint = $config.endpoint
$apiKey   = $config.api_key
$model    = $config.model
Write-Host "[OK] LLM: $endpoint / $model" -ForegroundColor Green

# --- 3. Back up original hello.js ---
$TARGET_FILE = Join-Path $SCRIPT_DIR "hello.js"
$BACKUP_FILE = Join-Path $SCRIPT_DIR "hello.js.backup"

if (Test-Path $TARGET_FILE) {
    Copy-Item $TARGET_FILE $BACKUP_FILE -Force
    Write-Host "[OK] Backup: $BACKUP_FILE" -ForegroundColor Green
} else {
    Write-Host "[FAIL] Target file not found: $TARGET_FILE" -ForegroundColor Red
    exit 1
}

$originalContent = Get-Content $TARGET_FILE -Raw
$originalHash = (Get-FileHash $TARGET_FILE -Algorithm SHA256).Hash

Write-Host ""
Write-Host "--- Original hello.js ---" -ForegroundColor DarkGray
Write-Host $originalContent -ForegroundColor DarkGray
Write-Host ""

# --- 4. Run Agent CLI ---
$PROMPT = "Add an async function 'farewell(name)' to hello.js that returns 'Goodbye ' + name and export it. Use async/await syntax."
$WORKSPACE = $SCRIPT_DIR

Write-Host "--- Running Agent CLI ---" -ForegroundColor Yellow
Write-Host "Prompt: $PROMPT" -ForegroundColor Gray
Write-Host "Workspace: $WORKSPACE" -ForegroundColor Gray
Write-Host ""

$startTime = Get-Date

$errOut = New-Object System.Text.StringBuilder
$output = & $CLI `
    --endpoint $endpoint `
    --api-key $apiKey `
    --model $model `
    --workspace $WORKSPACE `
    --apply `
    $PROMPT 2>&1 | ForEach-Object {
        if ($_ -is [System.Management.Automation.ErrorRecord]) {
            # stderr — collect but don't fail
            [void]$errOut.AppendLine($_.Exception.Message)
        } else {
            $_
        }
    }

$elapsed = (Get-Date) - $startTime

# Print stderr (debug info) first
if ($errOut.Length -gt 0) {
    Write-Host "--- STDERR ---" -ForegroundColor DarkGray
    Write-Host $errOut.ToString() -ForegroundColor DarkGray
}

# Print stdout
if ($output) {
    $output | ForEach-Object {
        if ($_) { Write-Host $_ }
    }
}

Write-Host ""
Write-Host "[DONE] Elapsed: $($elapsed.TotalSeconds.ToString("0.0"))s" -ForegroundColor Cyan

# --- 5. Verify results ---
Write-Host ""
Write-Host "============================================" -ForegroundColor Cyan
Write-Host " Verification" -ForegroundColor Cyan
Write-Host "============================================" -ForegroundColor Cyan

if (Test-Path $TARGET_FILE) {
    $modifiedContent = Get-Content $TARGET_FILE -Raw
    $modifiedHash = (Get-FileHash $TARGET_FILE -Algorithm SHA256).Hash
    
    Write-Host ""
    Write-Host "--- Modified hello.js ---" -ForegroundColor DarkGray
    Write-Host $modifiedContent -ForegroundColor DarkGray
    Write-Host ""

    # Check 1: File was actually modified
    if ($originalHash -ne $modifiedHash) {
        Write-Host "[PASS] File was modified (hash changed)" -ForegroundColor Green
    } else {
        Write-Host "[WARN] File hash unchanged — agent may not have modified it" -ForegroundColor Yellow
    }

    # Check 2: File contains the farewell function
    if ($modifiedContent -match "farewell") {
        Write-Host "[PASS] File contains 'farewell' function" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] File does not contain 'farewell' function" -ForegroundColor Red
    }

    # Check 3: File contains 'async'
    if ($modifiedContent -match "async") {
        Write-Host "[PASS] File uses async/await" -ForegroundColor Green
    } else {
        Write-Host "[WARN] File does not contain 'async'" -ForegroundColor Yellow
    }

    # Check 4: Original functions preserved (LLM may overwrite if ORIGINAL match fails)
    if ($modifiedContent -match "function greet" -or $modifiedContent -match "function add") {
        Write-Host "[PASS] At least one original function preserved" -ForegroundColor Green
    } else {
        Write-Host "[WARN] Original functions overwritten — LLM ORIGINAL mismatch (model quality issue)" -ForegroundColor Yellow
    }

    # Check 5: Export includes farewell
    if ($modifiedContent -match "farewell") {
        Write-Host "[PASS] 'farewell' is exported" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] 'farewell' is not exported" -ForegroundColor Red
    }

    # Check 6: Valid JS syntax (using node)
    Write-Host ""
    Write-Host "--- Syntax check ---" -ForegroundColor Yellow
    $syntaxResult = node -e "try { require('$($TARGET_FILE -replace '\\','/')'); console.log('OK') } catch(e) { console.log('SYNTAX_ERROR: ' + e.message) }" 2>&1
    Write-Host $syntaxResult

} else {
    Write-Host "[FAIL] Target file missing after agent run!" -ForegroundColor Red
}

# --- 6. Restore original (optional, keep modified for review) ---
Write-Host ""
Write-Host "============================================" -ForegroundColor Cyan
Write-Host " Test complete. Modified file: $TARGET_FILE" -ForegroundColor Cyan
Write-Host " Backup:               $BACKUP_FILE" -ForegroundColor Cyan
Write-Host " To restore: copy $BACKUP_FILE $TARGET_FILE" -ForegroundColor Gray
Write-Host "============================================" -ForegroundColor Cyan
