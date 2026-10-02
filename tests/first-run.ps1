param([string]$Engine = "$PSScriptRoot\..\src-tauri\target\debug\context-wheel.exe")
$ErrorActionPreference = 'Stop'
$Engine = [IO.Path]::GetFullPath($Engine)
if (!(Test-Path -LiteralPath $Engine)) { throw "Debug executable not found: $Engine" }
Add-Type -Path "$PSScriptRoot\NativeTest.cs"
$original = [NativeTest]::GetForegroundWindow()
$testRoot = Join-Path $env:TEMP ("ContextWheelFirstRun-" + [guid]::NewGuid().ToString('N'))
$configDir = Join-Path $testRoot 'config'
$trace = Join-Path $testRoot 'trace.txt'
New-Item -ItemType Directory -Force -Path $configDir | Out-Null
$env:CWE_TEST_INPUT = '1'
$env:CWE_TEST_TRACE = $trace
$env:CWE_TEST_CONFIG_DIR = $configDir
$env:CWE_TEST_INSTANCE_ID = [guid]::NewGuid().ToString('N')
$env:CWE_TEST_SKIP_HOOK = '1'
$process = $null
function Assert($condition, $message) {
    if (!$condition) { throw $message }
    Write-Output "PASS $message"
}
function Start-EngineAndWait($expectFirstRun) {
    [IO.File]::WriteAllText($trace, '')
    $script:process = Start-Process -FilePath $Engine -WindowStyle Hidden -PassThru
    $deadline = (Get-Date).AddSeconds(30)
    $studio = [IntPtr]::Zero
    while ((Get-Date) -lt $deadline -and !$process.HasExited) {
        $events = if (Test-Path -LiteralPath $trace) { [IO.File]::ReadAllText($trace) } else { '' }
        $studio = [NativeTest]::FindTitle('Profile Studio')
        $wanted = if ($expectFirstRun) { 'startup:first_run_studio' } else { 'startup:hook_skipped_for_test' }
        if ($events.Contains($wanted) -and $studio -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 150
    }
    Assert (!$process.HasExited) 'application remains running'
    $events = [IO.File]::ReadAllText($trace)
    Assert ($events.Contains('startup:hook_skipped_for_test')) 'test instance avoids the global mouse hook'
    Assert ($studio -ne [IntPtr]::Zero) 'Profile Studio window created'
    if ($expectFirstRun) {
        Assert ($events.Contains('startup:first_run_studio')) 'first run opens Profile Studio'
        Assert ([NativeTest]::IsWindowVisible($studio)) 'first-run settings are visible'
    } else {
        Assert (!$events.Contains('startup:first_run_studio')) 'existing profile does not trigger onboarding again'
        Assert (![NativeTest]::IsWindowVisible($studio)) 'existing installation stays in tray'
    }
}
try {
    Start-EngineAndWait $true
    $profilePath = Join-Path $configDir 'profiles.yaml'
    Assert (Test-Path -LiteralPath $profilePath) 'first run creates the default profile'
    $process.Kill()
    $process.WaitForExit()
    $process = $null
    Start-Sleep -Milliseconds 400
    Start-EngineAndWait $false
} finally {
    if ($process -and !$process.HasExited) { $process.Kill(); $process.WaitForExit() }
    if ($original -ne [IntPtr]::Zero) { [NativeTest]::Focus($original) | Out-Null }
    Remove-Item Env:CWE_TEST_INPUT -ErrorAction SilentlyContinue
    Remove-Item Env:CWE_TEST_TRACE -ErrorAction SilentlyContinue
    Remove-Item Env:CWE_TEST_CONFIG_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:CWE_TEST_INSTANCE_ID -ErrorAction SilentlyContinue
    Remove-Item Env:CWE_TEST_SKIP_HOOK -ErrorAction SilentlyContinue
    $safeTemp = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    if (!$resolvedTestRoot.StartsWith($safeTemp, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to clean a path outside the temporary directory: $resolvedTestRoot"
    }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force -ErrorAction SilentlyContinue
}
