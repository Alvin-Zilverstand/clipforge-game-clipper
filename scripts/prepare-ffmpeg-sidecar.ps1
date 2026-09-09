param(
  [string]$Destination = "$PSScriptRoot\..\src-tauri\binaries\ffmpeg-x86_64-pc-windows-msvc.exe",
  [string]$DownloadUrl = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
)

$ErrorActionPreference = "Stop"
$destinationPath = [System.IO.Path]::GetFullPath($Destination)
$destinationDir = Split-Path -Parent $destinationPath
New-Item -ItemType Directory -Force $destinationDir | Out-Null

if (Test-Path $destinationPath) {
  Write-Host "FFmpeg sidecar already exists: $destinationPath"
  exit 0
}

$systemFfmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
if ($systemFfmpeg) {
  Copy-Item -LiteralPath $systemFfmpeg.Source -Destination $destinationPath -Force
  Write-Host "Copied system FFmpeg to sidecar: $destinationPath"
  exit 0
}

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("clipforge-ffmpeg-" + [System.Guid]::NewGuid().ToString("N"))
$zipPath = Join-Path $tempRoot "ffmpeg.zip"
New-Item -ItemType Directory -Force $tempRoot | Out-Null

try {
  Write-Host "Downloading FFmpeg essentials build..."
  Invoke-WebRequest -UseBasicParsing -Uri $DownloadUrl -OutFile $zipPath
  Expand-Archive -LiteralPath $zipPath -DestinationPath $tempRoot -Force
  $ffmpeg = Get-ChildItem -Path $tempRoot -Recurse -Filter ffmpeg.exe | Select-Object -First 1
  if (-not $ffmpeg) {
    throw "Downloaded archive did not contain ffmpeg.exe"
  }
  Copy-Item -LiteralPath $ffmpeg.FullName -Destination $destinationPath -Force
  Write-Host "Prepared FFmpeg sidecar: $destinationPath"
} finally {
  Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
