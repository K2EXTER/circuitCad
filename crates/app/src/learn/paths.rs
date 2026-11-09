//! The learning paths themselves: the items that several paths share, then each path (stages and items).

use super::*;

pub(super) const fn lesson(id: &'static str, title: &'static str, about: &'static str, real: Option<&'static str>) -> Item {
    Item { id, title, about, kind: Kind::Lesson(id), build: false, requires: None, real }
}

// shown on the map before it is written (none is planned right now)
#[allow(dead_code)]
pub(super) const fn planned(id: &'static str, title: &'static str, about: &'static str, real: Option<&'static str>) -> Item {
    Item { id, title, about, kind: Kind::Planned, build: false, requires: None, real }
}

// ------------------------------------------------------------------ items that several paths share
// (the record is kept by the item's id, so a lesson finished in one path is finished in the others; a path sets its own `requires`)

pub(super) const I_BJT_SWITCH: Item = lesson("bjt_switch", "トランジスタのスイッチ", "ベース電流でコレクタ電流を制御する。電流を増やす部品の入口", Some("トランジスタで LED をオン・オフし、ベース電流とコレクタ電流を測ります"));
pub(super) const I_COIN_CELL: Item = lesson("coin_cell", "電池の内部抵抗 (負荷で電圧が下がる)", "電源の電圧が、電流を流すと下がる。これを小さくするのが安定化電源の役目", Some("電池に抵抗をつないで、つなぐ前後の端子電圧を測り、内部抵抗を求めます"));
pub(super) const I_PROTECTION: Item = lesson("protection", "電源を守る (逆接続・過電流・ヒューズ)", "壊れ方を知って、守る部品を足す", Some("ヒューズや逆接続保護ダイオードの働きを、安全な条件 (低電圧・電流制限つき) で確かめます"));
pub(super) const I_LDO: Item = lesson("ldo", "LDO (低ドロップアウト) レギュレータ", "入力の余裕 (ドロップアウト電圧) が小さくても動く。USB の 5V や電池から 3.3V を作る。電圧差は熱になる", Some("入力を下げながら出力を測り、出力が落ち始める入力を、データシートと比べます"));
pub(super) const I_HEAT: Item = lesson("heat", "熱と定格を考える", "電圧差 × 電流 = 熱。放熱板が必要になる理由。定格チェックの見方", Some("負荷を増やしながらレギュレータの温度を測り (触って確かめる前に、温度計で)、計算と比べます"));
pub(super) const I_BUCK: Item = lesson("buck","降圧コンバータ (スイッチング電源)", "発展: 熱にしない電源。リニアとの効率の違い", None);
pub(super) const I_ELECTROLYTIC_LIFE: Item = lesson("electrolytic_life", "電解コンデンサの寿命 (リップル電流と温度)", "ESR の発熱で内部が温まる、寿命は 10℃ で半分、定格リップル電流を超えない", Some("電解コンデンサを触って温度を感じ、データシートの定格リップルと寿命から、使う条件の寿命を見積もります"));
pub(super) const I_DIODE_RECOVERY: Item = lesson("diode_recovery", "ダイオードの逆回復 (切り替えの瞬間に逆向きに流れる)", "順方向のとき貯めた電荷が抜けるまで逆向きに流れる、速い品とショットキー、スイッチング電源の損失とノイズ", Some("整流ダイオードの種類 (1N4007、高速、ショットキー) をデータシートの trr で見比べます"));
pub(super) const I_SWITCHING_LOSS: Item = lesson("switching_loss", "MOSFET のスイッチング損失 (ゲートの速さと熱)", "ゲート容量の充電で遅れる、ミラー期間、切り替え中の V × I が熱になる、損失 = 1 回あたりのエネルギー × 周波数", Some("ゲート抵抗を変えて、ドレインの立ち下がりの速さと、MOSFET の温度の違いを見ます"));
pub(super) const I_LDO_VS_BUCK: Item = lesson("ldo_vs_buck", "リニアとスイッチングの比較 (熱にするか、貯めて運ぶか)", "リニアの効率 = 出力 ÷ 入力の電圧、損失は (Vin − Vout) × I、スイッチングは高い効率で、リップルと部品が増える", Some("入力と出力の電圧と電流を測って、効率を計算し、リニアとスイッチングの温度を比べます"));
pub(super) const I_SLEW_RATE: Item = lesson("slew_rate", "オペアンプのスルーレート (出力が追いつけない速さ)", "出力の電圧の変化の速さには上限 (V/µs) がある、大きく速い信号は三角波にゆがむ、フルパワー帯域 = SR ÷ (2π Vp)", Some("オペアンプのボルテージフォロワに大きさと周波数を変えた正弦波を入れ、出力の形がゆがむ周波数を測ります"));
pub(super) const I_EEPROM_BASICS: Item = lesson("eeprom_basics", "EEPROM (I²C) の素顔 (オープンドレインとプルアップ)", "バスは引き下げだけ・上げるのは抵抗、立ち上がり = 0.85 RC、プルアップの上限と下限、書き込みに数 ms", Some("I²C のバス (SDA) をオシロで見て、プルアップの値を変えながら、立ち上がりの形を比べます"));
pub(super) const I_FLYBACK: Item = lesson("flyback", "リレーやモータをつなぐ (フライバックダイオード)", "コイルの電流を急に切ると、大きな逆起電力が出てトランジスタを壊す。コイルと並列にダイオードを入れて止める", Some("リレー (または小さなモータ) をトランジスタで駆動し、ダイオードの向きを確かめます"));
pub(super) const I_PWM: Item = lesson("pwm", "PWM とフィルタ (analogWrite でアナログ電圧を作る)", "High と Low の割合 (デューティ) と RC フィルタで、途中の電圧を作る。揺れと反応の速さは引き換え。重い負荷には使えない", Some("analogWrite の値を変えて、フィルタの出力をテスタで測ります"));
pub(super) const I_BOARD: Item = Item { id: "board", title: "回路図から基板をつくる", about: "取り込み・配置・配線・DRC・発注の準備", kind: Kind::Board(0), requires: Some(&["draw_psu"]), build: false, real: Some("発注するなら、少量・少額で試します") };
pub(super) const I_BOARD_COPPER: Item = Item { id: "board_copper", title: "配線の太さと電圧降下を確かめる", about: "細い配線で電圧が落ちるのをシミュレーションで見て、電流から必要な太さを決めて直す", kind: Kind::Board(1), requires: Some(&["board"]), build: false, real: Some("電源の入口と負荷のピンの電圧を、テスタで測って比べます (大きな電流を流すときは、配線や部品の発熱に注意します)") };
pub(super) const I_BOARD_FIX: Item = Item { id: "board_fix", title: "発注前チェックをすべて通す (チャレンジ)", about: "部品が基板の外・配線が切れている・配線が細い・発注先のルール違反、という基板を、チェックの表示を頼りに自分で直す", kind: Kind::Board(2), requires: Some(&["board"]), build: false, real: Some("発注の前に、発注先のページで最新の条件を必ず確かめます") };
pub(super) const I_BOARD_ARDUINO: Item = Item { id: "board_arduino", title: "基板にする: 置き方の勘所と配線", about: "水晶はマイコンのそば・バイパスコンデンサは電源ピンのそば。通らない線は、グリッドを細かくして直す。DRC と発注の準備まで", kind: Kind::Board(3), build: false, requires: Some(&["draw_arduino", "board"]), real: Some("発注するなら、少量・少額で試します") };
pub(super) const I_BOARD_USB: Item = Item { id: "board_usb", title: "USB の配線 (差動ペア)", about: "D+ と D− を並べて、同じ長さで、短く引く。差動ペアの機能と、長さの差の確かめ方", kind: Kind::Board(4), build: false, requires: Some(&["usb_signal", "board"]), real: None };
pub(super) const I_BOARD_PAIRS: Item = Item { id: "board_pairs", title: "差動ペアどうしの長さを合わせる", about: "2 レーンの差動信号を同時に届けるため、ペアどうしの長さを蛇行で揃える。2 本をいっしょに曲げるので、間隔が保たれる", kind: Kind::Board(6), build: false, requires: Some(&["board_usb"]), real: None };
pub(super) const I_BOARD_MEGA: Item = Item { id: "board_mega", title: "基板を手で仕上げる (自動配線で通らない線)", about: "自動配置と自動配線のあと、18 本のネットが残った基板を、手で引く・ビアで渡す・邪魔な線を引き直す、で仕上げる。1 本通すごとに DRC を確かめる", kind: Kind::Board(5), build: false, requires: Some(&["board_arduino"]), real: None };
pub(super) const I_BOARD_REPAIR: Item = Item { id: "board_repair", title: "壊れた基板を直す (修理屋)", about: "ちゃんと動く基板に故障を 1 つ仕込み、症状から原因を探して直す。バイパスコンデンサが遠い・配線が切れている・細い・ショート・部品が基板の外。何度でも、別の故障で練習できる", kind: Kind::Board(7), build: false, requires: Some(&["board_fix"]), real: None };
pub(super) const I_MOSFET: Item = lesson("mosfet", "MOSFET のスイッチ (ゲート電圧とオンの深さ)", "電圧で駆動する。ゲート電圧がしきい値のあたりでオンになる。電流を流しすぎると発熱する", None);
pub(super) const I_PNP_HIGHSIDE: Item = lesson("pnp_highside", "PNP のハイサイドスイッチ (電源側で入り切り)", "NPN と逆に、入力を Low にするとオンになる。ベース抵抗の大きさが、オンの深さを決める", None);
pub(super) const I_TP4056: Item = lesson("tp4056", "リチウムイオン電池の充電 (TP4056)", "充電の 3 段階 (予備充電・定電流・定電圧)。IC の内部は見ず、外から見た動きで確かめる", None);

