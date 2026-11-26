# End-to-end check with stand-in programs (no Unity, no Codex or Claude account, no limits spent).
# Builds `uis` and `uis-stub`, makes a throwaway bed in %TEMP% (a git copy of
# tests\fixtures\SampleProject, stub codex/claude/unity/Unity.exe, a tool home with settings that
# point at them, its own CODEX_HOME), then:
#   1. uis run: capture (batch, through WMI like Unity) -> prompt -> rewrite -> picture -> finish
#      -> checks -> export, for two prefabs with a note
#   2. failure: Codex says the usage limit was reached (the second item is not sent)
#   3. failure: the rewrite answer is not JSON (no picture is asked for)
#   4. failure: a prefab that does not exist (capture fails, the other item goes on)
#   5. rewrite on both paths (Claude, then Codex) with the same note and model answer
#   6. review on both paths with the same capture and candidate
#   7. Claude missing: the rewrite falls back to Codex
#   8. keep the original (strict): a picture with an added sparkle fails the fidelity review and
#      one more try with the correction fixes it, on both review paths (Claude, then Codex)
#   9. composition: `uis camera` changes a prefab's camera; the capture job and the CAMERA
#      sentence follow it; reset; a note's camera wish on both rewrite paths
#  10. a batch capture rewrites a text file with CRLF only (put back) and really changes another
#      (left, warned about)
# Steps 5, 6, 8 and 9 print a table of the two paths; a path that did not run is a failure.
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

# Pictures are the capture keyed out of its white (a faithful "repaint" for the strict default),
# unless the scenario names another picture
function Set-Scenario($root, [string]$text) {
    if (-not $text.Contains('codex_image=')) { $text = "codex_image=copy`n" + $text }
    Write-Utf8 (Join-Path $root 'bin\stub-scenario.txt') $text
    $log = Join-Path $root 'bin\calls.log'
    if (Test-Path $log) { Remove-Item -Force $log }
}

function Get-Calls($b, [string]$role) {
    $log = Join-Path $b.Bin 'calls.log'
    if (-not (Test-Path $log)) { return 0 }
    return @(Get-Content $log | Where-Object { $_ -like ($role + "`t*") }).Count
}

# Runs `uis <args>` with the bed's home and CODEX_HOME; stdout parsed as JSON.
function Invoke-Uis($b, [string[]]$args2, [string]$tag = 'uis') {
    $env:UNITY_ICON_STUDIO_HOME = $b.Home
    $env:CODEX_HOME = $b.CodexHome
    $outFile = Join-Path $b.Root ($tag + '-out.json')
    $errFile = Join-Path $b.Root ($tag + '-err.txt')
    try {
        # Quoted one by one (paths and notes have spaces; none has a quote)
        $line = ($args2 | ForEach-Object { '"' + $_ + '"' }) -join ' '
        $p = Start-Process -FilePath $uis -ArgumentList $line -NoNewWindow -Wait -PassThru -RedirectStandardOutput $outFile -RedirectStandardError $errFile
        $code = $p.ExitCode
    } finally {
        Remove-Item Env:UNITY_ICON_STUDIO_HOME -ErrorAction SilentlyContinue
        Remove-Item Env:CODEX_HOME -ErrorAction SilentlyContinue
    }
    $text = [IO.File]::ReadAllText($outFile)
    try { $json = $text | ConvertFrom-Json } catch { $json = $null }
    return @{ Code = $code; Json = $json; Err = (Get-Content $errFile -ErrorAction SilentlyContinue) }
}

function Invoke-Run($b, [string[]]$more) {
    $r = Invoke-Uis $b (@('run', '--project', $b.Game, '--mode', 'batch', '--json') + $more) 'run'
    return @{ Code = $r.Code; Report = $r.Json; Steps = $r.Err }
}

