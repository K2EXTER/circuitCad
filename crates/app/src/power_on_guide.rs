//! "最初の電源投入のガイド" (candidate B1): for a board built from the open schematic, the current each supply should deliver, the range
//! a working board falls in (tolerances, data sheet spreads and self-heating: `sim_core::power_on`), the current limit to set on a bench
//! supply, the steps, and a verdict on the current the learner reads with the causes to look at. Opened from the menu (学ぶ), the
//! palette and the bench guides' power-on step.

use crate::learn::{spaced, NOTE};
use crate::schematic::SchEditor;
use eframe::egui::{self, Color32};
use sim_core::{PowerOn, PowerOnReading as Reading, PowerOnSettings};

const WARN: Color32 = crate::theme::WARN;
const OK: Color32 = crate::theme::OK;
const BAD: Color32 = crate::theme::ERROR;

const INTRO: &str = "組み立てた基板に初めて電源を入れるときは、電流を制限できる電源で入れて、流れた電流を、回路図のシミュレーションで出した「正常なら流れる電流」と比べます。ショートや部品の向き違いがあっても、電流が制限されるので、部品を壊さずに気づけます。";
const SAFETY: &str = "安全: 電流が制限の値に張り付いた (電源の CC の表示が消えない) ら、すぐ電源を切ります。熱い部品は、触らずに、手をかざすか温度計で探します。電解コンデンサは逆向きにつなぐと、ふくらんだり破裂したりします。";
const STEPS: &[&str] = &[
    "1. 電源を入れる前に: 拡大鏡で、はんだのブリッジ・部品の向き (電解コンデンサ・ダイオード・IC の 1 番ピン) を見ます。テスタの抵抗レンジで、電源と GND の間がショート (0Ω 付近) していないことを確かめます。",
    "2. 電源の電圧と、電流制限を、下の値に合わせます。出力を OFF にしたまま、基板につなぎます (+ と − を確かめて)。",
    "3. 出力を ON にして、電流の表示を見ます。最初の一瞬 CC (電流制限) の表示が出て、すぐ消えるのは、コンデンサの充電で、正常です。",
    "4. 落ち着いた電流を、下の「測った電流」に入れて、正常な範囲と比べます。",
];
const LIMITS: &str = "範囲は、抵抗などの許容差の端と、部品のモデルに書かれたデータシートの min / max、自己発熱の分を入れた計算です。モデルの無い部品 (コネクタの先の機器、マイコンのプログラムで変わる電流) は入っていません。マイコンや無線の部品は、動きによって電流が大きく変わるので、データシートの消費電流とも比べてください。";

/// The first power-on window.
pub struct PowerOnGuide {
    pub open: bool,
    tol_r: String,
    ambient: String,
    spreads: bool,
    /// The answer and the circuit signature it was found for.
    result: Option<(Result<PowerOn, String>, String)>,
    /// The current read on the bench, per supply (mA, as typed).
    measured: Vec<(String, String)>,
}

impl Default for PowerOnGuide {
    fn default() -> Self {
        Self { open: false, tol_r: "5".into(), ambient: "25".into(), spreads: true, result: None, measured: vec![] }
    }
}

fn amps(a: f64) -> String {
    format!("{}A", sim_core::format_value(a))
}

impl PowerOnGuide {
    pub fn open_for(&mut self, sch: &SchEditor) {
        self.open = true;
        self.run(sch);
    }

    fn settings(&self) -> Result<PowerOnSettings, String> {
        let tol: f64 = self.tol_r.trim().parse().ok().filter(|t: &f64| (0.0..100.0).contains(t)).ok_or("抵抗の許容差を 0〜100 未満の数 (%) で入れてください (例: 5)")?;
        let ambient: f64 = self.ambient.trim().parse().ok().filter(|t: &f64| *t > -273.0 && *t < 1000.0).ok_or("周囲の温度を数値 (℃) で入れてください (例: 25)")?;
        Ok(PowerOnSettings { tol_r: tol / 100.0, ambient, spreads: self.spreads, ..Default::default() })
    }

    fn run(&mut self, sch: &SchEditor) {
        let sig = sch.circuit_signature();
        let r = self.settings().and_then(|s| {
            let (c, heaters) = sch.power_on_inputs()?;
            let mut p = sim_core::power_on(&c, &heaters, &s).map_err(|e| e.to_string())?;
            // a circuit that keeps moving (a 555 that blinks) draws its running current, not its DC point's
            if let Some(run) = sch.running_tran() {
                let names: Vec<String> = p.supplies.iter().map(|x| x.name.clone()).collect();
                for n in names {
                    if let Some(sig) = run.signal(&format!("I({n})")) {
                        p.apply_running(&n, &run.time, &sig.values, s.margin);
                    }
                }
            }
            Ok(p)
        });
        self.result = Some((r, sig));
    }

    #[cfg(test)]
    pub fn result_for_test(&self) -> Option<&Result<PowerOn, String>> {
        self.result.as_ref().map(|(r, _)| r)
    }

