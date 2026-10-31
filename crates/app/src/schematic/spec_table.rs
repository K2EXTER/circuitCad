//! "スペック (合否)" section: targets for the design, checked against the analyses on screen.

use super::*;
use sim_core::{compare_measured, evaluate_spec, evaluate_worst_spec, measure_spec, Agreement, Spec, SpecEval, SpecInputs, SpecKind};

/// One row of the table: the spec plus the text boxes it is typed in (kept so typing is not overwritten).
pub(super) struct SpecRow {
    spec: Spec,
    lo: String,
    hi: String,
    at: String,
    /// What the learner measured on the bench (typed text; empty = not measured).
    measured: String,
}

impl SpecRow {
    fn new(kind: SpecKind, target: &str) -> Self {
        let (lo, hi, at) = match kind {
            SpecKind::DcVoltage | SpecKind::DcVoltageWorst | SpecKind::TranMean => ("4.75", "5.25", ""),
            SpecKind::TranPp => ("", "50m", ""),
            SpecKind::TranMax => ("", "5.5", ""),
            SpecKind::TranMin => ("4.5", "", ""),
            SpecKind::DcCurrent => ("", "20m", ""),
            SpecKind::AcCutoff | SpecKind::AcCutoffWorst => ("900", "1.1k", ""),
            SpecKind::AcGainAt => ("-23", "-17", "10k"),
            SpecKind::PhaseMargin => ("45", "", ""),
        };
        Self { spec: Spec { kind, target: target.into(), at: 0.0, lo: None, hi: None }, lo: lo.into(), hi: hi.into(), at: at.into(), measured: String::new() }
    }

    /// Copies the typed numbers into the spec; an empty or unreadable box means "no limit".
    fn sync(&mut self) {
        self.spec.lo = parse_value(&self.lo);
        self.spec.hi = parse_value(&self.hi);
        self.spec.at = parse_value(&self.at).unwrap_or(0.0);
    }
}

/// A spec as stored in a sheet file: the texts the learner typed, so they come back exactly as written.
#[derive(Clone, Debug, PartialEq)]
pub struct SpecItem {
    pub kind: String,
    pub target: String,
    pub lo: String,
    pub hi: String,
    pub at: String,
    /// The bench measurement, as typed (empty when none).
    pub measured: String,
}

impl SpecItem {
    fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({ "kind": self.kind, "target": self.target, "lo": self.lo, "hi": self.hi, "at": self.at });
        // older versions do not know the key: write it only when there is a measurement
        if !self.measured.is_empty() {
            v["measured"] = self.measured.clone().into();
        }
        v
    }

    fn from_json(v: &serde_json::Value) -> Option<SpecItem> {
        let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let kind = text("kind");
        SpecKind::from_key(&kind)?;
        Some(SpecItem { kind, target: text("target"), lo: text("lo"), hi: text("hi"), at: text("at"), measured: text("measured") })
    }
}

/// Adds the specs to a saved sheet (`schematic` JSON object) under `"specs"`; nothing is added when there are none.
pub fn with_specs(sheet: &mut serde_json::Value, items: &[SpecItem]) {
    if let (Some(obj), false) = (sheet.as_object_mut(), items.is_empty()) {
        obj.insert("specs".into(), items.iter().map(SpecItem::to_json).collect());
    }
}

/// Takes the specs out of a saved sheet. A missing key (an older file) or an unknown kind just gives fewer items.
pub fn take_specs(sheet: &mut serde_json::Value) -> Vec<SpecItem> {
    let taken = sheet.as_object_mut().and_then(|o| o.remove("specs"));
    taken.and_then(|v| v.as_array().map(|a| a.iter().filter_map(SpecItem::from_json).collect())).unwrap_or_default()
}

const PASS: Color32 = Color32::from_rgb(90, 190, 110);
const FAIL: Color32 = Color32::from_rgb(230, 70, 60);

impl SchEditor {
    /// The table as it is saved with the sheet.
    pub fn spec_items(&self) -> Vec<SpecItem> {
        self.specs
            .iter()
            .map(|r| SpecItem { kind: r.spec.kind.key().into(), target: r.spec.target.clone(), lo: r.lo.clone(), hi: r.hi.clone(), at: r.at.clone(), measured: r.measured.clone() })
            .collect()
    }

