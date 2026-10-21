//! "実機の作業ガイド": the hands-on steps of a learning path that the program cannot judge (soldering, flashing a bootloader).
//! A window with a safety note, tasks with tick boxes the learner ticks herself, and a short "if it does not work" table.
//! When every box is ticked the item is recorded as done (`learn::Progress`), like a lesson.

use crate::learn::{spaced, Progress, NOTE};
use eframe::egui::{self, Color32};
use std::collections::BTreeSet;

pub struct Task {
    pub title: &'static str,
    pub detail: &'static str,
    /// Things to tick off, one by one, when they are done.
    pub checks: &'static [&'static str],
}

pub struct Guide {
    /// The id of the learning-path item (the progress key).
    pub id: &'static str,
    pub title: &'static str,
    pub intro: &'static str,
    pub safety: &'static str,
    pub tasks: &'static [Task],
    /// (what you see, what to look at).
    pub trouble: &'static [(&'static str, &'static str)],
    /// Where this advice is only a general one: shown at the end.
    pub limits: &'static str,
}

const WARN: Color32 = Color32::from_rgb(230, 150, 40);
const OK: Color32 = Color32::from_rgb(60, 160, 90);

pub const SOLDER: Guide = Guide {
    id: "solder",
    title: "表面実装のはんだ付け (チップ部品と TQFP)",
    intro: "基板が届いたら、まず練習用 (または、うまくいかなくてもよい) の基板で、0805 のチップ部品と、足の間隔が 0.8mm の TQFP-32 (ATmega328P) のはんだ付けを練習します。はんだ付けは、手を動かして覚える技です。ここは、手順とコツの確認と、チェックリストです。",
    safety: "安全: こて先は 300℃ を超えて熱いので、こて台に必ず置き、持つのは握りの部分だけにします。はんだの煙 (フラックス) は吸わないように、換気をします。鉛入りのはんだは、作業のあとに手を洗います。はんだや部品が飛ぶことがあるので、保護メガネをかけます。こてのコードは、足やひじに引っかけないようにします。",
    tasks: &[
        Task {
            title: "1. 準備",
            detail: "道具: 温度調整つきのはんだごて (こて先は、細めの 1mm 以下)、はんだ (直径 0.5mm 以下・フラックス入り)、フラックス (ペンかペースト)、ピンセット、拡大鏡 (またはマイクロスコープ)、はんだ吸い取り線、テスタ。作業台は、燃えにくいマットの上にします。",
            checks: &["道具がそろっている", "換気ができて、こて台を置いた", "練習用の基板 (うまくいかなくてもよいもの) を用意した"],
        },
        Task {
            title: "2. チップ部品 (0805) をつける",
            detail: "片方のパッドに、少しだけはんだをのせます (予備はんだ)。ピンセットで部品をおさえ、その予備はんだを溶かして、部品を仮止めします。そのあと、反対側のパッドと端子に、こてとはんだを同時に当てて、つけます。最後に、仮止めした側を、もう一度温めて仕上げます。はんだは「少なめ」から始めて、足りなければ足します。",
            checks: &["片側を仮止めしてから、反対側をつけた", "両側のはんだが、パッドと部品の端子に、なだらかに広がっている (山盛りでも、はじかれてもいない)", "部品が傾いたり、片側が浮いて立ち上がったりしていない"],
        },
        Task {
            title: "3. TQFP-32 (0.8mm ピッチ) をつける",
            detail: "パッドにフラックスを塗ります。IC の 1 番ピンの印 (点やへこみ) を、基板の印に合わせて置き、対角の 2 か所のピンだけをはんだ付けして、固定します。位置がずれていないか、拡大鏡で確かめます (ずれていたら、固定したピンを温めて直します)。そのあと、フラックスをたっぷり塗り、こて先に少量のはんだをのせて、ピンの列に沿って、ゆっくり引きます (引きはんだ)。隣のピンとつながってしまった所 (ブリッジ) は、フラックスをつけて、吸い取り線ではんだを吸います。最後に、フラックスの残りを、アルコール (IPA) などでふき取ります。",
            checks: &["1 番ピンの向きを確かめてから、IC を固定した", "対角 2 か所で固定して、全部のピンがパッドの上に来ていることを、拡大鏡で確かめた", "引きはんだのあと、隣のピンとつながっている所 (ブリッジ) がない", "フラックスの残りをふき取った"],
        },
        Task {
            title: "4. 電源を入れる前に点検する",
            detail: "拡大鏡で、全部のピンを、1 本ずつ見ます (ブリッジ・つけ忘れ・ツヤのない「冷たいはんだ」)。テスタの抵抗レンジで、電源 (5V) と GND の間が、ショート (0Ω に近い値) していないことを確かめます。そのあと、電流の制限できる電源か USB で電源を入れ、IC やコンデンサが熱くないこと、5V が出ていることを確かめます。",
            checks: &["拡大鏡で、全部のピンを見た", "テスタで、5V と GND の間が、ショート (0Ω 付近) していない", "電源を入れて、5V が出ていて、部品が熱くない"],
        },
    ],
    trouble: &[
        ("隣のピンとつながった (ブリッジ)", "フラックスをたっぷり塗り、吸い取り線をはんだにのせて、その上からこてを当てて、はんだを吸います。ピンのつけ根を傷めないよう、こてを当てる時間は短く。"),
        ("つやのない、ざらざらしたはんだ (冷たいはんだ)", "フラックスを足して、こてで温め直し、はんだをなじませます。接触が不安定になりやすいところです。"),
        ("部品が片側だけ立ち上がった (ツームストーン)", "片側のはんだを温めて溶かし、ピンセットで押さえて、水平にしてから、もう一度つけます。"),
        ("電源を入れたら、部品が熱い・においがする", "すぐ電源を切ります。熱い部品の向き (ダイオード・コンデンサ・IC) や、ブリッジ・ショートを、もう一度点検します。"),
    ],
    limits: "ここは、一般的な手順とコツです。部品や基板、道具によって、こて先の温度 (一般に 320〜350℃ くらい) や、はんだの量の加減は変わります。部品のデータシートの「はんだ付けの条件 (温度と時間)」と、はんだやフラックスの注意書きを、確かめてください。",
};

