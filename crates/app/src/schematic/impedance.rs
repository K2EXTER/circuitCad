//! Impedance seen from a net (Zout of a rail or a stage, Zin of an input) and its plot.

use super::*;
use sim_core::{impedance_analysis, z_character, z_extrema, ImpedanceResult};

/// A finished impedance sweep.
pub(super) struct ZView {
    outcome: Result<ImpedanceResult, Vec<String>>,
    /// What was looked at, for the title: "OUT" or "OUT 対 GND".
    label: String,
    plot: PlotState,
}

impl SchEditor {
    pub fn analyze_impedance(&mut self) {
        self.run_z();
    }

    pub fn set_impedance_settings(&mut self, node: &str, reference: &str, remove: &str) {
        self.z_node = node.into();
        self.z_ref = reference.into();
        self.z_remove = remove.into();
    }

    /// A click while choosing the net: plain click = the net to look into, Shift+click = the reference net.
    pub(super) fn pick_z_node(&mut self, pt: Pt, shift: bool) {
        let Some(name) = net_at(&self.sch, pt) else {
            self.status = "そこには接続された配線・ピンがありません。部品のピンか配線の上をクリックしてください".into();
            return;
        };
        if shift {
            self.z_ref = if name == "GND" { String::new() } else { name };
            self.noise_marks.retain(|(_, r)| !*r);
            self.noise_marks.push((pt, true));
        } else {
            self.z_node = name;
            self.noise_marks.retain(|(_, r)| *r);
            self.noise_marks.push((pt, false));
        }
        self.noise_pick = false;
        self.z_pick = false;
        self.run_z();
    }

