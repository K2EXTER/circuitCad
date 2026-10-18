# Regenerates the pictures used by README.md (docs/images/*.png) with the app's own --screenshot option:
#   powershell -File tools/gen_screenshots.ps1               # all of them
#   powershell -File tools/gen_screenshots.ps1 -Only waveform  # one (name without .png)
# Needs a built app: cargo build -p app   (or -Exe path\to\circuitCad.exe, e.g. a release build).
# Add a picture by adding a line to $shots: name, then the app's arguments (see crates/app/src/args.rs).
param(
    [string]$Exe = "",
    [string]$Only = ""
)

$root = Split-Path -Parent $PSScriptRoot
if ($Exe -eq "") {
    # the most recently built one (a forgotten old build would take the pictures of an old screen)
    $found = @("target\debug\circuitCad.exe", "target\release\circuitCad.exe") | ForEach-Object { Join-Path $root $_ } | Where-Object { Test-Path $_ } | Sort-Object { (Get-Item $_).LastWriteTime } -Descending
    if ($found) { $Exe = @($found)[0] }
}
if (-not (Test-Path $Exe)) { throw "circuitCad.exe not found: $Exe (build it first: cargo build -p app)" }

$ex = Join-Path $root "docs\examples"
$out = Join-Path $root "docs\images"

# name -> arguments (the file to open comes first)
$shots = [ordered]@{
    "schematic-dc"           = @("$ex\series_regulator.schjson", "--dc")
    "waveform"               = @("$ex\ne555_blinker.schjson", "--tran", "3", "5m", "--signals", "VCC,TH,CT,OUT")
    "bode"                   = @("$ex\rc_lowpass.schjson", "--ac", "10", "1Meg", "100")
    "board"                  = @("$ex\rlc_board.pcbjson", "--drc")
    "order"                  = @("$ex\rlc_board.pcbjson", "--order")
    "guide"                  = @("--guide")
    "mixed-signal-schematic" = @("$ex\mixed_signal.schjson")
    "mixed-signal-waveform"  = @("$ex\mixed_signal.schjson", "--tran", "8m", "10u", "--signals", "CLK,Q1,Q2,FILT")
}

$failed = @()
foreach ($name in $shots.Keys) {
    if ($Only -ne "" -and $name -ne $Only) { continue }
    $png = Join-Path $out "$name.png"
    $argv = @($shots[$name]) + @("--screenshot", $png)
    $p = Start-Process -FilePath $Exe -ArgumentList $argv -PassThru
    if (-not $p.WaitForExit(120000)) {
        $p.Kill()
        $failed += $name
        Write-Host "TIMEOUT  $name"
    } elseif ($p.ExitCode -ne 0 -or -not (Test-Path $png)) {
        $failed += $name
        Write-Host "FAILED   $name (exit $($p.ExitCode))"
    } else {
        Write-Host ("ok       {0}  ({1:N0} KB)" -f $name, ((Get-Item $png).Length / 1KB))
    }
}
if ($failed.Count -gt 0) { exit 1 }

