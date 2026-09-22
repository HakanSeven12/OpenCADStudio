param(
    [Parameter(Mandatory = $true)]
    [string]$Directory,
    [string]$AutoCadCore = 'C:\Program Files\Autodesk\AutoCAD 2025\accoreconsole.exe',
    [int]$TimeoutSeconds = 25
)

$ErrorActionPreference = 'Stop'
$drawingDirectory = (Resolve-Path -LiteralPath $Directory).Path
$scriptPath = Join-Path $PSScriptRoot 'autocad_audit.scr'
$targets = @(
    @{ Version = '2000'; File = 'mcp-acceptance-2000.dwg' },
    @{ Version = '2013'; File = 'mcp-acceptance-2013.dwg' },
    @{ Version = '2018'; File = 'mcp-acceptance-2018.dwg' }
)
$results = foreach ($target in $targets) {
    $drawing = Join-Path $drawingDirectory $target.File
    if (-not (Test-Path -LiteralPath $drawing -PathType Leaf)) {
        throw "Missing acceptance drawing: $drawing"
    }
    $isolation = Join-Path $env:TEMP "ocs-autocad-acceptance-$($target.Version)"
    New-Item -ItemType Directory -Force -Path $isolation | Out-Null

    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $AutoCadCore
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.Encoding]::Unicode
    $start.StandardErrorEncoding = [Text.Encoding]::Unicode
    @(
        '/i', $drawing,
        '/s', $scriptPath,
        '/l', 'en-US',
        '/isolate', "ocsaccept$($target.Version)", $isolation,
        '/readonly'
    ) | ForEach-Object { [void]$start.ArgumentList.Add($_) }

    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    [void]$process.Start()
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    $forcedTermination = -not $process.WaitForExit($TimeoutSeconds * 1000)
    if ($forcedTermination) {
        $process.Kill($true)
        $process.WaitForExit()
    }
    $text = $stdout.Result + $stderr.Result
    $log = Join-Path $drawingDirectory "autocad-audit-$($target.Version).log"
    [IO.File]::WriteAllText($log, $text, [Text.UTF8Encoding]::new($false))
    $passed = [regex]::IsMatch($text, 'Total errors found\s+0\s+fixed\s+0')
    [pscustomobject]@{
        Version = $target.Version
        Passed = $passed
        ForcedTermination = $forcedTermination
        ExitCode = $process.ExitCode
        Log = $log
    }
}

$results | Format-Table -AutoSize
if ($results.Passed -contains $false) {
    exit 1
}
