param([Parameter(Mandatory)][string]$PackageDirectory)
$ErrorActionPreference = 'Stop'
$testRoot = Join-Path $PackageDirectory ('extraction-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
foreach ($architecture in @('x64', 'arm64')) {
    $package = Join-Path $PackageDirectory "pkgconf-$architecture-3.0.6.msi"
    $output = Join-Path $testRoot "image with spaces $architecture"
    & (Join-Path $PSScriptRoot 'extract-pkgconf.ps1') -MsiPath $package -Destination $output
    if (@(Get-ChildItem -LiteralPath $output -File).Count -ne 5) { throw 'Expected five extracted executables.' }
    $exe = Join-Path $output 'pkgconf.exe'
    $bytes = [IO.File]::ReadAllBytes($exe)
    $pe = [BitConverter]::ToInt32($bytes, 0x3c)
    $expectedMachine = if ($architecture -eq 'x64') { 0x8664 } else { 0xaa64 }
    if ([BitConverter]::ToUInt16($bytes, $pe + 4) -ne $expectedMachine) { throw 'Incorrect PE architecture.' }
    if ($architecture -eq 'x64') {
        $version = & $exe --version
        if ($LASTEXITCODE -ne 0 -or $version -ne '3.0.6') { throw 'pkgconf version smoke test failed.' }
        $pc = Join-Path $output 'probe.pc'
        @'
prefix=C:/SDK with spaces
libdir=${prefix}/lib
Name: probe
Description: Real pkgconf extraction smoke test
Version: 1.2.3
Libs: -L${libdir} -lprobe
'@ | Set-Content -LiteralPath $pc -Encoding utf8NoBOM
        $oldPath = $env:PKG_CONFIG_PATH
        try {
            $env:PKG_CONFIG_PATH = $output
            if ((& $exe --modversion probe) -ne '1.2.3' -or $LASTEXITCODE -ne 0) { throw 'PC discovery failed.' }
            if ((& $exe --variable=prefix probe) -ne 'C:/SDK with spaces' -or $LASTEXITCODE -ne 0) { throw 'Spaced prefix was not preserved.' }
        } finally { $env:PKG_CONFIG_PATH = $oldPath }
    }
    $rejected = $false
    try { & (Join-Path $PSScriptRoot 'extract-pkgconf.ps1') -MsiPath $package -Destination $output }
    catch { if ($_.Exception.Message -notmatch 'must not already exist') { throw }; $rejected = $true }
    if (-not $rejected) { throw 'Existing output was not protected.' }
}
$invalid = Join-Path $testRoot 'invalid.msi'
[IO.File]::WriteAllText($invalid, 'not the pinned package')
$rejected = $false
try { & (Join-Path $PSScriptRoot 'extract-pkgconf.ps1') -MsiPath $invalid -Destination (Join-Path $testRoot 'bad-output') }
catch { if ($_.Exception.Message -notmatch 'Only the pinned') { throw }; $rejected = $true }
if (-not $rejected) { throw 'Untrusted package was not rejected.' }
Write-Output "PASS: x64 live execution/PC lookup, ARM64 PE check, spaces, overwrite and hash rejection. Artifacts: $testRoot"
