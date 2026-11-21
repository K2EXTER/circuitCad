//! The stripboard and perfboard views of the proto tab (B5): the layout of `sch_core::stripboard` drawn from the parts' side — the
//! copper strips with their cuts (stripboard), or the bare wires that join the holes (perfboard) — with the parts, the wire links and
//! the supplies off the board; and the list to build it from (the parts' holes, the cuts, the links).

use super::*;
use sch_core::mount::{shopping, Mount};
use sch_core::perfboard::{links_along_pins, lock_first, perf_layout_around, perf_layout_with, LockWhy};
use sch_core::strip_print::{perf_wires, strip_svg, Face};
use sch_core::strip_edit::{area_moved, area_turned, turned_box, cut_toggled, link_end_moved, part_moved, part_turned, synced, with_bare, with_link, without_bare_at, without_link};
use sch_core::stripboard::{hole_name, net_at, strip_check, strip_layout_with, strip_rules, strip_unjoined, At, StripLayout, StripOptions};

/// What the person builds on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoardKind {
    #[default]
    Breadboard,
    /// Copper strips along the rows, cut where a strip must stop.
    Stripboard,
    /// Plain holes: the strips' work is done by bare wires along the rows.
    Perfboard,
}

/// What a press on the stripboard / perfboard does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StripTool {
    /// Drag a part, a link's end, or a box around a block and then the block; R turns the picked part, Delete takes the picked link away.
    #[default]
    Move,
    /// Click a hole, then another: a wire link between them.
    Link,
    /// Click a hole: the strip is cut there, or joined again (on a perfboard: the automatic bare wire parts there).
    Cut,
    /// Click a hole, then another on its row or column: a bare wire drawn on the copper side (perfboard only); a right click on one
    /// takes it away.
    Bare,
}

/// What is being dragged.
#[derive(Clone, Debug)]
enum StripDrag {
    Part { refdes: String, from: At },
    LinkEnd { k: usize, end: usize },
    /// The block in the box, from the hole the press was on.
    Block { from: At },
    /// A box being drawn from this hole.
    Select { from: At },
    /// A section's box (perfboard) by its name, from the hole the press was on to the one the pointer is on: let go, the box stays
    /// there and the others are packed round it (`perf_layout_around`).
    Section { k: usize, from: At, now: At },
}

/// The stripboard / perfboard view's own state.
#[derive(Default)]
pub struct StripState {
    pub tool: StripTool,
    /// The sections proposed ("区画の案を出す"), shown until kept or let go.
    plan: Option<Vec<sch_core::section_plan::SectionPlan>>,
    /// The layout made from the schematic, by the schematic's edit count, part count and the options it was made with.
    auto: Option<(u64, usize, StripOptions, bool, StripLayout)>,
    /// The saved layout kept in step with the schematic: the key, the saved layout it came from, and the result.
    synced: Option<(u64, usize, StripLayout, StripLayout)>,
    /// A layout being made on another thread (a perfboard's tries take up to 1.5 s: the window does not stop meanwhile).
    job: Option<StripJob>,
    /// "固定した部品のまわりを並べ直す" on another thread: the layout round the locked parts comes back here.
    around: Option<std::sync::mpsc::Receiver<StripLayout>>,
    /// A section's box let go where it was dragged, drawn there until the layout round it comes back.
    put_box: Option<(At, At)>,
    drag: Option<StripDrag>,
    /// The size and place of the board when the drag began: it stays put while the board grows under the pointer.
    frozen: Option<(Pos2, f32)>,
    /// The box picked (two corner holes).
    sel: Option<(At, At)>,
    link_start: Option<At>,
    picked_link: Option<usize>,
    /// Where hole (0, 0) was drawn and the pitch, last frame (for the tests' pointer).
    pub(super) drawn: Option<(Pos2, f32)>,
    /// The layouts before each hand edit (None: the automatic one), the latest last, and those undone (to redo).
    undo: Vec<Option<StripLayout>>,
    redo: Vec<Option<StripLayout>>,
    /// The layout the last edit, undo or redo left: when the sheet's differs (another file opened), the history is dropped.
    last: Option<Option<StripLayout>>,
    /// The board seen from below (the copper side, left and right swapped: where to solder), only to look at.
    pub back: bool,
    /// The net of the hole under the pointer on the copper side, by the schematic's edit count and the hole.
    back_net: Option<(u64, At, Option<String>)>,
    /// The checks of the layout shown, by the schematic's edit count and the layout (`strip_checks`).
    checks: Option<(u64, StripLayout, StripChecks)>,
}

/// The checks of a layout against the schematic: worked out again only when either changes (on a big perfboard they took about
/// 6 ms a frame in a debug build: half the frame).
#[derive(Clone, Default)]
pub(super) struct StripChecks {
    pub problems: Vec<String>,
    pub rules: Vec<sch_core::stripboard::RuleCheck>,
    pub unjoined: Vec<(String, At, At)>,
}

/// What a layout on another thread is for (the schematic's edit count and part count, the options, perfboard or not, and the saved
/// layout it keeps in step: None for the automatic one), and where it comes back.
struct StripJob {
    key: (u64, usize, StripOptions, bool),
    saved: Option<StripLayout>,
    rx: std::sync::mpsc::Receiver<StripLayout>,
}

const COPPER: Color32 =Color32::from_rgb(196, 128, 72);
/// The picked part, link and box.
const PICK: Color32 = Color32::from_rgb(70, 160, 255);
pub(super) const BOARD: Color32 = Color32::from_rgb(214, 190, 140);
const WIRE: Color32 = Color32::from_rgb(190, 196, 204);
/// An adapter board's green, and a socket's frame.
const ADAPTER: Color32 = Color32::from_rgb(40, 120, 70);
const SOCKET: Color32 = Color32::from_gray(85);
/// A module's board (an Arduino Nano) in its female headers.
const MODULE: Color32 = Color32::from_rgb(30, 85, 150);
/// A locked part's padlock (perfboard).
const LOCK: Color32 = Color32::from_rgb(235, 190, 60);
/// The dashed frame round a part worth placing by hand first (darker than the padlock: it is drawn on the board's tan).
const HINT: Color32 = Color32::from_rgb(190, 110, 0);

/// A colour for a net's links, the supplies red and ground black.
fn net_colour(net: &str) -> Color32 {
    let u = net.to_ascii_uppercase();
    if matches!(u.as_str(), "GND" | "0" | "VSS") {
        return Color32::from_gray(30);
    }
    if u.starts_with("VCC") || u.starts_with("VDD") || u.starts_with('+') || u.starts_with("VIN") || u.contains('V') && u.starts_with(|c: char| c.is_ascii_digit()) {
        return Color32::from_rgb(210, 40, 40);
    }
    const PALETTE: [Color32; 5] = [Color32::from_rgb(40, 110, 220), Color32::from_rgb(30, 150, 70), Color32::from_rgb(230, 140, 20), Color32::from_rgb(140, 60, 190), Color32::from_rgb(20, 160, 170)];
    PALETTE[net.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % PALETTE.len()]
}

/// How many hand edits are kept to undo.
const UNDO_STEPS: usize = 100;

impl BreadboardView {
    /// A hand edit (or "自動で並べ直す"): the layout before it goes on the undo list.
    fn strip_set(&mut self, sch: &mut SchEditor, m: Option<StripLayout>) {
        if sch.stripboard == m {
            return;
        }
        self.strip_history_check(sch);
        self.strip.undo.push(std::mem::replace(&mut sch.stripboard, m));
        if self.strip.undo.len() > UNDO_STEPS {
            self.strip.undo.remove(0);
        }
        self.strip.redo.clear();
        self.strip.last = Some(sch.stripboard.clone());
    }

    /// Drops the history when the sheet's layout was changed elsewhere (another file opened, the schematic's undo).
    fn strip_history_check(&mut self, sch: &SchEditor) {
        if self.strip.last.as_ref().is_some_and(|l| *l != sch.stripboard) {
            self.strip.undo.clear();
            self.strip.redo.clear();
            self.strip.last = None;
        }
    }

    /// Undo (`back`) or redo the last hand edit of the stripboard / perfboard.
    pub(super) fn strip_undo(&mut self, sch: &mut SchEditor, back: bool) {
        self.strip_history_check(sch);
        let (from, to) = if back { (&mut self.strip.undo, &mut self.strip.redo) } else { (&mut self.strip.redo, &mut self.strip.undo) };
        let Some(m) = from.pop() else { return };
        to.push(std::mem::replace(&mut sch.stripboard, m));
        self.strip.last = Some(sch.stripboard.clone());
        self.strip.sel = None;
        self.strip.picked_link = None;
        self.strip.drag = None;
    }

