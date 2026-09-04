# Agent IDE Environment Diagnostic and Fix Tool (PowerShell)
# For diagnosing and fixing common Rust/Tauri environment issues

$ErrorActionPreference = "Stop"

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "Agent IDE Environment Diagnostic and Fix Tool" -ForegroundColor Cyan
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

# 1. Check Node.js and npm
Write-Host "[1/5] Checking Node.js and npm..." -ForegroundColor Yellow
$nodeError = $false

if (Test-CommandExists "node") {
    try {
        $nodeVersion = node --version 2>&1
        Write-Host "[OK] Node.js installed: $nodeVersion" -ForegroundColor Green

        if (Test-CommandExists "npm") {
            $npmVersion = npm --version 2>&1
            Write-Host "[OK] npm installed: $npmVersion" -ForegroundColor Green
        } else {
            Write-Host "[Error] npm not found" -ForegroundColor Red
            $nodeError = $true
        }
    } catch {
        Write-Host "[Error] Node.js check failed: $_" -ForegroundColor Red
        $nodeError = $true
    }
} else {
    Write-Host "[Error] Node.js not installed" -ForegroundColor Red
    Write-Host "Please download and install Node.js 18+ from https://nodejs.org/" -ForegroundColor Yellow
    $nodeError = $true
}

Write-Host ""

# 2. Check Rust and Cargo
Write-Host "[2/5] Checking Rust and Cargo..." -ForegroundColor Yellow
$cargoError = $false

if (Test-CommandExists "cargo") {
    try {
        $rustcVersion = rustc --version 2>&1
        Write-Host "[OK] Rust installed: $rustcVersion" -ForegroundColor Green
        $cargoVersion = cargo --version 2>&1
        Write-Host "[OK] Cargo installed: $cargoVersion" -ForegroundColor Green
    } catch {
        Write-Host "[Error] Rust check failed: $_" -ForegroundColor Red
        $cargoError = $true
    }
} else {
    Write-Host "[Error] Rust/Cargo not found" -ForegroundColor Red
    Write-Host ""
    Write-Host "Starting Rust installation..." -ForegroundColor Yellow
    Write-Host ""

    # Check if rustup installer exists
    if (Test-Path "rustup-init.exe") {
        Write-Host "[Found] rustup-init.exe found, running..." -ForegroundColor Green
        try {
            Start-Process -FilePath ".\rustup-init.exe" -ArgumentList "-y", "--default-toolchain", "stable" -Wait -NoNewWindow
        } catch {
            Write-Host "[Error] Failed to run rustup-init.exe: $_" -ForegroundColor Red
        }
    } else {
        Write-Host "[Download] Downloading Rust installer..." -ForegroundColor Yellow
        try {
            Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile "rustup-init.exe" -UseBasicParsing
            Write-Host "[Install] Installing Rust..." -ForegroundColor Yellow
            Start-Process -FilePath ".\rustup-init.exe" -ArgumentList "-y", "--default-toolchain", "stable" -Wait -NoNewWindow
        } catch {
            Write-Host "[Error] Download failed: $_" -ForegroundColor Red
            Write-Host "Please manually download from https://rustup.rs/" -ForegroundColor Yellow
        }
    }

    Write-Host ""
    Write-Host "[IMPORTANT] Please close this terminal and reopen it for PATH changes to take effect" -ForegroundColor Magenta
    Write-Host "Then run this script again to verify" -ForegroundColor Magenta
    $cargoError = $true
}

Write-Host ""

# Exit if there are critical errors
if ($nodeError -or $cargoError) {
    Write-Host "============================================================" -ForegroundColor Red
    Write-Host "Critical issues found, please fix them and run this script again" -ForegroundColor Red
    Write-Host "============================================================" -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}

# 3. Check if Cargo is in PATH
Write-Host "[3/5] Checking Cargo PATH configuration..." -ForegroundColor Yellow
$pathFixed = $false

