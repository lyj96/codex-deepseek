[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "CodexDeepSeek"),
    [string]$DeepSeekKey,
    [string]$ReleaseTag = "latest",
    [string]$SshHost,
    [switch]$DiscoverSsh,
    [switch]$UpdateRemotes,
    [switch]$Yes
)

$ErrorActionPreference = "Stop"
$repo = "lyj96/codex-deepseek"
$asset = "codex-deepseek-package-x86_64-pc-windows-msvc.zip"
$catalogAsset = "deepseek-models.json"
$catalogName = "deepseek.json"
$configRoot = if ($env:APPDATA) { $env:APPDATA } else { $env:LOCALAPPDATA }
$managerDir = Join-Path $configRoot "CodexDeepSeek"
$managedHostsFile = Join-Path $managerDir "ssh-hosts"

function Assert-ReleaseTag {
    if ($ReleaseTag -ne "latest" -and $ReleaseTag -notmatch '^codex-v\d+\.\d+\.\d+-deepseek\.[1-9]\d*$') {
        throw "Invalid release tag: $ReleaseTag"
    }
}

function Assert-SshHost {
    param([Parameter(Mandatory)][string]$Name)

    if ([string]::IsNullOrWhiteSpace($Name) -or $Name.StartsWith('-') -or $Name -notmatch '^[A-Za-z0-9_.@:-]+$') {
        throw "Invalid SSH host or alias: $Name"
    }
}

function Get-RemoteInstallerUrl {
    if ($ReleaseTag -eq "latest") {
        return "https://github.com/$repo/releases/latest/download/install-linux.sh"
    }
    return "https://github.com/$repo/releases/download/$ReleaseTag/install-linux.sh"
}

function Get-LocalDeepSeekKey {
    param([string]$RequestedKey)

    $key = $RequestedKey
    if ([string]::IsNullOrWhiteSpace($key)) {
        $key = $env:DEEPSEEK_API_KEY
    }
    if ([string]::IsNullOrWhiteSpace($key)) {
        $key = [Environment]::GetEnvironmentVariable("DEEPSEEK_API_KEY", "User")
    }
    if (-not [string]::IsNullOrWhiteSpace($key) -and $key -match '[\r\n]') {
        throw "DeepSeek API Key cannot contain a newline."
    }
    return $key
}

