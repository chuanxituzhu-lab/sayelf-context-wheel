param([string]$Engine = "$PSScriptRoot\..\src-tauri\target\debug\context-wheel.exe",[string]$EvidenceDir = "$PSScriptRoot\..\..\..\work\e2e",[ValidateSet('xbutton1','xbutton2','middle')][string]$Trigger='xbutton1',[switch]$ExecutorCases)
$ErrorActionPreference='Stop'
$EvidenceDir=[IO.Path]::GetFullPath($EvidenceDir)
New-Item -ItemType Directory -Force -Path $EvidenceDir | Out-Null
New-Item -ItemType Directory -Force -Path "$EvidenceDir\config" | Out-Null
$button=@{xbutton1=1;xbutton2=2;middle=3}[$Trigger]
[IO.File]::WriteAllText("$EvidenceDir\config\profiles.yaml",(Get-Content "$PSScriptRoot\..\src-tauri\profiles\default.yaml" -Raw).Replace('trigger: xbutton1',"trigger: $Trigger"))
foreach($keyFile in @('default-keys.txt','sketchup-keys.txt','acad-keys.txt')){[IO.File]::WriteAllText("$EvidenceDir\$keyFile",'')}
& "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe" /nologo /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll "/out:$EvidenceDir\SketchUp.exe" "$PSScriptRoot\Fixture.cs"
if ($LASTEXITCODE -ne 0) { throw 'Fixture compile failed' }
Copy-Item -LiteralPath "$EvidenceDir\SketchUp.exe" -Destination "$EvidenceDir\DefaultFixture.exe" -Force
Copy-Item -LiteralPath "$EvidenceDir\SketchUp.exe" -Destination "$EvidenceDir\acad.exe" -Force
if($ExecutorCases){
 Copy-Item -LiteralPath "$EvidenceDir\SketchUp.exe" -Destination "$EvidenceDir\LaunchFixture.exe" -Force
 node "$PSScriptRoot\config-actions.mjs" $EvidenceDir;if($LASTEXITCODE -ne 0){throw 'Test config generation failed'}
 [IO.File]::WriteAllText("$EvidenceDir\config\usage.json",'{"version":1,"enabled":false,"theme":"cad_monochrome","counts":{},"layouts":{},"active_modes":{"acad.exe":"model"}}')
 Add-Type -AssemblyName System.IO.Compression
 Add-Type -AssemblyName System.IO.Compression.FileSystem
 Add-Type -AssemblyName System.Drawing
 $cuiPath="$EvidenceDir\autocad-test.cuix"
 if (Test-Path -LiteralPath $cuiPath) { Remove-Item -LiteralPath $cuiPath -Force }
 $archive=[IO.Compression.ZipFile]::Open($cuiPath,[IO.Compression.ZipArchiveMode]::Create)
 try {
  $entry=$archive.CreateEntry('MenuGroup.cui');$writer=[IO.StreamWriter]::new($entry.Open())
  $writer.Write('<MenuGroup><MenuMacro><Macro><Command>^C^C_LINE</Command><SmallImage Name="line_native" /></Macro></MenuMacro><MenuMacro><Macro><Command>^C^C_CIRCLE</Command><SmallImage Name="circle_native" /></Macro></MenuMacro></MenuGroup>');$writer.Dispose()
  foreach($icon in @(@{Name='line_native';Color=[Drawing.Color]::Aqua},@{Name='circle_native';Color=[Drawing.Color]::Orange})){
   $bitmap=[Drawing.Bitmap]::new(16,16);$graphics=[Drawing.Graphics]::FromImage($bitmap);$graphics.Clear($icon.Color);$memory=[IO.MemoryStream]::new();$bitmap.Save($memory,[Drawing.Imaging.ImageFormat]::Bmp)
   $imageEntry=$archive.CreateEntry("icons/$($icon.Name).bmp");$imageStream=$imageEntry.Open();$data=$memory.ToArray();$imageStream.Write($data,0,$data.Length);$imageStream.Dispose();$memory.Dispose();$graphics.Dispose();$bitmap.Dispose()
  }
 } finally {$archive.Dispose()}
 $env:CWE_TEST_CUIX=$cuiPath
}
Add-Type -Path "$PSScriptRoot\NativeTest.cs"
$original=[NativeTest]::GetForegroundWindow()
$env:CWE_TEST_INPUT='1';$env:CWE_TEST_TRACE="$EvidenceDir\trace.txt";$env:CWE_TEST_INSTANCE_ID=[guid]::NewGuid().ToString('N')
$env:CWE_TEST_CONFIG_DIR="$EvidenceDir\config"
[IO.File]::WriteAllText($env:CWE_TEST_TRACE,'')
$engineProcess=$null;$a=$null;$b=$null;$cad=$null;$launched=$null
function Assert($condition,$message){if (!$condition){throw $message};Write-Output "PASS $message"}
function Focus($p){for($attempt=0;$attempt -lt 8;$attempt++){$p.Refresh();[NativeTest]::Focus($p.MainWindowHandle) | Out-Null;Start-Sleep -Milliseconds 200;if([NativeTest]::GetForegroundWindow() -eq $p.MainWindowHandle){break}};if([NativeTest]::GetForegroundWindow() -ne $p.MainWindowHandle){Write-Output "main=$($p.MainWindowHandle) actual=$([NativeTest]::GetForegroundWindow())";Write-Output ([NativeTest]::TitlesForProcess($p.Id))};Assert ([NativeTest]::GetForegroundWindow() -eq $p.MainWindowHandle) 'fixture focus'}
try {
 $a=Start-Process -FilePath "$EvidenceDir\DefaultFixture.exe" -ArgumentList "`"$EvidenceDir\default-keys.txt`"" -WindowStyle Hidden -PassThru
 $b=Start-Process -FilePath "$EvidenceDir\SketchUp.exe" -ArgumentList "`"$EvidenceDir\sketchup-keys.txt`"" -WindowStyle Hidden -PassThru
 $cad=Start-Process -FilePath "$EvidenceDir\acad.exe" -ArgumentList "`"$EvidenceDir\acad-keys.txt`"" -WindowStyle Hidden -PassThru
 $engineProcess=Start-Process -FilePath ([IO.Path]::GetFullPath($Engine)) -WindowStyle Hidden -PassThru
 $readyLimit=(Get-Date).AddSeconds(30)
 while ((([IO.File]::ReadAllText($env:CWE_TEST_TRACE)) -notmatch '(?m)^ready\r?$' -or ([IO.File]::ReadAllText($env:CWE_TEST_TRACE)) -notmatch 'startup:hook_installed') -and (Get-Date) -lt $readyLimit){Start-Sleep -Milliseconds 250}
 Assert (([IO.File]::ReadAllText($env:CWE_TEST_TRACE)) -match '(?m)^ready\r?$') 'renderer ready'
 # Explicitly show our own fixtures for a bounded foreground/input test.
 Add-Type 'using System;using System.Runtime.InteropServices;public class FixtureShow{[DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int n);}'
 [FixtureShow]::ShowWindow([NativeTest]::ForProcess($a.Id),5)|Out-Null;[FixtureShow]::ShowWindow([NativeTest]::ForProcess($b.Id),5)|Out-Null;[FixtureShow]::ShowWindow([NativeTest]::ForProcess($cad.Id),5)|Out-Null
 Focus $a
 $overlay=[NativeTest]::FindTitle('Context Wheel')
 if($overlay -eq [IntPtr]::Zero){Write-Output ([NativeTest]::TitlesForProcess($engineProcess.Id))}
 Assert ($overlay -ne [IntPtr]::Zero) 'overlay exists'
 Assert (![NativeTest]::EarlyVisible($overlay,$button)) 'visual delay keeps overlay hidden at 25ms'
 Start-Sleep -Milliseconds 150
 Assert ([NativeTest]::IsWindowVisible($overlay)) 'overlay visible after delay'
 Assert ([NativeTest]::GetForegroundWindow() -eq $a.MainWindowHandle) 'overlay does not steal focus'
 Assert (([IO.File]::ReadAllText($env:CWE_TEST_TRACE)) -match 'render:default.safe:8:none:8::.*center_logo=true') 'brand logo fills the center while no command is selected'
 [NativeTest]::Move(900,400);Start-Sleep -Milliseconds 60;[NativeTest]::Button($false,$button);Start-Sleep -Milliseconds 200
 Assert (![NativeTest]::IsWindowVisible($overlay)) 'overlay hidden on release'
 Assert ((Get-Content "$EvidenceDir\default-keys.txt" -Raw) -match 'key:V') 'default east sends Ctrl+V to isolated fixture'
 Focus $b
 [NativeTest]::Move(600,400);[NativeTest]::Button($true,$button);Start-Sleep -Milliseconds 180;[NativeTest]::Move(900,400);Start-Sleep -Milliseconds 60;[NativeTest]::Button($false,$button);Start-Sleep -Milliseconds 200
 Assert ((Get-Content "$EvidenceDir\sketchup-keys.txt" -Raw) -match 'key:S') 'SketchUp far-east drag reaches the 16-slot outer ring and sends S (scale)'
 if($ExecutorCases){
  Focus $cad
  [NativeTest]::Move(600,400);[NativeTest]::Button($true,$button);Start-Sleep -Milliseconds 180;[NativeTest]::Move(900,400);Start-Sleep -Milliseconds 60;[NativeTest]::Button($false,$button);Start-Sleep -Milliseconds 800
  $cadTrace=[IO.File]::ReadAllText($env:CWE_TEST_TRACE);$cadKeys=[IO.File]::ReadAllText("$EvidenceDir\acad-keys.txt")
  Assert ($cadTrace -match 'down:autocad.model') 'AutoCAD mode profile overrides application profile'
  Assert ($cadTrace -match 'render:autocad.model:24:12') 'AutoCAD wheel selects the east slot in its sixteen-way outer ring'
  Assert ($cadTrace -match 'render:autocad.model:24:12:24:圆:circle:true:native_icons=3') 'AutoCAD CUIx images follow the expanded ring indices and the selected command caption fits the center'
  Assert ($cadTrace -match 'native_loaded=3:native_fallbacks=0:native_failures=0') 'all AutoCAD outer-ring raster icons decode in the overlay without broken-image placeholders'
  Assert ($cadTrace -match 'render:autocad.model:24:12:24:圆:circle:true:native_icons=3:theme=theme-cad_monochrome:labels=24:labels_fitted=true') 'CAD monochrome theme renders all 24 fitted icon labels'
  Assert ($cadTrace -match 'render:autocad.model:24:12:24:圆:circle:true:native_icons=3:theme=theme-cad_monochrome:labels=24:labels_fitted=true:center_logo=false') 'center logo gives way to the selected CAD command caption'
  Assert ($cadKeys -match 'char:95' -and $cadKeys -match 'char:67' -and $cadKeys -match 'key:(Return|Enter)') 'outer-ring CIRCLE command types its alias and presses Enter'
 }
 $trace=Get-Content $env:CWE_TEST_TRACE -Raw
 Assert ($trace -match 'down:default.safe' -and $trace -match 'down:sketchup.default') 'switch process automatically switches profile'
 Assert ($trace -match 'render:default.safe:8:2' -and $trace -match 'render:sketchup.default:24:12') 'SVG renders 8 sectors (Default) and 8+16 sectors (SketchUp) with the east highlight'
 [NativeTest]::Move(600,400);[NativeTest]::Button($true,$button);Start-Sleep -Milliseconds 180;[NativeTest]::Button($false,$button);Start-Sleep -Milliseconds 150
 Assert (([IO.File]::ReadAllText($env:CWE_TEST_TRACE)) -match 'cancel:dead_zone') 'center cancels'
 [NativeTest]::Move(600,400);[NativeTest]::Button($true,$button);Start-Sleep -Milliseconds 180;[NativeTest]::Move(900,400);Focus $a;[NativeTest]::Button($false,$button);Start-Sleep -Milliseconds 150
 Assert (([IO.File]::ReadAllText($env:CWE_TEST_TRACE)) -match 'execute:rejected') 'changed foreground refuses command'
 Focus $b
 $before=(Get-Content "$EvidenceDir\sketchup-keys.txt" | Where-Object {$_ -eq 'key:Q'}).Count
 [NativeTest]::FastMark($button);Start-Sleep -Milliseconds 180
 $after=(Get-Content "$EvidenceDir\sketchup-keys.txt" | Where-Object {$_ -eq 'key:Q'}).Count
 Assert ($after -eq $before+1) 'fast marking executes once'
 Assert (![NativeTest]::IsWindowVisible($overlay)) 'fast release leaves no overlay'
 [NativeTest]::Move(4,4);[NativeTest]::Button($true,$button);Start-Sleep -Milliseconds 180
 $rect=New-Object NativeTest+Rect
 [NativeTest]::GetWindowRect($overlay,[ref]$rect)|Out-Null
 Assert ($rect.Left -ge 0 -and $rect.Top -ge 0) 'top-left edge avoidance'
 [NativeTest]::Button($false,$button);Start-Sleep -Milliseconds 100
 $desktop=[NativeTest]::GetShellWindow()
 [NativeTest]::Focus($desktop)|Out-Null;Start-Sleep -Milliseconds 200
 Assert ([NativeTest]::GetForegroundWindow() -eq $desktop) 'desktop focus'
 [NativeTest]::Move(600,400);[NativeTest]::Button($true,$button);Start-Sleep -Milliseconds 180;[NativeTest]::Button($false,$button);Start-Sleep -Milliseconds 150
 Assert (([IO.File]::ReadAllText($env:CWE_TEST_TRACE)) -match 'down:desktop.default') 'actual Windows desktop resolves Desktop profile'
 if($ExecutorCases){
  Focus $a
  [NativeTest]::Move(600,400);[NativeTest]::Button($true,$button);Start-Sleep -Milliseconds 120;[NativeTest]::Move(900,100);Start-Sleep -Milliseconds 50;[NativeTest]::Button($false,$button);Start-Sleep -Milliseconds 250
  $keys=[IO.File]::ReadAllText("$EvidenceDir\default-keys.txt")
  Assert ($keys -match 'char:955' -and $keys -match 'char:27979') 'Unicode keystroke reaches isolated fixture'
  $rejectedBefore=([regex]::Matches([IO.File]::ReadAllText($env:CWE_TEST_TRACE),'execute:rejected')).Count
  [NativeTest]::Move(600,400);[NativeTest]::Button($true,$button);Start-Sleep -Milliseconds 120;[NativeTest]::Move(300,400);Start-Sleep -Milliseconds 50;[NativeTest]::Button($false,$button);Start-Sleep -Milliseconds 200
  $rejectedAfter=([regex]::Matches([IO.File]::ReadAllText($env:CWE_TEST_TRACE),'execute:rejected')).Count
  Assert ($rejectedAfter -eq $rejectedBefore+1) 'Adapter placeholder refuses execution explicitly'
  [NativeTest]::Move(600,400);[NativeTest]::Button($true,$button);Start-Sleep -Milliseconds 120;[NativeTest]::Move(600,100);Start-Sleep -Milliseconds 50;[NativeTest]::Button($false,$button);Start-Sleep -Milliseconds 500
  $launched=Get-Process -Name LaunchFixture -ErrorAction SilentlyContinue | Where-Object {$_.MainModule.FileName -eq "$EvidenceDir\LaunchFixture.exe"} | Select-Object -First 1
  Assert ($null -ne $launched) 'Launch starts exact isolated program with arguments'
 }
 Write-Output "E2E $Trigger completed using isolated fixtures, not a real SketchUp installation."
} finally {
 foreach($p in @($engineProcess,$a,$b,$cad,$launched)){if($p -and !$p.HasExited){Stop-Process -Id $p.Id -Force}}
 [NativeTest]::Focus($original)|Out-Null
 Remove-Item Env:CWE_TEST_INPUT,Env:CWE_TEST_TRACE,Env:CWE_TEST_CONFIG_DIR,Env:CWE_TEST_INSTANCE_ID,Env:CWE_TEST_CUIX -ErrorAction SilentlyContinue
}
