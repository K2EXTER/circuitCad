//! The guided lesson for the board editor: from a schematic to the files to order. Like the
//! schematic lessons, each step ticks itself off when the board reaches the matching state.

use super::*;

pub const TITLE: &str = "基板編. 回路図から基板をつくる";
const CIRCUIT: &str = include_str!("../../../../docs/examples/regulator_led.schjson");

/// The second lesson: copper that is too thin, from a finished sample project (`docs/examples/thin_power_line.ccproj`).
pub const COPPER_TITLE: &str = "基板編2. 配線の電圧降下";
pub const COPPER_ID: &str = "board_copper";
const COPPER_PROJECT: &str = include_str!("../../../../docs/examples/thin_power_line.ccproj");

/// The third lesson, a challenge: a board with several faults that the pre-order check finds (`docs/examples/order_fix.ccproj`).
pub const FIX_TITLE: &str = "基板編3. 発注前チェックをすべて通す";
pub const FIX_ID: &str = "board_fix";
const FIX_PROJECT: &str = include_str!("../../../../docs/examples/order_fix.ccproj");

/// The fourth lesson: where to put the crystal and the bypass capacitors, on the minimal Arduino-compatible board
/// (`docs/examples/arduino_minimal.schjson`), and what to do when the router leaves connections open.
pub const ARDUINO_TITLE: &str = "基板編4. 置き方の勘所 (Arduino 互換ボード)";
pub const ARDUINO_ID: &str = "board_arduino";
const ARDUINO_CIRCUIT: &str = include_str!("../../../../docs/examples/arduino_minimal.schjson");

/// The fifth lesson: the USB input of a device (connector, series resistors, a header for the microcontroller): find the
/// differential pairs, route D+ / D- side by side with the same length (`docs/examples/usb_device_input.schjson`).
pub const USB_TITLE: &str = "基板編5. USB の配線 (差動ペア)";
pub const USB_ID: &str = "board_usb";
const USB_CIRCUIT: &str = include_str!("../../../../docs/examples/usb_device_input.schjson");

/// The sixth lesson: the Arduino Mega 2560 compatible board as far as the autorouter gets (18 nets still open): finish it by hand
/// (`docs/examples/work_in_progress/`). The 100-pin chip with a 0.5 mm pitch is the reason the autorouter stops.
pub const MEGA_TITLE: &str = "基板編6. 100 ピンの基板を手で仕上げる (Arduino Mega 互換)";
pub const MEGA_ID: &str = "board_mega";
const MEGA_CIRCUIT: &str = include_str!("../../../../docs/examples/arduino_mega_usb.schjson");
const MEGA_BOARD: &str = include_str!("../../../../docs/examples/work_in_progress/arduino_mega_usb_open.pcbjson");
/// Nets still open on the board of the sixth lesson when it starts.
const MEGA_OPEN_AT_START: usize = 18;

pub(super) struct BoardLesson {
    /// 0 = from a schematic to the files to order, 1 = the copper lesson, 2 = the pre-order check challenge,
    /// 3 = the placement lesson of the Arduino-compatible board, 4 = the USB differential pair lesson,
    /// 5 = finishing the Mega board by hand.
    which: usize,
    done: Vec<bool>,
}

struct Step {
    title: &'static str,
    detail: &'static str,
    done: fn(&BoardEditor) -> bool,
}

fn parts_imported(e: &BoardEditor) -> bool {
    !e.board.parts.is_empty()
}

fn parts_on_board(e: &BoardEditor) -> bool {
    parts_imported(e) && e.board.outline.len() >= 3 && e.board.pads.iter().all(|p| inside_polygon(&e.board.outline, p.pos))
}

fn routed(e: &BoardEditor) -> bool {
    parts_on_board(e) && !e.board.tracks.is_empty() && ratsnest(&e.board).is_empty()
}

fn drc_clean(e: &BoardEditor) -> bool {
    routed(e) && e.drc_ran && e.violations.is_empty()
}

/// Number of nets that still have an unrouted connection.
fn open_nets(e: &BoardEditor) -> usize {
    let mut nets: Vec<String> = ratsnest(&e.board).iter().map(|w| w.net.clone()).collect();
    nets.sort();
    nets.dedup();
    nets.len()
}

/// The Mega board as it was given: DRC has been run, it is clean, and nets are still open (the DRC does not look for those).
fn mega_checked_as_given(e: &BoardEditor) -> bool {
    parts_on_board(e) && e.drc_ran && e.violations.is_empty() && open_nets(e) >= MEGA_OPEN_AT_START
}

/// At least one of the open nets has been routed by hand.
fn mega_one_routed(e: &BoardEditor) -> bool {
    parts_on_board(e) && open_nets(e) < MEGA_OPEN_AT_START
}