    /// Replaces the table by the one read from a file; it counts as saved.
    pub fn set_spec_items(&mut self, items: &[SpecItem]) {
        self.specs = items
            .iter()
            .filter_map(|it| {
                let mut row = SpecRow::new(SpecKind::from_key(&it.kind)?, &it.target);
                (row.lo, row.hi, row.at) = (it.lo.clone(), it.hi.clone(), it.at.clone());
                row.measured = it.measured.clone();
                row.sync();
                Some(row)
            })
            .collect();
        self.saved_specs = self.spec_items();
    }

    /// Adds a target from the command line (`--spec <kind index> <target> <lo|-> <hi|->`).
    pub fn add_spec(&mut self, kind: usize, target: &str, lo: &str, hi: &str, at: &str) {
        let Some(&kind) = SpecKind::ALL.get(kind) else { return };
        let dash = |s: &str| if s == "-" { String::new() } else { s.to_string() };
        let mut row = SpecRow::new(kind, target);
        (row.lo, row.hi, row.at) = (dash(lo), dash(hi), dash(at));
        self.specs.push(row);
        // show the analysis tab with every section open, so the table is visible
        self.panel_tab = PanelTab::Analyze;
        self.sec_mode = 1;
        self.sec_gen += 1;
    }

    /// Puts the rows of the "組み立てて、性能を測る" step into the table (input voltage, output voltage, a current to
    /// pick), shows the analysis tab and calculates the DC point so the simulated values are there to compare with.
    /// Rows that already exist are not added twice. Returns how many rows were added.
    pub fn add_measure_rows(&mut self) -> usize {
        let nets: Vec<String> = extract(&self.sch).iter().map(|n| n.name.clone()).collect();
        let find = |names: &[&str]| names.iter().find_map(|w| nets.iter().find(|n| n.eq_ignore_ascii_case(w)).cloned()).unwrap_or_default();
        let wanted = [
            (SpecKind::DcVoltage, find(&["VIN", "VCC", "V+"]), "", ""),
            (SpecKind::DcVoltage, find(&["VOUT", "OUT"]), "4.75", "5.25"),
            (SpecKind::DcCurrent, String::new(), "", ""),
        ];
        let mut added = 0;
        for (kind, target, lo, hi) in wanted {
            // a row that is already there (same kind and target, and a named target) is kept as it is
            if !target.is_empty() && self.specs.iter().any(|r| r.spec.kind == kind && r.spec.target.eq_ignore_ascii_case(&target)) {
                continue;
            }
            if target.is_empty() && kind == SpecKind::DcCurrent && self.specs.iter().any(|r| r.spec.kind == kind && r.measured.trim().is_empty()) {
                continue;
            }
            let mut row = SpecRow::new(kind, &target);
            (row.lo, row.hi) = (lo.into(), hi.into());
            self.specs.push(row);
            added += 1;
        }
        self.panel_tab = PanelTab::Analyze;
        self.sec_mode = 1;
        self.sec_gen += 1;
        self.analyze_dc();
        added
    }

    /// How many rows have a bench measurement that could be compared with a simulated value right now.
    pub fn compared_measurements(&self) -> usize {
        let dc = self.dc.as_ref().and_then(|v| v.outcome.as_ref().ok());
        let tran = self.tran.as_ref().and_then(|v| v.outcome.as_ref().ok());
        let ac = self.ac.as_ref().and_then(|v| v.outcome.as_ref().ok());
        let inputs = SpecInputs { dc, tran, ac };
        self.specs
            .iter()
            .filter(|r| parse_value(&r.measured).is_some())
            .filter(|r| {
                let mut spec = r.spec.clone();
                spec.at = parse_value(&r.at).unwrap_or(0.0);
                measure_spec(&spec, &inputs).is_ok()
            })
            .count()
    }

    /// Types a bench measurement into a row (command line `--measured`, for screenshots).
    pub fn set_spec_measured(&mut self, row: usize, text: &str) {
        if let Some(r) = self.specs.get_mut(row) {
            r.measured = text.into();
        }
    }

