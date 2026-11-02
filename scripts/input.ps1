# 测试辅助：模拟键鼠。用法：
#   input.ps1 keys "%+a"            （SendKeys 语法：% = Alt, + = Shift, ^ = Ctrl）
#   input.ps1 drag x1 y1 x2 y2      （物理像素）
#   input.ps1 click x y [right]
#   input.ps1 dblclick x y
#   input.ps1 move x y
#   input.ps1 wheel x y delta
param([string]$Cmd, [string]$A, [string]$B, [string]$C, [string]$D)
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public class In {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, int data, IntPtr extra);
}
"@
[In]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
function Down($r) { if ($r) { [In]::mouse_event(0x08, 0, 0, 0, [IntPtr]::Zero) } else { [In]::mouse_event(0x02, 0, 0, 0, [IntPtr]::Zero) } }
function Up($r) { if ($r) { [In]::mouse_event(0x10, 0, 0, 0, [IntPtr]::Zero) } else { [In]::mouse_event(0x04, 0, 0, 0, [IntPtr]::Zero) } }
switch ($Cmd) {
  "keys" { [System.Windows.Forms.SendKeys]::SendWait($A) }
  "move" { [In]::SetCursorPos([int]$A, [int]$B) | Out-Null }
  "click" {
    [In]::SetCursorPos([int]$A, [int]$B) | Out-Null; Start-Sleep -Milliseconds 80
    Down ($C -eq "right"); Start-Sleep -Milliseconds 60; Up ($C -eq "right")
  }
  "dblclick" {
    [In]::SetCursorPos([int]$A, [int]$B) | Out-Null; Start-Sleep -Milliseconds 80
    Down $false; Up $false; Start-Sleep -Milliseconds 80; Down $false; Up $false
  }
  "drag" {
    $x1 = [int]$A; $y1 = [int]$B; $x2 = [int]$C; $y2 = [int]$D
    [In]::SetCursorPos($x1, $y1) | Out-Null; Start-Sleep -Milliseconds 120
    Down $false; Start-Sleep -Milliseconds 60
    for ($i = 1; $i -le 20; $i++) {
      [In]::SetCursorPos([int]($x1 + ($x2 - $x1) * $i / 20), [int]($y1 + ($y2 - $y1) * $i / 20)) | Out-Null
      Start-Sleep -Milliseconds 15
    }
    Start-Sleep -Milliseconds 60; Up $false
  }
  "wheel" {
    [In]::SetCursorPos([int]$A, [int]$B) | Out-Null; Start-Sleep -Milliseconds 50
    [In]::mouse_event(0x0800, 0, 0, [int]$C, [IntPtr]::Zero)
  }
}
