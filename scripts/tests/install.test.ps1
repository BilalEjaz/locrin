$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
$tmp = Join-Path $env:TEMP ("locrin-install-test-" + [guid]::NewGuid())
$rel = Join-Path $tmp "releases\v9.9.9"
New-Item -ItemType Directory -Force -Path $rel | Out-Null
$dir = Join-Path $tmp "locrin-v9.9.9-x86_64-pc-windows-msvc"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
Set-Content -Path (Join-Path $dir "locrin.exe") -Value "not really an exe" -Encoding ascii
Compress-Archive -Path $dir -DestinationPath (Join-Path $rel "locrin-v9.9.9-x86_64-pc-windows-msvc.zip")
$hash = (Get-FileHash -Algorithm SHA256 (Join-Path $rel "locrin-v9.9.9-x86_64-pc-windows-msvc.zip")).Hash.ToLower()
Set-Content -Path (Join-Path $rel "SHA256SUMS") -Value "$hash  locrin-v9.9.9-x86_64-pc-windows-msvc.zip" -Encoding ascii

$out = Join-Path $tmp "out"
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1 -Version v9.9.9 -BaseUrl (Join-Path $tmp "releases") -InstallDir $out
if ($LASTEXITCODE -ne 0) { throw "install failed with $LASTEXITCODE" }
if (-not (Test-Path (Join-Path $out "locrin.exe"))) { throw "binary not installed" }

Set-Content -Path (Join-Path $rel "SHA256SUMS") -Value ("0" + $hash.Substring(1) + "  locrin-v9.9.9-x86_64-pc-windows-msvc.zip") -Encoding ascii
$out2 = Join-Path $tmp "out2"
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1 -Version v9.9.9 -BaseUrl (Join-Path $tmp "releases") -InstallDir $out2
if ($LASTEXITCODE -ne 2) { throw "mismatch must exit 2, got $LASTEXITCODE" }
if (Test-Path (Join-Path $out2 "locrin.exe")) { throw "must not install on mismatch" }
Remove-Item -Recurse -Force $tmp
Write-Output "ok   scripts/tests/install.test.ps1"