    /// The layout shown: the one saved with the sheet (in step with the schematic), else one made from the schematic.
    /// The layout is made on another thread: while it is, the one shown is the saved layout itself (without the parts added to the
    /// schematic since), or the last automatic one for this kind of board (the old schematic's: not to be edited), or none.
    pub(super) fn strip_shown(&mut self, sch: &SchEditor) -> StripLayout {
        let key = (sch.rev, sch.sch.components.len());
        let opt = strip_options(sch.strip_size);
        // a perfboard has a layout of its own (no strips); a layout saved for the other kind of board is not shown on this one
        let perf = self.kind == BoardKind::Perfboard;
        let want = (key.0, key.1, opt, perf);
        let saved = sch.stripboard.as_ref().filter(|s| s.perf == perf).cloned();
        let ready = match &saved {
            Some(s) => self.strip.synced.as_ref().is_some_and(|(r, n, from, _)| (*r, *n) == key && from == s),
            None => self.strip.auto.as_ref().is_some_and(|(r, n, o, p, _)| (*r, *n, *o, *p) == want),
        };
        if ready {
            self.strip.job = None;
        } else {
            let fresh = !self.strip.job.as_ref().is_some_and(|j| j.key == want && j.saved == saved);
            if fresh {
                let (tx, rx) = std::sync::mpsc::channel();
                let (s, from) = (sch.sch.clone(), saved.clone());
                std::thread::spawn(move || {
                    let l = match &from {
                        Some(f) => synced(&s, f, opt),
                        None if perf => perf_layout_with(&s, opt),
                        None => strip_layout_with(&s, opt),
                    };
                    let _ = tx.send(l);
                });
                self.strip.job = Some(StripJob { key: want, saved: saved.clone(), rx });
            }
            // a quick layout (most stripboards) is waited for a moment, not shown a frame late; the tests always wait
            use std::sync::mpsc::RecvTimeoutError;
            let wait = if cfg!(test) { std::time::Duration::from_secs(60) } else if fresh { std::time::Duration::from_millis(40) } else { std::time::Duration::ZERO };
            let got = match self.strip.job.as_ref().map(|j| j.rx.recv_timeout(wait)) {
                Some(Ok(l)) => Some(l),
                // the thread failed: the saved layout as it is (or none), not a wait for ever
                Some(Err(RecvTimeoutError::Disconnected)) => Some(saved.clone().unwrap_or_default()),
                _ => None,
            };
            if let Some(l) = got {
                self.strip.job = None;
                match &saved {
                    Some(from) => self.strip.synced = Some((key.0, key.1, from.clone(), l)),
                    None => self.strip.auto = Some((key.0, key.1, opt, perf, l)),
                }
            }
        }
        match &saved {
            Some(s) => self.strip.synced.as_ref().filter(|c| &c.2 == s).map(|c| c.3.clone()).unwrap_or_else(|| s.clone()),
            None => self.strip.auto.as_ref().filter(|c| c.3 == perf).map(|c| c.4.clone()).unwrap_or_default(),
        }
    }

    /// Whether a layout is being made on another thread, and whether the one shown meanwhile is an old automatic one (not to be edited).
    pub(crate) fn strip_busy(&self) -> (bool, bool) {
        match &self.strip.job {
            Some(j) => (true, j.saved.is_none()),
            None => (self.strip.around.is_some(), false),
        }
    }

