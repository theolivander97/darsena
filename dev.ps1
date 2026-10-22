# Prepara el toolchain (Rust GNU + WinLibs en D:\tools) y arranca Dársena.
# Uso:  .\dev.ps1          -> modo desarrollo
#       .\dev.ps1 build    -> instalador release
param([string]$cmd = 'dev')

$env:RUSTUP_HOME = 'D:\tools\rustup'
$env:CARGO_HOME = 'D:\tools\cargo'
$env:npm_config_cache = 'D:\tools\npm-cache'
$env:PATH = "D:\tools\cargo\bin;D:\tools\winlibs\mingw64\bin;$env:PATH"

Set-Location $PSScriptRoot
npm run $cmd
