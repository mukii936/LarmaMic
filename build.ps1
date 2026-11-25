# Build LarmaMic: trang iPhone (React + TS) -> nhúng vào app Rust -> larma-mic.exe
# Chạy:  .\build.ps1
$ErrorActionPreference = 'Stop'
$env:Path += ";$env:USERPROFILE\.cargo\bin"
Set-Location $PSScriptRoot

Push-Location web
if (-not (Test-Path node_modules)) { npm install --no-audit --no-fund }
npm run build
Pop-Location

cargo build --release
Write-Host "`nXong: $PSScriptRoot\target\release\larma-mic.exe"