    /// The layout round the locked parts, once made on its thread: kept with the sheet (one step to undo).
    fn strip_around_done(&mut self, sch: &mut SchEditor) {
        use std::sync::mpsc::TryRecvError;
        let Some(rx) = &self.strip.around else { return };
        let got = if cfg!(test) { rx.recv().map_err(|_| TryRecvError::Disconnected) } else { rx.try_recv() };
        match got {
            Ok(m) => {
                self.strip.around = None;
                self.strip.put_box = None;
                self.strip.sel = None;
                self.strip.picked_link = None;
                self.strip_set(sch, Some(m));
                sch.status = i18n::tr("固定した部品・置いた区画の箱はそのままで、ほかの部品を並べ直し、線を引き直しました (元に戻すで戻せます)").to_string();
            }
            Err(TryRecvError::Disconnected) => {
                self.strip.around = None;
                self.strip.put_box = None;
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    /// "固定した部品のまわりを並べ直す": the layout round `l`'s locked parts, made on another thread (`strip_around_done` keeps it).
    pub(super) fn strip_relayout(&mut self, sch: &SchEditor, l: &StripLayout) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (s, from, opt) = (sch.sch.clone(), l.clone(), strip_options(sch.strip_size));
        std::thread::spawn(move || {
            let _ = tx.send(perf_layout_around(&s, opt, &from));
        });
        self.strip.around = Some(rx);
    }

    /// A section's box let go at `now` (dragged from `from`): it stays there (`StripLayout::pinned`), its parts are placed afresh
    /// inside and the other boxes are packed round it, on another thread. Not with parts locked (the boxes are not packed round them).
    fn strip_put_box(&mut self, sch: &mut SchEditor, l: &StripLayout, k: usize, from: At, now: At) {
        if !l.locked.is_empty() {
            sch.status = i18n::tr("固定した部品があるときは、区画の箱は置けません (部品の固定を外してから置きます)").to_string();
            return;
        }
        if self.strip.around.is_some() {
            return;
        }
        let mut m = l.clone();
        let (name, a, b) = m.sections[k].clone();
        let (a, b) = box_moved(a, b, from, now);
        m.sections[k] = (name.clone(), a, b);
        m.pinned.insert(name.clone());
        self.strip.put_box = Some((a, b));
        sch.status = i18n::trf!("区画「{}」の箱を {} に置いて、ほかを並べ直しています…", name, hole_name(a));
        self.strip_relayout(sch, &m);
    }

    /// The sections proposed for the schematic, shown in the panel.
    pub(crate) fn strip_propose(&mut self, sch: &SchEditor) {
        self.strip.plan = Some(sch_core::section_plan::propose_sections(&sch.sch));
    }

    /// "区画の案を出す": the parts cut into groups with few nets between them (`sch_core::section_plan`), listed; "この案で区画にする"
    /// puts them in the requirements table as sections (the layout then follows them).
    fn strip_plan_ui(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        if ui
            .button(i18n::tr("区画の案を出す"))
            .on_hover_text(i18n::tr("部品を役割で分けた案を出します: デジタル (信号の半分以上が SPI・I²C・D22 のようなデジタルの線の IC とヘッダ)・リレー (K1 …)・アナログ・電源 (レギュレータ)。抵抗やコンデンサは、いちばんつながっている組へ、電源だけにつながるものは回路図で近くに描いた IC の組へ入れます。役割が 1 つしか無い回路は、間をつなぐネットが少ない所で分けます (電源と GND のネットは数えません)。ネットの名前で役割を決めるので、案を見てから区画にします"))
            .clicked()
        {
            self.strip_propose(sch);
        }
        let Some(plan) = self.strip.plan.clone() else { return };
        if plan.is_empty() {
            ui.small(egui::RichText::new(i18n::tr("分けられる所が見つかりませんでした (部品が少ないか、部品どうしのつながりが多い回路です)")).color(NOTE));
            return;
        }
        egui::Frame::group(ui.style()).show(ui, |ui| {
            for g in &plan {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(&g.name).strong());
                    ui.small(g.parts.join(", "));
                });
            }
            ui.horizontal(|ui| {
                if ui.button(i18n::tr("この案で区画にする")).on_hover_text(i18n::tr("この組を、設計の要件の「近くに置く組」に「区画」の印を付けて入れます。あとで設計の要件の窓で直したり消したりできます")).clicked() {
                    for g in &plan {
                        sch.sch.requirements.groups.push(design_spec::PlaceGroup { name: g.name.clone(), parts: g.parts.clone(), section: true, note: i18n::tr("区画の案から").to_string(), ..Default::default() });
                    }
                    self.strip.plan = None;
                    sch.status = i18n::trf!("{} 個の区画を設計の要件に入れました。並べ方は区画ごとに作り直されます", plan.len());
                }
                if ui.button(i18n::tr("やめる")).clicked() {
                    self.strip.plan = None;
                }
            });
        });
    }

    /// The picked part locked where it is, or let go (perfboard): a hand edit.
    fn strip_lock_toggle(&mut self, sch: &mut SchEditor, l: &mut StripLayout, refdes: &str) {
        let mut m = l.clone();
        if !m.locked.remove(refdes) {
            m.locked.insert(refdes.to_string());
        }
        self.strip_set(sch, Some(m.clone()));
        *l = m;
    }

    /// "まず手で置くとよい部品": the connectors, knobs, relays and big chips not locked yet (`lock_first`), each a button that picks
    /// it (then drag it, L locks it); the reason on hover. Drawn on the board with a dashed frame.
    fn strip_lock_hints_ui(&mut self, ui: &mut egui::Ui, l: &StripLayout) {
        let hints = lock_first(l);
        if hints.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.small(egui::RichText::new(i18n::tr("まず手で置くとよい部品:")).color(LOCK)).on_hover_text(i18n::tr(
                "全体の形を決める部品です。この部品を自分で置いて固定 (L) し、「固定した部品のまわりを並べ直す」で残りを自動に任せると、全部自動より表の線が少なくなることが多いです (DMM の B 段: 81 本 → 72〜75 本)。ほかの部品は近くに寄せて線をまっすぐ引く自動の方が得意です",
            ));
            for (r, why) in &hints {
                let tip = match why {
                    LockWhy::Connector => "コネクタ・ヘッダ・モジュール: 基板の縁や、ケースの穴に合う所に置きます",
                    LockWhy::Control => "つまみ・スイッチ: ケースの穴に合う所に置きます",
                    LockWhy::Relay => "リレー: 大きくて重い部品です。並べてまとめ、小さい信号の部品から離します",
                    LockWhy::BigChip => "足の多い IC: まわりの部品の置き場所が、この IC の場所で決まります",
                };
                if ui.small_button(r).on_hover_text(i18n::tr(tip)).clicked() {
                    self.picked = Some(r.clone());
                    self.strip.picked_link = None;
                }
            }
        });
    }

    /// The board's size: a few common ones, or typed; and whether the layout fits it.
    fn strip_size_ui(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, l: &StripLayout) {
        const SIZES: [(f32, f32); 5] = [(72.0, 47.0), (95.0, 72.0), (115.0, 95.0), (150.0, 100.0), (160.0, 100.0)];
        let label = |s: Option<(f32, f32)>| match s {
            None => i18n::tr("決めない").to_string(),
            Some((w, h)) => format!("{w:.0} × {h:.0} mm"),
        };
        ui.horizontal(|ui| {
            ui.label(i18n::tr("基板の大きさ")).on_hover_text(i18n::tr(
                "使う基板の横 × 縦 (mm) を選ぶと、IC を横の幅に収まるだけ並べ、はみ出す分は下の段に折り返します。決めないときは IC を横一列に並べます。横長の基板は横に長く置いた向きで選びます",
            ));
            let before = sch.strip_size;
            egui::ComboBox::from_id_salt("strip_size").selected_text(label(sch.strip_size)).show_ui(ui, |ui| {
                ui.selectable_value(&mut sch.strip_size, None, label(None));
                for s in SIZES {
                    ui.selectable_value(&mut sch.strip_size, Some(s), label(Some(s)));
                }
            });
            if let Some((w, h)) = &mut sch.strip_size {
                ui.add(egui::DragValue::new(w).range(20.0..=400.0).suffix(" mm")).on_hover_text(i18n::tr("横 (mm)"));
                ui.label("×");
                ui.add(egui::DragValue::new(h).range(20.0..=400.0).suffix(" mm")).on_hover_text(i18n::tr("縦 (mm)"));
            }
            if sch.strip_size != before && sch.stripboard.is_some() {
                sch.status = i18n::tr("手で動かした並べ方はそのままです。新しい大きさで並べるには「自動で並べ直す」を押します").to_string();
            }
        });
        if let Some((w, h)) = sch.strip_size {
            let (cols, rows) = holes_of(w, h);
            let (over_c, over_r) = (l.cols.saturating_sub(cols), l.rows.saturating_sub(rows));
            if over_c == 0 && over_r == 0 {
                ui.colored_label(crate::theme::OK, i18n::trf!("✔ {} × {} 穴の基板に収まります", cols, rows));
            } else {
                ui.colored_label(
                    crate::theme::WARN,
                    i18n::trf!("{} × {} 穴の基板に収まりません: 横が {} 穴・縦が {} 穴 ({:.0} × {:.0} mm) はみ出します", cols, rows, over_c, over_r, over_c as f32 * 2.54, over_r as f32 * 2.54),
                );
            }
        }
    }

    /// The tools and the automatic layout's button.
    fn strip_tools(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, l: &StripLayout) {
        let strip = self.kind == BoardKind::Stripboard;
        if (self.strip.tool == StripTool::Bare && strip) || (self.strip.tool == StripTool::Cut && !strip) {
            self.strip.tool = StripTool::Move;
        }
        ui.horizontal(|ui| {
            for (back, name, tip) in [
                (false, "表 (部品の面)", "部品を差す面から見た絵です。部品と線を動かして並べ方を直せます (F で裏と切り替え)"),
                (true, "裏 (はんだの面)", "基板を裏返して見た絵です (左右が逆)。はんだ付けする穴・すずめっき線・銅の帯と切る穴を、実物を裏返したときと同じ向きで見ます。穴にカーソルを置くと、そこにつながる穴と線が光ります。見るだけで、直すときは表に戻します (F で切り替え)"),
            ] {
                if ui.selectable_label(self.strip.back == back, i18n::tr(name)).on_hover_text(i18n::tr(tip)).clicked() {
                    self.strip_flip(back);
                }
            }
        });
        if self.strip.back {
            ui.label(egui::RichText::new(i18n::tr("裏は見るだけです。並べ方を直すときは表に戻します")).color(NOTE));
        }
        ui.add_enabled_ui(!self.strip.back, |ui| ui.horizontal_wrapped(|ui| {
            for (t, name, tip) in [
                (StripTool::Move, "動かす", "部品や線の端を穴ごとにドラッグして動かします。何も無い所からドラッグすると四角で囲み、囲んだ所をドラッグすると、中の部品・線の端・切る穴をまとめて動かし、R で回します (ストリップ基板は半回転: 帯は横のままなので、中のつながりは変わりません。ユニバーサル基板は 90°)。選んだ部品は R で回り (2 本足は立てる ↔ 寝かせる、IC はストリップ基板では向きを逆に、ユニバーサル基板では 90°)、選んだ線は Delete で消えます"),
                (StripTool::Link, "線を引く", "穴をクリックして、つなぐ先の穴をクリックすると、表を通す線を足します。Esc でやめます"),
                (StripTool::Cut, "帯を切る", "穴をクリックすると、そこで銅の帯を切ります。切った穴をもう一度クリックすると、元に戻します"),
                (StripTool::Bare, "すずめっき線", "裏 (銅の面) に引くすずめっき線を手で足します。穴をクリックして、同じ行か列の穴をクリックします。右クリックした穴を通る手の線は消えます。線の両端と、途中で部品の足や別の線に重なる穴がつながります"),
            ] {
                // the stripboard cuts its strips; the perfboard (no strips) has bare wires drawn by hand
                if (t == StripTool::Bare && strip) || (t == StripTool::Cut && !strip) {
                    continue;
                }
                if ui.selectable_label(self.strip.tool == t, i18n::tr(name)).on_hover_text(i18n::tr(tip)).clicked() {
                    self.strip.tool = t;
                    self.strip.link_start = None;
                }
            }
        }));
        self.strip_history_check(sch);
        ui.horizontal(|ui| {
            if crate::icons::icon_button_key(ui, crate::icons::Icon::Undo, "元に戻す", "Ctrl+Z", false, !self.strip.undo.is_empty(), "手で動かした・線を引いた・帯を切った・並べ直した操作を 1 つ取り消します (Ctrl+Z)").clicked() {
                self.strip_undo(sch, true);
            }
            if crate::icons::icon_button_key(ui, crate::icons::Icon::Redo, "やり直し", "Ctrl+Y", false, !self.strip.redo.is_empty(), "取り消した操作をもう一度します (Ctrl+Y)").clicked() {
                self.strip_undo(sch, false);
            }
        });
        ui.horizontal(|ui| {
            if sch.stripboard.is_some() {
                if ui.button(i18n::tr("自動で並べ直す")).on_hover_text(i18n::tr("回路図から並べ方を作り直します。手で動かした分は消えます (元に戻すで戻せます)")).clicked() {
                    self.strip_set(sch, None);
                    self.strip.sel = None;
                    self.strip.picked_link = None;
                }
            } else {
                ui.label(egui::RichText::new(i18n::tr("(自動で並べた形)")).color(NOTE)).on_hover_text(i18n::tr(
                    "まだ手で動かしていないので、回路図を変えると並べ直されます。動かすと、その並べ方が回路図のファイルに入り、回路図を変えたときは、足した部品だけが基板の右に加わり、そのネットへの線も引かれます (帯の片に空いた穴が無くて引けない線は、照合が言います)",
                ));
            }
        });
        // the perfboard: lock the parts the person placed, and lay the rest out round them
        if !strip {
            ui.horizontal_wrapped(|ui| {
                if let Some(r) = self.picked.clone().filter(|r| l.parts.iter().any(|p| &p.refdes == r)) {
                    let locked = l.locked.contains(&r);
                    let label = if locked { i18n::trf!("{} の固定を外す", r) } else { i18n::trf!("{} を固定する", r) };
                    if ui.button(label).on_hover_text(i18n::tr("選んだ部品を今の穴に固定します (L)。「固定した部品のまわりを並べ直す」で、固定した部品は動かさず、ほかの部品を並べ直します。コネクタやつまみのようにケースの穴に合わせる部品、大きい IC など、置き場所を自分で決めたい部品に使います")).clicked() {
                        let mut m = l.clone();
                        self.strip_lock_toggle(sch, &mut m, &r);
                    }
                }
                if !l.pinned.is_empty() {
                    let busy = self.strip.around.is_some();
                    if ui
                        .add_enabled(!busy, egui::Button::new(i18n::trf!("置いた区画の箱 ({} 個) を自動に戻す", l.pinned.len())))
                        .on_hover_text(i18n::tr("区画の名前をドラッグして置いた箱の場所を自動に任せ、箱を詰め直して線を引き直します (元に戻すで戻せます)"))
                        .clicked()
                    {
                        let mut m = l.clone();
                        m.pinned.clear();
                        self.strip_relayout(sch, &m);
                    }
                }
                let n = l.locked.len();
                if n > 0 {
                    let busy = self.strip.around.is_some();
                    if ui
                        .add_enabled(!busy, egui::Button::new(i18n::trf!("固定した部品 ({} 個) のまわりを並べ直す", n)))
                        .on_hover_text(i18n::tr("固定した部品はその穴のまま、ほかの部品を自動で置き直し、線を全部引き直します。固定した部品のまわりは、線の通り道に少し空けます (元に戻すで戻せます)"))
                        .clicked()
                    {
                        self.strip_relayout(sch, l);
                    }
                }
            });
            self.strip_lock_hints_ui(ui, l);
        }
        let hint = match self.strip.tool {
            StripTool::Move if strip => "部品・線の端をドラッグ / 何も無い所から四角で囲んでまとめて動かす・R で半回転 / R で部品を回す / Delete で線を消す / Ctrl+Z で戻す",
            StripTool::Move => "部品・線の端をドラッグ / 何も無い所から四角で囲んでまとめて動かす・R で 90° 回す / R で部品を 90° 回す / L で部品を固定 / 区画の名前をドラッグで箱を置く / Delete で線を消す / Ctrl+Z で戻す",
            StripTool::Link => "穴をクリックして、つなぐ先の穴をクリックします",
            StripTool::Cut => "帯を切る穴をクリックします (切った穴はもう一度で戻ります)",
            StripTool::Bare => "穴をクリックして、同じ行か列の穴をクリックします / 右クリックで手の線を消す",
        };
        ui.label(egui::RichText::new(i18n::tr(hint)).color(NOTE).small());
    }

    /// The left panel of the stripboard / perfboard views.
    /// The checks of `l` (`strip_check`, `strip_rules`, `strip_unjoined`), kept while the schematic and the layout stay the same.
    pub(super) fn strip_checks(&mut self, sch: &SchEditor, l: &StripLayout) -> StripChecks {
        if let Some((r, k, c)) = &self.strip.checks
            && *r == sch.rev
            && k == l
            && sch.settled()
        {
            return c.clone();
        }
        let c = StripChecks { problems: strip_check(&sch.sch, l), rules: strip_rules(&sch.sch, l), unjoined: strip_unjoined(&sch.sch, l) };
        self.strip.checks = Some((sch.rev, l.clone(), c.clone()));
        c
    }

    pub(super) fn strip_panel(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        let l = self.strip_shown(sch);
        let strip = self.kind == BoardKind::Stripboard;
        ui.label(
            spaced(if strip {
                "銅の帯が横に並んだ基板 (ストリップ基板) に組む並べ方です。IC の段の上と下に帯の組があり、1 本ずつ 1 つのネットです (左に名前)。2 本足の部品は、つなぐ 2 つの帯をまたいで縦に差します。IC は × の穴で帯を切って足ごとに分け、上半分の足は上の帯へ、下半分の足は下の帯へ色の線でつなぎます。同じネットの帯どうしは左の線でつなぎます。帯を切るのは、裏から穴の銅を削ります (ドリルの刃を手で回す・専用のカッター)"
            } else {
                "穴だけの基板 (ユニバーサル基板) に組む並べ方です。帯が無いので、部品はつなぐ相手の近くに置き (長いヘッダは上の縁に寝かせて)、ネットごとに灰色の線 (すずめっき線) を裏で縦・横にまっすぐはんだ付けします。ほかのネットの線とは触れません。裏で通れない所は、色の線 (表を通すビニール線) で飛び越えます"
            })
            .color(NOTE),
        );
        ui.add_space(4.0);
        if self.strip_busy().0 {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(i18n::tr("並べています…")).on_hover_text(i18n::tr(
                    "部品の置き方と線の引き方をいくつも試して、表の線がいちばん少ない形を選んでいます (ユニバーサル基板で最長 1.5 秒)。終わるまでは前の並べ方を見せ、自動の並べ方は動かせません",
                ));
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            // nothing to show yet: no check, size or list of an empty board
            if l.parts.is_empty() {
                return;
            }
        }
        self.strip_around_done(sch);
        self.strip_size_ui(ui, sch, &l);
        self.strip_tools(ui, sch, &l);
        self.build_ui(ui, sch, &l);
        let StripChecks { problems, rules, .. } = self.strip_checks(sch, &l);
        ui.add_space(4.0);
        if problems.is_empty() {
            ui.colored_label(crate::theme::OK, i18n::tr("✔ 回路図どおりにつながっています"));
        } else {
            for p in &problems {
                ui.colored_label(BAD, i18n::trf!("・{}", i18n::tr_dyn(p)));
            }
        }
        // a perfboard's wires on top that lie along a chip's row of pins (the automatic layout lays one so where nothing else gets through)
        if !strip {
            for (k, part) in links_along_pins(&l) {
                let w = &l.links[k];
                ui.colored_label(crate::theme::WARN, i18n::trf!("▲ 表の線 {}–{} が {} の足の列の上を通ります", hole_name(w.a), hole_name(w.b), part)).on_hover_text(i18n::tr(
                    "表の線が IC やヘッダの足の上に沿って走っていて、ソケットの上をまたぐ形になります。電気的には被覆があるのでつながりませんが、組みにくく、絵では足とつながって見えます。線の端をドラッグして足の列を避けるか、そのままにします (自動の並べ方は、ほかに通れる所が無いときにこう引きます)",
                ));
            }
        }
        if !l.sections.is_empty() || !rules.is_empty() {
            ui.label(egui::RichText::new(i18n::tr("区画とつなぎ方の決まり")).strong()).on_hover_text(i18n::tr(
                "回路図の「設計の要件」の表から: 「近くに置く組」で「区画」に印を付けた組ごとにまとめて置き、区画の間は帯を切ります。GND は区画ごとに 1 本の線で、表のいちばん上の区画の GND の帯 (1 点) へ戻します。「離す」の行は、基板の上のいちばん近い穴どうしの距離で確かめます。「ケルビン」の行は、測定の「分かれ目」: 抵抗の足から電流の線とセンスの線への道が、帯の上を一緒に走る長さです (自動の並べ方は、足が 2 本の線の間に来るように部品や線の端をずらします)",
            ));
            if !l.sections.is_empty() {
                ui.small(l.sections.iter().map(|(n, a, b)| format!("{n} ({} – {})", hole_name(*a), hole_name(*b))).collect::<Vec<_>>().join("、"));
            }
            for r in &rules {
                ui.colored_label(if r.ok { crate::theme::OK } else { crate::theme::WARN }, format!("{} {}", if r.ok { "✔" } else { "▲" }, i18n::tr_dyn(&r.message)));
            }
        } else {
            ui.small(i18n::tr("区画に分けるには、回路図の「設計の要件」→「近くに置く組」で組を作り、「区画」に印を付けます"));
        }
        // no sections yet: the proposal (also when the table has other rules)
        if l.sections.is_empty() {
            self.strip_plan_ui(ui, sch);
        }
        ui.small(i18n::trf!("大きさ: {} 列 × {} 行 (2.54mm の間隔で {:.0} × {:.0} mm)", l.cols, l.rows, l.cols as f32 * 2.54, l.rows as f32 * 2.54));
        if ui
            .button(i18n::tr("印刷用の図を保存 (表と裏)"))
            .on_hover_text(i18n::tr("穴の格子に部品・線・切る穴を実寸で描いた図を、表 (部品の面) と裏 (銅の面。左右が逆) の 2 つの SVG に保存します。ブラウザで開いて 100 % (実寸) で印刷すると、図の穴が基板の穴に重なります。右下の 10 mm の線で大きさを確かめられます"))
            .clicked()
        {
            self.save_strip_print(sch, &l);
        }
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(egui::RichText::new(i18n::tr("部品と差す穴")).strong());
            for p in &l.parts {
                ui.horizontal_top(|ui| {
                    ui.add_sized([44.0, 18.0], egui::Label::new(egui::RichText::new(&p.refdes).strong()));
                    if l.locked.contains(&p.refdes) {
                        ui.label(egui::RichText::new(i18n::tr("固定")).color(LOCK).small()).on_hover_text(i18n::tr("この部品は固定してあります。「固定した部品のまわりを並べ直す」で動きません"));
                    }
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(p.leads.iter().map(|(pin, at)| format!("{pin}:{}", hole_name(*at))).collect::<Vec<_>>().join("  ")).wrap());
                        // a socket or an adapter under the part (a leaded twin is in the list below, once for all)
                        let how = if p.mount == Mount::Leaded { String::new() } else { p.mount.describe() };
                        if !how.is_empty() {
                            ui.small(egui::RichText::new(i18n::tr_dyn(&how)).color(NOTE));
                        }
                    });
                });
            }
            let list = shopping(&l.parts.iter().map(|p| (p.refdes.clone(), p.mount.clone())).collect::<Vec<_>>());
            if !list.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr("ソケットと変換基板")).strong()).on_hover_text(i18n::tr(
                    "IC は基板に直接はんだ付けせず、丸ピンのソケットに差します。こての熱で傷めず、壊れたら差し替えられます。足の細かい表面実装の IC (SOP・QFN・QFP・SOT-23) は、変換基板にはんだ付けしてから、そのピンを丸ピンの連結ソケットに差します。変換基板はチップの 1 番ピンが変換基板の 1 番ピン (DIP の数え方) になるので、上の穴の表は、部品のピンの番号のままで差せます。チップ抵抗・コンデンサは、同じ値のリード部品で組めます",
                ));
                for (item, n, refs) in &list {
                    ui.small(format!("{}  ×{n}  ({})", i18n::tr_dyn(item), refs.join(", ")));
                }
            }
            if strip && !l.cuts.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::trf!("帯を切る穴 ({} か所)", l.cuts.len())).strong()).on_hover_text(i18n::tr("この穴で銅の帯を切り、左右を分けます。切ったら、テスターの抵抗のレンジで、左右がつながっていないことを確かめます"));
                ui.add(egui::Label::new(l.cuts.iter().map(|c| hole_name(*c)).collect::<Vec<_>>().join(" ")).wrap());
            }
            if !strip {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr("裏のすずめっき線")).strong()).on_hover_text(i18n::tr("同じ行の穴を、端から端まで 1 本の線でつないで、はんだ付けします"));
                for (a, b) in perf_wires(&l) {
                    ui.small(format!("{} – {}", hole_name(a), hole_name(b)));
                }
            }
            if !l.links.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::trf!("表の線 ({} 本)", l.links.len())).strong());
                for k in &l.links {
                    ui.small(format!("{} – {}  ({})", hole_name(k.a), hole_name(k.b), k.net));
                }
            }
            if !l.off.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr("基板の外 (電源・電池・信号源)")).strong());
                for (r, pins) in &l.off {
                    ui.small(format!("{r}  {}", pins.iter().map(|(pin, net, at)| format!("{pin} ({net}) → {}", hole_name(*at))).collect::<Vec<_>>().join("、")));
                }
            }
        });
    }

    /// The board drawn.
    pub(super) fn strip_canvas(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        let mut l = self.strip_shown(sch);
        let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, crate::canvas_theme::col(crate::canvas_theme::Role::Bg));
        let (busy, stale) = self.strip_busy();
        if busy {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        if l.parts.is_empty() {
            let msg = if busy { "並べています…" } else { "回路図に部品がありません" };
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, i18n::tr(msg), egui::FontId::proportional(16.0), Color32::from_gray(200));
            return;
        }
        // a margin on the left for the strips' names and the supplies off the board
        let left = 9.0;
        let w = left + l.cols as f32 + 1.0;
        let h = l.rows as f32 + 1.5;
        let pitch = ((rect.width() - 40.0) / w).min((rect.height() - 40.0) / h).clamp(5.0, 30.0);
        // (the copper side has the margin at the right: the board's left edge is there now)
        let back = self.strip.back;
        let origin = Pos2::new(rect.left() + (rect.width() - w * pitch) / 2.0 + if back { 1.0 } else { left } * pitch, rect.top() + 30.0 + pitch);
        if response.hovered() && self.strip.drag.is_none() && !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.is_none()) {
            self.strip_flip(!back);
        }
        if self.strip.back {
            self.strip_back(sch, &l, &response, &painter, origin, pitch);
            return;
        }
        // while dragging, the board keeps the size and place it had when the drag began (it may grow under the pointer)
        let (origin, pitch) = self.strip.frozen.unwrap_or((origin, pitch));
        let to = |a: At| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch;
        self.strip.drawn = Some((origin, pitch));
        // an old automatic layout shown while the new one is made is not edited (its parts are the old schematic's)
        if !stale {
            self.strip_interact(ui, &response, sch, &mut l, origin, pitch);
        }
        let board = Rect::from_min_max(to((0, 0)) - Vec2::splat(pitch * 0.8), to((l.rows - 1, l.cols - 1)) + Vec2::splat(pitch * 0.8));
        painter.rect_filled(board, 3.0, BOARD);
        let font = egui::FontId::monospace((pitch * 0.55).clamp(8.0, 13.0));
        let strip = self.kind == BoardKind::Stripboard;
        if strip {
            for (r, a, b) in l.pieces() {
                painter.rect_filled(Rect::from_min_max(to((r, a)) - Vec2::new(pitch * 0.45, pitch * 0.32), to((r, b)) + Vec2::new(pitch * 0.45, pitch * 0.32)), 1.0, COPPER.gamma_multiply(0.75));
            }
        }
        for r in 0..l.rows {
            for c in 0..l.cols {
                painter.circle_filled(to((r, c)), pitch * 0.13, Color32::from_gray(70));
            }
        }
        if strip {
            for cut in &l.cuts {
                let p = to(*cut);
                let d = pitch * 0.3;
                let s = Stroke::new(2.0, Color32::from_rgb(200, 30, 30));
                painter.line_segment([p - Vec2::splat(d), p + Vec2::splat(d)], s);
                painter.line_segment([p + Vec2::new(-d, d), p + Vec2::new(d, -d)], s);
            }
        } else {
            for (a, b) in perf_wires(&l) {
                painter.line_segment([to(a), to(b)], Stroke::new(pitch * 0.18, WIRE));
            }
        }
        // the home strips' names, and the row letters and column numbers
        for (r, net) in l.homes.iter().enumerate() {
            // ground's black would not show on the dark canvas: its name in grey
        let named = if net_colour(net) == Color32::from_gray(30) { Color32::from_gray(170) } else { net_colour(net) };
        painter.text(to((r as u16, 0)) - Vec2::new(pitch * 1.0, 0.0), egui::Align2::RIGHT_CENTER, net, font.clone(), named);
        }
        for c in (0..l.cols).step_by(5) {
            painter.text(to((0, c)) - Vec2::new(0.0, pitch * 1.0), egui::Align2::CENTER_BOTTOM, format!("{}", c + 1), font.clone(), Color32::from_gray(150));
        }
        // the parts
        for p in &l.parts {
            let pts: Vec<Pos2> = p.leads.iter().map(|(_, a)| to(*a)).collect();
            if pts.is_empty() {
                continue;
            }
            let bb = Rect::from_points(&pts);
            // an adapter board (green) or a socket's frame under the chip
            if p.mount.width().is_some() || p.mount.one_row() {
                let fill = match p.mount {
                    Mount::Socket { .. } => SOCKET,
                    Mount::Module { .. } => MODULE,
                    _ => ADAPTER,
                };
                painter.rect_filled(bb.expand2(Vec2::new(pitch * 0.45, pitch * 0.5)), 2.0, fill);
            }
            match p.look {
                sch_core::breadboard::Look::Dip | sch_core::breadboard::Look::Row => {
                    let body = bb.expand2(Vec2::new(pitch * 0.35, pitch * 0.45));
                    match &p.mount {
                        // the small chip in the middle of its adapter, the package's name under the reference
                        // a module: its name under the reference, on its own board (no chip body: the module is the board)
                        Mount::Module { name, .. } => {
                            painter.text(body.center(), egui::Align2::CENTER_BOTTOM, &p.refdes, font.clone(), Color32::WHITE);
                            painter.text(body.center() + Vec2::new(0.0, 2.0), egui::Align2::CENTER_TOP, name, egui::FontId::proportional((pitch * 0.5).clamp(8.0, 13.0)), Color32::from_gray(225));
                        }
                        Mount::Adapter { package, .. } => {
                            let side = (body.width().min(body.height()) * 0.5).clamp(pitch * 0.8, pitch * 2.5);
                            let chip = Rect::from_center_size(body.center(), Vec2::splat(side));
                            painter.rect_filled(chip, 1.0, Color32::from_gray(30));
                            painter.text(chip.center(), egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::WHITE);
                            painter.text(chip.center_bottom() + Vec2::new(0.0, 2.0), egui::Align2::CENTER_TOP, package, egui::FontId::proportional((pitch * 0.45).clamp(7.0, 11.0)), Color32::from_gray(225));
                        }
                        _ => {
                            // a DIP's body between its two rows of pins (side by side, or above and below when turned on a perfboard)
                            let distinct = |f: &dyn Fn(&At) -> u16| p.leads.iter().map(|(_, a)| f(a)).collect::<std::collections::BTreeSet<u16>>().len();
                            let turned = distinct(&|a| a.1) != 2 && distinct(&|a| a.0) == 2;
                            let across = if turned { Vec2::new(0.0, pitch * 0.55) } else { Vec2::new(pitch * 0.55, 0.0) };
                            painter.rect_filled(if p.look == sch_core::breadboard::Look::Dip { body.shrink2(across) } else { body }, 2.0, Color32::from_gray(35));
                            painter.text(body.center(), egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::WHITE);
                        }
                    }
                }
                _ => {
                    for w in pts.windows(2) {
                        painter.line_segment([w[0], w[1]], Stroke::new(1.5, Color32::from_gray(110)));
                    }
                    let mid = bb.center();
                    let body = if bb.height() >= bb.width() { Rect::from_center_size(mid, Vec2::new(pitch * 0.55, (bb.height() * 0.5).clamp(pitch * 0.8, pitch * 3.0))) } else { Rect::from_center_size(mid, Vec2::new((bb.width() * 0.5).max(pitch * 0.8), pitch * 0.55)) };
                    painter.rect_filled(body, 3.0, Color32::from_rgb(230, 205, 160));
                    painter.rect_stroke(body, 3.0, Stroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);
                    painter.text(body.right_center() + Vec2::new(2.0, 0.0), egui::Align2::LEFT_CENTER, &p.refdes, font.clone(), Color32::from_gray(20));
                }
            }
            for q in &pts {
                painter.circle_filled(*q, pitch * 0.18, Color32::from_gray(150));
            }
            // a locked part: a small padlock at its top-left corner
            if l.locked.contains(&p.refdes) {
                // (outside the picked part's frame, which is 0.6 of a hole out)
                padlock(&painter, bb.left_top() + Vec2::new(-pitch * 1.0, -pitch * 1.05), pitch * 0.62);
            }
        }
        // the links over the parts (one along a chip's pins marked orange beneath), and the supplies off the board
        if !strip {
            for (k, _) in links_along_pins(&l) {
                let w = &l.links[k];
                painter.line_segment([to(w.a), to(w.b)], Stroke::new(pitch * 0.42, crate::theme::WARN.gamma_multiply(0.7)));
            }
        }
        for k in &l.links {
            let c = net_colour(&k.net);
            painter.line_segment([to(k.a), to(k.b)], Stroke::new(pitch * 0.16, c));
            for e in [k.a, k.b] {
                painter.circle_filled(to(e), pitch * 0.17, c);
            }
        }
        // the sections: a frame and the name above it, and the strips' names at each section's left end
        for (name, a, b) in &l.sections {
            let r = Rect::from_two_pos(to(*a), to(*b)).expand(pitch * 0.45);
            painter.rect_stroke(r, 2.0, Stroke::new(1.5, Color32::from_rgb(90, 90, 200)), egui::StrokeKind::Outside);
            let tag = painter.text(r.left_top() + Vec2::new(2.0, -2.0), egui::Align2::LEFT_BOTTOM, name, font.clone(), Color32::from_rgb(150, 160, 255));
            // a box put by hand: a padlock after its name
            if l.pinned.contains(name) {
                padlock(&painter, Pos2::new(tag.right() + pitch * 0.35, tag.center().y), pitch * 0.5);
            }
        }
        // a section's box being dragged, or let go and waiting for the layout round it
        let ghost = match &self.strip.drag {
            Some(StripDrag::Section { k, from, now }) => l.sections.get(*k).map(|(_, a, b)| box_moved(*a, *b, *from, *now)),
            _ => self.strip.put_box,
        };
        if let Some((a, b)) = ghost {
            let r = Rect::from_two_pos(to(a), to(b)).expand(pitch * 0.45);
            painter.rect_filled(r, 2.0, PICK.gamma_multiply(0.1));
            painter.rect_stroke(r, 2.0, Stroke::new(2.0, PICK), egui::StrokeKind::Outside);
        }
        for (a, n) in &l.names {
            let named = if net_colour(n) == Color32::from_gray(30) { Color32::from_gray(20) } else { net_colour(n) };
            painter.text(to(*a) + Vec2::new(-pitch * 0.4, -pitch * 0.32), egui::Align2::LEFT_BOTTOM, n, egui::FontId::proportional((pitch * 0.42).clamp(7.0, 11.0)), named);
        }
        // what is still to be joined (as a board's ratsnest)
        for (_, a, b) in self.strip_checks(sch, &l).unjoined {
            super::rats_line(&painter, to(a), to(b), pitch);
        }
        // the parts worth placing by hand first (perfboard): a dashed frame
        if !strip && self.strip.tool == StripTool::Move {
            let hinted: BTreeSet<String> = lock_first(&l).into_iter().map(|x| x.0).collect();
            for p in l.parts.iter().filter(|p| hinted.contains(&p.refdes)) {
                let pts: Vec<Pos2> = p.leads.iter().map(|(_, a)| to(*a)).collect();
                let r = Rect::from_points(&pts).expand(pitch * 0.75);
                let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(egui::Shape::dashed_line(&c, Stroke::new(2.0, HINT), pitch * 0.4, pitch * 0.3));
            }
        }
        if let Some(m) = self.build_marks() {
            super::build_steps::draw_build_marks(&painter, &l, &m, to, pitch);
        }
        // the picked part and link, the box, the first end of a link being drawn
        if let Some(p) = self.picked.as_ref().and_then(|r| l.parts.iter().find(|p| &p.refdes == r)) {
            let pts: Vec<Pos2> = p.leads.iter().map(|(_, a)| to(*a)).collect();
            painter.rect_stroke(Rect::from_points(&pts).expand(pitch * 0.6), 3.0, Stroke::new(2.0, PICK), egui::StrokeKind::Outside);
        }
        if let Some(k) = self.strip.picked_link.and_then(|k| l.links.get(k)) {
            painter.line_segment([to(k.a), to(k.b)], Stroke::new(pitch * 0.34, PICK.gamma_multiply(0.6)));
        }
        if let Some((a, b)) = self.strip.sel {
            let r = Rect::from_two_pos(to(a), to(b)).expand(pitch * 0.5);
            painter.rect_filled(r, 2.0, PICK.gamma_multiply(0.12));
            painter.rect_stroke(r, 2.0, Stroke::new(1.5, PICK), egui::StrokeKind::Inside);
        }
        if let Some(a) = self.strip.link_start {
            painter.circle_stroke(to(a), pitch * 0.35, Stroke::new(2.0, PICK));
            // the bare wire it would draw, while the pointer is on the start's row or column
            if self.strip.tool == StripTool::Bare
                && let Some(p) = response.hover_pos()
            {
                let v = (p - origin) / pitch;
                let h = (v.y.round().max(0.0) as u16, v.x.round().max(0.0) as u16);
                if h != a && (h.0 == a.0 || h.1 == a.1) {
                    painter.line_segment([to(a), to(h)], Stroke::new(pitch * 0.18, PICK.gamma_multiply(0.7)));
                }
            }
        }
        for (k, (r, pins)) in l.off.iter().enumerate() {
            let box_at = origin + Vec2::new(-left * pitch + pitch * 0.5, (k as f32 * 2.2 - 0.8) * pitch);
            let body = Rect::from_min_size(box_at, Vec2::new(pitch * 3.0, pitch * 1.6));
            painter.rect_filled(body, 3.0, Color32::from_gray(60));
            painter.text(body.center(), egui::Align2::CENTER_CENTER, r, font.clone(), Color32::WHITE);
            for (pin, net, at) in pins {
                let _ = pin;
                painter.line_segment([body.right_center(), to(*at)], Stroke::new(pitch * 0.12, net_colour(net)));
            }
        }
    }
}