    /// The worst case behind a worst-case row: the tolerances of the variation box, on the circuit as drawn. Kept until the circuit,
    /// the tolerances or the node change (it is a few dozen solutions, not something for every frame).
    pub(super) fn worst_for_spec(&mut self, kind: SpecKind, node: &str) -> Result<sim_core::WorstCase, String> {
        use sim_core::{McMeasure, WcSettings};
        let key = format!("{}|{:?}|{node}|{}|{}|{}|{}|{}|{}", self.signature_cached(), kind, self.mc_tol_r, self.mc_tol_c, self.mc_tol_l, self.ac_fstart, self.ac_fstop, self.ac_ppd);
        if let Some((_, r)) = self.worst_spec_cache.iter().find(|(k, _)| *k == key) {
            return r.clone();
        }
        let result = (|| {
            if node.trim().is_empty() {
                return Err("測るノードを選んでください".to_string());
            }
            let (Some(tol_r), Some(tol_c), Some(tol_l)) = (variation::percent(&self.mc_tol_r), variation::percent(&self.mc_tol_c), variation::percent(&self.mc_tol_l)) else {
                return Err("許容差は「温度とばらつき」の欄に % の数値で入力してください (例: 5)".to_string());
            };
            let measure = if kind == SpecKind::AcCutoffWorst {
                let (Some(fstart), Some(fstop), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
                    return Err("AC解析の欄の周波数と点数を数値で入力してください".to_string());
                };
                McMeasure::Cutoff { fstart, fstop, points_per_decade: ppd.max(1.0) as usize }
            } else {
                McMeasure::DcVoltage
            };
            let c = self.sim_circuit().map_err(|e| e.first().cloned().unwrap_or_default())?;
            sim_core::worst_case(&c, &WcSettings { tol_r, tol_c, tol_l, tol_v: 0.0, output: node.trim().to_string(), measure }).map_err(|e| e.to_string())
        })();
        if self.worst_spec_cache.len() >= 12 {
            let _ = self.worst_spec_cache.remove(0);
        }
        self.worst_spec_cache.push((key, result.clone()));
        result
    }

