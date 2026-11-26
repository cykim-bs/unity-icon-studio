# Prints the installed Codex desktop app as one JSON line:
#   {"version":"...","resources":"<install folder>\app\resources","files":[{"name":"codex.exe","size":123},...]}
# or {} when the app is not installed. Reads only; copies nothing.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
try {
    $pkg = Get-AppxPackage -Name 'OpenAI.Codex' -ErrorAction SilentlyContinue | Sort-Object Version -Descending | Select-Object -First 1
} catch {
    $pkg = $null
}
if (-not $pkg) {
    Write-Output '{}'
    exit 0
}
$res = Join-Path $pkg.InstallLocation 'app\resources'
$files = @()
if (Test-Path -LiteralPath $res) {
    $files = @(Get-ChildItem -LiteralPath $res -Filter 'codex*.exe' -File | ForEach-Object {
        [ordered]@{ name = $_.Name; size = [int64]$_.Length }
    })
}
$out = [ordered]@{ version = [string]$pkg.Version; resources = $res; files = $files }
Write-Output ($out | ConvertTo-Json -Compress -Depth 4)
