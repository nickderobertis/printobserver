# Move a printobserver Windows service's configuration and record to a user's
# own per-user install: the one elevated step between the two.
#
# AGENTS.md's "The end-user install path" states the per-user install as the
# recommended one on Windows and the service as the isolated option; this is
# how a host already running the service moves to the per-user install without
# losing a print record, an image or a turn. It is the only step of that move
# that needs an elevated PowerShell, because the service's configuration and
# state are readable only by administrators and the service's own account.
#
# In order, and stopping without changing anything at the first refusal:
#   1. it stops the service and waits for its process to exit;
#   2. it copies the service's configuration and its whole state directory -
#      the record with its write-ahead files, every image, every turn record,
#      the harness sign-ins, the skill and the assets - under the user's own
#      local application data, owned by that user with the profile's own
#      inherited permissions;
#   3. it rewrites `state_dir` and `supervisor.skill_path` in the copied
#      configuration to the per-user ones, and leaves every other byte of it
#      as it was;
#   4. it verifies the copy - every file there, and every file's SHA-256 equal
#      to its original's - and only then deletes the service. A copy that does
#      not verify is removed again, the service is left registered, and the
#      service's own files are left exactly as they were.
# It never removes C:\ProgramData\printobserver or C:\Program Files\printobserver:
# it prints both, and how to remove them once you are satisfied.
#
# Usage, in an elevated PowerShell:
#   migrate-service-to-per-user.ps1 [-LocalAppData DIR] [-User DOMAIN\name] [-Root DIR]
#
#   -LocalAppData  the local application data of the user the per-user install
#                  is for. Defaults to this session's own.
#   -User          that user's account, who will own the copies. Defaults to
#                  the account running this. Elevating through UAC may run this
#                  as another administrator, so both name the person who is to
#                  run the server rather than whoever this runs as.
#   -Root          read the service's files beneath DIR rather than beneath the
#                  system drive, as `install-service.ps1 -Root` placed them.
#                  This is how a test owns a throwaway root.
param(
    [string]$LocalAppData = '',
    [string]$User = '',
    [string]$Root = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ServiceName = 'printobserver'
$Script = 'migrate-service-to-per-user.ps1'

# The service's places, as `install-service.ps1` writes them, and the per-user
# ones, as `install.ps1` writes them, relative to the user's local application
# data. `crates/printobserver/src/locations.rs` holds both to its own copies.
$ProgramDirectory = 'C:\Program Files\printobserver'
$ConfigPath = 'C:\ProgramData\printobserver\config.toml'
$StateDirectory = 'C:\ProgramData\printobserver\state'
$PerUserConfig = @('printobserver', 'config.toml')
$PerUserState = @('printobserver', 'state')

# What the manager answers a query about a service it does not have, and a stop
# of one that is not running.
$ServiceDoesNotExist = 1060
$ServiceNotActive = 1062

# Every refusal says why and what to do next, and exits non-zero. One made
# before the service is stopped changes nothing; one after says in its own
# words what it left stopped, copied or removed.
function Fail([string]$Why, [string]$Next) {
    [Console]::Error.WriteLine("${Script}: $Why")
    [Console]::Error.WriteLine("${Script}: $Next")
    exit 1
}

function Say([string]$Line) {
    [Console]::Error.WriteLine("${Script}: $Line")
}

# One of the service's places, beneath the throwaway root when one was given:
# the same path with the drive taken off the front, in this host's separators.
function Under-Root([string]$Path) {
    if ([string]::IsNullOrEmpty($Root)) {
        return $Path
    }
    $relative = ($Path -replace '^[A-Za-z]:\\', '') -replace '\\', [IO.Path]::DirectorySeparatorChar
    return Join-Path $Root $relative
}

function Under-LocalAppData([string[]]$Segments) {
    $path = $LocalAppData
    foreach ($segment in $Segments) {
        $path = Join-Path $path $segment
    }
    return $path
}

# One program, its exit status and what it said, with the error preference of
# its own that keeps Windows PowerShell from turning what it wrote to standard
# error into a terminating error before the status is read.
function Ask([scriptblock]$Command) {
    $ErrorActionPreference = 'Continue'
    $said = & $Command 2>&1 | Out-String
    return [pscustomobject]@{ Code = $LASTEXITCODE; Said = "$said".Trim() }
}

# Elevated is the token's integrity level, as Windows reports it in the groups
# of the token: high (`S-1-16-12288`) for an administrator who elevated, and
# system (`S-1-16-16384`) above it. A filtered administrator's token is medium.
$groups = Ask { whoami.exe /groups }
if ($groups.Code -ne 0 -or $groups.Said -notmatch 'S-1-16-(12288|16384)\b') {
    Fail 'this is not an elevated PowerShell, and the service''s configuration and record are readable only by administrators and the service' `
        'Nothing was changed. Run this again from an elevated PowerShell, passing -LocalAppData and -User for the account the per-user install is for.'
}

if (-not $LocalAppData) {
    $LocalAppData = "$env:LOCALAPPDATA"
}
if (-not $User) {
    try {
        $User = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    } catch {
        if ($env:USERNAME) {
            $User = if ($env:USERDOMAIN) { "$env:USERDOMAIN\$env:USERNAME" } else { $env:USERNAME }
        }
    }
}
if (-not $User) {
    Fail 'nothing names the account the per-user install is for' `
        'Nothing was changed. Pass -User DOMAIN\name for the account that will run the server.'
}
if (-not $LocalAppData -or -not (Test-Path -LiteralPath $LocalAppData -PathType Container)) {
    Fail "-LocalAppData names ``$LocalAppData``, which is not a directory" `
        "Nothing was changed. Pass -LocalAppData the local application data of $User, such as C:\Users\<name>\AppData\Local."
}
# Persist paths independently of the elevated shell's working directory.
$LocalAppData = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($LocalAppData)
# The account the copies will belong to, resolved before anything is stopped:
# a name Windows cannot resolve is refused here rather than by the owner-setter
# after the service is down. A host with no account database to ask - a test's
# own, under -Root - takes the name as it was given.
if ($User -notmatch '^[^\\/]+(\\[^\\/]+)?$') {
    Fail "-User is ``$User``, which is not an account name" `
        'Nothing was changed. Pass -User as DOMAIN\name or as a local account''s name.'
}
try {
    [void](New-Object System.Security.Principal.NTAccount($User)).Translate([System.Security.Principal.SecurityIdentifier])
} catch [System.PlatformNotSupportedException] {
} catch {
    Fail "-User names ``$User``, which is no account on this machine" `
        'Nothing was changed. Pass -User the account that will run the server, as DOMAIN\name.'
}
foreach ($given in @($LocalAppData, $User, $Root)) {
    if ($given.Contains("'") -or $given.Contains('"') -or $given.Contains("`n") -or $given.Contains("`r")) {
        Fail "``$given`` carries a quote or a newline, which the per-user configuration has no escape for" `
            'Nothing was changed. Pass a path and an account with neither in them.'
    }
}

