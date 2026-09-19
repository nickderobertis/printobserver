# Put the printobserver Windows service in place, and start nothing.
#
# The Windows form of the installer the end-user install path AGENTS.md's "The
# end-user install path" section states as the first of the `windows-service`
# pair; `just check-repo` refuses a tree in which this path, or the service's
# name below, differs from what that section says.
#
# It places FOUR things and no more: the program, the configuration, the state
# directory, and the service registration with the service control manager. It
# does NOT start the service and does NOT set it to start automatically, and it
# must never be changed to — nor given an option that does. This service
# commands a 3D printer, so installing a package must not, as a side effect,
# start a process that can move a machine — at install or at the next reboot.
# The registration is written to start on demand; setting it to start
# automatically and starting it is the operator's own second command, which this
# script prints when it is done.
#
# Usage, in an elevated PowerShell:
#   install-service.ps1 [-Root DIR] [-Binary PATH]
#
#   -Root     install beneath DIR rather than beneath the system drive. This is
#             how a test owns a throwaway root; the paths under it are the real
#             ones, and the service is registered naming them.
#   -Binary   the printobserver program to install. Defaults to the one on PATH,
#             which is what any of the install path's three routes puts there.
#
# The service runs as its own virtual account, `NT SERVICE\printobserver`, which
# Windows creates with the registration: no password, no profile, and no right
# to anything this script does not grant it.
param(
    [string]$Root = '',
    [string]$Binary = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# One way of handing arguments to a program, whichever PowerShell runs this.
# Windows PowerShell passes an argument's embedded quotes through unescaped,
# and a newer PowerShell escapes them for most programs; under `Legacy` both do
# the former, so the one command line below that carries embedded quotes is
# escaped by hand here and read the same way everywhere.
if (Test-Path variable:PSNativeCommandArgumentPassing) {
    $PSNativeCommandArgumentPassing = 'Legacy'
}

$ServiceName = 'printobserver'
$Program = 'printobserver.exe'

# The three places the Windows locations declare, spelled once; everything
# below writes into one of them. `crates/printobserver/src/locations.rs` holds
# the program's own copy of these three to what is written here.
$ProgramDirectory = 'C:\Program Files\printobserver'
$ConfigPath = 'C:\ProgramData\printobserver\config.toml'
$StateDirectory = 'C:\ProgramData\printobserver\state'

function Fail([string]$Message) {
    [Console]::Error.WriteLine("install-service.ps1: $Message")
    exit 1
}

# What a refusal of the host's own says, as one clause of a sentence here.
function Why($Error) {
    return "$Error".Trim().TrimEnd('.')
}

# What a reinstall found and kept, said once, in the one line printed at the
# end: a reader of that line learns what survived without a second line to
# find it in.
$Kept = @()

# Every value this script is given is written into a TOML document, in which a
# literal string has no escape for a single quote, and into the service's own
# command line, in which a double quote ends a path. So a value carrying either,
# or a newline, is refused here rather than registering a service that starts
# something else.
function Plain([string]$Name, [string]$Value) {
    if ($Value.Contains('"') -or $Value.Contains("'")) {
        Fail "$Name carries a quote, which a service's command line and a TOML document have no escape for. Pass $Name a path with no quote in it — move or rename the directory if its name has one."
    }
    if ($Value.Contains("`n") -or $Value.Contains("`r")) {
        Fail "$Name carries a newline, which a TOML document reads as the end of a setting. Pass $Name a path on one line."
    }
}

# One of the three places, beneath the throwaway root when one was given: the
# same path with the drive taken off the front. The separator is this host's
# own, so a root on a host that is not Windows holds real directories rather
# than names with backslashes in them.
function Under-Root([string]$Path) {
    # llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
    if ([string]::IsNullOrEmpty($Root)) {
        return $Path
    }
    $relative = ($Path -replace '^[A-Za-z]:\\', '') -replace '\\', [IO.Path]::DirectorySeparatorChar
    return Join-Path $Root $relative
}

# Run one program, and refuse to go on when it fails, naming what it said and
# what to do next.
#
# Its own error preference, because Windows PowerShell turns a program's
# standard error, read here so that it can be quoted back, into a terminating
# error under `Stop` — before the exit status that says whether it failed.
function Must([string]$What, [string]$Next, [scriptblock]$Command) {
    $ErrorActionPreference = 'Continue'
    $said = & $Command 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) {
        Fail "$What failed: $($said.Trim()). $Next"
    }
}

