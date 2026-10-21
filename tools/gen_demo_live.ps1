# Records the live demo (real mouse / keyboard operation of the app) and builds docs/images/demo-live.gif:
#   powershell -File tools/gen_demo_live.ps1 [-Exe path\to\circuitCad.exe] [-KeepFrames]
# The screen coordinates below are for a maximized window on a 1920x1080 screen and the release build
# (a debug build is too slow: clicks get lost). The real mouse is used: do not touch it until it ends.
param([string]$Exe = "")

. (Join-Path $PSScriptRoot "screenrec.ps1")
$root = Split-Path -Parent $PSScriptRoot
if ($Exe -eq "") { $Exe = Join-Path $root "target\release\circuitCad.exe" }
if (-not (Test-Path $Exe)) { throw "circuitCad.exe not found: $Exe (cargo build --release -p app)" }
$frames = Join-Path $root "target\demo-live-frames"

try {
    Rec-Launch $Exe ""
    Click 1000 10                                  # focus (the title bar)
    Rec-Start $frames 10

    Mark "① 部品を選んで置く (Aキーのクイック追加も使えます)"
    Pause 1200
    Click 108 301; Click 500 500                   # voltage source
    PressKey 0x41; Pause 400; Typing "res" 150; Pause 700; PressKey 0x0D; Pause 400; Click 800 400
    Click 105 277; Click 1100 500                  # LED
    Click 105 401; Click 500 650; Click 1139 650   # two grounds
    RightClick 1500 800; Pause 800

    Mark "② 配線ツールで、ピンからピンへ線を引く"
    Click 105 88
    Click 501 453; Click 765 409; RightClick 1500 800
    Click 853 409; Click 1050 497; RightClick 1500 800
    Click 1139 497; Click 1139 651; RightClick 1500 800
    Click 501 541; Click 501 651; RightClick 1500 800
    Pause 800

    Mark "③ 値を書き換えて、電圧と電流を計算 (LED には 330Ω)"
    Click 40 88                                    # select tool
    Click 809 409; Pause 700                       # R1 (the left panel gets wider)
    Click 160 539; CtrlKey 0x41; Typing "330" 180; PressKey 0x0D; Pause 700
    Click 68 57; Pause 700                         # analysis tab
    Click 33 149; Pause 400                        # auto recalculation
    Click 78 128; Pause 2200                       # DC operating point

    Mark "スライダーを動かすと、結果もその場で変わります"
    Click 26 57; Pause 500; Click 851 409; Pause 600
    Drag 136 560 100 560 1500; Pause 700
    Drag 100 560 136 560 1500; Pause 900

    Mark "④ ERC で配線ミスがないか点検"
    Click 95 733; Pause 2200

    Mark "⑤ 基板に取り込んで、部品を並べる"
    Click 105 33; Pause 1200                       # board tab
    Click 72 492; Pause 1500                       # import from schematic
    Click 110 530; Pause 2200                      # place parts, decide the outline

    Mark "⑥ 自動配線 → 配線を短くする"
    Click 165 708; Pause 2500                      # connect all nets
    Click 90 88; Pause 900                         # back to the select tool (the panel returns to the step list)
    Click 75 776; Pause 2000                       # shorten

    Mark "⑦ DRC で設計ルールを確認"
    Click 50 837; Pause 2500

    Mark "⑧ 3D で見る"
    Click 60 859; Pause 2500
    Drag 420 450 620 380 1800; Pause 800           # turn the board in the 3D window
    Drag 620 380 380 400 1800; Pause 1200
    Rec-Stop
} finally { [Rec]::Kill() }

python -I (Join-Path $PSScriptRoot "gen_demo_live.py") $frames (Join-Path $root "docs\images\demo-live.gif")
