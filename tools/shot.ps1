# One screenshot of the app, to look at a change (the app's own --screenshot: it draws a few frames, saves the PNG and quits).
#
#   powershell -File tools/shot.ps1 -AppArgs "docs\examples\ne555_blinker.schjson --proto --proto-kind perf" -Out shot.png
#   powershell -File tools/shot.ps1 -AppArgs "..." -Out shot.png -Crop 0,30,400,330 -Scale 2   # a part of it, enlarged
#   ... -NoBuild                                                                                 # the exe built already
#
# Builds the app (cargo build -p app), copies the exe to a folder of its own in TEMP and starts that copy (a running exe cannot
# be replaced: the next build would stop), waits for the picture (60 s at most). The app's arguments are one string: a list
# passed with -File is split wrongly. More flags of the app (--proto-back, --proto-tool, --shot-frames, --shot-zoom, ...): see
# crates/app/src/args/. Paths in -AppArgs are from the repository's folder.
param(
    [Parameter(Mandatory = $true)][string]$AppArgs,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Crop,
    [double]$Scale = 1,
    [int]$Frames = 120,
    [switch]$NoBuild
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $NoBuild) {
    Push-Location $root
    cargo build -q -p app
    $built = $LASTEXITCODE
    Pop-Location
    if ($built -ne 0) { throw "cargo build failed" }
}
$dir = Join-Path $env:TEMP 'circuitcad-shot'
New-Item -ItemType Directory -Force $dir | Out-Null
$exe = Join-Path $dir 'circuitCad.exe'
Copy-Item (Join-Path $root 'target\debug\circuitCad.exe') $exe -Force
$out = [System.IO.Path]::GetFullPath($Out)
if (Test-Path $out) { Remove-Item $out }
$p = Start-Process -FilePath $exe -ArgumentList "$AppArgs --shot-frames $Frames --screenshot `"$out`"" -PassThru -WorkingDirectory $root
if (-not $p.WaitForExit(60000)) { $p.Kill(); throw "the app did not finish in 60 s" }
if (-not (Test-Path $out)) { throw "no picture (exit code $($p.ExitCode))" }
if ($Crop) {
    # (x,y,width,height as one string: an int list passed with -File arrives as one string)
    $c = $Crop.Split(',') | ForEach-Object { [int]$_ }
    if ($c.Count -ne 4) { throw "-Crop wants x,y,width,height" }
    Add-Type -AssemblyName System.Drawing
    $img = [System.Drawing.Image]::FromFile($out)
    $w = [int]($c[2] * $Scale); $h = [int]($c[3] * $Scale)
    $cut = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($cut)
    $g.InterpolationMode = 'HighQualityBicubic'
    $g.DrawImage($img, (New-Object System.Drawing.Rectangle 0, 0, $w, $h), (New-Object System.Drawing.Rectangle $c[0], $c[1], $c[2], $c[3]), 'Pixel')
    $g.Dispose(); $img.Dispose()
    $cut.Save($out); $cut.Dispose()
}
Write-Output $out
