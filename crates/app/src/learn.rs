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
    Board,
    /// Only a "作ってみよう" challenge (by the item's id); done when the challenge is passed.
    Build,
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
    Item { id, title, about, kind: Kind::Lesson(id), build: false, real }
}

const fn planned(id: &'static str, title: &'static str, about: &'static str, real: Option<&'static str>) -> Item {
    Item { id, title, about, kind: Kind::Planned, build: false, real }
}

pub const PATHS: &[LearningPath] = &[LearningPath {
    id: "power_supply",
    title: "安定化電源を作ろう",
    goal: "ACアダプタや電池の電圧を、負荷が変わっても一定の 5V にする電源を、原理から理解して、回路図・基板・実機まで自分で作ります。実験は、安全のため 12V 以下の低い電圧だけで行います",
    stages: &[
        Stage {
            name: "1. 入門: 電圧・電流と測り方",
            about: "電圧を分ける、電流を制限する、電池が負荷で電圧を下げる。安定化電源が「何を一定にするのか」の土台です",
            items: &[
                Item { build: true, ..lesson("divider", "電圧を分ける (分圧回路)", "抵抗2本で好きな電圧を作る。出力が負荷で変わる理由の入口", Some("テスタで電圧を測る。抵抗2本をブレッドボードにつなぎ、計算値と測定値を比べます")) },
                Item { build: true, ..lesson("led", "LED と電流制限抵抗", "電流を決めるのは抵抗。電流を測る方法も", Some("LED の電流をテスタの電流レンジで測り、抵抗を変えて明るさと電流を比べます")) },
                lesson("coin_cell", "電池の内部抵抗 (負荷で電圧が下がる)", "電源の電圧が、電流を流すと下がる。これを小さくするのが安定化電源の役目", Some("電池に抵抗をつないで、つなぐ前後の端子電圧を測り、内部抵抗を求めます")),
            ],
        },
        Stage {
            name: "2. 基礎: 電源をつくる部品",
            about: "ダイオード・コンデンサ・ツェナー・トランジスタ。電圧を整え、ならし、一定にし、電流を増やす部品の働きです",
            items: &[
                lesson("diode_curve", "ダイオードの特性", "約 0.7V (ショットキーは約 0.3V) で流れ始め、そのあとは電圧がほとんど増えない", Some("ダイオードの順方向電圧を、シリコンとショットキーで測り、温めて変化を見ます")),
                lesson("rectifier", "交流を直流にする (整流・平滑・リップル)", "整流・平滑コンデンサ・リップルの大きさ。シミュレーションは商用電圧 (100V) を使いますが、実機では使いません", Some("実機は低電圧の交流 (小さなトランスの出力など) だけで行い、オシロ (またはテスタの交流レンジ) でリップルを測って、容量を変えて比べます。商用電源 (100V) の配線は、危険なので行いません")),
                Item { build: true, ..lesson("zener", "ツェナーで電圧を一定にする", "もっとも簡単な安定化。入力が変わっても動きにくいが、負荷が重いと崩れる。自分で作るチャレンジつき", Some("ツェナーと抵抗で 5.1V を作り、負荷と入力を変えて電圧の変化を測ります")) },
                lesson("bjt_switch", "トランジスタのスイッチ", "ベース電流でコレクタ電流を制御する。電流を増やす部品の入口", Some("トランジスタで LED をオン・オフし、ベース電流とコレクタ電流を測ります")),
                Item { build: true, ..lesson("emitter_follower", "エミッタフォロワ (電流を増やす)", "電圧はそのまま、流せる電流を増やす。直列パス型電源の中核。大電流では発熱する", Some("ツェナーの電圧を、トランジスタで大電流にし、負荷を変えて出力と温度を測ります")) },
            ],
        },
        Stage {
            name: "3. 応用: 安定化のしくみ",
            about: "出力の電圧を測って、ずれたら直す (帰還)。これが安定化電源の本質です",
            items: &[
                lesson("opamp_noninv", "オペアンプで信号を大きくする (非反転増幅)", "帰還の考え方。抵抗の比がゲインを決める", Some("オペアンプの増幅回路を作り、入力と出力をテスタやオシロで測ります")),
                Item { build: true, ..lesson("opamp_inv", "オペアンプの反転増幅 (仮想接地)", "反転入力が 0V に保たれる仕組み", None) },
                Item { build: true, ..lesson("series_regulator", "基準電圧と帰還で安定化する (直列レギュレータ)", "オペアンプ + 基準電圧 + パストランジスタで、自作の安定化電源を組む", Some("負荷を変えたとき (ロードレギュレーション)、入力を変えたとき (ラインレギュレーション) の出力電圧を測ります")) },
                Item { build: true, ..lesson("regulator_78xx", "三端子レギュレータ (78xx / LDO) を使う", "入出力のコンデンサ、ドロップアウト電圧、使い方と選び方", Some("7805 で 5V を作り、入力と負荷を変えて出力と温度を測ります")) },
                lesson("heat", "熱と定格を考える", "電圧差 × 電流 = 熱。放熱板が必要になる理由。定格チェックの見方", Some("負荷を増やしながらレギュレータの温度を測り (触って確かめる前に、温度計で)、計算と比べます")),
                lesson("protection", "電源を守る (逆接続・過電流・ヒューズ)", "壊れ方を知って、守る部品を足す", Some("ヒューズや逆接続保護ダイオードの働きを、安全な条件 (低電圧・電流制限つき) で確かめます")),
                lesson("coil_layer_short", "コイルのレアショート (連鎖して壊れる)", "1つの故障が次の部品の故障を呼ぶ連鎖。ヒューズで止められる場合と、止められない場合", Some("リレーのコイル抵抗をテスタで測り、定格電流の内側か確かめます。コイルを実際にショートさせる実験はしません")),
                lesson("variable_supply", "ダイヤルで電圧を変える電源 (トランス + LM317)", "100V → トランス → 整流 → 可変レギュレータ。ダイヤル(ボリューム)で出力が変わる式と、出せない電圧の理由", Some("低電圧の電源 (15〜18V の AC アダプタ) で LM317 を組み、ボリュームを回して出力を測ります。100V は実験に使いません")),
                lesson("buck","降圧コンバータ (スイッチング電源)", "発展: 熱にしない電源。リニアとの効率の違い", None),
            ],
        },
        Stage {
            name: "4. 製作: 回路図から基板、実機まで",
            about: "設計した電源を、基板にして、組み立てて、性能を測ります",
            items: &[
                Item { id: "draw_psu", title: "安定化電源の回路図を自分で設計する (最終課題)", about: "目標 (入力 9〜12V → 出力 5V、逆接続でも壊れない) だけ示し、自分で部品を選んで置き、配線する", kind: Kind::Build, build: true, real: None },
                Item { id: "board", title: "回路図から基板をつくる", about: "取り込み・配置・配線・DRC・発注の準備", kind: Kind::Board, build: false, real: Some("発注するなら、少量・少額で試します") },
                planned("build_psu", "組み立てて、性能を測る", "部品表・はんだ付け・負荷試験・リップル・温度。測定値をシミュレーションと比べる", Some("完成した電源の性能を測り、シミュレーションとの差とその理由を考えます")),
            ],
        },
    ],
}];

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

