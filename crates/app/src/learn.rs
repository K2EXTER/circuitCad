//! Learning paths: an ordered route through the lessons toward a goal (the first one: build a
//! regulated power supply yourself), and the record of which lessons the learner has finished.
//! The record is a small JSON file next to the part library, so it survives restarts.

use eframe::egui::{self, Color32};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ progress record

/// Finished lessons, by lesson id, saved as `{"version":1,"done":["divider", ...]}`.
#[derive(Default)]
pub struct Progress {
    done: BTreeSet<String>,
    file: Option<PathBuf>,
    /// The first-launch welcome card has been answered (never shown again); kept by `reset`.
    welcomed: bool,
    /// Why the last save failed (shown in the window), if it did.
    pub error: Option<String>,
}

impl Progress {
    /// `%APPDATA%\circuitCad\progress.json` (next to the part library folder).
    pub fn default_file() -> PathBuf {
        part_lib::default_dir().parent().map_or_else(|| PathBuf::from("progress.json"), |d| d.join("progress.json"))
    }

    /// Reads the record; a missing or unreadable file is an empty record (nothing is lost by starting over).
    pub fn load(file: &Path) -> Self {
        let json = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        let done = json
            .as_ref()
            .and_then(|v| v.get("done").and_then(|d| d.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()))
            .unwrap_or_default();
        let welcomed = json.as_ref().and_then(|v| v.get("welcomed")).and_then(|w| w.as_bool()).unwrap_or(false);
        Self { done, file: Some(file.to_path_buf()), welcomed, error: None }
    }

    /// True until the welcome card has been answered once. A learner who already finished a lesson never sees it.
    pub fn needs_welcome(&self) -> bool {
        !self.welcomed && self.done.is_empty()
    }

    pub fn mark_welcomed(&mut self) {
        if !self.welcomed {
            self.welcomed = true;
            self.save();
        }
    }

    pub fn is_done(&self, id: &str) -> bool {
        self.done.contains(id)
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.done.len()
    }

    /// Records a finished lesson and saves. True if it was new.
    pub fn mark(&mut self, id: &str) -> bool {
        if !self.done.insert(id.to_string()) {
            return false;
        }
        self.welcomed = true;
        self.save();
        true
    }

    /// Forgets everything (the "最初からやり直す" button).
    pub fn reset(&mut self) {
        self.done.clear();
        self.save();
    }

    fn save(&mut self) {
        let Some(file) = &self.file else { return };
        let json = serde_json::json!({ "version": 1, "done": self.done.iter().collect::<Vec<_>>(), "welcomed": self.welcomed });
        let result = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(file, serde_json::to_string_pretty(&json).unwrap_or_default()));
        self.error = result.err().map(|e| format!("進み具合を保存できませんでした: {e}。次に起動すると、この記録は消えます"));
    }
}

// ------------------------------------------------------------------ the paths

/// What an item of a path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A schematic lesson (by lesson id).
    Lesson(&'static str),
    /// The board lesson.
    Board(usize),
    /// Only a "作ってみよう" challenge (by the item's id); done when the challenge is passed.
    Build,
    /// The bench step: build it, measure it, compare with the simulation (`measure_guide.rs`); done when
    /// three measurements have been compared (the app marks the item's id).
    Measure,
    /// A hands-on task on the bench that the program cannot judge (soldering, flashing): a checklist the learner ticks herself
    /// (`bench_guide.rs`, by the item's id).
    Hands,
    /// Not written yet: shown so the whole route is visible.
    Planned,
}

pub struct Item {
    /// Key of the record for lessons; any short name for planned items.
    pub id: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub kind: Kind,
    /// The lesson has a "作ってみよう" challenge of the same id (build it yourself, judged by what it does).
    pub build: bool,
    /// What it is best to finish first, by item id (items of any path). `None` = the item just before it in its path
    /// (the original straight line); `Some(&[])` = a root with no prerequisite. Never a lock: a learner can open any item.
    pub requires: Option<&'static [&'static str]>,
    /// A hands-on experiment on a real board / breadboard belongs to this step (a note, until the cards exist).
    pub real: Option<&'static str>,
}

pub struct Stage {
    pub name: &'static str,
    pub about: &'static str,
    pub items: &'static [Item],
}

pub struct LearningPath {
    pub id: &'static str,
    pub title: &'static str,
    pub goal: &'static str,
    pub stages: &'static [Stage],
}

const fn lesson(id: &'static str, title: &'static str, about: &'static str, real: Option<&'static str>) -> Item {
    Item { id, title, about, kind: Kind::Lesson(id), build: false, requires: None, real }
}

// shown on the map before it is written
const fn planned(id: &'static str, title: &'static str, about: &'static str, real: Option<&'static str>) -> Item {
    Item { id, title, about, kind: Kind::Planned, build: false, requires: None, real }
}

pub const PATHS: &[LearningPath] = &[POWER_PATH, ARDUINO_PATH, MEGA_PATH];