pub const BOOTLOAD: Guide = Guide {
    id: "bootload",
    title: "ブートローダを書き込んで、LED を点滅させる",
    intro: "まっさらな ATmega328P には、ブートローダ (シリアルでプログラムを書き込むための小さなプログラム) が入っていません。書き込み器 (ICSP) で、最初に 1 回だけブートローダを書き込むと、あとは USB-シリアル変換から、Arduino IDE でスケッチを書き込めます。最後に、LED (D13) を 1 秒おきに点滅させます。",
    safety: "安全: 配線を変えるときは、必ず電源を切ります。電源は、ボードに 1 か所からだけ入れます (書き込み器と USB-シリアルの両方から 5V を入れない)。マイコンの「ヒューズ」(動作の設定) を、自分の判断で変えないでください。まちがえると、水晶を使う設定が外れて、書き込めなくなることがあります。ふつうは、IDE の「ブートローダを書き込む」が、ボードに合ったヒューズを自動で設定します。",
    tasks: &[
        Task {
            title: "1. 用意とつなぎ方",
            detail: "書き込み器 (USBasp など ICSP 書き込み器。または、別の Arduino に ArduinoISP のスケッチを入れたもの)、USB-シリアル変換 (FTDI など。電圧 5V と 3.3V を、ボードにあわせる)、Arduino IDE を用意します。ICSP のヘッダ (6 ピン) の並びは、1 = MISO、2 = VCC (電源)、3 = SCK、4 = MOSI、5 = RESET、6 = GND です。書き込み器の端子と、1 本ずつ対応させます。",
            checks: &["書き込み器 (ICSP) と、USB-シリアル変換を用意した", "ICSP の 6 ピンを、書き込み器の端子と対応させて、つないだ (MISO・MOSI・SCK・RESET・VCC・GND)", "ボードの電源は、1 か所 (書き込み器か USB-シリアル) だけにした"],
        },
        Task {
            title: "2. ブートローダを書き込む",
            detail: "Arduino IDE で、「ツール → ボード」を「Arduino Uno」に、「書き込み装置」を使う書き込み器 (USBasp など) にします。「ツール → ブートローダを書き込む」を実行します。エラーが出ずに終われば、ブートローダが入りました。",
            checks: &["ボードを「Arduino Uno」にした", "書き込み装置 (書き込み器) を選んだ", "「ブートローダを書き込む」が、エラーなく終わった"],
        },
        Task {
            title: "3. スケッチ (Blink) を、シリアルで書き込む",
            detail: "書き込み器をはずし、USB-シリアル変換をつなぎます (変換の TXD → マイコンの RXD、変換の RXD → マイコンの TXD、変換の DTR → 100nF → RESET、GND どうし)。「ツール → ポート」で変換のポートを選び、書き込み装置の選択を元に戻します。「ファイル → スケッチ例 → 01.Basics → Blink」を開き、「マイコンボードに書き込む」を押します。",
            checks: &["USB-シリアル変換を、TXD↔RXD を交差させてつないだ", "ポートを選んだ", "Blink を書き込めた (エラーなく終わった)"],
        },
        Task {
            title: "4. 確かめる",
            detail: "D13 の LED が、1 秒おきに点滅したら成功です。周期が 1 秒より大きくずれているときは、クロック (水晶・ヒューズ) の設定を疑います。シリアルモニタ (9600 bps) で、文字の送り・受け取りができるかも、試します (Serial.println を使うスケッチで)。",
            checks: &["LED (D13) が、1 秒おきに点滅した", "シリアルモニタ (9600 bps) で、文字を受け取れた"],
        },
    ],
    trouble: &[
        ("書き込み器で「デバイスの署名が読めない」(0x000000 や 0xFFFFFF) と出る", "ICSP の配線 (MISO・MOSI・SCK のつなぎ違い、ゆるみ)、ボードの電源、RESET の線、水晶がつながっているか、を確かめます。"),
        ("ブートローダは入ったが、LED が点滅しない", "D13 の LED の向きと抵抗、ボードの選択 (Arduino Uno)、スケッチが書き込まれているか、を確かめます。"),
        ("シリアルから書き込むと、「同期がとれない (not in sync)」と出る", "TXD と RXD を交差させたか、DTR → 100nF → RESET がつながっているか (リセットがかかるか)、ポートとボードの選択を確かめます。「リセット回路」のレッスンの回路です。"),
        ("点滅の周期が 1 秒より速い・遅い", "クロックが、想定 (16MHz の水晶) と違っています。水晶と 22pF のコンデンサのはんだ付け、「Arduino Uno」を選んでブートローダを書き込んだか、を確かめます。"),
    ],
    limits: "ここは、一般的な流れです。書き込み器や Arduino IDE の版によって、メニューの名前や手順は変わります。使う道具の最新の説明書を、確かめてください。ヒューズの値は、データシートと、使うブートローダ (Optiboot など) の資料で確かめるものです。",
};

