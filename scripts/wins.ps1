# Test helper: list windows of screenshot.exe with geometry and DPI
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public class W {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public struct RECT { public int L, T, R, B; }
}
"@
[W]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
$global:out = New-Object System.Collections.ArrayList
$global:ids = @(Get-Process screenshot -ErrorAction SilentlyContinue | % { $_.Id })
[W]::EnumWindows({ param($h, $l)
  $procId = 0; [W]::GetWindowThreadProcessId($h, [ref]$procId) | Out-Null
  if ($global:ids -contains [int]$procId) {
    $sb = New-Object System.Text.StringBuilder 256; [W]::GetWindowText($h, $sb, 256) | Out-Null
    $r = New-Object W+RECT; [W]::GetWindowRect($h, [ref]$r) | Out-Null
    if ($sb.Length -gt 0) { [void]$global:out.Add(("{0} vis={1} rect=({2},{3})-({4},{5}) dpi={6}" -f $sb, [W]::IsWindowVisible($h), $r.L, $r.T, $r.R, $r.B, [W]::GetDpiForWindow($h))) }
  }
  $true }, [IntPtr]::Zero) | Out-Null
$global:out