pub(super) const I_OPAMP_NONINV: Item = lesson("opamp_noninv", "オペアンプで信号を大きくする (非反転増幅)", "帰還の考え方。抵抗の比がゲインを決める", Some("オペアンプの増幅回路を作り、入力と出力をテスタやオシロで測ります"));
pub(super) const I_OPAMP_INV: Item = lesson("opamp_inv", "オペアンプの反転増幅 (仮想接地)", "反転入力が 0V に保たれる仕組み", None);
pub(super) const I_RC_LOWPASS: Item = lesson("rc_lowpass", "RC ローパスフィルタ", "高い周波数だけを弱める。カットオフ周波数は 1 ÷ (2π R C)。抵抗とコンデンサの値で決まる", None);
pub(super) const I_RLC: Item = lesson("rlc", "行き過ぎとリンギング (RLC 回路)", "コイルとコンデンサの回路は、ステップに対して行き過ぎて揺れる。抵抗で抑える", None);
pub(super) const I_HIERARCHY: Item = lesson("hierarchy", "同じ回路を何回も使う (階層)", "使い回したい回路を別のシートに1つ描き、記号 (箱) にして何回も置く。中を直すと全部に効く。コピーごとに部品とネットの名前が別になる", None);
pub(super) const I_NE555: Item = lesson("ne555", "555 で LED を点滅させる", "タイマー IC とコンデンサの充放電で、一定の間隔で点滅する。C や R を変えると、周期が変わる", None);
pub(super) const I_POLES: Item = lesson("poles", "極と零 (RLC の共振を式で見る)", "山の位置は極の周波数、鋭さは Q。抵抗を大きくすると、極が実数になって山が消える", None);
pub(super) const I_NOISE: Item = lesson("noise", "ノイズ解析 (増幅回路の雑音はどこから来る)", "抵抗の熱雑音とトランジスタのショット雑音を、部品ごとに分けて見る。信号源の抵抗を小さくすると減る", None);
pub(super) const I_PHASE_MARGIN: Item = lesson("phase_margin", "位相余裕 (容量の負荷でオペアンプが不安定になる)", "ゲインが 0dB になる周波数で、180° に対する余裕。出力の容量負荷が、余裕を食う", None);

pub const PATHS: &[LearningPath] = &[POWER_PATH, ARDUINO_PATH, MEGA_PATH, BOARD_PATH, SWITCH_PATH, BATTERY_PATH, SIGNAL_PATH, PARTS_PATH, REPAIR_PATH];

pub(super) const REPAIR_PATH: LearningPath = LearningPath {
    id: "repair",
    title: "動かない回路を直そう (修理の道)",
    goal: "作った回路が動かないとき、テスターで測って原因を突き止めます。回路図は正しい設計のまま、どこかが壊れた「基板」を、測った電圧とシミュレーションの値を比べて直します。故障は始めるたびに変わるので、何度でも練習できます",
    stages: &[
        Stage {
            name: "1. 測り方",
            about: "テスターの当て方と、「正しいはずの値」と「測った値」を比べる考え方です",
            items: &[Item { requires: Some(&["divider"]), ..lesson("repair_divider", "テスターの当て方 (分圧回路)", "部品 3 つの回路で、出力がおかしい原因を、測って当てる。もう 1 点を測ると見分けられる", Some("分圧回路をブレッドボードに組み、抵抗を 1 本抜いて、各点の電圧がどう変わるかをテスターで測ります")) }],
        },
        Stage {
            name: "2. 電源とスイッチを直す",
            about: "入口から出口へ順に測る。オンのはずのトランジスタの、どこで電流が止まっているかを探す",
            items: &[
                Item { requires: Some(&["zener", "repair_divider"]), ..lesson("repair_zener", "ツェナーの電源", "入口 VIN から出口 VOUT へ順に測る。出力が 0V になる故障は 2 つあり、もう 1 点で見分ける", None) },
                Item { requires: Some(&["bjt_switch", "repair_divider"]), ..lesson("repair_led", "トランジスタで点ける LED", "コレクタとベースを測って、トランジスタ・LED・抵抗・はんだ付けのどれかを見分ける", Some("LED が点かないとき、ベースとコレクタの電圧を測り、トランジスタがオンになっているかを確かめます")) },
            ],
        },
        Stage {
            name: "3. 帰還のある回路と、基板",
            about: "1 か所が壊れるとループ全体が狂う回路と、基板そのものの故障",
            items: &[
                Item { requires: Some(&["series_regulator", "repair_zener"]), ..lesson("repair_regulator", "帰還のある電源 (直列レギュレータ)", "基準・帰還・出力の「約束」のどれが破れているかで、ループの中の故障を絞る", None) },
                Item { requires: Some(&["board_fix", "repair_zener"]), ..I_BOARD_REPAIR },
            ],
        },
    ],
};

