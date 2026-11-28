# Records the live demo (real mouse / keyboard operation of the app) and builds docs/images/demo-live.gif (English: demo-live-en.gif):
#   powershell -File tools/gen_demo_live.ps1 [-Lang ja|en] [-Exe path\to\circuitCad.exe] [-Width 960] [-Fps 6]
# The story follows the four tabs with one sample (pot_dimmer: a potentiometer dims an LED): the schematic and its DC result,
# the knob turned with the results following, the breadboard switched on and its knob turned, the board imported, placed,
# routed and checked, the 3D view, the bench tab (assembly, first power-on), then a second sample (arduino_blocks): an Arduino
# sketch written with blocks, a block dragged in and undone, the power on and its button held (the running block lights).
# The screen coordinates below are absolute, for a maximized window on a 1920x1080 screen. The button places differ by language
# (the words are of different widths), so each language has its own table: ja taken 2026-11-19 (alpha.12, the blocks scene
# 2026-11-28), en taken 2026-11-28. After a UI change, check them again with screenshots (tools/screenrec.ps1 FullShot).
# The real mouse is used: do not touch it until it ends. A copy of the exe is started (a running exe blocks the next build).
param([ValidateSet("ja", "en")][string]$Lang = "ja", [string]$Exe = "", [int]$Width = 960, [int]$Fps = 6)

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
$out = if ($Lang -eq "en") { "docs\images\demo-live-en.gif" } else { "docs\images\demo-live.gif" }

# the blocks sample has no Japanese in its sketch (no comment line; it sends "presses: "), so both languages open it as it is
$blocks = "docs\examplesrduino_blocks.schjson"

# where to click, by language: [x, y] (a drag: [x1, y1, x2, y2])
$P = @{
    ja = @{
        AnalysisTab = 123, 72; AutoCalc = 27, 195; RunDc = 190, 253; DrawTab = 48, 72; Vr1 = 1054, 333
        Knob1 = 141, 357, 250, 357; Knob2 = 250, 357, 200, 357
        ProtoTab = 78, 36; PowerOn = 61, 242; BreadKnob1 = 879, 340, 760, 340; BreadKnob2 = 879, 340, 950, 340
        BoardTab = 159, 36; Import = 199, 102; Place = 199, 221; MoveD1 = 1172, 628, 1225, 628; MoveVr1 = 1016, 527, 1016, 585
        Deselect = 1600, 850; Step2 = 80, 446; Pour = 75, 278; Route = 199, 278; Step3 = 80, 468; Drc = 199, 305
        Step4 = 80, 654; View3d = 106, 338; Turn1 = 860, 450, 1000, 420; Turn2 = 1000, 420, 800, 440; Close3d = 1293, 99
        BenchTab = 228, 36; Fit1 = 21, 357; Fit2 = 21, 383; PowerStep = 88, 123
        SketchWindow = 253, 296; WaitChip = 677, 259, 520, 475; SketchUndo = 475, 198; CloseSketch = 1884, 98
        SketchPower = 152, 248; Button = 963, 525
    }
    en = @{
        AnalysisTab = 122, 73; AutoCalc = 27, 202; RunDc = 190, 260; DrawTab = 48, 74; Vr1 = 1054, 333
        Knob1 = 135, 302, 250, 302; Knob2 = 250, 302, 200, 302
        ProtoTab = 124, 38; PowerOn = 163, 256; BreadKnob1 = 879, 340, 760, 340; BreadKnob2 = 879, 340, 950, 340
        BoardTab = 200, 38; Import = 199, 104; Place = 199, 326; MoveD1 = 1172, 612, 1225, 612; MoveVr1 = 1016, 527, 1016, 585
        Deselect = 1600, 850; Step2 = 80, 558; Pour = 75, 389; Route = 199, 389; Step3 = 80, 585; Drc = 199, 404
        Step4 = 80, 789; View3d = 106, 439; Turn1 = 860, 450, 1000, 420; Turn2 = 1000, 420, 800, 440; Close3d = 1430, 102
        BenchTab = 274, 38; Fit1 = 21, 420; Fit2 = 21, 447; PowerStep = 73, 131
        SketchWindow = 226, 303; WaitChip = 692, 268, 520, 505; SketchUndo = 462, 207; CloseSketch = 1880, 102
        SketchPower = 163, 256; Button = 963, 525
    }
}[$Lang]