impl BreadboardView {
    /// Turns the board over (`back`: the copper side): what was being dragged or drawn is let go.
    fn strip_flip(&mut self, back: bool) {
        self.strip.back = back;
        self.strip.drag = None;
        self.strip.frozen = None;
        self.strip.link_start = None;
        self.strip.sel = None;
    }

    /// The copper side, seen from below with left and right swapped (as the board is held to solder): the copper (the strips and
    /// their cuts, or the bare wires), a ring of solder at every hole with a lead or a wire's end (a square one at a chip's pin 1),
    /// the parts above as faint outlines. The hole under the pointer lights what it is joined to and names its net; a click on a
    /// lead picks its part. Only to look at: nothing is edited here.
    fn strip_back(&mut self, sch: &SchEditor, l: &StripLayout, response: &egui::Response, painter: &egui::Painter, origin: Pos2, pitch: f32) {
        let last = l.cols.saturating_sub(1);
        let to = |a: At| origin + Vec2::new((last - a.1.min(last)) as f32, a.0 as f32) * pitch;
        let hole_at = |p: Pos2| -> Option<At> {
            let v = (p - origin) / pitch;
            let (r, c) = (v.y.round(), v.x.round());
            (r >= 0.0 && c >= 0.0 && r < l.rows as f32 && c < l.cols as f32).then(|| (r as u16, last - c as u16))
        };
        self.strip.drawn = Some((origin, pitch));
        let strip = self.kind == BoardKind::Stripboard;
        let font = egui::FontId::monospace((pitch * 0.55).clamp(8.0, 13.0));
        let board = Rect::from_two_pos(to((0, 0)), to((l.rows.saturating_sub(1), last))).expand(pitch * 0.8);
        painter.rect_filled(board, 3.0, BOARD);
        painter.text(response.rect.left_top() + Vec2::new(12.0, 6.0), egui::Align2::LEFT_TOP, i18n::tr("裏 (はんだの面): 左右が逆です。表の左の端が、ここでは右の端"), egui::FontId::proportional((pitch * 0.6).clamp(10.0, 14.0)), Color32::from_gray(200));
        // the copper: the strips and their cuts, or a perfboard's pads round every hole and its bare wires
        if strip {
            for (r, a, b) in l.pieces() {
                painter.rect_filled(Rect::from_two_pos(to((r, a)), to((r, b))).expand2(Vec2::new(pitch * 0.45, pitch * 0.32)), 1.0, COPPER);
            }
        }
        for r in 0..l.rows {
            for c in 0..l.cols {
                if !strip {
                    painter.circle_filled(to((r, c)), pitch * 0.3, COPPER.gamma_multiply(0.55));
                }
                painter.circle_filled(to((r, c)), pitch * 0.13, Color32::from_gray(70));
            }
        }
        let wires = if strip { vec![] } else { perf_wires(l) };
        for (a, b) in &wires {
            painter.line_segment([to(*a), to(*b)], Stroke::new(pitch * 0.22, WIRE));
        }
        if strip {
            for cut in &l.cuts {
                let (p, d) = (to(*cut), pitch * 0.3);
                let s = Stroke::new(2.0, Color32::from_rgb(200, 30, 30));
                painter.line_segment([p - Vec2::splat(d), p + Vec2::splat(d)], s);
                painter.line_segment([p + Vec2::new(-d, d), p + Vec2::new(d, -d)], s);
            }
        }
        // the rows' home strips named at the right (the board's left edge), the columns numbered every five
        for (r, net) in l.homes.iter().enumerate() {
            let named = if net_colour(net) == Color32::from_gray(30) { Color32::from_gray(170) } else { net_colour(net) };
            painter.text(to((r as u16, 0)) + Vec2::new(pitch * 1.0, 0.0), egui::Align2::LEFT_CENTER, net, font.clone(), named);
        }
        for c in (0..l.cols).step_by(5) {
            painter.text(to((0, c)) - Vec2::new(0.0, pitch * 1.0), egui::Align2::CENTER_BOTTOM, format!("{}", c + 1), font.clone(), Color32::from_gray(150));
        }
        // the parts above, faint: a frame round a chip's pins (a line between a two-lead part's), the name in the middle
        let faint = Color32::from_black_alpha(120);
        for p in &l.parts {
            let pts: Vec<Pos2> = p.leads.iter().map(|(_, a)| to(*a)).collect();
            if pts.is_empty() {
                continue;
            }
            let bb = Rect::from_points(&pts);
            if p.leads.len() > 2 {
                painter.rect_stroke(bb.expand(pitch * 0.45), 2.0, Stroke::new(1.0, faint), egui::StrokeKind::Outside);
            } else {
                for w in pts.windows(2) {
                    painter.line_segment([w[0], w[1]], Stroke::new(1.0, faint));
                }
            }
            painter.text(bb.center(), egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::from_black_alpha(190));
        }
        for (name, a, b) in &l.sections {
            let r = Rect::from_two_pos(to(*a), to(*b)).expand(pitch * 0.45);
            painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_rgb(90, 90, 200).gamma_multiply(0.6)), egui::StrokeKind::Outside);
            painter.text(r.left_top() + Vec2::new(2.0, -2.0), egui::Align2::LEFT_BOTTOM, name, font.clone(), Color32::from_rgb(150, 160, 255));
        }
        // the holes to solder: a lead, a link's end or a supply's wire in it
        let used = l.used();
        let solder = Color32::from_gray(205);
        for (a, _) in &used {
            painter.circle_filled(to(*a), pitch * 0.24, solder);
            painter.circle_stroke(to(*a), pitch * 0.24, Stroke::new(1.0, Color32::from_gray(90)));
        }
        for k in &l.links {
            for e in [k.a, k.b] {
                painter.circle_filled(to(e), pitch * 0.1, net_colour(&k.net));
            }
        }
        // a chip's pin 1: a square round it (the mark to find the chip's way round from below)
        for p in l.parts.iter().filter(|p| p.leads.len() > 2) {
            if let Some((_, a)) = p.leads.iter().find(|(pin, _)| pin == "1") {
                painter.rect_stroke(Rect::from_center_size(to(*a), Vec2::splat(pitch * 0.66)), 0.0, Stroke::new(1.5, Color32::from_gray(40)), egui::StrokeKind::Middle);
            }
        }
        for (_, a, b) in self.strip_checks(sch, l).unjoined {
            super::rats_line(painter, to(a), to(b), pitch);
        }
        if let Some(m) = self.build_marks() {
            super::build_steps::draw_build_marks(painter, l, &m, to, pitch);
        }
        // the picked part (in the list, or by a click on one of its leads here)
        if let Some(p) = self.picked.as_ref().and_then(|r| l.parts.iter().find(|p| &p.refdes == r)) {
            let pts: Vec<Pos2> = p.leads.iter().map(|(_, a)| to(*a)).collect();
            painter.rect_stroke(Rect::from_points(&pts).expand(pitch * 0.6), 3.0, Stroke::new(2.0, PICK), egui::StrokeKind::Outside);
        }
        let Some(h) = response.hover_pos().and_then(hole_at) else { return };
        // the hole under the pointer: what it is joined to, lit, and its net named
        let joined = l.joined_to(h);
        let lit = Stroke::new(pitch * 0.3, PICK.gamma_multiply(0.8));
        for (a, b) in wires.iter().filter(|(a, b)| joined.contains(a) && joined.contains(b)) {
            painter.line_segment([to(*a), to(*b)], lit);
        }
        if strip {
            for (r, a, b) in l.pieces().into_iter().filter(|(r, a, _)| joined.contains(&(*r, *a))) {
                painter.rect_stroke(Rect::from_two_pos(to((r, a)), to((r, b))).expand2(Vec2::new(pitch * 0.45, pitch * 0.32)), 1.0, Stroke::new(2.0, PICK), egui::StrokeKind::Outside);
            }
        }
        for (a, _) in used.iter().filter(|(a, _)| joined.contains(a)) {
            painter.circle_stroke(to(*a), pitch * 0.34, Stroke::new(2.0, PICK));
        }
        painter.circle_stroke(to(h), pitch * 0.42, Stroke::new(1.5, Color32::WHITE));
        if !self.strip.back_net.as_ref().is_some_and(|(r, a, _)| *r == sch.rev && *a == h && sch.settled()) {
            self.strip.back_net = Some((sch.rev, h, net_at(&sch.sch, l, h)));
        }
        let net = self.strip.back_net.as_ref().and_then(|x| x.2.clone());
        let here: Vec<String> = used
            .iter()
            .filter(|(a, _)| *a == h)
            .map(|(_, who)| match who.split_once('.') {
                Some((r, pin)) => i18n::trf!("{} の {} 番ピン", r, pin),
                None => i18n::tr("表の線の端").to_string(),
            })
            .collect();
        let mut tip = hole_name(h);
        if !here.is_empty() {
            tip = format!("{tip}: {}", here.join("、"));
        }
        if let Some(n) = &net {
            tip = format!("{tip}\n{}", i18n::trf!("ネット {} (つながる穴 {} 個)", n, joined.len()));
        }
        response.clone().on_hover_text_at_pointer(tip);
        if response.clicked()
            && let Some(p) = l.parts.iter().find(|p| p.leads.iter().any(|(_, a)| *a == h))
        {
            self.picked = if self.picked.as_deref() == Some(p.refdes.as_str()) { None } else { Some(p.refdes.clone()) };
        }
    }
}