pub(super) const POWER_PATH: LearningPath = LearningPath {
    id: "power_supply",
    title: "安定化電源を作ろう",
    goal: "ACアダプタや電池の電圧を、負荷が変わっても一定の 5V にする電源を、原理から理解して、回路図・基板・実機まで自分で作ります。実験は、安全のため 12V 以下の低い電圧だけで行います",
    stages: &[
        Stage {
            name: "1. 入門: 電圧・電流と測り方",
            about: "電圧を分ける、電流を制限する、電池が負荷で電圧を下げる。安定化電源が「何を一定にするのか」の土台です",
            items: &[
                Item { build: true, requires: Some(&[]), ..lesson("divider", "電圧を分ける (分圧回路)", "抵抗2本で好きな電圧を作る。出力が負荷で変わる理由の入口", Some("テスタで電圧を測る。抵抗2本をブレッドボードにつなぎ、計算値と測定値を比べます")) },
                Item { build: true, requires: Some(&[]), ..lesson("led", "LED と電流制限抵抗", "電流を決めるのは抵抗。電流を測る方法も", Some("LED の電流をテスタの電流レンジで測り、抵抗を変えて明るさと電流を比べます")) },
                Item { requires: Some(&["divider"]), ..I_COIN_CELL },
            ],
        },
        Stage {
            name: "2. 基礎: 電源をつくる部品",
            about: "ダイオード・コンデンサ・ツェナー・トランジスタ。電圧を整え、ならし、一定にし、電流を増やす部品の働きです",
            items: &[
                Item { requires: Some(&["led"]), ..lesson("diode_curve", "ダイオードの特性", "約 0.7V (ショットキーは約 0.3V) で流れ始め、そのあとは電圧がほとんど増えない", Some("ダイオードの順方向電圧を、シリコンとショットキーで測り、温めて変化を見ます")) },
                Item { requires: Some(&["diode_curve"]), ..lesson("rectifier", "交流を直流にする (整流・平滑・リップル)", "整流・平滑コンデンサ・リップルの大きさ。シミュレーションは商用電圧 (100V) を使いますが、実機では使いません", Some("実機は低電圧の交流 (小さなトランスの出力など) だけで行い、オシロ (またはテスタの交流レンジ) でリップルを測って、容量を変えて比べます。商用電源 (100V) の配線は、危険なので行いません")) },
                Item { requires: Some(&["rectifier"]), ..I_ELECTROLYTIC_LIFE },
                Item { build: true, requires: Some(&["diode_curve", "divider"]), ..lesson("zener", "ツェナーで電圧を一定にする", "もっとも簡単な安定化。入力が変わっても動きにくいが、負荷が重いと崩れる。自分で作るチャレンジつき", Some("ツェナーと抵抗で 5.1V を作り、負荷と入力を変えて電圧の変化を測ります")) },
                Item { requires: Some(&["led"]), ..I_BJT_SWITCH },
                Item { build: true, requires: Some(&["zener", "bjt_switch"]), ..lesson("emitter_follower", "エミッタフォロワ (電流を増やす)", "電圧はそのまま、流せる電流を増やす。直列パス型電源の中核。大電流では発熱する", Some("ツェナーの電圧を、トランジスタで大電流にし、負荷を変えて出力と温度を測ります")) },
                Item { requires: Some(&["zener"]), ..lesson("debug_circuit", "動かない回路を直す (デバッグ)", "ツェナーの電源に故障が 1 つ仕込まれている。電圧を順に見て原因を探し、直す。故障は始めるたびに変わる", None) },
            ],
        },
        Stage {
            name: "3. 応用: 安定化のしくみ",
            about: "出力の電圧を測って、ずれたら直す (帰還)。これが安定化電源の本質です",
            items: &[
                Item { requires: Some(&["divider"]), ..I_OPAMP_NONINV },
                Item { build: true, requires: Some(&["opamp_noninv"]), ..I_OPAMP_INV },
                Item { build: true, requires: Some(&["opamp_inv", "emitter_follower"]), ..lesson("series_regulator", "基準電圧と帰還で安定化する (直列レギュレータ)", "オペアンプ + 基準電圧 + パストランジスタで、自作の安定化電源を組む", Some("負荷を変えたとき (ロードレギュレーション)、入力を変えたとき (ラインレギュレーション) の出力電圧を測ります")) },
                Item { build: true, requires: Some(&["emitter_follower"]), ..lesson("regulator_78xx", "三端子レギュレータ (78xx / LDO) を使う", "入出力のコンデンサ、ドロップアウト電圧、使い方と選び方", Some("7805 で 5V を作り、入力と負荷を変えて出力と温度を測ります")) },
                Item { requires: Some(&["zener"]), ..lesson("tl431_ref", "TL431 (シャントレギュレータ): 好きな電圧の基準を作る", "REF を 2.495V に保つ部品。分圧の比で、出力を 2.5V 以上の好きな値にできる。負荷が重すぎると崩れる", Some("TL431 に 2 本の抵抗をつなぎ、出力が 2.495V × (1 + R2 ÷ R3) になるか測ります")) },
                Item { requires: Some(&["regulator_78xx"]), ..I_LDO },
                Item { requires: Some(&["regulator_78xx"]), ..I_HEAT },
                Item { requires: Some(&["diode_curve", "regulator_78xx"]), ..I_PROTECTION },
                Item { requires: Some(&["protection"]), ..lesson("coil_layer_short", "コイルのレアショート (連鎖して壊れる)", "1つの故障が次の部品の故障を呼ぶ連鎖。ヒューズで止められる場合と、止められない場合", Some("リレーのコイル抵抗をテスタで測り、定格電流の内側か確かめます。コイルを実際にショートさせる実験はしません")) },
                Item { requires: Some(&["rectifier", "regulator_78xx"]), ..lesson("variable_supply", "ダイヤルで電圧を変える電源 (トランス + LM317)", "100V → トランス → 整流 → 可変レギュレータ。ダイヤル(ボリューム)で出力が変わる式と、出せない電圧の理由", Some("低電圧の電源 (15〜18V の AC アダプタ) で LM317 を組み、ボリュームを回して出力を測ります。100V は実験に使いません")) },
                Item { requires: Some(&["regulator_78xx", "heat"]), ..I_BUCK },
            ],
        },
        Stage {
            name: "4. 製作: 回路図から基板、実機まで",
            about: "設計した電源を、基板にして、組み立てて、性能を測ります",
            items: &[
                Item { id: "draw_psu", title: "安定化電源の回路図を自分で設計する (最終課題)", about: "目標 (入力 9〜12V → 出力 5V、逆接続でも壊れない) だけ示し、自分で部品を選んで置き、配線する", kind: Kind::Build, requires: Some(&["series_regulator", "regulator_78xx", "protection"]), build: true, real: None },
                Item { requires: Some(&["draw_psu"]), ..I_BOARD },
                Item { requires: Some(&["board"]), ..I_BOARD_COPPER },
                Item { requires: Some(&["board"]), ..I_BOARD_FIX },
                Item { requires: Some(&["board_fix"]), ..I_BOARD_REPAIR },
                Item { id: "build_psu", title: "組み立てて、性能を測る", about: "部品表・組み立て・測定。測った値をスペック表の「実測」欄に書いて、シミュレーションとの差とその理由を考える", kind: Kind::Measure, requires: Some(&["board_fix", "board_copper"]), build: false, real: Some("完成した電源の入力電圧・出力電圧を、負荷をつないだ状態とはずした状態で測り、シミュレーションと比べます") },
            ],
        },
    ],
};

