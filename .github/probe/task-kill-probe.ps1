# TEMPORARY diagnostic, removed before this change is finished: what Task
# Scheduler tracks for the per-user task, and what keeps an instance it thinks
# is running alive after its server is killed. PROBE_MODE says what else holds
# the server's console when it is killed: nothing, a second process attached to
# it, or its delegated terminal host suspended.
$ErrorActionPreference = 'Stop'
Start-Process -FilePath python -ArgumentList "$PSScriptRoot\machine.py" -WindowStyle Hidden
Start-Sleep -Seconds 3
$task = 'poprobe'
$env:PRINTOBSERVER_TASK_NAME = $task
& "$PSScriptRoot\..\..\scripts\install.ps1"
$config = Join-Path $env:LOCALAPPDATA 'printobserver\config.toml'
$text = Get-Content -Raw $config
$text = ($text -split "`n" | Where-Object { $_ -notmatch '^(auto|idle)_compact_tokens' }) -join "`n"
$text = $text.Replace('api_key = ""', 'api_key = "k"').Replace('shared_secret = ""', 'shared_secret = "s"')
Set-Content -Path $config -Value $text -Encoding utf8
$skill = ([regex]::Match($text, "skill_path = '([^']+)'")).Groups[1].Value
New-Item -ItemType Directory -Force -Path (Split-Path $skill) | Out-Null
Copy-Item -Recurse -Force "$PSScriptRoot\..\..\skills\printobserver\*" (Split-Path $skill)
$program = Join-Path $env:LOCALAPPDATA 'Programs\printobserver\printobserver.exe'
Write-Output "task action: $((Get-ScheduledTask -TaskName $task).Actions | ForEach-Object { $_.Execute + ' ' + $_.Arguments })"

Add-Type -Namespace Probe -Name Native -MemberDefinition @'
[DllImport("ntdll.dll")] public static extern int NtSuspendProcess(System.IntPtr process);
[DllImport("ntdll.dll")] public static extern int NtResumeProcess(System.IntPtr process);
'@

function Servers { @(Get-CimInstance Win32_Process -Filter "Name='printobserver.exe'" | Where-Object { $_.ExecutablePath -eq $program }) }
function Listening { try { $c = New-Object Net.Sockets.TcpClient; $c.Connect('127.0.0.1', 8420); $c.Close(); $true } catch { $false } }
function Running {
    $svc = New-Object -ComObject Schedule.Service; $svc.Connect()
    @($svc.GetRunningTasks(1) | Where-Object { $_.Name -eq $task } | ForEach-Object { "engine=$($_.EnginePID)" }) -join '; '
}
function State { $t = Get-ScheduledTask -TaskName $task; $i = $t | Get-ScheduledTaskInfo; "state=$($t.State) lastrun=$($i.LastRunTime.ToString('HH:mm:ss')) result=$($i.LastTaskResult)" }
function Around([object[]]$ids, [datetime]$since) {
    # The listed processes, their children, and every console host started since `since`.
    $all = @(Get-CimInstance Win32_Process)
    $all | Where-Object { $ids -contains $_.ProcessId -or $ids -contains $_.ParentProcessId -or
        ($_.Name -match 'conhost|OpenConsole|WindowsTerminal' -and $_.CreationDate -ge $since) } |
        ForEach-Object { "    pid=$($_.ProcessId) ppid=$($_.ParentProcessId) $($_.Name) threads=$($_.ThreadCount) created=$($_.CreationDate.ToString('HH:mm:ss.fff')) $($_.CommandLine)" }
}
function Until([string]$what, [scriptblock]$ok, [int]$seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) { if (& $ok) { return }; Start-Sleep -Milliseconds 500 }
    Write-Output "TIMEOUT $what $(State)"
}

function NextRun { (Get-ScheduledTask -TaskName $task | Get-ScheduledTaskInfo).NextRunTime }
function Events([datetime]$since) {
    Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-TaskScheduler/Operational'; StartTime = $since } -ErrorAction SilentlyContinue |
        Where-Object { $_.Message -match $task } | Sort-Object TimeCreated |
        ForEach-Object { "    ev {0} {1:HH:mm:ss.fff} {2}" -f $_.Id, $_.TimeCreated, (($_.Message -replace '\s+', ' ') -replace '^(.{0,170}).*', '$1') }
}
foreach ($lead in $env:PROBE_LEADS -split ',') {
    for ($n = 1; $n -le [int]$env:PROBE_ROUNDS; $n++) {
        Stop-ScheduledTask -TaskName $task
        Until 'stopped' { (Servers).Count -eq 0 } 120
        $since = Get-Date
        Start-ScheduledTask -TaskName $task
        Until 'serving' { Listening } 120
        # Kill `lead` seconds before the next minute's start, as the failing run's kill fell.
        $next = NextRun
        if ($next -lt (Get-Date).AddSeconds([int]$lead + 1)) { $next = $next.AddMinutes(1) }
        $wait = ($next.AddSeconds(-[int]$lead) - (Get-Date)).TotalMilliseconds
        if ($wait -gt 0) { Start-Sleep -Milliseconds $wait }
        $killed = (Servers)[0].ProcessId
        Write-Output ("[lead {0}s {1}] kill {2} at {3:HH:mm:ss.fff}, next run {4:HH:mm:ss}: {5}" -f $lead, $n, $killed, (Get-Date), (NextRun), (State))
        $at = Get-Date
        & taskkill.exe /F /PID $killed | Out-Null
        $last = ''
        $back = $false
        while (((Get-Date) - $at).TotalSeconds -lt 200) {
            $now = Servers
            $line = "servers=$($now.ProcessId -join ',') $(State) next=$((NextRun).ToString('HH:mm:ss')) running=[$(Running)]"
            if ($line -ne $last) { Write-Output ("  +{0,5:N1}s {1}" -f ((Get-Date) - $at).TotalSeconds, $line); $last = $line }
            if ($now.Count -ge 1 -and $now[0].ProcessId -ne $killed -and (Listening)) { $back = $true; break }
            Start-Sleep -Milliseconds 1000
        }
        Write-Output ("[lead {0}s {1}] {2} after {3:N1}s" -f $lead, $n, $(if ($back) { 'BACK' } else { 'NOT BACK' }), ((Get-Date) - $at).TotalSeconds)
        Events $at.AddSeconds(-2)
    }
}
