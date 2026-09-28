[CmdletBinding()]
param([switch]$SkipBuild, [switch]$SkipZip)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$toolsRoot = Join-Path $repoRoot '.tools'
$downloads = Join-Path $toolsRoot 'downloads'
$stage = Join-Path $toolsRoot 'package-windows\SharbCut'
$release = Join-Path $repoRoot 'release'
$env:TEMP = Join-Path $toolsRoot 'tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $downloads, $env:TEMP, $release | Out-Null

function Get-PinnedDownload([string]$url, [string]$file, [long]$expectedSize) {
    if ((Test-Path -LiteralPath $file) -and (Get-Item -LiteralPath $file).Length -eq $expectedSize) { return }
    & curl.exe --fail --location --retry 3 --connect-timeout 15 --output $file $url
    if ($LASTEXITCODE -ne 0 -or (Get-Item -LiteralPath $file).Length -ne $expectedSize) {
        throw "Download failed or has unexpected size: $url"
    }
}

& (Join-Path $PSScriptRoot 'setup-dev.ps1')
if (-not $SkipBuild) {
    Push-Location (Join-Path $repoRoot 'engine')
    try {
        & cargo build -p concat --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    } finally { Pop-Location }
}

$exe = Join-Path $repoRoot 'target\release\concat.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "Release executable missing: $exe" }
$resolvedStage = [IO.Path]::GetFullPath($stage)
$resolvedTools = [IO.Path]::GetFullPath($toolsRoot)
if (-not $resolvedStage.StartsWith("$resolvedTools\", [StringComparison]::OrdinalIgnoreCase)) {
    throw "Packaging stage is outside .tools: $resolvedStage"
}
if (Test-Path -LiteralPath $resolvedStage) { Remove-Item -LiteralPath $resolvedStage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item -LiteralPath $exe -Destination (Join-Path $stage 'SharbCut.exe')

$vcRuntime = Join-Path $env:VCToolsRedistDir 'x64\Microsoft.VC143.CRT'
if (-not (Test-Path -LiteralPath (Join-Path $vcRuntime 'vcruntime140.dll'))) {
    throw "MSVC x64 runtime not found: $vcRuntime"
}
Copy-Item -Path (Join-Path $vcRuntime '*.dll') -Destination $stage

$ffmpegBin = Join-Path $env:FFMPEG_DIR 'bin'
Copy-Item -Path (Join-Path $ffmpegBin '*.dll') -Destination $stage
$runtime = Join-Path $stage 'bgm-runtime'
$runtimeFfmpeg = Join-Path $runtime 'ffmpeg\bin'
New-Item -ItemType Directory -Force -Path $runtimeFfmpeg | Out-Null
Copy-Item -Path (Join-Path $ffmpegBin '*') -Destination $runtimeFfmpeg

foreach ($name in @('DirectML.dll', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll')) {
    $source = @((Join-Path $repoRoot "target\release\$name"), (Join-Path $repoRoot "target\debug\$name")) |
        Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $source) { throw "Native runtime DLL missing: $name" }
    Copy-Item -LiteralPath $source -Destination $stage
}

$bgmSource = Join-Path $repoRoot 'vendor\bgm-montage'
$bgmTarget = Join-Path $runtime 'bgm-montage'
New-Item -ItemType Directory -Force -Path $bgmTarget | Out-Null
foreach ($name in @('scripts', 'agents', 'references', 'LICENSE', 'LICENSE-NOTICE.md', 'README.md')) {
    Copy-Item -LiteralPath (Join-Path $bgmSource $name) -Destination $bgmTarget -Recurse
}
$liteSource = Join-Path $repoRoot 'vendor\bmts-lite'
$liteTarget = Join-Path $runtime 'bmts-lite'
New-Item -ItemType Directory -Force -Path $liteTarget | Out-Null
foreach ($name in @('engine', 'worker.py', 'LICENSE', 'LICENSE-NOTICE.md', 'README.md')) {
    Copy-Item -LiteralPath (Join-Path $liteSource $name) -Destination $liteTarget -Recurse
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'bmts-lite-plan.py') -Destination $runtime

$pythonArchive = Join-Path $downloads 'python-3.11.9-embed-amd64.zip'
Get-PinnedDownload 'https://www.python.org/ftp/python/3.11.9/python-3.11.9-embed-amd64.zip' $pythonArchive 11249023
$pythonDir = Join-Path $runtime 'python'
Expand-Archive -LiteralPath $pythonArchive -DestinationPath $pythonDir
$python = Join-Path $pythonDir 'python.exe'
if ((Get-AuthenticodeSignature -LiteralPath $python).Status -ne 'Valid') { throw 'Python runtime signature is invalid.' }
$pythonLib = Join-Path $pythonDir 'Lib'
New-Item -ItemType Directory -Force -Path $pythonLib | Out-Null
Copy-Item -LiteralPath (Join-Path $toolsRoot 'bgm-venv\Lib\site-packages') -Destination $pythonLib -Recurse
$headers = Join-Path $pythonLib 'site-packages\torch\include'
$resolvedHeaders = [IO.Path]::GetFullPath($headers)
if (-not $resolvedHeaders.StartsWith("$resolvedStage\", [StringComparison]::OrdinalIgnoreCase)) {
    throw "Packaged build headers are outside stage: $resolvedHeaders"
}
if (Test-Path -LiteralPath $resolvedHeaders) { Remove-Item -LiteralPath $resolvedHeaders -Recurse -Force }
@('python311.zip', '.', 'Lib\site-packages', '..\bgm-montage\scripts', 'import site') |
    Set-Content -LiteralPath (Join-Path $pythonDir 'python311._pth') -Encoding ascii
& $python -c 'import numpy, librosa, torch, transformers, cv2'
if ($LASTEXITCODE -ne 0) { throw 'Embedded Python dependencies failed to import.' }
& $python (Join-Path $bgmTarget 'scripts\bgm_montage.py') --version
if ($LASTEXITCODE -ne 0) { throw 'Packaged BGM Montage could not start.' }

$notices = Join-Path $stage 'ThirdParty'
New-Item -ItemType Directory -Force -Path $notices | Out-Null
foreach ($name in @('README.md', 'LICENSE', 'LICENSE-EXCEPTIONS.md', 'THIRD_PARTY_NOTICES.md')) {
    Copy-Item -LiteralPath (Join-Path $repoRoot $name) -Destination $stage
}
Copy-Item -LiteralPath (Join-Path $env:FFMPEG_DIR 'LICENSE.txt') -Destination (Join-Path $notices 'FFmpeg-LICENSE.txt')
Copy-Item -LiteralPath (Join-Path $pythonDir 'LICENSE.txt') -Destination (Join-Path $notices 'Python-LICENSE.txt')

$iscc = Join-Path $toolsRoot 'inno-setup\ISCC.exe'
if (-not (Test-Path -LiteralPath $iscc)) {
    $installer = Join-Path $downloads 'innosetup-6.7.3.exe'
    Get-PinnedDownload 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe' $installer 10592232
    if ((Get-AuthenticodeSignature -LiteralPath $installer).Status -ne 'Valid') { throw 'Inno Setup signature is invalid.' }
    $arguments = @('/PORTABLE=1', '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/CURRENTUSER', "/DIR=$(Join-Path $toolsRoot 'inno-setup')")
    $process = Start-Process -FilePath $installer -ArgumentList $arguments -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $iscc)) { throw 'Inno Setup installation failed.' }
}
$chinese = Join-Path $toolsRoot 'inno-setup\Languages\ChineseSimplified.isl'
if (-not (Test-Path -LiteralPath $chinese) -or (Get-Item -LiteralPath $chinese).Length -lt 1000) {
    & curl.exe --fail --location --retry 3 --connect-timeout 15 --max-time 60 --output $chinese 'https://raw.githubusercontent.com/jrsoftware/issrc/main/Files/Languages/ChineseSimplified.isl'
    if ($LASTEXITCODE -ne 0 -or (Get-Item -LiteralPath $chinese -ErrorAction SilentlyContinue).Length -lt 1000) {
        Remove-Item -LiteralPath $chinese -Force -ErrorAction SilentlyContinue
        Write-Warning 'Chinese installer translation was unavailable; English installer will still be built.'
    }
}
$env:SHARBCUT_STAGE = $stage
$env:SHARBCUT_RELEASE = $release
& $iscc /Qp (Join-Path $PSScriptRoot 'sharbcut-installer.iss')
if ($LASTEXITCODE -ne 0) { throw 'SharbCut installer build failed.' }
if (-not $SkipZip) {
    # Only the ZIP keeps settings and downloaded models beside the executable.
    New-Item -ItemType Directory -Force -Path (Join-Path $stage 'portable') | Out-Null
    Compress-Archive -LiteralPath $stage -DestinationPath (Join-Path $release 'SharbCut-Windows-x64.zip') -CompressionLevel Optimal -Force
}
Write-Host "Windows release ready: $(Join-Path $release 'SharbCutSetup.exe')"
