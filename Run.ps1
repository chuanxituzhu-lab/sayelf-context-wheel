$ErrorActionPreference='Stop'
$enginePath=Join-Path $PSScriptRoot 'bin\context-wheel.exe'
if (!(Test-Path -LiteralPath $enginePath)) {$enginePath=Join-Path $PSScriptRoot 'src-tauri\target\debug\context-wheel.exe'}
if (!(Test-Path -LiteralPath $enginePath)) {throw '请先在项目目录执行 npm ci 和 npm run tauri -- build --debug --no-bundle'}
Start-Process -FilePath $enginePath -WindowStyle Hidden
