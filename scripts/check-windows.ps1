$ErrorActionPreference = 'SilentlyContinue'

function Check-Command($Name, $Hint) {
  $cmd = Get-Command $Name -ErrorAction SilentlyContinue
  if ($cmd) {
    Write-Host "[OK] $Name -> $($cmd.Source)" -ForegroundColor Green
    return $true
  }
  Write-Host "[MISSING] $Name" -ForegroundColor Yellow
  Write-Host "          $Hint" -ForegroundColor DarkGray
  return $false
}

Write-Host "HED Space Windows development check" -ForegroundColor Cyan
Write-Host ""

$node = Check-Command 'node' 'Install a current Node.js LTS release.'
$npm = Check-Command 'npm' 'npm is normally installed with Node.js.'
$rustc = Check-Command 'rustc' 'Install Rust using rustup with the stable MSVC toolchain.'
$cargo = Check-Command 'cargo' 'Cargo is installed with Rust.'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$bin = Join-Path $root 'src-tauri\bin'

$mpvBundled = Test-Path (Join-Path $bin 'mpv.exe')
$ffprobeBundled = Test-Path (Join-Path $bin 'ffprobe.exe')
$mpvPath = Get-Command 'mpv.exe' -ErrorAction SilentlyContinue
$ffprobePath = Get-Command 'ffprobe.exe' -ErrorAction SilentlyContinue

if ($mpvBundled) { Write-Host "[OK] mpv.exe bundled in src-tauri\bin" -ForegroundColor Green }
elseif ($mpvPath) { Write-Host "[OK] mpv.exe on PATH -> $($mpvPath.Source)" -ForegroundColor Green }
else { Write-Host "[MISSING] mpv.exe - required for Watch/fullscreen Atmospheres" -ForegroundColor Yellow }

if ($ffprobeBundled) { Write-Host "[OK] ffprobe.exe bundled in src-tauri\bin" -ForegroundColor Green }
elseif ($ffprobePath) { Write-Host "[OK] ffprobe.exe on PATH -> $($ffprobePath.Source)" -ForegroundColor Green }
else { Write-Host "[OPTIONAL] ffprobe.exe missing - browsing works, but scan metadata/durations will be limited" -ForegroundColor Yellow }

Write-Host ""
if ($node -and $npm -and $rustc -and $cargo -and ($mpvBundled -or $mpvPath)) {
  Write-Host "Core development prerequisites look ready." -ForegroundColor Green
} else {
  Write-Host "Resolve the missing items above before running npm run dev." -ForegroundColor Yellow
}