/// DRC run again after the hand work, and still clean.
fn mega_still_clean(e: &BoardEditor) -> bool {
    mega_one_routed(e) && e.drc_ran && e.violations.is_empty()
}

fn order_opened(e: &BoardEditor) -> bool {
    drc_clean(e) && e.order.open
}

/// At least two differential pairs are set (D+ / D- at the connector and at the microcontroller side).
fn pairs_found(e: &BoardEditor) -> bool {
    parts_on_board(e) && e.board.diff_pairs.len() >= 2
}

/// Both lines of the pair are routed, run side by side for some length, and their lengths differ by no more than the pair allows.
fn pair_routed_together(e: &BoardEditor, d: &pcb_core::diffpair::DiffPair) -> bool {
    let r = pcb_core::diffpair::pair_report(&e.board, d);
    ratsnest(&e.board).iter().all(|w| w.net != d.p && w.net != d.n) && r.len_p > 0.0 && r.len_n > 0.0 && r.coupled > 0.5 && r.skew <= d.max_skew + 1e-9
}

/// The straight lines of the two nets of a pair (before routing) have about the same length and run in the same direction: the
/// parts are placed so that the pair can run side by side. A pair that is already routed counts as placed.
fn pair_airwires_alike(e: &BoardEditor, d: &pcb_core::diffpair::DiffPair) -> bool {
    let wires = ratsnest(&e.board);
    let of = |net: &str| wires.iter().filter(|a| a.net == net).collect::<Vec<_>>();
    let (p, n) = (of(&d.p), of(&d.n));
    if p.is_empty() && n.is_empty() {
        return true;
    }
    if p.len() != 1 || n.len() != 1 {
        return false;
    }
    let v = |a: &pcb_core::ratsnest::Airwire| (a.b.x - a.a.x, a.b.y - a.a.y);
    let (u, t) = (v(p[0]), v(n[0]));
    let (lu, lt) = (u.0.hypot(u.1), t.0.hypot(t.1));
    lu > 1e-9 && lt > 1e-9 && (lu - lt).abs() <= 1.0 && (u.0 * t.1 - u.1 * t.0).abs() / (lu * lt) < 0.2 && (u.0 * t.0 + u.1 * t.1).abs() / (lu * lt) > 0.9
}

fn pairs_placed_alike(e: &BoardEditor) -> bool {
    pairs_found(e) && e.board.diff_pairs.iter().all(|d| pair_airwires_alike(e, d))
}

fn pairs_routed(e: &BoardEditor) -> bool {
    pairs_found(e) && e.board.diff_pairs.iter().all(|d| pair_routed_together(e, d))
}

