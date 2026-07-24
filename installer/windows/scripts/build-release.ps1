param(
  [Parameter(Mandatory = $true)]
  [string]$PayloadPath,

  [Parameter(Mandatory = $true)]
  [ValidatePattern('^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$')]
  [string]$Version,

  [string]$OutputDirectory = "release-assets"
)

$ErrorActionPreference = "Stop"

$installerRoot = Split-Path -Parent $PSScriptRoot
$resolvedPayload = (Resolve-Path -LiteralPath $PayloadPath).Path
if ([IO.Path]::GetExtension($resolvedPayload) -ne ".exe") {
  throw "The embedded NSIS payload must be an .exe file."
}

$resolvedOutput = [IO.Path]::GetFullPath(
  (Join-Path $installerRoot $OutputDirectory)
)
New-Item -ItemType Directory -Path $resolvedOutput -Force | Out-Null

$previousPayload = $env:MINE_MAIL_NSIS_PAYLOAD
$previousVersion = $env:MINE_MAIL_RELEASE_VERSION
$configOverridePath = [IO.Path]::GetTempFileName()

try {
  $env:MINE_MAIL_NSIS_PAYLOAD = $resolvedPayload
  $env:MINE_MAIL_RELEASE_VERSION = $Version
  @{ version = $Version } |
    ConvertTo-Json |
    Set-Content -LiteralPath $configOverridePath -Encoding utf8

  Push-Location $installerRoot
  try {
    npm run tauri:build -- --no-bundle --config $configOverridePath
    if ($LASTEXITCODE -ne 0) {
      throw "The branded installer shell build failed."
    }
  }
  finally {
    Pop-Location
  }
}
finally {
  $env:MINE_MAIL_NSIS_PAYLOAD = $previousPayload
  $env:MINE_MAIL_RELEASE_VERSION = $previousVersion
  Remove-Item -LiteralPath $configOverridePath -Force -ErrorAction SilentlyContinue
}

$targetRoot = if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
  Join-Path $installerRoot "src-tauri/target"
}
else {
  [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
}
$shellBinary = Join-Path $targetRoot "release/mine-mail-installer.exe"
if (-not (Test-Path -LiteralPath $shellBinary -PathType Leaf)) {
  throw "The branded installer executable was not produced."
}

$assetName = "Mine-Mail_${Version}_x64-setup.exe"
$assetPath = Join-Path $resolvedOutput $assetName
Copy-Item -LiteralPath $shellBinary -Destination $assetPath -Force

Write-Output $assetPath