    /// Types `ma` into the reading of the main supply (the one giving the most power), as the start argument `--power-on 60` does.
    pub fn set_measured_main(&mut self, ma: &str) {
        let Some((Ok(p), _)) = &self.result else { return };
        let Some(name) = p.supplies.first().map(|s| s.name.clone()) else { return };
        self.measured.retain(|(n, _)| *n != name);
        self.measured.push((name, ma.into()));
    }

    pub fn show(&mut self, ctx: &egui::Context, sch: &SchEditor) {
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new(i18n::tr("最初の電源投入のガイド")).open(&mut open).default_pos(crate::widgets::beside_panel()).default_width(560.0).default_height(680.0).vscroll(true).show(ctx, |ui| self.contents(ui, sch));
        if !open {
            self.open = false;
        }
    }

    /// Calculates for the schematic unless an answer is there already (the bench tab's step shows it without a press).
    pub fn ensure_run(&mut self, sch: &SchEditor) {
        if self.result.is_none() {
            self.run(sch);
        }
    }

    /// The supplies the answer has (their part names, V1 ...), for the bench tab to light up on the board.
    pub fn supply_names(&self) -> Vec<String> {
        match &self.result {
            Some((Ok(p), _)) => p.supplies.iter().map(|s| s.name.clone()).collect(),
            _ => vec![],
        }
    }

    /// Every supply has a reading typed in, and each is in the range a working board falls in.
    pub fn all_normal(&self) -> bool {
        let Some((Ok(p), _)) = &self.result else { return false };
        !p.supplies.is_empty()
            && p.supplies.iter().all(|s| {
                let typed = self.measured.iter().find(|(n, _)| *n == s.name).map(|(_, t)| t.trim()).unwrap_or("");
                sim_core::parse_value(typed).filter(|x| x.is_finite()).is_some_and(|ma| s.judge(ma * 1e-3) == Reading::Normal)
            })
    }

    /// The window's contents, also drawn in the step "電源投入" of the bench tab (`bench_tab.rs`).
    pub fn contents(&mut self, ui: &mut egui::Ui, sch: &SchEditor) {
        if self.body(ui, sch) {
            self.run(sch);
        }
    }

