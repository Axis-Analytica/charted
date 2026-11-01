# Records Power BI Desktop's answers to conformance/queries.json in
# conformance/expected.json. See conformance/README.md.
#
# 1. Open conformance\golden\golden.pbip in Power BI Desktop and leave it open.
# 2. From the repository folder, in Windows PowerShell:
#      powershell -ExecutionPolicy Bypass -File conformance\run.ps1
# 3. Copy conformance\expected.json back (or commit it from the VM).
#
# -Port picks the Desktop instance when several are open; -NoRefresh skips loading
# the data (if you already pressed Refresh).
param(
    [int]$Port = 0,
    [switch]$NoRefresh
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$inv = [Globalization.CultureInfo]::InvariantCulture

function Find-Port {
    $roots = @(
        (Join-Path $env:LOCALAPPDATA 'Microsoft\Power BI Desktop\AnalysisServicesWorkspaces'),
        (Join-Path $env:USERPROFILE 'Microsoft\Power BI Desktop Store App\AnalysisServicesWorkspaces')
    )
    $files = foreach ($r in $roots) {
        if (Test-Path $r) { Get-ChildItem -Path $r -Recurse -Filter 'msmdsrv.port.txt' -ErrorAction SilentlyContinue }
    }
    $latest = $files | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $latest) { throw 'No running Power BI Desktop found. Open conformance\golden\golden.pbip first.' }
    $text = [IO.File]::ReadAllText($latest.FullName, [Text.Encoding]::Unicode)
    [int]($text -replace '[^0-9]', '')
}

function Load-Adomd {
    $candidates = @(
        "$env:ProgramFiles\Microsoft Power BI Desktop\bin\Microsoft.PowerBI.AdomdClient.dll",
        "${env:ProgramFiles(x86)}\Microsoft Power BI Desktop\bin\Microsoft.PowerBI.AdomdClient.dll"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { Add-Type -Path $c; return }
    }
    # The Microsoft Store edition keeps its files out of reach: use the NuGet package.
    $dir = Join-Path $env:TEMP 'charted-adomd'
    $dll = Join-Path $dir 'lib\net45\Microsoft.AnalysisServices.AdomdClient.dll'
    if (-not (Test-Path $dll)) {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $zip = Join-Path $dir 'adomd.zip'
        Write-Host 'Downloading the ADOMD.NET client from nuget.org...'
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -UseBasicParsing -OutFile $zip `
            -Uri 'https://www.nuget.org/api/v2/package/Microsoft.AnalysisServices.AdomdClient.retail.amd64'
        Expand-Archive -Force -Path $zip -DestinationPath $dir
    }
    Add-Type -Path $dll
}

function Json-String([string]$s) {
    $b = New-Object Text.StringBuilder
    [void]$b.Append('"')
    foreach ($ch in $s.ToCharArray()) {
        switch ($ch) {
            '"' { [void]$b.Append('\"') }
            '\' { [void]$b.Append('\\') }
            "`n" { [void]$b.Append('\n') }
            "`r" { [void]$b.Append('\r') }
            "`t" { [void]$b.Append('\t') }
            default {
                if ([int]$ch -lt 32) { [void]$b.Append(('\u{0:x4}' -f [int]$ch)) }
                else { [void]$b.Append($ch) }
            }
        }
    }
    [void]$b.Append('"')
    $b.ToString()
}

function Json-Number([double]$x) {
    if ([double]::IsNaN($x)) { return '{"num": "NaN"}' }
    if ([double]::IsPositiveInfinity($x)) { return '{"num": "Infinity"}' }
    if ([double]::IsNegativeInfinity($x)) { return '{"num": "-Infinity"}' }
    $x.ToString('R', $inv)
}

function Json-Cell($v) {
    if ($v -eq $null -or $v -is [DBNull]) { return 'null' }
    if ($v -is [bool]) { if ($v) { return 'true' } else { return 'false' } }
    if ($v -is [DateTime]) {
        if ($v.TimeOfDay.Ticks -eq 0) { return Json-String $v.ToString('yyyy-MM-dd', $inv) }
        return Json-String $v.ToString('yyyy-MM-ddTHH:mm:ss.FFFFFFF', $inv).TrimEnd('.')
    }
    if ($v -is [decimal]) { return $v.ToString($inv) }
    if ($v -is [double] -or $v -is [single]) { return Json-Number ([double]$v) }
    if ($v -is [int64] -or $v -is [int32] -or $v -is [int16] -or $v -is [byte]) { return $v.ToString($inv) }
    Json-String ([string]$v)
}