const POWER_PATH: LearningPath = LearningPath {
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
                Item { requires: Some(&["divider"]), ..lesson("coin_cell", "電池の内部抵抗 (負荷で電圧が下がる)", "電源の電圧が、電流を流すと下がる。これを小さくするのが安定化電源の役目", Some("電池に抵抗をつないで、つなぐ前後の端子電圧を測り、内部抵抗を求めます")) },
            ],
        },
        Stage {
            name: "2. 基礎: 電源をつくる部品",
            about: "ダイオード・コンデンサ・ツェナー・トランジスタ。電圧を整え、ならし、一定にし、電流を増やす部品の働きです",
            items: &[
                Item { requires: Some(&["led"]), ..lesson("diode_curve", "ダイオードの特性", "約 0.7V (ショットキーは約 0.3V) で流れ始め、そのあとは電圧がほとんど増えない", Some("ダイオードの順方向電圧を、シリコンとショットキーで測り、温めて変化を見ます")) },
                Item { requires: Some(&["diode_curve"]), ..lesson("rectifier", "交流を直流にする (整流・平滑・リップル)", "整流・平滑コンデンサ・リップルの大きさ。シミュレーションは商用電圧 (100V) を使いますが、実機では使いません", Some("実機は低電圧の交流 (小さなトランスの出力など) だけで行い、オシロ (またはテスタの交流レンジ) でリップルを測って、容量を変えて比べます。商用電源 (100V) の配線は、危険なので行いません")) },
                Item { build: true, requires: Some(&["diode_curve", "divider"]), ..lesson("zener", "ツェナーで電圧を一定にする", "もっとも簡単な安定化。入力が変わっても動きにくいが、負荷が重いと崩れる。自分で作るチャレンジつき", Some("ツェナーと抵抗で 5.1V を作り、負荷と入力を変えて電圧の変化を測ります")) },
                Item { requires: Some(&["led"]), ..lesson("bjt_switch", "トランジスタのスイッチ", "ベース電流でコレクタ電流を制御する。電流を増やす部品の入口", Some("トランジスタで LED をオン・オフし、ベース電流とコレクタ電流を測ります")) },
                Item { build: true, requires: Some(&["zener", "bjt_switch"]), ..lesson("emitter_follower", "エミッタフォロワ (電流を増やす)", "電圧はそのまま、流せる電流を増やす。直列パス型電源の中核。大電流では発熱する", Some("ツェナーの電圧を、トランジスタで大電流にし、負荷を変えて出力と温度を測ります")) },
            ],
        },
        Stage {
            name: "3. 応用: 安定化のしくみ",
            about: "出力の電圧を測って、ずれたら直す (帰還)。これが安定化電源の本質です",
            items: &[
                Item { requires: Some(&["divider"]), ..lesson("opamp_noninv", "オペアンプで信号を大きくする (非反転増幅)", "帰還の考え方。抵抗の比がゲインを決める", Some("オペアンプの増幅回路を作り、入力と出力をテスタやオシロで測ります")) },
                Item { build: true, requires: Some(&["opamp_noninv"]), ..lesson("opamp_inv", "オペアンプの反転増幅 (仮想接地)", "反転入力が 0V に保たれる仕組み", None) },
                Item { build: true, requires: Some(&["opamp_inv", "emitter_follower"]), ..lesson("series_regulator", "基準電圧と帰還で安定化する (直列レギュレータ)", "オペアンプ + 基準電圧 + パストランジスタで、自作の安定化電源を組む", Some("負荷を変えたとき (ロードレギュレーション)、入力を変えたとき (ラインレギュレーション) の出力電圧を測ります")) },
                Item { build: true, requires: Some(&["emitter_follower"]), ..lesson("regulator_78xx", "三端子レギュレータ (78xx / LDO) を使う", "入出力のコンデンサ、ドロップアウト電圧、使い方と選び方", Some("7805 で 5V を作り、入力と負荷を変えて出力と温度を測ります")) },
                Item { requires: Some(&["zener"]), ..lesson("tl431_ref", "TL431 (シャントレギュレータ): 好きな電圧の基準を作る", "REF を 2.495V に保つ部品。分圧の比で、出力を 2.5V 以上の好きな値にできる。負荷が重すぎると崩れる", Some("TL431 に 2 本の抵抗をつなぎ、出力が 2.495V × (1 + R2 ÷ R3) になるか測ります")) },
                Item { requires: Some(&["regulator_78xx"]), ..lesson("ldo", "LDO (低ドロップアウト) レギュレータ", "入力の余裕 (ドロップアウト電圧) が小さくても動く。USB の 5V や電池から 3.3V を作る。電圧差は熱になる", Some("入力を下げながら出力を測り、出力が落ち始める入力を、データシートと比べます")) },
                Item { requires: Some(&["regulator_78xx"]), ..lesson("heat", "熱と定格を考える", "電圧差 × 電流 = 熱。放熱板が必要になる理由。定格チェックの見方", Some("負荷を増やしながらレギュレータの温度を測り (触って確かめる前に、温度計で)、計算と比べます")) },
                Item { requires: Some(&["diode_curve", "regulator_78xx"]), ..lesson("protection", "電源を守る (逆接続・過電流・ヒューズ)", "壊れ方を知って、守る部品を足す", Some("ヒューズや逆接続保護ダイオードの働きを、安全な条件 (低電圧・電流制限つき) で確かめます")) },
                Item { requires: Some(&["protection"]), ..lesson("coil_layer_short", "コイルのレアショート (連鎖して壊れる)", "1つの故障が次の部品の故障を呼ぶ連鎖。ヒューズで止められる場合と、止められない場合", Some("リレーのコイル抵抗をテスタで測り、定格電流の内側か確かめます。コイルを実際にショートさせる実験はしません")) },
                Item { requires: Some(&["rectifier", "regulator_78xx"]), ..lesson("variable_supply", "ダイヤルで電圧を変える電源 (トランス + LM317)", "100V → トランス → 整流 → 可変レギュレータ。ダイヤル(ボリューム)で出力が変わる式と、出せない電圧の理由", Some("低電圧の電源 (15〜18V の AC アダプタ) で LM317 を組み、ボリュームを回して出力を測ります。100V は実験に使いません")) },
                Item { requires: Some(&["regulator_78xx", "heat"]), ..lesson("buck","降圧コンバータ (スイッチング電源)", "発展: 熱にしない電源。リニアとの効率の違い", None) },
            ],
        },
        Stage {
            name: "4. 製作: 回路図から基板、実機まで",
            about: "設計した電源を、基板にして、組み立てて、性能を測ります",
            items: &[
                Item { id: "draw_psu", title: "安定化電源の回路図を自分で設計する (最終課題)", about: "目標 (入力 9〜12V → 出力 5V、逆接続でも壊れない) だけ示し、自分で部品を選んで置き、配線する", kind: Kind::Build, requires: Some(&["series_regulator", "regulator_78xx", "protection"]), build: true, real: None },
                Item { id: "board", title: "回路図から基板をつくる", about: "取り込み・配置・配線・DRC・発注の準備", kind: Kind::Board(0), requires: Some(&["draw_psu"]), build: false, real: Some("発注するなら、少量・少額で試します") },
                Item { id: "board_copper", title: "配線の太さと電圧降下を確かめる", about: "細い配線で電圧が落ちるのをシミュレーションで見て、電流から必要な太さを決めて直す", kind: Kind::Board(1), requires: Some(&["board"]), build: false, real: Some("電源の入口と負荷のピンの電圧を、テスタで測って比べます (大きな電流を流すときは、配線や部品の発熱に注意します)") },
                Item { id: "board_fix", title: "発注前チェックをすべて通す (チャレンジ)", about: "部品が基板の外・配線が切れている・配線が細い・発注先のルール違反、という基板を、チェックの表示を頼りに自分で直す", kind: Kind::Board(2), requires: Some(&["board"]), build: false, real: Some("発注の前に、発注先のページで最新の条件を必ず確かめます") },
                Item { id: "build_psu", title: "組み立てて、性能を測る", about: "部品表・組み立て・測定。測った値をスペック表の「実測」欄に書いて、シミュレーションとの差とその理由を考える", kind: Kind::Measure, requires: Some(&["board_fix", "board_copper"]), build: false, real: Some("完成した電源の入力電圧・出力電圧を、負荷をつないだ状態とはずした状態で測り、シミュレーションと比べます") },
            ],
        },
    ],
};