$ServiceConfig = Under-Root $ConfigPath
$UserConfig = Under-LocalAppData $PerUserConfig
$UserState = Under-LocalAppData $PerUserState
$UserSkill = Join-Path (Join-Path (Join-Path $UserState 'skills') 'printobserver') 'SKILL.md'

$queried = Ask { sc.exe query $ServiceName }
if ($queried.Code -eq $ServiceDoesNotExist) {
    Fail "no $ServiceName service is registered on this machine, so there is nothing to move" `
        'Nothing was changed. Install the per-user server directly: irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.ps1 | iex'
}
if ($queried.Code -ne 0) {
    Fail "asking the service control manager about $ServiceName failed: $($queried.Said)" `
        "Nothing was changed. Run ``sc.exe query $ServiceName`` to see what it says."
}
if (Test-Path -LiteralPath $UserConfig) {
    Fail "$UserConfig is already there, so $User already has a per-user install this would overwrite" `
        "Nothing was changed. Move $UserConfig aside if it is not the one you want, and run this again."
}
if (Test-Path -LiteralPath $UserState) {
    Fail "$UserState is already there, so $User already has per-user state this would mix the service's into" `
        "Nothing was changed. Move $UserState aside if it holds nothing you want, and run this again."
}
if (-not (Test-Path -LiteralPath $ServiceConfig -PathType Leaf)) {
    Fail "$ServiceConfig is not there, so the service has no configuration to move" `
        'Nothing was changed. Run this on the machine the service was installed on.'
}

# The two lines this rewrites, and nothing else: `state_dir` before the first
# table, and `skill_path` in `[supervisor]`. Every other byte - comments, line
# endings, the operator's own values - is carried over as it was. Each is
# required to be one plain string, which is what the service installer writes,
# and only the characters between its quotes are replaced: the indentation, the
# spacing around `=`, the quote it was written in and an inline comment after
# it are kept exactly. The state directory moved is the one `state_dir` names,
# which is the record the service kept.
$PlainValue = '^(?<lead>\s*(state_dir|skill_path)\s*=\s*)(''(?<single>[^'']*)''|"(?<double>([^"\\]|\\\\)*)")(?<rest>\s*(#.*)?)$'

# `$Line` with its one plain string's value replaced by `$Path`, written in the
# quote the line already used; $null when `$Line` is not one plain string.
function Replace-Value([string]$Line, [string]$Path) {
    $found = [regex]::Match($Line, $PlainValue)
    if (-not $found.Success) {
        return $null
    }
    if ($found.Groups['single'].Success) {
        $value = $found.Groups['single'].Value
        $quoted = "'$Path'"
    } else {
        # A basic string's one escape a path needs: `\\` for a backslash.
        $value = $found.Groups['double'].Value.Replace('\\', '\')
        $quoted = '"' + $Path.Replace('\', '\\') + '"'
    }
    return [pscustomobject]@{
        Value = $value
        Line = $found.Groups['lead'].Value + $quoted + $found.Groups['rest'].Value
    }
}
$ServiceState = ''

try {
    $original = [IO.File]::ReadAllText($ServiceConfig)
} catch {
    Fail "$ServiceConfig could not be read: $("$_".Trim())" `
        'Nothing was changed. Run this from an elevated PowerShell, which the service''s configuration is readable from.'
}
$lines = [regex]::Split($original, '(?<=\n)')
$table = ''
$rewroteState = $false
$rewroteSkill = $false
$rewritten = New-Object System.Text.StringBuilder
foreach ($line in $lines) {
    $ending = ''
    $body = $line
    if ($body.EndsWith("`r`n")) {
        $ending = "`r`n"
    } elseif ($body.EndsWith("`n")) {
        $ending = "`n"
    }
    $body = $body.Substring(0, $body.Length - $ending.Length)
    if ($body -match '^\s*\[([^\]]+)\]\s*(#.*)?$') {
        $table = $Matches[1].Trim()
    } elseif ($table -eq '' -and $body -match '^\s*state_dir\s*=') {
        $replaced = Replace-Value $body $UserState
        if ($null -eq $replaced) {
            Fail "$ServiceConfig's state_dir is not one plain string, so this cannot tell which directory the record is in" `
                "Nothing was changed. Write state_dir as install-service.ps1 does, such as state_dir = '$StateDirectory', and run this again."
        }
        $ServiceState = $replaced.Value
        $body = $replaced.Line
        $rewroteState = $true
    } elseif ($table -eq 'supervisor' -and $body -match '^\s*skill_path\s*=') {
        $replaced = Replace-Value $body $UserSkill
        if ($null -eq $replaced) {
            Fail "$ServiceConfig's supervisor.skill_path is not one plain string" `
                'Nothing was changed. Write it as install-service.ps1 does, and run this again.'
        }
        $body = $replaced.Line
        $rewroteSkill = $true
    }
    [void]$rewritten.Append($body).Append($ending)
}
if (-not $rewroteState -or -not $rewroteSkill) {
    Fail "$ServiceConfig names no state_dir before its first table or no skill_path in [supervisor], so there is nothing this can point at the per-user state" `
        "Nothing was changed. Write both into $ServiceConfig as install-service.ps1 writes them, and run this again."
}
if (-not (Test-Path -LiteralPath $ServiceState -PathType Container)) {
    Fail "$ServiceState, which $ServiceConfig names as state_dir, is not there, so the service has no record to move" `
        'Nothing was changed. Run this on the machine the service was installed on.'
}
# The copy goes beside the record, never into it nor around it: a destination
# inside the state directory would be copied into itself, and one holding it
# would be the record's own directory rewritten.
$from = [IO.Path]::GetFullPath($ServiceState).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
$to = [IO.Path]::GetFullPath($UserState).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
if ($to.StartsWith($from, [StringComparison]::OrdinalIgnoreCase) -or $from.StartsWith($to, [StringComparison]::OrdinalIgnoreCase)) {
    Fail "$UserState and the service's record at $ServiceState overlap, so the copy would be made into or around the record it copies" `
        'Nothing was changed. Pass -LocalAppData the local application data of the user the install is for, which holds no part of the service''s state.'
}

# 1. The service stopped, and its process gone, before anything is read: a
# record copied while it is being written is not the record.
# llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
$stopped = Ask { sc.exe stop $ServiceName }
if ($stopped.Code -ne 0 -and $stopped.Code -ne $ServiceNotActive) {
    Fail "stopping the $ServiceName service failed: $($stopped.Said)" `
        "Nothing was copied. Stop it yourself with ``sc.exe stop $ServiceName``, and run this again."
}
$deadline = (Get-Date).AddSeconds(60)
while ($true) {
    $state = Ask { sc.exe queryex $ServiceName }
    if ($state.Said -match 'STATE\s*:\s*\d+\s+STOPPED' -and $state.Said -match 'PID\s*:\s*0\b') {
        break
    }
    if ((Get-Date) -gt $deadline) {
        Fail "the $ServiceName service has not stopped a minute after it was asked to: $($state.Said)" `
            "Nothing was copied. End it yourself with ``sc.exe queryex $ServiceName`` and ``taskkill /F /PID <pid>``, and run this again."
    }
    Start-Sleep -Milliseconds 250
}

# 2. The copies, owned by the user. Every directory and file is created new
# under the user's own profile, so each inherits that profile's permissions
# rather than the service's private ones.
$created = @()
# Removing what this copied before a refusal: what could not be removed is
# named, because an unverified copy left in place is one the next run refuses.
function Remove-Copies {
    foreach ($path in $created) {
        try {
            Remove-Item -LiteralPath $path -Recurse -Force
        } catch {
            # llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
            Say "$path, copied by this run, could not be removed ($("$_".Trim())); remove it before running this again"
        }
    }
}
try {
    $parent = Split-Path $UserConfig
    if (-not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
        $created += $parent
    }
    New-Item -ItemType Directory -Path $UserState -Force | Out-Null
    $created += $UserState
    foreach ($entry in Get-ChildItem -LiteralPath $ServiceState -Force) {
        Copy-Item -LiteralPath $entry.FullName -Destination $UserState -Recurse -Force
    }
    [IO.File]::WriteAllText($UserConfig, $rewritten.ToString())
    $created += $UserConfig
} catch {
    Remove-Copies
    Fail "copying the service's configuration and record failed: $("$_".Trim())" `
        "The service is stopped and still registered, and its files are as they were. Start it again with ``sc.exe start $ServiceName``, or free space under $LocalAppData and run this again."
}
# llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
$owned = Ask { icacls $parent /setowner $User /T /C }
if ($owned.Code -ne 0) {
    Remove-Copies
    Fail "making $User the owner of $parent failed: $($owned.Said)" `
        "The service is stopped and still registered, and its files are as they were. Check that $User is an account on this machine, and run this again."
}

# 3. Verified before anything is deleted: every file of the service's state is
# there in the copy with the same SHA-256, and the configuration is the
# original but for the two lines this rewrote.
$differences = @()
try {
    foreach ($file in Get-ChildItem -LiteralPath $ServiceState -Recurse -File -Force) {
        $relative = $file.FullName.Substring($ServiceState.Length).TrimStart('\', '/')
        $copy = Join-Path $UserState $relative
        if (-not (Test-Path -LiteralPath $copy -PathType Leaf)) {
            $differences += "$relative is missing"
            continue
        }
        $want = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
        $got = (Get-FileHash -LiteralPath $copy -Algorithm SHA256).Hash
        if ($want -ne $got) {
            $differences += "$relative differs"
        }
    }
    if ([IO.File]::ReadAllText($UserConfig) -ne $rewritten.ToString()) {
        $differences += "$UserConfig differs from what was written"
    }
} catch {
    $differences += "the copy could not be read back ($("$_".Trim()))"
}
if ($differences.Count -gt 0) {
    Remove-Copies
    Fail "the copy did not verify ($($differences -join '; ')), so the service was not deleted" `
        "The service is stopped and still registered, and its files are as they were. Start it again with ``sc.exe start $ServiceName``, and run this again once nothing is writing to $ServiceState."
}

$deleted = Ask { sc.exe delete $ServiceName }
if ($deleted.Code -ne 0) {
    Fail "deleting the $ServiceName service failed: $($deleted.Said)" `
        "The copies are in place under $LocalAppData. Delete the service yourself with ``sc.exe delete $ServiceName`` before installing the per-user server."
}

# What a migration that finished says, and all of it: one line of what moved
# where and what was left, then the commands the operator runs - the two next
# steps as themselves, and the removal once they are satisfied.
function Report-Migrated {
    $left = Under-Root 'C:\ProgramData\printobserver'
    $program = Under-Root $ProgramDirectory
    $report = @(
        "${Script}: moved the configuration to $UserConfig and the record to $UserState, owned by $User and verified, deleted the $ServiceName service, and left $left and $program; next, as $User and not elevated, then elevated once satisfied:",
        "  irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.ps1 | iex",
        "  printobserver sign-in",
        "  Remove-Item -Recurse -Force '$left', '$program'"
    )
    # llmlint: ignore[tool_output_is_signal] suppressions.toml has the reason.
    [Console]::Error.WriteLine($report -join [Environment]::NewLine)
}
Report-Migrated
