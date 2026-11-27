# Builds (when the source is newer) and installs Icon Studio for this user, then starts it.
# Install folder: %LOCALAPPDATA%\UnityIconStudio. Use -ForceRebuild to always rebuild.
# Run it from Explorer (start.bat) or your own terminal: a packaged agent shell
# would install into its private copy of AppData.
# A running copy is asked to close (same as its close box); it is never ended by force.
# ASCII only: Windows PowerShell 5.1 reads BOM-less scripts in the system code page.
# After unity-slot-widget scripts/launch-user.ps1 (MIT).

param(
  [switch]$ForceRebuild
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$InstallDir = Join-Path $env:LOCALAPPDATA "UnityIconStudio"
$InstallExe = Join-Path $InstallDir "unity-icon-studio.exe"
$ReleaseExe = Join-Path $Root "target\release\unity-icon-studio.exe"
$AppTitle = "Icon Studio"

function Show-Error([string]$Message) {
  Add-Type -AssemblyName PresentationFramework | Out-Null
  [System.Windows.MessageBox]::Show($Message, $AppTitle, "OK", "Error") | Out-Null
}

function Find-VcVars {
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  if (-not (Test-Path $vswhere)) { return $null }
  $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
  if (-not $vsPath) { return $null }
  $bat = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
  if (Test-Path $bat) { return $bat }
  return $null
}

function Get-SourceStamp {
  $latest = [datetime]::MinValue
  foreach ($p in @("package.json", "index.html", "vite.config.ts", "Cargo.toml", "src-tauri\Cargo.toml", "src-tauri\tauri.conf.json")) {
    $full = Join-Path $Root $p
    if (Test-Path $full) {
      $t = (Get-Item $full).LastWriteTimeUtc
      if ($t -gt $latest) { $latest = $t }
    }
  }
  foreach ($dir in @("src", "src-tauri\src", "src-tauri\capabilities", "src-tauri\icons", "crates")) {
    $full = Join-Path $Root $dir
    if (-not (Test-Path $full)) { continue }
    Get-ChildItem $full -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object {
      if ($_.LastWriteTimeUtc -gt $latest) { $latest = $_.LastWriteTimeUtc }
    }
  }
  return $latest
}

function Test-NeedsRebuild {
  if ($ForceRebuild) { return $true }
  if (-not (Test-Path $ReleaseExe)) { return $true }
  return (Get-SourceStamp) -gt (Get-Item $ReleaseExe).LastWriteTimeUtc
}

function Build-Release {
  $vcvars = Find-VcVars
  if (-not $vcvars) {
    Show-Error "The build needs Visual Studio C++ Build Tools, Rust and Node.js. See README (Build from source)."
    exit 1
  }
  Write-Host "Building release..."
  if (-not (Test-Path (Join-Path $Root "node_modules"))) {
    Push-Location $Root
    npm ci
    $code = $LASTEXITCODE
    Pop-Location
    if ($code -ne 0) { Show-Error "npm ci failed."; exit 1 }
  }
  cmd /c "`"$vcvars`" >nul && cd /d `"$Root`" && npx tauri build --no-bundle"
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path $ReleaseExe)) {
    Show-Error "Build failed. Check Node.js, Rust and the C++ Build Tools."
    exit 1
  }
}

# Asks the installed copy to close (like its close box) so the new exe can replace it.
function Close-RunningApp {
  $running = @(Get-Process -Name "unity-icon-studio" -ErrorAction SilentlyContinue | Where-Object {
      $_.Path -and ([IO.Path]::GetFullPath($_.Path) -ieq [IO.Path]::GetFullPath($InstallExe))
    })
  foreach ($p in $running) {
    Write-Host "Asking the running app to close (PID $($p.Id))..."
    [void]$p.CloseMainWindow()
    if (-not $p.WaitForExit(10000)) {
      Show-Error "Icon Studio is still running (it may be asking a question). Close it, then start again."
      exit 1
    }
  }
}

if (Test-NeedsRebuild) { Build-Release }
if (-not (Test-Path $ReleaseExe)) { Show-Error "Release exe not found."; exit 1 }

Close-RunningApp
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$tmp = Join-Path $InstallDir "unity-icon-studio.new.exe"
Copy-Item -Force $ReleaseExe $tmp
if (Test-Path $InstallExe) { Remove-Item -Force $InstallExe }
Rename-Item -Force $tmp (Split-Path $InstallExe -Leaf)
Write-Host "Installed: $InstallExe"
Start-Process -FilePath $InstallExe
exit 0