/// A planned item of a tree path: shown on the map, not yet written (all items of the Arduino path are written; the Mega path has some).
const fn plan(id: &'static str, title: &'static str, about: &'static str, requires: &'static [&'static str], real: Option<&'static str>) -> Item {
    Item { requires: Some(requires), ..planned(id, title, about, real) }
}

/// The second path: a tree. Each item says what to finish first (`requires`), so the independent branches (power,
/// clock, reset, pins) can be learned in any order and meet at the schematic. Items without a lesson yet are `plan`.
const ARDUINO_PATH: LearningPath = LearningPath {
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
                Item { requires: Some(&["pin_led"]), ..lesson("flyback", "リレーやモータをつなぐ (フライバックダイオード)", "コイルの電流を急に切ると、大きな逆起電力が出てトランジスタを壊す。コイルと並列にダイオードを入れて止める", Some("リレー (または小さなモータ) をトランジスタで駆動し、ダイオードの向きを確かめます")) },
                Item { requires: Some(&["pin_led"]), ..lesson("pwm", "PWM とフィルタ (analogWrite でアナログ電圧を作る)", "High と Low の割合 (デューティ) と RC フィルタで、途中の電圧を作る。揺れと反応の速さは引き換え。重い負荷には使えない", Some("analogWrite の値を変えて、フィルタの出力をテスタで測ります")) },
                Item { requires: Some(&["pullup"]), ..lesson("levels", "3.3V と 5V をつなぐ (レベル変換)", "電圧の違う相手を直接つなぐと壊れる・読めない。分圧と MOSFET (BSS138) での変換と、プルアップによる波形の遅れ", Some("5V のピンの出力を、分圧してから 3.3V の部品に入れる前に、テスタで測って確かめます")) },
                Item { requires: Some(&["levels", "reset"]), ..lesson("serial", "シリアル通信と USB 変換 (発展)", "TX/RX の交差、ボーレートと 1 ビットの長さ、配線の抵抗と容量でなまると文字化けする", Some("端末ソフトで 9600bps と別のボーレートを比べて、文字化けを見ます")) },
            ],
        },
        Stage {
            name: "3b. 発展: USB につなぐ (電気の基本)",
            about: "USB の電源 (VBUS)、速度の見分け、信号線の波形。USB のプロトコルそのものは扱わず、電気の面を確かめます",
            items: &[
                Item { requires: Some(&["decoupling"]), ..lesson("usb_power", "USB の電源 (VBUS)", "ケーブルの抵抗で電圧が下がること、差し込んだ瞬間の突入電流と、VBUS の容量の目安 (10µF 程度)", Some("USB テスターなどで、機器の端の VBUS の電圧と電流を測ります")) },
                Item { requires: Some(&["pullup"]), ..lesson("usb_speed", "USB の速度の見分け", "D+ か D− を 1.5kΩ で引くと、ホストが機器の速度を知る。プルアップがないと認識されない", Some("機器をつなぐ前後の D+ / D− の電圧を測ります (止まった状態で)")) },
                Item { requires: Some(&["usb_speed"]), ..lesson("usb_signal", "USB の信号線と ESD 保護の容量", "12Mbit/s の 1 ビットは約 83ns。ESD 保護部品の容量が大きいと、立ち上がりが遅れて読めなくなる", None) },
                Item { id: "board_usb", title: "USB の配線 (差動ペア)", about: "D+ と D− を並べて、同じ長さで、短く引く。差動ペアの機能と、長さの差の確かめ方", kind: Kind::Board(4), build: false, requires: Some(&["usb_signal", "board"]), real: None },
            ],
        },
        Stage {
            name: "4. 設計して基板に",
            about: "ここまでの部品を1枚にまとめて、基板にします",
            items: &[
                Item { id: "draw_arduino", title: "Arduino 互換ボードの回路図を自分で描く (最終課題)", about: "ATmega328P・水晶・リセット・バイパス・LED・書き込みヘッダを、目標だけ見て自分で配線する。つなぎ方と部品の値を判定する", kind: Kind::Build, build: true, requires: Some(&["decoupling", "crystal", "reset", "pin_led", "serial"]), real: None },
                Item { id: "board_arduino", title: "基板にする: 置き方の勘所と配線", about: "水晶はマイコンのそば・バイパスコンデンサは電源ピンのそば。通らない線は、グリッドを細かくして直す。DRC と発注の準備まで", kind: Kind::Board(3), build: false, requires: Some(&["draw_arduino", "board"]), real: Some("発注するなら、少量・少額で試します") },
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
const MEGA_PATH: LearningPath = LearningPath {
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
                plan("ch340g", "CH340G: USB-シリアル変換", "USB の D+ / D− をシリアルに変える IC。12MHz の水晶と、V3 ピンのコンデンサが要る。DTR でリセットをかける仕組み", &["usb_signal", "serial"], Some("PC につなぎ、シリアルモニタに文字が出るか確かめます")),
            ],
        },
        Stage {
            name: "2. 100 ピンの基板",
            about: "0.5mm ピッチの 100 ピンの IC は、自動配線が通せない線が残ります。残った線を、手で仕上げます",
            items: &[plan("board_mega", "基板を手で仕上げる (自動配線で通らない線)", "自動配置と自動配線のあと、18 本の線が残った基板を、部品の置き直し・配線のやり直し・ビアで、手で通しきる", &["board_arduino"], None)],
        },
        Stage {
            name: "3. 作って動かす",
            about: "100 ピンのチップを、はんだ付けします",
            items: &[Item { id: "solder_tqfp100", title: "100 ピン (0.5mm ピッチ) のはんだ付け", about: "TQFP-100 のはんだ付け。ブリッジの直し方、フラックスの使い方、電源ピンの点検と、CH340 の COM ポートが出るまで", kind: Kind::Hands, build: false, requires: Some(&["board_mega", "solder"]), real: Some("まず練習用の基板で、0.5mm ピッチのチップを試します") }],
        },
    ],
};

