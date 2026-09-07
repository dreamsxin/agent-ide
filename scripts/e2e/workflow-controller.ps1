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
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
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
  $requested = [Win32E2E]::SetForegroundWindow($hwnd)
  Start-Sleep -Milliseconds 500
  # `SetForegroundWindow` 在调用进程不持有前台权限时会**静默失败**，返回 false。
  # 原来这里把返回值 `| Out-Null` 丢掉了，于是脚本以为自己抢到了焦点，继续对着
  # 别的窗口点击，最后在 30 秒后报"元素找不到" —— 把环境问题伪装成产品缺陷。
  # 所以这里必须真的确认前台窗口是不是 app。
  $script:AppIsForeground = $requested -and ([Win32E2E]::GetForegroundWindow() -eq $hwnd)
  return $script:AppIsForeground
}

## 这套 harness 需要独占的交互式桌面。
##
## 抢不到前台就没有任何后续步骤是可信的：截图抓的是主屏幕（会拍到别的窗口），
## 而 `Click-Element` 的退化路径用 SendKeys —— 那是发给**当前有焦点的窗口**的，
## 会把按键打进无关程序。所以这里提前失败，而不是继续跑完一堆假动作。
function Require-Foreground($step) {
  if (Focus-App) { return }
  Fail $step @"
Agent IDE window could not be brought to the foreground.

This harness drives the real desktop app through UI Automation and SendKeys, so it
needs an exclusive interactive desktop: no RDP/terminal window stealing focus, no
locked session, and nobody else using the machine. Windows silently refuses
SetForegroundWindow when another process owns the foreground.

Nothing below this point would have been trustworthy: screenshots capture the
primary screen rather than the app window, and SendKeys would have gone to
whichever window does hold focus.
"@
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
    # `Focus-App` 现在返回 bool；不丢掉的话它会混进 `Wait-Element` 的返回值里，
    # 调用方拿到的就是 [bool, element] 数组而不是元素。
    Focus-App | Out-Null
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
  # 退化路径用 SendKeys，而 SendKeys 发给的是**当前有焦点的窗口**。app 不在前台时
  # 这一按键会打进无关程序（实测打进过别的应用窗口），所以宁可失败也不能乱发。
  if (-not (Focus-App)) {
    Fail "click-without-foreground" "Element '$($element.Current.Name)' exposes no InvokePattern and the app is not in the foreground, so SendKeys would have gone to another window."
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
Require-Foreground "environment"
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

Click-Name "Changes"
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