# The one next action every refusal of the service control manager or the
# access-control tool shares: both need an elevated PowerShell, and the manager
# says what it holds when asked.
$Elevate = "Run this from an elevated PowerShell; ``sc.exe query $ServiceName`` says what the service control manager holds."

if ($Root) { Plain '-Root' $Root }
if ($Binary) { Plain '-Binary' $Binary }

if (-not $Binary) {
    $found = Get-Command $ServiceName -CommandType Application -ErrorAction SilentlyContinue
    if ($found) { $Binary = $found.Source }
}
if (-not $Binary) {
    Fail "no $ServiceName program on PATH. Take one of the three routes AGENTS.md's install path states, or pass -Binary."
}
if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) {
    Fail "$Binary is not a file. Pass -Binary the program one of the install path's three routes put on your path."
}
Plain '-Binary' $Binary

$InstalledBinary = Join-Path (Under-Root $ProgramDirectory) $Program
$InstalledConfig = Under-Root $ConfigPath
$InstalledState = Under-Root $StateDirectory

foreach ($directory in @((Split-Path $InstalledBinary), (Split-Path $InstalledConfig), $InstalledState)) {
    try {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    } catch {
        Fail "$directory could not be created: $(Why $_). Run this from an elevated PowerShell, or pass -Root a directory you can write to."
    }
    if (-not (Test-Path -LiteralPath $directory -PathType Container)) {
        Fail "$directory could not be created: something that is not a directory is in its way. Pass -Root a directory of its own, or move what is there."
    }
}

try {
    Copy-Item -LiteralPath $Binary -Destination $InstalledBinary -Force
} catch {
    Fail "$Binary could not be copied to ${InstalledBinary}: $(Why $_). Run this from an elevated PowerShell, or pass -Root a directory you can write to."
}