pub const SOLDER_TQFP100: Guide = Guide {
    id: "solder_tqfp100",
    title: "100 ピン (0.5mm ピッチ) のはんだ付け (ATmega2560)",
    intro: "ATmega2560 は TQFP-100 で、足の間隔が 0.5mm、足が 100 本あります。TQFP-32 (0.8mm) の練習のあと、まず練習用の基板か、はんだ付け練習用の TQFP-100 の基板で試してから、本番の基板にします。足が細くて近いので、ブリッジ (隣とのつながり) はほぼ必ず起きます。慌てずに、フラックスをたっぷり使って直す前提で進めます。CH340G (SOP-16、1.27mm ピッチ) と、12MHz・16MHz の水晶 (3.2×2.5mm) も、同じ基板にあります。",
    safety: "安全: こて先は 300℃ を超えて熱いので、こて台に必ず置きます。細かい作業で顔が近づくので、はんだの煙 (フラックス) を吸わないよう換気をし、保護メガネをかけます。熱風はんだ付け器 (ホットエアー) を使うときは、風を人や燃えやすいものに向けず、使ったあと冷めるまで触りません。IPA (アルコール) は火気のそばで使いません。",
    tasks: &[
        Task {
            title: "1. 準備",
            detail: "道具: 温度調整つきのはんだごて (先が 0.5mm〜1mm の細いもの。ナイフ形も向く)、直径 0.3〜0.5mm のはんだ、フラックス (ペンかゲル。粘りのあるもの)、吸い取り線 (幅の細いもの)、ピンセット、拡大鏡 (できれば 10 倍以上かマイクロスコープ)、IPA と綿棒、テスタ。部品は、IC・水晶・CH340G・コンデンサなど、小さいものを先につけ、IC は最後でもよいですが、IC の下に隠れる配線 (ビア) がないか、先に確かめます。",
            checks: &["細いこて先と、細いはんだ、フラックスがある", "拡大鏡かマイクロスコープがある", "練習用の TQFP-100 の基板か、うまくいかなくてもよい基板を用意した"],
        },
        Task {
            title: "2. 小さい部品を先につける",
            detail: "コンデンサ (0603 や 0805)、抵抗、水晶、CH340G を、先につけます。水晶 (4 パッド) は、パッドにはんだを少し盛り、位置を合わせて 1 か所を仮止めしてから、残りをつけます。CH340G の 1 番ピンの印を、基板の印に合わせます。",
            checks: &["コンデンサと抵抗をつけた", "水晶 (12MHz と 16MHz) の向きと位置を確かめて、つけた", "CH340G の 1 番ピンの向きを確かめて、つけた"],
        },
        Task {
            title: "3. ATmega2560 (TQFP-100) をつける",
            detail: "パッドにフラックスを塗ります。1 番ピンの印を基板に合わせ、IC を置き、対角の 2 か所のピンだけをはんだ付けして固定します。拡大鏡で、100 本全部のピンが、パッドの真ん中に乗っているかを確かめます (ずれていたら、固定したピンを温めて直します)。そのあと、フラックスをたっぷり塗り、こて先に少量のはんだをのせて、1 辺ずつ、ピンの列に沿ってゆっくり引きます (引きはんだ)。はんだは、少なすぎるくらいから始めます。多いぶんは、ブリッジになります。4 辺とも終わったら、フラックスの残りを IPA でふき取ります。",
            checks: &["1 番ピンの向きを確かめてから、IC を固定した", "対角 2 か所で固定して、100 本全部がパッドの真ん中に乗っていることを、拡大鏡で確かめた", "4 辺とも、引きはんだをした", "フラックスの残りを IPA でふき取った"],
        },
        Task {
            title: "4. ブリッジを直す",
            detail: "拡大鏡で、足と足のあいだを、1 辺ずつ見ます。はんだがつながっている所 (ブリッジ) には、フラックスをつけ、吸い取り線をのせ、その上からこてを当てて、はんだを吸います。直らないときは、こて先にはんだを少し足して、いったん溶かし直すと、流れて離れることもあります。直したら、IPA でふき取って、もう一度見ます。",
            checks: &["全部の辺を、1 本ずつ見た", "ブリッジが見つかったところを、吸い取り線で直した", "ツヤのない (冷たい) はんだを、温め直した"],
        },
        Task {
            title: "5. 電源を入れる前の点検と、最初の起動",
            detail: "テスタの抵抗レンジで、5V と GND の間が、ショート (0Ω に近い値) していないことを確かめます。ATmega2560 の電源ピン (VCC は 10・31・61・80 番、AVCC は 100 番、GND は 11・32・62・81・99 番) と、AREF (98 番) が、5V と GND の間でショートしていないかを、見ます。電流を制限できる電源か USB で電源を入れ、IC やコンデンサが熱くないことを確かめます。USB でパソコンにつなぎ、デバイスマネージャーに、CH340 の COM ポートが出れば、USB とシリアル変換の部分は動いています。",
            checks: &["テスタで、5V と GND の間が、ショートしていない", "電源の 5 組のピンを、1 組ずつテスタで確かめた", "電源を入れて、部品が熱くなかった", "パソコンに CH340 の COM ポートが出た"],
        },
    ],
    trouble: &[
        ("隣のピンとつながった (ブリッジ)", "フラックスをたっぷり塗り、吸い取り線をはんだにのせて、その上からこてを当てて、はんだを吸います。0.5mm のピンは熱に弱いので、一度に長く温めず、少しずつ繰り返します。"),
        ("ピンが曲がった、パッドから浮いた", "フラックスをつけ、こてで温めながら、ピンセットの先で、そっと位置を直します。曲がったピンを無理に引くと、折れます。"),
        ("5V と GND がショートしている", "拡大鏡で電源ピンの付近と、コンデンサ (0603) のはんだを見ます。IC の下や、コンデンサの両端のブリッジがよくあります。電源を入れず、ショートが直るまで探します。"),
        ("COM ポートが出ない", "USB の D+ / D− のはんだ (CH340G の 5・6 番ピン、コネクタ) と、CH340G の 12MHz の水晶、V3 ピンのコンデンサ (0.1µF) を確かめます。USB ケーブルが、充電専用でないかも見ます。"),
        ("COM ポートは出るが、書き込めない", "ATmega2560 の XTAL1・XTAL2 (16MHz の水晶) と、RESET (DTR からの 100nF と 10kΩ) と、TXD0・RXD0 (交差) を確かめます。ブートローダが入っているかも確かめます。"),
    ],
    limits: "ここは、一般的な手順とコツです。部品・基板・道具によって、こて先の温度 (一般に 320〜350℃ くらい) や、はんだの量の加減は変わります。部品のデータシートの「はんだ付けの条件 (温度と時間)」と、はんだやフラックスの注意書きを、確かめてください。ピン番号は、ATmega2560 のデータシートと、基板の部品の向きで確かめます。",
};

