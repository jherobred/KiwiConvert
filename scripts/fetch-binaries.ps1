# Downloads the native engines KiwiConvert bundles (FFmpeg and PDFium) into ./vendor.
# Every archive is verified against a pinned SHA-256 before extraction.
# Usage: npm run fetch:binaries   (or: powershell -File scripts/fetch-binaries.ps1 -Force)

param([switch]$Force)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$root = Split-Path -Parent $PSScriptRoot
$vendor = Join-Path $root "vendor"
$cache = Join-Path $vendor ".cache"
New-Item -ItemType Directory -Force -Path $cache | Out-Null

$packages = @(
  @{
    Name   = "ffmpeg"
    Url    = "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-full_build-shared.zip"
    File   = "ffmpeg-9.0.2-full_build-shared.zip"
    Sha256 = "8d31e162f1616e37aab3fa2db991b97e1b8dbeb1c8465fd81a81be16ff91b328"
  },
  @{
    Name   = "pdfium"
    Url    = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8066/pdfium-win-x64.tgz"
    File   = "pdfium-win-x64-8066.tgz"
    Sha256 = "739a57d597d864297909cc40a2411eba728490c76a0fa25e3ea299c7f6b07020"
  }
)

function Get-Verified($pkg) {
  $archive = Join-Path $cache $pkg.File
  if (-not (Test-Path $archive)) {
    Write-Host "Downloading $($pkg.Name) from $($pkg.Url)"
    Invoke-WebRequest -UseBasicParsing -Uri $pkg.Url -OutFile $archive
  }
  $hash = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash.ToLowerInvariant()
  if ($hash -ne $pkg.Sha256) {
    Remove-Item -Force $archive
    throw "SHA-256 mismatch for $($pkg.File). Expected $($pkg.Sha256), got $hash. The file was deleted."
  }
  Write-Host "Verified $($pkg.File) ($hash)"
  return $archive
}

function Expand-To($archive, $dest) {
  if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
  New-Item -ItemType Directory -Force -Path $dest | Out-Null
  tar -xf $archive -C $dest
  if ($LASTEXITCODE -ne 0) { throw "Failed to extract $archive" }
}

# FFmpeg: keep ffmpeg.exe, ffprobe.exe and every DLL they load. ffplay is not needed.
$ffmpegOut = Join-Path $vendor "ffmpeg"
if ($Force -or -not (Test-Path (Join-Path $ffmpegOut "ffmpeg.exe"))) {
  $archive = Get-Verified $packages[0]
  $tmp = Join-Path $cache "ffmpeg-extract"
  Expand-To $archive $tmp
  $bin = Get-ChildItem -Path $tmp -Recurse -Directory -Filter bin | Select-Object -First 1
  if (Test-Path $ffmpegOut) { Remove-Item -Recurse -Force $ffmpegOut }
  New-Item -ItemType Directory -Force -Path $ffmpegOut | Out-Null
  Copy-Item (Join-Path $bin.FullName "ffmpeg.exe") $ffmpegOut
  Copy-Item (Join-Path $bin.FullName "ffprobe.exe") $ffmpegOut
  Copy-Item (Join-Path $bin.FullName "*.dll") $ffmpegOut
  $license = Get-ChildItem -Path $tmp -Recurse -File -Filter LICENSE* | Select-Object -First 1
  if ($license) { Copy-Item $license.FullName (Join-Path $ffmpegOut "LICENSE.txt") }
  Remove-Item -Recurse -Force $tmp
}

# PDFium: a single DLL, plus one notice file holding the licenses of PDFium and every
# library built into it.
$pdfiumOut = Join-Path $vendor "pdfium"
$pdfiumNotice = Join-Path $pdfiumOut "LICENSE.txt"
$noticeStale = -not (Test-Path $pdfiumNotice) -or -not (Select-String -Quiet -SimpleMatch "==== pdfium.txt" $pdfiumNotice)
if ($Force -or $noticeStale -or -not (Test-Path (Join-Path $pdfiumOut "pdfium.dll"))) {
  $archive = Get-Verified $packages[1]
  $tmp = Join-Path $cache "pdfium-extract"
  Expand-To $archive $tmp
  if (Test-Path $pdfiumOut) { Remove-Item -Recurse -Force $pdfiumOut }
  New-Item -ItemType Directory -Force -Path $pdfiumOut | Out-Null
  Copy-Item (Join-Path $tmp "bin\pdfium.dll") $pdfiumOut
  $notice = Get-Content -Raw (Join-Path $tmp "LICENSE")
  foreach ($file in Get-ChildItem (Join-Path $tmp "licenses") -File | Sort-Object Name) {
    $notice += "`n`n==== $($file.Name) ====`n`n" + (Get-Content -Raw $file.FullName)
  }
  [IO.File]::WriteAllText($pdfiumNotice, $notice)
  Remove-Item -Recurse -Force $tmp
}

$size = (Get-ChildItem -Recurse -File $ffmpegOut, $pdfiumOut | Measure-Object -Sum Length).Sum
Write-Host ("Binaries ready in vendor/ ({0:N1} MB)" -f ($size / 1MB))