/// A planned item of a tree path: shown on the map, not yet written (all items of the Arduino path are written; the Mega path has some).
#[allow(dead_code)]
pub(super) const fn plan(id: &'static str, title: &'static str, about: &'static str, requires: &'static [&'static str], real: Option<&'static str>) -> Item {
    Item { requires: Some(requires), ..planned(id, title, about, real) }
}

/// The second path: a tree. Each item says what to finish first (`requires`), so the independent branches (power,
/// clock, reset, pins) can be learned in any order and meet at the schematic. Items without a lesson yet are `plan`.
pub(super) const ARDUINO_PATH: LearningPath = LearningPath {
    id: "arduino_board",
    title: "Arduino 互換ボードを作ろう",
    goal: "ATmega328P に水晶・リセット・電源まわりをつけた、Arduino 互換のボードを、原理から理解して、回路図・基板・実機まで自分で作ります。マイコンの中身はシミュレーションできないので、周りの回路 (電源・水晶・リセット・LED) を確かめます。先に電源の学習パスの「電圧を分ける」「LED と電流制限抵抗」を済ませておくと進めやすいです",
    stages: &[
        Stage {
            name: "1. 土台: 読む・浮かせない・安定させる",
            about: "どのマイコン回路でも使う基本の技です。順番は自由です",
            items: &[
                Item { requires: Some(&[]), ..lesson("datasheet", "データシートの読み方", "絶対最大定格と推奨動作条件の違い。回路の電流・電力と比べて、2 倍の余裕で部品を選ぶ", Some("手元の部品のデータシートで、電源電圧の範囲・1ピンの電流の上限を探します")) },
                Item { requires: Some(&["divider"]), ..lesson("pullup", "プルアップ (ピンを浮かせない)", "入力ピンが何にもつながっていないと、電圧が定まらず、ノイズで揺れる。10kΩ で電源に引く理由と、値の決め方", Some("入力ピンをプルアップあり・なしで、テスタで測り比べます")) },
                Item { requires: Some(&["divider"]), ..lesson("decoupling", "電源のバイパスコンデンサ", "IC の電源ピンのそばに 0.1µF を置く理由。インピーダンスのグラフで、電解とセラミックの分担と、自己共振を見る", Some("IC の電源ピンの電圧を、オシロで見て、コンデンサの有無で比べます")) },
            ],
        },
        Stage {
            name: "2. マイコンが動く条件: クロックとリセット",
            about: "電源が入っても、クロックとリセットが整っていないとマイコンは動きません",
            items: &[
                Item { requires: Some(&["datasheet"]), ..lesson("crystal", "水晶とクロック", "16MHz 水晶の共振の細い山と、すぐ上の深い谷を、AC解析で見る。細い山は刻みが粗いと見落とす", Some("水晶の発振波形をオシロで見ます (プローブの容量で止まることがあります)")) },
                Item { requires: Some(&["pullup"]), ..lesson("reset", "リセット回路", "DTR・100n のコンデンサ・10k のプルアップで、自動リセットのパルスを作る。値を変えるとかからなくなる。跳ね上がりをダイオードで抑える", Some("シリアルモニタを開く瞬間のリセットを、LED (D13) の点滅や、オシロの波形で確かめます")) },
            ],
        },
        Stage {
            name: "3. つなぐ: 入出力と通信",
            about: "ピンで LED を光らせ、パソコンと話し、電圧の違う相手とつなぎます",
            items: &[
                Item { requires: Some(&["led", "datasheet"]), ..lesson("pin_led", "ピンで LED を光らせる", "ピンから流せる電流の上限 (20〜40mA) と、抵抗の決め方。D13 の LED", Some("D13 の LED の電流を測って、データシートの上限と比べます")) },
                Item { requires: Some(&["temp_sensors"]), ..lesson("current_sensors", "電流センサ (シャント抵抗とホール式)", "シャントの電圧をアンプで増やす方法と、ホール式。小さな電流の読みやすさ、アンプの頭打ち、シャントの選び方", Some("シャントの両端の電圧を測り、電流計と比べます (大電流のときは、シャントの発熱に注意)")) },
                Item { requires: Some(&["temp_sensors"]), ..lesson("light_sensors", "光センサ (LDR とフォトダイオード)", "LDR は暗いと 1MΩ で、変わり方はゆるやか。フォトダイオードは光に比例するが、強いと飽和する。分圧の抵抗の選び方", Some("LDR を 10kΩ と分圧して、部屋の明かりと手で隠したときの電圧を測ります")) },
                Item { requires: Some(&["datasheet", "divider"]), ..lesson("temp_sensors", "温度センサを読む (LM35・TMP36・NTC)", "温度に比例する電圧の IC と、抵抗が変わるサーミスタ。感度、0℃ より低い温度、分圧抵抗の選び方", Some("センサの電圧を測って式で温度にし、温度計と比べます")) },
                Item { requires: Some(&["pullup"]), ..lesson("debounce", "ボタンのチャタリング", "接点が数 ms ばたつき、1 回押しただけなのに何回も数えられる。RC で止めると遅れる。大きすぎると押したのが分からない", Some("押した回数を数えるスケッチで、1 回押して、カウントが 2 以上になるか見ます")) },
                Item { requires: Some(&["pin_led"]), ..I_FLYBACK },
                Item { requires: Some(&["flyback", "datasheet"]), ..lesson("equiv_model", "モデルのない IC を、等価回路で再現する (ULN2803A)", "SPICE モデルがない IC を、データシートの内部回路図から基本の部品で組み直す。データシートの代表値と比べて、ずれの理由を考えて直す", Some("ULN2803A でリレーを動かし、出力の電圧をテスタで測って、データシートの表と比べます")) },
                Item { requires: Some(&["pin_led"]), ..I_PWM },
                Item { requires: Some(&["pullup"]), ..lesson("levels", "3.3V と 5V をつなぐ (レベル変換)", "電圧の違う相手を直接つなぐと壊れる・読めない。分圧と MOSFET (BSS138) での変換と、プルアップによる波形の遅れ", Some("5V のピンの出力を、分圧してから 3.3V の部品に入れる前に、テスタで測って確かめます")) },
                Item { requires: Some(&["levels", "reset"]), ..lesson("serial", "シリアル通信と USB 変換 (発展)", "TX/RX の交差、ボーレートと 1 ビットの長さ、配線の抵抗と容量でなまると文字化けする", Some("端末ソフトで 9600bps と別のボーレートを比べて、文字化けを見ます")) },
                Item { requires: Some(&["pullup"]), ..I_EEPROM_BASICS },
            ],
        },
        Stage {
            name: "3b. 発展: USB につなぐ (電気の基本)",
            about: "USB の電源 (VBUS)、速度の見分け、信号線の波形。USB のプロトコルそのものは扱わず、電気の面を確かめます",
            items: &[
                Item { requires: Some(&["decoupling"]), ..lesson("usb_power", "USB の電源 (VBUS)", "ケーブルの抵抗で電圧が下がること、差し込んだ瞬間の突入電流と、VBUS の容量の目安 (10µF 程度)", Some("USB テスターなどで、機器の端の VBUS の電圧と電流を測ります")) },
                Item { requires: Some(&["pullup"]), ..lesson("usb_speed", "USB の速度の見分け", "D+ か D− を 1.5kΩ で引くと、ホストが機器の速度を知る。プルアップがないと認識されない", Some("機器をつなぐ前後の D+ / D− の電圧を測ります (止まった状態で)")) },
                Item { requires: Some(&["usb_speed"]), ..lesson("usb_signal", "USB の信号線と ESD 保護の容量", "12Mbit/s の 1 ビットは約 83ns。ESD 保護部品の容量が大きいと、立ち上がりが遅れて読めなくなる", None) },
                Item { requires: Some(&["usb_signal", "board"]), ..I_BOARD_USB },
                Item { requires: Some(&["board_usb"]), ..I_BOARD_PAIRS },
            ],
        },
        Stage {
            name: "4. 設計して基板に",
            about: "ここまでの部品を1枚にまとめて、基板にします",
            items: &[
                Item { id: "draw_arduino", title: "Arduino 互換ボードの回路図を自分で描く (最終課題)", about: "ATmega328P・水晶・リセット・バイパス・LED・書き込みヘッダを、目標だけ見て自分で配線する。つなぎ方と部品の値を判定する", kind: Kind::Build, build: true, requires: Some(&["decoupling", "crystal", "reset", "pin_led", "serial"]), real: None },
                Item { requires: Some(&["draw_arduino", "board"]), ..I_BOARD_ARDUINO },
            ],
        },
        Stage {
            name: "5. 作って動かす",
            about: "はんだ付けして、ブートローダを書き込み、LED を点滅させます",
            items: &[
                Item { id: "solder", title: "表面実装のはんだ付け", about: "0805 のチップ部品と TQFP-32 (0.8mm ピッチ) のはんだ付けのコツ。ブリッジの直し方と、電源を入れる前の点検", kind: Kind::Hands, build: false, requires: Some(&["board_arduino"]), real: Some("まず練習用の基板で、0805 のチップ部品から始めます") },
                Item { id: "bootload", title: "ブートローダを書き込んで LED を点滅させる", about: "ICSP ヘッダでの書き込み、Arduino IDE の設定、シリアルでのスケッチの書き込み。うまくいかないときの見方", kind: Kind::Hands, build: false, requires: Some(&["solder", "serial"]), real: None },
            ],
        },
    ],
};

/// The third path: the Arduino Mega 2560 compatible board (`docs/examples/arduino_mega_usb.schjson`). It builds on the Arduino path (crystal,
/// reset, bypass, serial, the board lessons) and adds what a 100-pin board needs. Only the new items are listed; the others are `requires`.
pub(super) const MEGA_PATH: LearningPath = LearningPath {
    id: "arduino_mega",
    title: "Arduino Mega 互換ボードを作ろう",
    goal: "ATmega2560 (100 ピン) に、USB-シリアル変換 (CH340G)・12MHz と 16MHz の 2 つの水晶・電源まわりをつけた、Arduino Mega 互換のボードを、回路図・基板・実機まで作ります。Arduino 互換ボードの学習パスで、水晶・リセット・バイパス・シリアル・基板の置き方を済ませてから進めると分かりやすいです。ピンの数が多いので、基板は自動配線で通らない線を、手で仕上げます",
    stages: &[
        Stage {
            name: "1. Mega ならではの電気",
            about: "ピンの数と部品の数が増えると、新しく考えることが出てきます。順番は自由です",
            items: &[
                Item { requires: Some(&["pin_led", "datasheet"]), ..lesson("adc_input", "ADC の入力 (信号源の抵抗は 10kΩ 以下)", "ADC は、入力のコンデンサを短い時間で充電して読む。信号源の抵抗が大きいと充電しきれず、読み違える。ピンに大きなコンデンサをつけて直す。Mega は 16 チャンネル", Some("1MΩ 2 本で分圧した 2.5V を A0 で読み、コンデンサをつけて値の変化を見ます")) },
                Item { requires: Some(&["decoupling"]), ..lesson("mega_power", "電源ピンが 5 組ある (バイパスは、容量より近さ)", "ATmega2560 は VCC と GND が 5 組。すべてのペアのそばにバイパスコンデンサを置く。遠い 100nF より、近い 1nF のほうが効く。AVCC・AREF にも 0.1µF", Some("電源ピンの電圧を、バイパスコンデンサの有無でオシロで見比べます")) },
                Item { requires: Some(&["crystal"]), ..lesson("crystal_load", "水晶の負荷容量 (22pF の理由と周波数のずれ)", "水晶の両端の 22pF は、水晶から見た負荷容量 CL を決める。小さいほど周波数が高く、数百 ppm ずれる。データシートの CL に合わせる", Some("水晶の周波数を、プローブを当てる前後で比べます")) },
                Item { requires: Some(&["usb_signal", "serial"]), ..lesson("ch340g", "CH340G と USB の信号 (差動で受けるとノイズに強い)", "USB の D+ と D− は逆向きの 2 本で、受け側は差を読む。同じノイズが 2 本に乗れば消え、片方だけだと信号の余裕を超えたところで読み違える。仮のモデルの CH340G を使う", Some("PC につなぎ、シリアルモニタに文字が出るか確かめます。オシロで D+ と D− の差の波形を見ます")) },
            ],
        },
        Stage {
            name: "2. 100 ピンの基板",
            about: "0.5mm ピッチの 100 ピンの IC は、自動配線が通せない線が残ります。残った線を、手で仕上げます",
            items: &[Item { requires: Some(&["board_arduino"]), ..I_BOARD_MEGA }],
        },
        Stage {
            name: "3. 作って動かす",
            about: "100 ピンのチップを、はんだ付けします",
            items: &[Item { id: "solder_tqfp100", title: "100 ピン (0.5mm ピッチ) のはんだ付け", about: "TQFP-100 のはんだ付け。ブリッジの直し方、フラックスの使い方、電源ピンの点検と、CH340 の COM ポートが出るまで", kind: Kind::Hands, build: false, requires: Some(&["board_mega", "solder"]), real: Some("まず練習用の基板で、0.5mm ピッチのチップを試します") }],
        },
    ],
};

pub(super) const BOARD_PATH: LearningPath = LearningPath {
    id: "board_craft",
    title: "基板づくりの道",
    goal: "回路図から基板を作り、発注前チェックを通し、マイコンの基板の置き方・USB の差動ペア・100 ピンの IC のような難しい基板も扱い、壊れた基板の故障を探して直せるようになります。",
    stages: &[
        Stage {
            name: "1. はじめての基板",
            about: "取り込み・配置・配線・DRC・発注の準備の一巡と、電流に合わせた配線の太さ、発注前チェックの直し方",
            items: &[
                Item { requires: Some(&[]), ..I_BOARD },
                Item { requires: Some(&["board"]), ..I_BOARD_COPPER },
                Item { requires: Some(&["board"]), ..I_BOARD_FIX },
            ],
        },
        Stage {
            name: "2. 置き方と配線の勘所",
            about: "マイコンの基板で水晶とバイパスコンデンサを寄せる。USB の D+ / D− を差動ペアで引く。2 組のペアの長さを揃える",
            items: &[
                Item { requires: Some(&["board_fix"]), ..I_BOARD_ARDUINO },
                Item { requires: Some(&["board_fix"]), ..I_BOARD_USB },
                Item { requires: Some(&["board_usb"]), ..I_BOARD_PAIRS },
            ],
        },
        Stage {
            name: "3. 難しい基板と、壊れた基板",
            about: "自動配線が通せない 100 ピンの基板を手で仕上げる。故障を仕込んだ基板の原因を、症状から探して直す",
            items: &[
                Item { requires: Some(&["board_arduino"]), ..I_BOARD_MEGA },
                Item { requires: Some(&["board_fix"]), ..I_BOARD_REPAIR },
            ],
        },
    ],
};

pub(super) const SWITCH_PATH: LearningPath = LearningPath {
    id: "switching",
    title: "スイッチで負荷を動かそう",
    goal: "トランジスタや MOSFET で、LED・リレー・モーターのような負荷を入り切りし、コイルの逆起電力から守り、PWM で加減し、スイッチングで電圧を変える (降圧) ところまで、原理から理解します。",
    stages: &[
        Stage {
            name: "1. 半導体のスイッチ",
            about: "ベース電流やゲート電圧で、負荷を入り切りする。NPN・PNP・MOSFET の違いと、オンの深さ",
            items: &[
                Item { requires: Some(&[]), ..I_BJT_SWITCH },
                Item { requires: Some(&["bjt_switch"]), ..I_MOSFET },
                Item { requires: Some(&["bjt_switch"]), ..I_PNP_HIGHSIDE },
            ],
        },
        Stage {
            name: "2. 負荷を守って、加減する",
            about: "コイル (リレー・モーター) を切ると出る逆起電力から守る。入り切りの割合で、電圧を加減する (PWM)",
            items: &[
                Item { requires: Some(&["bjt_switch"]), ..I_FLYBACK },
                Item { requires: Some(&["mosfet"]), ..I_PWM },
            ],
        },
        Stage {
            name: "3. スイッチングで電圧を変える",
            about: "スイッチを高速に入り切りして、熱にせずに電圧を下げる降圧コンバータ。損失はどこで出るか (MOSFET の切り替え、ダイオードの逆回復) と、リニアとの比較",
            items: &[
                Item { requires: Some(&["mosfet", "flyback"]), ..I_BUCK },
                Item { requires: Some(&["buck"]), ..I_SWITCHING_LOSS },
                Item { requires: Some(&["buck"]), ..I_DIODE_RECOVERY },
                Item { requires: Some(&["buck"]), ..I_LDO_VS_BUCK },
            ],
        },
        Stage {
            name: "4. 基板にする",
            about: "大きな電流が流れる基板の、配線の太さと電圧降下",
            items: &[Item { requires: Some(&["buck"]), ..I_BOARD_COPPER }],
        },
    ],
};

pub(super) const BATTERY_PATH: LearningPath = LearningPath {
    id: "battery",
    title: "電池で動かそう",
    goal: "電池の性質 (内部抵抗・残量) から、リチウムイオン電池の充電、守り (逆接続・ヒューズ)、低い電圧の作り方 (LDO・降圧)、熱と定格まで、電池で動く機器の電源を考えられるようになります。",
    stages: &[
        Stage {
            name: "1. 電池を知る",
            about: "電池は理想の電圧源ではない。内部抵抗と残量。リチウムイオン電池の充電の 3 段階",
            items: &[Item { requires: Some(&[]), ..I_COIN_CELL }, Item { requires: Some(&["coin_cell"]), ..I_TP4056 }],
        },
        Stage {
            name: "2. 守って、電圧を整える",
            about: "逆接続・ショートから守る。電池の電圧から、機器に必要な電圧を作る。電圧差は熱になる",
            items: &[
                Item { requires: Some(&["tp4056"]), ..I_PROTECTION },
                Item { requires: Some(&["coin_cell"]), ..I_LDO },
                Item { requires: Some(&["ldo"]), ..I_HEAT },
            ],
        },
        Stage {
            name: "3. 電池を長持ちさせる",
            about: "電圧差を熱にしないスイッチング電源。リニアとの効率の違い",
            items: &[Item { requires: Some(&["ldo", "heat"]), ..I_BUCK }],
        },
    ],
};

pub(super) const PARTS_PATH: LearningPath = LearningPath {
    id: "parts",
    title: "部品の素顔: 動作と特性と計算式",
    goal: "部品を 1 種類ずつ取り上げて、何をする部品か、特性を表す数字は何か、その数字を決める式は何かを、シミュレーションで確かめながら学びます。",
    stages: &[
        Stage {
            name: "0. はじめに: 電圧・電流と、回路図の読み方",
            about: "電圧・電流・抵抗の関係 (オームの法則) と、回路図の線・ラベル・GND、部品の値の書き方 (k / M / µ)。部品を 1 つずつ学ぶ前の、最初の 2 本です",
            items: &[
                Item { requires: Some(&[]), ..lesson("intro_ohm", "はじめに 1: 電圧・電流・抵抗 (オームの法則)", "電圧を上げると電流が増え、抵抗を大きくすると電流が減る。I = V ÷ R", Some("電池と抵抗 1 本で、テスタの電圧レンジ・電流レンジを使い、計算と測定を比べます (電流は電池 1.5V 以下で)")) },
                Item { requires: Some(&["intro_ohm"]), ..lesson("intro_read", "はじめに 2: 回路図の読み方と、値の書き方", "ラベルは線の代わり、GND は 0V の基準、値は 4.7k・100n・1Meg のように書く (M は 1/1000 なので注意)", Some("手元の抵抗のカラーコードと、値の表記 (4k7、472 など) を読みます")) },
            ],
        },
        Stage {
            name: "1. 抵抗・コンデンサ・コイル",
            about: "電気回路の三つの基本の部品。電圧・電流・時間の関係を、式と数字で確かめます",
            items: &[
                Item { requires: Some(&["intro_read"]), ..lesson("resistor", "抵抗の素顔 (オームの法則・直列・並列・電力)", "V = I × R、P = V × I、直列は足し算・並列は小さくなる、定格電力と許容差", Some("抵抗の値をテスタで測り、直列・並列の合成と、中点の電圧を計算と比べます")) },
                Item { requires: Some(&["resistor"]), ..lesson("capacitor", "コンデンサの素顔 (電流は電圧の変化に比例する)", "I = C × dV/dt、充電の時定数 τ = R × C、ためるエネルギー ½CV²、並列と直列の容量", Some("大きな電解コンデンサを抵抗で充電し、63% に達する時間を測って τ = R × C と比べます")) },
                Item { requires: Some(&["capacitor"]), ..lesson("inductor", "コイルの素顔 (電流は急には変えられない)", "V = L × di/dt、時定数 τ = L ÷ R、ためるエネルギー ½LI²、飽和電流", Some("コイルの DCR を測り、抵抗といっしょにした立ち上がりの時間から L ÷ R を確かめます")) },
            ],
        },
        Stage {
            name: "2. ダイオードの仲間",
            about: "一方通行の部品。電流で変わる抵抗、光る LED、電圧を止めるツェナー",
            items: &[
                Item { requires: Some(&["diode_curve"]), ..lesson("diode_basics", "ダイオードの素顔 (一方通行と、電流で変わる抵抗)", "I = Is (e^(V / nVt) − 1)、V は電流の対数、動的な抵抗 rd = 26mV ÷ I", Some("1N4148 の電流を変えて電圧を測り、順向きと逆向きの違いを確かめます")) },
                Item { requires: Some(&["diode_basics", "led"]), ..lesson("led_basics", "LED の素顔 (電圧ではなく電流で光る)", "I = (電源 − Vf) ÷ R、Vf は色で違う、電流の上限、抵抗の発熱", Some("赤と青の LED を 5V と 3.3V で光らせ、電流と明るさを比べます")) },
                Item { requires: Some(&["led_basics"]), ..lesson("segment_basics", "7 セグメント LED の素顔 (共通の抵抗は明るさをばらつかせる)", "光る線の数で変わる電流、セグメントごとの抵抗、共通ピンに流れる合計の電流", Some("7 セグメント LED の 1 つの数字を、共通の抵抗とセグメントごとの抵抗で光らせ、明るさを比べます")) },
                Item { requires: Some(&["diode_basics", "zener"]), ..lesson("zener_basics", "ツェナーダイオードの素顔 (逆向きで電圧が止まる)", "降伏電圧 Vz、動作抵抗 rz、Iz = (Vin − Vz) ÷ R、電力 P = Vz × Iz と定格", Some("1N4733A に抵抗をつないで、電源を変えながら Z の電圧を測り、止まる様子を確かめます")) },
            ],
        },
        Stage {
            name: "3. 増やす部品",
            about: "電流を増やすトランジスタ、電圧で制御する MOSFET、信号を増やすオペアンプ",
            items: &[
                Item { requires: Some(&[]), ..lesson("transistor", "トランジスタの素顔 (電流を増やす部品)", "Ic = hFE × Ib、Vbe 約 0.65V、飽和 (Vce(sat))、損失 Vce × Ic", Some("2N3904 のベース抵抗を変えて、Ic ÷ Ib の hFE と、飽和したときのコレクタ電圧を測ります")) },
                Item { requires: Some(&["transistor"]), ..lesson("mosfet_basics", "MOSFET の素顔 (電圧で電流を制御する)", "しきい値 Vth、オン抵抗 Rds(on)、Id ∝ (Vgs − Vth)²、中途半端な電圧は損失が大きい", Some("2N7000 のゲートの電圧を変えながらドレインの電圧を測り、しきい値とオン抵抗を確かめます")) },
                Item { requires: Some(&["opamp_noninv"]), ..lesson("opamp_basics", "オペアンプの素顔 (増幅の式・帯域・振れ幅の限界)", "倍率 1 + Rf ÷ Rg、GBW = 倍率 × 帯域、出力は電源で頭打ち", Some("LM358 の非反転増幅で倍率と −3dB の周波数を測り、入力を大きくして出力が切れるのを見ます")) },
            ],
        },
        Stage {
            name: "4. センサと光の部品",
            about: "温度や光を電気に変える部品と、光で信号を絶縁して伝えるフォトカプラ",
            items: &[
                Item { requires: Some(&["temp_sensors"]), ..lesson("thermistor_basics", "サーミスタの素顔 (温度で抵抗が桁で変わる)", "R(T) = R25 × exp(B (1/T − 1/T25))、感度 −B/T² ≒ −4.4%/K、分圧の抵抗は R25 に合わせる", Some("NTC の抵抗を、氷水・室温・お湯で測り、B 定数を計算して、データシートの値と比べます")) },
                Item { requires: Some(&["light_sensors"]), ..lesson("phototransistor_basics", "フォトトランジスタの素顔 (光電流が hFE 倍になる)", "Ic = hFE × 光電流、感度はフォトダイオードの数百倍、強い光で飽和、応答は遅い", Some("フォトトランジスタに LED の光を当て、距離を変えて、出力の電圧が飽和する様子を測ります")) },
                Item { requires: Some(&["led_basics"]), ..lesson("photocoupler_basics", "フォトカプラの素顔 (光で信号を絶縁して伝える)", "Ic = CTR × If、絶縁、CTR の個体差と、負荷が重いと飽和する", Some("PC817 の LED の電流を変えて、出力のトランジスタの電流と電圧を測り、CTR を求めます")) },
                Item { requires: Some(&["current_sensors"]), ..lesson("hall_basics", "ホールセンサの素顔 (磁界に比例して出力が振れる)", "ゼロ磁界で電源の半分、感度 mV/ガウス、極性、電源に比例 (レシオメトリック)", Some("磁石を近づけ・遠ざけながら、出力の電圧を測り、極性と、距離による変化を見ます")) },
            ],
        },
        Stage {
            name: "5. 電源と保護の部品",
            about: "電圧を変えるトランス、電圧を安定にするレギュレータ、回路を守るヒューズ",
            items: &[
                Item { requires: Some(&["coin_cell"]), ..lesson("battery_basics", "電池の素顔 (内部抵抗が電池の種類を分ける)", "端子電圧 = 開放電圧 − 電流 × 内部抵抗、種類による違い、最大電力は内部抵抗と同じ抵抗のとき", Some("単3電池とコイン電池に同じ抵抗をつなぎ、電圧の下がり方を比べます (ショートさせる実験はしません)")) },
                Item { requires: Some(&["regulator_78xx"]), ..lesson("regulator_basics", "三端子レギュレータの素顔 (電源のゆれを消す力と、その限界)", "リップル除去 (PSRR)、ドロップアウト電圧、入力に余裕がないと素通りする", Some("7805 に電池や AC アダプタをつなぎ、入力を下げながら出力を測って、ドロップアウトを確かめます")) },
                Item { requires: Some(&["rectifier", "inductor"]), ..lesson("transformer_basics", "トランスの素顔 (電圧は巻数比、電流は逆比)", "Vs ÷ Vp = Ns ÷ Np、Is ÷ Ip = Np ÷ Ns、定格 VA、無負荷と全負荷の電圧の差", Some("小さなトランスの巻線の抵抗をテスタで測り、一次と二次の比と電圧降下を計算と比べます (商用電源につなぐ実験はしません)")) },
                Item { requires: Some(&["transformer_basics"]), ..lesson("bridge_basics", "ダイオードブリッジの素顔 (整流の損失とピーク電流)", "全波は半波の半分のリップル、ダイオード 2 本ぶんの電圧降下、平滑コンデンサに流れる短く大きな電流", Some("低電圧の交流 (トランスの出力) を整流し、半波と全波のリップルを、オシロで比べます (商用電源の整流は行いません)")) },
                Item { requires: Some(&["protection"]), ..lesson("fuse_basics", "ヒューズの素顔 (大電流ほど速く切れる)", "定格は切れない電流、時間-電流特性、負荷は定格の 7 割以下、切れたら戻らない", Some("ヒューズの抵抗を測り、定格の電流の範囲で使う負荷の選び方を、データシートの時間-電流曲線と照らします")) },
                Item { requires: Some(&["fuse_basics"]), ..lesson("varistor_basics", "バリスタの素顔 (電圧が高くなると導通して守る)", "V ∝ I^(1/α) の曲線、動作電圧の手前の漏れ、サージを吸収して制限電圧に止める、劣化", Some("バリスタの抵抗を測り、定格の連続電圧が回路の電圧より十分高いことを、データシートで確かめます (サージを実際に加える実験はしません)")) },
            ],
        },
        Stage {
            name: "6. スイッチ・時計・音・ロジック",
            about: "コイルで動くリレー、周波数を決める水晶、音を出すスピーカーとブザー、High と Low を扱うロジック IC",
            items: &[
                Item { requires: Some(&["pullup"]), ..lesson("switch_basics", "スイッチの素顔 (閉じた接点は小さな抵抗)", "接点抵抗と電圧降下・発熱、小さな電流の信号には金メッキ、定格と寿命", Some("スイッチを閉じた状態の抵抗を測り、押すたびに変わるか、電流を流して電圧降下を見ます")) },
                Item { requires: Some(&["switch_basics"]), ..lesson("analog_switch_basics", "アナログスイッチの素顔 (オン抵抗と、サンプル用のコンデンサ)", "オン抵抗 Ron の分圧誤差、オフ時の漏れが小さい、サンプルホールドの時定数 (Rs + Ron) × C", Some("アナログスイッチ IC で信号を通し、負荷の抵抗を変えて、電圧の下がりを測ります")) },
                Item { requires: Some(&["inductor", "flyback"]), ..lesson("relay_basics", "リレーの素顔 (コイルの電流で接点が動く)", "コイル = 抵抗 + インダクタンス、動作電圧 (定格の約 75%)、τ = L ÷ R、電力 V² ÷ R", Some("5V 用リレーのコイルの抵抗を測り、電圧を上げながらカチッと動作する電圧を探します")) },
                Item { requires: Some(&["crystal", "rlc"]), ..lesson("crystal_basics", "水晶の素顔 (Q が桁違いに大きい共振)", "等価回路 Rm・Lm・Cm・C0、fs = 1 ÷ (2π√(Lm Cm))、Q = ωL ÷ R、帯域幅 = fs ÷ Q", Some("水晶を LC 共振回路と並べ、ふつうの LC より山が桁違いに細いことを、発振器の周波数の安定として確かめます")) },
                Item { requires: Some(&["inductor"]), ..lesson("speaker_basics", "スピーカーとブザーの素顔 (インピーダンスは 8Ω 一定ではない)", "共振で上がるインピーダンス、高い周波数でコイルとして上がる、圧電ブザーは容量+共振", Some("スピーカーのインピーダンスを、テスタで測る直流の抵抗 (約 8Ω ではなく 6〜7Ω) と、アンプの表示を比べます")) },
                Item { requires: Some(&["levels"]), ..lesson("logic_basics", "ロジック IC の素顔 (しきい値・ヒステリシス・出力の力)", "しきい値は電源の半分、シュミットトリガは 0.6 / 0.4 Vcc、出力は電源につながる約 60Ω の抵抗", Some("74HC04 と 74HC14 にゆっくり変わる信号とノイズを入れ、出力が揺れるか確かめます")) },
            ],
        },
    ],
};

pub(super) const SIGNAL_PATH: LearningPath = LearningPath {
    id: "signals",
    title: "信号を扱おう: フィルタ・増幅・発振",
    goal: "周波数ごとに信号を弱める (フィルタ)、大きくする (増幅)、自分で振動させる (発振) を、AC 解析と過渡解析で見ます。山の正体 (極と Q) と、増幅が不安定になる理由 (位相余裕) まで、原理から理解します。",
    stages: &[
        Stage {
            name: "1. フィルタ",
            about: "抵抗とコンデンサで高い周波数を弱める。コイルを足すと、行き過ぎて揺れる。その山を、極と零で読む",
            items: &[
                Item { build: true, requires: Some(&[]), ..I_RC_LOWPASS },
                Item { requires: Some(&["rc_lowpass"]), ..I_RLC },
                Item { requires: Some(&["rc_lowpass"]), ..I_HIERARCHY },
                Item { requires: Some(&["rlc"]), ..I_POLES },
            ],
        },
        Stage {
            name: "2. 増幅と安定",
            about: "オペアンプの帰還でゲインを決める。出力に容量をつなぐと、位相余裕が減って不安定になる",
            items: &[
                Item { requires: Some(&["rc_lowpass"]), ..I_OPAMP_NONINV },
                Item { build: true, requires: Some(&["opamp_noninv"]), ..I_OPAMP_INV },
                Item { requires: Some(&["opamp_inv", "poles"]), ..I_PHASE_MARGIN },
                Item { requires: Some(&["opamp_noninv"]), ..I_SLEW_RATE },
                Item { requires: Some(&["opamp_noninv"]), ..I_NOISE },
            ],
        },
        Stage {
            name: "3. 発振",
            about: "コンデンサの充放電で、自分で一定の間隔を作る",
            items: &[Item { requires: Some(&["rc_lowpass"]), ..I_NE555 }],
        },
    ],
};
