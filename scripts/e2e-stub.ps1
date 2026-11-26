# End-to-end check with stand-in programs (no Unity, no Codex or Claude account, no limits spent).
# Builds `uis` and `uis-stub`, makes a throwaway bed in %TEMP% (a git copy of
# tests\fixtures\SampleProject, stub codex/claude/unity/Unity.exe, a tool home with settings that
# point at them, its own CODEX_HOME), then:
#   1. uis run: capture (batch, through WMI like Unity) -> prompt -> rewrite -> picture -> finish
#      -> checks -> export, for two prefabs with a note
#   2. failure: Codex says the usage limit was reached (the second item is not sent)
#   3. failure: the rewrite answer is not JSON (no picture is asked for)
#   4. failure: a prefab that does not exist (capture fails, the other item goes on)
# Exit code 0 when every check passed.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\e2e-stub.ps1 [-Keep] [-BedOnly] [-Bed <folder>]
#
# -BedOnly makes the bed and prints its paths (for driving the app window against the stubs, see
# scripts\drive-window.mjs); -Keep leaves the bed after the run.

[CmdletBinding()]
param(
    [switch]$Keep,
    [switch]$BedOnly,
    [string]$Bed = ''
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Bed) { $Bed = Join-Path $env:TEMP ('uis-e2e-' + [DateTime]::Now.ToString('yyyyMMdd-HHmmss')) }
$script:failures = 0

function Say([string]$text) { Write-Host $text }
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Host ('  ok    ' + $what) }
    else { Write-Host ('  FAIL  ' + $what) -ForegroundColor Red; $script:failures++ }
}
function Write-Utf8([string]$path, [string]$text) {
    [IO.File]::WriteAllText($path, $text, (New-Object Text.UTF8Encoding($false)))
}

Say 'build: uis, uis-stub'
Push-Location $repo
try {
    & cargo build -q -p uis-cli -p uis-stub
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
} finally { Pop-Location }
$uis = Join-Path $repo 'target\debug\uis.exe'
$stub = Join-Path $repo 'target\debug\uis-stub.exe'

function New-Bed([string]$root) {
    if (Test-Path $root) { Remove-Item -Recurse -Force $root }
    $null = New-Item -ItemType Directory -Force $root
    $game = Join-Path $root 'Game'
    Copy-Item -Recurse (Join-Path $repo 'tests\fixtures\SampleProject') $game
    $g = @('-C', $game, '-c', 'user.email=e2e', '-c', 'user.name=e2e', '-c', 'core.autocrlf=false')
    & git @g init -q
    & git @g config core.autocrlf false
    & git @g add -A
    & git @g commit -q -m fixture
    $bin = Join-Path $root 'bin'
    $null = New-Item -ItemType Directory -Force $bin
    foreach ($n in 'codex', 'claude', 'codex-code-mode-host', 'unity', 'Unity') { Copy-Item $stub (Join-Path $bin ($n + '.exe')) }
    $home2 = Join-Path $root 'home'
    $null = New-Item -ItemType Directory -Force $home2
    $settings = [ordered]@{
        schema      = 'unity-icon-studio.settings/1'
        exportDir   = (Join-Path $root 'out')
        exportSizes = @(256, 512)
        capture     = [ordered]@{
            unityCli = (Join-Path $bin 'unity.exe')
            editors  = @{ '6000.0.1f1' = (Join-Path $bin 'Unity.exe') }
        }
        codex       = [ordered]@{ path = (Join-Path $bin 'codex.exe'); imageTimeoutSec = 60; textTimeoutSec = 20 }
        claude      = [ordered]@{ path = (Join-Path $bin 'claude.exe'); timeoutSec = 20 }
    }
    Write-Utf8 (Join-Path $home2 'settings.json') ($settings | ConvertTo-Json -Depth 6)
    Set-Scenario $root "delay_ms=400`ntouch=Assets/Props/Crate.prefab.meta`n"
    return @{ Root = $root; Game = $game; Bin = $bin; Home = $home2; CodexHome = (Join-Path $root 'codex-home'); Out = (Join-Path $root 'out') }
}

function Set-Scenario($root, [string]$text) {
    Write-Utf8 (Join-Path $root 'bin\stub-scenario.txt') $text
    $log = Join-Path $root 'bin\calls.log'
    if (Test-Path $log) { Remove-Item -Force $log }
}

function Get-Calls($b, [string]$role) {
    $log = Join-Path $b.Bin 'calls.log'
    if (-not (Test-Path $log)) { return 0 }
    return @(Get-Content $log | Where-Object { $_ -like ($role + "`t*") }).Count
}

function Invoke-Run($b, [string[]]$more) {
    $env:UNITY_ICON_STUDIO_HOME = $b.Home
    $env:CODEX_HOME = $b.CodexHome
    $outFile = Join-Path $b.Root 'run-report.json'
    $errFile = Join-Path $b.Root 'run-steps.txt'
    try {
        $args2 = @('run', '--project', $b.Game, '--mode', 'batch', '--json') + $more
        # Quoted one by one (paths and the note have spaces; none has a quote)
        $line = ($args2 | ForEach-Object { '"' + $_ + '"' }) -join ' '
        $p = Start-Process -FilePath $uis -ArgumentList $line -NoNewWindow -Wait -PassThru -RedirectStandardOutput $outFile -RedirectStandardError $errFile
        $code = $p.ExitCode
    } finally {
        Remove-Item Env:UNITY_ICON_STUDIO_HOME -ErrorAction SilentlyContinue
        Remove-Item Env:CODEX_HOME -ErrorAction SilentlyContinue
    }
    $text = [IO.File]::ReadAllText($outFile)
    try { $report = $text | ConvertFrom-Json } catch { $report = $null }
    return @{ Code = $code; Report = $report; Steps = (Get-Content (Join-Path $b.Root 'run-steps.txt') -ErrorAction SilentlyContinue) }
}