# the captions, by language
$T = @{
    ja = @{
        Dc = "① 回路図: サンプルを開いて、DC 動作点を計算"; Knob = "ボリュームのつまみを回すと、電圧と電流がその場で変わる"
        Proto = "② 試作: ブレッドボードに差して、電源を入れる"; BreadKnob = "絵の上のボリュームを回すと、LED の明るさが変わる"
        Board = "③ 基板: 取り込んで、部品を並べる"; Route = "GND のベタ → 全部つなぐ (自動配線) → DRC"; View3d = "④ 3D で見る"
        Bench = "⑤ 実機: 背の低い部品から組み立てる"; PowerOn = "最初の電源投入: 正常なら流れる電流と、電流制限の値"
        Blocks = "⑥ Arduino のスケッチをブロックで: 並べるとコードも変わる"; BlocksUndo = "元に戻す"
        BlocksRun = "電源を入れると、今動いているブロックが光る。ボタンを押すと LED が点く"
    }
    en = @{
        Dc = "1. Schematic: open a sample and run the DC operating point"; Knob = "Turn the potentiometer: voltages and currents follow at once"
        Proto = "2. Prototype: put it on a breadboard and switch the power on"; BreadKnob = "Turn the knob on the picture: the LED gets brighter or dimmer"
        Board = "3. PCB: import the parts and place them"; Route = "GND pour, connect all (autoroute), DRC"; View3d = "4. View in 3D"
        Bench = "5. Bench: assemble from the lowest parts up"; PowerOn = "First power-on: the normal current and the current limit to set"
        Blocks = "6. An Arduino sketch in blocks: the code follows as you place them"; BlocksUndo = "Undo"
        BlocksRun = "Power on: the running block lights up. Hold the button and the LED turns on"
    }
}[$Lang]

# absolute screen points -> the window-relative ones the recording helpers take (they log the cursor and the press for the GIF)
$script:o = @(0, 0)
function C($p) { Click ($p[0] - $script:o[0]) ($p[1] - $script:o[1]) }
function D($p, $ms = 1200) { Drag ($p[0] - $script:o[0]) ($p[1] - $script:o[1]) ($p[2] - $script:o[0]) ($p[3] - $script:o[1]) $ms }
function C2($x, $y) { Click ($x - $script:o[0]) ($y - $script:o[1]) }

try {
    Push-Location $root
    Rec-Launch $copy "`"docs\examples\pot_dimmer.schjson`" --paper --lang $Lang"
    Pop-Location
    $c = Rec-Client; $script:o = @($c[0], $c[1])
    C2 1000 10                                      # focus (the title bar)
    Rec-Start $frames 10

    Mark $T.Dc
    Pause 1500
    C $P.AnalysisTab; Pause 500
    C $P.AutoCalc; Pause 300                        # auto recalculation
    C $P.RunDc; Pause 2200                          # DC operating point: volts and currents on the wires

    Mark $T.Knob
    C $P.DrawTab; Pause 400
    C $P.Vr1; Pause 900                             # VR1: its knob appears on the left
    D $P.Knob1 1800; Pause 700
    D $P.Knob2 1000; Pause 1200

    Mark $T.Proto
    C $P.ProtoTab; Pause 1800
    C $P.PowerOn; Pause 2500

    Mark $T.BreadKnob
    D $P.BreadKnob1 1800; Pause 1500                # towards pin 1 (VCC): brighter
    D $P.BreadKnob2 1500; Pause 1500

    Mark $T.Board
    C $P.BoardTab; Pause 1200
    C $P.Import; Pause 1500                         # import from the schematic
    C $P.Place; Pause 2000                          # place the parts, decide the outline
    D $P.MoveD1 900; Pause 500                      # D1 a little right
    D $P.MoveVr1 900; Pause 500                     # VR1 down, clear of Q1
    C $P.Deselect; Pause 600                        # deselect (the panel goes back to the steps)

    Mark $T.Route
    C $P.Step2; Pause 700
    C $P.Pour; Pause 1500
    C $P.Route; Pause 2500
    C $P.Step3; Pause 600
    C $P.Drc; Pause 2200                            # DRC: 0 violations

    Mark $T.View3d
    C $P.Step4; Pause 500
    C $P.View3d; Pause 2500
    D $P.Turn1 1800; Pause 600
    D $P.Turn2 1800; Pause 1000
    C $P.Close3d; Pause 600

    Mark $T.Bench
    C $P.BenchTab; Pause 1800
    C $P.Fit1; Pause 1000                           # R1 fitted
    C $P.Fit2; Pause 1500                           # D1 fitted: Q1 lights next

    Mark $T.PowerOn
    C $P.PowerStep; Pause 4000

    # the second sample in a second window, maximized over the first (the capture keeps the same screen area)
    Mark $T.Blocks
    $first = [Rec]::App
    Push-Location $root
    Rec-Launch $copy "`"$blocks`" --proto --lang $Lang"
    Pop-Location
    try { $first.Kill() } catch { }
    C $P.SketchWindow; Pause 2000                   # blocks and code side by side
    $w = $P.WaitChip
    DragRest ($w[0] - $script:o[0]) ($w[1] - $script:o[1]) ($w[2] - $script:o[0]) ($w[3] - $script:o[1]) 1500 700
    Pause 2500                                      # "Wait" in the setup: the code has delay(500) now
    Mark $T.BlocksUndo
    C $P.SketchUndo; Pause 1500
    C $P.CloseSketch; Pause 600

    Mark $T.BlocksRun
    C $P.SketchPower; Pause 2500
    $b = $P.Button
    Hold ($b[0] - $script:o[0]) ($b[1] - $script:o[1]) 2500; Pause 1500
    Hold ($b[0] - $script:o[0]) ($b[1] - $script:o[1]) 1500; Pause 2000
    Rec-Stop
} finally { [Rec]::Kill(); if ($first) { try { $first.Kill() } catch { } } }

python -I (Join-Path $PSScriptRoot "gen_demo_live.py") $frames (Join-Path $root $out) $Width $Fps
