$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
foreach ($tool in @('node','npm','cargo')) {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
    throw "Missing $tool. See README.md for Windows prerequisites."
  }
}
npm ci
if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
npm test
if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
cargo test --locked --manifest-path src-tauri/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }
npm run desktop:build
if ($LASTEXITCODE -ne 0) { throw 'Tauri build failed' }
Write-Host 'Installer: src-tauri\target\release\bundle\nsis\'
