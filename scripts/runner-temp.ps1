# Route this job's children before bootstrap; never sweep a shared temp root.
param(
    [Parameter(Mandatory)][ValidateSet('setup', 'cleanup')][string]$Action,
    [Parameter(Mandatory)][string]$DriveRoot,
    [string]$Fallback = [System.IO.Path]::GetTempPath()
)
$ErrorActionPreference = 'Stop'
if ($env:PRINTOBSERVER_PLATFORM -ne 'windows-x86_64') { return }

if ($Action -eq 'cleanup') {
    $scratch = $env:PRINTOBSERVER_JOB_SCRATCH
    if (-not $scratch) { return }
    $leaf = Split-Path -Leaf $scratch
    $marker = Join-Path $scratch '.printobserver-owned'
    if ($leaf -ne $env:PRINTOBSERVER_JOB_SCRATCH_OWNER -or
        $leaf -notmatch '^printobserver-[0-9a-f]{32}$' -or
        -not (Test-Path -LiteralPath $marker -PathType Leaf) -or
        (Get-Content -LiteralPath $marker -Raw).Trim() -ne $leaf) {
        throw "Refusing cleanup of unowned scratch: $scratch. Clear the foreign PRINTOBSERVER_JOB_SCRATCH and PRINTOBSERVER_JOB_SCRATCH_OWNER values."
    }
    Remove-Item -LiteralPath $scratch -Recurse -Force
    return
}

if (-not $env:GITHUB_ENV) { throw 'Set GITHUB_ENV to a writable environment file for later steps.' }
foreach ($path in @($DriveRoot, $Fallback, $env:GITHUB_ENV)) {
    if ($path -match '[\r\n]') { throw 'Paths must contain no line breaks. Supply a single-line path.' }
}
$scratch = ''
$owner = ''
$routing = 'existing temp; scratch drive is absent'
if (Test-Path -LiteralPath $DriveRoot -PathType Container) {
    $candidate = Join-Path $DriveRoot ('printobserver-' + [guid]::NewGuid().ToString('N'))
    $created = $false
    try {
        New-Item -ItemType Directory -Path $candidate | Out-Null
        $created = $true
        $leaf = Split-Path -Leaf $candidate
        [System.IO.File]::WriteAllText((Join-Path $candidate '.printobserver-owned'), $leaf)
        $scratch = $candidate
        $owner = $leaf
        $routing = 'job-owned scratch'
    } catch {
        $cause = $_.Exception.Message
        # Only a directory created by this invocation can be removed here.
        if ($created) { Remove-Item -LiteralPath $candidate -Recurse -Force }
        $routing = "existing temp; scratch creation failed: $cause"
    }
}
$chosen = if ($scratch) { $scratch } else { $Fallback }
$probe = Join-Path $chosen ([guid]::NewGuid().ToString('N') + '.probe')
$probeCreated = $false
try {
    $handle = [System.IO.File]::Open($probe, 'CreateNew', 'Write')
    $probeCreated = $true
    $handle.Dispose()
} catch {
    if ($scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
    throw "Temp directory $chosen is unusable: $($_.Exception.Message). Supply an existing writable fallback directory."
} finally {
    if ($probeCreated) { Remove-Item -LiteralPath $probe }
}
try {
    @("TEMP=$chosen", "TMP=$chosen", "PRINTOBSERVER_JOB_SCRATCH=$scratch",
      "PRINTOBSERVER_JOB_SCRATCH_OWNER=$owner") |
        Out-File -LiteralPath $env:GITHUB_ENV -Encoding utf8 -Append
} catch {
    if ($scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
    throw "Could not write GITHUB_ENV: $($_.Exception.Message). Set GITHUB_ENV to a writable environment file."
}
Write-Output "Children will use temp directory $chosen ($routing)"
