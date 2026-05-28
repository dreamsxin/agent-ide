param(
  [Parameter(Mandatory=$true)][int]$ProcessId,
  [Parameter(Mandatory=$true)][string]$Workspace,
  [Parameter(Mandatory=$true)][string]$RunDir,
  [int]$TimeoutSeconds = 180
)

$ErrorActionPreference = "Stop"
$shots = Join-Path $RunDir "screenshots"
$logs = Join-Path $RunDir "logs"
New-Item -ItemType Directory -Force -Path $shots,$logs | Out-Null

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win32E2E {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hWnd, int X, int Y, int nWidth, int nHeight, bool bRepaint);
}
"@

function Log($message) {
  $line = "$(Get-Date -Format o) $message"
  Add-Content -LiteralPath (Join-Path $logs "controller.log") -Value $line
  Write-Host $message
}

function Fail($step, $message) {
  Screenshot "failed-$step"
  Set-Content -LiteralPath (Join-Path $RunDir "failed-step.txt") -Value "$step`n$message" -Encoding UTF8
  throw "$($step): $message"
}

function Window() {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($proc -and $proc.MainWindowHandle -ne 0) {
      return $proc.MainWindowHandle
    }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  throw "Timed out waiting for Agent IDE window"
}

function Focus-App() {
  $hwnd = Window
  [Win32E2E]::ShowWindow($hwnd, 9) | Out-Null
  [Win32E2E]::MoveWindow($hwnd, 40, 40, 1440, 900, $true) | Out-Null
  [Win32E2E]::SetForegroundWindow($hwnd) | Out-Null
  Start-Sleep -Milliseconds 500
}

function Screenshot($name) {
  $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
  $path = Join-Path $shots "$name.png"
  $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $graphics.Dispose()
  $bitmap.Dispose()
}

function RootElement() {
  [System.Windows.Automation.AutomationElement]::FromHandle((Window))
}

function Find-AllByName($name) {
  $cond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::NameProperty,
    $name
  )
  (RootElement).FindAll([System.Windows.Automation.TreeScope]::Subtree, $cond)
}

function Find-FirstByName($name) {
  $items = Find-AllByName $name
  if ($items.Count -gt 0) { return $items.Item(0) }
  $all = (RootElement).FindAll(
    [System.Windows.Automation.TreeScope]::Subtree,
    [System.Windows.Automation.Condition]::TrueCondition
  )
  for ($i = 0; $i -lt $all.Count; $i++) {
    $item = $all.Item($i)
    if ($item.Current.Name -and $item.Current.Name.Contains($name)) {
      return $item
    }
  }
  return $null
}

function Wait-Element($name, [int]$seconds = 20) {
  $deadline = (Get-Date).AddSeconds($seconds)
  do {
    Focus-App
    $item = Find-FirstByName $name
    if ($item) { return $item }
    Start-Sleep -Milliseconds 500
  } while ((Get-Date) -lt $deadline)
  return $null
}

function Click-Element($element) {
  if (-not $element) { throw "Cannot click null element" }
  $pattern = $null
  if ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    $pattern.Invoke()
    Start-Sleep -Milliseconds 500
    return
  }
  $rect = $element.Current.BoundingRectangle
  [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point(
    [int]($rect.X + $rect.Width / 2),
    [int]($rect.Y + $rect.Height / 2)
  )
  [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
  Start-Sleep -Milliseconds 500
}

function Click-Name($name, [int]$seconds = 20) {
  $element = Wait-Element $name $seconds
  if (-not $element) { Fail "click-$name" "Element '$name' not found" }
  Click-Element $element
}

function Type-Text($text) {
  [System.Windows.Forms.Clipboard]::SetText($text)
  [System.Windows.Forms.SendKeys]::SendWait("^v")
  Start-Sleep -Milliseconds 300
}

function Assert-FileContent($expected) {
  $actual = (Get-Content -Raw -LiteralPath (Join-Path $Workspace "smoke.txt")).Trim()
  if ($actual -ne $expected) {
    Fail "file-content" "Expected smoke.txt '$expected', got '$actual'"
  }
}

function Assert-GitHasSmokeChange() {
  $status = git -C $Workspace status --short -- smoke.txt
  if (-not ($status -match "smoke\.txt")) {
    Fail "git-status" "Git status did not include smoke.txt"
  }
}

function Commit-SmokeChange() {
  git -C $Workspace add smoke.txt
  git -C $Workspace commit -m "E2E workflow smoke commit" | Out-File -LiteralPath (Join-Path $logs "git-commit.log") -Encoding UTF8
  $status = git -C $Workspace status --short -- smoke.txt
  if ($status) {
    Fail "git-commit" "smoke.txt still dirty after commit: $status"
  }
}

Log "Starting Windows desktop workflow E2E"
Focus-App
Screenshot "01-boot"

Click-Name "Commands"
Screenshot "02-commands"

Click-Name "workflow" 30
Start-Sleep -Seconds 2
Screenshot "03-workflow-failed"

Click-Name "Problems"
if (-not (Wait-Element "workflow smoke expected fixed" 20)) {
  Fail "problems" "Problem text did not appear"
}
Screenshot "04-problems"

$fix = Wait-Element "Fix" 10
if (-not $fix) { Fail "fix-problem" "Problem Fix button did not appear" }
Click-Element $fix
Start-Sleep -Seconds 6
Screenshot "05-agent-ran"

Click-Name "Diff"
if (-not (Wait-Element "Apply hunk" 30)) {
  Fail "diff" "Diff hunk did not appear"
}
Screenshot "06-diff"
Click-Name "Apply hunk"
Start-Sleep -Seconds 1
Assert-FileContent "fixed"
Screenshot "07-applied"

Click-Name "Commands"
Click-Name "Rerun"
Start-Sleep -Seconds 2
if (-not (Wait-Element "success" 20)) {
  Fail "rerun" "Workflow command did not report success"
}
Screenshot "08-rerun-success"

Assert-GitHasSmokeChange
Click-Name "Git"
Start-Sleep -Seconds 1
Screenshot "09-git"
Commit-SmokeChange
Screenshot "10-committed"

Log "Windows desktop workflow E2E passed"
