# Installs the locrin binary on Windows from a GitHub release.
#   irm https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.ps1 | iex
#   & ([scriptblock]::Create((irm .../install.ps1))) -Version v0.5.0
param(
  [string]$Version = "latest",
  [string]$InstallDir = "$env:LOCALAPPDATA\locrin\bin",
  [string]$BaseUrl = "https://github.com/BilalEjaz/locrin/releases/download"
)
$ErrorActionPreference = "Stop"
$repo = "BilalEjaz/locrin"
$tmp = $null
# Exits 2 when run as a file (the -File contract the tests assert); throws when the script was
# piped into iex, so the one-liner reports the error without closing the user's terminal.
function Fail($msg) {
  [Console]::Error.WriteLine("locrin: $msg")
  if ($tmp -and (Test-Path $tmp)) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
  if ($PSCommandPath) { exit 2 }
  throw $msg
}

if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") { Fail "no prebuilt binary for $env:PROCESSOR_ARCHITECTURE; see https://github.com/$repo/releases" }
$target = "x86_64-pc-windows-msvc"

if ($Version -eq "latest") {
  # On PowerShell 5.1 -ErrorAction SilentlyContinue makes the blocked redirect return the response
  # object, so the Location header is readable. A DNS or HTTP error still throws, hence the catch.
  $location = $null
  try {
    $probe = Invoke-WebRequest -Uri "https://github.com/$repo/releases/latest" -MaximumRedirection 0 -UseBasicParsing -ErrorAction SilentlyContinue
    if ($probe) { $location = $probe.Headers["Location"] }
  } catch { $location = $null }
  if (-not $location) { Fail "could not resolve the latest release" }
  $Version = $location.Split("/")[-1]
}

$asset = "locrin-$Version-$target.zip"
$tmp = Join-Path $env:TEMP ("locrin-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
function Get-Asset($name, $dest) {
  $src = "$BaseUrl/$Version/$name"
  if ($src -like "http*") { Invoke-WebRequest -Uri $src -OutFile $dest -UseBasicParsing }
  else { Copy-Item -LiteralPath $src -Destination $dest }
}
Get-Asset $asset (Join-Path $tmp $asset)
Get-Asset "SHA256SUMS" (Join-Path $tmp "SHA256SUMS")

$line = Get-Content (Join-Path $tmp "SHA256SUMS") | Where-Object { $_ -match ("\s" + [regex]::Escape($asset) + "$") }
if (-not $line) { Fail "$asset is not listed in SHA256SUMS" }
$expected = (($line | Select-Object -First 1) -split "\s+")[0].ToLower()
$actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $asset)).Hash.ToLower()
if ($actual -ne $expected) { Fail "checksum mismatch for $asset" }

Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $tmp -Force
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Move-Item -Force -Path (Join-Path $tmp "locrin-$Version-$target\locrin.exe") -Destination (Join-Path $InstallDir "locrin.exe")
Remove-Item -Recurse -Force $tmp
Write-Output "locrin $Version installed to $InstallDir\locrin.exe"
if (($env:PATH -split ";") -notcontains $InstallDir) { Write-Output "add $InstallDir to your PATH" }