/// The next item to do: the first available one that is not finished.
pub fn next_item(path: &LearningPath, progress: &Progress) -> Option<&'static Item> {
    items(path).find(|i| i.kind != Kind::Planned && !item_done(progress, i))
}

// ------------------------------------------------------------------ the window

/// What the learner asked for in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Lesson(&'static str),
    /// The "作ってみよう" challenge of a lesson (by the lesson's id).
    Build(&'static str),
    Board,
}

/// Colour of the grey explanation lines (`weak()` was too faint on the dark background).
const NOTE: Color32 = Color32::from_gray(175);

/// Wrapped Japanese text with a taller line (the font's default line height makes lines touch).
fn spaced(text: &str) -> egui::RichText {
    egui::RichText::new(text).line_height(Some(20.0))
}

/// The action that opens an item, if it can be opened yet.
fn item_action(item: &Item) -> Option<Action> {
    match item.kind {
        Kind::Lesson(id) => Some(Action::Lesson(id)),
        Kind::Build => Some(Action::Build(item.id)),
        Kind::Board => Some(Action::Board),
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
        ui.label(spaced("自由に描きたいときは、このカードを閉じて、左の一覧から部品を選んで画面をクリックします。").color(NOTE));
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
}

impl LearnWindow {
    pub fn show(&mut self, ctx: &egui::Context, progress: &mut Progress) -> Option<Action> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut action = None;
        let path = &PATHS[0];
        egui::Window::new("学習パス").open(&mut open).default_width(600.0).default_height(720.0).vscroll(true).show(ctx, |ui| {
            ui.heading(path.title);
            ui.label(spaced(path.goal));
            let (finished, available, all) = summary(path, progress);
            ui.add(egui::ProgressBar::new(finished as f32 / available.max(1) as f32).text(format!("完了 {finished} / いま学べる {available} (全体 {all})")));
            if let Some(next) = next_item(path, progress) {
                ui.horizontal_wrapped(|ui| {
                    ui.label("次のおすすめ:");
                    ui.colored_label(Color32::from_rgb(220, 150, 40), next.title);
                });
            } else {
                ui.colored_label(Color32::from_rgb(60, 160, 90), "いま学べるレッスンは、すべて完了しました。残りは準備中です");
            }
            let next_id = next_item(path, progress).map(|i| i.id);
            for stage in path.stages {
                ui.add_space(8.0);
                ui.strong(stage.name);
                ui.label(spaced(stage.about).color(NOTE));
                for item in stage.items {
                    let done = item_done(progress, item);
                    let (mark, color) = match item.kind {
                        Kind::Planned => ("…", Color32::from_gray(120)),
                        _ if done => ("✔", Color32::from_rgb(60, 160, 90)),
                        _ if Some(item.id) == next_id => ("▶", Color32::from_rgb(220, 150, 40)),
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
                                _ => Action::Board,
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
        if !open {
            self.open = false;
        }
        action
    }
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

    #[test]
    fn the_power_supply_path_walks_from_basics_to_building() {
        let p = &PATHS[0];
        assert_eq!(p.stages.len(), 4);
        let mut progress = Progress::default();
        let (done, available, all) = summary(p, &progress);
        assert_eq!(done, 0);
        assert!(available >= 12 && all > available, "some steps are still to be written");
        // the first thing to do is the first lesson; finishing it moves the recommendation on
        assert_eq!(next_item(p, &progress).unwrap().id, "divider");
        progress.mark("divider");
        assert_eq!(next_item(p, &progress).unwrap().id, "led");
        // planned items are never recommended, however far the learner is
        for it in items(p).filter(|i| i.kind != Kind::Planned) {
            let key = if it.kind == Kind::Build { crate::schematic::challenge_progress_key(it.id) } else { it.id.to_string() };
            progress.mark(&key);
        }
        assert!(next_item(p, &progress).is_none());
    }
}
