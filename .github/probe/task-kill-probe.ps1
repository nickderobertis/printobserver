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

foreach ($mode in $env:PROBE_MODES -split ',') {
    for ($n = 1; $n -le [int]$env:PROBE_ROUNDS; $n++) {
        Stop-ScheduledTask -TaskName $task
        Until 'stopped' { (Servers).Count -eq 0 } 120
        $since = Get-Date
        Start-ScheduledTask -TaskName $task
        Until 'serving' { Listening } 120
        $s = Servers
        $killed = $s[0].ProcessId
        $held = $null
        $suspended = @()
        if ($mode -eq 'attached') {
            # A second process on the server's console, as any process that
            # attaches to it or inherits it is.
            $helper = Join-Path $env:RUNNER_TEMP 'attach-helper.ps1'
            Set-Content -Path $helper -Value @'
param([uint32]$Target, [string]$Said)
Add-Type -Namespace H -Name C -MemberDefinition @"
[DllImport("kernel32.dll")] public static extern bool FreeConsole();
[DllImport("kernel32.dll")] public static extern bool AttachConsole(uint id);
"@
[void][H.C]::FreeConsole()
"attached $([H.C]::AttachConsole($Target))" | Out-File -FilePath $Said
Start-Sleep -Seconds 150
'@
            $held = Start-Process -PassThru -WindowStyle Hidden -FilePath powershell.exe -ArgumentList '-NoProfile', '-File', $helper, $killed, "$env:RUNNER_TEMP\attached.txt"
            Start-Sleep -Seconds 5
            Write-Output "  helper: $(Get-Content "$env:RUNNER_TEMP\attached.txt" -ErrorAction SilentlyContinue)"
        }
        if ($mode -eq 'stalled') {
            # The delegated terminal host started with this server's console,
            # suspended: a terminal slow to notice its client is gone.
            $suspended = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'OpenConsole.exe' -and $_.CreationDate -ge $since })
            foreach ($p in $suspended) { [void][Probe.Native]::NtSuspendProcess((Get-Process -Id $p.ProcessId).Handle) }
            Write-Output "  suspended: $($suspended.ProcessId -join ',')"
        }
        Write-Output "[$mode $n] before kill: server=$killed $(State) running=[$(Running)]"
        Around @($killed) $since
        & taskkill.exe /F /PID $killed | Out-Null
        $start = Get-Date
        $last = ''
        $back = $false
        while (((Get-Date) - $start).TotalSeconds -lt 140) {
            $now = Servers
            $line = "servers=$($now.ProcessId -join ',') $(State) running=[$(Running)]"
            if ($line -ne $last) {
                Write-Output ("  +{0,5:N1}s {1}" -f ((Get-Date) - $start).TotalSeconds, $line); $last = $line
                if ($now.Count -and $now[0].ProcessId -eq $killed) { Around @($killed) $since }
            }
            if ($now.Count -ge 1 -and $now[0].ProcessId -ne $killed -and (Listening)) { $back = $true; break }
            Start-Sleep -Milliseconds 1000
        }
        Write-Output ("[$mode $n] " + $(if ($back) { 'BACK' } else { 'NOT BACK' }) + (" after {0:N1}s" -f ((Get-Date) - $start).TotalSeconds))
        if (-not $back) { Around @($killed) $since }
        if ($held) { Stop-Process -Id $held.Id -Force -ErrorAction SilentlyContinue }
        foreach ($p in $suspended) { try { [void][Probe.Native]::NtResumeProcess((Get-Process -Id $p.ProcessId).Handle) } catch {} }
        Start-Sleep -Seconds 3
        if (-not $back) { Write-Output "  after releasing the holder: servers=$((Servers).ProcessId -join ',') $(State) running=[$(Running)]" }
    }
}
