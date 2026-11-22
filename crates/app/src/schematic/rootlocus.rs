//! Root locus: one part's value stepped through a range (the parameter sweep's part and range), the poles of V(out)/V(in)
//! found again at each value (the poles-and-zeros box's nets), and their paths drawn in the complex plane.

use super::*;
use crate::plot::xy::{self, Line, XyCurve, XyMark, XyStyle, XyView};
use sim_core::{root_locus, sweep_values, AcSweep, LocusSettings, RootLocus, MAX_RUNS};
use std::f64::consts::TAU;

/// A finished root locus.
pub(super) struct LocusView {
    outcome: Result<RootLocus, Vec<String>>,
    /// Part, its unit (Ω, F, H) and the nets of the transfer function, for the titles.
    part: String,
    unit: &'static str,
    label: String,
    view: XyView,
    signature: String,
}

fn unit_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Resistor => "Ω",
        Kind::Capacitor => "F",
        Kind::Inductor => "H",
        _ => "",
    }
}

/// A point of the s plane drawn in Hz: (σ/2π, ω/2π), so a pole's distance from the origin is its frequency in the pole list.
fn hz(p: sim_core::C64) -> (f64, f64) {
    (p.re / TAU, p.im / TAU)
}

/// The box the automatic range shows: every pole within 20 times the middle size of all of them (a fast pole of an op-amp model
/// far out would squash the ones that move into a dot at the origin; it stays one wheel turn away).
fn focus_box(points: &[(f64, f64)]) -> Option<((f64, f64), (f64, f64))> {
    let mut mags: Vec<f64> = points.iter().map(|p| p.0.hypot(p.1)).filter(|m| m.is_finite() && *m > 0.0).collect();
    if mags.is_empty() {
        return None;
    }
    mags.sort_by(f64::total_cmp);
    let limit = mags[mags.len() / 2] * 20.0;
    let mut x = (0.0f64, 0.0f64);
    let mut y = (0.0f64, 0.0f64);
    for p in points.iter().filter(|p| p.0.hypot(p.1) <= limit) {
        x = (x.0.min(p.0), x.1.max(p.0));
        y = (y.0.min(p.1), y.1.max(p.1));
    }
    // the imaginary axis (the border of oscillation) always in sight, and the plane symmetric about the real axis
    let ym = y.0.abs().max(y.1.abs()).max((x.1 - x.0) * 0.1);
    Some(((x.0.min(0.0), x.1.max(0.0)), (-ym, ym)))
}

impl SchEditor {
    pub fn set_locus_settings(&mut self, part: &str, from: &str, to: &str, n: &str, out: &str, input: &str) {
        self.param_in.part = part.into();
        self.param_in.from = from.into();
        self.param_in.to = to.into();
        self.locus_n = n.into();
        self.pz_out = out.into();
        self.pz_in = input.into();
    }

    pub fn analyze_locus(&mut self) {
        self.last_run = LastRun::Locus;
        self.run_locus();
    }