# The registration: this program, started by the service control manager with
# the installed configuration, on demand and as its own virtual account. A
# reinstall over an existing registration updates what it runs and leaves its
# start type as the operator set it, exactly as a rewritten unit leaves a
# systemd enablement alone.
$binPath = "\`"$InstalledBinary\`" server --config \`"$InstalledConfig\`""
$account = "NT SERVICE\$ServiceName"
# Whether the manager already holds this service: it answers a query about one
# it has with success, about one it does not have with 1060, and anything else
# — access denied, a manager that is not running — is a refusal to go on over
# rather than evidence of either.
$ServiceDoesNotExist = 1060
$asked = sc.exe query $ServiceName 2>&1 | Out-String
if ($LASTEXITCODE -eq 0) {
    $Kept += "the registration of $ServiceName was updated and its start type left as set"
    Must "updating the service registration" $Elevate { sc.exe config $ServiceName binPath= $binPath obj= $account }
} elseif ($LASTEXITCODE -eq $ServiceDoesNotExist) {
    Must "registering the service" $Elevate {
        sc.exe create $ServiceName binPath= $binPath start= demand obj= $account DisplayName= $ServiceName
    }
} else {
    Fail "asking the service control manager about $ServiceName failed: $($asked.Trim()). $Elevate"
}
Must "describing the service" $Elevate {
    sc.exe description $ServiceName "printobserver, a supervision layer between a 3D printer and an agent"
}
# What the manager does when the process ends without having reported that it
# stopped: bring it back, five seconds later, however often that happens; a
# day without a failure starts the count again. This is the whole of what
# makes it a service rather than a program somebody has to restart.
Must "setting the service's failure actions" $Elevate {
    sc.exe failure $ServiceName reset= 86400 actions= restart/5000/restart/5000/restart/5000
}

# The state directory and the configuration are the service's own and readable
# by nothing else on the machine but the administrators who installed it: the
# directory holds the whole record of what a printer did and what an agent
# decided, and the configuration holds the printer's key and the ingress secret.
# Inheritance is cut so that nothing granted further up reaches in. The program
# directory is left readable as its parent is, and the service account is
# granted what it needs to run the program from it.
Must "making the state directory private" $Elevate {
    icacls $InstalledState /inheritance:r /grant:r "${account}:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-18:(OI)(CI)F"
}
Must "letting the service run its program" $Elevate {
    icacls (Split-Path $InstalledBinary) /grant "${account}:(OI)(CI)RX"
}

# An existing configuration is left exactly as it is: a reinstall must not
# overwrite the operator's own values with a template's.
if (Test-Path -LiteralPath $InstalledConfig -PathType Leaf) {
    $Kept += "$InstalledConfig was already there and was left alone"
} else {
    $configuration = @"
# printobserver's one configuration file.
#
# Every value below is validated when the service starts, and a value that
# cannot work is refused naming this file's own field for it. Fill in the three
# marked FILL IN before starting the service.

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

[ingress]
# FILL IN: the shared secret Obico's webhook notification plugin must carry.
# Anything that can post to the ingress can pause a printer.
shared_secret = ""
# The answer bound is left out on purpose. The default this program ships is
# already below the timeout Obico posts under, and writing that number here
# would be a second copy of it to keep right; see AGENTS.md, "The Obico ingress
# answer bound".

# The API credential every client presents is not written in this file. Left
# out, the service generates one the first time it starts, into the file
# api-credential in its state directory, readable by the service's account and
# the machine's administrators alone, and writes it with the address into
# client.toml beside it for the clients on this host. To choose the credential
# yourself, add an [api] table whose credential key holds a long random value;
# the generated file is then neither read nor written.

# What any actor may ask for at all. A manifest may narrow these; nothing may
# widen them.
[safety]
agent_min_interval_s = 30

[safety.allowed]
feedrate = { min = 0.5, max = 1.5 }
flowrate = { min = 0.9, max = 1.1 }
fan = { min = 0.0, max = 100.0 }
bed_target = { min = 0.0, max = 110.0 }
"tool_target:0" = { min = 0.0, max = 260.0 }

[safety.actions]
operator = ["pause", "resume", "cancel", "start_print", "set_feedrate_factor",
            "set_flowrate_factor", "set_tool_target_c", "set_bed_target_c",
            "set_fan_percent", "acknowledge_failure"]
agent = ["pause", "set_feedrate_factor", "set_flowrate_factor",
         "set_fan_percent", "acknowledge_failure"]
system = ["set_feedrate_factor", "set_flowrate_factor", "set_tool_target_c",
          "set_bed_target_c", "set_fan_percent"]
"@
    try {
        [IO.File]::WriteAllText($InstalledConfig, $configuration.Replace("`r`n", "`n"))
    # llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
    } catch {
        Fail "$InstalledConfig could not be written: $(Why $_). Run this from an elevated PowerShell, or pass -Root a directory you can write to."
    }
    Must "making the configuration private" $Elevate {
        icacls $InstalledConfig /inheritance:r /grant:r "${account}:R" "*S-1-5-32-544:F" "*S-1-5-18:F"
    }
}

$kept = if ($Kept.Count -gt 0) { " (" + ($Kept -join '; ') + ")" } else { '' }
[Console]::Error.WriteLine("install-service.ps1: installed $InstalledBinary, $InstalledConfig, $InstalledState and the service $ServiceName$kept, and started nothing; edit $InstalledConfig, then run: Set-Service -Name $ServiceName -StartupType Automatic -Status Running")
