# Put the printobserver program on your path, already built for your platform,
# and run its server as you - the recommended install on Windows.
#
# The Windows form of the third of the three ALTERNATIVE routes AGENTS.md's "The
# end-user install path" section states - `scripts/install.sh` is the same route
# on Linux and macOS - and `just check-repo` refuses a tree in which this path
# differs from what that section's fetch URL names. It is the route for a
# machine that has neither package manager: nothing here needs Python, Node or
# a Rust toolchain, and nothing here compiles anything, because the host this
# runs on is the small machine beside the printer, which is the worst place to
# build a Rust workspace.
#
# It obtains the program exactly as the shell script does elsewhere, against
# the same artifact and the same checksum file. It FAILS CLOSED: where it cannot
# work out the platform, cannot download, or cannot verify what it downloaded,
# it stops without installing and says both why and what to do next.
# Verification happens before anything reaches a path: the archive is unpacked
# into a directory of this script's own, checked against the release's own
# checksum file, and only then moved to where you asked for it.
#
# With no switch it is also the installer of the `windows-scheduled-task` pair
# that section states: the per-user install, with no elevation at any point.
# Beside the program it writes your own server configuration under
# %LOCALAPPDATA%\printobserver - only when none is there - and its state
# directory, and registers a Scheduled Task, as you, that runs the server at
# your logon and brings it back when it stops, and starts it. Unlike every
# service installer this repository ships, it starts the server by design: it
# installs nothing for anybody but the person running it, and running it is the
# decision to supervise. Run again, it upgrades in place: it stops the task,
# waits for the server to exit, replaces the program, leaves the configuration
# and the state as they are, and starts the task again.
#
# Usage:
#   irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.ps1 | iex
#   & ([scriptblock]::Create((irm <that URL>))) [-Version vX.Y.Z] [-To DIR] [-ClientOnly] [-ObservationIntervalSeconds N]
#
#   -Version  install that release rather than the newest one.
#   -To       install into DIR rather than into $env:LOCALAPPDATA\Programs\printobserver.
#   -ClientOnly
#             put the program on your path and nothing else, for a host whose
#             server is the service `scripts/install-service.ps1` installs.
#   -ObservationIntervalSeconds
#             how often every active print is observed, written into a new
#             configuration as supervisor.observation_interval_s, exactly as
#             the service installer writes it.
#
# PRINTOBSERVER_RELEASE_BASE points this at somewhere other than the project's
# own releases - a mirror, or a directory holding a release. It takes the same
# two shapes the forge serves: `<base>/latest/download/<asset>` for the newest
# release and `<base>/download/<tag>/<asset>` for a pinned one.
# PRINTOBSERVER_TASK_NAME names the task something other than `printobserver`,
# which is how a test registers a task of its own beside a real one.
param(
    [string]$Version = '',
    [string]$To = '',
    [switch]$ClientOnly,
    [string]$ObservationIntervalSeconds = '',
    [switch]$Help
)