/// Every item of a path, in order.
pub fn items(path: &LearningPath) -> impl Iterator<Item = &'static Item> + '_ {
    path.stages.iter().flat_map(|s| s.items.iter())
}

/// Whether an item is finished: a lesson is marked by its id, a challenge-only item by its challenge's key.
pub fn item_done(progress: &Progress, item: &Item) -> bool {
    if item.kind == Kind::Build { progress.is_done(&crate::schematic::challenge_progress_key(item.id)) } else { progress.is_done(item.id) }
}

/// Progress of a path: (finished, available now, all).
pub fn summary(path: &LearningPath, progress: &Progress) -> (usize, usize, usize) {
    let all = items(path).count();
    let available = items(path).filter(|i| i.kind != Kind::Planned).count();
    let finished = items(path).filter(|i| i.kind != Kind::Planned && item_done(progress, i)).count();
    (finished, available, all)
}

/// Every item of every path (a prerequisite may point into another path).
fn all_items() -> impl Iterator<Item = &'static Item> {
    PATHS.iter().flat_map(items)
}

/// The item with this id, in any path.
pub fn find_item(id: &str) -> Option<&'static Item> {
    all_items().find(|i| i.id == id)
}

/// What is best finished before `item`: its `requires`, or the item just before it in `path` (planned ones included,
/// for drawing the map).
pub fn requirements(path: &LearningPath, item: &Item) -> Vec<&'static Item> {
    let ids: Vec<&str> = match item.requires {
        Some(r) => r.to_vec(),
        None => items(path).take_while(|i| i.id != item.id).last().map(|i| vec![i.id]).unwrap_or_default(),
    };
    ids.into_iter().filter_map(find_item).collect()
}

/// `requirements` without the planned items: they cannot be learned yet, so they never hold anything back.
pub fn prerequisites(path: &LearningPath, item: &Item) -> Vec<&'static Item> {
    requirements(path, item).into_iter().filter(|i| i.kind != Kind::Planned).collect()
}

/// A path is drawn as a map when its items name their prerequisites (a straight line is just a list).
pub fn is_tree(path: &LearningPath) -> bool {
    items(path).any(|i| i.requires.is_some())
}

/// Rows below the roots: 0 for an item with no prerequisite inside the path, else one more than its deepest one.
fn level(path: &LearningPath, item: &Item, guard: usize) -> usize {
    if guard > 200 {
        return 0;
    }
    requirements(path, item).iter().filter(|p| items(path).any(|i| i.id == p.id)).map(|p| level(path, p, guard + 1) + 1).max().unwrap_or(0)
}

/// The items to do now, in path order: available, not finished, and every prerequisite finished. A tree has several
/// at once; a straight line has one.
pub fn recommended(path: &LearningPath, progress: &Progress) -> Vec<&'static Item> {
    items(path).filter(|i| i.kind != Kind::Planned && !item_done(progress, i) && prerequisites(path, i).iter().all(|p| item_done(progress, p))).collect()
}