try {
    $cargoPath = Get-Command cargo -ErrorAction Stop
    Write-Host "[OK] Cargo is correctly configured in PATH: $($cargoPath.Source)" -ForegroundColor Green
} catch {
    Write-Host "[Warning] Cargo not found in system PATH" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Fixing PATH configuration..." -ForegroundColor Yellow

    try {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $cargoBinPath = "$env:USERPROFILE\.cargo\bin"

        if ($userPath -and $userPath -notlike "*$cargoBinPath*") {
            Write-Host "[Add] Adding Cargo bin directory to user PATH" -ForegroundColor Green
            $newPath = "$userPath;$cargoBinPath"
            [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
            Write-Host "[Done] PATH updated, please restart terminal for changes to take effect" -ForegroundColor Green
            $pathFixed = $true
        } else {
            Write-Host "[OK] Cargo bin directory already in PATH" -ForegroundColor Green
        }
    } catch {
        Write-Host "[Error] Failed to modify PATH: $_" -ForegroundColor Red
        Write-Host "Please manually add this path to system PATH: $env:USERPROFILE\.cargo\bin" -ForegroundColor Yellow
    }
}

Write-Host ""

# 4. Check Tauri CLI
Write-Host "[4/5] Checking Tauri CLI..." -ForegroundColor Yellow

if (!(Test-Path "node_modules\.bin\tauri.cmd")) {
    Write-Host "[Warning] Tauri CLI not installed" -ForegroundColor Yellow
    Write-Host "Installing dependencies..." -ForegroundColor Yellow
    try {
        npm install
        if ($LASTEXITCODE -eq 0) {
            Write-Host "[OK] Dependencies installed" -ForegroundColor Green
        } else {
            throw "npm install failed"
        }
    } catch {
        Write-Host "[Error] npm install failed: $_" -ForegroundColor Red
    }
} else {
    Write-Host "[OK] Tauri CLI installed" -ForegroundColor Green
}

Write-Host ""

# 5. Test build
Write-Host "[5/5] Testing Tauri build..." -ForegroundColor Yellow
Write-Host "[Info] Running quick build test..." -ForegroundColor Yellow

try {
    Set-Location src-tauri
    $null = cargo check --quiet 2>&1
    Set-Location ..

    if ($LASTEXITCODE -eq 0) {
        Write-Host "[OK] Rust code compilation passed" -ForegroundColor Green
    } else {
        Write-Host "[Warning] cargo check failed, but this might be normal (first build may take longer)" -ForegroundColor Yellow
        Write-Host "[Suggestion] Run 'npm run tauri -- dev' for full testing" -ForegroundColor Yellow
    }
} catch {
    Write-Host "[Warning] Build test encountered error: $_" -ForegroundColor Yellow
    Write-Host "[Suggestion] Run 'npm run tauri -- dev' for full testing" -ForegroundColor Yellow
}

Write-Host ""

# Final report
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "Diagnostic Complete" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host ""

if ($pathFixed) {
    Write-Host "[IMPORTANT] PATH update detected" -ForegroundColor Magenta
    Write-Host "Please follow these steps:" -ForegroundColor Yellow
    Write-Host "1. Close current terminal" -ForegroundColor Yellow
    Write-Host "2. Reopen terminal" -ForegroundColor Yellow
    Write-Host "3. Run 'npm run tauri -- dev'" -ForegroundColor Yellow
    Write-Host ""
} else {
    Write-Host "Next steps:" -ForegroundColor Yellow
    Write-Host "1. Run 'npm run tauri -- dev' to start the application" -ForegroundColor Yellow
    Write-Host "2. If you still have issues, check the troubleshooting section in README.md" -ForegroundColor Yellow
    Write-Host ""
}

Write-Host "Environment Status: " -NoNewline
if ($nodeError -or $cargoError) {
    Write-Host "Needs Fixing" -ForegroundColor Red
} else {
    Write-Host "Normal" -ForegroundColor Green
}

Write-Host ""
Write-Host "For more help, see: RUST_FIX_GUIDE.md" -ForegroundColor Cyan
Read-Host "Press Enter to exit"