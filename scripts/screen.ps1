# 测试辅助：把整个虚拟桌面截成 PNG（物理像素）
param([string]$Out, [double]$Scale = 0.5)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class DpiUtil { [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v); }
"@
[DpiUtil]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
$b = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
$w = [int]($b.Width * $Scale); $h = [int]($b.Height * $Scale)
$small = New-Object System.Drawing.Bitmap $w, $h
$g2 = [System.Drawing.Graphics]::FromImage($small)
$g2.InterpolationMode = 'HighQualityBicubic'
$g2.DrawImage($bmp, 0, 0, $w, $h)
$small.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
"$($b.Left),$($b.Top),$($b.Width),$($b.Height)"
