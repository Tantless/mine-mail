[CmdletBinding()]
param(
    [string]$Model = "deepseek-v4-pro",

    [string]$BaseUrl = "https://api.deepseek.com",

    [switch]$ConfirmBillableRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $ConfirmBillableRun) {
    throw "This command can call a billable AI API. Re-run it with -ConfirmBillableRun."
}
if ([string]::IsNullOrWhiteSpace($env:DEEPSEEK_API_KEY)) {
    throw "DEEPSEEK_API_KEY is not available in this process environment."
}

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\..\..\.."))
$tauriRoot = Join-Path $repoRoot "web\src-tauri"
$bootstrapData = Join-Path $env:LOCALAPPDATA "com.minemail.desktop"
$locatorPath = Join-Path $bootstrapData "storage-location.json"
$productData = $bootstrapData
if (Test-Path -LiteralPath $locatorPath -PathType Leaf) {
    $locator = Get-Content -LiteralPath $locatorPath -Raw | ConvertFrom-Json
    if ($locator.schema_version -ne 1 -or [string]::IsNullOrWhiteSpace($locator.data_root)) {
        throw "Mine Mail's storage locator is invalid."
    }
    $productData = [IO.Path]::GetFullPath([string]$locator.data_root)
}
if (-not (Test-Path -LiteralPath $productData -PathType Container)) {
    throw "Mine Mail local product data was not found."
}

$managedNames = @(
    "MINE_MAIL_RUN_AI_CHAIN",
    "MINE_MAIL_AI_TEST_DATA_ROOT",
    "MINE_MAIL_AI_TEST_MODEL",
    "MINE_MAIL_AI_TEST_BASE_URL"
)
$previous = @{}
foreach ($name in $managedNames) {
    $previous[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}

try {
    $env:MINE_MAIL_RUN_AI_CHAIN = "1"
    $env:MINE_MAIL_AI_TEST_DATA_ROOT = $productData
    $env:MINE_MAIL_AI_TEST_MODEL = $Model.Trim()
    $env:MINE_MAIL_AI_TEST_BASE_URL = $BaseUrl.Trim()

    Push-Location $tauriRoot
    try {
        $testOutput = @(cargo test ai::manual_chain_tests::manual_deepseek_ai_chain --lib -- --ignored --exact --nocapture 2>&1)
        if ($LASTEXITCODE -ne 0) {
            $testOutput |
                Where-Object { "$_" -match "panicked at|FAILED|test result:" } |
                ForEach-Object {
                    $safeLine = "$_" -replace "panicked at .*:\d+:\d+:", "panicked:"
                    Write-Host $safeLine
                }
            throw "The manual AI chain test failed with exit code $LASTEXITCODE."
        }
    }
    finally {
        Pop-Location
    }

    $reportLine = $testOutput | Where-Object { "$_".Contains("AI_CHAIN_REPORT ") } | Select-Object -Last 1
    if (-not $reportLine) {
        throw "The AI chain test completed without producing its privacy-safe report."
    }
    $reportText = "$reportLine"
    $reportOffset = $reportText.IndexOf("AI_CHAIN_REPORT ") + "AI_CHAIN_REPORT ".Length
    $report = $reportText.Substring($reportOffset) | ConvertFrom-Json
    [pscustomobject]@{
        Provider = $report.provider
        Protocol = $report.protocol
        Model = $report.model
        RealMailDigest = $report.real_mail_digest
        Passed = @($report.cases | Where-Object { $_.status -eq "passed" }).Count
        Total = @($report.cases).Count
    } | Format-List
    $report.cases | Select-Object case, status, duration_ms, tool_activity_count, changed_fields, decision, output_bytes, output_digest, note | Format-Table -AutoSize
}
finally {
    foreach ($name in $managedNames) {
        [Environment]::SetEnvironmentVariable($name, $previous[$name], "Process")
    }
}
