# Checks run.ps1's JSON writing without Power BI (CI runs it on Windows).
$ErrorActionPreference = 'Stop'
$env:CHARTED_CONFORMANCE_FUNCTIONS_ONLY = '1'
. (Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) 'run.ps1')
$cases = @(
    @([DBNull]::Value, 'null'),
    @($true, 'true'),
    @($false, 'false'),
    @([double]1.5, '1.5'),
    @([double]0.1, '0.1'),
    @([double]::PositiveInfinity, '{"num": "Infinity"}'),
    @([double]::NaN, '{"num": "NaN"}'),
    @([decimal]1234.5, '1234.5'),
    @([int64]42, '42'),
    @((Get-Date -Year 2024 -Month 1 -Day 31).Date, '"2024-01-31"'),
    @([DateTime]::new(2024, 1, 31, 13, 5, 0), '"2024-01-31T13:05:00"'),
    @('say "hi"\', '"say \"hi\"\\"'),
    @("a`nb", '"a\nb"')
)
$bad = 0
foreach ($c in $cases) {
    $got = Json-Cell $c[0]
    if ($got -ne $c[1]) { Write-Host "Json-Cell($($c[0])) gave $got, not $($c[1])"; $bad++ }
}
[void](ConvertFrom-Json ('[' + (($cases | ForEach-Object { Json-Cell $_[0] }) -join ',') + ']'))
if ($bad) { exit 1 }
Write-Host "run.ps1: $($cases.Count) JSON cases pass"