function Register-ManagedHost {
    param([Parameter(Mandatory)][string]$Name)

    New-Item -ItemType Directory -Path $managerDir -Force | Out-Null
    if (-not (Test-Path -LiteralPath $managedHostsFile)) {
        [IO.File]::WriteAllText($managedHostsFile, "", [Text.UTF8Encoding]::new($false))
    }
    $managed = @(Get-Content -LiteralPath $managedHostsFile | Where-Object { $_ })
    if ($managed -notcontains $Name) {
        [IO.File]::AppendAllText($managedHostsFile, $Name + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    }
}

function Get-DiscoveredSshHosts {
    $sshConfig = Join-Path $env:USERPROFILE ".ssh\config"
    if (-not (Test-Path -LiteralPath $sshConfig -PathType Leaf)) {
        return @()
    }
    $candidates = foreach ($line in Get-Content -LiteralPath $sshConfig) {
        $withoutComment = ($line -replace '#.*$', '').Trim()
        if (-not $withoutComment) {
            continue
        }
        $parts = @($withoutComment -split '\s+')
        if ($parts.Count -lt 2 -or $parts[0] -ine 'Host') {
            continue
        }
        foreach ($candidate in $parts[1..($parts.Count - 1)]) {
            if ($candidate -notmatch '[*!?]' -and -not $candidate.StartsWith('!')) {
                $candidate
            }
        }
    }
    return @($candidates | Sort-Object -Unique)
}

function Show-DiscoveredSshHosts {
    $sshConfig = Join-Path $env:USERPROFILE ".ssh\config"
    $candidates = @(Get-DiscoveredSshHosts)
    if ($candidates.Count -eq 0) {
        Write-Host "No concrete SSH host aliases found in: $sshConfig"
        return
    }
    $managed = if (Test-Path -LiteralPath $managedHostsFile) { @(Get-Content -LiteralPath $managedHostsFile) } else { @() }
    Write-Host "SSH host candidates from $sshConfig`:"
    foreach ($candidate in $candidates) {
        $suffix = if ($managed -contains $candidate) { " (managed)" } else { "" }
        Write-Host "  $candidate$suffix"
    }
    Write-Host "Install and register one with: -SshHost HOST"
}

function Install-RemoteHost {
    param(
        [Parameter(Mandatory)][string]$Name,
        [switch]$ForwardLocalKey
    )

    Assert-SshHost -Name $Name
    $ssh = Get-Command ssh -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $ssh) {
        throw "OpenSSH client 'ssh' is required."
    }
    $installerUrl = Get-RemoteInstallerUrl
    $forwardKey = if ($ForwardLocalKey) { Get-LocalDeepSeekKey -RequestedKey $DeepSeekKey } else { $null }
    $remoteCommand = @'
set -eu
platform="$(uname -s)/$(uname -m)"
if [ "$platform" != "Linux/x86_64" ]; then
  echo "Unsupported remote platform: $platform (expected Linux/x86_64)" >&2
  exit 1
fi
tmp="$(mktemp "${TMPDIR:-/tmp}/codex-deepseek-installer.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
curl -fL --retry 3 '__INSTALLER_URL__' -o "$tmp"
chmod 700 "$tmp"
bash "$tmp" --ssh-remote --release '__RELEASE_TAG__'
'@
    $remoteCommand = $remoteCommand.Replace('__INSTALLER_URL__', $installerUrl).Replace('__RELEASE_TAG__', $ReleaseTag)
    Write-Host "Installing Codex DeepSeek on SSH host: $Name"
    if (-not [string]::IsNullOrWhiteSpace($forwardKey)) {
        $remoteCommand = 'IFS= read -r DEEPSEEK_API_KEY; DEEPSEEK_API_KEY="$(printf ''%s'' "$DEEPSEEK_API_KEY" | tr -d ''\r'')"; export DEEPSEEK_API_KEY' + [Environment]::NewLine + $remoteCommand
        $forwardKey | & $ssh.Source -- $Name $remoteCommand
    }
    else {
        & $ssh.Source -t -- $Name $remoteCommand
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Remote installation failed for SSH host: $Name"
    }
    Register-ManagedHost -Name $Name
    Write-Host "Registered managed SSH host: $Name"
}

function Update-ManagedRemoteHosts {
    param([string]$DesiredPackageVersion)

    if (-not (Test-Path -LiteralPath $managedHostsFile -PathType Leaf)) {
        Write-Host "No managed SSH hosts. Register one with -SshHost HOST."
        return
    }
    $managed = @(Get-Content -LiteralPath $managedHostsFile | Where-Object { $_ } | Sort-Object -Unique)
    if ($managed.Count -eq 0) {
        Write-Host "No managed SSH hosts. Register one with -SshHost HOST."
        return
    }
    $failed = @()
    foreach ($name in $managed) {
        try {
            Assert-SshHost -Name $name
            $remoteVersion = & ssh -o BatchMode=yes -o ConnectTimeout=8 -- $name 'test -f "$HOME/.config/codex-deepseek/ssh-managed" && sed -n "s/^package_version=//p" "$HOME/.config/codex-deepseek/ssh-managed"' 2>$null
            if ($LASTEXITCODE -ne 0) {
                throw "Host is unavailable or is no longer managed."
            }
            $remoteVersion = ([string]($remoteVersion | Select-Object -First 1)).Trim()
            if ($DesiredPackageVersion -and $remoteVersion -eq $DesiredPackageVersion) {
                Write-Host "Already current on SSH host: $name ($DesiredPackageVersion)"
                continue
            }
            Install-RemoteHost -Name $name
        }
        catch {
            Write-Warning "Skipping SSH host $name`: $($_.Exception.Message)"
            $failed += $name
        }
    }
    if ($failed.Count -gt 0) {
        throw "One or more managed SSH hosts could not be updated: $($failed -join ', ')"
    }
}

