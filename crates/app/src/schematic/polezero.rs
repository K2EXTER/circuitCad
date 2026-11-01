//! Poles and zeros of a transfer function V(out)/V(in), from a rational fit to the AC response. The fit is drawn over the
//! circuit's own response in the AC viewer, and the poles and zeros are listed in the panel.

use super::*;
use sim_core::{transfer_function, AcSignal, PoleZero, Root};

/// What the panel lists after a run.
pub(super) struct PzSummary {
    pz: PoleZero,
    label: String,
    signature: String,
}

fn describe(r: &Root) -> String {
    match r.q() {
        Some(q) => format!("{} (一対の複素数、Q = {:.2}{})", eng(r.freq_hz(), "Hz"), q, if q > 1.0 { "、山ができて振動する" } else { "" }),
        None => format!("{}{}", eng(r.freq_hz(), "Hz"), if r.pair { " (複素数)" } else { "" }),
    }
}

impl SchEditor {
    pub fn set_pz_settings(&mut self, out: &str, input: &str) {
        self.pz_out = out.into();
        self.pz_in = input.into();
    }

    pub fn analyze_pz(&mut self) {
        self.run_pz();
    }

    /// A click while choosing the net: plain click = the output net, Shift+click = the input net.
    pub(super) fn pick_pz_node(&mut self, pt: Pt, shift: bool) {
        let Some(name) = net_at(&self.sch, pt) else {
            self.status = "そこには接続された配線・ピンがありません。部品のピンか配線の上をクリックしてください".into();
            return;
        };
        if shift {
            self.pz_in = name;
            self.noise_marks.retain(|(_, r)| !*r);
            self.noise_marks.push((pt, true));
            self.status = format!("入力のネットを {} にしました", self.pz_in);
            return;
        }
        self.pz_out = name;
        self.noise_marks.retain(|(_, r)| *r);
        self.noise_marks.push((pt, false));
        self.noise_pick = false;
        self.pz_pick = false;
        self.run_pz();
    }

