# start-batch.ps1 - start Unity in batch mode through WMI (Win32_Process.Create) and print its id.
#
# The process is created by the WMI service, not as a child of the caller: it gets the user's normal
# environment (not the caller's) and its normal AppData, and closing the caller does not end it.
# This script only starts Unity. It never waits for it and never ends it.
#
# Output (stdout, one line): "pid=<id>" on success, "error=<text>" otherwise (exit 1).
param(
    [Parameter(Mandatory = $true)][string]$UnityExe,
    [Parameter(Mandatory = $true)][string]$ProjectPath,
    [Parameter(Mandatory = $true)][string]$LogFile,
    [Parameter(Mandatory = $true)][string]$JobFile,
    [string]$Method = 'UnityIconStudio.PrefabIconCapture.RunFromCommandLine'
)
$ErrorActionPreference = 'Stop'

function Quote-Arg([string]$a) {
    if ($a -eq '') { return '""' }
    if ($a -notmatch '[\s"]') { return $a }
    # Windows rules: backslashes before a quote are doubled, the quote is escaped
    $s = $a -replace '(\\*)"', '$1$1\"'
    $s = $s -replace '(\\+)$', '$1$1'
    return '"' + $s + '"'
}

try {
    if (-not (Test-Path -LiteralPath $UnityExe -PathType Leaf)) { Write-Output ('error=unity exe not found: ' + $UnityExe); exit 1 }
    $argList = @('-batchmode', '-quit', '-projectPath', $ProjectPath, '-logFile', $LogFile,
        '-executeMethod', $Method, '-iconStudioJob', $JobFile)
    $cmd = (Quote-Arg $UnityExe) + ' ' + (($argList | ForEach-Object { Quote-Arg $_ }) -join ' ')
    $r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = $cmd; CurrentDirectory = $ProjectPath }
    if ($r.ReturnValue -ne 0) { Write-Output ('error=Win32_Process.Create returned ' + $r.ReturnValue); exit 1 }
    Write-Output ('pid=' + $r.ProcessId)
    exit 0
}
catch {
    Write-Output ('error=' + $_.Exception.Message)
    exit 1
}
