# Temporary probe: what Task Scheduler tracks for the per-user task, and what survives a kill.
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

function Servers { @(Get-CimInstance Win32_Process -Filter "Name='printobserver.exe'" | Where-Object { $_.ExecutablePath -eq $program }) }
function Listening { try { $c = New-Object Net.Sockets.TcpClient; $c.Connect('127.0.0.1', 8420); $c.Close(); $true } catch { $false } }
function Running {
    $svc = New-Object -ComObject Schedule.Service; $svc.Connect()
    @($svc.GetRunningTasks(1) | Where-Object { $_.Name -eq $task } | ForEach-Object { "engine=$($_.EnginePID) action=$($_.CurrentAction) guid=$($_.InstanceGuid)" }) -join '; '
}
function Tree([int]$root) {
    $all = @(Get-CimInstance Win32_Process)
    $all | Where-Object { $_.ProcessId -eq $root -or $_.ParentProcessId -eq $root -or $_.Name -match 'printobserver|conhost|OpenConsole|WindowsTerminal|taskhost' } |
        ForEach-Object { "  pid=$($_.ProcessId) ppid=$($_.ParentProcessId) $($_.Name) created=$($_.CreationDate) cmd=$($_.CommandLine)" }
}
function State { $t = Get-ScheduledTask -TaskName $task; $i = $t | Get-ScheduledTaskInfo; "state=$($t.State) lastrun=$($i.LastRunTime) result=$($i.LastTaskResult)" }
function Said {
    Write-Output "  foreground run says:"
    $j = Start-Job { param($p, $c) & $p server --config $c 2>&1 } -ArgumentList $program, $config
    Start-Sleep -Seconds 15; Receive-Job $j | ForEach-Object { "    $_" }; Stop-Job $j
    Get-NetTCPConnection -LocalPort 8420 -ErrorAction SilentlyContinue | ForEach-Object { "  port: $($_.State) owner=$($_.OwningProcess)" }
}
function Until([string]$what, [scriptblock]$ok, [int]$seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) { if (& $ok) { return $true }; Start-Sleep -Milliseconds 500 }
    Write-Output "TIMEOUT $what $(State)"; Said; return $false
}

$failures = 0
for ($n = 1; $n -le [int]$env:PROBE_ROUNDS; $n++) {
    Stop-ScheduledTask -TaskName $task; Start-ScheduledTask -TaskName $task
    if (-not (Until 'serving' { Listening } 120)) { Get-ChildItem -Recurse (Join-Path $env:LOCALAPPDATA 'printobserver') -Filter *.log | ForEach-Object { Get-Content -Tail 20 $_.FullName }; State; exit 1 }
    Stop-ScheduledTask -TaskName $task
    (Until 'stopped' { (Servers).Count -eq 0 } 120)
    Start-ScheduledTask -TaskName $task
    (Until 'serving again' { Listening } 120)
    Start-Sleep -Seconds ([int]$env:PROBE_SETTLE)
    $s = Servers
    Write-Output "round $n before kill: servers=$($s.ProcessId -join ',') $(State) running=[$(Running)]"
    Tree $s[0].ProcessId
    $killed = $s[0].ProcessId
    $handle = Get-Process -Id $killed
    & taskkill.exe /F /PID $killed
    $start = Get-Date
    $back = $false
    $last = ''
    while (((Get-Date) - $start).TotalSeconds -lt 180) {
        $now = Servers
        $line = "servers=$($now.ProcessId -join ',') exited=$($handle.HasExited) $(State) running=[$(Running)]"
        if ($line -ne $last) { Write-Output ("  +{0:N1}s {1}" -f ((Get-Date) - $start).TotalSeconds, $line); $last = $line }
        if ($now.Count -ge 1 -and $now[0].ProcessId -ne $killed -and (Listening)) { $back = $true; break }
        Start-Sleep -Milliseconds 1000
    }
    if (-not $back) {
        $failures++
        Write-Output "round $n NOT BACK"
        Tree $killed
        foreach ($p in Servers) { Write-Output "  survivor $($p.ProcessId) ppid=$($p.ParentProcessId) created=$($p.CreationDate) threads=$((Get-Process -Id $p.ProcessId).Threads.Count) handles=$((Get-Process -Id $p.ProcessId).HandleCount)" }
        Get-WinEvent -LogName 'Microsoft-Windows-TaskScheduler/Operational' -MaxEvents 30 -ErrorAction SilentlyContinue | Where-Object { $_.Message -match $task } | ForEach-Object { "  ev $($_.Id) $($_.TimeCreated): $($_.Message -replace '\s+',' ')" }
        foreach ($p in Servers) { & taskkill.exe /F /PID $p.ProcessId }
        Stop-ScheduledTask -TaskName $task
    } else {
        Write-Output ("round {0} back after {1:N1}s" -f $n, ((Get-Date) - $start).TotalSeconds)
    }
}
Write-Output "failures=$failures of $env:PROBE_ROUNDS"
