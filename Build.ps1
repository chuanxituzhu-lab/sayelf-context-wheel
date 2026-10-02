# SAYELF Context Wheel - one-click build and test (Windows PowerShell 5.1+ / PowerShell 7)
# Usage, from the project folder:
#   powershell -ExecutionPolicy Bypass -File .\Build.ps1            # tests + installer
#   powershell -ExecutionPolicy Bypass -File .\Build.ps1 -SkipE2E   # skip the desktop E2E run
param([switch]$SkipE2E)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot

function Step($text) { Write-Host "`n==> $text" -ForegroundColor Cyan }
function Need($command, $hint) {
    if (-not (Get-Command $command -ErrorAction SilentlyContinue)) { throw "缺少 $command。$hint" }
}
function Run($exe, [string[]]$arguments) {
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "$exe $($arguments -join ' ') 失败（退出码 $LASTEXITCODE）" }
}

Step '检查工具链'
Need node  '请安装 Node.js 20.19 或更新版本：https://nodejs.org'
Need npm   '请安装 Node.js（自带 npm）'
Need cargo '请安装 Rust stable MSVC：https://rustup.rs'
$nodeVersion = [version]((node --version).TrimStart('v'))
if ($nodeVersion -lt [version]'20.19.0') { throw "Node.js 版本 $nodeVersion 过低，需要 20.19+" }
Write-Host "Node $nodeVersion / $(cargo --version)"

Step '安装前端依赖（按 package-lock.json 锁定版本）'
Run npm @('ci', '--no-audit', '--no-fund')

Step '配置与命令库校验'
Run node @('tests/schema.mjs')
Step '轮盘模型测试（16 格增减、对比度、图标匹配）'
Run node @('tests/wheel-model.test.mjs')
Step 'Rust 单元测试'
Run cargo @('test', '--manifest-path', 'src-tauri/Cargo.toml', '--locked')

Step '构建 NSIS 安装包'
Run npm @('run', 'tauri', '--', 'build', '--bundles', 'nsis')

if (-not $SkipE2E) {
    Step '桌面 E2E（隔离测试窗口，不碰真实 CAD 文档；约 1 分钟内会接管鼠标，请勿操作）'
    Run npm @('run', 'tauri', '--', 'build', '--debug', '--no-bundle')
    & powershell -ExecutionPolicy Bypass -File .\tests\e2e.ps1 -Trigger xbutton1 -ExecutorCases
    if ($LASTEXITCODE -ne 0) { throw 'E2E 测试失败，详见上方输出' }
}

$version = (Get-Content -Raw src-tauri\tauri.conf.json | ConvertFrom-Json).version
$installer = Get-ChildItem "src-tauri\target\release\bundle\nsis\*_${version}_x64-setup.exe" | Select-Object -First 1
if (-not $installer) { throw "没有找到 v$version 安装包" }
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer.FullName).Hash
Write-Host "`n完成：$($installer.FullName)" -ForegroundColor Green
Write-Host ("大小：{0:N1} MB   SHA256：{1}" -f ($installer.Length / 1MB), $hash)
