# Installs the latest KiwiConvert release for the current user:
#   irm https://github.com/jherobred/KiwiConvert/releases/latest/download/install.ps1 | iex
#
# Downloads the installer from the latest GitHub release, checks it against the release's
# SHA256SUMS.txt, and runs it.

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$release = Invoke-RestMethod "https://api.github.com/repos/jherobred/KiwiConvert/releases/latest" `
  -Headers @{ "User-Agent" = "KiwiConvert-install" }
$asset = $release.assets | Where-Object { $_.name -like "KiwiConvert-Setup-*.exe" } | Select-Object -First 1
$sums = $release.assets | Where-Object { $_.name -eq "SHA256SUMS.txt" } | Select-Object -First 1
if (-not $asset -or -not $sums) { throw "Release $($release.tag_name) has no installer. Try again later." }

$setup = Join-Path $env:TEMP $asset.name
Write-Host ("Downloading {0} ({1:N0} MB)" -f $asset.name, ($asset.size / 1MB))
Invoke-WebRequest -UseBasicParsing $asset.browser_download_url -OutFile $setup

$expected = (Invoke-RestMethod $sums.browser_download_url) -split "`n" |
  Where-Object { $_ -match [regex]::Escape($asset.name) } |
  ForEach-Object { ($_ -split "\s+")[0] } | Select-Object -First 1
$actual = (Get-FileHash -Algorithm SHA256 $setup).Hash
if (-not $expected -or $actual -ne $expected.ToUpperInvariant()) {
  Remove-Item $setup -Force
  throw "The download doesn't match its published checksum, so it was deleted. Try again."
}

Write-Host "Checksum verified. Starting the installer."
Start-Process -FilePath $setup -Wait
Remove-Item $setup -Force -ErrorAction SilentlyContinue
