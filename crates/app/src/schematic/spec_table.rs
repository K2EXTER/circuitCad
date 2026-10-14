//! "スペック (合否)" section: targets for the design, checked against the analyses on screen.

use super::*;
use sim_core::{evaluate_spec, Spec, SpecInputs, SpecKind};

/// One row of the table: the spec plus the text boxes it is typed in (kept so typing is not overwritten).
pub(super) struct SpecRow {
    spec: Spec,
    lo: String,
    hi: String,
    at: String,
}

impl SpecRow {
    fn new(kind: SpecKind, target: &str) -> Self {
        let (lo, hi, at) = match kind {
            SpecKind::DcVoltage | SpecKind::TranMean => ("4.75", "5.25", ""),
            SpecKind::TranPp => ("", "50m", ""),
            SpecKind::TranMax => ("", "5.5", ""),
            SpecKind::TranMin => ("4.5", "", ""),
            SpecKind::DcCurrent => ("", "20m", ""),
            SpecKind::AcCutoff => ("900", "1.1k", ""),
            SpecKind::AcGainAt => ("-23", "-17", "10k"),
            SpecKind::PhaseMargin => ("45", "", ""),
        };
        Self { spec: Spec { kind, target: target.into(), at: 0.0, lo: None, hi: None }, lo: lo.into(), hi: hi.into(), at: at.into() }
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
}

impl SpecItem {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "kind": self.kind, "target": self.target, "lo": self.lo, "hi": self.hi, "at": self.at })
    }

    fn from_json(v: &serde_json::Value) -> Option<SpecItem> {
        let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let kind = text("kind");
        SpecKind::from_key(&kind)?;
        Some(SpecItem { kind, target: text("target"), lo: text("lo"), hi: text("hi"), at: text("at") })
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
            .map(|r| SpecItem { kind: r.spec.kind.key().into(), target: r.spec.target.clone(), lo: r.lo.clone(), hi: r.hi.clone(), at: r.at.clone() })
            .collect()
    }

    /// Replaces the table by the one read from a file; it counts as saved.
    pub fn set_spec_items(&mut self, items: &[SpecItem]) {
        self.specs = items
            .iter()
            .filter_map(|it| {
                let mut row = SpecRow::new(SpecKind::from_key(&it.kind)?, &it.target);
                (row.lo, row.hi, row.at) = (it.lo.clone(), it.hi.clone(), it.at.clone());
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

    pub(super) fn panel_spec(&mut self, ui: &mut egui::Ui) {
        ui.small("作りたい回路の目標値を書いておくと、解析の結果が合格か ✓/× で分かります。値を変えて計算し直すと、表も変わります");
        // names to pick from, taken from the results that are on screen
        let dc = self.dc.as_ref().and_then(|v| v.outcome.as_ref().ok());
        let tran = self.tran.as_ref().and_then(|v| v.outcome.as_ref().ok());
        let ac = self.ac.as_ref().and_then(|v| v.outcome.as_ref().ok());
        let inputs = SpecInputs { dc, tran, ac };
        let nodes: Vec<String> = dc.map(|r| r.voltages.iter().map(|(n, _)| n.clone()).filter(|n| n != "0").collect()).unwrap_or_default();
        let parts: Vec<String> = dc.map(|r| r.elements.iter().map(|e| e.name.clone()).collect()).unwrap_or_default();
        let tran_names: Vec<String> = tran.map(|r| r.signals.iter().map(|s| s.name.clone()).filter(|n| !n.contains('.')).collect()).unwrap_or_default();
        let ac_names: Vec<String> = ac.map(|r| r.signals.iter().map(|s| s.name.clone()).filter(|n| !n.contains('.')).collect()).unwrap_or_default();
        let choices = |k: SpecKind| -> &Vec<String> {
            match k {
                SpecKind::DcVoltage => &nodes,
                SpecKind::DcCurrent => &parts,
                SpecKind::TranMean | SpecKind::TranPp | SpecKind::TranMax | SpecKind::TranMin => &tran_names,
                _ => &ac_names,
            }
        };

        let evals: Vec<_> = self.specs.iter_mut().map(|r| {
            r.sync();
            evaluate_spec(&r.spec, &inputs)
        }).collect();
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
                (Some(v), Some(true)) => egui::RichText::new(format!("  測った値 {}", eng(v, row.spec.kind.unit()))).color(PASS),
                (Some(v), _) => egui::RichText::new(format!("  測った値 {}  ({})", eng(v, row.spec.kind.unit()), ev.note)).color(FAIL),
                (None, _) => egui::RichText::new(format!("  {}", ev.note)).color(Color32::from_gray(150)),
            };
            ui.small(detail);
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
        SpecItem { kind: kind.into(), target: target.into(), lo: lo.into(), hi: hi.into(), at: String::new() }
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
    fn a_new_sheet_starts_with_an_empty_table() {
        let mut e = SchEditor::default();
        e.add_spec(0, "OUT", "4", "6", "");
        e.adopt(Schematic::default(), None);
        assert!(e.spec_items().is_empty());
        assert!(!e.dirty());
    }
}