/// The first of `recommended`.
pub fn next_item(path: &LearningPath, progress: &Progress) -> Option<&'static Item> {
    recommended(path, progress).into_iter().next()
}

// ------------------------------------------------------------------ the window

/// What the learner asked for in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Lesson(&'static str),
    /// The "作ってみよう" challenge of a lesson (by the lesson's id).
    Build(&'static str),
    Board(usize),
    /// The guide of the bench step.
    Measure,
    /// The checklist of a hands-on item (by the item's id).
    Hands(&'static str),
}

/// Colour of the grey explanation lines (`weak()` was too faint on the dark background).
pub(crate) const NOTE: Color32 = Color32::from_gray(175);

/// Wrapped Japanese text with a taller line (the font's default line height makes lines touch).
pub(crate) fn spaced(text: &str) -> egui::RichText {
    egui::RichText::new(text).line_height(Some(20.0))
}

/// The action that opens an item, if it can be opened yet.
fn item_action(item: &Item) -> Option<Action> {
    match item.kind {
        Kind::Lesson(id) => Some(Action::Lesson(id)),
        Kind::Build => Some(Action::Build(item.id)),
        Kind::Board(n) => Some(Action::Board(n)),
        Kind::Measure => Some(Action::Measure),
        Kind::Hands => Some(Action::Hands(item.id)),
        Kind::Planned => None,
    }
}

/// What the learner chose on the first-launch card.
pub enum Welcome {
    /// Start the recommended first lesson.
    Start(Action),
    /// Open the learning-path window.
    Path,
    /// Open the operation guide.
    Guide,
    /// Closed with the × (or the "あとで" button).
    Close,
}

/// The card shown over an empty sheet on the very first launch: where to begin, in three buttons.
pub fn welcome_card(ctx: &egui::Context, progress: &Progress) -> Option<Welcome> {
    let mut choice = None;
    let mut open = true;
    egui::Window::new("ようこそ circuitCad へ").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).default_width(460.0).show(ctx, |ui| {
        ui.label(spaced("回路図を描き、動かして確かめ、基板にして、発注用のデータまで作れるソフトです。電子工作が初めてでも、レッスンを順にたどれば進められます。"));
        ui.add_space(6.0);
        if let Some((next, action)) = next_item(&PATHS[0], progress).and_then(|i| item_action(i).map(|a| (i, a))) {
            let button = egui::Button::new(egui::RichText::new(format!("▶ 最初のレッスンを始める: {}", next.title)).strong());
            if ui.add(button).on_hover_text("画面の案内にそって、部品を置いて配線し、電圧を測ります").clicked() {
                choice = Some(Welcome::Start(action));
            }
        }
        if ui.button("学習パス全体を見る").on_hover_text("電圧を分ける回路から、安定化電源・基板づくりまでの道すじを開きます (「学ぶ」メニューからいつでも開けます)").clicked() {
            choice = Some(Welcome::Path);
        }
        if ui.button("操作ガイドを開く (F1)").on_hover_text("部品の置き方・配線・シミュレーションのやり方を説明します").clicked() {
            choice = Some(Welcome::Guide);
        }
        ui.add_space(4.0);
        ui.label(spaced("自由に描きたいときは、このカードを閉じて、左の一覧から部品を選んで画面をクリックします。できあがった回路を先に見たいときは、「ファイル → サンプル回路を開く」から。").color(NOTE));
        if ui.small_button("あとで (もう表示しない)").on_hover_text("このカードを閉じます。次の起動から表示しません").clicked() {
            choice = Some(Welcome::Close);
        }
    });
    if !open && choice.is_none() {
        choice = Some(Welcome::Close);
    }
    choice
}

#[derive(Default)]
pub struct LearnWindow {
    pub open: bool,
    /// Which of `PATHS` is shown.
    selected: usize,
}

