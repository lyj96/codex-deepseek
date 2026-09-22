param(
    [Parameter(Mandatory)][string]$MsiPath,
    [Parameter(Mandatory)][string]$Destination
)
$ErrorActionPreference = 'Stop'
# Read the pinned package as an archive. Never execute MSI actions or change
# Windows Installer registration, machine settings, or installed products.
$allowedHashes = @(
    '5604cf25ef38bb6a09520cff25ae9f0ecd8c2443053b15e40df4ad1eae0e4405',
    'd5752ce2ac2296c8abb91fb12c12e75ee99ba8a18b52ce5febf325847b6f062b'
)
if ((Get-FileHash -LiteralPath $MsiPath -Algorithm SHA256).Hash.ToLowerInvariant() -notin $allowedHashes) {
    throw 'Only the pinned pkgconf 3.0.6 x64/ARM64 packages may be extracted.'
}
$Destination = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $Destination) { throw 'Extraction destination must not already exist.' }
New-Item -ItemType Directory -Path $Destination | Out-Null
$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.OpenDatabase([IO.Path]::GetFullPath($MsiPath), 0)
$view = $database.OpenView('SELECT `Data` FROM `_Streams` WHERE `Name` = ''cab1.cab''')
$view.Execute()
$record = $view.Fetch()
if ($null -eq $record) { throw 'Pinned pkgconf cabinet is missing.' }
$cabinet = Join-Path $Destination 'payload.cab'
$stream = [IO.File]::Create($cabinet)
try {
    do {
        $chunk = $record.ReadStream(1, 65536, 1)
        if ($chunk.Length -gt 0) {
            $bytes = [byte[]][char[]]$chunk
            $stream.Write($bytes, 0, $bytes.Length)
        }
    } while ($chunk.Length -gt 0)
} finally {
    $stream.Dispose()
    $view.Close()
    foreach ($object in @($record, $view, $database, $installer)) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($object)
    }
}
& "$env:SystemRoot\System32\expand.exe" '-F:*' $cabinet $Destination | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Cabinet extraction failed: $LASTEXITCODE" }
$files = @{
    PkgconfExeFile = 'pkgconf.exe'
    PkgconfigExeFile = 'pkg-config.exe'
    BomtoolExeFile = 'bomtool.exe'
    SpdxtoolExeFile = 'spdxtool.exe'
    PccriticExeFile = 'pccritic.exe'
}
foreach ($entry in $files.GetEnumerator()) {
    Move-Item -LiteralPath (Join-Path $Destination $entry.Key) -Destination (Join-Path $Destination $entry.Value)
}
Remove-Item -LiteralPath $cabinet
