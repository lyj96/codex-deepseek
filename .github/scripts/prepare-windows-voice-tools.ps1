# Build-only tools from public sources; never copied into release packages.
$ErrorActionPreference = 'Stop'
if ($env:RUNNER_OS -ne 'Windows' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
    throw 'This provisioning script is restricted to official GitHub Windows runners.'
}
$architecture = switch ($env:RUNNER_ARCH) {
    'X64' { 'x64' }
    'ARM64' { 'arm64' }
    default { throw "Unsupported runner architecture: $env:RUNNER_ARCH" }
}
$digest = @{
    x64 = '5604cf25ef38bb6a09520cff25ae9f0ecd8c2443053b15e40df4ad1eae0e4405'
    arm64 = 'd5752ce2ac2296c8abb91fb12c12e75ee99ba8a18b52ce5febf325847b6f062b'
}[$architecture]
$repository = Join-Path $env:RUNNER_TEMP 'voice-windows-tools'
$installer = Join-Path $env:RUNNER_TEMP "pkgconf-$architecture-3.0.6.msi"
Invoke-WebRequest "https://github.com/pkgconf/pkgconf/releases/download/pkgconf-3.0.6/pkgconf-$architecture-3.0.6.msi" -OutFile $installer
if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant() -ne $digest) {
    throw 'pkgconf installer SHA-256 mismatch'
}
$image = Join-Path $repository 'pkgconf-image'
$process = Start-Process msiexec.exe -ArgumentList @('/a', "`"$installer`"", '/qn', "TARGETDIR=`"$image`"") -Wait -PassThru -WindowStyle Hidden
if ($process.ExitCode -ne 0) { throw "pkgconf extraction failed: $($process.ExitCode)" }
$pkgconf = @(Get-ChildItem -LiteralPath $image -Filter pkgconf.exe -Recurse)
if ($pkgconf.Count -ne 1) { throw 'Expected exactly one native pkgconf.exe' }
$tools = @{
    shell = 'cygwin/bin/bash.exe'
    make = 'cygwin/bin/make.exe'
    cygpath = 'cygwin/bin/cygpath.exe'
    automake = 'cygwin/bin/automake-1.18'
    pkg_config = [IO.Path]::GetRelativePath($repository, $pkgconf[0].FullName).Replace('\', '/')
}
foreach ($tool in $tools.Values) {
    if (-not (Test-Path -LiteralPath (Join-Path $repository $tool))) { throw "Missing declared tool: $tool" }
}
$target = if ($architecture -eq 'arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
$manifest = @{ schemaVersion = 1; target = $target; cygwinArchitecture = 'x86_64'; tools = $tools }
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $repository 'voice-tools.json') -Encoding utf8NoBOM
# Declare the installed support trees, not only their executable entrypoints.
'filegroup(name = "tools", srcs = glob(["cygwin/bin/**", "cygwin/lib/**", "cygwin/usr/**", "cygwin/etc/**", "pkgconf-image/**"]) + ["voice-tools.json"], visibility = ["//visibility:public"])' |
    Set-Content -LiteralPath (Join-Path $repository 'BUILD.bazel') -Encoding utf8NoBOM
'module(name = "voice_windows_tools")' | Set-Content -LiteralPath (Join-Path $repository 'MODULE.bazel') -Encoding utf8NoBOM
"VOICE_WINDOWS_BAZEL_REPOSITORY=$($repository.Replace('\', '/'))" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
