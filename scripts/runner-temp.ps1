# Route this job's children before bootstrap; never sweep a shared temp root.
param(
    [ValidateSet('setup', 'cleanup')][string]$Action = 'setup',
    [string]$DriveRoot = 'D:\',
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
        throw "Refusing cleanup of unowned scratch: $scratch"
    }
    Remove-Item -LiteralPath $scratch -Recurse -Force
    return
}

if (-not $env:GITHUB_ENV) { throw 'GITHUB_ENV must name the environment file for later steps' }
$scratch = ''
$owner = ''
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
    } catch {
        # Only a directory created by this invocation can be removed here.
        if ($created) {
            Remove-Item -LiteralPath $candidate -Recurse -Force
        }
        Write-Warning "Scratch drive is not writable; using existing temp: $Fallback"
    }
}
$chosen = if ($scratch) { $scratch } else { $Fallback }
$probe = Join-Path $chosen ([guid]::NewGuid().ToString('N') + '.probe')
try {
    $handle = [System.IO.File]::Open($probe, 'CreateNew', 'Write')
    $handle.Dispose()
} finally {
    if (Test-Path -LiteralPath $probe -PathType Leaf) { Remove-Item -LiteralPath $probe }
}
@("TEMP=$chosen", "TMP=$chosen", "PRINTOBSERVER_JOB_SCRATCH=$scratch",
  "PRINTOBSERVER_JOB_SCRATCH_OWNER=$owner") |
    Out-File -LiteralPath $env:GITHUB_ENV -Encoding utf8 -Append
Write-Output "Children will use temp directory $chosen"