    pub(super) fn panel_locus(&mut self, ui: &mut egui::Ui) {
        let parts = self.sweepable();
        let selected_part = match self.selected {
            Some(Sel::Comp(i)) => self.sch.components.get(i).map(|c| (c.refdes.clone(), c.value.clone())),
            _ => None,
        };
        ui.horizontal(|ui| {
            ui.label(i18n::tr("部品"));
            let mut pick = None;
            let shown = if self.param_in.part.is_empty() { "選ぶ".to_string() } else { self.param_in.part.clone() };
            egui::ComboBox::from_id_salt("locus_part")
                .width(70.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for (r, v) in &parts {
                        if ui.selectable_label(*r == self.param_in.part, format!("{r} ({v})")).clicked() {
                            pick = Some((r.clone(), v.clone()));
                        }
                    }
                })
                .response
                .on_hover_text(i18n::tr("値を変える部品 (抵抗・コンデンサ・コイル)。選ぶと、今の値の 1/10 〜 10倍が範囲に入ります。パラメータスイープと同じ欄です"));
            if pick.is_none()
                && let Some(sel) = selected_part.filter(|(r, _)| parts.iter().any(|p| &p.0 == r))
                && ui.small_button(i18n::tr("選択中の部品")).on_hover_text(i18n::tr("回路図で選んでいる部品の値を変えます")).clicked()
            {
                pick = Some(sel);
            }
            if let Some((r, v)) = pick {
                self.param_choose(&r, &v);
            }
        });
        ui.horizontal(|ui| {
            ui.label(i18n::tr("から"));
            ui.add(egui::TextEdit::singleline(&mut self.param_in.from).desired_width(48.0)).on_hover_text(i18n::tr("最初の値。例: 100, 1k, 10n"));
            ui.label(i18n::tr("まで"));
            ui.add(egui::TextEdit::singleline(&mut self.param_in.to).desired_width(48.0)).on_hover_text(i18n::tr("最後の値"));
            ui.label(i18n::tr("点数"));
            ui.add(egui::TextEdit::singleline(&mut self.locus_n).desired_width(24.0)).on_hover_text(i18n::trf!("値を何通り試すか (2〜{MAX_RUNS})。多いほど軌跡がなめらかで、時間がかかります。15 くらいが目安です"));
            ui.checkbox(&mut self.param_in.log, i18n::tr("対数")).on_hover_text(i18n::tr("オンで 10, 31.6, 100 のように倍率が一定の間隔、オフで足し算の等間隔"));
        });
        ui.horizontal(|ui| {
            ui.label(i18n::tr("出力"));
            ui.add(egui::TextEdit::singleline(&mut self.pz_out).desired_width(56.0).hint_text("OUT")).on_hover_text(i18n::tr("伝達関数の出力にするネット (「極と零」と同じ欄)"));
            ui.label(i18n::tr("入力"));
            ui.add(egui::TextEdit::singleline(&mut self.pz_in).desired_width(56.0).hint_text(i18n::tr("空欄"))).on_hover_text(i18n::tr("入力のネット (AC の電源がつながっているネット)。空欄なら出力の電圧そのもの"));
        });
        if self.lit(ui.button(i18n::tr("根軌跡を描く")), "根軌跡を描く").on_hover_text(i18n::tr("部品の値を1つずつ入れ替えて、そのたびに AC解析と極・零の当てはめをして、極の位置をつないで描きます。回路そのものは変わりません")).clicked() {
            self.run_locus();
        }
        let Some(v) = self.locus.as_ref() else { return };
        let Ok(r) = &v.outcome else { return };
        ui.separator();
        if v.signature != self.signature() {
            ui.label(i18n::tr("(回路が変わりました。もう一度実行してください)"));
        }
        let (first, last) = (r.steps.first().expect("two steps"), r.steps.last().expect("two steps"));
        let fmt = |x: f64| format!("{}{}", format_value(x), v.unit);
        match r.first_unstable() {
            Some(at) => {
                ui.colored_label(crate::theme::error(), i18n::trf!("{} = {} で、極が右半面に出ます (この値から先は発振します)", v.part, fmt(at)));
            }
            None if first.unstable() => {
                ui.colored_label(crate::theme::error(), i18n::trf!("{} = {} で、もう右半面に極があります (発振します)", v.part, fmt(first.value)));
            }
            None => {
                ui.label(i18n::tr("この範囲では、極はすべて左半面にあります (安定です)"));
            }
        }
        for (s, word) in [(first, "最初"), (last, "最後")] {
            let damp = s.min_damping().map_or_else(|| i18n::tr("極はすべて実数 (振動しない)").to_string(), |d| i18n::trf!("いちばん小さい減衰比 {d:.2}{}", if d < 0.0 { " (負: 振動が大きくなる)" } else if d < 0.3 { " (長く振動する)" } else { "" }));
            ui.label(i18n::trf!("{word} ({} = {}): 極 {} 個、{damp}", v.part, fmt(s.value), s.pole_points().len()))
                .on_hover_text(i18n::tr("減衰比は、複素数の極の対が虚軸にどれだけ近いかです。1 で振動せず、0.7 で少し行き過ぎ、0.3 未満でステップ応答が長く振動し、0 で発振します"));
        }
    }

    pub(super) fn run_locus(&mut self) {
        let outcome = self.locus_outcome();
        let part = self.param_in.part.trim().to_string();
        let unit = self.sch.components.iter().find(|c| c.refdes.eq_ignore_ascii_case(&part)).map_or("", |c| unit_of(c.kind));
        let label = if self.pz_in.trim().is_empty() { format!("V({})", self.pz_out.trim()) } else { format!("V({}) / V({})", self.pz_out.trim(), self.pz_in.trim()) };
        self.status = match &outcome {
            Ok(r) => match r.first_unstable() {
                Some(v) => format!("根軌跡: {part} = {}{unit} で極が右半面に出ます (発振)", format_value(v)),
                None => format!("根軌跡: {part} を {} 通りに変えて極をたどりました{}", r.steps.len(), if r.failed.is_empty() { String::new() } else { format!(" ({} 通りは計算できず)", r.failed.len()) }),
            },
            Err(e) => format!("根軌跡に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.locus = Some(LocusView { outcome, part, unit, label, view: XyView::new(true), signature: self.signature() });
        self.wave_mode = WaveMode::Locus;
    }

    fn locus_outcome(&self) -> Result<RootLocus, Vec<String>> {
        let temp = match self.temp_c.trim() {
            "" => T_NOM_C,
            t => t.parse::<f64>().map_err(|_| vec![format!("温度「{t}」を数値(℃)で入力してください (例: 27)")])?,
        };
        let part = self.param_in.part.trim();
        if part.is_empty() {
            return Err(vec!["値を変える部品を選んでください (抵抗・コンデンサ・コイル)".into()]);
        }
        if self.pz_out.trim().is_empty() {
            return Err(vec!["出力のネットの名前を入力してください".into()]);
        }
        let (Some(a), Some(b), Some(n)) = (parse_value(&self.param_in.from), parse_value(&self.param_in.to), parse_value(&self.locus_n)) else {
            return Err(vec!["「から」「まで」「点数」を数値で入力してください (例: 10, 200, 15)".into()]);
        };
        if a <= 0.0 || b <= 0.0 {
            return Err(vec!["値は 0 より大きい数にしてください".into()]);
        }
        let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
            return Err(vec!["AC解析の周波数の設定を数値で入力してください (例: 10, 1Meg, 20)".into()]);
        };
        let values = sweep_values(a, b, n.round().clamp(2.0, MAX_RUNS as f64) as usize, self.param_in.log);
        let input = Some(self.pz_in.trim().to_string()).filter(|s| !s.is_empty());
        let s = LocusSettings { refdes: part.into(), values, output: self.pz_out.trim().into(), input, ac: AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) }, temp_c: temp, max_poles: 6, tolerance: 0.02 };
        root_locus(&self.sch, &s).map_err(|e| vec![e.to_string()])
    }

    pub(super) fn locus_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(v) = self.locus.as_mut() else { return };
        let mut close = false;
        let LocusView { outcome, part, unit, label, view, .. } = v;
        ui.horizontal(|ui| {
            crate::widgets::heading_inline(ui, i18n::trf!("根軌跡: {part} を変えたときの {label} の極"))
                .on_hover_text(i18n::tr("極は、回路が自分で持っている「くせ」の周波数です。左半面 (σ < 0) の極は時間とともに収まり、虚軸に近いほど長く振動し、右半面 (赤い所) に出ると振動が大きくなっていく = 発振します。縦軸の値は振動の周波数、横軸は収まる速さです"));
            if crate::widgets::close_x(ui, "根軌跡の表示") {
                close = true;
            }
        });
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(crate::theme::error(), i18n::tr_dyn(m));
                }
            }
            Ok(r) => {
                crate::widgets::warning_lines(ui, "locus_warn", &r.warnings, |_| false);
                for (val, why) in &r.failed {
                    ui.colored_label(crate::theme::warn(), i18n::trf!("{part} = {}{unit} は計算できませんでした: {why}", format_value(*val)));
                }
                let (first, last) = (r.steps[0].value, r.steps[r.steps.len() - 1].value);
                ui.small(i18n::trf!("○ = {part} が {}{unit} のときの極、× = {}{unit} のときの極。線は値を変えたときの極の道すじ (点が 1 つの値)", format_value(first), format_value(last)));
                let branches = r.branches();
                let pts: Vec<Vec<(f64, f64)>> = branches.iter().map(|b| b.iter().map(|(_, p)| hz(*p)).collect()).collect();
                let tags: Vec<Vec<f64>> = branches.iter().map(|b| b.iter().map(|(k, _)| r.steps[*k].value).collect()).collect();
                let curves: Vec<XyCurve> = pts
                    .iter()
                    .zip(&tags)
                    .enumerate()
                    .map(|(k, (p, t))| XyCurve { name: "極", color: crate::plot::series(k % 7), pts: p, tags: t, line: Line::WithDots })
                    .collect();
                let mut marks = vec![];
                let n = r.steps.len() - 1;
                for (b, p) in branches.iter().zip(&pts) {
                    if b[0].0 == 0 {
                        marks.push(XyMark { at: p[0], color: Color32::from_gray(230), shape: xy::Shape::Ring, text: String::new(), inline: true });
                    }
                    if b[b.len() - 1].0 == n {
                        marks.push(XyMark { at: p[p.len() - 1], color: Color32::from_gray(230), shape: xy::Shape::Cross, text: String::new(), inline: true });
                    }
                }
                let all: Vec<(f64, f64)> = pts.iter().flatten().copied().collect();
                let style = XyStyle { x_name: "実部 σ/2π (Hz)  ← 早く収まる", y_name: "虚部 ω/2π (Hz)", x_unit: "", y_unit: "", plain: false, tag_name: part, tag_unit: unit, shade_right: true };
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                xy::draw_xy(ui, rect, egui::Id::new("root_locus"), view, &curves, &marks, focus_box(&all), &style);
            }
        }
        if close {
            self.locus = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_focus_box_leaves_out_a_far_pole_and_keeps_the_imaginary_axis() {
        let pts = [(-1000.0, 800.0), (-1000.0, -800.0), (-1200.0, 0.0), (-5e6, 0.0)];
        let ((x0, x1), (y0, y1)) = focus_box(&pts).unwrap();
        assert!(x0 <= -1200.0 && x0 > -1e5, "{x0}");
        assert!(x1 >= 0.0);
        assert!((y0 + y1).abs() < 1e-9 && y1 >= 800.0);
        assert!(focus_box(&[]).is_none());
    }

    fn sample(name: &str) -> Schematic {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn the_rlc_sample_gives_a_locus_from_the_panel_settings() {
        let mut e = SchEditor::default();
        e.sch = sample("rlc_poles.schjson");
        e.set_ac_settings("100", "1Meg", "20");
        e.set_locus_settings("R1", "5", "200", "6", "OUT", "IN");
        e.param_in.log = true;
        e.analyze_locus();
        assert!(matches!(e.wave_mode, WaveMode::Locus));
        let r = e.locus.as_ref().unwrap().outcome.as_ref().unwrap();
        assert_eq!(r.steps.len(), 6);
        assert_eq!(r.first_unstable(), None);
        assert_eq!(e.locus.as_ref().unwrap().unit, "Ω");
        // a missing net is said in words, not a panic
        e.set_locus_settings("R1", "5", "200", "6", "", "IN");
        e.analyze_locus();
        assert!(e.locus.as_ref().unwrap().outcome.is_err());
    }
}