$b = New-Bed $Bed
if ($BedOnly) {
    Say ''
    Say ('bed       ' + $b.Root)
    Say ('project   ' + $b.Game)
    Say ('home      ' + $b.Home + '   (UNITY_ICON_STUDIO_HOME)')
    Say ('codexHome ' + $b.CodexHome + '   (CODEX_HOME)')
    Say ('export    ' + $b.Out)
    Say ('scenario  ' + (Join-Path $b.Bin 'stub-scenario.txt'))
    exit 0
}

$crate = 'Assets/Props/Crate.prefab'
$house = 'Assets/Buildings/House.prefab'

Say ''
Say '1. capture -> prompt -> rewrite -> picture -> finish -> checks -> export'
$r = Invoke-Run $b @('--prefab', $crate, $house, '--note', 'snow on the roof, winter mood', '--to', $b.Out, '--export-failing')
$rep = $r.Report
Check ($r.Code -eq 0 -and $rep.ok) 'run ok (exit 0)'
Check ($rep.capture.mode -eq 'batch' -and $rep.capture.ok) 'one batch capture for both items'
Check (@($rep.items | Where-Object { $_.state -eq 'ok' }).Count -eq 2) 'both items made'
Check (@($rep.items | Where-Object { $_.rewrite.cli }).Count -eq 2) 'the note was rewritten for each item'
Check ((Get-Calls $b 'claude') -eq 2) 'two rewrite calls (claude)'
Check ((Get-Calls $b 'codex') -eq 2) 'two picture calls (codex)'
foreach ($it in $rep.items) {
    $post = $it.candidates[0].post
    Check ($null -ne $post -and @($post.sizes).Count -eq 2) ('finished 256 and 512: ' + $it.name)
    Check ($null -ne $post.sizes[0].checks.items -and @($post.sizes[0].checks.items).Count -ge 10) ('checks ran: ' + $it.name)
    Check ($it.export.ok -and @($it.export.files).Count -eq 2) ('exported: ' + $it.name)
    $job = Get-Content -Raw (Join-Path $it.jobDir 'job.json') | ConvertFrom-Json
    Check ($job.asset -eq $it.asset -and $job.runId -eq $rep.runId) ('job.json records the run: ' + $it.name)
}
Check (Test-Path (Join-Path $b.Out 'icon_Crate.png')) 'out\icon_Crate.png'
Check (Test-Path (Join-Path $b.Out 'icon_House_512.png')) 'out\icon_House_512.png'
Check (-not (Test-Path (Join-Path $b.Game 'Assets\__UnityIconStudioCapture'))) 'no capture entry left in the project'
$dirty = & git -C $b.Game -c core.autocrlf=false status --porcelain
Check ([string]::IsNullOrWhiteSpace(($dirty -join ''))) 'project git status clean after the run'
$steps = $r.Steps -join "`n"
foreach ($s in 'capture start', 'prompt ok', 'rewrite ok', 'generate ok', 'post ok', 'export ok', 'done ok') {
    Check ($steps -match [regex]::Escape($s)) ('step reported: ' + $s)
}

Say ''
Say '2. failure: usage limit (stops the items still waiting)'
Set-Scenario $b.Root "delay_ms=400`ncodex=limit`n"
$r = Invoke-Run $b @('--prefab', $crate, $house)
$rep = $r.Report
Check ($r.Code -ne 0 -and -not $rep.ok) 'run not ok'
Check ($rep.stopped -eq 'limit') 'stopped: limit'
Check ($rep.items[0].error -eq 'image:limit' -and $rep.items[0].stage -eq 'generate') 'first item failed at generate with image:limit'
Check ($rep.items[1].state -eq 'stopped') 'second item not sent'
Check ((Get-Calls $b 'codex') -eq 1) 'one picture call only'

Say ''
Say '3. failure: the rewrite answer is not JSON (no fixed prompt sent behind the note)'
Set-Scenario $b.Root "delay_ms=400`nclaude=bad_json`n"
$r = Invoke-Run $b @('--prefab', $crate, '--note', 'make it blue', '--cli', 'claude')
$rep = $r.Report
Check ($rep.items[0].state -eq 'failed' -and $rep.items[0].stage -eq 'rewrite') 'failed at rewrite'
Check ($rep.items[0].error -like 'rewrite:*') ('error code: ' + $rep.items[0].error)
Check ((Get-Calls $b 'codex') -eq 0) 'no picture call'

Say ''
Say '4. failure: a prefab that does not exist (the other item goes on)'
Set-Scenario $b.Root "delay_ms=400`n"
$r = Invoke-Run $b @('--prefab', 'Assets/Props/Nope.prefab', $crate)
$rep = $r.Report
Check ($rep.items[0].state -eq 'failed' -and $rep.items[0].stage -eq 'capture' -and $rep.items[0].error -eq 'prefab_not_found') 'missing prefab failed at capture'
Check ($rep.items[1].state -eq 'ok') 'the other item was made'

Say ''
if ($script:failures -eq 0) { Say 'e2e-stub: all checks passed' } else { Say ('e2e-stub: ' + $script:failures + ' check(s) failed') }
if ($Keep -or $script:failures) { Say ('bed kept: ' + $b.Root) } else { Remove-Item -Recurse -Force $b.Root -ErrorAction SilentlyContinue }
exit $script:failures
