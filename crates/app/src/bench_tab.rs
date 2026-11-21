//! The bench tab ("実機", candidate B12): the place to open when the board has come back from the maker. On the left the steps of
//! the work on the real board (組み立て → 電源投入 → 測る → 合否 → 修理), in the middle the board's placement drawing, where the
//! step lights up the parts to fit or the points to touch with the meter. The steps gather what was there as separate windows: the
//! first power-on guide (`power_on_guide.rs`, drawn in the step), the spec table's "実測" column (`schematic/spec_table.rs`), the
//! hands-on checklists (`bench_guide.rs`) and the repair lessons (opened from the step). With no board yet, the drawing says so and
//! the steps that need only the schematic (電源投入・測る・合否・修理) still work. Notes: `docs/code-notes/app/bench-tab.md`.

use crate::learn::{spaced, NOTE};
use crate::power_on_guide::PowerOnGuide;
use crate::schematic::SchEditor;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use pcb_core::assembly_sheet::{fitting_order, FitItem};
use pcb_core::{Board, PadShape, Point, Side};
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Step {
    #[default]
    Assemble,
    PowerOn,
    Measure,
    Judge,
    Repair,
}

impl Step {
    pub const ALL: [Step; 5] = [Step::Assemble, Step::PowerOn, Step::Measure, Step::Judge, Step::Repair];

    fn title(self) -> &'static str {
        match self {
            Step::Assemble => "1. 組み立て",
            Step::PowerOn => "2. 電源投入",
            Step::Measure => "3. 測る",
            Step::Judge => "4. 合否",
            Step::Repair => "5. 修理",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Step::Assemble => "部品を背の低い順に付けます。付けた部品に印をつけると、次に付ける部品が基板の絵の上で光ります",
            Step::PowerOn => "電流を制限できる電源で初めて電源を入れ、流れた電流を、回路図から出した正常な範囲と比べます",
            Step::Measure => "テスターで電圧を測る点と、正常ならその点に出る電圧 (DC 動作点) を並べます。点を選ぶと、基板の絵の上でそのパッドが光ります",
            Step::Judge => "測った値をスペック表の「実測」欄に書くと、シミュレーションとの差と、合っているかが出ます",
            Step::Repair => "動かないときに、症状から原因を探す練習と道具です",
        }
    }

    /// The name used on the command line (`--bench-tab measure`).
    pub fn from_key(k: &str) -> Option<Step> {
        Some(match k {
            "assemble" => Step::Assemble,
            "power_on" => Step::PowerOn,
            "measure" => Step::Measure,
            "judge" => Step::Judge,
            "repair" => Step::Repair,
            _ => return None,
        })
    }
}

