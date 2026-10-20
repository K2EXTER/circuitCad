//! Challenge of the Arduino-compatible board path: draw the schematic of a minimal ATmega328P board. The microcontroller
//! has no simulation model, so the judge looks at the wiring itself (which pins share a net, which parts stand between
//! two nets, and whether their values are sensible) instead of simulating.

use super::*;

pub(super) const DRAW_ARDUINO: Challenge = Challenge {
    id: "draw_arduino",
    lesson: "reset",
    title: "Arduino 互換ボードの回路図を自分で描く (最終課題)",
    goal: "ATmega328P (U1) を使った、最小の Arduino 互換ボードの回路図を描きます。パソコンとは、シリアル変換のヘッダ J1 (1 = GND、3 = 5V、4 = TXD、5 = RXD、6 = DTR) でつなぎます。条件: (1) 電源 5V (V1 と J1 の 3 番) を、マイコンの VCC (4・6 番) と AVCC (18 番) に、GND (V1 の −、J1 の 1 番) を、マイコンの GND (3・5・21 番) につなぐ。(2) 16MHz の水晶 Y1 を XTAL1 (7 番) と XTAL2 (8 番) の間に入れ、両端と GND の間に 22pF 前後のコンデンサをつける。(3) リセット (29 番) を 10kΩ 前後で電源に引き、J1 の DTR (6 番) から 100nF 前後のコンデンサでつなぐ。(4) 電源 (VCC と GND の間) に、バイパスコンデンサ 0.1µF 前後を 2 個、AREF (20 番) と GND の間にも 1 個つける。(5) 13 番ピン (PB5、17 番) に、1kΩ 前後の抵抗と LED (D1) を直列にして GND へ。(6) J1 の TXD (4 番) をマイコンの RXD (PD0、30 番)、J1 の RXD (5 番) をマイコンの TXD (PD1、31 番) へ (交差)。つなぎ方は、線を引いても、同じ名前のラベルをつけても構いません",
    hints: &[
        "ネットに名前をつけて、ラベルでつなぐと、線を引くより楽です。ラベルツールで、つなぎたいピンに同じ名前 (5V、GND、XTAL1 など) をつけます。同じ名前のところは、線がなくてもつながります。",
        "まず電源から: マイコンの VCC (4・6 番) と AVCC (18 番) を 5V に、GND (3・5・21 番) を GND につなぎます。バイパスコンデンサ (100n) を、VCC と GND の間に 2 個 (電源ピンのそば)、AREF (20 番) と GND の間に 1 個。「電源のバイパスコンデンサ」のレッスンを思い出しましょう。",
        "クロックとリセット: 水晶 Y1 の 1 番を XTAL1 (7 番) に、3 番を XTAL2 (8 番) につなぎ、7 番と GND、8 番と GND の間に 22p のコンデンサをそれぞれ入れます (水晶の 2・4 番はケースで、GND につなぎます)。リセット (29 番) は、10k の抵抗で 5V に引き、100n のコンデンサを通して J1 の 6 番 (DTR) につなぎます。「リセット回路」のレッスンのとおりです。",
        "LED とシリアル: 13 番ピン (PB5、マイコンの 17 番) → 抵抗 1k → LED のアノード、LED のカソードは GND。J1 の 4 番 (TXD) を 30 番 (RXD) に、J1 の 5 番 (RXD) を 31 番 (TXD) へ。送信と受信は交差させます。答えの回路は「答えの回路を見る」で見られます (使わないピンは、そのまま空けて構いません)。",
    ],
    solution: include_str!("../../../../../docs/examples/arduino_minimal.schjson"),
    drop: &[],
    values: NO,
    labels: &[],
    rename: &[],
    parts0: &[
        ("V1", (104, 52)),
        ("U1", (14, 24)),
        ("Y1", (32, 9)),
        ("C1", (46, 8)),
        ("C2", (60, 8)),
        ("C3", (74, 8)),
        ("C4", (88, 8)),
        ("C5", (102, 8)),
        ("C6", (10, 50)),
        ("R1", (24, 50)),
        ("R2", (38, 50)),
        ("D1", (52, 50)),
        ("J1", (62, 50)),
    ],
    parts1: &[("V1", (104, 52)), ("U1", (14, 24)), ("J1", (62, 50))],
    start_values: NO,
    grounds0: &[("V1", "-", 0, 0)],
    grounds1: &[("V1", "-", 0, 0)],
    checks: &[
        Check::Erc,
        Check::Joined {
            label: "電源 5V が、マイコンの VCC (4・6 番) と AVCC (18 番) につながっている",
            groups: &[&[("V1", "+"), ("J1", "3"), ("U1", "4"), ("U1", "6"), ("U1", "18")]],
            hint: "V1 の + と J1 の 3 番 (5V) が、マイコンの VCC 2 本と AVCC に届いているか確かめます。AVCC (18 番) を忘れやすいところです",
        },
        Check::Joined {
            label: "GND が、マイコンの GND (3・5・21 番) と J1 の 1 番につながっている",
            groups: &[&[("V1", "-"), ("J1", "1"), ("U1", "3"), ("U1", "5"), ("U1", "21")]],
            hint: "GND のピンは 3 本あります。1 本でも浮いていると、マイコンは安定して動きません",
        },
        Check::Joined {
            label: "水晶 Y1 の両端が、マイコンの XTAL1 (7 番) と XTAL2 (8 番) につながっている",
            groups: &[&[("Y1", "1"), ("U1", "7")], &[("Y1", "3"), ("U1", "8")]],
            hint: "水晶の 1 番を XTAL1 (7 番)、3 番を XTAL2 (8 番) につなぎます。水晶を置いていないときは、ライブラリの XTAL_16MHz_3225 を置きます",
        },
        Check::Between {
            label: "水晶の両端と GND の間に、22pF 前後のコンデンサがある",
            kind: Kind::Capacitor,
            links: &[
                Link { name: "XTAL1 (7 番) と GND の間", a: ("U1", "7"), b: ("U1", "3"), min: 1, lo: 10e-12, hi: 47e-12 },
                Link { name: "XTAL2 (8 番) と GND の間", a: ("U1", "8"), b: ("U1", "3"), min: 1, lo: 10e-12, hi: 47e-12 },
            ],
            hint: "負荷容量は、水晶のデータシートの値 (CL) から決めます。16MHz の水晶には、22pF が定番です。「水晶とクロック」のレッスンを見直しましょう",
        },
        Check::Between {
            label: "リセット (29 番) が、10kΩ 前後で電源に引かれている",
            kind: Kind::Resistor,
            links: &[Link { name: "リセット (29 番) と VCC (4 番) の間", a: ("U1", "29"), b: ("U1", "4"), min: 1, lo: 4.7e3, hi: 47e3 }],
            hint: "プルアップが無いと、リセットピンが浮いて、勝手にリセットされます。10k が定番です",
        },
        Check::Between {
            label: "DTR (J1 の 6 番) から、100nF 前後のコンデンサでリセットにつながっている",
            kind: Kind::Capacitor,
            links: &[Link { name: "J1 の 6 番 (DTR) とリセット (29 番) の間", a: ("J1", "6"), b: ("U1", "29"), min: 1, lo: 47e-9, hi: 220e-9 }],
            hint: "DTR を直接つなぐと、リセットが Low のままになります。コンデンサを通して、変化だけを伝えます (「リセット回路」のレッスン)",
        },
        Check::Between {
            label: "バイパスコンデンサ (0.1µF 前後) が、電源と GND の間に 2 個、AREF と GND の間に 1 個ある",
            kind: Kind::Capacitor,
            links: &[
                Link { name: "VCC (4 番) と GND の間", a: ("U1", "4"), b: ("U1", "3"), min: 2, lo: 10e-9, hi: 1e-6 },
                Link { name: "AREF (20 番) と GND の間", a: ("U1", "20"), b: ("U1", "3"), min: 1, lo: 10e-9, hi: 1e-6 },
            ],
            hint: "電源のコンデンサは、VCC と AVCC のそれぞれのそばに 1 個 (あわせて 2 個)、AREF にも 1 個です。0.1µF = 100n を使います",
        },
        Check::Between {
            label: "13 番ピン (PB5、17 番) に、1kΩ 前後の抵抗がつながっている",
            kind: Kind::Resistor,
            links: &[Link { name: "17 番ピンと LED のアノードの間", a: ("U1", "17"), b: ("D1", "A"), min: 1, lo: 330.0, hi: 4.7e3 }],
            hint: "ピンから LED に直接つなぐと、電流が大きすぎます。「ピンで LED を光らせる」のレッスンのとおり、抵抗を直列に入れます",
        },
        Check::Joined {
            label: "LED のカソードが GND につながっている",
            groups: &[&[("D1", "K"), ("U1", "3")]],
            hint: "LED は、アノード (+) がピン側、カソード (縞のある側) が GND 側です",
        },
        Check::Joined {
            label: "シリアルの TXD と RXD が交差してつながっている (J1 の 4 番 ↔ 30 番、5 番 ↔ 31 番)",
            groups: &[&[("J1", "4"), ("U1", "30")], &[("J1", "5"), ("U1", "31")]],
            hint: "変換の TXD (送信) は、マイコンの RXD (受信、30 番) へ。変換の RXD は、マイコンの TXD (31 番) へ。同じ名前どうしをつなぐのは、まちがいです",
        },
    ],
};