pub const GUIDES: &[&Guide] = &[&SOLDER, &BOOTLOAD, &SOLDER_TQFP100];

pub fn guide(id: &str) -> Option<&'static Guide> {
    GUIDES.iter().copied().find(|g| g.id == id)
}

#[derive(Default)]
pub struct BenchGuide {
    /// The guide shown, if any.
    pub open: Option<&'static str>,
    /// Ticked boxes: (guide id, task, check).
    ticks: BTreeSet<(&'static str, usize, usize)>,
}

impl BenchGuide {
    pub fn show_guide(&mut self, id: &str) {
        self.open = guide(id).map(|g| g.id);
    }

    /// Draws the open guide. Returns the guide's id when the last box was just ticked (the caller records it).
    pub fn show(&mut self, ctx: &egui::Context, progress: &Progress) -> Option<&'static str> {
        let g = self.open.and_then(guide)?;
        let total: usize = g.tasks.iter().map(|t| t.checks.len()).sum();
        let mut open = true;
        let mut finished = None;
        let at = egui::pos2((ctx.content_rect().right() - 600.0).max(0.0), 40.0);
        egui::Window::new(g.title).open(&mut open).default_pos(at).default_width(580.0).default_height(700.0).vscroll(true).show(ctx, |ui| {
            ui.label(spaced(g.intro));
            ui.add_space(4.0);
            ui.colored_label(WARN, g.safety);
            let ticked = self.ticks.iter().filter(|(id, _, _)| *id == g.id).count();
            ui.add(egui::ProgressBar::new(ticked as f32 / total.max(1) as f32).text(format!("チェック {ticked} / {total}")));
            ui.separator();
            for (ti, task) in g.tasks.iter().enumerate() {
                ui.strong(task.title);
                ui.label(spaced(task.detail).color(NOTE));
                for (ci, check) in task.checks.iter().enumerate() {
                    let key = (g.id, ti, ci);
                    let mut on = self.ticks.contains(&key);
                    if ui.checkbox(&mut on, *check).on_hover_text("やり終えたら、チェックを入れます。全部入れると、学習パスのこの項目が完了になります").changed() {
                        if on {
                            self.ticks.insert(key);
                        } else {
                            self.ticks.remove(&key);
                        }
                    }
                }
                ui.add_space(6.0);
            }
            ui.separator();
            egui::CollapsingHeader::new("うまくいかないとき").show(ui, |ui| {
                for (what, look) in g.trouble {
                    ui.label(egui::RichText::new(*what).strong());
                    ui.indent(("bench_trouble", *what), |ui| {
                        ui.label(spaced(look).color(NOTE));
                    });
                }
            });
            ui.add_space(4.0);
            ui.label(spaced(g.limits).color(NOTE));
            if progress.is_done(g.id) || self.ticks.iter().filter(|(id, _, _)| *id == g.id).count() == total {
                ui.separator();
                ui.colored_label(OK, "できました! 学習パスのこの項目は完了です");
            }
        });
        let ticked = self.ticks.iter().filter(|(id, _, _)| *id == g.id).count();
        if ticked == total && !progress.is_done(g.id) {
            finished = Some(g.id);
        }
        if !open {
            self.open = None;
        }
        finished
    }