/// Distance in mm between the pad `pin` of `refdes` and the pad `pin2` of `refdes2` (None if either is missing).
fn pad_distance(e: &BoardEditor, refdes: &str, pin: &str, refdes2: &str, pin2: &str) -> Option<f64> {
    let at = |r: &str, p: &str| e.board.pads.iter().find(|x| x.refdes == r && x.pin == p).map(|x| x.pos);
    let (a, b) = (at(refdes, pin)?, at(refdes2, pin2)?);
    Some(((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt())
}

/// The crystal's two ends are within 9 mm of the microcontroller's XTAL1 (7) and XTAL2 (8): a short clock line.
fn crystal_near(e: &BoardEditor) -> bool {
    parts_on_board(e) && pad_distance(e, "Y1", "1", "U1", "7").is_some_and(|d| d <= 9.0) && pad_distance(e, "Y1", "3", "U1", "8").is_some_and(|d| d <= 9.0)
}

/// A capacitor of the supply net stands within 7 mm of the microcontroller's VCC (4) and of its AVCC (18) pin.
fn bypass_near(e: &BoardEditor) -> bool {
    if !parts_on_board(e) {
        return false;
    }
    ["4", "18"].iter().all(|pin| {
        let Some(p) = e.board.pads.iter().find(|x| x.refdes == "U1" && x.pin == *pin) else { return false };
        e.board.pads.iter().any(|c| c.refdes.starts_with('C') && c.net == p.net && ((c.pos.x - p.pos.x).powi(2) + (c.pos.y - p.pos.y).powi(2)).sqrt() <= 7.0)
    })
}

/// Where the board tools stop: shown as "実物との違い".
const BOARD_LIMITS: &[&str] = &[
    "自動配置は「叩き台」です。つながりの強い部品を近くに並べるだけで、部品の高さ、コネクタを基板の端に置くこと、放熱、ノイズを出す部品と弱い部品の距離は考えません。",
    "自動配線は、2層・45°の格子の上で、設計ルール (線の間隔) を守る配線だけを採用します。電流が大きい線の太さ (線幅は一律です)、電源とグランドの引き方、ノイズ対策、信号の速さ (インピーダンス) は考えません。大電流や高速の回路は、手で引き直してください。",
    "DRC は、設計ルールを守っているかの機械的な検査です。回路が正しく動くか、ノイズや熱が大丈夫かは調べません。それは回路図のシミュレーションと、実際に作っての確認で調べます。",
    "発注先ごとの最小の線幅・穴径などは目安です。発注の前に、必ず発注先のページで最新の条件を確かめてください。",
];

fn copper_applied(e: &BoardEditor) -> bool {
    e.post_layout_fresh
}

fn currents_declared(e: &BoardEditor) -> bool {
    !e.board.net_currents.is_empty()
}

fn copper_wide_enough(e: &BoardEditor) -> bool {
    let conn = drc::connectivity(&e.board);
    let report = pcb_core::current::net_widths(&e.board, &conn, &pcb_core::current::CopperSpec::default());
    !report.is_empty() && report.iter().all(|w| !w.too_thin())
}

const COPPER_STEPS: &[Step] = &[
    Step {
        title: "配線の抵抗を入れて、電圧が落ちるのを見る",
        detail: "左パネルの「高度な配線」→「配線の太さ (電流から)」を開き、「配線の電圧降下」の「シミュレーションに反映」を押します。そのあと上の「回路図」タブで「解析」→「DC動作点を解析」を押すと、負荷 R1 の + 側が 5 V ではなく約 4.5 V になります。細い配線 (0.25 mm、長さ 60 mm) の抵抗で、電圧が落ちたのです",
        done: copper_applied,
    },
    Step {
        title: "DC解析の電流を入れる",
        detail: "基板に戻り、「配線の太さ (電流から)」の「DC解析の電流を入れる」を押します。DC解析で求めた電流 (約 2.3 A) が、VIN と GND の欄に入ります。必要な線幅 (約 1.1 mm) と、今の線幅 (0.25 mm) が赤く出ます",
        done: currents_declared,
    },
    Step {
        title: "足りない配線を太くする",
        detail: "「足りないネットをすべて推奨幅にする」を押します。配線が必要な太さになります。太くしたら、間隔が足りているか、「DRC を実行」でも確かめましょう",
        done: copper_wide_enough,
    },
    Step {
        title: "もう一度反映して、電圧が戻るのを確かめる",
        detail: "もう一度「シミュレーションに反映」を押し、回路図で「DC動作点を解析」を押します。R1 の + 側が約 4.9 V に戻ります。残りの差は、まだ配線にある抵抗です。もっと太く・短くすれば、さらに小さくなります",
        done: copper_applied,
    },
];

fn parts_inside(e: &BoardEditor) -> bool {
    e.board.outline.len() >= 3 && !e.board.parts.is_empty() && e.board.pads.iter().all(|p| inside_polygon(&e.board.outline, p.pos))
}

/// The pre-order check of the chosen fab has no error and no warning (the notes may stay), and a real fab (not the generic one) is chosen.
fn order_clean(e: &BoardEditor) -> bool {
    e.order.fab_id != "standard" && !pcb_core::fab::preflight(&e.board, e.selected_fab()).iter().any(|c| matches!(c.level, pcb_core::fab::Level::Error | pcb_core::fab::Level::Warn))
}

const FIX_STEPS: &[Step] = &[
    Step {
        title: "発注の準備を開いて、赤い ✖ と黄色い ▲ を読む",
        detail: "「発注の準備…」を押して、「2. 発注前のチェック」を見ます。✖ は直さないと発注できない所、▲ は発注はできるけれど、たぶん意図と違う所です。項目をクリックすると、基板のその場所に飛びます。この基板には、わざと直す所がいくつもあります",
        done: |e| e.order.open,
    },
    Step {
        title: "基板の外にある部品を、基板の中へ入れる",
        detail: "基板の外 (右の方) に D1 があります。ドラッグして、基板の中の空いた所へ置きます。置いたら、D1 につながっていた線が切れているので、黄色の線 (未配線) が出ます",
        done: parts_inside,
    },
    Step {
        title: "未配線をつなぐ",
        detail: "「自動配線 → 全部」を押すか、「配線」ツールで黄色の線をなぞります。つながらないときは、D1 の位置を少し変えてやり直します。「発注の準備」の「もう一度チェック」で、✖ が消えたか確かめましょう",
        done: routed,
    },
    Step {
        title: "電流に対して細い配線を太くする",
        detail: "この基板には「V1 の + 側 (N$1) とGNDに 1.5 A 流れる」という電流が設定してあります。左の「配線の太さ (電流から)」を開き、「足りないネットをすべて推奨幅にする」を押します。新しく引いた配線も、細いままなら太くします。太くしたあと、間隔が足りているか、DRC も確かめましょう",
        done: copper_wide_enough,
    },
    Step {
        title: "発注先を選び、ルールを合わせて、▲ と ✖ をなくす",
        detail: "「発注の準備」で発注先 (例: JLCPCB) を選び、「ルールをこの発注先に合わせる」を押します。そのあと「もう一度チェック」を押して、✖ と ▲ がなくなれば完成です (ℹ の豆知識は残っていて構いません)。DRC の違反が出たら、その場所を直します",
        done: order_clean,
    },
];

const ARDUINO_STEPS: &[Step] = &[
    Step {
        title: "回路図から取り込む",
        detail: "左パネルの「回路図から取り込み」を押します。ATmega328P (U1)・水晶 (Y1)・コンデンサ・LED・ヘッダなど 16 個の部品が並び、つなぐべき所に黄色の線 (未配線) が出ます。",
        done: parts_imported,
    },
    Step {
        title: "部品を並べて、基板の外形を決める",
        detail: "「自動配置 → 部品を並べて外形を決める」を押します。つながりの強い部品が近くなるように並びますが、叩き台です。次の 2 つは、自動配置が考えてくれないので、人が直します。",
        done: parts_on_board,
    },
    Step {
        title: "水晶 Y1 を、マイコンの XTAL ピンのそばへ動かす",
        detail: "水晶は、マイコンの XTAL1 (7 番)・XTAL2 (8 番) のすぐそばに置きます。配線が長いと、浮遊容量が増えて、発振が不安定になります。U1 の左下にある 7・8 番のパッドを探し、Y1 をドラッグして、水晶の両端がパッドから 9 mm 以内 (できれば 5 mm 以内) になるようにします。R キーで向きを回すと、両端がそろいます。",
        done: crystal_near,
    },
    Step {
        title: "バイパスコンデンサを、電源ピンのそばへ動かす",
        detail: "バイパスコンデンサ (100n) は、IC の電源ピンのすぐそばに置きます。自動配置が、U1 の VCC (4 番) と AVCC (18 番) のそばに、コンデンサ (C3・C4) を置いています。それぞれ 7 mm 以内か、確かめましょう。遠いものがあれば、近くへドラッグします。遠いと、配線のインダクタンスで、高い周波数に効きません (レッスン「電源のバイパスコンデンサ」)。",
        done: bypass_near,
    },
    Step {
        title: "配線する (グリッドを細かくしてから)",
        detail: "この基板は、IC の足の間隔が狭い (0.8 mm) ので、標準のグリッド (0.5 mm) のままだと、1〜2 本が通らないことがあります。左パネルの「グリッド」を 0.1 mm にしてから、「自動配線 → 全部」を押します。細かい格子ほど、狭い所を通せます。先に標準のまま配線して、通らない線が残ったときは、Ctrl+Z で配線を取り消してから、グリッドを細かくしてやり直します (残した配線が邪魔をして、あとからでは通らないことがあります)。それでも残るときは、「配線」ツールで手で引きます。",
        done: routed,
    },
    Step {
        title: "DRC で配線のルール違反を調べる",
        detail: "「DRC を実行」を押します。違反が出たら、一覧をクリックして、その場所を直します。0 件になるまで繰り返します。",
        done: drc_clean,
    },
    Step {
        title: "発注の準備を開く",
        detail: "「発注の準備…」を押します。発注先を選び、チェックと出来上がりの確認 (ガーバーの読み戻し) をします。バイパスコンデンサの距離も、チェックの項目です。実際に注文するときは、発注先のページで最新の条件を確かめてください。",
        done: order_opened,
    },
];

const USB_STEPS: &[Step] = &[
    Step {
        title: "回路図から取り込む",
        detail: "左パネルの「回路図から取り込み」を押します。USB コネクタ (J1)・直列抵抗 (R1・R2、22Ω)・コンデンサ (C1)・マイコンの代わりのヘッダ (J2) が並びます。D+ と D− には、USB_DP / USB_DM (コネクタ側) と MCU_DP / MCU_DM (マイコン側) という名前がついています。",
        done: parts_imported,
    },
    Step {
        title: "部品を並べて、基板の外形を決める",
        detail: "「自動配置 → 部品を並べて外形を決める」を押します。USB の基板では、コネクタを基板の端に置き、マイコンの近くに D+ / D− の直列抵抗を置きます。自動配置は、そこまでは考えてくれないので、気になる部品はドラッグで動かします (R キーで回転)。",
        done: parts_on_board,
    },
    Step {
        title: "差動ペアを作る",
        detail: "左パネルの「差動ペア」を開き、「名前から探す」を押します。USB_DP / USB_DM と MCU_DP / MCU_DM が、2 組のペアとして見つかります。USB の D+ と D− は、逆の向きに動く 1 組の信号なので、ペアとして扱います。",
        done: pairs_found,
    },
    Step {
        title: "D+ と D− が並んで走れるように、部品を並べ直す",
        detail: "2 本を並べて引くには、2 本の「つなぐ線」(黄色の線) が、同じ向きで、同じ長さになるように、部品を置きます。自動配置は、そこまで考えてくれないので、人が直します。やり方: コネクタ J1 の D− (2 番)・D+ (3 番) の真下に、R2 と R1 を、縦向きに (R キーで回転) 並べます。その下に、ヘッダ J2 を、横向きに、2 番・3 番ピンが R2・R1 の下に来るように置きます。黄色の線が、ペアの 2 本で、平行で同じ長さになれば、次に進めます (「並走できる」と判定されます)。",
        done: pairs_placed_alike,
    },
    Step {
        title: "ペアで配線する",
        detail: "差動ペアの 1 組ごとに、「ペアで配線」を押します。＋側 (D+) を引いてから、− 側 (D−) が、そのすぐ隣を並んで引かれます (並べ方が悪いと、並走 0% のままになります。その場合は、前の手順に戻って、部品の置き方を直します)。2 本の長さの差 (「長さの差」の表示) が、許容値 (0.5mm) 以内で、「✔ 長さが合っています」と出るまで確かめます。差が大きいときは、「長さを合わせる」で、短いほうに蛇行を足します。USB のフルスピードは、数 mm の差なら問題になりませんが、2 本を並べて同じ長さで引くのが、基本です。",
        done: pairs_routed,
    },
    Step {
        title: "残りの配線をする",
        detail: "「自動配線 → 全部」を押して、VBUS と GND を引きます。D+ / D− のペアは、すでに引いてあるので、そのままです。通らない線が残ったら、部品の位置を動かします。",
        done: routed,
    },
    Step {
        title: "DRC で配線のルール違反を調べる",
        detail: "「DRC を実行」を押します。差動ペアも調べられ、「2 本の間隔が、目標の 0.20mm からずれています」と出ます。コネクタのパッドは 2mm 離れているので、パッドを出たあとも、線の間隔が約 1.75mm のままだからです。理想は、パッドを出たらすぐに、0.2〜0.3mm まで 2 本を寄せることです (「配線」ツールで手で引き直せます)。ここでは、差動ペアの「間隔」欄を、実際の間隔 (1.75mm) に合わせて、違反をなくします。間隔が広いと、差動インピーダンスは 90Ω より高くなりますが、フルスピード (12Mbit/s) の USB は、短い配線なら動くことが多いです。ハイスピードでは、寄せる必要があります。違反が 0 件になるまで直します。",
        done: drc_clean,
    },
    Step {
        title: "発注の準備を開く",
        detail: "「発注の準備…」を押します。実際の USB 機器では、D+ / D− の差動インピーダンス (90Ω が目安) は、基板の厚み・銅の層・線幅・2 本の間隔で決まります。2 層の標準の基板で、これを合わせるのは難しいので、発注先の「インピーダンスの計算ツール」で、線幅と間隔を確かめます。低速 (フルスピード以下) の USB なら、ここまでの「並べて、同じ長さで、短く」で、ほとんどの場合動きます。",
        done: order_opened,
    },
];

const MEGA_STEPS: &[Step] = &[
    Step {
        title: "いまの基板の状態を確かめる",
        detail: "この基板は、自動配置と自動配線で、通るところまで配線したものです。左パネルの「DRC を実行」を押すと、違反は 0 件です。でも、基板の上には黄色の線 (未配線) が残っています。左パネルの「未配線」で数えると、18 本のネットが残っています。DRC は「引いた配線がルールを守っているか」を調べるもので、「引き忘れ」は調べません。引き忘れは、未配線の表示と、発注前チェックで見つけます。",
        done: mega_checked_as_given,
    },
    Step {
        title: "未配線を 1 本、手で通す",
        detail: "黄色の線の短いものから選びます (DTR・MOSI・PA0 など)。「配線」ツールで、パッドからパッドへクリックして引きます。通れないときは、「ビア」ツールで裏面へ渡します。邪魔をしている別の線は、一度消して (その線を選んで Delete)、先に通したい線を引いてから、消した線を引き直します (自動配線の「選んだネットだけ」も使えます)。ヘッダなどの部品を少し動かすだけで通ることもあります。0.5mm ピッチの IC の足の間は狭いので、拡大して、グリッドを 0.1mm や 0.05mm に細かくして引くと、置きやすくなります。1 本通ると、次に進めます。",
        done: mega_one_routed,
    },
    Step {
        title: "DRC を、もう一度実行して、0 件のままか確かめる",
        detail: "手で配線を引いたり、部品を動かしたりすると、思わぬ所で線どうしが近づきすぎることがあります。「DRC を実行」を押して、違反が 0 件のままか確かめます。違反が出たら、一覧をクリックしてその場所を直します。そのあとも、残りの未配線を少しずつ通していけます (すべて通すのは、時間のかかる作業です。1 本通すごとに DRC を確かめる習慣をつけましょう)。",
        done: mega_still_clean,
    },
];

const STEPS: &[Step] = &[
    Step {
        title: "回路図から取り込む",
        detail: "左パネルの「回路図から取り込み」を押します。回路図の部品が基板の右側に並び、部品どうしをつなぐべき所に黄色の線 (未配線、ラッツネスト) が出ます。回路図の「つながり」が、そのまま基板の配線の指示になります。",
        done: parts_imported,
    },
    Step {
        title: "部品を並べて、基板の外形を決める",
        detail: "「自動配置 → 部品を並べて外形を決める」を押します。つながりの強い部品が近くなるように並び、外形も部品に合わせて決まります。叩き台なので、気になる部品はドラッグで動かしてみましょう (R キーで回転)。黄色の線が短くなるように置くのがコツです。",
        done: parts_on_board,
    },
    Step {
        title: "配線する",
        detail: "「自動配線 → 全部」を押します。黄色の線が銅の配線 (赤が表面、青が裏面) に変わります。手で引くなら「配線」ツールでパッドからパッドへクリックします。通せなかった線が残ったら、部品を動かしてからやり直しましょう。",
        done: routed,
    },
    Step {
        title: "DRC で配線のルール違反を調べる",
        detail: "「DRC を実行」を押します。線が近すぎる、基板の端にはみ出している、などがないか調べます。違反が出たら、一覧をクリックするとその場所に飛びます。直して、0 件になるまで繰り返します。",
        done: drc_clean,
    },
    Step {
        title: "発注の準備を開く",
        detail: "「発注の準備…」を押します。発注先を選び、チェックと出来上がりの確認 (書き出すガーバーファイルを読み戻した絵) をして、製造ファイルを書き出せます。ここまで来れば、回路図から実際の基板を注文するところまでの流れがつながりました。",
        done: order_opened,
    },
];

impl BoardEditor {
    pub fn start_lesson(&mut self) {
        self.new_file();
        self.drc_ran = false;
        self.lesson = Some(BoardLesson { which: 0, done: vec![false; STEPS.len()] });
        self.status = format!("レッスン「{TITLE}」を始めました。右の案内に沿って操作してみましょう");
    }

    /// Starts the placement lesson of the Arduino-compatible board (the app loads its circuit first).
    pub fn start_arduino_lesson(&mut self) {
        self.new_file();
        self.drc_ran = false;
        self.lesson = Some(BoardLesson { which: 3, done: vec![false; ARDUINO_STEPS.len()] });
        self.status = format!("レッスン「{ARDUINO_TITLE}」を始めました。右の案内に沿って操作してみましょう");
    }

    /// Starts the USB differential pair lesson (the app loads its circuit first).
    pub fn start_usb_lesson(&mut self) {
        self.new_file();
        self.drc_ran = false;
        self.lesson = Some(BoardLesson { which: 4, done: vec![false; USB_STEPS.len()] });
        self.status = format!("レッスン「{USB_TITLE}」を始めました。右の案内に沿って操作してみましょう");
    }

    /// The sample circuit of the USB lesson.
    pub fn usb_lesson_circuit() -> &'static str {
        USB_CIRCUIT
    }

    /// The sample circuit of the placement lesson.
    pub fn arduino_lesson_circuit() -> &'static str {
        ARDUINO_CIRCUIT
    }

    /// The project (schematic and board) the copper lesson works on.
    pub fn copper_lesson_project() -> &'static str {
        COPPER_PROJECT
    }

    /// Starts the copper lesson on the board that was just opened (the app loads the project first).
    pub fn start_copper_lesson(&mut self) {
        self.board.net_currents.clear();
        self.post_layout_fresh = false;
        self.drc_ran = false;
        self.lesson = Some(BoardLesson { which: 1, done: vec![false; COPPER_STEPS.len()] });
        self.status = format!("レッスン「{COPPER_TITLE}」を始めました。右の案内に沿って操作してみましょう");
    }

    /// The project (schematic and board) the pre-order check challenge works on.
    pub fn fix_lesson_project() -> &'static str {
        FIX_PROJECT
    }

    /// Starts the pre-order check challenge on the board that was just opened (the app loads the project first).
    pub fn start_fix_lesson(&mut self) {
        self.post_layout_fresh = false;
        self.drc_ran = false;
        self.order = order::OrderState::default();
        self.lesson = Some(BoardLesson { which: 2, done: vec![false; FIX_STEPS.len()] });
        self.status = format!("チャレンジ「{FIX_TITLE}」を始めました。右の案内に沿って、発注前チェックの赤と黄色をなくしましょう");
    }

    /// The sample circuit of the Mega lesson.
    pub fn mega_lesson_circuit() -> &'static str {
        MEGA_CIRCUIT
    }

    /// The half-routed board of the Mega lesson (JSON).
    pub fn mega_lesson_board() -> &'static str {
        MEGA_BOARD
    }

    /// Starts the Mega lesson on the board that was just adopted (the app loads the circuit and the board first).
    pub fn start_mega_lesson(&mut self) {
        self.drc_ran = false;
        self.lesson = Some(BoardLesson { which: 5, done: vec![false; MEGA_STEPS.len()] });
        self.status = format!("レッスン「{MEGA_TITLE}」を始めました。右の案内に沿って操作してみましょう");
    }

    fn lesson_steps(&self) -> &'static [Step] {
        match self.lesson.as_ref().map(|l| l.which) {
            Some(1) => COPPER_STEPS,
            Some(2) => FIX_STEPS,
            Some(3) => ARDUINO_STEPS,
            Some(4) => USB_STEPS,
            Some(5) => MEGA_STEPS,
            _ => STEPS,
        }
    }

    /// The sample circuit this lesson imports.
    pub fn lesson_circuit() -> &'static str {
        CIRCUIT
    }

    /// The progress key of the running board lesson once every step is ticked.
    pub fn finished_board_lesson(&self) -> Option<&'static str> {
        let l = self.lesson.as_ref().filter(|l| l.done.iter().all(|d| *d))?;
        Some(match l.which {
            1 => COPPER_ID,
            2 => FIX_ID,
            3 => ARDUINO_ID,
            4 => USB_ID,
            5 => MEGA_ID,
            _ => "board",
        })
    }

    #[cfg(test)]
    pub fn lesson_progress(&mut self) -> usize {
        self.update_lesson();
        self.lesson.as_ref().map_or(0, |l| l.done.iter().filter(|d| **d).count())
    }

    /// What the learner does in the order window: pick the fab, make the rules match it, check again.
    #[cfg(test)]
    pub fn choose_fab_for_test(&mut self, id: &str) {
        self.order.fab_id = id.into();
        if let Some(f) = self.selected_fab() {
            self.apply_fab(f);
        }
        self.run_checks();
    }

    fn update_lesson(&mut self) {
        let Some(l) = self.lesson.as_ref() else { return };
        let Some(cur) = l.done.iter().position(|d| !d) else { return };
        if (self.lesson_steps()[cur].done)(self)
            && let Some(l) = self.lesson.as_mut() {
                l.done[cur] = true;
            }
    }

    pub fn lesson_window(&mut self, ctx: &egui::Context) {
        self.update_lesson();
        let Some(l) = self.lesson.as_ref() else { return };
        let done = l.done.clone();
        let copper = l.which == 1;
        let fix = l.which == 2;
        let arduino = l.which == 3;
        let usb = l.which == 4;
        let mega = l.which == 5;
        let steps = self.lesson_steps();
        let cur = done.iter().position(|d| !d);
        let mut close = false;
        egui::Window::new(format!("レッスン: {}", if copper { COPPER_TITLE } else if fix { FIX_TITLE } else if arduino { ARDUINO_TITLE } else if usb { USB_TITLE } else if mega { MEGA_TITLE } else { TITLE })).default_width(360.0).anchor(egui::Align2::LEFT_BOTTOM, Vec2::new(268.0, -56.0)).show(ctx, |ui| {
            ui.label(if mega {
                "ATmega2560 (100 ピン、0.5mm ピッチ) の基板です。自動配線は、通せる所まで配線して、18 本のネットを残しました。残りを、手で引く・ビアで渡す・邪魔な線を引き直す・部品を動かす、で仕上げる練習です"
            } else if usb {
                "USB の D+ と D− は、2 本で 1 つの信号を運ぶ「差動ペア」です。並べて、同じ長さで、短く引きます。差動ペアの機能で、2 本を並べて配線し、長さの差を確かめます"
            } else if arduino {
                "ATmega328P の Arduino 互換ボードを基板にします。自動配置は叩き台なので、水晶を、人が正しい場所へ寄せます (バイパスコンデンサは、自動配置が電源ピンのそばに置きますが、確かめます)。そのあと配線し、通らない線が残ったときの直し方も身につけます"
            } else if fix {
                "発注前チェックに引っかかる基板です。部品が基板の外にある、配線が切れている、電流に対して配線が細い、発注先のルールに合っていない、という4つを、チェックの表示を頼りに自分で直します"
            } else if copper {
                "電源と負荷が遠く、配線が細い基板です。配線の抵抗で電圧がどれだけ落ちるかを、シミュレーションで見て、電流から必要な太さを決めて直します"
            } else {
                "回路図で描いた回路を、基板のパターンにして、発注用のデータを作るところまでを一通り体験します"
            });
            ui.small(format!("進み具合: {} / {}", done.iter().filter(|d| **d).count(), done.len()));
            ui.separator();
            for (k, step) in steps.iter().enumerate() {
                let (mark, color) = if done[k] {
                    ("✔", Color32::from_rgb(60, 160, 90))
                } else if Some(k) == cur {
                    ("▶", Color32::from_rgb(220, 150, 40))
                } else {
                    ("○", Color32::from_gray(140))
                };
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, mark);
                    ui.label(egui::RichText::new(step.title).strong().color(if Some(k) == cur { color } else { ui.visuals().text_color() }));
                });
                if Some(k) == cur {
                    ui.indent(("board_lesson_detail", k), |ui| {
                        ui.label(step.detail);
                    });
                }
            }
            ui.separator();
            if !copper && !fix && !arduino && !usb && !mega {
            egui::CollapsingHeader::new("できること・できないこと (ツールの限界)")
                .id_salt(("board_limits", cur.is_none()))
                .default_open(cur.is_none())
                .show(ui, |ui| {
                    ui.small("自動の機能は便利ですが、万能ではありません。次の点は、人が確かめます");
                    for l in BOARD_LIMITS {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("・");
                            ui.label(*l);
                        });
                    }
                });
            }
            if cur.is_none() && copper {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "レッスン完了!");
                ui.label("配線は銅の抵抗を持ち、電流が大きいほど、細く長いほど、電圧が落ちます。流れる電流から必要な太さを決め (IPC-2221)、シミュレーションで確かめる、という順で考えます。この見積もりはDCの電流だけで、スイッチングの瞬間の大きな電流や、配線のインダクタンスは入っていません。余裕を見て、実機でも測りましょう。");
            } else if cur.is_none() && fix {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "チャレンジ達成!");
                ui.label("発注前チェックの ✖ と ▲ がなくなりました。このチェックは「機械的に見つかる間違い」を調べるもので、回路が正しく動くことは保証しません。発注の前に、回路図のシミュレーションと、発注先のページの最新の条件を、もう一度確かめましょう。");
            } else if cur.is_none() && mega {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "レッスン完了!");
                ui.label("自動配線が通せなかった線は、人が仕上げます。足の間が狭い IC では、「先に通したい線から引く」「邪魔な線を一度消して引き直す」「ビアで裏面へ渡す」「部品の位置を直す」の順に考えます。発注の前には、未配線が 0 本で、DRC が 0 件であることを、必ず確かめてください (未配線のまま発注すると、基板は動きません)。");
            } else if cur.is_none() && usb {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "レッスン完了!");
                ui.label("USB の配線の基本は、「D+ と D− を並べて、同じ長さで、短く」「ビアや枝分かれを避ける」「ESD 保護部品はコネクタのそば」「下の層を GND のベタにする」です。高速 (ハイスピード 480Mbit/s) では、差動インピーダンス 90Ω の設計が必須で、基板の積層を決めてから配線します。");
            } else if cur.is_none() && arduino {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "レッスン完了!");
                ui.label("マイコンの基板では、「水晶は近く」「バイパスコンデンサは電源ピンのそば」「通らない線は、グリッドを細かくするか、手で引く」の 3 つが基本です。ほかにも、GND はベタで広く取る、リセットやクロックの線にノイズの源 (スイッチング電源など) を近づけない、があります。実機で動かないときは、まずこれらを疑いましょう。");
            } else if cur.is_none() {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "レッスン完了!");
                ui.label("基板づくりの流れは「回路図 → 取り込み → 配置 → 配線 → DRC → 発注データ」です。DRC に通らないと発注データは書き出せないので、配線のあとは必ず DRC を実行する習慣をつけましょう。実際に注文するときは、発注先のページで最新の条件を確かめてください。");
            }
            ui.separator();
            if ui.button("閉じる").clicked() {
                close = true;
            }
        });
        if close {
            self.lesson = None;
        }
    }
}
