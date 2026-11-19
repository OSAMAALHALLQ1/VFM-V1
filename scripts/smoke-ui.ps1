param(
    [string]$Output = (Join-Path $PSScriptRoot '..\docs\demo'),
    [switch]$CloseCases
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
New-Item -ItemType Directory -Force -Path $Output | Out-Null
$Output = (Resolve-Path -LiteralPath $Output).Path
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class SpikeWindowCapture {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);
    private delegate bool EnumProc(IntPtr hwnd, IntPtr data);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc callback, IntPtr data);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int length);
    public static IntPtr FindPlayerWindow(int pid) {
        IntPtr result = IntPtr.Zero;
        EnumWindows(delegate(IntPtr hwnd, IntPtr data) {
            uint owner; GetWindowThreadProcessId(hwnd, out owner);
            var title = new StringBuilder(256); GetWindowText(hwnd, title, title.Capacity);
            if (owner == pid && IsWindowVisible(hwnd) && title.ToString().StartsWith("Player")) { result = hwnd; return false; }
            return true;
        }, IntPtr.Zero);
        return result;
    }
    public static void Mouse(IntPtr hwnd, uint message, int x, int y, bool down) {
        var point = new IntPtr((y << 16) | (x & 65535));
        if (!PostMessage(hwnd, message, new IntPtr(down ? 1 : 0), point)) throw new InvalidOperationException("Player mouse message failed");
    }
}
'@
$oldDpiContext = [SpikeWindowCapture]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
if ($oldDpiContext -eq [IntPtr]::Zero) { throw 'Could not establish per-monitor DPI coordinates for the test thread.' }
$env:LIBVLC_PATH = Join-Path $root 'third_party\vlc-3.0.24'
$exe = Join-Path $root 'target\release\spike.exe'
$media = Join-Path $root 'media\h264_1080p60.mp4'
if ($CloseCases) {
    try {
        foreach ($state in @('Playing', 'Paused', 'Stopped', 'MissingRuntime')) {
            $log = Join-Path $Output "close-$state.stdout.txt"
            $err = Join-Path $Output "close-$state.stderr.txt"
            $arguments = @(('"' + $media + '"'), '--size', '1920x1080')
            if ($state -eq 'MissingRuntime') {
                $arguments += @('--runtime', ('"' + (Join-Path $root 'media') + '"'), '--bench', '3')
            }
            $p = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru -RedirectStandardOutput $log -RedirectStandardError $err
            $null = $p.Handle
            try {
                $startup = [Diagnostics.Stopwatch]::StartNew()
                $hwnd = [IntPtr]::Zero
                while ($hwnd -eq [IntPtr]::Zero -and $startup.Elapsed.TotalSeconds -lt 120) {
                    $p.Refresh()
                    if ($p.HasExited) { throw "$state exited before its UI appeared; inspect $err" }
                    $hwnd = [SpikeWindowCapture]::FindPlayerWindow($p.Id)
                    if ($hwnd -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 100 }
                }
                if ($hwnd -eq [IntPtr]::Zero) { throw "$state window startup timed out" }
                Start-Sleep -Seconds 3
                if ($state -in @('Paused', 'Stopped')) {
                    $r = New-Object SpikeWindowCapture+RECT
                    if (-not [SpikeWindowCapture]::GetClientRect($hwnd, [ref]$r)) { throw 'GetClientRect failed' }
                    $scale = [SpikeWindowCapture]::GetDpiForWindow($hwnd) / 96.0
                    $x = [int][math]::Round($(if ($state -eq 'Paused') { 91 } else { 195 }) * $scale)
                    $y = [int][math]::Round(($r.Bottom - $r.Top) - 58 * $scale)
                    [SpikeWindowCapture]::Mouse($hwnd, 0x0200, $x, $y, $false)
                    [SpikeWindowCapture]::Mouse($hwnd, 0x0201, $x, $y, $true)
                    [SpikeWindowCapture]::Mouse($hwnd, 0x0202, $x, $y, $false)
                    Start-Sleep -Milliseconds 900
                }
                if ($state -eq 'MissingRuntime') {
                    $r = New-Object SpikeWindowCapture+RECT
                    if (-not [SpikeWindowCapture]::GetWindowRect($hwnd, [ref]$r)) { throw 'GetWindowRect failed' }
                    $bitmap = New-Object System.Drawing.Bitmap(($r.Right - $r.Left), ($r.Bottom - $r.Top))
                    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
                    try {
                        $hdc = $graphics.GetHdc()
                        try {
                            if (-not [SpikeWindowCapture]::PrintWindow($hwnd, $hdc, 2)) { throw 'Player-only error capture failed' }
                        } finally { $graphics.ReleaseHdc($hdc) }
                        $bitmap.Save((Join-Path $Output '09-missing-runtime.png'), [System.Drawing.Imaging.ImageFormat]::Png)
                    } finally { $graphics.Dispose(); $bitmap.Dispose() }
                } else {
                    if ([SpikeWindowCapture]::FindPlayerWindow($p.Id) -ne $hwnd) { throw 'Owned player window no longer exists' }
                    if (-not [SpikeWindowCapture]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) { throw 'Owned-window close failed' }
                }
                if (-not $p.WaitForExit(10000)) { throw "$state did not shut down within 10 seconds" }
                $expectedExit = if ($state -eq 'MissingRuntime') { 1 } else { 0 }
                if ($p.ExitCode -ne $expectedExit) { throw "$state exited $($p.ExitCode), expected $expectedExit" }
                if ($state -ne 'MissingRuntime' -and -not [IO.File]::ReadAllText($log).Contains("event=$state")) {
                    throw "$state event not confirmed in the owned process log"
                }
                Write-Output "$state shutdown verified, exit $($p.ExitCode)"
            } finally {
                $p.Refresh()
                if (-not $p.HasExited) {
                    $p.Kill()
                    if (-not $p.WaitForExit(5000)) { Write-Warning 'Owned process termination has not completed within 5 seconds' }
                }
                $p.Dispose()
            }
        }
    } finally { [SpikeWindowCapture]::SetThreadDpiAwarenessContext($oldDpiContext) | Out-Null }
    return
}
$log = Join-Path $Output 'smoke.stdout.txt'
$err = Join-Path $Output 'smoke.stderr.txt'
$p = Start-Process -FilePath $exe -ArgumentList @(('"' + $media + '"'), '--size', '1920x1080', '--bench', '26') -PassThru -RedirectStandardOutput $log -RedirectStandardError $err
$null = $p.Handle
try {
    $startup = [Diagnostics.Stopwatch]::StartNew()
    $hwnd = [IntPtr]::Zero
    while ($hwnd -eq [IntPtr]::Zero -and $startup.Elapsed.TotalSeconds -lt 120) {
        $p.Refresh()
        if ($p.HasExited) { throw "Spike exited before UI tests; inspect $err" }
        $hwnd = [SpikeWindowCapture]::FindPlayerWindow($p.Id)
        if ($hwnd -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 100 }
    }
    if ($hwnd -eq [IntPtr]::Zero) { throw 'Player window did not become visible within 120 seconds.' }
    Start-Sleep -Seconds 3
    $rtl = $false
    function Assert-Player {
        $p.Refresh()
        if ($p.HasExited -or [SpikeWindowCapture]::FindPlayerWindow($p.Id) -ne $hwnd) { throw 'Owned player window no longer exists.' }
    }
    function Invoke-Control([string]$action) {
        Assert-Player
        $r = New-Object SpikeWindowCapture+RECT
        if (-not [SpikeWindowCapture]::GetClientRect($hwnd, [ref]$r)) { throw 'GetClientRect failed' }
        $scale = [SpikeWindowCapture]::GetDpiForWindow($hwnd) / 96.0
        $w = ($r.Right - $r.Left) / $scale
        $h = ($r.Bottom - $r.Top) / $scale
        $y = $h - 58
        switch ($action) {
            'Play' { $x = if ($rtl) { $w - 91 } else { 91 } }
            'Stop' { $x = if ($rtl) { $w - 195 } else { 195 } }
            'Language' { $x = if ($rtl) { 90 } else { $w - 90 }; $y = 57 }
            'Mute' { $x = if ($rtl) { 161 } else { $w - 337 } }
            'Full' { $x = if ($rtl) { 76 } else { $w - 76 } }
            'Seek' { $x = $w / 2; $y = $h - 106 }
            'Volume' { $x = if ($rtl) { 249 } else { $w - 249 } }
            default { throw 'Unknown test control' }
        }
        $px = [int][math]::Round($x * $scale)
        $py = [int][math]::Round($y * $scale)
        if ($action -eq 'Volume') {
            $startX = [int][math]::Round($(if ($rtl) { 327 } else { $w - 171 }) * $scale)
            [SpikeWindowCapture]::Mouse($hwnd, 0x0200, $startX, $py, $false)
            [SpikeWindowCapture]::Mouse($hwnd, 0x0201, $startX, $py, $true)
            [SpikeWindowCapture]::Mouse($hwnd, 0x0200, $px, $py, $true)
            [SpikeWindowCapture]::Mouse($hwnd, 0x0202, $px, $py, $false)
        } else {
            [SpikeWindowCapture]::Mouse($hwnd, 0x0200, $px, $py, $false)
            [SpikeWindowCapture]::Mouse($hwnd, 0x0201, $px, $py, $true)
            [SpikeWindowCapture]::Mouse($hwnd, 0x0202, $px, $py, $false)
        }
    }
    function Capture([string]$name) {
        Assert-Player
        $r = New-Object SpikeWindowCapture+RECT
        if (-not [SpikeWindowCapture]::GetWindowRect($hwnd, [ref]$r)) { throw 'GetWindowRect failed' }
        $bitmap = New-Object System.Drawing.Bitmap(($r.Right - $r.Left), ($r.Bottom - $r.Top))
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $hdc = $graphics.GetHdc()
            try {
                if (-not [SpikeWindowCapture]::PrintWindow($hwnd, $hdc, 2)) { throw 'Player-only capture failed.' }
            } finally { $graphics.ReleaseHdc($hdc) }
            $bitmap.Save((Join-Path $Output "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
    }
    Capture '01-playing-ltr'
    Invoke-Control 'Play'
    Start-Sleep -Milliseconds 900
    Capture '02-paused-overlay'
    Start-Sleep -Milliseconds 300
    Capture '08-paused-confirm'
    Invoke-Control 'Play'
    Invoke-Control 'Seek'
    Start-Sleep -Milliseconds 900
    Invoke-Control 'Mute'
    Invoke-Control 'Language'
    $rtl = $true
    Start-Sleep -Milliseconds 900
    Capture '03-playing-rtl-muted'
    Invoke-Control 'Full'
    Start-Sleep -Milliseconds 900
    Capture '04-fullscreen-overlay'
    Invoke-Control 'Full'
    Start-Sleep -Milliseconds 500
    Invoke-Control 'Volume'
    Start-Sleep -Milliseconds 500
    Capture '07-volume-pointer'
    Invoke-Control 'Stop'
    Start-Sleep -Milliseconds 900
    Capture '05-stopped'
    Invoke-Control 'Play'
    Start-Sleep -Milliseconds 900
    Capture '06-restarted'
    if (-not $p.WaitForExit(35000)) { throw 'Spike did not exit at benchmark deadline' }
    if ($p.ExitCode -ne 0) { throw "Spike exited $($p.ExitCode); inspect $err" }
    Write-Output "Captured eight states in $Output using messages and PrintWindow restricted to the owned player. Inspect images and logs; real keyboard/focus behavior is not tested."
} finally {
    $p.Refresh()
    if (-not $p.HasExited) {
        $p.Kill()
        if (-not $p.WaitForExit(5000)) { Write-Warning 'Owned process termination has not completed within 5 seconds' }
    }
    $p.Dispose()
    [SpikeWindowCapture]::SetThreadDpiAwarenessContext($oldDpiContext) | Out-Null
}
