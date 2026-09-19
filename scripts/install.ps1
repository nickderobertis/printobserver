# Put the printobserver program on your path, already built for your platform.
#
# The Windows form of the third of the three ALTERNATIVE routes AGENTS.md's "The
# end-user install path" section states — `scripts/install.sh` is the same route
# on Linux and macOS — and `just check-repo` refuses a tree in which this path
# differs from what that section's fetch URL names. It is the route for a
# machine that has neither package manager: nothing here needs Python, Node or
# a Rust toolchain, and nothing here compiles anything, because the host this
# runs on is the small machine beside the printer, which is the worst place to
# build a Rust workspace.
#
# It does on Windows exactly what the shell script does elsewhere, against the
# same artifact and the same checksum file. It FAILS CLOSED: where it cannot
# work out the platform, cannot download, or cannot verify what it downloaded,
# it stops without installing and says both why and what to do next.
# Verification happens before anything reaches a path: the archive is unpacked
# into a directory of this script's own, checked against the release's own
# checksum file, and only then moved to where you asked for it.
#
# Usage:
#   irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.ps1 | iex
#   & ([scriptblock]::Create((irm <that URL>))) [-Version vX.Y.Z] [-To DIR]
#
#   -Version  install that release rather than the newest one.
#   -To       install into DIR rather than into $env:LOCALAPPDATA\Programs\printobserver.
#
# PRINTOBSERVER_RELEASE_BASE points this at somewhere other than the project's
# own releases — a mirror, or a directory holding a release. It takes the same
# two shapes the forge serves: `<base>/latest/download/<asset>` for the newest
# release and `<base>/download/<tag>/<asset>` for a pinned one.
param(
    [string]$Version = '',
    [string]$To = '',
    [switch]$Help
)

