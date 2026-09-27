[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$toolsRoot = Join-Path $repoRoot '.tools'
$vendorRoot = Join-Path $repoRoot 'vendor'
$source = Join-Path $vendorRoot 'bgm-montage'
$liteSource = Join-Path $vendorRoot 'bmts-lite'
$python = Join-Path $toolsRoot 'bgm-venv\Scripts\python.exe'
$requirements = Join-Path $source 'requirements.lock.txt'
$stamp = Join-Path $toolsRoot 'bgm-requirements.sha256'
$ffmpegBin = Join-Path $vendorRoot 'ffmpeg-n8.1-latest-win64-gpl-shared-8.1\bin'
$commit = 'ff89f181645e4ebf952e2eb1ed99efad0e23d2e7'
New-Item -ItemType Directory -Force -Path $toolsRoot, $vendorRoot, (Join-Path $toolsRoot 'pip-cache'), (Join-Path $toolsRoot 'tmp') | Out-Null

if (-not (Test-Path -LiteralPath $source)) {
    & git clone --branch v1.4.6 --depth 1 https://github.com/sharbvane/bgm-montage.git $source
    if ($LASTEXITCODE -ne 0) { throw 'Could not download BGM Montage v1.4.6.' }
}
if (-not (Test-Path -LiteralPath (Join-Path $source '.git'))) {
    throw "BGM Montage source is not a Git checkout: $source"
}
$actual = & git -C $source rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actual.Trim() -ne $commit) {
    throw "BGM Montage must be pinned to v1.4.6 ($commit); found $actual"
}

$liteCommit = '883b3c05b35a8974bb8a1b0dde61e521535f0b15'
if (-not (Test-Path -LiteralPath (Join-Path $liteSource 'worker.py'))) {
    $archive = Join-Path $toolsRoot "downloads\bmts-lite-$liteCommit.zip"
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $archive) | Out-Null
    if (-not (Test-Path -LiteralPath $archive)) {
        & curl.exe --fail --location --retry 3 --output $archive "https://codeload.github.com/sharbvane/BMTS-Lite/zip/$liteCommit"
        if ($LASTEXITCODE -ne 0) { throw 'Could not download BMTS Lite.' }
    }
    Expand-Archive -LiteralPath $archive -DestinationPath $vendorRoot -Force
    $expanded = Join-Path $vendorRoot "BMTS-Lite-$liteCommit"
    if (-not (Test-Path -LiteralPath (Join-Path $expanded 'worker.py'))) {
        throw "BMTS Lite archive did not contain worker.py: $expanded"
    }
    $resolvedVendor = (Resolve-Path -LiteralPath $vendorRoot).Path
    $resolvedExpanded = (Resolve-Path -LiteralPath $expanded).Path
    if (-not $resolvedExpanded.StartsWith("$resolvedVendor\", [StringComparison]::OrdinalIgnoreCase)) {
        throw 'BMTS Lite extraction escaped vendor directory.'
    }
    Move-Item -LiteralPath $resolvedExpanded -Destination $liteSource
}
foreach ($file in @(
    @('worker.py', 'ee65e7849dac904f45cf30c7df4e6931e11db2fd'),
    @('engine\analyze_bgm.py', 'a121d333cf055c131e37a62162298aad034b3d73'),
    @('engine\timeline_planner.py', 'bd4256e3d81d9bcaf982ce5c676d21e4fd27f695'),
    @('engine\montage.py', 'bbad0b2caea83eb3d7c4e3cd852841fa7abb5669'),
    @('engine\visual_intelligence.py', '78287fefbd01443505b0ee3ecab9a1162f94c30d')
)) {
    $liteHash = & git hash-object (Join-Path $liteSource $file[0])
    if ($LASTEXITCODE -ne 0 -or $liteHash.Trim() -ne $file[1]) {
        throw "BMTS Lite source does not match the pinned release: $($file[0])"
    }
}

if (-not (Test-Path -LiteralPath $python)) {
    & py -3.11 -m venv (Join-Path $toolsRoot 'bgm-venv')
    if ($LASTEXITCODE -ne 0) { throw 'Python 3.11 is required to create the project-local BGM environment.' }
}
$env:PIP_CACHE_DIR = Join-Path $toolsRoot 'pip-cache'
$env:TEMP = Join-Path $toolsRoot 'tmp'
$env:TMP = $env:TEMP
$env:PIP_DISABLE_PIP_VERSION_CHECK = '1'
$hash = (Get-FileHash -LiteralPath $requirements -Algorithm SHA256).Hash
$installed = (Test-Path -LiteralPath $stamp) -and ((Get-Content -LiteralPath $stamp -Raw).Trim() -eq $hash)
if (-not $installed) {
    & $python -m pip install --requirement $requirements
    if ($LASTEXITCODE -ne 0) { throw 'BGM Montage dependency installation failed.' }
    & $python -m pip check
    if ($LASTEXITCODE -ne 0) { throw 'BGM Montage dependencies conflict.' }
    Set-Content -LiteralPath $stamp -Value $hash -NoNewline
}
if (-not (Test-Path -LiteralPath (Join-Path $ffmpegBin 'ffmpeg.exe'))) {
    throw "FFmpeg is missing: $ffmpegBin. Run scripts/setup-dev.ps1 first."
}
$env:Path = "$ffmpegBin;$env:Path"
& $python (Join-Path $source 'scripts\bgm_montage.py') --version
if ($LASTEXITCODE -ne 0) { throw 'BGM Montage CLI did not start.' }
Write-Host "BGM Montage v1.4.6 ready in $source"
Write-Host "BMTS Lite ready in $liteSource"