    pub(super) fn panel_spec(&mut self, ui: &mut egui::Ui) {
        crate::icons::help_small(ui, "目標値と合否の見方", "作りたい回路の目標値を書いておくと、解析の結果が合格か ✓/× で分かります。値を変えて計算し直すと、表も変わります");
        // the worst-case rows are judged on the circuit itself (not on a result on screen); the others on the results
        let worst_rows: Vec<(usize, SpecKind, String)> = self.specs.iter().enumerate().filter(|(_, r)| r.spec.kind.is_worst()).map(|(i, r)| (i, r.spec.kind, r.spec.target.clone())).collect();
        let worst_results: Vec<(usize, Result<sim_core::WorstCase, String>)> = worst_rows.into_iter().map(|(i, k, t)| (i, self.worst_for_spec(k, &t))).collect();
        // names to pick from, taken from the results that are on screen
        let dc = self.dc.as_ref().and_then(|v| v.outcome.as_ref().ok());
        let tran = self.tran.as_ref().and_then(|v| v.outcome.as_ref().ok());
        let ac = self.ac.as_ref().and_then(|v| v.outcome.as_ref().ok());
        let inputs = SpecInputs { dc, tran, ac };
        let nodes: Vec<String> = dc.map(|r| r.voltages.iter().map(|(n, _)| n.clone()).filter(|n| n != "0").collect()).unwrap_or_default();
        let parts: Vec<String> = dc.map(|r| r.elements.iter().map(|e| e.name.clone()).collect()).unwrap_or_default();
        let tran_names: Vec<String> = tran.map(|r| r.signals.iter().map(|s| s.name.clone()).filter(|n| !n.contains('.')).collect()).unwrap_or_default();
        let ac_names: Vec<String> = ac.map(|r| r.signals.iter().map(|s| s.name.clone()).filter(|n| !n.contains('.')).collect()).unwrap_or_default();
        let worst_nodes: Vec<String> = if self.specs.iter().any(|r| r.spec.kind.is_worst()) { extract(&self.sch).iter().map(|n| n.name.clone()).filter(|n| n != "GND").collect() } else { vec![] };
        let choices = |k: SpecKind| -> &Vec<String> {
            match k {
                SpecKind::DcVoltageWorst | SpecKind::AcCutoffWorst => &worst_nodes,
                SpecKind::DcVoltage => &nodes,
                SpecKind::DcCurrent => &parts,
                SpecKind::TranMean | SpecKind::TranPp | SpecKind::TranMax | SpecKind::TranMin => &tran_names,
                _ => &ac_names,
            }
        };

        let mut evals: Vec<SpecEval> = self.specs.iter_mut().map(|r| {
            r.sync();
            evaluate_spec(&r.spec, &inputs)
        }).collect();
        for (i, res) in &worst_results {
            evals[*i] = match res {
                Ok(w) => evaluate_worst_spec(&self.specs[*i].spec, w),
                Err(note) => SpecEval { value: None, pass: None, note: note.clone() },
            };
        }
        let judged = evals.iter().filter(|e| e.pass.is_some()).count();
        let passed = evals.iter().filter(|e| e.pass == Some(true)).count();
        if !self.specs.is_empty() {
            let (text, col) = if judged == 0 {
                ("まだ判定できる項目がありません (解析を実行してください)".to_string(), Color32::from_gray(160))
            } else if passed == judged {
                (format!("合格 {passed} / {judged}"), PASS)
            } else {
                (format!("合格 {passed} / {judged}  (× が {} 件)", judged - passed), FAIL)
            };
            ui.label(egui::RichText::new(text).strong().color(col));
        }

        let mut remove = None;
        for (i, (row, ev)) in self.specs.iter_mut().zip(&evals).enumerate() {
            ui.separator();
            ui.horizontal(|ui| {
                let mark = match ev.pass {
                    Some(true) => egui::RichText::new("✓").color(PASS).strong(),
                    Some(false) => egui::RichText::new("×").color(FAIL).strong(),
                    None => egui::RichText::new("—").color(Color32::from_gray(140)),
                };
                ui.label(mark);
                egui::ComboBox::from_id_salt(("spec_kind", i))
                    .selected_text(row.spec.kind.label())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for k in SpecKind::ALL {
                            if ui.selectable_value(&mut row.spec.kind, k, k.label()).changed() {
                                row.spec.target.clear();
                            }
                        }
                    })
                    .response
                    .on_hover_text(format!("何を測るか。{} の結果から読みます。過渡の「平均値」「リップル」は、起動直後を避けるため後半半分で測ります", row.spec.kind.needs()));
                let list = choices(row.spec.kind);
                if list.is_empty() {
                    ui.add(egui::TextEdit::singleline(&mut row.spec.target).desired_width(80.0).hint_text(if row.spec.kind.target_is_part() { "R1" } else { "V(out)" }))
                        .on_hover_text(format!("測る対象の名前。{} を実行すると、一覧から選べるようになります", row.spec.kind.needs()));
                } else {
                    egui::ComboBox::from_id_salt(("spec_target", i))
                        .selected_text(if row.spec.target.is_empty() { "(選ぶ)" } else { row.spec.target.as_str() })
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for n in list {
                                ui.selectable_value(&mut row.spec.target, n.clone(), n);
                            }
                        })
                        .response
                        .on_hover_text("測る対象。結果にある名前から選びます");
                }
                if ui.small_button("🗑").on_hover_text("この項目を消します").clicked() {
                    remove = Some(i);
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                if row.spec.kind == SpecKind::AcGainAt {
                    ui.label("@");
                    ui.add(egui::TextEdit::singleline(&mut row.at).desired_width(48.0)).on_hover_text("ゲインを読む周波数 (10k, 1M など)");
                    ui.label("Hz");
                }
                let unit = row.spec.kind.unit();
                let tip = "許される範囲。空欄ならその側は制限なしです (数字は 4.75, 50m, 1k など)";
                ui.add(egui::TextEdit::singleline(&mut row.lo).desired_width(56.0).hint_text("下限")).on_hover_text(tip);
                ui.label("〜");
                ui.add(egui::TextEdit::singleline(&mut row.hi).desired_width(56.0).hint_text("上限")).on_hover_text(tip);
                ui.label(unit);
            });
            let detail = match (ev.value, ev.pass) {
                // a worst-case row shows the whole range it judged, pass or fail
                _ if row.spec.kind.is_worst() => egui::RichText::new(format!("  {}", ev.note)).color(match ev.pass { Some(true) => PASS, Some(false) => FAIL, None => Color32::from_gray(150) }),
                (Some(v), Some(true)) => egui::RichText::new(format!("  測った値 {}", eng(v, row.spec.kind.unit()))).color(PASS),
                (Some(v), _) => egui::RichText::new(format!("  測った値 {}  ({})", eng(v, row.spec.kind.unit()), ev.note)).color(FAIL),
                (None, _) => egui::RichText::new(format!("  {}", ev.note)).color(Color32::from_gray(150)),
            };
            ui.small(detail);
            // the bench measurement, compared with the simulated value above
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label("実測");
                ui.add(egui::TextEdit::singleline(&mut row.measured).desired_width(64.0).hint_text("例 4.93"))
                    .on_hover_text("実際に作って測った値 (テスタやオシロの読み。4.93 や 50m のように書きます)。シミュレーションの値と比べて、差と、考えられる理由が出ます");
                ui.label(row.spec.kind.unit());
            });
            if !row.measured.trim().is_empty() {
                match (parse_value(&row.measured), ev.value) {
                    (None, _) => {
                        ui.small(egui::RichText::new("  実測の数字を読めません (4.93、50m、1k のように書きます)").color(FAIL));
                    }
                    (Some(_), None) => {
                        ui.small(egui::RichText::new("  シミュレーションの値が出ると、比べられます (解析を実行してください)").color(Color32::from_gray(150)));
                    }
                    (Some(m), Some(sim)) => {
                        let c = compare_measured(row.spec.kind, sim, m);
                        let unit = row.spec.kind.unit();
                        let (what, col) = match c.level {
                            Agreement::Close => ("よく合っています", PASS),
                            Agreement::Some => ("少し差があります", Color32::from_rgb(230, 170, 40)),
                            Agreement::Far => ("大きく違います", FAIL),
                        };
                        let pct = c.rel.map(|r| format!(" ({:+.1}%)", r * 100.0)).unwrap_or_default();
                        ui.label(egui::RichText::new(format!("  {what}{pct}")).color(col).strong());
                        ui.small(format!("  実測 {} / シミュレーション {} / 差 {}", eng(m, unit), eng(sim, unit), eng(c.diff, unit)));
                        if !c.hints.is_empty() {
                            egui::CollapsingHeader::new("差の理由の手がかり").id_salt(("spec_why", i)).show(ui, |ui| {
                                for h in &c.hints {
                                    ui.small(format!("・{h}"));
                                }
                            });
                        }
                    }
                }
            }
        }
        if let Some(i) = remove {
            self.specs.remove(i);
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("＋ 項目を追加").on_hover_text("目標値を1つ足します。いま画面にある解析 (DC → 過渡 → AC の順) に合う種類が選ばれます").clicked() {
                let kind = if tran.is_some() {
                    SpecKind::TranMean
                } else if ac.is_some() {
                    SpecKind::AcCutoff
                } else {
                    SpecKind::DcVoltage
                };
                let target = choices(kind).iter().find(|n| n.eq_ignore_ascii_case("V(out)") || n.eq_ignore_ascii_case("out") || n.eq_ignore_ascii_case("vout")).or(choices(kind).first()).cloned().unwrap_or_default();
                self.specs.push(SpecRow::new(kind, &target));
            }
            if !self.specs.is_empty() && ui.button("すべて消す").on_hover_text("目標値の表を空にします").clicked() {
                self.specs.clear();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_boxes_mean_no_limit_and_numbers_are_read() {
        let mut r = SpecRow::new(SpecKind::AcGainAt, "V(out)");
        r.sync();
        assert_eq!((r.spec.lo, r.spec.hi, r.spec.at), (Some(-23.0), Some(-17.0), 10e3));
        r.lo.clear();
        r.hi = "abc".into();
        r.sync();
        assert_eq!((r.spec.lo, r.spec.hi), (None, None));
    }
}

#[cfg(test)]
mod save_tests {
    use super::*;

    fn item(kind: &str, target: &str, lo: &str, hi: &str) -> SpecItem {
        SpecItem { kind: kind.into(), target: target.into(), lo: lo.into(), hi: hi.into(), at: String::new(), measured: String::new() }
    }

    #[test]
    fn specs_ride_along_in_the_sheet_json_and_come_back() {
        let mut v = serde_json::to_value(Schematic::default()).unwrap();
        with_specs(&mut v, &[item("dc_voltage", "OUT", "4.75", "5.25"), item("ac_gain_at", "OUT", "-23", "-17")]);
        let back = take_specs(&mut v);
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].kind, "ac_gain_at");
        // taking them out leaves a plain schematic
        assert!(serde_json::from_value::<Schematic>(v).is_ok());
    }

    #[test]
    fn an_old_file_without_specs_and_an_unknown_kind_are_harmless() {
        let mut v = serde_json::to_value(Schematic::default()).unwrap();
        assert!(take_specs(&mut v).is_empty());
        with_specs(&mut v, &[]);
        assert!(v.get("specs").is_none(), "an empty table adds nothing to the file");
        v["specs"] = serde_json::json!([{ "kind": "from_the_future", "target": "X" }, { "kind": "dc_voltage", "target": "OUT" }]);
        let back = take_specs(&mut v);
        assert_eq!(back.len(), 1);
        assert_eq!((back[0].lo.as_str(), back[0].hi.as_str()), ("", ""));
    }

    #[test]
    fn a_saved_sheet_brings_its_spec_table_back_and_editing_it_is_an_unsaved_change() {
        let dir = std::env::temp_dir().join(format!("cc_spec_save_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("with_specs.schjson");

        let mut e = SchEditor::default();
        e.add_spec(0, "OUT", "4.75", "5.25", "");
        assert!(e.dirty(), "adding a spec is a change to save");
        e.path = Some(f.clone());
        e.save(false);
        assert!(!e.dirty());
        e.specs[0].hi = "5.5".into();
        assert!(e.dirty(), "editing a limit is a change to save");

        let mut opened = SchEditor::default();
        opened.add_spec(1, "R1", "", "10m", ""); // replaced by the file's table
        opened.load_path(f);
        assert_eq!(opened.spec_items(), vec![item("dc_voltage", "OUT", "4.75", "5.25")]);
        assert!(!opened.dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bench_measurement_is_saved_with_the_spec_and_old_files_still_open() {
        let mut it = item("dc_voltage", "OUT", "4.75", "5.25");
        it.measured = "4.93".into();
        let mut v = serde_json::to_value(Schematic::default()).unwrap();
        with_specs(&mut v, &[it.clone(), item("dc_current", "R1", "", "20m")]);
        assert!(v["specs"][1].get("measured").is_none(), "no key when there is no measurement");
        let back = take_specs(&mut v);
        assert_eq!(back[0].measured, "4.93");
        assert_eq!(back[1].measured, "");
        // a file written before the measurement existed reads as "not measured"
        let mut old = serde_json::to_value(Schematic::default()).unwrap();
        old["specs"] = serde_json::json!([{ "kind": "dc_voltage", "target": "OUT", "lo": "4", "hi": "6", "at": "" }]);
        assert_eq!(take_specs(&mut old)[0].measured, "");
        // typing a measurement is an unsaved change
        let mut e = SchEditor::default();
        e.add_spec(0, "OUT", "4.75", "5.25", "");
        e.mark_saved();
        assert!(!e.dirty());
        e.specs[0].measured = "4.93".into();
        assert!(e.dirty());
    }

    #[test]
    fn the_measure_step_adds_its_rows_once_and_counts_compared_readings() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut e = SchEditor::default();
        e.load_path(root.join("series_regulator.schjson"));
        assert_eq!(e.compared_measurements(), 0);
        assert_eq!(e.add_measure_rows(), 3, "VIN, VOUT and a current to pick");
        assert_eq!(e.specs[0].spec.target, "VIN");
        assert_eq!(e.specs[1].spec.target, "VOUT");
        assert_eq!(e.add_measure_rows(), 0, "a second press does not add them again");
        // readings only count when there is a simulated value to compare with (the DC point was calculated)
        e.specs[0].measured = "9.02".into();
        e.specs[1].measured = "4.9".into();
        assert_eq!(e.compared_measurements(), 2);
        e.specs[2].measured = "49m".into();
        assert_eq!(e.compared_measurements(), 2, "the current row has no part chosen yet");
        e.specs[2].spec.target = "R2".into();
        assert_eq!(e.compared_measurements(), 3);
        e.specs[0].measured = "abc".into();
        assert_eq!(e.compared_measurements(), 2, "an unreadable reading does not count");
    }

    #[test]
    fn a_new_sheet_starts_with_an_empty_table() {
        let mut e = SchEditor::default();
        e.add_spec(0, "OUT", "4", "6", "");
        e.adopt(Schematic::default(), None);
        assert!(e.spec_items().is_empty());
        assert!(!e.dirty());
    }
}
