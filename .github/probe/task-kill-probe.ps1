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
function Said {
    $j = Start-Job { param($p, $c) & $p server --config $c 2>&1 } -ArgumentList $program, $config
    Start-Sleep -Seconds 12; $out = Receive-Job $j; Stop-Job $j; Remove-Job $j -Force
    $out | Select-Object -Last 3 | ForEach-Object { "    server says: $_" }
}
foreach ($hold in $env:PROBE_HOLDS -split ',') {
    Stop-ScheduledTask -TaskName $task
    Until 'stopped' { (Servers).Count -eq 0 } 120
    Start-ScheduledTask -TaskName $task
    Until 'serving' { Listening } 120
    $killed = (Servers)[0].ProcessId
    $at = Get-Date
    & taskkill.exe /F /PID $killed | Out-Null
    Until 'killed gone' { (Servers).Count -eq 0 } 30
    # Something else takes 127.0.0.1:8420 as a source port, as any outgoing
    # loopback connection handed that ephemeral port does.
    $client = $null
    if ($hold -ne 'none') {
        $client = New-Object System.Net.Sockets.Socket([System.Net.Sockets.AddressFamily]::InterNetwork, [System.Net.Sockets.SocketType]::Stream, [System.Net.Sockets.ProtocolType]::Tcp)
        $client.Bind((New-Object System.Net.IPEndPoint([System.Net.IPAddress]::Loopback, 8420)))
        $client.Connect('127.0.0.1', 5000)
        if ($hold -eq 'timewait') { $client.Shutdown('Both'); $client.Close(); $client = $null }
        Get-NetTCPConnection -LocalPort 8420 -ErrorAction SilentlyContinue | ForEach-Object { "    port 8420: $($_.LocalAddress):$($_.LocalPort) -> $($_.RemoteAddress):$($_.RemotePort) $($_.State)" }
    }
    Write-Output ("[hold {0}] killed {1} at {2:HH:mm:ss}; next run {3:HH:mm:ss}" -f $hold, $killed, $at, (NextRun))
    $back = $false
    $saidOnce = $false
    while (((Get-Date) - $at).TotalSeconds -lt 170) {
        if ((Servers).Count -ge 1 -and (Listening)) { $back = $true; break }
        if (-not $saidOnce -and ((Get-Date) - $at).TotalSeconds -gt 75) { Said; $saidOnce = $true }
        Start-Sleep -Milliseconds 1000
    }
    Write-Output ("[hold {0}] {1} after {2:N1}s: {3}" -f $hold, $(if ($back) { 'BACK' } else { 'NOT BACK' }), ((Get-Date) - $at).TotalSeconds, (State))
    Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-TaskScheduler/Operational'; StartTime = $at.AddSeconds(-2) } -ErrorAction SilentlyContinue |
        Where-Object { $_.Message -match $task -and $_.Id -in 100, 102, 129, 201, 322, 329, 101, 103 } | Sort-Object TimeCreated |
        ForEach-Object { "    ev {0} {1:HH:mm:ss} {2}" -f $_.Id, $_.TimeCreated, ((($_.Message -replace '\s+', ' ') -replace '.*(return code \S+|process ID \S+|already running|instance of the).*', '$1')) }
    if ($client) { $client.Close() }
    Until 'back once released' { (Servers).Count -ge 1 -and (Listening) } 150
    Write-Output "    after release: $(State) servers=$((Servers).ProcessId -join ',')"
}
