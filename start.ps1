# Agent IDE 启动脚本 (PowerShell)
# 智能检测环境并选择合适的启动方式

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "Agent IDE Launcher" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host ""

# Function to check if command exists
function Test-CommandExists {
    param($Command)
    try {
        $null = Get-Command $Command -ErrorAction Stop
        return $true
    } catch {
        return $false
    }
}

# Check environment
Write-Host "Checking development environment..." -ForegroundColor Yellow

# Check Node.js
if (-not (Test-CommandExists "node")) {
    Write-Host "[Error] Node.js not installed" -ForegroundColor Red
    Write-Host "Please download and install Node.js from https://nodejs.org/" -ForegroundColor Yellow
    Read-Host "Press Enter to exit"
    exit 1
}

# Check Cargo
if (-not (Test-CommandExists "cargo")) {
    Write-Host "[Error] Rust/Cargo not found" -ForegroundColor Red
    Write-Host ""
    Write-Host "Please fix the Rust environment first:" -ForegroundColor Yellow
    Write-Host "1. Run: .\fix-environment.ps1 (recommended)" -ForegroundColor White
    Write-Host "2. Or: .\fix-environment.bat" -ForegroundColor White
    Write-Host "3. Or: See RUST_FIX_GUIDE.md for manual instructions" -ForegroundColor White
    Write-Host ""
    Write-Host "For web-only development, you can run: npm run dev" -ForegroundColor Yellow
    Write-Host ""
    Read-Host "Press Enter to exit"
    exit 1
}

# Environment is OK, start full application
Write-Host "[OK] Environment check passed" -ForegroundColor Green
Write-Host ""
Write-Host "Starting Agent IDE (full mode)..." -ForegroundColor Yellow
Write-Host ""

npm run tauri -- dev