    pub(super) fn run_z(&mut self) {
        let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
            self.status = "開始・終了周波数と点数/デケードを数値で入力してください (例: 10, 1Meg, 20)".into();
            return;
        };
        let node = self.z_node.trim().to_string();
        let reference = self.z_ref.trim().to_string();
        let remove: Vec<String> = self.z_remove.split([',', ' ', '、']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        let outcome = if node.is_empty() {
            Err(vec!["見るネットの名前を入力してください (ネットラベル名など。「回路図で選ぶ」でも入れられます)".to_string()])
        } else {
            match sim_core::from_schematic_with_layout(&self.sch, &self.wiring, &self.couplings) {
                Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
                Ok(c) => {
                    let s = AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) };
                    impedance_analysis(&c, &node, if reference.is_empty() { None } else { Some(reference.as_str()) }, &remove, &s).map_err(|e| vec![e.to_string()])
                }
            }
        };
        let label = if reference.is_empty() { node.clone() } else { format!("{node} 対 {reference}") };
        self.status = match &outcome {
            Ok(r) => {
                let m = r.mag();
                format!("インピーダンス: {label} は {} 〜 {} ({} Hz〜{} Hz)", eng(m.iter().cloned().fold(f64::MAX, f64::min), "Ω"), eng(m.iter().cloned().fold(0.0, f64::max), "Ω"), f0, f1)
            }
            Err(e) => format!("インピーダンス解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        self.z = Some(ZView { outcome, label, plot: PlotState::log(lo.max(1e-30), hi) });
        self.wave_mode = WaveMode::Impedance;
    }

    pub(super) fn panel_z(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("ネット");
            ui.add(egui::TextEdit::singleline(&mut self.z_node).desired_width(56.0).hint_text("OUT"))
                .on_hover_text("どのネットから回路の中を見るか。そのネットに 1 A の交流の電流を流したときの電圧が、インピーダンスです。\n名前が分からないときは下の「回路図で選ぶ」を使います");
            ui.label("基準");
            ui.add(egui::TextEdit::singleline(&mut self.z_ref).desired_width(44.0).hint_text("GND"))
                .on_hover_text("どのネットとの間のインピーダンスか。空欄なら GND との間です");
        });
        ui.horizontal(|ui| {
            ui.label("外す部品");
            ui.add(egui::TextEdit::singleline(&mut self.z_remove).desired_width(90.0).hint_text("RL, V1"))
                .on_hover_text("先に回路から取り除く部品の番号 (カンマで区切る)。\n段の出力インピーダンスを知りたいときは、負荷 (RL など) を外します。\n入力インピーダンスを知りたいときは、信号源とそれに直列の抵抗を外して、その先のネットを見ます。\n空欄なら、電源をすべて切っただけの回路を見ます (電源ラインのインピーダンスはこちら)");
        });
        ui.horizontal(|ui| {
            let label = if self.z_pick { "選択中… (Esc で中止)" } else { "回路図で選ぶ" };
            if ui.selectable_label(self.z_pick, label).on_hover_text("押してから回路図のピンや配線をクリックすると、そのネットを見て、すぐ解析します。\nShift+クリックで基準のネット").clicked() {
                self.z_pick = !self.z_pick;
                self.noise_pick = self.z_pick;
                self.pick_probe = false;
                self.pick_hover = None;
                if self.z_pick {
                    self.status = "回路図の見たいネットをクリックしてください".into();
                }
            }
            if self.lit(ui.button("インピーダンスを実行"), "インピーダンスを実行").on_hover_text("ネットから見たインピーダンス |Z| と位相を周波数ごとに求めます。山 (並列共振) と谷 (直列共振)、抵抗・コイル・コンデンサのどれに見えるかも出ます。周波数範囲・点数は AC解析の欄を共用します").clicked() {
                self.run_z();
            }
        });
    }

    pub(super) fn z_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.z.as_mut() else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong(format!("インピーダンス: {}", view.label)).on_hover_text("0 dB = 1 Ω、20 dB = 10 Ω、-20 dB = 0.1 Ω。電源ラインでは、低いほど電圧が揺れにくく、山は「そこで電圧が揺れやすい周波数」です");
            if ui.button("閉じる").clicked() {
                close = true;
            }
        });
        let ZView { outcome, plot: st, .. } = view;
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(res) => {
                for w in &res.warnings {
                    ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                }
                let (mag, db, ph) = (res.mag(), res.db(), res.phase_deg());
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(250.0);
                        let (first, last) = (0, mag.len() - 1);
                        let kind = |p: f64| match z_character(p) {
                            "inductive" => "コイルのように見える",
                            "capacitive" => "コンデンサのように見える",
                            _ => "抵抗のように見える",
                        };
                        ui.label(format!("{}: {} ({})", eng(res.freq[first], "Hz"), eng(mag[first], "Ω"), kind(ph[first])));
                        ui.label(format!("{}: {} ({})", eng(res.freq[last], "Hz"), eng(mag[last], "Ω"), kind(ph[last])));
                        let ex = z_extrema(&res.freq, &mag, 3.0);
                        if ex.is_empty() {
                            ui.small("山や谷はありません (なだらかに変わります)");
                        } else {
                            egui::Grid::new("z_extrema").striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                                for e in &ex {
                                    let (word, tip) = if e.peak {
                                        ("山", "並列共振: コイルとコンデンサが打ち消し合って、インピーダンスが高くなる周波数。電源ラインでは電圧が揺れやすい所です。バイパスコンデンサを足す・容量の違うものを並べる、で下げます")
                                    } else {
                                        ("谷", "直列共振: コンデンサの容量とその足の細かいインダクタンスが打ち消し合って、インピーダンスが最も低くなる周波数。その上は、足のインダクタンスのせいでコイルのように上がります")
                                    };
                                    ui.label(word).on_hover_text(tip);
                                    ui.label(eng(e.freq, "Hz"));
                                    ui.label(eng(e.ohm, "Ω"));
                                    ui.end_row();
                                }
                            });
                        }
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    let h = rect.height() / 2.0;
                    let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), h - 2.0));
                    let bottom = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h), Vec2::new(rect.width(), h - 2.0));
                    let ms = Style { unit: " dBΩ", x_unit: "Hz", x_name: "f", plain_y: true };
                    let ps = Style { unit: "°", x_unit: "Hz", x_name: "f", plain_y: true };
                    plot::draw_group(ui, top, egui::Id::new("z_mag"), &[Trace { name: "|Z|", color: PALETTE[0], t: &res.freq, v: &db }], st, 0, &ms, false);
                    plot::draw_group(ui, bottom, egui::Id::new("z_phase"), &[Trace { name: "位相", color: PALETTE[0], t: &res.freq, v: &ph }], st, 1, &ps, true);
                });
            }
        }
        if close {
            self.z = None;
        }
    }
}
