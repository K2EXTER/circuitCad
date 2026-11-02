//! Loop gain of a feedback circuit by injection. The result is shown in the AC viewer (Bode plot, crossover cursors, margins), as a
//! signal "T".

use super::*;
use sim_core::{loop_gain, AcSignal, BreakDirection};

/// A note for a loop that has the bundled op-amp models in it: they are one-pole amplifiers (a gain, one pole, an output
/// resistor), so the phase margin they give is larger than a real part's, which has a second pole. None without such a model.
/// The models are told by the first stage, an `Eop` source inside a part (`U1.Eop`).
pub(super) fn opamp_model_note(c: &sim_core::Circuit) -> Option<String> {
    let mut parts: Vec<String> = c
        .elements
        .iter()
        .filter_map(|e| match e {
            sim_core::Element::Vcvs { name, .. } => name.strip_suffix(".Eop").map(str::to_string),
            _ => None,
        })
        .collect();
    parts.sort();
    parts.dedup();
    if parts.is_empty() {
        return None;
    }
    Some(format!(
        "注意: このループには、オペアンプのモデル ({}) があります。同梱のモデルは「増幅 → 極が1つ → 出力抵抗」の簡単な形で、実物の部品が持つ第2の極を持ちません。位相余裕は、そのモデルでの値で、実物より大きく出ます (負荷の容量による分は表れます)。実機の余裕は、メーカーのモデルか、実機で確かめてください",
        parts.join("、")
    ))
}

impl SchEditor {
    pub fn set_loop_settings(&mut self, part: &str, from: &str) {
        self.loop_part = part.into();
        self.loop_from = from.into();
    }

    pub fn analyze_loop(&mut self) {
        self.run_loop();
    }

    /// A click while choosing: the body of a part = the part to cut, a pin or wire = the net on its output side.
    pub(super) fn pick_loop(&mut self, g: (f32, f32), pt: Pt) {
        if let Some(i) = self.comp_at(g).filter(|i| !self.sch.components[*i].kind.is_power_symbol()) {
            self.loop_part = self.sch.components[i].refdes.clone();
            self.noise_pick = false;
            self.loop_pick = false;
            self.run_loop();
        } else if let Some(name) = net_at(&self.sch, pt) {
            self.loop_from = name;
            self.status = format!("出力側のネットを {} にしました。切る部品の本体をクリックしてください", self.loop_from);
        } else {
            self.status = "部品の本体 (切る部品) か、ピン・配線 (出力側のネット) をクリックしてください".into();
        }
    }

