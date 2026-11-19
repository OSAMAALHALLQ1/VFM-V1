param([string]$Destination = (Join-Path $PSScriptRoot '..\third_party'))
$ErrorActionPreference = 'Stop'
$version = '3.0.24'
$sha256 = 'fcf30850371ad10c9373cc4f0f4501e7dee49e3e9ae9f20c72fb2661a1ca6323'
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
$Destination = (Resolve-Path -LiteralPath $Destination).Path
$archive = Join-Path $Destination "vlc-$version-win64.zip"
$runtime = Join-Path $Destination "vlc-$version"
if (Test-Path -LiteralPath $runtime) {
    throw "Runtime directory already exists: $runtime. Use a new Destination; existing files will not be overwritten."
}
if (-not (Test-Path -LiteralPath $archive)) {
    Invoke-WebRequest -UseBasicParsing -Uri "https://download.videolan.org/pub/videolan/vlc/$version/win64/vlc-$version-win64.zip" -OutFile $archive
}
$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
if ($actual -ne $sha256) { throw "SHA-256 mismatch. Expected $sha256; got $actual. Archive was not extracted." }
Expand-Archive -LiteralPath $archive -DestinationPath $Destination
foreach ($required in @('libvlc.dll', 'libvlccore.dll', 'plugins')) {
    if (-not (Test-Path -LiteralPath (Join-Path $runtime $required))) { throw "Runtime missing $required" }
}
& (Join-Path $runtime 'vlc-cache-gen.exe') (Join-Path $runtime 'plugins')
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath (Join-Path $runtime 'plugins\plugins.dat'))) { throw 'Plugin cache generation failed.' }
Write-Output "Verified and extracted libVLC $version to $runtime; plugin cache generated."
Write-Output 'This archive has no SDK folder. The runtime loader does not need headers or an import library.'