# Changes one key of the bed's settings.json (dotted path), or removes it with $null.
function Set-BedSetting($b, [string]$key, $value) {
    $file = Join-Path $b.Home 'settings.json'
    $doc = Get-Content -Raw $file | ConvertFrom-Json
    $parts = $key.Split('.')
    $node = $doc
    foreach ($part in $parts[0..($parts.Count - 2)]) {
        if (-not ($node.PSObject.Properties.Name -contains $part)) { $node | Add-Member -NotePropertyName $part -NotePropertyValue ([pscustomobject]@{}) }
        $node = $node.$part
    }
    $last = $parts[-1]
    if ($node.PSObject.Properties.Name -contains $last) { $node.PSObject.Properties.Remove($last) }
    if ($null -ne $value) { $node | Add-Member -NotePropertyName $last -NotePropertyValue $value }
    Write-Utf8 $file ($doc | ConvertTo-Json -Depth 8)
}

# One row of the two-path table for a rewrite report
function Get-RewriteRow([string]$path, $r) {
    $s = @($r.diff.sentences)
    $locked = @($s | Where-Object { $_.locked })
    return [pscustomobject]@{
        path     = $path
        cli      = $r.cli
        fallback = $r.fallbackFrom
        calls    = @($r.calls).Count
        replaced = @($s | Where-Object { $_.status -eq 'replaced' }).Count
        added    = @($s | Where-Object { $_.status -eq 'added' }).Count
        rejected = @($r.diff.rejected).Count
        lockedOk = (@($locked | Where-Object { $_.status -ne 'same' }).Count -eq 0 -and $locked.Count -gt 0)
        ms       = [int](@($r.calls) | Measure-Object -Property elapsedMs -Sum).Sum
        pass     = [bool]$r.ok
    }
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
$pictures = 0
$log = Join-Path $b.Bin 'calls.log'
if (Test-Path $log) { $pictures = @(Get-Content $log | Where-Object { $_ -like "codex`timage`t*" }).Count }
Check ($pictures -eq 2) 'two picture calls (codex)'
Check ((Get-Calls $b 'codex') -eq 4) 'and two fidelity reviews (codex, the default reviewer)'
Check (@($rep.items | Where-Object { @($_.candidates).Count -eq 1 }).Count -eq 2) 'faithful pictures: no retry'
foreach ($it in $rep.items) {
    $post = $it.candidates[0].post
    Check ($null -ne $post -and @($post.sizes).Count -eq 2) ('finished 256 and 512: ' + $it.name)
    Check ($null -ne $post.sizes[0].checks.items -and @($post.sizes[0].checks.items).Count -ge 10) ('checks ran: ' + $it.name)
    Check ($it.export.ok -and @($it.export.files).Count -eq 3) ('exported 256, 512 and the 1024 original: ' + $it.name)
    $job = Get-Content -Raw (Join-Path $it.jobDir 'job.json') | ConvertFrom-Json
    Check ($job.asset -eq $it.asset -and $job.runId -eq $rep.runId) ('job.json records the run: ' + $it.name)
}
Check (Test-Path (Join-Path $b.Out 'icon_Crate.png')) 'out\icon_Crate.png'
Check (Test-Path (Join-Path $b.Out 'icon_House_512.png')) 'out\icon_House_512.png'
Check (Test-Path (Join-Path $b.Out 'icon_House_1024.png')) 'out\icon_House_1024.png (the original)'
Check (-not (Test-Path (Join-Path $b.Game 'Assets\__UnityIconStudioCapture'))) 'no capture entry left in the project'
$dirty = & git -C $b.Game -c core.autocrlf=false status --porcelain
Check ([string]::IsNullOrWhiteSpace(($dirty -join ''))) 'project git status clean after the run'
$steps = $r.Steps -join "`n"
foreach ($s in 'capture start', 'prompt ok', 'rewrite ok', 'generate ok', 'post ok', 'export ok', 'done ok') {
    Check ($steps -match [regex]::Escape($s)) ('step reported: ' + $s)
}

$reviewJob = $rep.items[0].jobDir

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


# The steps where a CLI can be picked (rewrite, review) run on both paths, Claude and Codex, with
# the same input. Pictures are Codex only. A path that did not run counts as a failure.
Say ''
Say '5. rewrite on both paths: the same note with Claude, then with Codex'
Set-Scenario $b.Root "delay_ms=400`n"
# The model's answer: one replaced and one added sentence, one addition the code refuses (a logo),
# one change to a locked sentence the code refuses, one part the model left out itself
$reply = '{"replace":[{"id":"STYLE.color","text":"Color: cool winter palette with a sky-blue roof and snow-white accents; no haze, no fog, no bloom."},{"id":"CAMERA.view","text":"Show it from the front."}],"add":[{"block":"SUBJECT","text":"A thin layer of snow rests on the roof."},{"block":"SUBJECT","text":"Put a small logo on the roof."}],"avoidAdd":[],"outline":"keep","camera":"keep","rejected":[{"part":"make it photoreal","reason":"conflicts with the casual style"}],"summaryKo":"winter"}'
Write-Utf8 (Join-Path $b.Bin 'reply.json') $reply
$note = 'snow on the roof, winter mood, add a logo, make it photoreal'
$rows = @()
foreach ($cli in 'claude', 'codex') {
    $before = Get-Calls $b $cli
    $r = Invoke-Uis $b @('rewrite', '--kind', 'building', '--note', $note, '--cli', $cli, '--json') ('rewrite-' + $cli)
    $row = Get-RewriteRow $cli $r.Json
    $row | Add-Member -NotePropertyName cliCalls -NotePropertyValue ((Get-Calls $b $cli) - $before)
    $rows += $row
    Check ($r.Code -eq 0 -and $row.pass) ('rewrite ok on the ' + $cli + ' path')
    Check ($row.cli -eq $cli -and -not $row.fallback) ('answered by ' + $cli + ' itself (no fallback)')
    Check ($row.cliCalls -eq 1) ('one ' + $cli + ' call')
    Check ($row.replaced -eq 1 -and $row.added -eq 1) ($cli + ': 1 replaced, 1 added')
    Check ($row.rejected -eq 3) ($cli + ': 3 left out (model 1, logo, locked camera); got ' + $row.rejected)
    Check $row.lockedOk ($cli + ': every locked sentence kept')
}
Remove-Item (Join-Path $b.Bin 'reply.json')
Check ($rows.Count -eq 2) 'both paths ran'

Say ''
Say '6. review on both paths: the same capture and candidate'
$cap = Join-Path $reviewJob 'capture\capture.png'
$cand = Join-Path $reviewJob 'post\cand-01\master-1024.png'
$reviewRows = @()
foreach ($cli in 'claude', 'codex') {
    $before = Get-Calls $b $cli
    $r = Invoke-Uis $b @('review', '--capture', $cap, '--candidate', $cand, '--kind', 'prop', '--cli', $cli, '--json') ('review-' + $cli)
    $j = $r.Json
    $verdict = '-'
    if ($null -ne $j -and $null -ne $j.review) { if ($j.review.pass) { $verdict = 'pass' } else { $verdict = 'fail' } }
    $row = [pscustomobject]@{
        path    = $cli
        cli     = $j.call.cli
        calls   = (Get-Calls $b $cli) - $before
        verdict = $verdict
        issues  = @($j.review.issues).Count
        ms      = [int]$j.call.elapsedMs
        pass    = [bool]$j.ok
    }
    $reviewRows += $row
    $why = ''
    if (-not $row.pass -and $null -ne $j) { $why = ': ' + ($j.failure | ConvertTo-Json -Compress) }
    Check ($row.pass) ('review answered on the ' + $cli + ' path' + $why)
    Check ($row.calls -eq 1) ('one ' + $cli + ' call')
}
Check ($reviewRows.Count -eq 2) 'both paths ran'
# An answer that is not a review (an edit list) is a failure on both paths, not a pass
Write-Utf8 (Join-Path $b.Bin 'reply.json') '{"replace":[],"add":[],"avoidAdd":[],"outline":"keep","camera":"keep","rejected":[],"summaryKo":""}'
foreach ($cli in 'claude', 'codex') {
    $r = Invoke-Uis $b @('review', '--capture', $cap, '--candidate', $cand, '--kind', 'prop', '--cli', $cli, '--json') ('review-bad-' + $cli)
    Check ((-not $r.Json.ok) -and $r.Json.failure.kind -eq 'bad_json') ($cli + ': a non-review answer is bad_json')
}
Remove-Item (Join-Path $b.Bin 'reply.json')

Say ''
Say '7. Claude missing: the rewrite falls back to Codex'
Set-BedSetting $b 'claude.path' (Join-Path $b.Bin 'no-such-claude.exe')
$beforeClaude = Get-Calls $b 'claude'
$beforeCodex = Get-Calls $b 'codex'
$r = Invoke-Uis $b @('rewrite', '--kind', 'building', '--note', 'snow on the roof', '--cli', 'claude', '--json') 'rewrite-fallback'
$fb = Get-RewriteRow 'claude missing' $r.Json
Check ($fb.pass -and $fb.cli -eq 'codex' -and $fb.fallback -eq 'claude') 'answered by codex, fallback from claude'
Check ((Get-Calls $b 'claude') -eq $beforeClaude -and (Get-Calls $b 'codex') -eq ($beforeCodex + 1)) 'no claude call, one codex call'
Set-BedSetting $b 'claude.path' (Join-Path $b.Bin 'claude.exe')

Say ''
Say '8. keep the original: fidelity review on both paths, then one retry with the correction'
$fidRows = @()
foreach ($cli in 'claude', 'codex') {
    Set-Scenario $b.Root "codex_image=copy_sparkle_once`nfidelity=added_once`n"
    foreach ($f in 'codex-images.count', 'claude-fidelity.count', 'codex-fidelity.count') {
        $p = Join-Path $b.Bin $f
        if (Test-Path $p) { Remove-Item -Force $p }
    }
    $r = Invoke-Uis $b @('run', '--project', $b.Game, '--prefab', 'Assets/Buildings/House.prefab', '--capture', $cap, '--kind', 'building', '--fidelity', 'strict', '--review', $cli, '--retries', '1', '--json') ('fidelity-' + $cli)
    $it = $null
    if ($null -ne $r.Json) { $it = $r.Json.items[0] }
    $row = [pscustomobject]@{ path = $cli; cands = 0; firstReview = '-'; added = '-'; retryReview = '-'; best = '-'; correction = $false; reviewCalls = (Get-Calls $b $cli); pass = $false }
    if ($null -ne $it) {
        $c = @($it.candidates)
        $row.cands = $c.Count
        if ($c.Count -ge 1 -and $null -ne $c[0].post) {
            $rv = @($c[0].post.reviews)[0]
            if ($rv.pass) { $row.firstReview = 'pass' } else { $row.firstReview = 'fail' }
            $row.added = @($rv.failing | Where-Object { $_.kind -eq 'added' }).Count
        }
        if ($c.Count -ge 2 -and $null -ne $c[1].post) {
            $rv2 = @($c[1].post.reviews)[0]
            if ($rv2.pass) { $row.retryReview = 'pass' } else { $row.retryReview = 'fail' }
            $row.correction = ([string]$c[1].correction).StartsWith('Corrections from the previous try: do not add')
        }
        $row.best = $it.best
        $row.pass = ($row.cands -eq 2 -and $row.firstReview -eq 'fail' -and $row.retryReview -eq 'pass' -and $row.best -eq 2 -and $row.correction -and @($c[0].post.reviews)[0].cli -eq $cli)
    }
    $fidRows += $row
    Check ($row.pass) ('strict on the ' + $cli + ' path: sparkle found, retry with the correction passes')
}
Set-Scenario $b.Root ''
Check ($fidRows.Count -eq 2) 'both review paths ran'

Say ''
Say '9. composition: a person changes the camera; the capture and the CAMERA sentence follow'
Set-Scenario $b.Root "delay_ms=400`n"
$crateAsset = 'Assets/Props/Crate.prefab'
$r = Invoke-Uis $b @('camera', '--project', $b.Game, '--prefab', $crateAsset, '--json') 'camera-start'
$start = $r.Json.effective
Check ($r.Code -eq 0 -and $null -ne $start.yaw) ('start values: yaw ' + $start.yaw + ', pitch ' + $start.pitch + ' (kind preset)')
$r = Invoke-Uis $b @('camera', '--project', $b.Game, '--prefab', $crateAsset, '--set', 'yawSign=-1', '--set', 'pitch=45', '--set', 'yaw=40', '--set', 'rotation=270,0,0', '--json') 'camera-set'
$e = $r.Json.effective
Check ($e.yaw -eq -40 -and $e.pitch -eq 45 -and ((@($e.rotation) | ForEach-Object { [int]$_ }) -join ',') -eq '270,0,0') 'stored: yaw -40 (left side), pitch 45, rotation 270,0,0'
$r = Invoke-Uis $b @('camera', '--project', $b.Game, '--prefab', $crateAsset, '--set', 'pitch=200', '--json') 'camera-bad'
Check ($r.Code -ne 0) 'a pitch out of range is refused'
$r = Invoke-Run $b @('--prefab', $crateAsset)
$it = $r.Report.items[0]
Check ($it.state -eq 'ok') 'run ok with the changed composition'
$job = Get-Content -Raw (Join-Path (Split-Path -Parent $it.capture) 'job-in.json') | ConvertFrom-Json
$capItem = @($job.items)[0]
Check ($capItem.yaw -eq -40 -and $capItem.pitch -eq 45) 'the capture job had the person''s camera'
$fixed = [IO.File]::ReadAllText((Join-Path $it.jobDir 'prompt\fixed.txt'))
Check ($fixed.Contains('looking down at about 45 degrees') -and $fixed.Contains('turned about 40 degrees') -and $fixed.Contains('the left side face')) 'the CAMERA sentence says the same (45, 40, left)'
$r = Invoke-Uis $b @('camera', '--project', $b.Game, '--prefab', $crateAsset, '--reset', '--json') 'camera-reset'
Check ($r.Json.effective.yaw -eq $start.yaw -and $r.Json.effective.pitch -eq $start.pitch -and $null -eq $r.Json.person) 'back to the start: the preset again, nothing stored'
# The composition is back to the preset, the 45-degree capture is passed as it is: the sentence
# says what that picture shows (45, left), not the composition now
$r = Invoke-Run $b @('--prefab', $crateAsset, '--capture', $it.capture, '--kind', 'prop')
$fixed2 = [IO.File]::ReadAllText((Join-Path $r.Report.items[0].jobDir 'prompt\fixed.txt'))
Check ($null -eq $r.Report.capture -and $fixed2.Contains('looking down at about 45 degrees') -and $fixed2.Contains('the left side face')) 'an earlier capture keeps its own camera sentence'

# A note that asks for another camera: both rewrite paths report it, the locked CAMERA sentence stays
$camRows = @()
Write-Utf8 (Join-Path $b.Bin 'reply.json') '{"replace":[],"add":[],"avoidAdd":[],"additions":[],"outline":"keep","camera":"front","rejected":[{"part":"show it from the front","reason":"the camera comes from the capture"}],"summaryKo":"front"}'
foreach ($cli in 'claude', 'codex') {
    $r = Invoke-Uis $b @('rewrite', '--kind', 'prop', '--note', 'show it from the front', '--cli', $cli, '--json') ('camera-rewrite-' + $cli)
    $row = Get-RewriteRow $cli $r.Json
    $row | Add-Member -NotePropertyName camera -NotePropertyValue $r.Json.diff.camera
    $camRows += $row
    Check ($row.pass -and $row.cli -eq $cli -and $row.camera -eq 'front' -and $row.lockedOk) ($cli + ': camera wish reported (front), CAMERA sentence kept')
}
Remove-Item (Join-Path $b.Bin 'reply.json')
Check ($camRows.Count -eq 2) 'both rewrite paths ran'


Say ''
Say '10. a batch capture that rewrites line endings, and one that really changes a file'
Set-Scenario $b.Root "delay_ms=400`ntouch_crlf=README.md`ntouch_edit=Assets/Props/CrateRed.prefab`n"
$beforeStatus = (& git -C $b.Game -c core.autocrlf=false status --short) -join "`n"
$r = Invoke-Uis $b @('capture', '--project', $b.Game, '--prefab', 'Assets/Props/Crate.prefab', '--mode', 'batch', '--json') 'capture-eol'
$g = $r.Json.git
Check ((@($g.lineEndings) -join ',') -eq 'README.md') 'put back: README.md (only its line endings changed)'
Check ((@($g.left) -join ',') -eq ' M Assets/Props/CrateRed.prefab') 'left and reported: the real change'
Check ((@($r.Err) -join "`n") -match 'WARNING: the capture left 1 changed file') 'the command line warns about it (stderr, with --json too)'
$readme = [IO.File]::ReadAllBytes((Join-Path $b.Game 'README.md'))
Check (-not ([Text.Encoding]::ASCII.GetString($readme).Contains("`r`n"))) 'README.md is LF again'
& git -C $b.Game checkout -q -- Assets/Props/CrateRed.prefab
$afterStatus = (& git -C $b.Game -c core.autocrlf=false status --short) -join "`n"
Check ($afterStatus -eq $beforeStatus) 'after putting the test edit back: git status as before'
Set-Scenario $b.Root "delay_ms=400`n"
Say ''
Say 'two paths, side by side'
Say '  rewrite (same note, same model answer)'
($rows + @($fb)) | Format-Table path, cli, fallback, calls, replaced, added, rejected, lockedOk, ms, pass -AutoSize | Out-String -Width 200 | ForEach-Object { Write-Host $_.TrimEnd() }
Say '  camera wish in a note (same note, same model answer)'
$camRows | Format-Table path, cli, calls, camera, rejected, lockedOk, ms, pass -AutoSize | Out-String -Width 200 | ForEach-Object { Write-Host $_.TrimEnd() }
Say '  review (same capture and candidate)'
$reviewRows | Format-Table path, cli, calls, verdict, issues, ms, pass -AutoSize | Out-String -Width 200 | ForEach-Object { Write-Host $_.TrimEnd() }
Say '  fidelity review and retry (same capture, a sparkle on the first picture)'
$fidRows | Format-Table path, cands, firstReview, added, retryReview, best, correction, reviewCalls, pass -AutoSize | Out-String -Width 200 | ForEach-Object { Write-Host $_.TrimEnd() }
$table = @{ rewrite = @($rows + @($fb)); review = $reviewRows; fidelity = $fidRows; camera = $camRows } | ConvertTo-Json -Depth 5
Write-Utf8 (Join-Path $b.Root 'two-paths.json') $table
Say ''
if ($script:failures -eq 0) { Say 'e2e-stub: all checks passed' } else { Say ('e2e-stub: ' + $script:failures + ' check(s) failed') }
if ($Keep -or $script:failures) { Say ('bed kept: ' + $b.Root) } else { Remove-Item -Recurse -Force $b.Root -ErrorAction SilentlyContinue }
exit $script:failures