impl BreadboardView {
    /// Asks where to save, and writes the parts' side and the copper side as two SVG files beside each other (`…_表.svg`, `…_裏.svg`).
    fn save_strip_print(&mut self, sch: &mut SchEditor, l: &StripLayout) {
        let perf = self.kind == BoardKind::Perfboard;
        let stem = sch.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "board".into());
        let name = format!("{stem}_{}", if perf { "perfboard" } else { "stripboard" });
        let Some(path) = rfd::FileDialog::new().add_filter("SVG画像", &["svg"]).set_file_name(format!("{name}.svg")).save_file() else { return };
        let base = path.with_extension("");
        let title = format!("{stem} — {}", if perf { "ユニバーサル基板" } else { "ストリップ基板" });
        let mut written = vec![];
        for (face, tag) in [(Face::Parts, "表"), (Face::Copper, "裏")] {
            let file = base.with_file_name(format!("{}_{tag}.svg", base.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()));
            if let Err(e) = std::fs::write(&file, strip_svg(l, face, perf, &title)) {
                sch.status = format!("印刷用の図を保存できませんでした: {e}。保存先のフォルダに書き込めるか確認してください");
                return;
            }
            written.push(file.display().to_string());
        }
        sch.status = format!("印刷用の図を保存しました: {}", written.join("、"));
    }

    /// The presses and drags on the board: a change is drawn while dragging (`l` becomes it) and kept with the sheet when let go.
    fn strip_interact(&mut self, ui: &egui::Ui, response: &egui::Response, sch: &mut SchEditor, l: &mut StripLayout, origin: Pos2, pitch: f32) {
        let to = |a: At| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch;
        let hole_at = |p: Pos2| -> Option<At> {
            let v = (p - origin) / pitch;
            let (r, c) = (v.y.round(), v.x.round());
            (r >= 0.0 && c >= 0.0 && r < 2000.0 && c < 2000.0).then_some((r as u16, c as u16))
        };
        let in_box = |a: At, (p, q): (At, At)| a.0 >= p.0.min(q.0) && a.0 <= p.0.max(q.0) && a.1 >= p.1.min(q.1) && a.1 <= p.1.max(q.1);
        let link_end_at = |l: &StripLayout, p: Pos2| l.links.iter().enumerate().flat_map(|(k, w)| [(k, 0, w.a), (k, 1, w.b)]).find(|(_, _, h)| to(*h).distance(p) < pitch * 0.4).map(|(k, e, _)| (k, e));
        let part_at = |l: &StripLayout, p: Pos2| {
            l.parts
                .iter()
                .find(|x| {
                    let pts: Vec<Pos2> = x.leads.iter().map(|(_, a)| to(*a)).collect();
                    !pts.is_empty() && Rect::from_points(&pts).expand(pitch * 0.5).contains(p)
                })
                .map(|x| x.refdes.clone())
        };
        // a section's name above its frame (perfboard): dragged, the box goes there
        let perf = self.kind == BoardKind::Perfboard;
        let section_at = |l: &StripLayout, p: Pos2| {
            l.sections.iter().position(|(name, a, b)| {
                let r = Rect::from_two_pos(to(*a), to(*b)).expand(pitch * 0.45);
                let w = (name.chars().count() as f32 * pitch * 0.6 + pitch * 1.5).min(r.width());
                perf && Rect::from_min_max(r.left_top() - Vec2::new(0.0, pitch * 0.9), r.left_top() + Vec2::new(w, pitch * 0.2)).contains(p)
            })
        };
        let tool = self.strip.tool;
        if tool == StripTool::Move
            && response.drag_started()
            // where the button went down (the drag starts a few points later)
            && let Some(p) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos())
            && let Some(h) = hole_at(p)
        {
            self.strip.frozen = Some((origin, pitch));
            self.strip.drag = Some(if let Some((k, end)) = link_end_at(l, p) {
                self.strip.picked_link = Some(k);
                StripDrag::LinkEnd { k, end }
            } else if self.strip.sel.is_some_and(|b| in_box(h, b)) {
                StripDrag::Block { from: h }
            } else if let Some(k) = section_at(l, p) {
                self.strip.sel = None;
                StripDrag::Section { k, from: h, now: h }
            } else if let Some(r) = part_at(l, p) {
                self.picked = Some(r.clone());
                self.strip.sel = None;
                StripDrag::Part { refdes: r, from: h }
            } else {
                self.strip.sel = None;
                StripDrag::Select { from: h }
            });
        }
        if let Some(d) = self.strip.drag.clone()
            && let Some(h) = response.interact_pointer_pos().or(ui.ctx().pointer_latest_pos()).and_then(hole_at)
        {
            let delta = |from: At| (h.0 as i32 - from.0 as i32, h.1 as i32 - from.1 as i32);
            let changed = match &d {
                StripDrag::LinkEnd { k, end } => Some(link_end_moved(&sch.sch, l, *k, *end, h)),
                StripDrag::Part { refdes, from } => {
                    let (dr, dc) = delta(*from);
                    part_moved(l, refdes, dr, dc)
                }
                StripDrag::Block { from } => {
                    let (dr, dc) = delta(*from);
                    self.strip.sel.and_then(|(a, b)| area_moved(l, a, b, dr, dc))
                }
                StripDrag::Select { from } => {
                    self.strip.sel = Some((*from, h));
                    None
                }
                StripDrag::Section { k, from, .. } => {
                    self.strip.drag = Some(StripDrag::Section { k: *k, from: *from, now: h });
                    if response.drag_stopped() && h != *from {
                        self.strip_put_box(sch, l, *k, *from, h);
                    }
                    None
                }
            };
            if let Some(m) = changed {
                if response.drag_stopped() && m != *l {
                    if let StripDrag::Block { from } = d {
                        let (dr, dc) = delta(from);
                        let shift = |a: At| ((a.0 as i32 + dr).max(0) as u16, (a.1 as i32 + dc).max(0) as u16);
                        self.strip.sel = self.strip.sel.map(|(a, b)| (shift(a), shift(b)));
                    }
                    self.strip_set(sch, Some(m.clone()));
                }
                *l = m;
            }
        }
        if response.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
            self.strip.drag = None;
            self.strip.frozen = None;
        }
        // clicks
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
            && let Some(h) = hole_at(p)
        {
            match tool {
                StripTool::Move => {
                    self.strip.picked_link = link_end_at(l, p).map(|(k, _)| k).or_else(|| {
                        // a click on a link's middle picks it too
                        l.links.iter().position(|k| {
                            let (a, b) = (to(k.a), to(k.b));
                            let t = ((p - a).dot(b - a) / (b - a).length_sq().max(1.0)).clamp(0.0, 1.0);
                            (a + (b - a) * t).distance(p) < pitch * 0.3
                        })
                    });
                    self.picked = if self.strip.picked_link.is_some() { None } else { part_at(l, p) };
                    self.strip.sel = None;
                }
                StripTool::Link => match self.strip.link_start.take() {
                    None => self.strip.link_start = Some(h),
                    Some(a) if a != h => {
                        let m = with_link(&sch.sch, l, a, h);
                        self.strip_set(sch, Some(m.clone()));
                        *l = m;
                    }
                    Some(_) => {}
                },
                StripTool::Cut => {
                    let m = cut_toggled(l, h);
                    self.strip_set(sch, Some(m.clone()));
                    *l = m;
                }
                StripTool::Bare => match self.strip.link_start.take() {
                    None => self.strip.link_start = Some(h),
                    Some(a) if a == h => {}
                    Some(a) => match with_bare(l, a, h) {
                        Some(m) => {
                            self.strip_set(sch, Some(m.clone()));
                            *l = m;
                        }
                        None => {
                            self.strip.link_start = Some(a);
                            sch.status = i18n::tr("すずめっき線は、同じ行か同じ列の穴へまっすぐ引きます (曲げるときは 2 本に分けます)").to_string();
                        }
                    },
                },
            }
        }
        // a right click on a bare wire drawn by hand takes it away
        if tool == StripTool::Bare
            && response.secondary_clicked()
            && let Some(h) = response.interact_pointer_pos().and_then(hole_at)
            && let Some(m) = without_bare_at(l, h)
        {
            self.strip.link_start = None;
            self.strip_set(sch, Some(m.clone()));
            *l = m;
        }
        // keys: R turns the box (half a turn) or else the picked part, Delete takes the picked link away, Esc lets go
        if response.hovered() {
            if tool == StripTool::Move
                && ui.input(|i| i.key_pressed(egui::Key::R))
                && let Some((a, b)) = self.strip.sel
            {
                let m = area_turned(l, a, b);
                self.strip.sel = Some(turned_box(l, a, b));
                self.strip_set(sch, Some(m.clone()));
                *l = m;
            } else if tool == StripTool::Move
                && ui.input(|i| i.key_pressed(egui::Key::R))
                && let Some(r) = self.picked.clone()
            {
                match part_turned(l, &r) {
                    Some(m) => {
                        self.strip_set(sch, Some(m.clone()));
                        *l = m;
                    }
                    None => sch.status = format!("{r} は、ここでは回せません (基板の上か左にはみ出す向き)"),
                }
            }
            if tool == StripTool::Move
                && self.kind == BoardKind::Perfboard
                && ui.input(|i| i.key_pressed(egui::Key::L))
                && let Some(r) = self.picked.clone().filter(|r| l.parts.iter().any(|p| &p.refdes == r))
            {
                self.strip_lock_toggle(sch, l, &r);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
                && let Some(k) = self.strip.picked_link.take()
            {
                let m = without_link(l, k);
                self.strip_set(sch, Some(m.clone()));
                *l = m;
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.strip.link_start = None;
            self.strip.sel = None;
        }
        // Ctrl+Z undoes a hand edit, Ctrl+Y (or Ctrl+Shift+Z) redoes it; the layout shown follows next frame
        if !ui.ctx().egui_wants_keyboard_input() && self.strip.drag.is_none() {
            let (cmd, shift, z, y) = ui.input(|i| (i.modifiers.command, i.modifiers.shift, i.key_pressed(egui::Key::Z), i.key_pressed(egui::Key::Y)));
            if cmd && (z || y) {
                self.strip_undo(sch, z && !shift);
                ui.ctx().request_repaint();
            }
        }
    }
}

