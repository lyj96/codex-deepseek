[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$secureKey = Read-Host 'DeepSeek API key (input is hidden)' -AsSecureString
$keyPointer = [IntPtr]::Zero
$plainKey = $null
try {
    $keyPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secureKey)
    $plainKey = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($keyPointer)
    if ([string]::IsNullOrWhiteSpace($plainKey)) {
        throw 'The API key cannot be empty.'
    }
    [Environment]::SetEnvironmentVariable('DEEPSEEK_API_KEY', $plainKey.Trim(), 'User')
    Write-Host 'DEEPSEEK_API_KEY was stored in the current Windows user environment.'
    Write-Host 'Fully quit and reopen Codex Desktop before testing.'
} finally {
    $plainKey = $null
    if ($keyPointer -ne [IntPtr]::Zero) {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($keyPointer)
    }
}