    /// Draws the guide; true when "計算する" was pressed.
    fn body(&mut self, ui: &mut egui::Ui, sch: &SchEditor) -> bool {
        let mut rerun = false;
        {
            ui.label(spaced(INTRO));
            ui.add_space(4.0);
            ui.colored_label(WARN, i18n::tr(SAFETY));
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(i18n::tr("抵抗の許容差"));
                ui.add(egui::TextEdit::singleline(&mut self.tol_r).desired_width(36.0)).on_hover_text(i18n::tr("抵抗の値のずれの幅 (%)。ふつうのチップ抵抗は 1 % か 5 %"));
                ui.label("%");
                ui.label(i18n::tr("周囲の温度"));
                ui.add(egui::TextEdit::singleline(&mut self.ambient).desired_width(36.0)).on_hover_text(i18n::tr("部品の自己発熱は、この温度の上に乗ります"));
                ui.label("℃");
                ui.checkbox(&mut self.spreads, i18n::tr("データシートの min / max")).on_hover_text(i18n::tr("部品のモデルに書かれた、データシートの最小・最大 (レギュレータの消費電流など) も、端に置いて範囲に入れます"));
                if ui.button(i18n::tr("計算する")).on_hover_text(i18n::tr("いまの回路図で、電源ごとの電流と範囲を計算し直します")).clicked() {
                    rerun = true;
                }
            });
            let Some((result, sig)) = &self.result else {
                ui.label(i18n::tr("「計算する」を押すと、いまの回路図から電流を計算します"));
                return rerun;
            };
            if *sig != sch.circuit_signature() {
                ui.colored_label(WARN, i18n::tr("回路図が変わりました。「計算する」で計算し直してください"));
            }
            let p = match result {
                Ok(p) => p,
                Err(e) => {
                    ui.colored_label(BAD, i18n::tr_dyn(e));
                    return rerun;
                }
            };
            ui.separator();
            for st in STEPS {
                ui.label(spaced(st).color(NOTE));
            }
            for s in &p.supplies {
                ui.separator();
                crate::widgets::section_heading(ui, i18n::trf!("{} ({} V)", s.name, sim_core::format_value(s.volts)));
                egui::Grid::new(("power_on_grid", &s.name)).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    ui.label(i18n::tr("正常なら流れる電流"));
                    ui.strong(format!("{} 〜 {}", amps(s.min), amps(s.max))).on_hover_text(i18n::tr("部品の許容差の端・データシートの min / max・自己発熱を入れた範囲。組んだ基板の電流は、この中に入るはずです"));
                    ui.end_row();
                    ui.label(i18n::tr("部品が表示どおりの値なら"));
                    ui.label(amps(s.nominal));
                    ui.end_row();
                    if let Some(w) = s.warm {
                        ui.label(i18n::tr("部品が温まったあと"));
                        ui.label(amps(w)).on_hover_text(i18n::tr("部品が自分の電力で温まって、値やしきい値が変わったあとの電流"));
                        ui.end_row();
                    }
                    ui.label(i18n::tr("電流制限に入れる値"));
                    ui.colored_label(OK, egui::RichText::new(amps(s.limit)).strong()).on_hover_text(i18n::tr("正常な範囲の上の端の 1.5 倍を、電源のつまみで合わせやすい値に切り上げたもの (10 mA より小さくはしません)"));
                    ui.end_row();
                });
                if let Some(run) = s.running {
                    ui.label(
                        spaced(&i18n::trf!(
                            "この回路は動き続けます (発振・パルスなど)。過渡解析の後半では、電流が {} 〜 {} の間で変わり、平均は {} です。電源の電流の表示は平均に近い値になるので、範囲は平均を中心にしています。DC 動作点の {} は、動きが止まった状態の値で、実物では流れません。",
                            amps(run.min),
                            amps(run.max),
                            amps(run.mean),
                            amps(run.dc)
                        ))
                        .color(WARN),
                    )
                    .on_hover_text(i18n::tr("過渡解析は、回路図の解析タブの過渡解析の時間で計算します (前に実行した結果があれば、それを使います)。ゆっくり動く回路は、その時間を数周期ぶんにしてから計算し直してください"));
                }
                if !s.parts.is_empty() {
                    let top: Vec<String> = s.parts.iter().take(3).map(|(n, lo, hi)| format!("{n} (±{})", amps(lo.abs().max(hi.abs())))).collect();
                    ui.label(egui::RichText::new(i18n::trf!("電流を動かす部品: {}", top.join("、"))).color(NOTE)).on_hover_text(i18n::tr("その部品だけを許容差の端にしたときの、電流の変わり方"));
                }
                // the reading and its verdict
                let entry = match self.measured.iter().position(|(n, _)| *n == s.name) {
                    Some(i) => i,
                    None => {
                        self.measured.push((s.name.clone(), String::new()));
                        self.measured.len() - 1
                    }
                };
                ui.horizontal(|ui| {
                    ui.label(i18n::tr("測った電流"));
                    ui.add(egui::TextEdit::singleline(&mut self.measured[entry].1).desired_width(60.0)).on_hover_text(i18n::tr("電源の表示か電流計で読んだ、落ち着いたあとの電流 (mA)"));
                    ui.label("mA");
                });
                let typed = self.measured[entry].1.trim().to_string();
                if typed.is_empty() {
                    continue;
                }
                let Some(ma) = sim_core::parse_value(&typed).filter(|x| x.is_finite()) else {
                    ui.colored_label(BAD, i18n::tr("電流を数で入れてください (例: 12.5)"));
                    continue;
                };
                let verdict = s.judge(ma * 1e-3);
                let color = match verdict {
                    Reading::Normal => OK,
                    Reading::MuchHigh => BAD,
                    _ => WARN,
                };
                ui.colored_label(color, egui::RichText::new(i18n::tr(verdict.title())).strong());
                if verdict != Reading::Normal {
                    ui.label(egui::RichText::new(i18n::tr("疑う所 (上から順に):")).color(NOTE));
                }
                for c in verdict.causes() {
                    ui.label(spaced(&format!("・{}", i18n::tr(c))).color(NOTE));
                }
            }
            if !p.unsolved.is_empty() {
                ui.colored_label(WARN, i18n::trf!("解けなかった部品 (表示どおりの値のまま): {}", p.unsolved.join(", ")));
            }
            ui.separator();
            ui.label(spaced(LIMITS).color(NOTE));
        }
        rerun
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, g: &mut PowerOnGuide, sch: &SchEditor) {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| g.show(ui.ctx(), sch));
        out.textures_delta.clear();
    }

    #[test]
    fn the_led_sample_gives_a_range_a_limit_and_a_verdict() {
        let mut sch = SchEditor::default();
        sch.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/power_on.schjson"));
        let mut g = PowerOnGuide::default();
        g.open_for(&sch);
        let s = g.result_for_test().unwrap().as_ref().unwrap().supplies[0].clone();
        assert!(s.min < s.nominal && s.nominal < s.max && s.max < s.limit, "{s:?}");
        assert!(s.running.is_none(), "an LED on a resistor sits at its DC point: {s:?}");
        g.set_measured_main(&format!("{}", s.nominal * 1e3));
        let ctx = egui::Context::default();
        frame(&ctx, &mut g, &sch);
        assert!(g.open);
    }

    #[test]
    fn a_schematic_without_a_supply_says_so() {
        let sch = SchEditor::default();
        let mut g = PowerOnGuide::default();
        g.open_for(&sch);
        assert!(g.result_for_test().unwrap().is_err());
        frame(&egui::Context::default(), &mut g, &sch);
    }
}
