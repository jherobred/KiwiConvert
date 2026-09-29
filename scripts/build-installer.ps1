# Builds KiwiConvert and its installer into installer/target/dist:
#   KiwiConvert-Setup-<version>.exe   the installer
#   SHA256SUMS.txt                    its checksum
#
# Steps: build the app, build the uninstaller (the installer crate without a payload),
# stage everything to install, then build the installer with the staged files inside.
#
# Code signing: set KIWI_SIGN_COMMAND to a command that signs one file, with "%1" where the
# path goes, for example
#   trusted-signing-cli -e <endpoint> -a <account> -c <profile> -d KiwiConvert "%1"
# The app, the uninstaller and the installer are each signed. See docs/SIGNING.md.
#
# Usage: powershell -File scripts/build-installer.ps1 [-SkipApp]

param([switch]$SkipApp)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -Raw (Join-Path $root "src-tauri\tauri.conf.json") | ConvertFrom-Json).version

function Invoke-Native([string]$what, [scriptblock]$block) {
  Write-Host "==> $what"
  & $block
  if ($LASTEXITCODE -ne 0) { throw "$what failed (exit code $LASTEXITCODE)" }
}

function Invoke-Sign([string]$path) {
  if (-not $env:KIWI_SIGN_COMMAND) { return }
  $command = $env:KIWI_SIGN_COMMAND.Replace("%1", $path)
  Invoke-Native "Sign $(Split-Path -Leaf $path)" { cmd /c $command }
}

& (Join-Path $PSScriptRoot "fetch-binaries.ps1")

if (-not $SkipApp) {
  Push-Location $root
  try { Invoke-Native "Build the app" { npx tauri build --no-bundle } } finally { Pop-Location }
}

$installer = Join-Path $root "installer"
Push-Location $installer
try {
  Remove-Item Env:KIWI_STAGING -ErrorAction SilentlyContinue
  Invoke-Native "Build the uninstaller" { cargo build --release --locked }
} finally { Pop-Location }

$stage = Join-Path $installer "target\staging"
if (Test-Path $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $stage "ffmpeg"), (Join-Path $stage "pdfium") | Out-Null
Copy-Item (Join-Path $root "src-tauri\target\release\kiwiconvert.exe") (Join-Path $stage "KiwiConvert.exe")
Copy-Item (Join-Path $installer "target\release\kiwiconvert-setup.exe") (Join-Path $stage "Uninstall KiwiConvert.exe")
Copy-Item (Join-Path $root "vendor\ffmpeg\*") (Join-Path $stage "ffmpeg")
Copy-Item (Join-Path $root "vendor\pdfium\*") (Join-Path $stage "pdfium")
Copy-Item (Join-Path $root "LICENSE") (Join-Path $stage "LICENSE.txt")
Copy-Item (Join-Path $root "THIRD_PARTY_NOTICES.md") (Join-Path $stage "THIRD_PARTY_NOTICES.txt")
Invoke-Sign (Join-Path $stage "KiwiConvert.exe")
Invoke-Sign (Join-Path $stage "Uninstall KiwiConvert.exe")

Push-Location $installer
try {
  $env:KIWI_STAGING = $stage
  Invoke-Native "Build the installer" { cargo build --release --locked }
} finally {
  Remove-Item Env:KIWI_STAGING -ErrorAction SilentlyContinue
  Pop-Location
}

$dist = Join-Path $installer "target\dist"
New-Item -ItemType Directory -Force $dist | Out-Null
$setup = Join-Path $dist "KiwiConvert-Setup-$version.exe"
Copy-Item (Join-Path $installer "target\release\kiwiconvert-setup.exe") $setup -Force
Invoke-Sign $setup

$hash = (Get-FileHash -Algorithm SHA256 $setup).Hash.ToLowerInvariant()
[IO.File]::WriteAllText((Join-Path $dist "SHA256SUMS.txt"), "$hash  $(Split-Path -Leaf $setup)`n")
Write-Host ("Built {0} ({1:N1} MB)" -f $setup, ((Get-Item $setup).Length / 1MB))
