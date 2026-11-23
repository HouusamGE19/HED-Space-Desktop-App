param(
  [Parameter(Mandatory=$true)][string]$MpvPath,
  [Parameter(Mandatory=$false)][string]$FfprobePath
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$dest = Join-Path $root 'src-tauri\bin'
New-Item -ItemType Directory -Force -Path $dest | Out-Null

if (!(Test-Path $MpvPath -PathType Leaf)) { throw "mpv file not found: $MpvPath" }
Copy-Item -Force $MpvPath (Join-Path $dest 'mpv.exe')
Write-Host "Copied mpv.exe" -ForegroundColor Green

if ($FfprobePath) {
  if (!(Test-Path $FfprobePath -PathType Leaf)) { throw "ffprobe file not found: $FfprobePath" }
  Copy-Item -Force $FfprobePath (Join-Path $dest 'ffprobe.exe')
  Write-Host "Copied ffprobe.exe" -ForegroundColor Green
}

Write-Host "Tools prepared in $dest" -ForegroundColor Cyan