# The whole route is one function, called once at the end, so that what it sets
# — the strict mode, the error preference, the progress preference — is scoped
# to it and leaves the session of a caller who ran `irm ... | iex` as it was.
function Install-Printobserver([string]$Version, [string]$To, [bool]$Help) {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'

    # The program's own file name here, and the stem the release names its
    # artifact by: `printobserver-windows-x86_64.tar.gz` carries
    # `printobserver.exe`. `just check-repo`'s `platform-facts` holds both to
    # the platform declaration.
    $PROGRAM = 'printobserver.exe'
    $NAME = 'printobserver'
    $OWNER = 'nickderobertis'
    $REPOSITORY = 'printobserver'
    $CHECKSUMS = 'SHA256SUMS'
    $SCRIPT = 'install.ps1'

    # Every failure says why it stopped and what to do next, and then stops
    # with `throw` rather than `exit`. The call at the end of this file catches
    # it: under `irm ... | iex` an `exit` would close the window the caller
    # typed the command into, taking both sentences with it, so there the
    # caller gets the prompt back; run as a file, it is the non-zero exit a job
    # reads. A route that fails silently is worse for the user in front of the
    # printer than one that is absent, and a route that says only that it
    # failed is barely better.
    function Stop-Install([string]$Why, [string]$Next) {
        [Console]::Error.WriteLine("${SCRIPT}: $Why")
        [Console]::Error.WriteLine("${SCRIPT}: $Next")
        throw "${SCRIPT}: $Why"
    }

    function Say([string]$Line) {
        [Console]::Error.WriteLine("${SCRIPT}: $Line")
    }

    if ($Help) {
        Say 'Usage: irm <the fetch URL> | iex'
        Say '       & ([scriptblock]::Create((irm <the fetch URL>))) [-Version vX.Y.Z] [-To DIR]'
        Say '  -Version  install that release rather than the newest one.'
        Say '  -To       install into DIR rather than into $env:LOCALAPPDATA\Programs\printobserver.'
        return
    }

    $releaseBase = $env:PRINTOBSERVER_RELEASE_BASE
    if (-not $releaseBase) {
        $releaseBase = "https://github.com/$OWNER/$REPOSITORY/releases"
    }
    # A directory is what a test, a mirror on a shared filesystem and an
    # air-gapped install all look like; everything else is fetched.
    $localBase = $releaseBase
    if ($localBase.StartsWith('file://')) {
        $localBase = $localBase.Substring(7)
    }
    $isLocal = Test-Path -LiteralPath $localBase -PathType Container
    if ($isLocal) {
        $releaseBase = $localBase
    }

    # The platform, as the release artifacts name it — read the way Windows
    # itself names the host: `OS` is `Windows_NT` on every Windows, and the
    # processor is what the system reports rather than what this process runs
    # as, because an emulated PowerShell on an ARM64 machine still installs on
    # an ARM64 machine. A platform this release publishes nothing for is a stop
    # rather than a guess: installing a program built for another machine is a
    # failure the user meets at the printer.
    if ($env:OS -ne 'Windows_NT') {
        $described = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
        Stop-Install "this is $described, and $SCRIPT is the Windows form of the install script" `
            "On Linux and macOS run the shell form instead: curl -fsSL https://raw.githubusercontent.com/$OWNER/$REPOSITORY/main/scripts/install.sh | sh"
    }
    $machine = $env:PROCESSOR_ARCHITECTURE
    if ($env:PROCESSOR_ARCHITEW6432) {
        $machine = $env:PROCESSOR_ARCHITEW6432
    }
    switch ("$machine") {
        'AMD64' { $platform = 'windows-x86_64' }
        'ARM64' { $platform = 'windows-aarch64' }
        default {
            Stop-Install "this is windows/$machine, which printobserver publishes no program for" `
                "The Windows platforms it publishes for are windows-x86_64 and windows-aarch64. On anything else, build it from source with ``cargo install printobserver``."
        }
    }

    $asset = "$NAME-$platform.tar.gz"
    if ($Version -and $Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+([-+.][0-9A-Za-z.]+)*$') {
        Stop-Install "-Version was given ``$Version``, which is not a release tag" `
            'Pass it a tag of the form v0.1.0, or leave it out for the newest release.'
    }
    if ($Version) {
        $from = "$releaseBase/download/$Version"
        $which = "release $Version"
    } else {
        $from = "$releaseBase/latest/download"
        $which = 'the newest release'
    }

    if (-not $To) {
        if (-not $env:LOCALAPPDATA) {
            Stop-Install 'this session has no LOCALAPPDATA, so there is no default directory to install into' `
                'Pass -To a directory you can write to.'
        }
        $To = Join-Path (Join-Path $env:LOCALAPPDATA 'Programs') 'printobserver'
    }

    # A directory of this script's own, removed whatever happens below: nothing
    # reaches the directory the caller asked for until it has been verified.
    $work = Join-Path ([System.IO.Path]::GetTempPath()) ("printobserver-install-" + [System.Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work -Force | Out-Null
    try {
        # One way of obtaining a file, whichever kind of place the release is
        # in: copied out of a directory, fetched from anywhere else. Answers
        # nothing when the file is in hand, and otherwise why it is not, in the
        # words of whatever refused — so the stop can carry the exact cause.
        function Get-Published([string]$FileName, [string]$Into) {
            if ($isLocal) {
                $source = Join-Path $from $FileName
                if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
                    return "$source is not there"
                }
                Copy-Item -LiteralPath $source -Destination $Into
                return ''
            }
            try {
                if ([System.Net.ServicePointManager]::SecurityProtocol -ne [System.Net.SecurityProtocolType]::SystemDefault) {
                    [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor [System.Net.SecurityProtocolType]::Tls12
                }
                Invoke-WebRequest -Uri "$from/$FileName" -OutFile $Into -UseBasicParsing
                return ''
            } catch {
                return "$($_.Exception.Message)".Trim()
            }
        }

        $refused = Get-Published $asset (Join-Path $work $asset)
        if ($refused) {
            Stop-Install "$which has no $asset to download from $from ($refused)" `
                "Check that $which publishes a program for $platform and that $from is reachable from here, or pass -Version a release that does."
        }
        $refused = Get-Published $CHECKSUMS (Join-Path $work $CHECKSUMS)
        if ($refused) {
            Stop-Install "$which publishes no $CHECKSUMS, so what was downloaded cannot be verified ($refused)" `
                "Nothing was installed. Check that $from serves $CHECKSUMS beside its artifacts."
        }

        # What the release says the artifact's digest is, and what it actually
        # is. A mismatch stops here, with the download still in this script's
        # own directory and nothing on any path.
        $expected = ''
        foreach ($line in Get-Content -LiteralPath (Join-Path $work $CHECKSUMS)) {
            $fields = -split $line
            if ($fields.Count -ge 2 -and ($fields[1] -eq $asset -or $fields[1] -eq "*$asset")) {
                $expected = $fields[0]
                break
            }
        }
        if (-not $expected) {
            Stop-Install "$CHECKSUMS names no digest for $asset, so what was downloaded cannot be verified" `
                "Nothing was installed. Check that $which published $asset and its digest together."
        }
        $actual = (Get-FileHash -LiteralPath (Join-Path $work $asset) -Algorithm SHA256).Hash
        if ($actual.ToLowerInvariant() -ne $expected.ToLowerInvariant()) {
            Stop-Install "$asset does not match the digest $which published for it (expected $expected, got $($actual.ToLowerInvariant()))" `
                'Nothing was installed. The download was altered or is incomplete; try again, and if it happens twice report it rather than installing it.'
        }

        # The same archive every platform's route unpacks, and Windows has
        # carried a `tar` that reads it since Windows 10 — the system's own,
        # taken by its path where it is there, because another `tar` ahead of
        # it on PATH (Git's, on a runner) reads `C:\...` as a host to connect
        # to rather than a drive.
        $tar = ''
        if ($env:SystemRoot) {
            $own = Join-Path (Join-Path $env:SystemRoot 'System32') 'tar.exe'
            if (Test-Path -LiteralPath $own -PathType Leaf) {
                $tar = $own
            }
        }
        if (-not $tar) {
            $found = Get-Command tar -ErrorAction SilentlyContinue
            if ($found) {
                $tar = $found.Source
            }
        }
        if (-not $tar) {
            Stop-Install 'this machine has no tar, so nothing here can unpack what it downloaded' `
                "Nothing was installed. Windows 10 and later carry one at C:\Windows\System32\tar.exe; put it on your PATH, or unpack $asset by hand and verify it against $from/$CHECKSUMS."
        }
        # What is in it, before any of it reaches the filesystem: a member
        # naming a place outside the directory of this script's own, or one
        # that is not a plain file — a link would land wherever it points —
        # is not one this installs, whatever the digest said. The verbose
        # listing's first character is the member's kind, `-` for a file.
        $members = @(& $tar -tzf (Join-Path $work $asset) 2>$null)
        if ($LASTEXITCODE -ne 0) {
            Stop-Install "$asset could not be unpacked" `
                'Nothing was installed. The download may be incomplete; try again.'
        }
        $kinds = @(& $tar -tvzf (Join-Path $work $asset) 2>$null)
        # `$entry` rather than `$name`: PowerShell's variables are
        # case-insensitive, so `$name` here would be `$NAME` above.
        foreach ($member in $members) {
            $entry = "$member".Trim()
            if ($entry -match '^([A-Za-z]:|[\\/])' -or ($entry -split '[\\/]') -contains '..') {
                Stop-Install "$asset carries ``$entry``, which names a place outside where it is unpacked" `
                    "Nothing was installed. Report this against ${which}: the artifact is not the one this script installs."
            }
        }
        foreach ($kind in $kinds) {
            if (-not ("$kind".TrimStart().StartsWith('-'))) {
                Stop-Install "$asset carries ``$("$kind".Trim())``, which is not a plain file" `
                    "Nothing was installed. Report this against ${which}: the artifact is not the one this script installs."
            }
        }
        & $tar -xzf (Join-Path $work $asset) -C $work
        if ($LASTEXITCODE -ne 0) {
            Stop-Install "$asset could not be unpacked" `
                'Nothing was installed. The download may be incomplete; try again.'
        }
        $unpacked = Join-Path $work $PROGRAM
        if (-not (Test-Path -LiteralPath $unpacked -PathType Leaf)) {
            Stop-Install "$asset carries no $PROGRAM program" `
                "Nothing was installed. Report this against ${which}: the artifact is not the one this script installs."
        }

        try {
            New-Item -ItemType Directory -Path $To -Force | Out-Null
        } catch {
            Stop-Install "$To could not be created" 'Nothing was installed. Pass -To a directory you can write to.'
        }
        $installed = Join-Path $To $PROGRAM
        try {
            Move-Item -LiteralPath $unpacked -Destination $installed -Force
        } catch {
            Stop-Install "$PROGRAM could not be put in $To" 'Nothing was installed. Pass -To a directory you can write to.'
        }
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }

    # On the path of this session, and of every session after it: Windows has
    # no profile a shell reads a directory onto its path from, so the user's
    # own PATH is where a directory is put for the next window. The session's
    # copy is what lets `printobserver --version` be typed next, here.
    $separator = [System.IO.Path]::PathSeparator
    function Test-OnPath([string]$Listed) {
        $wanted = $To.TrimEnd('\', '/')
        foreach ($entry in "$Listed".Split($separator)) {
            if ($entry.TrimEnd('\', '/') -eq $wanted) {
                return $true
            }
        }
        return $false
    }
    $added = $false
    if (-not (Test-OnPath $env:Path)) {
        $env:Path = "$To$separator$env:Path"
        $persistent = [System.Environment]::GetEnvironmentVariable('Path', 'User')
        if (-not (Test-OnPath $persistent)) {
            $joined = if ($persistent) { "$persistent$separator$To" } else { $To }
            [System.Environment]::SetEnvironmentVariable('Path', $joined, 'User')
        }
        $added = $true
    }

    # What a successful install says, and all of it: where the program went
    # and from which release, that its directory is now on the path where it
    # was not, and the two commands the operator runs next — the same lines,
    # line for line, that the shell form of this route prints.
    # llmlint: ignore[tool_output_is_signal] suppressions.toml has the reason.
    function Report-Installed {
        Say "installed $installed from $which"
        if ($added) {
            Say "$To was added to your PATH; open a new PowerShell for other windows to see it, or run $installed by its whole name there."
        }
        Say 'next, in an elevated PowerShell, put the service in place and then start it:'
        [Console]::Error.WriteLine("  irm https://raw.githubusercontent.com/$OWNER/$REPOSITORY/main/scripts/install-service.ps1 | iex")
        [Console]::Error.WriteLine("  Set-Service -Name $NAME -StartupType Automatic -Status Running")
    }
    Report-Installed
}

# The status a caller reads. Run as a file, it is the process's own exit
# status. Under `irm ... | iex` there is no process of the script's own to
# exit — an `exit` here would close the caller's window — so the status goes
# where a session reads a program's: `$LASTEXITCODE`, which is what a caller
# checks after a native command, and what a CI step's PowerShell reads back.
try {
    Install-Printobserver -Version $Version -To $To -Help ([bool]$Help)
    $global:LASTEXITCODE = 0
} catch {
    # A stop of this script's own has already said why and what to do next;
    # anything else is a failure this script did not foresee, said in the
    # host's own words with the one next action there is for it.
    if ("$_" -notlike 'install.ps1: *') {
        [Console]::Error.WriteLine("install.ps1: stopped on an error it did not expect: $_")
        [Console]::Error.WriteLine("install.ps1: Nothing was installed unless a line above says so. Report this, with the lines above, at https://github.com/nickderobertis/printobserver/issues.")
    }
    if ($MyInvocation.MyCommand.Path) {
        exit 1
    }
    $global:LASTEXITCODE = 1
    return
}
