param(
    [string]$Key = 'M',
    [int]$VirtualKey = 0x4D,
    [string]$Name = 'shortcut-mute'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ShortcutTest {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc proc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
}
'@
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe = Join-Path $root 'target\release\player.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw 'Build the player first.' }
$env:LIBVLC_PATH = Join-Path $root 'third_party\vlc-3.0.24'
$env:PLAYER_KEEP_UI = '1'
$proc = Start-Process -FilePath $exe -ArgumentList '.\media\h264_1080p60.mp4' -PassThru
try {
    $hwnd = [IntPtr]::Zero; $best = 0
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline -and $best -lt 360000) {
        $cb = [ShortcutTest+EnumWindowsProc] {
            param($h, $l)
            $pidOut = [uint32]0
            [void][ShortcutTest]::GetWindowThreadProcessId($h, [ref]$pidOut)
            if ($pidOut -eq $proc.Id -and [ShortcutTest]::IsWindowVisible($h)) {
                $r = New-Object ShortcutTest+RECT
                if ([ShortcutTest]::GetWindowRect($h, [ref]$r)) {
                    $area = ($r.Right - $r.Left) * ($r.Bottom - $r.Top)
                    if ($area -gt $script:best) { $script:best = $area; $script:hwnd = $h }
                }
            }
            return $true
        }
        $script:best = 0; $script:hwnd = [IntPtr]::Zero
        [void][ShortcutTest]::EnumWindows($cb, [IntPtr]::Zero)
        $best = $script:best; $hwnd = $script:hwnd
        if ($best -lt 360000) { Start-Sleep -Milliseconds 200 }
    }
    if ($hwnd -eq [IntPtr]::Zero) { throw 'Player window not found' }
    Start-Sleep -Milliseconds 900

    # WM_KEYDOWN (0x0100) then WM_KEYUP (0x0101) with repeat count 1.
    [void][ShortcutTest]::PostMessage($hwnd, 0x0100, [IntPtr]$VirtualKey, [IntPtr]1)
    Start-Sleep -Milliseconds 120
    [void][ShortcutTest]::PostMessage($hwnd, 0x0101, [IntPtr]$VirtualKey, [IntPtr]1)
    Start-Sleep -Milliseconds 300

    $out = Join-Path $root "docs\compare\$Name.png"
    $old = [ShortcutTest]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
    try {
        $r = New-Object ShortcutTest+RECT
        [void][ShortcutTest]::GetWindowRect($hwnd, [ref]$r)
        $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $g.GetHdc()
        try { [void][ShortcutTest]::PrintWindow($hwnd, $hdc, 2) } finally { $g.ReleaseHdc($hdc); $g.Dispose() }
        $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
    } finally { [ShortcutTest]::SetThreadDpiAwarenessContext($old) | Out-Null }
    Write-Output "Shortcut $Key sent; capture: $out"
} finally {
    if (-not $proc.HasExited) { $proc.Kill() }
}