impl LearnWindow {
    /// Shows the path with this id (command line `--learn <id>`); an unknown id leaves the choice alone.
    pub fn select(&mut self, id: &str) {
        if let Some(k) = PATHS.iter().position(|p| p.id == id) {
            self.selected = k;
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, progress: &mut Progress) -> Option<Action> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut action = None;
        let mut chosen = self.selected.min(PATHS.len() - 1);
        let path = &PATHS[chosen];
        egui::Window::new("学習パス").open(&mut open).default_width(if is_tree(path) { 760.0 } else { 600.0 }).default_height(720.0).vscroll(true).show(ctx, |ui| {
            if PATHS.len() > 1 {
                ui.horizontal_wrapped(|ui| {
                    for (k, p) in PATHS.iter().enumerate() {
                        ui.selectable_value(&mut chosen, k, p.title).on_hover_text("この学習パスに切り替えます (進み具合は、どのパスでも同じレッスンなら共通です)");
                    }
                });
                ui.separator();
            }
            ui.heading(path.title);
            ui.label(spaced(path.goal));
            let (finished, available, all) = summary(path, progress);
            ui.add(egui::ProgressBar::new(finished as f32 / available.max(1) as f32).text(format!("完了 {finished} / いま学べる {available} (全体 {all})")));
            let next_ids: Vec<&str> = recommended(path, progress).iter().map(|i| i.id).collect();
            if let Some(next) = next_item(path, progress) {
                ui.horizontal_wrapped(|ui| {
                    ui.label("次のおすすめ:");
                    ui.colored_label(Color32::from_rgb(220, 150, 40), next.title);
                    if next_ids.len() > 1 {
                        ui.label(format!("(ほかに {} 件、すぐ学べます)", next_ids.len() - 1));
                    }
                });
            } else {
                if available == 0 {
                    ui.label(spaced("このパスのレッスンは準備中です。道すじ (地図) だけ先に見られます").color(NOTE));
                } else {
                    ui.colored_label(Color32::from_rgb(60, 160, 90), "いま学べるレッスンは、すべて完了しました。残りは準備中です");
                }
            }
            if is_tree(path) {
                ui.add_space(6.0);
                egui::CollapsingHeader::new("学習の地図 (上から下へ進みます。線は「先に済ませると分かりやすい」の関係です)").default_open(true).show(ui, |ui| {
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        if let Some(a) = tree_map(ui, path, progress, &next_ids) {
                            action = Some(a);
                        }
                    });
                    ui.label(spaced("緑 = 完了、橙の枠 = いますぐ学べる、灰 = 準備中。順番は強制ではなく、どれからでも始められます").color(NOTE));
                });
            }
            for stage in path.stages {
                ui.add_space(8.0);
                ui.strong(stage.name);
                ui.label(spaced(stage.about).color(NOTE));
                for item in stage.items {
                    let done = item_done(progress, item);
                    let (mark, color) = match item.kind {
                        Kind::Planned => ("…", Color32::from_gray(120)),
                        _ if done => ("✔", Color32::from_rgb(60, 160, 90)),
                        _ if next_ids.contains(&item.id) => ("▶", Color32::from_rgb(220, 150, 40)),
                        _ => ("○", Color32::from_gray(150)),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(color, mark);
                        let title = egui::RichText::new(item.title).strong();
                        ui.label(if item.kind == Kind::Planned { title.color(Color32::from_gray(130)) } else { title });
                        if item.kind == Kind::Planned {
                            ui.label(egui::RichText::new("(準備中)").weak());
                        } else if item.kind != Kind::Build && ui.small_button(if done { "もう一度" } else { "始める" }).on_hover_text("この内容のレッスンを開きます").clicked() {
                            action = Some(match item.kind {
                                Kind::Lesson(id) => Action::Lesson(id),
                                Kind::Measure => Action::Measure,
                                Kind::Hands => Action::Hands(item.id),
                                Kind::Board(n) => Action::Board(n),
                                _ => Action::Measure,
                            });
                        }
                        if item.build {
                            let passed = progress.is_done(&crate::schematic::challenge_progress_key(item.id));
                            if ui.small_button(if passed { "✔ 作ってみよう" } else { "作ってみよう" }).on_hover_text("目標だけを示して、自分で部品を置き、配線し、値を決める練習です。作ったあと、判定ボタンで調べます").clicked() {
                                action = Some(Action::Build(item.id));
                            }
                        }
                        if item.real.is_some() {
                            ui.colored_label(Color32::from_rgb(90, 170, 220), "[実験あり]").on_hover_text(item.real.unwrap_or_default());
                        }
                    });
                    ui.indent(("path_item", path.id, item.id), |ui| {
                        ui.label(spaced(item.about).color(NOTE));
                        if item.requires.is_some_and(|r| !r.is_empty()) {
                            let names: Vec<&str> = requirements(path, item).iter().map(|p| p.title).collect();
                            ui.label(spaced(&format!("先に済ませると分かりやすい: {}", names.join("、"))).color(NOTE));
                        }
                    });
                    ui.add_space(6.0);
                }
            }
            ui.add_space(8.0);
            ui.separator();
            ui.label(spaced("[実験あり] = 実機で確かめられる実験があります (ポインタを置くと内容が出ます)。実験は、安全のため低い電圧 (12V 以下) だけで行い、商用電源 (100V) の配線は行いません"));
            if let Some(e) = &progress.error {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
            }
            if ui.small_button("進み具合をリセット").on_hover_text("完了の印をすべて消して、最初からやり直します").clicked() {
                progress.reset();
            }
        });
        self.selected = chosen;
        if !open {
            self.open = false;
        }
        action
    }
}

