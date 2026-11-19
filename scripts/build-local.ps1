param([ValidateSet('Build', 'Verify')][string]$Action = 'Build')
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$deps = Join-Path $root 'third_party'
$package = Join-Path $deps 'windows-sdk-x64.nupkg'
$sdk = Join-Path $deps 'windows-sdk-x64'
$sha = 'ad6ce90ff94010699d29548b3c2b1e52c89eb81143e2199155d9a5c38001b52b'
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
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = Join-Path $vc.FullName 'bin\Hostx64\x64\link.exe'
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
