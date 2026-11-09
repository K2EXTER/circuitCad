//! Learning paths: an ordered route through the lessons toward a goal (the first one: build a
//! regulated power supply yourself), and the record of which lessons the learner has finished.
//! The record is a small JSON file next to the part library, so it survives restarts.

use eframe::egui::{self, Color32};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod paths;
mod window;
#[cfg(test)]
mod tests;

pub use paths::PATHS;
pub use window::{welcome_card, LearnWindow};

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
    // an item that several paths share has the prerequisites of each path: look in this path first, then in the others
    ids.into_iter().filter_map(|id| items(path).find(|i| i.id == id).or_else(|| find_item(id))).collect()
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


/// A feature to try once the lessons are done: what it is for, and where it is in the program.
pub struct NextStep {
    pub title: &'static str,
    pub about: &'static str,
    pub where_: &'static str,
}

/// "この先へ": the features that go beyond the lessons, for the learner who stays (shown folded at the bottom of the
/// learning-path window, open once every lesson of the path is done). `where_` names the button or menu as it reads on
/// the screen. When a button is renamed, change it here too.
pub const NEXT_STEPS: &[NextStep] = &[
    NextStep { title: "配線の見た目と作りやすさを整える", about: "配線の角を丸め、太さが変わる所をなめらかにし、2本が合流する所のすき間を銅で埋めます。製造のとき、エッチングで細いすき間がつぶれたり、液がたまったりするのを防ぎます", where_: "基板エディタの「仕上げ (任意)」" },
    NextStep { title: "配線の太さを、流れる電流から決める", about: "回路の電流から、配線に必要な太さを求め、電圧降下 (配線の抵抗で電圧が下がる量) も確かめます", where_: "基板エディタの「③ 確認して発注」の「電流と電圧降下」" },
    NextStep { title: "差動ペアと長さ合わせ", about: "USB のように2本で信号を送る線を並べて引き、長さをそろえます。高い周波数の信号が乱れにくくなります", where_: "同じ「高度な配線 (中級者向け)」の「差動ペア」「長さ合わせ (蛇行)」" },
    NextStep { title: "発注と、買うものの一覧", about: "ガーバー・ドリル・部品表・配置ファイルと一緒に、部品ごとの数量と、秋月電子・DigiKey・LCSC のリンク (型番が分かる部品は商品ページ) を書き出します", where_: "基板エディタの「③ 確認して発注」の「発注の準備…」" },
    NextStep { title: "部品の故障を試す", about: "抵抗が焼ける、コンデンサがショートするなど、部品が壊れたときに回路がどうなるか、ほかの部品が連鎖して壊れないかを調べます", where_: "回路図の左パネルの「故障を試す」「連鎖故障を追う」" },
    NextStep { title: "部品の誤差と温度の影響", about: "抵抗やコンデンサの誤差、温度の変化で、出力がどれだけ動くかを、たくさんの組み合わせで調べます", where_: "回路図の左パネルの「ばらつき解析を実行」「温度スイープを実行」" },
    NextStep { title: "ほかのソフトと行き来する", about: "回路を SPICE のネットリストで書き出して LTspice や ngspice で動かしたり、KiCad の部品を取り込んだり、自分で部品を作って足したりできます", where_: "回路図の左パネルの「SPICEネットリストを書き出し…」「＋ 部品を作る…」(開いた窓の「① 基本」に、KiCad・JSON から取り込むボタンがあります)" },
    NextStep { title: "画面なしで動かす (ccad)", about: "1行1コマンドのスクリプトで、回路づくりから解析・基板・製造データまでを自動で実行します。AI エージェントから使うこともできます", where_: "ソフトに付いてくる `ccad`。使い方は docs/cli.md" },
];

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
    egui::RichText::new(i18n::tr_dyn(text)).line_height(Some(20.0))
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

/// The path a new learner starts on: the parts path, whose first lessons explain voltage, current and how to read a drawn circuit.
pub const FIRST_PATH_ID: &str = "parts";

/// The path of `FIRST_PATH_ID` (the first of `PATHS` if it were renamed).
pub fn first_path() -> &'static LearningPath {
    PATHS.iter().find(|p| p.id == FIRST_PATH_ID).unwrap_or(&PATHS[0])
}
