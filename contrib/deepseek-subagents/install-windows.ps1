[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "CodexDeepSeek"),
    [string]$DeepSeekKey,
    [string]$ReleaseTag = "latest"
)

$ErrorActionPreference = "Stop"
$repo = "lyj96/codex-deepseek"
$asset = "codex-deepseek-package-x86_64-pc-windows-msvc.zip"
$catalogAsset = "deepseek-models.json"
$catalogName = "deepseek.json"

if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne [Runtime.InteropServices.Architecture]::X64) {
    throw "This installer only supports Windows x86_64."
}
if ([string]::IsNullOrWhiteSpace($DeepSeekKey)) {
    $secureKey = Read-Host "DeepSeek API Key" -AsSecureString
    $keyPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secureKey)
    try {
        $DeepSeekKey = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($keyPointer)
    }
    finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($keyPointer)
    }
}
if ([string]::IsNullOrWhiteSpace($DeepSeekKey)) {
    throw "DeepSeek API Key cannot be empty."
}
if ($DeepSeekKey -match '[\r\n]') {
    throw "DeepSeek API Key cannot contain a newline."
}

$resolvedInstallDir = [IO.Path]::GetFullPath($InstallDir)
$installRoot = [IO.Path]::GetPathRoot($resolvedInstallDir)
if ($resolvedInstallDir.TrimEnd('\') -eq $installRoot.TrimEnd('\')) {
    throw "InstallDir cannot be a drive root."
}

if ($ReleaseTag -eq "latest") {
    $downloadRoot = "https://github.com/$repo/releases/latest/download"
}
else {
    $downloadRoot = "https://github.com/$repo/releases/download/$ReleaseTag"
}

$tempDir = Join-Path ([IO.Path]::GetTempPath()) ("codex-deepseek-" + [Guid]::NewGuid())
$archivePath = Join-Path $tempDir $asset
$stagingDir = Join-Path $tempDir "package"
$catalogPath = Join-Path $tempDir $catalogAsset
$checksumsPath = Join-Path $tempDir "SHA256SUMS"
$currentDir = Join-Path $resolvedInstallDir "current"
$backupDir = $null

try {
    New-Item -ItemType Directory -Path $tempDir, $stagingDir -Force | Out-Null
    Invoke-WebRequest "$downloadRoot/$asset" -OutFile $archivePath
    Invoke-WebRequest "$downloadRoot/$catalogAsset" -OutFile $catalogPath
    Invoke-WebRequest "$downloadRoot/SHA256SUMS" -OutFile $checksumsPath
    $checksumLines = Get-Content -LiteralPath $checksumsPath
    foreach ($download in @(
        @{ Name = $asset; Path = $archivePath },
        @{ Name = $catalogAsset; Path = $catalogPath }
    )) {
        $pattern = "^([0-9a-fA-F]{64})\s+\*?" + [Regex]::Escape($download.Name) + "$"
        $match = $checksumLines | Select-String -Pattern $pattern | Select-Object -First 1
        if (-not $match) {
            throw "SHA256SUMS does not contain $($download.Name)."
        }
        $expectedHash = $match.Matches[0].Groups[1].Value.ToLowerInvariant()
        $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $download.Path).Hash.ToLowerInvariant()
        if ($actualHash -ne $expectedHash) {
            throw "SHA-256 verification failed for $($download.Name)."
        }
    }
    Expand-Archive -LiteralPath $archivePath -DestinationPath $stagingDir

    $stagedCodex = Join-Path $stagingDir "bin\codex.exe"
    if (-not (Test-Path -LiteralPath $stagedCodex -PathType Leaf)) {
        throw "Downloaded package does not contain bin\codex.exe."
    }
    & $stagedCodex --version | Out-Host

    New-Item -ItemType Directory -Path $resolvedInstallDir -Force | Out-Null
    if (Test-Path -LiteralPath $currentDir) {
        $backupDir = Join-Path $resolvedInstallDir ("previous-" + (Get-Date -Format "yyyyMMddHHmmss") + "-" + [Guid]::NewGuid().ToString("N").Substring(0, 6))
        Move-Item -LiteralPath $currentDir -Destination $backupDir
    }
    try {
        Move-Item -LiteralPath $stagingDir -Destination $currentDir
    }
    catch {
        if ($backupDir -and (Test-Path -LiteralPath $backupDir) -and -not (Test-Path -LiteralPath $currentDir)) {
            Move-Item -LiteralPath $backupDir -Destination $currentDir
        }
        throw
    }

    $codexHome = Join-Path $env:USERPROFILE ".codex"
    $modelCatalogDir = Join-Path $codexHome "model-catalogs"
    New-Item -ItemType Directory -Path $modelCatalogDir -Force | Out-Null
    Copy-Item -LiteralPath $catalogPath -Destination (Join-Path $modelCatalogDir $catalogName) -Force

    $configPath = Join-Path $codexHome "config.toml"
    if (-not (Test-Path -LiteralPath $configPath)) {
        New-Item -ItemType File -Path $configPath -Force | Out-Null
    }
    $configText = Get-Content -LiteralPath $configPath -Raw
    if ($configText -notmatch '(?m)^\s*\[model_providers\.deepseek\]\s*(?:#.*)?$') {
        if ((Get-Item -LiteralPath $configPath).Length -gt 0) {
            Copy-Item -LiteralPath $configPath -Destination ($configPath + ".bak." + (Get-Date -Format "yyyyMMddHHmmss"))
        }
        $providerConfig = @"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/"
env_key = "DEEPSEEK_API_KEY"
wire_api = "responses"
supports_websockets = false
"@
        [IO.File]::AppendAllText(
            $configPath,
            $providerConfig + [Environment]::NewLine,
            [Text.UTF8Encoding]::new($false)
        )
    }

    $codexPath = Join-Path $currentDir "bin\codex.exe"
    $launcherPath = Join-Path $resolvedInstallDir "codex-deepseek.cmd"
    [IO.File]::WriteAllText(
        $launcherPath,
        "@echo off`r`n`"%~dp0current\bin\codex.exe`" %*`r`n",
        [Text.Encoding]::ASCII
    )
    [Environment]::SetEnvironmentVariable("DEEPSEEK_API_KEY", $DeepSeekKey, "User")
    [Environment]::SetEnvironmentVariable("CODEX_CLI_PATH", $codexPath, "User")
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = @($userPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if (-not ($pathEntries | Where-Object { $_.TrimEnd('\') -ieq $resolvedInstallDir.TrimEnd('\') })) {
        $newUserPath = (@($pathEntries) + $resolvedInstallDir) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
    }
    $env:DEEPSEEK_API_KEY = $DeepSeekKey
    $env:CODEX_CLI_PATH = $codexPath
    if (-not (($env:Path -split ';') | Where-Object { $_.TrimEnd('\') -ieq $resolvedInstallDir.TrimEnd('\') })) {
        $env:Path = "$resolvedInstallDir;$env:Path"
    }

    Write-Host "Installed Codex DeepSeek to: $currentDir"
    Write-Host "CODEX_CLI_PATH: $codexPath"
    Write-Host "CLI command: codex-deepseek"
    if ($backupDir) {
        Write-Host "Previous installation kept at: $backupDir"
    }
    Write-Host "Fully quit and reopen Codex Desktop before using DeepSeek subagents."
}
finally {
    if (Test-Path -LiteralPath $tempDir) {
        Remove-Item -LiteralPath $tempDir -Recurse -Force
    }
}
