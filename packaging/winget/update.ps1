# Fills the winget manifests from a GitHub release and writes them to packaging/winget/out/<version>.
# Usage:  .\update.ps1 -Version 0.6.0
# Then submit with:  wingetcreate submit out\<version>   (or open a pull request to microsoft/winget-pkgs yourself)
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$repo = 'mukii936/LarmaMic'
$url = "https://github.com/$repo/releases/download/v$Version/larma-mic.exe"
$tmp = Join-Path $env:TEMP "larma-mic-$Version.exe"
Invoke-WebRequest -UseBasicParsing $url -OutFile $tmp
$sha = (Get-FileHash $tmp -Algorithm SHA256).Hash
$date = Get-Date -Format 'yyyy-MM-dd'
$out = Join-Path $PSScriptRoot "out\$Version"
New-Item -ItemType Directory -Force $out | Out-Null
foreach ($f in 'mukii936.LarmaMic.yaml', 'mukii936.LarmaMic.installer.yaml', 'mukii936.LarmaMic.locale.en-US.yaml') {
    (Get-Content (Join-Path $PSScriptRoot "template\$f") -Raw).Replace('{{VERSION}}', $Version).Replace('{{URL}}', $url).Replace('{{SHA256}}', $sha).Replace('{{DATE}}', $date) |
        Set-Content (Join-Path $out $f) -Encoding utf8
}
Write-Host "Manifests written to $out"
