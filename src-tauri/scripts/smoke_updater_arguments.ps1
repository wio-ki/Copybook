# NSIS argument parser smoke test; the probe only writes a file beside itself.
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$makensis = Join-Path $repo 'src-tauri/target/.tauri/NSIS/makensis.exe'
if (-not (Test-Path -LiteralPath $makensis)) { throw 'Build an NSIS bundle first to cache makensis.' }
$probeDir = Join-Path $repo ('.review/updater-args-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $probeDir | Out-Null
$probeScript = Join-Path $probeDir 'probe.nsi'
$probeExe = Join-Path $probeDir 'probe.exe'
$source = @'
Unicode true
RequestExecutionLevel user
Name "Copybook updater argument probe"
OutFile "__OUTPUT__"
SilentInstall silent
AutoCloseWindow true
InstallDir "unused"
!include FileFunc.nsh
Section
  ${GetOptions} $CMDLINE "/ARGS" $R0
  FileOpen $0 "$EXEDIR\arguments.txt" w
  FileWriteUTF16LE $0 "$R0$\r$\n$INSTDIR"
  FileClose $0
SectionEnd
'@
$source.Replace('__OUTPUT__', $probeExe) | Set-Content -LiteralPath $probeScript -Encoding utf8
& $makensis '/V2' $probeScript
if ($LASTEXITCODE -ne 0) { throw 'Could not build the harmless NSIS argument probe.' }
$restartArgs = '--data-dir "C:\中文 data\\" --autostart'
$installDir = 'C:\中文 install path'
$parameters = '/P /UPDATE /R /ARGS ' + $restartArgs + ' /D=' + $installDir
$locked = [System.IO.File]::Open($probeExe, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
try {
    $probe = Start-Process -FilePath $probeExe -ArgumentList $parameters -WindowStyle Hidden -PassThru
    if (-not $probe.WaitForExit(10000)) {
        Stop-Process -Id $probe.Id
        throw 'NSIS argument probe timed out.'
    }
    if ($probe.ExitCode -ne 0) { throw 'NSIS argument probe failed.' }
} finally {
    $locked.Dispose()
}
$actual = Get-Content -LiteralPath (Join-Path $probeDir 'arguments.txt') -Encoding Unicode
if ($actual.Count -ne 2 -or $actual[0] -ne $restartArgs -or $actual[1] -ne $installDir) {
    throw 'NSIS did not preserve quoted restart arguments and the final install directory.'
}
Write-Output 'NSIS /ARGS preserved Unicode, spaces and trailing backslashes; final unquoted /D stayed separate.'
Write-Output 'NSIS started and read its own payload while held with read-only sharing.'
Write-Output 'Probe only wrote its local result file; no application was installed or launched.'
