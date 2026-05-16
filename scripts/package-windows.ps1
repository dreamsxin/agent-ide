param(
    [string]$BundleTargets = "nsis,msi",
    [string]$Configuration = "release",
    [switch]$SkipChecks,
    [switch]$CleanOutput,
    [switch]$SkipDependencyNote
)

$ErrorActionPreference = "Stop"

function Invoke-Step {
    param(
        [string]$Title,
        [scriptblock]$Command
    )

    Write-Host ""
    Write-Host "==> $Title" -ForegroundColor Cyan
    & $Command
}

function Require-Command {
    param([string]$Name)

    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command not found: $Name"
    }
}

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $RepoRoot

$isWindowsHost = if (Get-Variable -Name IsWindows -Scope Global -ErrorAction SilentlyContinue) {
    $IsWindows
} else {
    $env:OS -eq "Windows_NT"
}

if (-not $isWindowsHost) {
    throw "Windows packaging must run on Windows because the installer targets are Windows-specific."
}

Require-Command "node"
Require-Command "npm"
Require-Command "cargo"
Require-Command "rustc"

$packageJson = Get-Content -Path (Join-Path $RepoRoot "package.json") -Raw | ConvertFrom-Json
$productName = "Agent IDE"
$version = [string]$packageJson.version
$outputDir = Join-Path $RepoRoot "release\windows\$version"
$bundleDir = Join-Path $RepoRoot "src-tauri\target\$Configuration\bundle"

Write-Host "Agent IDE Windows package"
Write-Host "Version: $version"
Write-Host "Bundle targets: $BundleTargets"
Write-Host "Output: $outputDir"

if (-not $SkipDependencyNote) {
    Write-Host ""
    Write-Host "Packaging note:" -ForegroundColor Yellow
    Write-Host " - The first Tauri Windows bundle may download NSIS, nsis_tauri_utils.dll, and/or WiX tooling."
    Write-Host " - If bundling times out while downloading tools, rerun this script or use the GitHub Actions workflow."
    Write-Host " - To build only one installer type, pass -BundleTargets nsis or -BundleTargets msi."
}

if (-not (Test-Path (Join-Path $RepoRoot "node_modules"))) {
    Invoke-Step "Install npm dependencies" {
        npm ci
    }
}

if (-not $SkipChecks) {
    Invoke-Step "Frontend build" {
        npm run build
    }
    Invoke-Step "Frontend tests" {
        npm test -- --run
    }
    Invoke-Step "Rust check" {
        Push-Location (Join-Path $RepoRoot "src-tauri")
        try {
            cargo check
        } finally {
            Pop-Location
        }
    }
    Invoke-Step "Rust tests" {
        Push-Location (Join-Path $RepoRoot "src-tauri")
        try {
            cargo test
        } finally {
            Pop-Location
        }
    }
}

if ($CleanOutput -and (Test-Path $outputDir)) {
    Invoke-Step "Clean package output" {
        Remove-Item -LiteralPath $outputDir -Recurse -Force
    }
}

Invoke-Step "Tauri Windows build" {
    npm run tauri -- build --bundles $BundleTargets
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri bundling failed with exit code $LASTEXITCODE. Check installer toolchain downloads for NSIS/WiX."
    }
}

$artifactPatterns = @(
    "nsis\*.exe",
    "msi\*.msi",
    "wix\*.msi"
)

$artifacts = foreach ($pattern in $artifactPatterns) {
    Get-ChildItem -Path (Join-Path $bundleDir $pattern) -File -ErrorAction SilentlyContinue
}

if (-not $artifacts -or $artifacts.Count -eq 0) {
    throw "No Windows installer artifacts found under $bundleDir"
}

New-Item -ItemType Directory -Force -Path $outputDir | Out-Null

$manifestArtifacts = @()
foreach ($artifact in $artifacts) {
    $destination = Join-Path $outputDir $artifact.Name
    Copy-Item -LiteralPath $artifact.FullName -Destination $destination -Force
    $hash = Get-FileHash -Algorithm SHA256 -LiteralPath $destination
    $manifestArtifacts += [pscustomobject]@{
        file = $artifact.Name
        source = $artifact.FullName
        sizeBytes = (Get-Item -LiteralPath $destination).Length
        sha256 = $hash.Hash.ToLowerInvariant()
    }
}

$shaFile = Join-Path $outputDir "SHA256SUMS.txt"
$manifestArtifacts |
    Sort-Object file |
    ForEach-Object { "$($_.sha256)  $($_.file)" } |
    Set-Content -Path $shaFile -Encoding utf8

$manifest = [pscustomobject]@{
    productName = $productName
    version = $version
    platform = "windows"
    bundleTargets = $BundleTargets
    configuration = $Configuration
    generatedAt = (Get-Date).ToUniversalTime().ToString("o")
    artifacts = $manifestArtifacts | Sort-Object file
}
$manifest | ConvertTo-Json -Depth 4 | Set-Content -Path (Join-Path $outputDir "manifest.json") -Encoding utf8

Write-Host ""
Write-Host "Windows package artifacts:" -ForegroundColor Green
$manifestArtifacts |
    Sort-Object file |
    ForEach-Object {
        Write-Host " - $($_.file) ($($_.sizeBytes) bytes, sha256 $($_.sha256))"
    }
Write-Host ""
Write-Host "Output directory: $outputDir"