/// The map of a tree path: one row per level, an item per box, a line from each prerequisite down to what needs it.
fn tree_map(ui: &mut egui::Ui, path: &LearningPath, progress: &Progress, next_ids: &[&str]) -> Option<Action> {
    const W: f32 = 160.0;
    const H: f32 = 54.0;
    const GX: f32 = 12.0;
    const GY: f32 = 26.0;
    let all: Vec<&'static Item> = items(path).collect();
    let levels: Vec<usize> = all.iter().map(|i| level(path, i, 0)).collect();
    let rows = levels.iter().max().copied().unwrap_or(0) + 1;
    let mut in_row = vec![0usize; rows];
    let slot: Vec<usize> = levels.iter().map(|&l| { in_row[l] += 1; in_row[l] - 1 }).collect();
    let widest = in_row.iter().max().copied().unwrap_or(1);
    let size = egui::vec2(widest as f32 * (W + GX) - GX, rows as f32 * (H + GY) - GY);
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let boxes: Vec<egui::Rect> = (0..all.len())
        .map(|k| {
            let left = (widest - in_row[levels[k]]) as f32 * (W + GX) / 2.0 + slot[k] as f32 * (W + GX);
            egui::Rect::from_min_size(area.min + egui::vec2(left, levels[k] as f32 * (H + GY)), egui::vec2(W, H))
        })
        .collect();
    let line = ui.visuals().widgets.noninteractive.fg_stroke.color;
    for (k, item) in all.iter().enumerate() {
        for p in requirements(path, item) {
            if let Some(j) = all.iter().position(|i| i.id == p.id) {
                ui.painter().line_segment([boxes[j].center_bottom(), boxes[k].center_top()], egui::Stroke::new(1.0, line));
            }
        }
    }
    let mut action = None;
    for (k, item) in all.iter().enumerate() {
        let (fill, stroke, text) = if item.kind == Kind::Planned {
            (Color32::from_gray(60), Color32::from_gray(90), Color32::from_gray(150))
        } else if item_done(progress, item) {
            (Color32::from_rgb(40, 100, 60), Color32::from_rgb(60, 160, 90), Color32::WHITE)
        } else if next_ids.contains(&item.id) {
            (Color32::from_gray(45), Color32::from_rgb(220, 150, 40), Color32::WHITE)
        } else {
            (Color32::from_gray(45), Color32::from_gray(120), Color32::from_gray(210))
        };
        let response = ui.interact(boxes[k], ui.id().with(("tree_item", item.id)), egui::Sense::click());
        let painter = ui.painter().with_clip_rect(boxes[k]);
        painter.rect_filled(boxes[k], 4.0, fill);
        painter.rect_stroke(boxes[k], 4.0, egui::Stroke::new(if next_ids.contains(&item.id) { 2.0 } else { 1.0 }, stroke), egui::StrokeKind::Inside);
        let galley = painter.layout(item.title.to_string(), egui::FontId::proportional(14.0), text, W - 10.0);
        painter.galley(boxes[k].min + egui::vec2(5.0, 4.0), galley, text);
        let response = response.on_hover_text(if item.kind == Kind::Planned { format!("{}
(準備中)", item.about) } else { item.about.to_string() });
        if response.clicked() {
            action = item_action(item);
        }
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("circuitcad-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("progress.json")
    }

    #[test]
    fn progress_is_saved_and_read_back() {
        let f = temp_file("progress");
        let mut p = Progress::load(&f);
        assert_eq!(p.count(), 0, "a missing file is an empty record");
        assert!(p.mark("divider") && p.mark("led"));
        assert!(!p.mark("divider"), "marking again is not new");
        assert!(p.error.is_none(), "{:?}", p.error);
        let q = Progress::load(&f);
        assert!(q.is_done("divider") && q.is_done("led") && !q.is_done("buck") && q.count() == 2);
        let mut q = q;
        q.reset();
        assert_eq!(Progress::load(&f).count(), 0);
        let _ = std::fs::remove_dir_all(f.parent().unwrap());
    }

    #[test]
    fn welcome_is_shown_once_and_not_to_learners_with_progress() {
        let f = temp_file("welcome");
        let mut p = Progress::load(&f);
        assert!(p.needs_welcome(), "a fresh start shows the card");
        p.mark_welcomed();
        assert!(!Progress::load(&f).needs_welcome(), "answered once: never again");
        let mut q = Progress::load(&f);
        q.reset();
        assert!(!Progress::load(&f).needs_welcome(), "reset keeps the answer");
        let g = temp_file("welcome-old");
        let mut old = Progress::load(&g);
        old.mark("divider");
        let mut old = Progress::load(&g);
        assert!(!old.needs_welcome(), "someone with finished lessons is not welcomed again");
        old.reset();
        let _ = std::fs::remove_dir_all(f.parent().unwrap());
        let _ = std::fs::remove_dir_all(g.parent().unwrap());
    }

    #[test]
    fn a_broken_record_file_is_treated_as_empty() {
        let f = temp_file("broken");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "{ not json").unwrap();
        assert_eq!(Progress::load(&f).count(), 0);
        let _ = std::fs::remove_dir_all(f.parent().unwrap());
    }

    #[test]
    fn every_lesson_of_a_path_exists_and_ids_are_unique() {
        let known = crate::schematic::lesson_ids();
        let mut seen = std::collections::HashSet::new();
        for path in PATHS {
            for item in items(path) {
                assert!(seen.insert(item.id), "duplicate id {}", item.id);
                if let Kind::Lesson(id) = item.kind {
                    assert!(known.contains(&id), "{} refers to the missing lesson {id}", item.title);
                    assert_eq!(item.id, id);
                }
                if item.kind == Kind::Hands {
                    assert!(crate::bench_guide::guide(item.id).is_some(), "{} has no hands-on guide", item.title);
                }
            }
        }
    }

    /// An item that offers "作ってみよう" must have a challenge behind it.
    #[test]
    fn items_with_a_build_button_have_a_challenge() {
        let mut with = 0;
        for item in items(&PATHS[0]).filter(|i| i.build) {
            with += 1;
            assert!(crate::schematic::challenge_of_lesson(item.id).is_some() || crate::schematic::challenge_exists(item.id), "{} has no challenge", item.title);
        }
        assert!(with >= 5, "{with} items offer a challenge");
    }

    /// Every `requires` names an existing item, nothing needs itself through a loop, and the tree has several roots.
    #[test]
    fn the_prerequisites_of_every_path_exist_and_never_loop() {
        for path in PATHS {
            for item in items(path) {
                for id in item.requires.unwrap_or_default() {
                    assert!(find_item(id).is_some(), "{} needs the missing item {id}", item.title);
                    assert_ne!(*id, item.id, "{} needs itself", item.title);
                }
                // with no loop the depth is below the number of items; a loop would run into the guard
                assert!(level(path, item, 0) < items(path).count(), "{} is too deep (a loop?)", item.title);
            }
        }
    }

    #[test]
    fn a_tree_path_recommends_several_independent_items_and_waits_for_all_prerequisites() {
        let mut progress = Progress::default();
        // planned items are never recommended, and a planned prerequisite holds nothing back. At the start only the root can
        // be done: the data sheet lesson (the others need it, or the divider of the power path)
        let ready = |p: &Progress| recommended(&ARDUINO_PATH, p).iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(ready(&progress), ["datasheet"]);
        assert!(recommended(&ARDUINO_PATH, &progress).iter().all(|i| i.kind != Kind::Planned));
        // the divider (a lesson of the power path) opens the two that need only it; the pull-up opens the reset; the data
        // sheet opens the crystal and the pin LED (which also needs the LED lesson of the power path)
        progress.mark("divider");
        assert!(ready(&progress).contains(&"pullup") && ready(&progress).contains(&"decoupling"));
        progress.mark("pullup");
        assert!(ready(&progress).contains(&"reset") && ready(&progress).contains(&"levels"));
        assert!(!ready(&progress).contains(&"crystal"));
        progress.mark("datasheet");
        assert!(ready(&progress).contains(&"crystal"));
        assert!(!ready(&progress).contains(&"pin_led"), "the pin LED also needs the LED lesson");
        progress.mark("led");
        assert!(ready(&progress).contains(&"pin_led"));
        let mut progress = Progress::default();
        assert!(is_tree(&ARDUINO_PATH) && is_tree(&POWER_PATH));
        // a straight line (items without `requires`) needs just the item before it: a two-item test path that reuses real ids
        const LINE: LearningPath = LearningPath {
            id: "test_line",
            title: "",
            goal: "",
            stages: &[Stage { name: "", about: "", items: &[lesson("divider", "", "", None), lesson("led", "", "", None)] }],
        };
        assert!(!is_tree(&LINE));
        let led_in_line = items(&LINE).nth(1).unwrap();
        assert_eq!(prerequisites(&LINE, led_in_line).iter().map(|i| i.id).collect::<Vec<_>>(), ["divider"], "a straight line needs the item before it");
        assert_eq!(recommended(&LINE, &progress).iter().map(|i| i.id).collect::<Vec<_>>(), ["divider"]);
        progress.mark("divider");
        assert_eq!(recommended(&LINE, &progress).iter().map(|i| i.id).collect::<Vec<_>>(), ["led"]);
        // the Arduino items name power-path items from another path
        let pin_led = find_item("pin_led").unwrap();
        assert!(requirements(&ARDUINO_PATH, pin_led).iter().any(|i| i.id == "led"));
        // roots sit on row 0, the final tasks below everything they need
        assert_eq!(level(&ARDUINO_PATH, find_item("datasheet").unwrap(), 0), 0);
        assert!(level(&ARDUINO_PATH, find_item("draw_arduino").unwrap(), 0) > level(&ARDUINO_PATH, find_item("reset").unwrap(), 0));
    }

    #[test]
    fn the_mega_path_builds_on_the_arduino_path_and_opens_after_its_prerequisites() {
        let mega = PATHS.iter().find(|p| p.id == "arduino_mega").unwrap();
        assert!(is_tree(mega));
        let mut progress = Progress::default();
        // the ADC lesson needs the data sheet and the pin LED (which need the power path's LED lesson)
        assert!(recommended(mega, &progress).is_empty());
        for id in ["divider", "led", "datasheet", "pin_led"] {
            progress.mark(id);
        }
        assert_eq!(recommended(mega, &progress).iter().map(|i| i.id).collect::<Vec<_>>(), ["adc_input"]);
        // the planned items hold nothing back and are never recommended
        assert!(items(mega).filter(|i| i.kind == Kind::Planned).count() >= 2);
        assert!(requirements(mega, find_item("ch340g").unwrap()).iter().any(|i| i.id == "serial"));
    }

    #[test]
    fn the_power_supply_path_walks_from_basics_to_building() {
        let p = &PATHS[0];
        assert_eq!(p.stages.len(), 4);
        let mut progress = Progress::default();
        let (done, available, all) = summary(p, &progress);
        assert_eq!(done, 0);
        assert!(available >= 12 && all >= available);
        // a tree: the two roots (the divider and the LED) can both be started; what finishing one opens depends on the branch
        let ready = |p: &Progress| recommended(&PATHS[0], p).iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(ready(&progress), ["divider", "led"]);
        assert_eq!(next_item(p, &progress).unwrap().id, "divider");
        progress.mark("divider");
        assert_eq!(ready(&progress), ["led", "coin_cell", "opamp_noninv"], "the divider opens the battery and the amplifier");
        progress.mark("led");
        assert!(ready(&progress).contains(&"diode_curve") && ready(&progress).contains(&"bjt_switch"), "{:?}", ready(&progress));
        // a lesson whose prerequisites are not all done is not recommended: the follower needs both the zener and the switch
        progress.mark("diode_curve");
        progress.mark("zener");
        assert!(!ready(&progress).contains(&"emitter_follower"));
        progress.mark("bjt_switch");
        assert!(ready(&progress).contains(&"emitter_follower"));
        // the last steps wait for the branches they join
        assert!(!ready(&progress).contains(&"draw_psu") && !ready(&progress).contains(&"build_psu"));
        // planned items are never recommended, however far the learner is
        for it in items(p).filter(|i| i.kind != Kind::Planned) {
            let key = if it.kind == Kind::Build { crate::schematic::challenge_progress_key(it.id) } else { it.id.to_string() };
            progress.mark(&key);
        }
        assert!(next_item(p, &progress).is_none());
    }
}
