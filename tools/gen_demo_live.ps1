# Records the live demo (real mouse / keyboard operation of the app) and builds docs/images/demo-live.gif:
#   powershell -File tools/gen_demo_live.ps1 [-Exe path\to\circuitCad.exe] [-Width 960] [-Fps 6]
# The story follows the four tabs with one sample (pot_dimmer: a potentiometer dims an LED): the schematic and its DC result,
# the knob turned with the results following, the breadboard switched on and its knob turned, the board imported, placed,
# routed and checked, the 3D view, then the bench tab (assembly, first power-on).
# The screen coordinates below are absolute, for a maximized window on a 1920x1080 screen (taken 2026-11-19, alpha.12).
# The real mouse is used: do not touch it until it ends. A copy of the exe is started (a running exe blocks the next build).
param([string]$Exe = "", [int]$Width = 960, [int]$Fps = 6)

. (Join-Path $PSScriptRoot "screenrec.ps1")
$root = Split-Path -Parent $PSScriptRoot
if ($Exe -eq "") {
    $found = @("target\release\circuitCad.exe", "target\debug\circuitCad.exe") | ForEach-Object { Join-Path $root $_ } | Where-Object { Test-Path $_ } | Sort-Object { (Get-Item $_).LastWriteTime } -Descending
    if ($found) { $Exe = @($found)[0] }
}
if (-not (Test-Path $Exe)) { throw "circuitCad.exe not found: $Exe (cargo build -p app)" }
$copy = Join-Path $env:TEMP "circuitCad-demo\circuitCad.exe"
New-Item -ItemType Directory -Force (Split-Path $copy) | Out-Null
Copy-Item $Exe $copy -Force
$frames = Join-Path $root "target\demo-live-frames"
$sample = Join-Path $root "docs\examples\pot_dimmer.schjson"

# absolute screen points -> the window-relative ones the recording helpers take (they log the cursor and the press for the GIF)
$script:o = @(0, 0)
function C($x, $y) { Click ($x - $script:o[0]) ($y - $script:o[1]) }
function D($x1, $y1, $x2, $y2, $ms = 1200) { Drag ($x1 - $script:o[0]) ($y1 - $script:o[1]) ($x2 - $script:o[0]) ($y2 - $script:o[1]) $ms }

try {
    Push-Location $root
    Rec-Launch $copy "`"$sample`" --paper"
    Pop-Location
    $c = Rec-Client; $script:o = @($c[0], $c[1])
    C 1000 10                                       # focus (the title bar)
    Rec-Start $frames 10

    Mark "① 回路図: サンプルを開いて、DC 動作点を計算"
    Pause 1500
    C 123 72; Pause 500                             # the analysis tab
    C 27 195; Pause 300                             # auto recalculation
    C 190 253; Pause 2200                           # DC operating point: volts and currents on the wires

    Mark "ボリュームのつまみを回すと、電圧と電流がその場で変わる"
    C 48 72; Pause 400                              # the drawing tab
    C 1054 333; Pause 900                           # VR1: its knob appears on the left
    D 141 357 250 357 1800; Pause 700
    D 250 357 200 357 1000; Pause 1200

    Mark "② 試作: ブレッドボードに差して、電源を入れる"
    C 78 36; Pause 1800                             # the prototype tab
    C 61 242; Pause 2500                            # power on

    Mark "絵の上のボリュームを回すと、LED の明るさが変わる"
    D 879 340 760 340 1800; Pause 1500              # towards pin 1 (VCC): brighter
    D 879 340 950 340 1500; Pause 1500

    Mark "③ 基板: 取り込んで、部品を並べる"
    C 159 36; Pause 1200                            # the board tab
    C 199 102; Pause 1500                           # import from the schematic
    C 199 221; Pause 2000                           # place the parts, decide the outline
    D 1172 628 1225 628 900; Pause 500              # D1 a little right
    D 1016 527 1016 585 900; Pause 500              # VR1 down, clear of Q1
    C 1600 850; Pause 600                           # deselect (the panel goes back to the steps)

    Mark "GND のベタ → 全部つなぐ (自動配線) → DRC"
    C 80 446; Pause 700                             # step 2
    C 75 278; Pause 1500                            # GND pour
    C 199 278; Pause 2500                           # route all
    C 80 468; Pause 600                             # step 3
    C 199 305; Pause 2200                           # DRC: 0 violations

    Mark "④ 3D で見る"
    C 80 654; Pause 500                             # step 4
    C 106 338; Pause 2500                           # 3D
    D 860 450 1000 420 1800; Pause 600
    D 1000 420 800 440 1800; Pause 1000
    C 1293 99; Pause 600                            # close the 3D window

    Mark "⑤ 実機: 背の低い部品から組み立てる"
    C 228 36; Pause 1800                            # the bench tab
    C 21 357; Pause 1000                            # R1 fitted
    C 21 383; Pause 1500                            # D1 fitted: Q1 lights next

    Mark "最初の電源投入: 正常なら流れる電流と、電流制限の値"
    C 88 123; Pause 4000
    Rec-Stop
} finally { [Rec]::Kill() }

python -I (Join-Path $PSScriptRoot "gen_demo_live.py") $frames (Join-Path $root "docs\images\demo-live.gif") $Width $Fps