function Offer-RemoteUpdates {
    param([string]$DesiredPackageVersion)

    if (-not (Test-Path -LiteralPath $managedHostsFile -PathType Leaf)) {
        return
    }
    $managed = @(Get-Content -LiteralPath $managedHostsFile | Where-Object { $_ } | Sort-Object -Unique)
    if ($managed.Count -eq 0) {
        return
    }
    $shouldUpdate = $Yes
    if (-not $shouldUpdate) {
        $answer = Read-Host "Update $($managed.Count) registered SSH host(s) with this release? [y/N]"
        $shouldUpdate = $answer -match '^(?i:y|yes)$'
    }
    if ($shouldUpdate) {
        try {
            Update-ManagedRemoteHosts -DesiredPackageVersion $DesiredPackageVersion
        }
        catch {
            Write-Warning $_.Exception.Message
        }
    }
    else {
        Write-Host "Remote hosts were not changed. Run again with -UpdateRemotes when ready."
    }
}

Assert-ReleaseTag
$sshModeCount = @($DiscoverSsh.IsPresent, $UpdateRemotes.IsPresent, -not [string]::IsNullOrWhiteSpace($SshHost)).Where({ $_ }).Count
if ($sshModeCount -gt 1) {
    throw "Choose only one SSH operation at a time."
}
if ($DiscoverSsh) {
    Show-DiscoveredSshHosts
    return
}
if (-not [string]::IsNullOrWhiteSpace($SshHost)) {
    Install-RemoteHost -Name $SshHost -ForwardLocalKey
    return
}
if ($UpdateRemotes) {
    Update-ManagedRemoteHosts
    return
}

if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne [Runtime.InteropServices.Architecture]::X64) {
    throw "This installer only supports Windows x86_64."
}
$DeepSeekKey = Get-LocalDeepSeekKey -RequestedKey $DeepSeekKey
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
$packageVersion = $null

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
    $packageManifestPath = Join-Path $currentDir "codex-package.json"
    $packageVersion = [string](Get-Content -LiteralPath $packageManifestPath -Raw | ConvertFrom-Json).version
    if ([string]::IsNullOrWhiteSpace($packageVersion)) {
        throw "Installed package metadata does not contain a version."
    }

    $codexHome = if ($env:CODEX_HOME) { [IO.Path]::GetFullPath($env:CODEX_HOME) } else { Join-Path $env:USERPROFILE ".codex" }
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
    $launcherDir = Join-Path $env:LOCALAPPDATA "CodexDeepSeekLauncher"
    $launcherPath = Join-Path $launcherDir "codex-deepseek.cmd"
    New-Item -ItemType Directory -Path $launcherDir -Force | Out-Null
    [IO.File]::WriteAllText(
        $launcherPath,
        "@echo off`r`n`"%CODEX_DEEPSEEK_INSTALL_DIR%\current\bin\codex.exe`" %*`r`n",
        [Text.Encoding]::ASCII
    )
    [Environment]::SetEnvironmentVariable("DEEPSEEK_API_KEY", $DeepSeekKey, "User")
    [Environment]::SetEnvironmentVariable("CODEX_CLI_PATH", $codexPath, "User")
    [Environment]::SetEnvironmentVariable("CODEX_DEEPSEEK_INSTALL_DIR", $resolvedInstallDir, "User")
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = @($userPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if (-not ($pathEntries | Where-Object { $_.TrimEnd('\') -ieq $launcherDir.TrimEnd('\') })) {
        $newUserPath = (@($pathEntries) + $launcherDir) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
    }
    $env:DEEPSEEK_API_KEY = $DeepSeekKey
    $env:CODEX_CLI_PATH = $codexPath
    $env:CODEX_DEEPSEEK_INSTALL_DIR = $resolvedInstallDir
    if (-not (($env:Path -split ';') | Where-Object { $_.TrimEnd('\') -ieq $launcherDir.TrimEnd('\') })) {
        $env:Path = "$launcherDir;$env:Path"
    }

    & $codexPath features enable multi_agent_v2 | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to enable multi_agent_v2."
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

Offer-RemoteUpdates -DesiredPackageVersion $packageVersion