/// What the tab asks the app to do (it owns the editors and the other windows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BenchAction {
    ToBoard,
    /// Show the schematic with this analysis section open ("" = just the schematic).
    ToSchematic(&'static str),
    /// Start the schematic lesson of this index (`schematic::lesson_ids`).
    Lesson(usize),
    BoardRepairLesson,
    /// Open a hands-on checklist (`bench_guide::GUIDES`).
    Guide(&'static str),
    MeasureGuide,
    /// Save the printable assembly sheet (`BenchTab::save_sheet`).
    SaveSheet,
    /// Save the full-size print to try the parts on (`pcb_core::print_check`, X4).
    SavePrintCheck,
}

#[derive(Default)]
pub struct BenchTab {
    pub step: Step,
    /// Parts ticked as fitted (by reference designator). Kept while the app runs (the printed sheet is the record to keep).
    fitted: BTreeSet<String>,
    /// The part or the net picked in the list or on the drawing: it glows brightest.
    picked_part: Option<String>,
    picked_net: Option<String>,
    /// The order of fitting and the board it was worked out for (its parts' designators and footprints).
    order: Vec<FitItem>,
    order_key: String,
    /// The DC point was worked out once on opening the step "測る" (not again every frame when it fails).
    dc_asked: bool,
    /// The nets that move while the circuit runs ((net, mean, min, max) V, `SchEditor::running_voltages`) and the circuit they were
    /// found for: worked out once per circuit, as it may run a transient.
    running: Option<(String, Vec<(String, f64, f64, f64)>)>,
    /// The readings as typed in the step "測る", by net (a half-typed one stays as typed until it reads).
    typed: std::collections::BTreeMap<String, String>,
}

const GLOW: Color32 = crate::theme::HIGHLIGHT;

fn sch_judge(pred: f64, range: Option<(f64, f64)>, volts: f64, uncert: f64) -> crate::schematic::Agree {
    crate::schematic::judge(pred, range, volts, uncert).1
}
const GROUND: Color32 = crate::theme::INFO;
const FITTED: Color32 = crate::theme::OK;
const PIN1: Color32 = crate::theme::ERROR;

/// Connectors: where the supply and the outside world go on.
fn is_connector(refdes: &str) -> bool {
    let letters: String = refdes.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    matches!(letters.to_ascii_uppercase().as_str(), "J" | "CN" | "P" | "X" | "BT")
}

/// Whether the bench guide `id` is about something on this board: surface-mount soldering when a part has pads without holes, the
/// bootloader when an ATmega is there, the 100-pin soldering when a part has 100 pads or more.
fn guide_fits(id: &str, board: &Board) -> bool {
    let pads_of = |r: &str| board.pads.iter().filter(|p| p.refdes == r).count();
    match id {
        "solder" => board.pads.iter().any(|p| !p.refdes.is_empty() && p.drill.is_none()),
        "bootload" => board.parts.iter().any(|p| p.value.to_ascii_uppercase().starts_with("ATMEGA")),
        "solder_tqfp100" => board.parts.iter().any(|p| pads_of(&p.refdes) >= 100),
        _ => true,
    }
}

fn is_ground(net: &str) -> bool {
    let n = net.to_ascii_uppercase();
    n == "GND" || n == "0" || n == "AGND" || n == "DGND" || n == "VSS"
}

impl BenchTab {
    fn refresh_order(&mut self, board: &Board) {
        let key: String = board.parts.iter().map(|p| format!("{}:{};", p.refdes, p.footprint)).collect();
        if key != self.order_key {
            self.order = fitting_order(board);
            self.order_key = key;
            let names: BTreeSet<&str> = self.order.iter().map(|f| f.refdes.as_str()).collect();
            self.fitted.retain(|r| names.contains(r.as_str()));
        }
    }

    /// The parts to fit next: those not fitted yet whose bodies are about as low as the lowest one left.
    fn next_to_fit(&self) -> Vec<&str> {
        let Some(first) = self.order.iter().find(|f| !self.fitted.contains(&f.refdes)) else { return vec![] };
        self.order.iter().filter(|f| !self.fitted.contains(&f.refdes) && f.height <= first.height + 0.3).map(|f| f.refdes.as_str()).collect()
    }

    fn done(&self, step: Step, sch: &SchEditor, power_on: &PowerOnGuide) -> bool {
        match step {
            Step::Assemble => !self.order.is_empty() && self.order.iter().all(|f| self.fitted.contains(&f.refdes)),
            Step::PowerOn => power_on.all_normal(),
            Step::Measure => sch.spec_rows() > 0,
            Step::Judge => sch.compared_measurements() > 0,
            Step::Repair => false,
        }
    }

    /// Writes the printable assembly sheet (`pcb_core::assembly_sheet`) where the person picks; returns the message for the status line.
    pub fn save_sheet(&self, board: &Board) -> Option<String> {
        let name = format!("{}-assembly.svg", if board.name.is_empty() { "board" } else { &board.name });
        let p = rfd::FileDialog::new().add_filter("SVG", &["svg"]).set_file_name(name).save_file()?;
        let fitted: Vec<String> = self.fitted.iter().cloned().collect();
        Some(match pcb_core::assembly_sheet::assembly_svg(board, &fitted).and_then(|s| std::fs::write(&p, s).map_err(|e| e.to_string())) {
            Ok(()) => format!("組み立て表を保存しました: {} (ブラウザで開いて印刷できます)", p.display()),
            Err(e) => format!("組み立て表を保存できませんでした: {e} (保存先のフォルダに書き込めるか確かめてください)"),
        })
    }

    /// The status line's hint while the tab is open.
    pub fn hint(&self) -> String {
        match self.step {
            Step::Assemble => "部品を背の低い順に付け、付けたら左の一覧に印をつけます。次に付ける部品が黄色く光り、向きのある部品には赤い印が出ます".into(),
            Step::PowerOn => "電流を制限できる電源で、左の「電流制限に入れる値」に合わせてから電源を入れ、落ち着いた電流を入れて比べます".into(),
            Step::Measure => "左の一覧で点を選ぶと、基板の絵の上でそのネットのパッドが光ります。黒い棒は青い GND へ".into(),
            Step::Judge => "測った値を表の「実測」欄に書くと、シミュレーションとの差と、合っているかが出ます".into(),
            Step::Repair => "症状の出ている点から測って、正常な電圧と比べます。練習は、故障を仕込んだ回路で何度でもできます".into(),
        }
    }

    #[cfg(test)]
    pub fn fit_all_for_test(&mut self) {
        self.fitted = self.order.iter().map(|f| f.refdes.clone()).collect();
    }

    /// The left panel: the steps, then what the chosen one needs.
    pub fn panel(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, board: &Board, power_on: &mut PowerOnGuide) -> Option<BenchAction> {
        self.refresh_order(board);
        let mut action = None;
        crate::widgets::section_heading(ui, i18n::tr("実機の手順"));
        for s in Step::ALL {
            let mark = if self.done(s, sch, power_on) { "✔ " } else { "　 " };
            if ui.selectable_label(self.step == s, format!("{mark}{}", i18n::tr(s.title()))).on_hover_text(i18n::tr(s.about())).clicked() && self.step != s {
                self.step = s;
                self.picked_net = None;
                self.dc_asked = false;
            }
        }
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(spaced(self.step.about()).color(NOTE));
            ui.add_space(4.0);
            match self.step {
                Step::Assemble => self.assemble_ui(ui, board, &mut action),
                Step::PowerOn => {
                    power_on.ensure_run(sch);
                    ui.label(spaced("基板の絵では、電源をつなぐ所 (コネクタ・電源の部品) が光ります。").color(NOTE));
                    power_on.contents(ui, sch);
                }
                Step::Measure => self.measure_ui(ui, sch, &mut action),
                Step::Judge => {
                    if sch.spec_rows() == 0 {
                        ui.label(spaced("スペック表に行がまだありません。「3. 測る」の「測る項目を表に入れる」で入れるか、回路図の解析タブの「スペック」で足します。").color(NOTE));
                    }
                    if ui.button(i18n::tr("DC 動作点を計算し直す")).on_hover_text(i18n::tr("表の「シミュレーション」の値は、回路図の解析の結果から読みます。回路図を変えたあとは、計算し直してください")).clicked() {
                        sch.analyze_dc();
                    }
                    sch.spec_table_ui(ui);
                }
                Step::Repair => repair_ui(ui, &mut action),
            }
        });
        action
    }

    fn assemble_ui(&mut self, ui: &mut egui::Ui, board: &Board, action: &mut Option<BenchAction>) {
        if self.order.is_empty() {
            ui.label(spaced("基板にまだ部品がありません。基板の画面で「回路図から取り込み」をして、配置すると、ここに付ける順の部品の一覧が出ます。").color(NOTE));
            if ui.button(i18n::tr("基板の画面へ")).on_hover_text(i18n::tr("基板パターンの画面に切り替えます")).clicked() {
                *action = Some(BenchAction::ToBoard);
            }
        } else {
            let n = self.order.iter().filter(|f| self.fitted.contains(&f.refdes)).count();
            ui.add(egui::ProgressBar::new(n as f32 / self.order.len() as f32).text(i18n::trf!("付けた部品 {n} / {}", self.order.len())));
            if ui.button(i18n::tr("組み立て表を印刷用に保存 (SVG)…")).on_hover_text(i18n::tr("基板の配置図 (向きの印つき) と、付ける順の部品の一覧 (□ に印をつけながら進める) を、A4 の幅の 1 枚の図 (SVG) にします。ブラウザで開いて印刷するか、印刷から PDF にできます。いま印をつけた部品は、印がついた形で入ります")).clicked() {
                *action = Some(BenchAction::SaveSheet);
            }
            if ui.button(i18n::tr("原寸の確かめ用の図を保存 (SVG)…")).on_hover_text(i18n::tr("パッドと穴を原寸で描いた図 (50 mm の目盛りつき) を保存します。「実際のサイズ」(100 %) で印刷し、実物の部品を当てて足とパッドが合うかを見ます。発注の前に、届いた部品で確かめるのがいちばんです")).clicked() {
                *action = Some(BenchAction::SavePrintCheck);
            }
            if board.parts.iter().any(|p| p.side == Side::Bottom) {
                ui.label(egui::RichText::new(i18n::tr("「裏」の部品は、基板を裏返して付けます (絵では薄く描きます)")).color(NOTE));
            }
            let next = self.next_to_fit();
            let next: BTreeSet<String> = next.into_iter().map(String::from).collect();
            egui::Grid::new("bench_fit").num_columns(4).striped(true).spacing([8.0, 2.0]).show(ui, |ui| {
                for f in &self.order {
                    let mut on = self.fitted.contains(&f.refdes);
                    if ui.checkbox(&mut on, "").on_hover_text(i18n::tr("付け終えたら印をつけます。次に付ける部品が光ります")).changed() {
                        if on {
                            self.fitted.insert(f.refdes.clone());
                        } else {
                            self.fitted.remove(&f.refdes);
                        }
                    }
                    let mut name = egui::RichText::new(&f.refdes).strong();
                    if next.contains(&f.refdes) {
                        name = name.color(GLOW);
                    }
                    let picked = self.picked_part.as_deref() == Some(f.refdes.as_str());
                    if ui.selectable_label(picked, name).on_hover_text(i18n::tr("選ぶと、基板の絵の上でこの部品が光ります")).clicked() {
                        self.picked_part = if picked { None } else { Some(f.refdes.clone()) };
                    }
                    ui.label(&f.value).on_hover_text(&f.footprint);
                    let mut notes = vec![];
                    if let Some((m, _)) = f.mark {
                        notes.push(i18n::trf!("向き: {}", i18n::tr(m.label())));
                    }
                    if f.bottom {
                        notes.push(i18n::tr("裏").to_string());
                    }
                    if f.height > 0.0 {
                        notes.push(format!("{:.1} mm", f.height));
                    }
                    let l = ui.label(egui::RichText::new(notes.join("・")).color(NOTE));
                    if f.mark.is_some() {
                        l.on_hover_text(i18n::tr("向きのある部品です。基板の絵の赤い印 (1 = 1 番ピン、K = カソード、+ = 電解コンデンサの + 側) に合わせて付けます。データシートと基板のシルクの印でも確かめます"));
                    }
                    ui.end_row();
                }
            });
        }
        ui.separator();
        ui.label(egui::RichText::new(i18n::tr("はんだ付けの手順とチェック表")).strong());
        // only the guides for what is on this board (a 555 blinker was offered the ATmega2560's 100 pins and its bootloader)
        let fits: Vec<_> = crate::bench_guide::GUIDES.iter().filter(|g| guide_fits(g.id, board)).collect();
        if fits.is_empty() {
            ui.label(spaced("この基板に合う手順書はまだありません。リード部品は、背の低い順に足を穴に通し、裏からはんだ付けして、余った足を切ります。向きのある部品は、付ける前に向きの印を確かめます。").color(NOTE));
        }
        for g in fits {
            if ui.button(i18n::tr(g.title)).on_hover_text(i18n::tr(g.intro)).clicked() {
                *action = Some(BenchAction::Guide(g.id));
            }
        }
    }

    fn measure_ui(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, action: &mut Option<BenchAction>) {
        ui.label(spaced("テスターを DC 電圧のレンジにして、黒い棒を GND (絵では青) に、赤い棒を測る点に当てます。").color(NOTE));
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("測る項目を表に入れる")).on_hover_text(i18n::tr("回路図のスペック表に、入力電圧・出力電圧・電流の行を足して、DC 動作点を計算します。回路図に VIN・VOUT という名前があると、自動で選ばれます。測った値は「4. 合否」で書き込みます")).clicked() {
                sch.add_measure_rows();
            }
            if ui.button(i18n::tr("DC 動作点を計算する")).on_hover_text(i18n::tr("いまの回路図で、正常ならそれぞれの点に出る電圧を計算します")).clicked() {
                sch.analyze_dc();
            }
        });
        let (spec_nets, _) = sch.spec_points();
        if sch.bench_voltages().is_none() && !self.dc_asked {
            self.dc_asked = true;
            sch.analyze_dc();
        }
        match sch.bench_voltages() {
            None => {
                ui.label(spaced("正常なら出る電圧は、DC 動作点から出します。「DC 動作点を計算する」を押してください。").color(NOTE));
            }
            Some(mut v) => {
                // the nets that move while it runs: their DC voltage is not what the board shows (a 555's timing capacitor
                // swings between 1/3 and 2/3 of the supply; its DC point sits at 1/3 with the output low)
                let sig = sch.circuit_signature();
                if self.running.as_ref().is_none_or(|(s, _)| *s != sig) {
                    self.running = Some((sig, sch.running_voltages()));
                }
                let running = self.running.as_ref().map(|(_, r)| r.as_slice()).unwrap_or_default();
                if !running.is_empty() {
                    ui.label(spaced("この回路は動き続けるので、時間とともに変わる点は、過渡解析の後半の平均と、変わる幅を出しています。テスターは平均に近い値を示し、数字が揺れることがあります。").color(NOTE));
                }
                // ground is where the black probe goes, not a reading
                v.retain(|(n, _)| !is_ground(n));
                // the rows of the spec table first, then the named nets, then the rest
                let rank = |n: &str| (if spec_nets.iter().any(|s| s.eq_ignore_ascii_case(n)) { 0 } else if n.starts_with("N$") || n.starts_with("net") { 2 } else { 1 }, n.to_string());
                v.sort_by_key(|(n, _)| rank(n));
                let readings = v.iter().filter(|(n, _)| sch.dc_reading(n).is_some()).count();
                ui.label(spaced("測った値を「実測」に入れると、予測との差と、合っているか (✔ 合う・▲ ずれ・✖ 大きくずれ) が出ます。テスターの不確かさは「4.98 ±0.02」のように書けます。入れた値は回路図と一緒に保存されます。").color(NOTE));
                if readings > 0 {
                    let good = v.iter().filter(|(n, p)| {
                        let range = running.iter().find(|(m, ..)| m == n).map(|(_, mean, lo, hi)| (*mean, Some((*lo, *hi))));
                        let (pred, rng) = range.unwrap_or((*p, None));
                        sch.dc_reading(n).is_some_and(|r| sch_judge(pred, rng, r.volts, r.uncert) == crate::schematic::Agree::Good)
                    });
                    ui.label(egui::RichText::new(i18n::trf!("{} 点を測りました。合っているのは {} 点", readings, good.count())).strong());
                }
                let mut set: Vec<(String, Option<(f64, f64)>)> = vec![];
                egui::Grid::new("bench_points").num_columns(5).striped(true).spacing([12.0, 2.0]).show(ui, |ui| {
                    ui.label(egui::RichText::new(i18n::tr("ネット")).strong());
                    ui.label(egui::RichText::new(i18n::tr("予測")).strong());
                    ui.label(egui::RichText::new(i18n::tr("実測")).strong()).on_hover_text(i18n::tr("テスターで測った電圧 (V)。4.98 や 4.98 ±0.02 のように"));
                    ui.label(egui::RichText::new(i18n::tr("差")).strong());
                    ui.label("");
                    ui.end_row();
                    for (net, volts) in &v {
                        let picked = self.picked_net.as_deref().is_some_and(|p| p.eq_ignore_ascii_case(net));
                        let mut name = egui::RichText::new(net.as_str());
                        if spec_nets.iter().any(|s| s.eq_ignore_ascii_case(net)) {
                            name = name.strong();
                        }
                        if ui.selectable_label(picked, name).on_hover_text(i18n::tr("選ぶと、基板の絵の上で、このネットのパッドが光ります。太字はスペック表で測る点です")).clicked() {
                            self.picked_net = if picked { None } else { Some(net.clone()) };
                        }
                        match running.iter().find(|(n, ..)| n == net) {
                            Some((_, mean, lo, hi)) => {
                                ui.label(i18n::trf!("平均 {}V ({}V 〜 {}V で変わる)", sim_core::format_value(*mean), sim_core::format_value(*lo), sim_core::format_value(*hi)))
                                    .on_hover_text(i18n::trf!("過渡解析の後半から出した値です。DC 動作点の {}V は、動きが止まった状態の値で、実物では出ません", sim_core::format_value(*volts)));
                            }
                            None => {
                                ui.label(format!("{}V", sim_core::format_value(*volts)));
                            }
                        }
                        // the reading, the difference and whether it agrees
                        let (pred, range) = running.iter().find(|(n, ..)| n == net).map_or((*volts, None), |(_, mean, lo, hi)| (*mean, Some((*lo, *hi))));
                        let reading = sch.dc_reading(net).cloned();
                        let text = self.typed.entry(net.clone()).or_insert_with(|| reading.as_ref().map_or(String::new(), |r| if r.uncert > 0.0 { format!("{} ±{}", sim_core::format_value(r.volts), sim_core::format_value(r.uncert)) } else { sim_core::format_value(r.volts) }));
                        let edit = ui.add(egui::TextEdit::singleline(text).desired_width(120.0).hint_text("—"));
                        if edit.changed() {
                            match text.trim() {
                                "" => set.push((net.clone(), None)),
                                t => {
                                    if let Some(r) = crate::schematic::parse_reading(t) {
                                        set.push((net.clone(), Some(r)));
                                    }
                                }
                            }
                        }
                        let bad_text = !text.trim().is_empty() && crate::schematic::parse_reading(text).is_none();
                        match reading.filter(|_| !bad_text) {
                            Some(r) => {
                                let (diff, agree) = crate::schematic::judge(pred, range, r.volts, r.uncert);
                                ui.label(format!("{}{}V", if diff >= 0.0 { "+" } else { "" }, sim_core::format_value(diff)));
                                let (mark, color, tip) = match agree {
                                    crate::schematic::Agree::Good => ("✔", crate::theme::OK, "予測と合っています (部品の誤差・テスターの誤差の内)"),
                                    crate::schematic::Agree::Off => ("▲", crate::theme::WARN, "少しずれています。測る点・テスターのレンジ・部品の値を確かめましょう"),
                                    crate::schematic::Agree::Far => ("✖", crate::theme::ERROR, "大きくずれています。部品の付け間違い・はんだ付け・向き・電源を確かめます (「5. 修理」で原因を探せます)"),
                                };
                                ui.label(egui::RichText::new(mark).color(color)).on_hover_text(i18n::trf!("{} (許す幅 ±{}V)", i18n::tr(tip), sim_core::format_value(crate::schematic::allowance(pred, r.uncert))));
                            }
                            None if bad_text => {
                                ui.colored_label(crate::theme::WARN, i18n::tr("読めません"));
                                ui.label("");
                            }
                            None => {
                                ui.label("");
                                ui.label("");
                            }
                        }
                        ui.end_row();
                    }
                });
                for (net, r) in set {
                    sch.set_dc_reading(&net, r);
                }
            }
        }
        ui.separator();
        if ui.button(i18n::tr("学習パスの「組み立てて、性能を測る」…")).on_hover_text(i18n::tr("自分で設計した電源を作って、シミュレーションとの差を調べる、学習パスの手順の窓を開きます")).clicked() {
            *action = Some(BenchAction::MeasureGuide);
        }
        if ui.button(i18n::tr("オシロの波形を予測に重ねる (回路図の過渡解析)…")).on_hover_text(i18n::tr("オシロで記録した CSV を、回路図の過渡解析の波形に重ねて比べます。予測の波形がある回路図の画面に移ります")).clicked() {
            *action = Some(BenchAction::ToSchematic("tran"));
        }
    }

    /// The placement drawing, with what the step points at lit up.
    pub fn canvas(&mut self, ui: &mut egui::Ui, board: &Board, sch: &SchEditor, power_on: &PowerOnGuide) -> Option<BenchAction> {
        self.refresh_order(board);
        let mut action = None;
        let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, crate::canvas_theme::col(crate::canvas_theme::Role::Bg));
        // the extent: the outline, or the pads when there is none
        let mut pts: Vec<Point> = board.outline.clone();
        if pts.is_empty() {
            pts = board.pads.iter().map(|p| p.pos).collect();
        }
        if board.parts.is_empty() || pts.is_empty() {
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(40.0)), |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(rect.height() * 0.3);
                    ui.heading(i18n::tr("基板がまだありません"));
                    ui.label(i18n::tr("基板の画面で、回路図から部品を取り込んで配置すると、ここに基板の配置図が出て、手順ごとに付ける部品や測る点が光ります。"));
                    ui.label(i18n::tr("電源投入・測る・合否・修理の手順は、回路図だけでも使えます (ブレッドボードで組んだときなど)。"));
                    if ui.button(i18n::tr("基板の画面へ")).clicked() {
                        action = Some(BenchAction::ToBoard);
                    }
                });
            });
            return action;
        }
        let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
        for p in &pts {
            (lo.x, lo.y, hi.x, hi.y) = (lo.x.min(p.x), lo.y.min(p.y), hi.x.max(p.x), hi.y.max(p.y));
        }
        let inner = rect.shrink(36.0);
        let scale = (inner.width() as f64 / (hi.x - lo.x).max(1.0)).min(inner.height() as f64 / (hi.y - lo.y).max(1.0)) as f32;
        let mid = Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        let c = inner.center();
        let to = |p: Point| Pos2::new(c.x + (p.x - mid.x) as f32 * scale, c.y - (p.y - mid.y) as f32 * scale);
        if board.outline.len() >= 3 {
            let poly: Vec<Pos2> = board.outline.iter().map(|p| to(*p)).collect();
            painter.add(egui::Shape::convex_polygon(poly.clone(), Color32::from_rgb(28, 64, 44), Stroke::NONE));
            painter.add(egui::Shape::closed_line(poly, Stroke::new(1.5, Color32::from_gray(200))));
        }
        // what the step lights up
        let (spec_nets, spec_parts) = sch.spec_points();
        let supplies = power_on.supply_names();
        let next: BTreeSet<String> = if self.step == Step::Assemble { self.next_to_fit().into_iter().map(String::from).collect() } else { BTreeSet::new() };
        let lit_part = |r: &str| -> bool {
            match self.step {
                Step::Assemble => next.contains(r),
                Step::PowerOn => is_connector(r) || supplies.iter().any(|s| s == r),
                Step::Measure | Step::Judge => spec_parts.iter().any(|p| p == r),
                Step::Repair => false,
            }
        };
        let lit_net = |n: &str| -> bool {
            !n.is_empty()
                && match self.step {
                    Step::Measure | Step::Judge => self.picked_net.as_deref().map_or_else(|| spec_nets.iter().any(|s| s.eq_ignore_ascii_case(n)), |p| p.eq_ignore_ascii_case(n)),
                    _ => false,
                }
        };
        let probe_ground = matches!(self.step, Step::Measure | Step::Judge);
        // the parts: a box round the body (or the pads), the designator in it
        let mut boxes: Vec<(usize, Rect)> = vec![];
        for (i, part) in board.parts.iter().enumerate() {
            let (mut plo, mut phi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
            for pad in board.pads.iter().filter(|p| p.refdes == part.refdes) {
                let (w, h) = pad.shape.size();
                (plo.x, plo.y, phi.x, phi.y) = (plo.x.min(pad.pos.x - w / 2.0), plo.y.min(pad.pos.y - h / 2.0), phi.x.max(pad.pos.x + w / 2.0), phi.y.max(pad.pos.y + h / 2.0));
            }
            if let Some((a, b)) = pcb_core::model3d::body_extent(board, part) {
                (plo.x, plo.y, phi.x, phi.y) = (plo.x.min(a.x), plo.y.min(a.y), phi.x.max(b.x), phi.y.max(b.y));
            }
            if plo.x > phi.x {
                continue;
            }
            let r = Rect::from_two_pos(to(Point::new(plo.x - 0.3, plo.y - 0.3)), to(Point::new(phi.x + 0.3, phi.y + 0.3)));
            boxes.push((i, r));
        }
        let hovered = response.hover_pos().and_then(|h| boxes.iter().filter(|(_, r)| r.contains(h)).min_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(i, _)| *i));
        for (i, r) in &boxes {
            let part = &board.parts[*i];
            let bottom = part.side == Side::Bottom;
            let fitted = self.step == Step::Assemble && self.fitted.contains(&part.refdes);
            let picked = self.picked_part.as_deref() == Some(part.refdes.as_str());
            let lit = lit_part(&part.refdes);
            let alpha = if bottom { 0.45 } else { 1.0 };
            let fill = if picked {
                GLOW.gamma_multiply(0.55)
            } else if lit {
                GLOW.gamma_multiply(0.3)
            } else if fitted {
                FITTED.gamma_multiply(0.25)
            } else {
                Color32::from_black_alpha(90)
            };
            let stroke = if picked || lit { Stroke::new(2.0, GLOW) } else if Some(*i) == hovered { Stroke::new(1.5, Color32::WHITE) } else { Stroke::new(1.0, Color32::from_gray(150).gamma_multiply(alpha)) };
            painter.rect(*r, 2.0, fill.gamma_multiply(alpha), stroke, egui::StrokeKind::Middle);
        }
        // the pads: grey, the measuring points lit, the ground (the black probe) blue
        for pad in &board.pads {
            let p = to(pad.pos);
            let (w, h) = pad.shape.size();
            let size = egui::vec2((w as f32 * scale).max(2.0), (h as f32 * scale).max(2.0));
            let bottom_only = pad.drill.is_none() && pad.side == Side::Bottom;
            let col = if lit_net(&pad.net) {
                GLOW
            } else if probe_ground && is_ground(&pad.net) {
                GROUND
            } else {
                Color32::from_gray(if bottom_only { 90 } else { 170 })
            };
            match pad.shape {
                PadShape::Circle { .. } => {
                    painter.circle_filled(p, size.x / 2.0, col);
                }
                _ => {
                    painter.rect_filled(Rect::from_center_size(p, size), 1.0, col);
                }
            }
            if lit_net(&pad.net) {
                painter.circle_stroke(p, size.x.max(size.y) / 2.0 + 4.0, Stroke::new(2.0, GLOW));
            }
        }
        // pin 1 of the parts that go on one way only, while fitting
        if self.step == Step::Assemble {
            let font = egui::FontId::proportional(12.0);
            for (m, at) in self.order.iter().filter_map(|f| f.mark) {
                let p = to(at);
                painter.circle_stroke(p, 5.0, Stroke::new(2.0, PIN1));
                painter.text(p + egui::vec2(7.0, -7.0), egui::Align2::LEFT_BOTTOM, m.letter(), font.clone(), PIN1);
            }
        }
        // the designators on top
        let font = egui::FontId::proportional((scale * 1.2).clamp(9.0, 15.0));
        for (i, r) in &boxes {
            let part = &board.parts[*i];
            let col = if part.side == Side::Bottom { Color32::from_gray(150) } else { Color32::WHITE };
            painter.text(r.center(), egui::Align2::CENTER_CENTER, &part.refdes, font.clone(), col);
        }
        // the legend of this step
        let legend = match self.step {
            Step::Assemble => "黄: 次に付ける部品・選んだ部品　緑: 付けた部品　赤: 向きの印 (1 = 1 番ピン、K = カソード、+ = 電解コンデンサの + 側)",
            Step::PowerOn => "黄: 電源をつなぐ所 (コネクタ・電源の部品)",
            Step::Measure | Step::Judge => "黄: 測る点 (赤い棒)　青: GND (黒い棒)",
            Step::Repair => "症状から原因を探すのは、回路図の上で行います",
        };
        painter.text(rect.left_top() + egui::vec2(12.0, 10.0), egui::Align2::LEFT_TOP, i18n::tr(legend), egui::FontId::proportional(13.0), Color32::from_gray(210));
        if let Some(i) = hovered {
            let p = &board.parts[i];
            let side = if p.side == Side::Bottom { i18n::tr(" (裏)").to_string() } else { String::new() };
            response.clone().on_hover_text(format!("{} {}{side}\n{}", p.refdes, p.value, p.footprint));
            if response.clicked() {
                self.picked_part = if self.picked_part.as_deref() == Some(p.refdes.as_str()) { None } else { Some(p.refdes.clone()) };
            }
        }
        action
    }
}