/// A padlock (a locked part, a section's box put by hand) centred at `at`, about `size` high.
fn padlock(painter: &egui::Painter, at: Pos2, size: f32) {
    let s = size.clamp(7.0, 16.0);
    painter.circle_stroke(at + Vec2::new(0.0, -s * 0.15), s * 0.42, Stroke::new((s * 0.22).max(1.2), LOCK));
    painter.rect_filled(Rect::from_center_size(at + Vec2::new(0.0, s * 0.35), Vec2::new(s * 1.1, s * 0.8)), 1.5, LOCK);
}

/// A section's box (corners `a`, `b`) dragged from hole `from` to `now`: its corners there, the top-left kept on the board.
fn box_moved(a: At, b: At, from: At, now: At) -> (At, At) {
    let top = ((a.0 as i32 + now.0 as i32 - from.0 as i32).max(0) as u16, (a.1 as i32 + now.1 as i32 - from.1 as i32).max(0) as u16);
    (top, (b.0 - a.0 + top.0, b.1 - a.1 + top.1))
}

/// The holes across and down a board of `w` × `h` mm (2.54 mm apart).
fn holes_of(w: f32, h: f32) -> (u16, u16) {
    ((w / 2.54).floor().max(1.0) as u16, (h / 2.54).floor().max(1.0) as u16)
}

/// The layout's options for a board of this size (None: one row of chips).
fn strip_options(size: Option<(f32, f32)>) -> StripOptions {
    StripOptions { cols: size.map(|(w, h)| holes_of(w, h).0) }
}
