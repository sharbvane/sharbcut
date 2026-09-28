[CmdletBinding()]
param(
    [string]$RustDistServer = 'https://rsproxy.cn',
    [string]$CargoRegistryIndex = 'sparse+https://rsproxy.cn/index/',
    [switch]$SkipMontage
)

$ErrorActionPreference = 'Stop'

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or -not [Environment]::Is64BitOperatingSystem) {
    throw 'SharbCut development setup requires 64-bit Windows.'
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$toolsRoot = Join-Path $repoRoot '.tools'
$vendorRoot = Join-Path $repoRoot 'vendor'
$downloadsRoot = Join-Path $toolsRoot 'downloads'
New-Item -ItemType Directory -Force -Path $toolsRoot, $vendorRoot, $downloadsRoot | Out-Null

function Get-Download([string]$Uri, [string]$Path) {
    if ((Test-Path -LiteralPath $Path) -and (Get-Item -LiteralPath $Path).Length -eq 0) {
        [IO.File]::Delete($Path)
    }
    if (-not (Test-Path -LiteralPath $Path)) {
        Write-Host "Downloading $([IO.Path]::GetFileName($Path))"
        & curl.exe --fail --location --retry 3 --output $Path $Uri
        if ($LASTEXITCODE -ne 0) { throw "Download failed: $Uri" }
    }
}

function Import-VsDevEnvironment {
    $localDevCmd = Join-Path $toolsRoot 'vs-buildtools\Common7\Tools\VsDevCmd.bat'
    $devCmd = if (Test-Path -LiteralPath $localDevCmd) { $localDevCmd } else { $null }

    if (-not $devCmd) {
        $vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
        if (Test-Path -LiteralPath $vswhere) {
            $installPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
            if ($installPath) {
                $candidate = Join-Path $installPath 'Common7\Tools\VsDevCmd.bat'
                if (Test-Path -LiteralPath $candidate) { $devCmd = $candidate }
            }
        }
    }

    if (-not $devCmd) {
        throw 'MSVC Build Tools were not found. Install Visual Studio Build Tools with the Desktop development with C++ workload, then rerun this script.'
    }

    $environment = & $env:ComSpec /d /s /c "`"$devCmd`" -arch=x64 -host_arch=x64 >nul && set"
    if ($LASTEXITCODE -ne 0) { throw "VsDevCmd failed: $devCmd" }
    foreach ($line in $environment) {
        $pair = $line -split '=', 2
        if ($pair.Count -eq 2) { Set-Item -Path "Env:$($pair[0])" -Value $pair[1] }
    }
}

$env:CARGO_HOME = Join-Path $toolsRoot 'cargo'
$env:RUSTUP_HOME = Join-Path $toolsRoot 'rustup'
$env:CARGO_TARGET_DIR = Join-Path $repoRoot 'target'
$env:RUSTUP_DIST_SERVER = $RustDistServer.TrimEnd('/')
$env:RUSTUP_USE_CURL = '1'
if ($CargoRegistryIndex) {
    @"
[source.crates-io]
replace-with = "project-registry"

[source.project-registry]
registry = "$($CargoRegistryIndex.TrimEnd('/') + '/')"
"@ | Set-Content -LiteralPath (Join-Path $env:CARGO_HOME 'config.toml') -NoNewline
}
$rustBin = Join-Path $env:CARGO_HOME 'bin'
$rustup = Join-Path $rustBin 'rustup.exe'
$rustupInit = Join-Path $downloadsRoot 'rustup-init.exe'
$toolchain = '1.93.1-x86_64-pc-windows-msvc'

if (-not (Test-Path -LiteralPath $rustup)) {
    Get-Download 'https://win.rustup.rs/x86_64' $rustupInit
    & $rustupInit -y --profile minimal --default-toolchain $toolchain --no-modify-path
    if ($LASTEXITCODE -ne 0) { throw 'Rust installation failed.' }
}
$installedToolchains = & $rustup toolchain list
if ($LASTEXITCODE -ne 0) { throw 'Unable to inspect installed Rust toolchains.' }
if (-not ($installedToolchains | Select-String -Quiet ([regex]::Escape($toolchain)))) {
    & $rustup toolchain install $toolchain --profile minimal --component rustfmt --component clippy --target wasm32-unknown-unknown
    if ($LASTEXITCODE -ne 0) { throw "Rust toolchain installation failed: $toolchain" }
}
$env:RUSTUP_TOOLCHAIN = $toolchain
$env:Path = "$rustBin;$env:Path"

$cmakeRoot = Join-Path $toolsRoot 'cmake-3.31.6-windows-x86_64'
$cmake = Join-Path $cmakeRoot 'bin\cmake.exe'
if (-not (Test-Path -LiteralPath $cmake)) {
    $cmakeArchive = Join-Path $downloadsRoot 'cmake-3.31.6-windows-x86_64.zip'
    Get-Download 'https://github.com/Kitware/CMake/releases/download/v3.31.6/cmake-3.31.6-windows-x86_64.zip' $cmakeArchive
    Expand-Archive -LiteralPath $cmakeArchive -DestinationPath $toolsRoot
}
if (-not (Test-Path -LiteralPath $cmake)) { throw 'CMake extraction did not produce cmake.exe.' }

$llvmRoot = Join-Path $toolsRoot 'libclang-20.1.2'
$libclang = Join-Path $llvmRoot 'runtimes\win-x64\native\libclang.dll'
if (-not (Test-Path -LiteralPath $libclang)) {
    $llvmArchive = Join-Path $downloadsRoot 'libclang.runtime.win-x64.20.1.2.nupkg'
    Get-Download 'https://api.nuget.org/v3-flatcontainer/libclang.runtime.win-x64/20.1.2/libclang.runtime.win-x64.20.1.2.nupkg' $llvmArchive
    Expand-Archive -LiteralPath $llvmArchive -DestinationPath $llvmRoot
}
if (-not (Test-Path -LiteralPath $libclang)) { throw 'libclang extraction did not produce libclang.dll.' }

$ffmpegRoot = Join-Path $vendorRoot 'ffmpeg-n8.1-latest-win64-gpl-shared-8.1'
$ffmpegHeader = Join-Path $ffmpegRoot 'include\libavformat\avformat.h'
if (-not (Test-Path -LiteralPath $ffmpegHeader)) {
    $ffmpegArchive = Join-Path $downloadsRoot 'ffmpeg-n8.1-latest-win64-gpl-shared-8.1.zip'
    Get-Download 'https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/ffmpeg-n8.1-latest-win64-gpl-shared-8.1.zip' $ffmpegArchive
    Expand-Archive -LiteralPath $ffmpegArchive -DestinationPath $vendorRoot
}
if (-not (Test-Path -LiteralPath $ffmpegHeader)) { throw 'FFmpeg extraction did not produce development headers.' }

$sdkRoot = Join-Path $toolsRoot 'windows-sdk-x64'
$dxcore = Get-ChildItem -LiteralPath $sdkRoot -Filter dxcore.lib -File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $dxcore) {
    $sdkArchive = Join-Path $downloadsRoot 'microsoft.windows.sdk.cpp.x64.10.0.26100.6901.zip'
    Get-Download 'https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.cpp.x64/10.0.26100.6901/microsoft.windows.sdk.cpp.x64.10.0.26100.6901.nupkg' $sdkArchive
    Expand-Archive -LiteralPath $sdkArchive -DestinationPath $sdkRoot
    $dxcore = Get-ChildItem -LiteralPath $sdkRoot -Filter dxcore.lib -File -Recurse | Select-Object -First 1
}
if (-not $dxcore) { throw 'Windows SDK package did not provide dxcore.lib.' }

Import-VsDevEnvironment
$env:FFMPEG_DIR = $ffmpegRoot
$env:LIBCLANG_PATH = (Split-Path -Parent $libclang)
$env:LIB = "$($dxcore.DirectoryName);$env:LIB"
$env:CMAKE_GENERATOR = 'NMake Makefiles'
Remove-Item Env:CMAKE_GENERATOR_INSTANCE -ErrorAction SilentlyContinue
$env:Path = "$rustBin;$($cmakeRoot + '\bin');$(Split-Path -Parent $libclang);$($ffmpegRoot + '\bin');$env:Path"

& cargo --version
& cmake --version | Select-Object -First 1
Write-Host "MSVC compiler: $((Get-Command cl -ErrorAction Stop).Source)"
Write-Host "Environment ready. Run: Set-Location '$repoRoot\engine'; cargo check -p concat --locked"
if (-not $SkipMontage) { & (Join-Path $PSScriptRoot 'setup-bgm.ps1') }