    #[cfg(test)]
    pub fn tick_all_for_test(&mut self, id: &str) {
        if let Some(g) = guide(id) {
            for (ti, t) in g.tasks.iter().enumerate() {
                for ci in 0..t.checks.len() {
                    self.ticks.insert((g.id, ti, ci));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws the open window once (a frame of egui) and returns what `show` answered.
    fn frame(ctx: &egui::Context, g: &mut BenchGuide, progress: &Progress) -> Option<&'static str> {
        let mut out = None;
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut frame = ctx.run_ui(input, |ui| {
            out = g.show(ui.ctx(), progress);
        });
        frame.textures_delta.clear();
        out
    }

    #[test]
    fn ticking_every_box_finishes_the_item_once() {
        let ctx = egui::Context::default();
        let mut progress = Progress::default();
        let mut g = BenchGuide::default();
        assert_eq!(frame(&ctx, &mut g, &progress), None, "nothing is open");
        g.show_guide("solder");
        assert_eq!(frame(&ctx, &mut g, &progress), None, "nothing is ticked yet");
        g.tick_all_for_test("bootload");
        assert_eq!(frame(&ctx, &mut g, &progress), None, "the ticks of another guide do not count");
        g.tick_all_for_test("solder");
        assert_eq!(frame(&ctx, &mut g, &progress), Some("solder"));
        progress.mark("solder");
        assert_eq!(frame(&ctx, &mut g, &progress), None, "recorded once");
    }

    #[test]
    fn every_guide_has_safety_tasks_checks_and_a_way_out() {
        let mut seen = std::collections::HashSet::new();
        for g in GUIDES {
            assert!(seen.insert(g.id), "duplicate id {}", g.id);
            assert!(g.safety.starts_with("安全") && g.intro.chars().count() > 40, "{}", g.id);
            assert!(g.tasks.len() >= 3 && g.trouble.len() >= 3 && !g.limits.is_empty(), "{}", g.id);
            for t in g.tasks {
                assert!(!t.checks.is_empty() && t.detail.chars().count() > 40, "{}: {}", g.id, t.title);
            }
        }
    }
}
