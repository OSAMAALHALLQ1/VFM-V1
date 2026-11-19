#requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateNotNullOrEmpty()]
    [string]$Output = (Join-Path (Split-Path -Parent $PSScriptRoot) 'media'),
    [ValidateNotNullOrEmpty()]
    [string]$Ffmpeg = 'ffmpeg.exe'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Ensure-Directory {
    param([string]$Path)
    $ancestor = $Path
    while (-not (Test-Path -LiteralPath $ancestor)) {
        $parent = [IO.Path]::GetDirectoryName($ancestor)
        if ([string]::IsNullOrEmpty($parent) -or $parent -eq $ancestor) {
            throw "Cannot find an existing parent directory for: $Path"
        }
        $ancestor = $parent
    }
    if (-not (Test-Path -LiteralPath $ancestor -PathType Container)) {
        throw "A directory path is occupied by a file: $ancestor"
    }
    [void][IO.Directory]::CreateDirectory($Path)
}

foreach ($argument in @($Output, $Ffmpeg)) {
    if ($argument.IndexOf('"') -ge 0 -or $argument.IndexOf([char]0) -ge 0 -or
        $argument.IndexOf("`r") -ge 0 -or $argument.IndexOf("`n") -ge 0) {
        throw 'Paths must not contain double quotes, NUL, or newline characters.'
    }
}
# Use an already installed ffmpeg. No downloads, driver changes, or NVENC.
$ffmpegCommand = Get-Command -Name $Ffmpeg -CommandType Application -ErrorAction Stop | Select-Object -First 1
$ffmpegPath = $ffmpegCommand.Source
$outputPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)
$clips = @(
    @{ name = 'h264_1080p60.mp4'; size = '1920x1080'; rate = '60'; bitrate = '25M'; maxrate = '30M'; buffer = '50M'; gop = '120' },
    @{ name = 'h264_2160p30.mp4'; size = '3840x2160'; rate = '30'; bitrate = '50M'; maxrate = '60M'; buffer = '100M'; gop = '60' }
)
# Preflight both destinations; never silently overwrite an existing clip.
foreach ($clip in $clips) {
    $destination = Join-Path $outputPath $clip.name
    if (Test-Path -LiteralPath $destination) {
        throw "Destination already exists; choose a different Output directory: $destination"
    }
}
Ensure-Directory $outputPath
foreach ($clip in $clips) {
    $destination = Join-Path $outputPath $clip.name
    $filter = 'testsrc2=size={0}:rate={1},noise=alls=12:allf=t' -f $clip.size, $clip.rate
    $arguments = @(
        '-hide_banner', '-nostdin', '-n',
        '-f', 'lavfi', '-i', $filter,
        '-t', '30', '-an',
        '-c:v', 'libx264', '-preset', 'veryfast',
        '-b:v', $clip.bitrate, '-maxrate', $clip.maxrate, '-bufsize', $clip.buffer,
        '-g', $clip.gop,
        '-pix_fmt', 'yuv420p', $destination
    )
    Write-Verbose ("Generating {0}: {1}; libx264 veryfast {2}, GOP {3}, 30 seconds" -f $destination, $filter, $clip.bitrate, $clip.gop)
    # Native argv array preserves spaces and Unicode paths. Output stays on the
    # console, with no redirected pipe that could deadlock during encoding.
    & $ffmpegPath @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "ffmpeg exited with code $LASTEXITCODE while generating $destination. A partial file may remain; it will not be overwritten."
    }
    if (-not (Test-Path -LiteralPath $destination -PathType Leaf) -or
        (Get-Item -LiteralPath $destination).Length -eq 0) {
        throw "ffmpeg did not produce a nonempty clip: $destination"
    }
    Write-Output $destination
}