    pub(super) fn run_pz(&mut self) {
        let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
            self.status = "開始・終了周波数と点数/デケードを数値で入力してください (例: 10, 1Meg, 20)".into();
            return;
        };
        let out = self.pz_out.trim().to_string();
        let input = self.pz_in.trim().to_string();
        let outcome: Result<(AcResult, PoleZero), Vec<String>> = if out.is_empty() {
            Err(vec!["出力のネットの名前を入力してください (「回路図で選ぶ」でも入れられます)".to_string()])
        } else {
            match sim_core::from_schematic_with_layout(&self.sch, &self.wiring, &self.couplings) {
                Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
                Ok(c) => {
                    let s = AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) };
                    transfer_function(&c, &out, if input.is_empty() { None } else { Some(input.as_str()) }, &s, 6, 0.02)
                        .map(|tf| {
                            let sig = |name: &str, v: &[sim_core::C64]| AcSignal { name: name.into(), unit: "V", re: v.iter().map(|z| z.re).collect(), im: v.iter().map(|z| z.im).collect() };
                            let signals = vec![sig("H 回路の応答", &tf.h), sig("H 当てはめ", &tf.pz.fitted)];
                            (AcResult { freq: tf.freq, signals, warnings: tf.warnings }, tf.pz)
                        })
                        .map_err(|e| vec![e.to_string()])
                }
            }
        };
        let label = if input.is_empty() { out.clone() } else { format!("{out} / {input}") };
        let mut selected = BTreeSet::new();
        selected.insert("H 回路の応答".to_string());
        selected.insert("H 当てはめ".to_string());
        let (ac_outcome, summary) = match outcome {
            Ok((ac, pz)) => {
                self.status = format!("極と零: {label} は 極 {} 個・零 {} 個 (当てはめの誤差 {:.2}%)", pz.poles.len(), pz.zeros.len(), pz.error * 100.0);
                (Ok(ac), Some(PzSummary { pz, label, signature: self.signature() }))
            }
            Err(e) => {
                self.status = format!("極と零の計算に失敗: {}", e.first().cloned().unwrap_or_default());
                (Err(e), None)
            }
        };
        self.pz = summary;
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        let ghost = self.ac.as_ref().and_then(|v| v.ghost.clone());
        self.ac = Some(AcView { outcome: ac_outcome, selected, plot: PlotState::log(lo.max(1e-30), hi), ghost, signature: self.signature(), loop_run: false, pz_run: true });
        self.wave_mode = WaveMode::Ac;
    }

    pub(super) fn panel_pz(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("出力");
            ui.add(egui::TextEdit::singleline(&mut self.pz_out).desired_width(56.0).hint_text("OUT"))
                .on_hover_text("伝達関数の出力にするネット。名前が分からないときは下の「回路図で選ぶ」を使います");
            ui.label("入力");
            ui.add(egui::TextEdit::singleline(&mut self.pz_in).desired_width(56.0).hint_text("空欄"))
                .on_hover_text("入力のネット (AC の電源がつながっているネット)。出力 ÷ 入力を伝達関数にします。空欄なら出力の電圧そのもの (入力の電源が「AC 1」のとき、それが伝達関数です)");
        });
        ui.horizontal(|ui| {
            let label = if self.pz_pick { "選択中… (Esc で中止)" } else { "回路図で選ぶ" };
            if ui.selectable_label(self.pz_pick, label).on_hover_text("押してから回路図のピンや配線をクリックすると、そのネットが出力になり、すぐ計算します。Shift+クリックで入力のネット").clicked() {
                self.pz_pick = !self.pz_pick;
                self.noise_pick = self.pz_pick;
                self.z_pick = false;
                self.loop_pick = false;
                self.pick_probe = false;
                self.pick_hover = None;
                if self.pz_pick {
                    self.status = "回路図の出力にしたいネットをクリックしてください".into();
                }
            }
            if self.lit(ui.button("極と零を求める"), "極と零を求める").on_hover_text("AC解析の結果に、分数の形の式 (多項式 ÷ 多項式) を当てはめて、極と零の周波数を求めます。極は、周波数が上がるとゲインが下がり始める所 (1つごとに -20 dB/dec、位相が 90° 遅れる)。零は、ゲインが上がり始める所です。周波数範囲・点数は AC解析の欄を共用します").clicked() {
                self.run_pz();
            }
        });
        let Some(s) = self.pz.as_ref() else { return };
        let stale = s.signature != self.signature();
        ui.separator();
        ui.label(format!("{} の伝達関数{}", s.label, if stale { " (回路が変わりました。もう一度実行してください)" } else { "" }));
        if s.pz.error >= 0.02 {
            ui.colored_label(crate::theme::WARN, format!("当てはめの誤差が {:.1}% あります。この範囲の応答は、6次までの分数の式では表せないか、周波数が足りません。範囲を狭める・点数を増やすと良くなることがあります", s.pz.error * 100.0));
        }
        let db = 20.0 * s.pz.dc_gain.abs().max(1e-15).log10();
        ui.label(format!("低い周波数のゲイン: {:.3} 倍 ({:.1} dB)", s.pz.dc_gain, db)).on_hover_text("周波数が一番低いところの値に近い、直流のゲインです");
        let list = |ui: &mut egui::Ui, title: &str, roots: &[Root], hover: &str| {
            if roots.is_empty() {
                ui.label(format!("{title}: なし")).on_hover_text(hover);
                return;
            }
            for (i, r) in roots.iter().enumerate() {
                let text = format!("{title} {}: {}{}", i + 1, describe(r), if r.right_half() { "  (右半面)" } else { "" });
                let color = if r.right_half() { Color32::from_rgb(230, 80, 60) } else { ui.visuals().text_color() };
                ui.colored_label(color, text).on_hover_text(hover);
            }
        };
        list(ui, "極", &s.pz.poles, "ここから先、周波数が10倍になるごとにゲインが 20 dB ずつ下がり、位相が遅れていきます。複素数の対は、Q が大きいほど山が鋭く、振動しやすい回路です。右半面 (赤) の極があると、回路は自分で発振します");
        list(ui, "零", &s.pz.zeros, "ここから先、ゲインが上がる (または下がり方がゆるくなる) 所です。右半面の零は、ゲインは上がるのに位相は遅れる、やっかいな零です (昇圧型の電源などに出ます)");
        let verdict = if s.pz.poles.iter().any(|p| p.right_half()) {
            "右半面に極があります: この回路は、このままでは不安定です"
        } else if s.pz.poles.iter().any(|p| p.damping().is_some_and(|d| d < 0.3)) {
            "安定ですが、減衰が小さい極の対があります (ステップ応答で長く振動します)"
        } else {
            "すべての極が左半面にあります (安定です)"
        };
        ui.label(verdict);
    }
}