# The whole route is one function, called once at the end, so that what it sets
# - the strict mode, the error preference, the progress preference - is scoped
# to it and leaves the session of a caller who ran `irm ... | iex` as it was.
function Install-Printobserver([string]$Version, [string]$To, [bool]$ClientOnly, [string]$ObservationIntervalSeconds, [bool]$Help) {
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
        Say '       & ([scriptblock]::Create((irm <the fetch URL>))) [-Version vX.Y.Z] [-To DIR] [-ClientOnly] [-ObservationIntervalSeconds N]'
        Say '  -Version  install that release rather than the newest one.'
        Say '  -To       install into DIR rather than into $env:LOCALAPPDATA\Programs\printobserver.'
        Say '  -ClientOnly  put the program on your path and nothing else, for a host whose server is the service.'
        Say '  -ObservationIntervalSeconds N  how often a new configuration has every active print observed.'
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

    # The platform, as the release artifacts name it - read the way Windows
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

    # The per-user install's three places, under the user's own local
    # application data, spelled once: `crates/printobserver/src/locations.rs`
    # holds the program's own copy of them to what is written here. Every one
    # keeps the profile's own permissions: nothing here grants anything.
    $PerUserConfig = @('printobserver', 'config.toml')
    $PerUserState = @('printobserver', 'state')
    $PerUserProgram = @('Programs', 'printobserver')
    # The task the per-user server runs under, in the root task folder, and the
    # service whose presence means this host's server is not the user's own.
    $TaskName = 'printobserver'
    $ServiceName = 'printobserver'
    $TaskFolder = '\'

    function Under-LocalAppData([string[]]$Segments) {
        $path = $env:LOCALAPPDATA
        foreach ($segment in $Segments) {
            $path = Join-Path $path $segment
        }
        return $path
    }

    if (-not $To) {
        if (-not $env:LOCALAPPDATA) {
            Stop-Install 'this session has no LOCALAPPDATA, so there is no default directory to install into' `
                'Pass -To a directory you can write to.'
        }
        $To = Under-LocalAppData $PerUserProgram
    }

    if ($ClientOnly -and $ObservationIntervalSeconds) {
        Stop-Install '-ObservationIntervalSeconds was given with -ClientOnly, which installs no server for it to configure' `
            'Leave out -ClientOnly to install the server as you, or leave out -ObservationIntervalSeconds.'
    }
    $installed = Join-Path $To $PROGRAM
    if (-not $ClientOnly) {
        # Everything the per-user install will write is decided, and every
        # refusal it can make is made, before anything is fetched: a refused
        # run changes nothing on disk and nothing in Task Scheduler.
        if ($env:PRINTOBSERVER_TASK_NAME) {
            $TaskName = $env:PRINTOBSERVER_TASK_NAME
        }
        if ($TaskName -notmatch '^[A-Za-z0-9._-]+$') {
            Stop-Install "PRINTOBSERVER_TASK_NAME names the task ``$TaskName``, which is not a name this script registers" `
                'Name it with letters, digits, dots, dashes and underscores alone, or leave PRINTOBSERVER_TASK_NAME unset.'
        }
        # The server's `supervisor.observation_interval_s`, held to what the
        # server accepts exactly as the service installer holds it.
        $Interval = '# observation_interval_s = 120'
        if ($ObservationIntervalSeconds) {
            $seconds = [long]0
            if (-not ($ObservationIntervalSeconds -cmatch '^[1-9][0-9]*$' -and
                    [long]::TryParse($ObservationIntervalSeconds, [ref]$seconds))) {
                Stop-Install "-ObservationIntervalSeconds takes a whole number of seconds above zero, written in digits alone, and '$ObservationIntervalSeconds' is not one" `
                    "Nothing was installed. Pass a positive number of seconds, or leave -ObservationIntervalSeconds out for the server's default."
            }
            $Interval = "observation_interval_s = $seconds"
        }
        if (-not $env:LOCALAPPDATA) {
            Stop-Install "this session has no LOCALAPPDATA, so there is nowhere of your own to keep the server's configuration and state" `
                'Nothing was installed. Run this as the user the server is to run as, or pass -ClientOnly to install the program alone.'
        }
        $account = ''
        try {
            $account = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
        } catch {
            if ($env:USERNAME) {
                $account = if ($env:USERDOMAIN) { "$env:USERDOMAIN\$env:USERNAME" } else { $env:USERNAME }
            }
        }
        if (-not $account) {
            Stop-Install 'this session names no user, so there is nobody to register the server''s task for' `
                'Nothing was installed. Run this as the user the server is to run as, in a session of their own.'
        }
        $InstalledConfig = Under-LocalAppData $PerUserConfig
        $InstalledState = Under-LocalAppData $PerUserState
        $InstalledSkills = Join-Path $InstalledState 'skills'
        $InstalledSkill = Join-Path (Join-Path $InstalledSkills 'printobserver') 'SKILL.md'
        # Written into a TOML document as literal strings, which have no escape
        # for a single quote, and into the task's command line, in which a
        # double quote ends a path.
        foreach ($written in @($InstalledConfig, $InstalledState, $installed)) {
            if ($written.Contains('"') -or $written.Contains("'") -or $written.Contains("`n") -or $written.Contains("`r")) {
                Stop-Install "$written carries a quote or a newline, which the server's configuration and its task's command line have no escape for" `
                    'Nothing was installed. Pass -To a directory with no quote in its name, or run this as a user whose profile has none.'
            }
        }

        # A host whose server is the service is not one a second, per-user
        # server belongs on: both would supervise the one printer. The manager
        # answers a query about a service it has with success, about one it
        # does not have with 1060, and anything else is no answer at all.
        $ServiceDoesNotExist = 1060
        $asked = & {
            $ErrorActionPreference = 'Continue'
            sc.exe query $ServiceName 2>&1 | Out-String
        }
        $answered = $LASTEXITCODE
        if ($answered -eq 0) {
            Stop-Install "the $ServiceName service is installed on this machine, and a server of your own beside it would supervise the same printer twice" `
                "Nothing was changed. Move the service's configuration and record to your own account with the one elevated step, from an elevated PowerShell: & ([scriptblock]::Create((irm https://raw.githubusercontent.com/$OWNER/$REPOSITORY/main/scripts/migrate-service-to-per-user.ps1))) -LocalAppData '$env:LOCALAPPDATA' -User '$account' - and then run this again. Or keep the service, and pass -ClientOnly to install the program alone."
        }
        if ($answered -ne $ServiceDoesNotExist) {
            Stop-Install "asking the service control manager whether the $ServiceName service is installed failed: $("$asked".Trim())" `
                "Nothing was installed. Run ``sc.exe query $ServiceName`` to see what it says, and run this again once it answers."
        }
    }

    # Every process running the program at one path: the server the task
    # started, which the upgrade waits out. Read through each process's own
    # path, which a process of another user's answers none for.
    function Get-Running([string]$Path) {
        return @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
                $running = $null
                try { $running = $_.Path } catch { $running = $null }
                $running -and [string]::Equals($running, $Path, [System.StringComparison]::OrdinalIgnoreCase)
            })
    }

    # Put a task held off for an upgrade back as it was, where the upgrade
    # stopped before it replaced anything.
    function Resume-Task([bool]$Paused) {
        if ($Paused) {
            Enable-ScheduledTask -TaskName $TaskName -TaskPath $TaskFolder -ErrorAction SilentlyContinue | Out-Null
            Start-ScheduledTask -TaskName $TaskName -TaskPath $TaskFolder -ErrorAction SilentlyContinue
        }
    }

    # The task, as the document Task Scheduler reads: the installing user's own,
    # run only while they are logged on, with their limited token. It runs the
    # server at their logon, and every minute after it is registered starts it
    # again if it is not running - which is what brings the server back after
    # it exits, however it exited: the restart-on-failure setting alone covers
    # a start that failed, not a server that ran and then ended. One instance
    # at a time, so the minute's start of a running server does nothing; no
    # time limit; and the same on battery power as on mains.
    function Get-TaskDefinition([string]$Account, [string]$Program, [string]$Configuration) {
        $user = [System.Security.SecurityElement]::Escape($Account)
        $command = [System.Security.SecurityElement]::Escape("`"$Program`"")
        $arguments = [System.Security.SecurityElement]::Escape("server --config `"$Configuration`"")
        $registered = (Get-Date).ToString('yyyy-MM-ddTHH:mm:ss')
        return @"
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>printobserver, a supervision layer between a 3D printer and an agent, run as $user</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>$user</UserId>
    </LogonTrigger>
    <TimeTrigger>
      <Enabled>true</Enabled>
      <StartBoundary>$registered</StartBoundary>
      <Repetition>
        <Interval>PT1M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>$user</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>999</Count>
    </RestartOnFailure>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>$command</Command>
      <Arguments>$arguments</Arguments>
    </Exec>
  </Actions>
</Task>
"@
    }

    # A directory of this script's own, removed whatever happens below: nothing
    # reaches the directory the caller asked for until it has been verified.
    $work = Join-Path ([System.IO.Path]::GetTempPath()) ("printobserver-install-" + [System.Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work -Force | Out-Null
    try {
        # One way of obtaining a file, whichever kind of place the release is
        # in: copied out of a directory, fetched from anywhere else. Answers
        # nothing when the file is in hand, and otherwise why it is not, in the
        # words of whatever refused - so the stop can carry the exact cause.
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
        # carried a `tar` that reads it since Windows 10 - the system's own,
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
        # that is not a plain file - a link would land wherever it points -
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
        # An upgrade: the task is held off and stopped, and the server it ran
        # waited out, before its program is replaced - a running program's file
        # cannot be replaced on Windows. Held off first, because the task
        # starts the server again on its own while it is enabled.
        $paused = $false
        if (-not $ClientOnly) {
            if (Get-ScheduledTask -TaskName $TaskName -TaskPath $TaskFolder -ErrorAction SilentlyContinue) {
                Disable-ScheduledTask -TaskName $TaskName -TaskPath $TaskFolder | Out-Null
                Stop-ScheduledTask -TaskName $TaskName -TaskPath $TaskFolder
                $paused = $true
            }
            $deadline = (Get-Date).AddSeconds(60)
            while (@(Get-Running $installed).Count -gt 0) {
                if ((Get-Date) -gt $deadline) {
                    Resume-Task $paused
                    Stop-Install "the server $installed runs is still running a minute after its task was stopped" `
                        "Nothing was replaced. Stop it - Stop-Process -Name $NAME - and run this again."
                }
                Start-Sleep -Milliseconds 250
            }
        }
        try {
            Move-Item -LiteralPath $unpacked -Destination $installed -Force
        } catch {
            Resume-Task $paused
            Stop-Install "$PROGRAM could not be put in $To" 'Nothing was installed. Pass -To a directory you can write to.'
        }
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }

    # The per-user server: its configuration, only where none is there, its
    # state directory, and the task that runs it - registered as the user
    # running this, which needs no elevation, and started.
    $keptConfig = $false
    if (-not $ClientOnly) {
        $keptConfig = Test-Path -LiteralPath $InstalledConfig -PathType Leaf
        foreach ($directory in @((Split-Path $InstalledConfig), $InstalledState)) {
            try {
                New-Item -ItemType Directory -Path $directory -Force | Out-Null
            } catch {
                Stop-Install "$directory could not be created" `
                    "The program is installed at $installed, and its server is not. Make $directory writable by you, and run this again."
            }
        }
        if (-not $keptConfig) {
        $configuration = @"
# printobserver's one configuration file.
#
# Every value below is validated when the service starts, and a value that
# cannot work is refused naming this file's own field for it. Fill in the three
# marked FILL IN, then restart the server's task.

state_dir = '$InstalledState'
listen = "127.0.0.1:8420"

[octoprint]
# FILL IN: where your OctoPrint answers, and the API key it authenticates by.
url = "http://127.0.0.1:5000"
api_key = ""
# "commandable" if the machine has a part-cooling fan this service may command,
# "absent" if it has none.
fan = "commandable"

[supervisor]
# The harness identity supervision turns run on.
harness = "claude-code"
# The agent's skill, which this program does not carry. Install it here, as
# yourself and with no elevation:
#   gh skill install nickderobertis/printobserver printobserver --dir $InstalledSkills
skill_path = '$InstalledSkill'
# How often every active print is looked in on, and its agent given a turn,
# whether or not the detector has alerted. Left out, every 120 seconds: an
# eight-hour print is 240 turns. Raise it to spend less; zero is refused.
$Interval

[ingress]
# FILL IN: the shared secret Obico's webhook notification plugin must carry.
# Anything that can post to the ingress can pause a printer.
shared_secret = ""
# The answer bound is left out on purpose. The default this program ships is
# already below the timeout Obico posts under, and writing that number here
# would be a second copy of it to keep right; see AGENTS.md, "The Obico ingress
# answer bound".

# The operator's credential is never written in this file: the service keeps
# only its verifier, so nothing a supervision turn can read authenticates as the
# operator. As yourself, run
#   printobserver credential issue
# which writes the credential into your own client configuration under
# %APPDATA%, and prints one line to put here, above the first [table] header:
#   api.credential_verifier = "sha256:..."
# then restart the server's task. Until that line is here the server supervises and
# refuses every operator request. Each supervision turn is handed a credential
# of its own in its environment, so nothing else is needed for the agent.

# Optional: a fresh look at the print. A URL answering one still image, such as
# go2rtc's frame.jpeg for the printer's camera. Left out, a supervising agent's
# looks carry the printer's state but no frame.
# [camera]
# snapshot_url = "http://127.0.0.1:1984/api/frame.jpeg?src=camera"

# Optional: Obico's own API, so that a print Obico paused, and the agent adjusted
# and the supervisor resumed, is acknowledged to Obico, which re-arms its
# detection for the rest of the print. Give both keys or neither; the token is
# an OAuth2 access token from the Obico instance's own administration.
# [obico]
# url = "http://127.0.0.1:3334"
# access_token = ""

# How a start checks the spools loaded against what its file needs, by
# OctoPrint's own analysis of it. Every key is optional; left out, a start is
# refused while the spool on a tool holds less than the print needs plus 10%
# (and at least 5 g), while its figure is unverified, or while OctoPrint has
# no analysis of the file after waiting 300 seconds for one. "warn" lets such
# a start go ahead and records that it warned; "off" checks nothing. An
# operator may start one anyway with "--override-filament-check true".
# [spools]
# preflight = "refuse"
# buffer_percent = 10.0
# buffer_min_g = 5.0
# analysis_wait_seconds = 300

# What any actor may ask for at all. A manifest may narrow these; nothing may
# widen them. The system resumes a print the detector paused once the agent's
# adjustment to it is in, and puts back a bounded adjustment when it expires.
[safety]
agent_min_interval_s = 30

[safety.allowed]
feedrate = { min = 0.5, max = 1.5 }
flowrate = { min = 0.9, max = 1.1 }
fan = { min = 0.0, max = 100.0 }
bed_target = { min = 0.0, max = 110.0 }
"tool_target:0" = { min = 0.0, max = 260.0 }

# Who may ask for what. The spool commands (registering, loading,
# unloading, correcting and retiring a spool) and closing a print record left
# open are the operator's, and the agent is granted none of them: a
# configuration granting the agent close_print is refused at start.
[safety.actions]
operator = ["pause", "resume", "cancel", "start_print", "set_feedrate_factor",
            "set_flowrate_factor", "set_tool_target_c", "set_bed_target_c",
            "set_fan_percent", "acknowledge_failure", "close_print",
            "register_spool", "load_spool", "unload_spool", "adjust_spool",
            "retire_spool"]
agent = ["pause", "set_feedrate_factor", "set_flowrate_factor",
         "set_fan_percent", "acknowledge_failure"]
system = ["resume", "set_feedrate_factor", "set_flowrate_factor",
          "set_tool_target_c", "set_bed_target_c", "set_fan_percent"]
"@
            try {
                [IO.File]::WriteAllText($InstalledConfig, $configuration.Replace("`r`n", "`n"))
            } catch {
                Stop-Install "$InstalledConfig could not be written" `
                    "The program is installed at $installed, and its server is not. Make $(Split-Path $InstalledConfig) writable by you, and run this again."
            }
        }
        $definition = Get-TaskDefinition $account $installed $InstalledConfig
        try {
            Register-ScheduledTask -TaskName $TaskName -TaskPath $TaskFolder -Xml $definition -Force | Out-Null
            Start-ScheduledTask -TaskName $TaskName -TaskPath $TaskFolder
        } catch {
            Stop-Install "Task Scheduler would not register or start the task $TaskName for ${account}: $("$_".Trim())" `
                "The program is installed at $installed and its configuration at $InstalledConfig. Run this again as $account, from a session of your own."
        }
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
    # was not, and the two commands the operator runs next - the same lines,
    # line for line, that the shell form of this route prints.
    # llmlint: ignore[tool_output_is_signal] suppressions.toml has the reason.
    function Report-Installed {
        Say "installed $installed from $which"
        if ($added) {
            Say "$To was added to your PATH; open a new PowerShell for other windows to see it, or run $installed by its whole name there."
        }
        if ($ClientOnly) {
            Say 'next, in an elevated PowerShell, put the service in place and then start it:'
            [Console]::Error.WriteLine("  irm https://raw.githubusercontent.com/$OWNER/$REPOSITORY/main/scripts/install-service.ps1 | iex")
            [Console]::Error.WriteLine("  Set-Service -Name $NAME -StartupType Automatic -Status Running")
            return
        }
        if ($keptConfig) {
            Say "kept $InstalledConfig and $InstalledState as they were"
            if ($ObservationIntervalSeconds) {
                Say "so -ObservationIntervalSeconds was not written; set supervisor.observation_interval_s in $InstalledConfig yourself"
            }
        } else {
            Say "wrote $InstalledConfig; fill in the three values marked FILL IN"
        }
        Say "registered the task $TaskName, which runs the server as $account at your logon and brings it back when it stops, and started it"
        Say 'next, as yourself, in a PowerShell that is not elevated:'
        [Console]::Error.WriteLine("  printobserver credential issue")
        [Console]::Error.WriteLine("    and put the api.credential_verifier line it prints into $InstalledConfig, above its first [table] header")
        [Console]::Error.WriteLine("  gh skill install nickderobertis/printobserver printobserver --dir $InstalledSkills")
        [Console]::Error.WriteLine("  printobserver sign-in")
        [Console]::Error.WriteLine("  Stop-ScheduledTask -TaskName $TaskName; Start-ScheduledTask -TaskName $TaskName")
    }
    Report-Installed
}

# The status a caller reads. Run as a file, it is the process's own exit
# status. Under `irm ... | iex` there is no process of the script's own to
# exit - an `exit` here would close the caller's window - so the status goes
# where a session reads a program's: `$LASTEXITCODE`, which is what a caller
# checks after a native command, and what a CI step's PowerShell reads back.
try {
    Install-Printobserver -Version $Version -To $To -ClientOnly ([bool]$ClientOnly) -ObservationIntervalSeconds $ObservationIntervalSeconds -Help ([bool]$Help)
    $global:LASTEXITCODE = 0
} catch {
    # A stop of this script's own has already said why and what to do next;
    # anything else is a failure this script did not foresee, said in the
    # host's own words with the one next action there is for it.
    if ("$_" -notlike 'install.ps1: *') {
        [Console]::Error.WriteLine("install.ps1: stopped on an error it did not expect: $_")
        [Console]::Error.WriteLine("install.ps1: Nothing was installed unless a line above says so. Report this, with the lines above, at https://github.com/nickderobertis/printobserver/issues.")
    }
    # llmlint: ignore[cli_output_contract] suppressions.toml has the reason.
    if ($MyInvocation.MyCommand.Path) {
        exit 1
    }
    $global:LASTEXITCODE = 1
    return
}
