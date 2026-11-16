param([string]$Binary = "target/debug/copybook.exe", [switch]$Autostart, [switch]$CheckSecondLaunch)
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$binaryPath = (Resolve-Path (Join-Path $repo "src-tauri/$Binary")).Path
$testDir = Join-Path $repo (".review/smoke-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $testDir | Out-Null
$original = Get-Clipboard -Raw
Add-Type -Path (Join-Path $PSScriptRoot "native-smoke.cs")
$originalForeground = [CopybookSmokeWindow]::GetForegroundWindow()
$targetWindow = [IntPtr]::Zero
$oldDataDir = $env:COPYBOOK_DATA_DIR
$env:COPYBOOK_DATA_DIR = $testDir
$env:COPYBOOK_SMOKE_TEXT = "copybook smoke " + [guid]::NewGuid().ToString("N")
$appProcess = $null
try {
    $launch = @{ FilePath=$binaryPath; WindowStyle="Hidden"; PassThru=$true; RedirectStandardOutput=(Join-Path $testDir "stdout.log"); RedirectStandardError=(Join-Path $testDir "stderr.log") }
    if ($Autostart) { $launch.ArgumentList="--autostart" }
    $appProcess = Start-Process @launch
    $ready = $false
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        if ($appProcess.HasExited) { throw "Copybook exited before startup" }
        $log = Join-Path $testDir "logs/copybook.log"
        if ((Test-Path -LiteralPath $log) -and (Select-String -LiteralPath $log -SimpleMatch "Backend ready" -Quiet)) { $ready = $true; break }
        Start-Sleep -Milliseconds 200
    }
    if (-not $ready) { throw "Copybook startup timeout" }
    Write-Output "Startup: backend ready; isolated data, logs, and WebView2 profile"
    $visible = [CopybookSmokeWindow]::VisibleWindows($appProcess.Id)
    $expectedVisible = if ($Autostart) { 0 } else { 1 }
    if ($visible -ne $expectedVisible) { throw "Unexpected startup windows: expected $expectedVisible, got $visible" }
    Write-Output "Startup visible windows: $visible (expected $expectedVisible)"
    # Set-Clipboard uses a null HWND owner. The contract then attributes it to the
    # foreground process, so provide a real external target after first-run settings opens.
    $targetWindow = [CopybookSmokeWindow]::CreateWindowExW(0, "STATIC", "Copybook smoke target", 0x10CF0000, 40, 40, 280, 140, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero)
    if ($targetWindow -eq [IntPtr]::Zero) { throw "Could not create smoke target" }
    if (-not [CopybookSmokeWindow]::Focus($targetWindow)) { throw "Smoke target did not receive foreground focus" }
    Set-Clipboard -Value $env:COPYBOOK_SMOKE_TEXT
    Start-Sleep -Milliseconds 1500
    python (Join-Path $PSScriptRoot "smoke_verify.py") 1
    if ($LASTEXITCODE -ne 0) { throw "First capture verification failed" }
    Set-Clipboard -Value $env:COPYBOOK_SMOKE_TEXT
    Start-Sleep -Milliseconds 1500
    python (Join-Path $PSScriptRoot "smoke_verify.py") 2
    if ($LASTEXITCODE -ne 0) { throw "Dedupe verification failed" }
    if ($CheckSecondLaunch) {
        $second = Start-Process -FilePath $binaryPath -WindowStyle Hidden -PassThru
        if (-not $second.WaitForExit(5000)) { Stop-Process -Id $second.Id; throw "Second launch did not exit" }
        $focused = $false
        for ($attempt = 0; $attempt -lt 30; $attempt++) {
            $foregroundOwner=0
            [void][CopybookSmokeWindow]::GetWindowThreadProcessId([CopybookSmokeWindow]::GetForegroundWindow(),[ref]$foregroundOwner)
            if ($foregroundOwner -eq $appProcess.Id) { $focused=$true; break }
            Start-Sleep -Milliseconds 10
        }
        if (-not $focused -or [CopybookSmokeWindow]::VisibleWindows($appProcess.Id) -lt 1) { throw "Second launch did not show and focus the panel" }
        Write-Output "Single instance: second process exited; original panel shown and foreground"
    }
} finally {
    if ($appProcess -and -not $appProcess.HasExited) { Stop-Process -Id $appProcess.Id; $appProcess.WaitForExit(); Write-Output "Stopped Copybook smoke process" }
    if ($targetWindow -ne [IntPtr]::Zero) { [void][CopybookSmokeWindow]::DestroyWindow($targetWindow) }
    [void][CopybookSmokeWindow]::SetForegroundWindow($originalForeground)
    $env:COPYBOOK_DATA_DIR = $oldDataDir
    Remove-Item Env:COPYBOOK_SMOKE_TEXT -ErrorAction SilentlyContinue
    if ($null -ne $original) { Set-Clipboard -Value $original } else { Set-Clipboard -Value "" }
    Write-Output "Restored clipboard text"
}
