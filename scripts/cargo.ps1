# Runs cargo with the local MSVC linker and staged Windows SDK (no global changes).
# Usage: powershell -NoProfile -File scripts/cargo.ps1 <cargo args...>
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$deps = Join-Path $root 'third_party'
$sdk = Join-Path $deps 'windows-sdk-x64'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Install Visual Studio C++ Build Tools first.' }
$vc = Get-ChildItem -LiteralPath (Join-Path $vs 'VC\Tools\MSVC') -Directory | Sort-Object Name -Descending | Select-Object -First 1
$hostBin = Join-Path $vc.FullName 'bin\Hostx64\x64'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = Join-Path $hostBin 'link.exe'
# cc-rs needs cl.exe (for bundled SQLite) and both libraries and headers.
$env:CC = Join-Path $hostBin 'cl.exe'
$env:AR = Join-Path $hostBin 'lib.exe'
$sdkInc = Join-Path $root 'third_party\windows-sdk-headers\c\Include\10.0.26100.0'
$env:INCLUDE = ((Join-Path $vc.FullName 'include'), (Join-Path $sdkInc 'ucrt'), (Join-Path $sdkInc 'shared'), (Join-Path $sdkInc 'um')) -join ';'
$env:LIB = (Join-Path $sdk 'c\ucrt\x64') + ';' + (Join-Path $sdk 'c\um\x64') + ';' + (Join-Path $vc.FullName 'lib\x64') + ';' + $env:LIB
Push-Location $root
try {
    & cargo @args
    exit $LASTEXITCODE
} finally { Pop-Location }
