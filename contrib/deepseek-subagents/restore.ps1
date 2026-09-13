[CmdletBinding()]
param(
    [string]$BackupDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$codexHomeDir = Join-Path $env:USERPROFILE '.codex'
$backupRoot = Join-Path $codexHomeDir 'backup-deepseek-subagents'
$configPath = Join-Path $codexHomeDir 'config.toml'

if ([string]::IsNullOrWhiteSpace($BackupDirectory)) {
    $latestBackup = Get-ChildItem -LiteralPath $backupRoot -Directory -ErrorAction Stop |
        Sort-Object Name -Descending |
        Select-Object -First 1
    if ($null -eq $latestBackup) {
        throw "No backup was found under $backupRoot."
    }
    $BackupDirectory = $latestBackup.FullName
}

$resolvedBackup = (Resolve-Path -LiteralPath $BackupDirectory).Path
if (-not $resolvedBackup.StartsWith((Resolve-Path -LiteralPath $backupRoot).Path, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Backup directory must be inside $backupRoot."
}

$statePath = Join-Path $resolvedBackup 'install-state.json'
$configExistedPath = Join-Path $resolvedBackup 'config-existed.txt'
if (-not (Test-Path -LiteralPath $statePath) -or -not (Test-Path -LiteralPath $configExistedPath)) {
    throw 'The selected backup is incomplete.'
}

$state = Get-Content -LiteralPath $statePath -Raw -Encoding UTF8 | ConvertFrom-Json
$configExisted = [bool]::Parse((Get-Content -LiteralPath $configExistedPath -Raw -Encoding UTF8).Trim())
if ($configExisted) {
    $savedConfig = Join-Path $resolvedBackup 'config.toml'
    if (-not (Test-Path -LiteralPath $savedConfig)) {
        throw 'The backup does not contain config.toml.'
    }
    Copy-Item -LiteralPath $savedConfig -Destination $configPath -Force
} else {
    if (Test-Path -LiteralPath $configPath) {
        $restoredAside = Join-Path $resolvedBackup 'config-created-by-install.toml'
        Move-Item -LiteralPath $configPath -Destination $restoredAside
    }
}

[Environment]::SetEnvironmentVariable('CODEX_CLI_PATH', $state.PreviousCodexCliPath, 'User')
Write-Host "Restored Codex configuration from $resolvedBackup"
Write-Host 'The DeepSeek API key was not removed. Fully quit and reopen Codex Desktop.'