    pub(super) fn run_loop(&mut self) {
        let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
            self.status = "開始・終了周波数と点数/デケードを数値で入力してください (例: 10, 1Meg, 20)".into();
            return;
        };
        let part = self.loop_part.trim().to_string();
        let from = self.loop_from.trim().to_string();
        let outcome = if part.is_empty() {
            Err(vec!["ループを切る部品の名前を入力してください (帰還の抵抗が一番かんたんです。「回路図で選ぶ」でも入れられます)".to_string()])
        } else {
            match sim_core::from_schematic_with_layout(&self.sch, &self.wiring, &self.couplings) {
                Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
                Ok(c) => {
                    // the net on the output side picks the direction; without it both are tried
                    let dir = if from.is_empty() {
                        BreakDirection::Auto
                    } else {
                        let ends = c.elements.iter().find(|e| e.name().eq_ignore_ascii_case(&part)).and_then(|e| match e {
                            sim_core::Element::Resistor { a, b, .. } | sim_core::Element::Capacitor { a, b, .. } | sim_core::Element::Inductor { a, b, .. } => Some((*a, *b)),
                            _ => None,
                        });
                        match ends {
                            Some((a, _)) if c.names[a].eq_ignore_ascii_case(&from) => BreakDirection::AToB,
                            Some((_, b)) if c.names[b].eq_ignore_ascii_case(&from) => BreakDirection::BToA,
                            _ => BreakDirection::Auto,
                        }
                    };
                    loop_gain(&c, &part, dir, &AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) })
                        .map(|r| {
                            let sig = |name: &str, v: &[sim_core::C64]| AcSignal { name: name.into(), unit: "V", re: v.iter().map(|z| z.re).collect(), im: v.iter().map(|z| z.im).collect() };
                            let mut warnings = r.warnings.clone();
                            warnings.insert(0, format!("{} の所でループを切りました (信号は {} → {} の向き)", r.element, r.from_net, r.to_net));
                            warnings.extend(opamp_model_note(&c));
                            AcResult { freq: r.freq.clone(), signals: vec![sig("T ループゲイン", &r.t), sig("Tv 電圧注入だけ", &r.tv), sig("Ti 電流注入だけ", &r.ti)], warnings }
                        })
                        .map_err(|e| vec![e.to_string()])
                }
            }
        };
        let mut selected = BTreeSet::new();
        selected.insert("T ループゲイン".to_string());
        self.status = match &outcome {
            Ok(r) => {
                let t = r.signal("T ループゲイン").expect("T");
                match stability_margins(&r.freq, &t.db(), &t.phase_deg()).filter(|m| m.gain_crossover.is_some()) {
                    Some(m) => format!("ループゲイン: 0 dB の周波数 {}、位相余裕 {:.0}°", eng(m.gain_crossover.unwrap_or(0.0), "Hz"), m.phase_margin.unwrap_or(0.0)),
                    None => "ループゲイン: 計算範囲に 0 dB の交点がありません (周波数の範囲を変えてみてください)".to_string(),
                }
            }
            Err(e) => format!("ループゲインに失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        let ghost = self.ac.as_ref().and_then(|v| v.ghost.clone());
        self.ac = Some(AcView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi), ghost, signature: self.signature(), loop_run: true, pz_run: false });
        self.wave_mode = WaveMode::Ac;
    }

    pub(super) fn panel_loop(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("切る部品");
            ui.add(egui::TextEdit::singleline(&mut self.loop_part).desired_width(44.0).hint_text("R1"))
                .on_hover_text("帰還のループの途中にある抵抗・コンデンサ・コイルの番号。ここでループを1か所切って、1周したときのゲインを測ります。帰還の抵抗 (出力からマイナス入力につながる抵抗) が一番かんたんです");
            ui.label("出力側");
            ui.add(egui::TextEdit::singleline(&mut self.loop_from).desired_width(50.0).hint_text("自動"))
                .on_hover_text("切った部品の、出力に近いほうのネットの名前。空欄なら、両方の向きを試して、ゲインが大きいほうを使います");
        });
        ui.horizontal(|ui| {
            let label = if self.loop_pick { "選択中… (Esc で中止)" } else { "回路図で選ぶ" };
            if ui.selectable_label(self.loop_pick, label).on_hover_text("押してから、部品の本体をクリックすると切る部品になり、すぐ解析します。ピンや配線をクリックすると、出力側のネットになります").clicked() {
                self.loop_pick = !self.loop_pick;
                self.noise_pick = self.loop_pick;
                self.z_pick = false;
                self.pz_pick = false;
                self.pick_probe = false;
                self.pick_hover = None;
                if self.loop_pick {
                    self.status = "回路図の、切りたい部品の本体をクリックしてください".into();
                }
            }
            if self.lit(ui.button("ループゲインを実行"), "ループゲインを実行").on_hover_text("帰還ループを切って、電圧と電流の試験信号を1回ずつ入れ、1周したときのゲイン (一巡伝達関数) を求めます。0 dB を横切る所の位相余裕で、安定かどうかが分かります。結果は AC解析と同じボード線図に出ます。周波数範囲・点数は AC解析の欄を共用します").clicked() {
                self.run_loop();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{Circuit, Element};

    fn example(name: &str) -> Circuit {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let s: sch_core::Schematic = serde_json::from_str(&std::fs::read_to_string(root.join(name)).unwrap()).unwrap();
        sim_core::from_schematic(&s).unwrap_or_else(|e| panic!("{name}: {e:?}"))
    }

    #[test]
    fn only_a_loop_with_an_op_amp_model_gets_the_note() {
        let mut c = Circuit::new();
        let (a, b) = (c.node("A"), c.node("B"));
        // a gain source that is not the first stage of a model, and one that is, in two parts
        c.elements.push(Element::Vcvs { name: "E1".into(), p: a, n: 0, cp: b, cn: 0, gain: 2.0 });
        assert!(opamp_model_note(&c).is_none());
        c.elements.push(Element::Vcvs { name: "U2.Eop".into(), p: a, n: 0, cp: b, cn: 0, gain: 1e5 });
        c.elements.push(Element::Vcvs { name: "U1.Eop".into(), p: a, n: 0, cp: b, cn: 0, gain: 1e5 });
        let note = opamp_model_note(&c).expect("a note");
        assert!(note.contains("U1、U2") && note.contains("第2の極") && !note.contains("E1"), "{note}");
    }

    #[test]
    fn the_examples_with_an_op_amp_get_the_note_and_the_passive_ones_do_not() {
        assert!(opamp_model_note(&example("opamp_loop_gain.schjson")).is_some_and(|n| n.contains("U1")));
        assert!(opamp_model_note(&example("rc_lowpass.schjson")).is_none());
        assert!(opamp_model_note(&example("rlc_poles.schjson")).is_none());
    }
}
