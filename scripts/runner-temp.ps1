# Route this job's children before bootstrap; never sweep a shared temp root.
param(
    [Parameter(Mandatory)][ValidateSet('setup', 'cleanup')][string]$Action,
    [Parameter(Mandatory)][string]$DriveRoot,
    [string]$Fallback = [System.IO.Path]::GetTempPath()
)
$ErrorActionPreference = 'Stop'
if ($env:PRINTOBSERVER_PLATFORM -ne 'windows-x86_64') { return }

function Assert-LinePath([string]$Path) {
    if ($Path -match '[\r\n]') { throw 'Paths must contain no line breaks. Supply a single-line path.' }
}

function Assert-UnlinkedDirectory([string]$Path) {
    $directory = [System.IO.DirectoryInfo]::new($Path)
    while ($null -ne $directory) {
        if ($directory.Exists -and
            ($directory.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw "Choose a physical temp directory without symlinks or junctions: $($directory.FullName)"
        }
        $directory = $directory.Parent
    }
}

function Assert-WritableTemp([string]$Directory) {
    $probe = Join-Path $Directory ([guid]::NewGuid().ToString('N') + '.probe')
    $probeCreated = $false
    try {
        $handle = [System.IO.File]::Open($probe, 'CreateNew', 'Write')
        $probeCreated = $true
        $handle.Dispose()
    } catch {
        throw "Temp directory $Directory is unusable: $($_.Exception.Message). Supply an existing writable fallback directory."
    } finally {
        if ($probeCreated) { Remove-Item -LiteralPath $probe }
    }
}

function Publish-Temp([string]$Chosen, [string]$Scratch, [string]$Owner, [string]$Restored) {
    if (-not $env:GITHUB_ENV) { throw 'Set GITHUB_ENV to a writable environment file for later steps.' }
    foreach ($path in @($Chosen, $Scratch, $Restored, $env:GITHUB_ENV)) { Assert-LinePath $path }
    try {
        @("TEMP=$Chosen", "TMP=$Chosen", "PRINTOBSERVER_JOB_SCRATCH=$Scratch",
          "PRINTOBSERVER_JOB_SCRATCH_OWNER=$Owner", "PRINTOBSERVER_JOB_TEMP_FALLBACK=$Restored") |
            Out-File -LiteralPath $env:GITHUB_ENV -Encoding utf8 -Append
    } catch {
        throw "Could not write GITHUB_ENV: $($_.Exception.Message). Set GITHUB_ENV to a writable environment file."
    }
}

try {
    if ($Action -eq 'cleanup') {
        $scratch = $env:PRINTOBSERVER_JOB_SCRATCH
        if (-not $scratch) { return }
        $scratch = [System.IO.Path]::GetFullPath($scratch)
        $leaf = Split-Path -Leaf $scratch
        $marker = Join-Path $scratch '.printobserver-owned'
        if ($leaf -ne $env:PRINTOBSERVER_JOB_SCRATCH_OWNER -or
            $leaf -notmatch '^printobserver-[0-9a-f]{32}$' -or
            -not (Test-Path -LiteralPath $marker -PathType Leaf) -or
            (Get-Content -LiteralPath $marker -Raw).Trim() -ne $leaf) {
            throw "Refusing cleanup of unowned scratch: $scratch. Clear the foreign PRINTOBSERVER_JOB_SCRATCH and PRINTOBSERVER_JOB_SCRATCH_OWNER values."
        }
        $restored = $env:PRINTOBSERVER_JOB_TEMP_FALLBACK
        Assert-LinePath $restored
        $restored = [System.IO.Path]::GetFullPath($restored)
        Assert-UnlinkedDirectory $scratch
        Assert-UnlinkedDirectory $restored
        $relative = [System.IO.Path]::GetRelativePath($scratch, $restored)
        $separator = [System.IO.Path]::DirectorySeparatorChar
        if ($relative -eq '.' -or
            (-not [System.IO.Path]::IsPathRooted($relative) -and
             $relative -ne '..' -and -not $relative.StartsWith("..$separator"))) {
            throw "Choose a fallback outside this job's scratch before cleanup: $restored"
        }
        Assert-WritableTemp $restored
        # Post-job cache actions still spawn children after the workflow steps.
        Publish-Temp $restored '' '' $restored
        Remove-Item -LiteralPath $scratch -Recurse -Force
        return
    }

    if (-not $env:GITHUB_ENV) { throw 'Set GITHUB_ENV to a writable environment file for later steps.' }
    foreach ($path in @($DriveRoot, $Fallback, $env:GITHUB_ENV)) { Assert-LinePath $path }
    $DriveRoot = [System.IO.Path]::GetFullPath($DriveRoot)
    $Fallback = [System.IO.Path]::GetFullPath($Fallback)
    Assert-UnlinkedDirectory $DriveRoot
    Assert-UnlinkedDirectory $Fallback
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
    try {
        Assert-WritableTemp $chosen
        Publish-Temp $chosen $scratch $owner $Fallback
    } catch {
        if ($scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
        throw
    }
    Write-Output "Children will use temp directory $chosen ($routing)"
} catch {
    [Console]::Error.WriteLine("$($_.Exception.Message) Restore access to the named job files and retry just runner-temp $Action.")
    exit 1
}
