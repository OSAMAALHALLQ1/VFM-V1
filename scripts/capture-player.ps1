param(
    [string]$Name = 'player-dark-en',
    [string]$GalleryArgs = '',
    [string]$OutDir = 'docs/compare'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class GalleryCapture {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc proc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
}
'@
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe = Join-Path $root 'target\release\player.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw 'Build the player first: scripts/cargo.ps1 build --release -p player-app --bin player' }
$env:LIBVLC_PATH = (Join-Path $root 'third_party\vlc-3.0.24')
$env:PLAYER_KEEP_UI = '1'
$out = Join-Path $root (Join-Path $OutDir "$Name.png")
New-Item -ItemType Directory -Force -Path (Split-Path $out) | Out-Null

$argList = if ($GalleryArgs.Trim()) { $GalleryArgs -split '\s+' } else { @() }
Write-Output "PlayerArgs: $($argList -join '|')"
$childErr = Join-Path $env:TEMP 'gallery-capture.stderr.txt'
if ($argList.Count -gt 0) {
    $proc = Start-Process -FilePath $exe -ArgumentList $argList -PassThru -RedirectStandardError $childErr
} else {
    $proc = Start-Process -FilePath $exe -PassThru -RedirectStandardError $childErr
}
try {
    $hwnd = [IntPtr]::Zero
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline -and $hwnd -eq [IntPtr]::Zero) {
        $cb = [GalleryCapture+EnumWindowsProc] {
            param($h, $l)
            $pidOut = [uint32]0
            [void][GalleryCapture]::GetWindowThreadProcessId($h, [ref]$pidOut)
            if ($pidOut -eq $proc.Id -and [GalleryCapture]::IsWindowVisible($h)) {
                $r = New-Object GalleryCapture+RECT
                if ([GalleryCapture]::GetWindowRect($h, [ref]$r)) {
                    $area = ($r.Right - $r.Left) * ($r.Bottom - $r.Top)
                    if ($area -gt $script:bestArea) { $script:bestArea = $area; $script:found = $h }
                }
            }
            return $true
        }
        $script:found = [IntPtr]::Zero
        $script:bestArea = 0
        [void][GalleryCapture]::EnumWindows($cb, [IntPtr]::Zero)
        if ($script:bestArea -gt 360000) { $hwnd = $script:found }
        Start-Sleep -Milliseconds 200
    }
    if ($hwnd -eq [IntPtr]::Zero) { throw 'Gallery window not found' }
    Start-Sleep -Milliseconds 700
    $old = [GalleryCapture]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
    try {
        $r = New-Object GalleryCapture+RECT
        $w = 0; $h = 0
        for ($try = 0; $try -lt 40; $try++) {
            if ([GalleryCapture]::GetWindowRect($hwnd, [ref]$r)) {
                $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
                if ($w -gt 600 -and $h -gt 400) { break }
            }
            Start-Sleep -Milliseconds 200
        }
        if ($w -le 600 -or $h -le 400) { throw "Window too small for capture: ${w}x${h}" }
        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $g.GetHdc()
        try { if (-not [GalleryCapture]::PrintWindow($hwnd, $hdc, 2)) { throw 'PrintWindow failed' } }
        finally { $g.ReleaseHdc($hdc); $g.Dispose() }
        $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
    } finally { [GalleryCapture]::SetThreadDpiAwarenessContext($old) | Out-Null }
    Write-Output "Captured $out (${w}x${h})"
} finally {
    if (-not $proc.HasExited) { $proc.CloseMainWindow() | Out-Null; Start-Sleep -Milliseconds 400; if (-not $proc.HasExited) { $proc.Kill() } }
}