fn repair_ui(ui: &mut egui::Ui, action: &mut Option<BenchAction>) {
    ui.label(spaced("基板が動かないときは、症状の出ている点から測り、正常な電圧と比べて、原因の部品を絞り込みます。回路図の「症状から原因を探す」は、測った電圧から、怪しい部品の順を出します。").color(NOTE));
    if ui.button(i18n::tr("症状から原因を探す (回路図の解析)…")).on_hover_text(i18n::tr("いまの回路図で、測った電圧を入れると、どの部品の故障が一番ありそうかと、次に測るとよい点を出します")).clicked() {
        *action = Some(BenchAction::ToSchematic("diagnose"));
    }
    ui.separator();
    ui.label(egui::RichText::new(i18n::tr("修理の練習")).strong());
    let ids = crate::schematic::lesson_ids();
    let titles = crate::schematic::lesson_titles();
    for (i, id) in ids.iter().enumerate().filter(|(_, id)| id.starts_with("repair_")) {
        let _ = id;
        if ui.button(i18n::tr(titles[i])).on_hover_text(i18n::tr("故障が 1 つ仕込まれた回路を、テスターで測って、原因を見つけます (サンプルの回路が開きます)")).clicked() {
            *action = Some(BenchAction::Lesson(i));
        }
    }
    if ui.button(i18n::tr(crate::board_editor::lesson::REPAIR_TITLE)).on_hover_text(i18n::tr("ちゃんと動く基板に、故障を 1 つ仕込みます。症状から原因を探して、点検の道具で確かめ、直します")).clicked() {
        *action = Some(BenchAction::BoardRepairLesson);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, Mode};

    fn frame(ctx: &egui::Context, app: &mut App) {
        let mut f = eframe::Frame::_new_kittest();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    }

    #[test]
    fn every_step_draws_with_and_without_a_board() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("power_on.schjson"));
        app.mode = Mode::Bench;
        let ctx = egui::Context::default();
        // no board yet: every step still draws (the drawing says there is no board)
        for s in Step::ALL {
            app.bench_tab.step = s;
            frame(&ctx, &mut app);
        }
        app.board_ed.import_from_schematic(&app.sch);
        for s in Step::ALL {
            app.bench_tab.step = s;
            frame(&ctx, &mut app);
        }
        assert!(app.mode == Mode::Bench, "drawing the steps does not switch the tab");
        assert!(app.power_on.result_for_test().is_some(), "the power-on step works the currents out by itself");
        assert!(!app.power_on.open, "and does not open the window as well");
    }

    // Found by a usability test (2026-11-19): a 555 blinker's assembly step offered the ATmega2560's 100-pin soldering and its
    // bootloader.
    /// The step "測る" with readings entered (L2): they are kept with the schematic (and through a save and a load), judged against
    /// the prediction, and the step draws with them.
    #[test]
    fn readings_are_kept_and_judged() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("power_on.schjson"));
        app.mode = Mode::Bench;
        app.bench_tab.step = Step::Measure;
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let five = app.sch.bench_voltages().unwrap().into_iter().find(|(n, _)| n == "5V").unwrap().1;
        app.sch.set_dc_reading("5V", Some((five - 0.03, 0.01)));
        app.sch.set_dc_reading("VIN", Some((3.0, 0.0)));
        frame(&ctx, &mut app);
        let r = app.sch.dc_reading("5V").unwrap().clone();
        assert_eq!(crate::schematic::judge(five, None, r.volts, r.uncert).1, crate::schematic::Agree::Good);
        // 9 V predicted at VIN, 3 V read: far off
        let vin = app.sch.bench_voltages().unwrap().into_iter().find(|(n, _)| n == "VIN").unwrap().1;
        assert_eq!(crate::schematic::judge(vin, None, 3.0, 0.0).1, crate::schematic::Agree::Far);
        // through the file
        let dir = std::env::temp_dir().join(format!("ccad_dc_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("measured.schjson");
        std::fs::write(&path, app.sch.sheet_json_for_test()).unwrap();
        let mut back = SchEditor::default();
        back.load_path(path.clone());
        assert_eq!(back.dc_reading("5V").map(|r| r.volts), Some(five - 0.03));
        assert_eq!(back.dc_reading("VIN").map(|r| r.uncert), Some(0.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_guides_for_what_is_on_the_board_are_offered() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let board_of = |name: &str| {
            let mut sch = SchEditor::default();
            sch.load_path(root.join(name));
            let mut ed = crate::board_editor::BoardEditor::default();
            ed.import_from_schematic(&sch);
            ed.board
        };
        let fits = |b: &Board| crate::bench_guide::GUIDES.iter().filter(|g| guide_fits(g.id, b)).map(|g| g.id).collect::<Vec<_>>();
        // the 555 blinker: chip parts (0805) and a DIP-8, no ATmega
        assert_eq!(fits(&board_of("ne555_blinker.schjson")), vec!["solder"]);
        // the minimal Arduino: an ATmega328P (32 pins)
        let arduino = fits(&board_of("arduino_minimal.schjson"));
        assert!(arduino.contains(&"bootload") && !arduino.contains(&"solder_tqfp100"), "{arduino:?}");
    }

    #[test]
    fn the_lowest_parts_come_first_and_ticking_them_moves_on() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut sch = SchEditor::default();
        sch.load_path(root.join("power_on.schjson"));
        let mut ed = crate::board_editor::BoardEditor::default();
        ed.import_from_schematic(&sch);
        let mut t = BenchTab::default();
        t.refresh_order(&ed.board);
        assert!(!t.order.is_empty());
        assert!(t.order.windows(2).all(|w| w[0].height <= w[1].height), "{:?}", t.order.iter().map(|f| (&f.refdes, f.height)).collect::<Vec<_>>());
        let first = t.next_to_fit();
        assert!(!first.is_empty());
        assert!(t.order.iter().filter(|f| f.refdes.starts_with('D')).all(|f| f.mark.is_some()), "diodes and LEDs are marked as going on one way");
        let first: Vec<String> = first.into_iter().map(String::from).collect();
        t.fitted.extend(first.iter().cloned());
        assert!(t.next_to_fit().iter().all(|r| !first.iter().any(|f| f == r)), "the fitted ones are not asked for again");
        t.fit_all_for_test();
        assert!(t.next_to_fit().is_empty());
    }

    #[test]
    fn step_keys_round_trip() {
        for (k, s) in [("assemble", Step::Assemble), ("power_on", Step::PowerOn), ("measure", Step::Measure), ("judge", Step::Judge), ("repair", Step::Repair)] {
            assert_eq!(Step::from_key(k), Some(s));
        }
        assert_eq!(Step::from_key("x"), None);
        assert!(is_connector("J1") && is_connector("CN2") && !is_connector("JP1") && !is_connector("R1"));
    }
}
