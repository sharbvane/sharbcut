[CmdletBinding()]
param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
& (Join-Path $PSScriptRoot 'setup-dev.ps1')

if (-not $SkipBuild) {
    Push-Location (Join-Path $repoRoot 'engine')
    try {
        & cargo build -p concat --locked
        if ($LASTEXITCODE -ne 0) { throw 'SharbCut build failed.' }
    } finally {
        Pop-Location
    }
}

$outputDir = Join-Path $repoRoot 'target\debug'
$exe = Join-Path $outputDir 'concat.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "SharbCut executable not found: $exe" }
Copy-Item -Path (Join-Path $env:FFMPEG_DIR 'bin\*.dll') -Destination $outputDir -Force
& $exe
