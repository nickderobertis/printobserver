param([Parameter(Mandatory=$true)][string]$Scratch)
$ErrorActionPreference = 'Stop'
$trace = $null
$protocol = Get-Content -Raw (Join-Path $Scratch 'etw-protocol.json') | ConvertFrom-Json
function Write-Response([string]$Kind, [hashtable]$Fields = @{}) {
    $frame = @{ kind=$Kind; schema_version=$protocol.schema_version }
    foreach ($entry in $Fields.GetEnumerator()) { $frame[$entry.Key]=$entry.Value }
    $golden = $protocol.responses.$Kind
    $actual = @($frame.Keys | Where-Object { $_ -ne 'schema_version' } | Sort-Object)
    $expected = @($golden.PSObject.Properties.Name | Sort-Object)
    if (@(Compare-Object $actual $expected).Count) { throw 'ETW response differs from the committed golden' }
    [Console]::Out.WriteLine(($frame | ConvertTo-Json -Compress))
}
try {
    Add-Type -Path (Join-Path $Scratch 'etw.cs')
    $trace = [PrintobserverTrace]::new($Scratch)
    Write-Response 'ready' @{ session_guid=$trace.SessionId }
    while ($null -ne ($line = [Console]::In.ReadLine())) {
        $request = ConvertFrom-Json -InputObject $line -NoEnumerate
        if ($null -eq $request -or $request.GetType() -ne [System.Management.Automation.PSCustomObject]) { throw 'ETW request envelope must be a JSON object' }
        if ($request.schema_version -isnot [long] -or $request.schema_version -ne $protocol.schema_version) { throw 'Unsupported ETW request schema version' }
        if ($request.kind -isnot [string] -or $protocol.requests.PSObject.Properties.Name -notcontains $request.kind) {
            throw 'Unknown ETW request kind: expected a supported scalar string'
        }
        $template = $protocol.requests.($request.kind)
        if ($null -ne $template) {
            $actual = @($request.PSObject.Properties.Name | Where-Object { $_ -ne 'schema_version' } | Sort-Object)
            $expected = @($template.PSObject.Properties.Name | Sort-Object)
            if (@(Compare-Object $actual $expected).Count) { throw 'ETW request differs from the committed golden' }
        }
        switch ($request.kind) {
            'prepare' {
                $boundary = $trace.Prepare()
                Write-Response 'prepared'
            }
            'finish' {
                if ($request.root -isnot [long] -or $request.root -le 0 -or $request.root -gt [uint32]::MaxValue) {
                    throw 'ETW root must be a positive uint32 JSON integer'
                }
                $endpoints = @($trace.Finish([uint32]$request.root))
                Write-Response 'completed' @{ endpoints=@($endpoints) }
            }
            'invalidate' {
                $trace.InvalidateNextChunk()
                Write-Response 'invalidated'
            }
            'unclosed' {
                $trace.ReadUnclosedChunk()
                Write-Response 'completed' @{ endpoints=@() }
            }
            'quit' {
                $trace.Dispose()
                $trace = $null
                Write-Response 'stopped'
                exit 0
            }
            default { throw "Unknown ETW request kind: $($request.kind)" }
        }
    }
} catch {
    Write-Response 'error' @{ message=$_.Exception.ToString() }
    exit 1
} finally {
    if ($null -ne $trace) { $trace.Dispose() }
}
