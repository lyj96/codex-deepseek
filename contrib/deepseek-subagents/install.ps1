[CmdletBinding()]
param(
    [string]$CodexExe,
    [switch]$SkipDesktopOverride
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$officialSetupUrl = 'https://cdn.deepseek.com/api-docs/codex-deepseek-setup-en.ps1'
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$codexHomeDir = Join-Path $env:USERPROFILE '.codex'
$agentsDir = Join-Path $codexHomeDir 'agents'
$configPath = Join-Path $codexHomeDir 'config.toml'
$installRoot = Join-Path $env:LOCALAPPDATA 'CodexDeepSeek'
$installDir = Join-Path $installRoot 'current'
$resourcesDir = Join-Path $installDir 'resources'
$backupRoot = Join-Path $codexHomeDir 'backup-deepseek-subagents'
$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$backupDir = Join-Path $backupRoot $timestamp
$beginMarker = '# BEGIN CODEX DEEPSEEK SUBAGENTS (managed)'
$endMarker = '# END CODEX DEEPSEEK SUBAGENTS (managed)'

if ([string]::IsNullOrWhiteSpace($CodexExe)) {
    $releaseCandidate = Join-Path $repoRoot 'codex-rs\target\x86_64-pc-windows-gnu\release\codex.exe'
    $debugCandidate = Join-Path $repoRoot 'codex-rs\target\x86_64-pc-windows-gnu\debug\codex.exe'
    if (Test-Path -LiteralPath $releaseCandidate) {
        $CodexExe = $releaseCandidate
    } elseif (Test-Path -LiteralPath $debugCandidate) {
        $CodexExe = $debugCandidate
    } else {
        throw 'Custom codex.exe was not found. Build the Windows target first or pass -CodexExe.'
    }
}

$resolvedCodexExe = (Resolve-Path -LiteralPath $CodexExe).Path
$versionOutput = & $resolvedCodexExe --version
if ($LASTEXITCODE -ne 0) {
    throw "Custom Codex executable failed its version check: $resolvedCodexExe"
}

New-Item -ItemType Directory -Force -Path $codexHomeDir, $agentsDir, $installDir, $resourcesDir, $backupDir | Out-Null

$configExisted = Test-Path -LiteralPath $configPath
if ($configExisted) {
    Copy-Item -LiteralPath $configPath -Destination (Join-Path $backupDir 'config.toml')
}
[System.IO.File]::WriteAllText(
    (Join-Path $backupDir 'config-existed.txt'),
    $configExisted.ToString() + "`n",
    $utf8NoBom
)

$previousCliPath = [Environment]::GetEnvironmentVariable('CODEX_CLI_PATH', 'User')
$state = [ordered]@{
    InstalledAt = (Get-Date).ToString('o')
    BackupDirectory = $backupDir
    PreviousCodexCliPath = $previousCliPath
    CustomCodexPath = (Join-Path $installDir 'codex.exe')
}
[System.IO.File]::WriteAllText(
    (Join-Path $backupDir 'install-state.json'),
    ($state | ConvertTo-Json -Depth 3) + "`n",
    $utf8NoBom
)

$officialScript = Invoke-RestMethod -Uri $officialSetupUrl
$catalogMatch = [regex]::Match(
    $officialScript,
    "(?s)\`$ModelsJson\s*=\s*@'\r?\n(?<json>.*?)\r?\n'@"
)
if (-not $catalogMatch.Success) {
    throw 'Could not extract the official DeepSeek model catalog.'
}
$catalogText = $catalogMatch.Groups['json'].Value.TrimEnd("`r", "`n") + "`n"
$catalog = $catalogText | ConvertFrom-Json
$slugs = @($catalog.models | ForEach-Object { $_.slug })
foreach ($requiredSlug in @('deepseek-flash', 'deepseek-v4-pro')) {
    if ($requiredSlug -notin $slugs) {
        throw "Official DeepSeek catalog does not contain $requiredSlug."
    }
}
$catalogPath = Join-Path $agentsDir 'deepseek-models.json'
[System.IO.File]::WriteAllText($catalogPath, $catalogText, $utf8NoBom)

$roleToml = @'
model = "deepseek-v4-pro"
model_provider = "deepseek"
model_catalog_json = "deepseek-models.json"
model_reasoning_effort = "high"
developer_instructions = "Work as a focused implementation subagent. Complete the assigned task independently, verify your work, and report concise evidence to the parent agent. Do not assume access to the parent conversation; rely on the task message and files in the shared workspace."
'@
$rolePath = Join-Path $agentsDir 'deepseek-worker.toml'
[System.IO.File]::WriteAllText($rolePath, $roleToml.Trim() + "`n", $utf8NoBom)

$configText = if ($configExisted) {
    [System.IO.File]::ReadAllText($configPath)
} else {
    ''
}
$managedPattern = '(?ms)^' + [regex]::Escape($beginMarker) + '.*?^' + [regex]::Escape($endMarker) + '\s*'
$configWithoutManagedBlock = [regex]::Replace($configText, $managedPattern, '')
foreach ($section in @('model_providers.deepseek', 'agents.deepseek_worker')) {
    $sectionPattern = '(?m)^\s*\[' + [regex]::Escape($section) + '\]\s*$'
    if ([regex]::IsMatch($configWithoutManagedBlock, $sectionPattern)) {
        throw "config.toml already contains [$section] outside the managed block. The original file is backed up at $backupDir."
    }
}

$roleTomlPath = $rolePath -replace '\\', '/'
$managedBlock = @"
$beginMarker
[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/"
env_key = "DEEPSEEK_API_KEY"
wire_api = "responses"
supports_websockets = false

[agents.deepseek_worker]
description = "Delegate a self-contained coding task to DeepSeek V4 Pro through the official DeepSeek Responses API."
config_file = "$roleTomlPath"
nickname_candidates = ["Turing", "Hopper", "Shannon"]
$endMarker
"@
$updatedConfig = $configWithoutManagedBlock.TrimEnd() + "`r`n`r`n" + $managedBlock.Trim() + "`r`n"
[System.IO.File]::WriteAllText($configPath, $updatedConfig, $utf8NoBom)

$targetCodex = Join-Path $installDir 'codex.exe'
Copy-Item -LiteralPath $resolvedCodexExe -Destination $targetCodex -Force

$package = Get-AppxPackage -Name OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
if ($null -eq $package) {
    throw 'The Codex desktop package is not installed.'
}
$desktopResources = Join-Path $package.InstallLocation 'app\resources'
foreach ($helperName in @(
    'codex-code-mode-host.exe',
    'codex-command-runner.exe',
    'codex-windows-sandbox-setup.exe',
    'rg.exe'
)) {
    $helperPath = Join-Path $desktopResources $helperName
    if (-not (Test-Path -LiteralPath $helperPath)) {
        throw "Desktop helper is missing: $helperPath"
    }
    Copy-Item -LiteralPath $helperPath -Destination (Join-Path $installDir $helperName) -Force
    Copy-Item -LiteralPath $helperPath -Destination (Join-Path $resourcesDir $helperName) -Force
}

if (-not $SkipDesktopOverride) {
    [Environment]::SetEnvironmentVariable('CODEX_CLI_PATH', $targetCodex, 'User')
}

Write-Host "Installed custom Codex: $targetCodex"
Write-Host "Custom CLI version: $versionOutput"
Write-Host "DeepSeek role: deepseek_worker"
Write-Host "Backup: $backupDir"
if ($SkipDesktopOverride) {
    Write-Host 'Desktop override was skipped.'
} else {
    Write-Host 'CODEX_CLI_PATH is set for new processes. Fully quit and reopen Codex Desktop after setting the API key.'
}
