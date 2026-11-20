param([ValidateSet('Build', 'Verify')][string]$Action = 'Build')
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$deps = Join-Path $root 'third_party'
$package = Join-Path $deps 'windows-sdk-x64.nupkg'
$sdk = Join-Path $deps 'windows-sdk-x64'
$sha = 'ad6ce90ff94010699d29548b3c2b1e52c89eb81143e2199155d9a5c38001b52b'
# Bundled SQLite needs UCRT headers in addition to the x64 import libraries.
$headersPackage = Join-Path $deps 'windows-sdk-cpp.nupkg'
$headers = Join-Path $deps 'windows-sdk-headers'
$headersSha = 'ff455660bee071a744715a9665666fa29c07681579d3d815959a9eec5a556424'
if (-not (Test-Path -LiteralPath (Join-Path $headers 'c\Include\10.0.26100.0\ucrt\stdio.h'))) {
    if (-not (Test-Path -LiteralPath $headersPackage)) {
        Invoke-WebRequest -UseBasicParsing -Uri 'https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.cpp/10.0.26100.7705/microsoft.windows.sdk.cpp.10.0.26100.7705.nupkg' -OutFile $headersPackage
    }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $headersPackage).Hash.ToLowerInvariant() -ne $headersSha) { throw 'SDK headers SHA-256 mismatch' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory((Resolve-Path $headersPackage).Path, $headers)
}
if (-not (Test-Path -LiteralPath (Join-Path $sdk 'c\ucrt\x64\ucrt.lib'))) {
    New-Item -ItemType Directory -Force -Path $deps | Out-Null
    if (-not (Test-Path -LiteralPath $package)) {
        Invoke-WebRequest -UseBasicParsing -Uri 'https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.cpp.x64/10.0.26100.7705/microsoft.windows.sdk.cpp.x64.10.0.26100.7705.nupkg' -OutFile $package
    }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $package).Hash.ToLowerInvariant() -ne $sha) { throw 'SDK package SHA-256 mismatch' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory($package, $sdk)
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Install Visual Studio C++ Build Tools first.' }
$vc = Get-ChildItem -LiteralPath (Join-Path $vs 'VC\Tools\MSVC') -Directory | Sort-Object Name -Descending | Select-Object -First 1
$hostBin = Join-Path $vc.FullName 'bin\Hostx64\x64'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = Join-Path $hostBin 'link.exe'
$env:CC = Join-Path $hostBin 'cl.exe'
$env:AR = Join-Path $hostBin 'lib.exe'
$env:PATH = (Join-Path $root 'third_party\windows-sdk-headers\c\bin\10.0.26100.0\x64') + ';' + $env:PATH
$sdkInc = Join-Path $headers 'c\Include\10.0.26100.0'
$env:INCLUDE = ((Join-Path $vc.FullName 'include'), (Join-Path $sdkInc 'ucrt'), (Join-Path $sdkInc 'shared'), (Join-Path $sdkInc 'um')) -join ';'
$env:LIB = (Join-Path $sdk 'c\ucrt\x64') + ';' + (Join-Path $sdk 'c\um\x64') + ';' + (Join-Path $vc.FullName 'lib\x64') + ';' + $env:LIB
Push-Location $root
try {
    if ($Action -eq 'Verify') {
        & cargo fmt --all -- --check
        if ($LASTEXITCODE -ne 0) { throw 'cargo fmt failed' }
        & cargo clippy --workspace --all-targets -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw 'cargo clippy failed' }
        & cargo test --workspace
        if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
    } else {
        & cargo build --release -p player-app
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    }
} finally { Pop-Location }