function Scalar($conn, [string]$dax) {
    $cmd = $conn.CreateCommand()
    $cmd.CommandText = $dax
    $r = $cmd.ExecuteReader()
    try { [void]$r.Read(); $r.GetValue(0) } finally { $r.Close() }
}

function Main {
    Load-Adomd
    if ($Port -eq 0) { $Port = Find-Port }
    Write-Host "Connecting to Power BI Desktop on localhost:$Port"
    $conn = New-Object Microsoft.AnalysisServices.AdomdClient.AdomdConnection "Data Source=localhost:$Port"
    $conn.Open()

    if (-not $NoRefresh) {
        $db = Scalar $conn 'SELECT [CATALOG_NAME] FROM $SYSTEM.DBSCHEMA_CATALOGS'
        Write-Host 'Refreshing the data...'
        try {
            $cmd = $conn.CreateCommand()
            $cmd.CommandText = '{"refresh": {"type": "full", "objects": [{"database": ' + (Json-String $db) + '}]}}'
            [void]$cmd.ExecuteNonQuery()
        } catch {
            throw "Refreshing failed ($($_.Exception.Message)). Press Refresh in Power BI Desktop, then run this again with -NoRefresh."
        }
    }
    $rows = Scalar $conn 'EVALUATE ROW("n", COUNTROWS(''Sales''))'
    if ($rows -ne 90) {
        throw "Sales has $rows rows, not 90: is golden.pbip the project open, and did its data load?"
    }

    $version = '?'
    try {
        $p = Get-Process -Name PBIDesktop -ErrorAction Stop | Select-Object -First 1
        $version = $p.MainModule.FileVersionInfo.ProductVersion
    } catch { }

    $queries = Get-Content -Raw -Encoding UTF8 (Join-Path $here 'queries.json') | ConvertFrom-Json
    $out = New-Object Text.StringBuilder
    [void]$out.Append("{`n  ""fixture"": " + (Json-String $queries.fixture) + ",`n")
    [void]$out.Append("  ""powerbi"": " + (Json-String $version) + ",`n  ""results"": {")
    $n = 0
    $failed = 0
    foreach ($q in $queries.queries) {
        if ($n -gt 0) { [void]$out.Append(',') }
        $n++
        if ($n % 100 -eq 0) { Write-Host "  $n of $($queries.queries.Count)" }
        $entry = "`n    " + (Json-String $q.id) + ": {""hash"": " + (Json-String $q.hash) + ', '
        try {
            $cmd = $conn.CreateCommand()
            $cmd.CommandText = $q.dax
            $r = $cmd.ExecuteReader()
            $lines = New-Object Collections.Generic.List[string]
            try {
                while ($r.Read()) {
                    $cells = for ($i = 0; $i -lt $r.FieldCount; $i++) { Json-Cell ($r.GetValue($i)) }
                    $lines.Add('[' + ($cells -join ', ') + ']')
                }
            } finally { $r.Close() }
            $entry += '"rows": [' + ($lines -join ', ') + ']}'
        } catch {
            $failed++
            $msg = $_.Exception.Message
            if ($_.Exception.InnerException) { $msg = $_.Exception.InnerException.Message }
            $entry += '"error": ' + (Json-String $msg) + '}'
        }
        [void]$out.Append($entry)
    }
    [void]$out.Append("`n  }`n}`n")
    $conn.Close()

    $path = Join-Path $here 'expected.json'
    [IO.File]::WriteAllText($path, $out.ToString(), (New-Object Text.UTF8Encoding $false))
    Write-Host "Wrote $path ($n queries, $failed of them errors in Power BI $version)."
}

# Tests load the functions above without connecting (conformance/test.ps1).
if (-not $env:CHARTED_CONFORMANCE_FUNCTIONS_ONLY) { Main }
