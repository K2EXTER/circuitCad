//! Noise analysis and the node picker.

use super::*;

impl SchEditor {
    pub fn analyze_noise(&mut self) {
        self.run_noise();
    }

    pub fn set_noise_settings(&mut self, out: &str, input: &str, temp: &str) {
        self.noise_out = out.into();
        self.noise_in = input.into();
        self.noise_temp = temp.into();
    }

    /// A click at `pt` while choosing the noise output: plain click = output node,
    /// Shift+click = reference node (differential output).
    pub(super) fn pick_noise_node(&mut self, g: (f32, f32), pt: Pt, shift: bool) {
        if self.pick_probe {
            self.probe_node(g, pt, shift);
            return;
        }
        let Some(name) = net_at(&self.sch, pt) else {
            self.status = "そこには接続された配線・ピンがありません。部品のピンか配線の上をクリックしてください".into();
            return;
        };
        if name == "GND" && !shift {
            self.status = "GND は電圧の基準なので出力にできません。測りたいノードをクリックしてください (Shift+クリックで基準に使えます)".into();
            return;
        }
        if shift {
            let first = self.noise_out.split(',').next().unwrap_or("").trim().to_string();
            self.noise_out = if first.is_empty() { name } else { format!("{first},{name}") };
            self.noise_marks.retain(|(_, r)| !*r);
            self.noise_marks.push((pt, true));
        } else {
            self.noise_out = name;
            self.noise_marks.clear();
            self.noise_marks.push((pt, false));
        }
        self.noise_pick = false;
        self.run_noise();
    }

    /// Starts choosing points on the sheet to add to the waveform viewer.
    pub(super) fn start_probe(&mut self) {
        self.noise_pick = true;
        self.pick_probe = true;
        self.pick_hover = None;
        self.status = "ピンや配線をクリック → その点の電圧、部品の本体をクリック (Shift+ピンでも可) → その部品の電流を、波形に追加します。続けて追加できます (Esc か右クリックで終了)".into();
    }

    /// A click while probing puts a signal into the open transient / AC viewer: V(net) for a pin or wire,
    /// I(part) for the body of a part (or Shift+click on one of its pins).
    pub(super) fn probe_node(&mut self, g: (f32, f32), pt: Pt, shift: bool) {
        let pin_owner = || self.sch.components.iter().find(|c| !c.kind.is_power_symbol() && c.pins().iter().any(|(_, p)| *p == pt)).map(|c| c.refdes.clone());
        let sig = if shift && let Some(r) = pin_owner() {
            format!("I({r})")
        } else if let Some(name) = net_at(&self.sch, pt) {
            format!("V({name})")
        } else if let Some(i) = self.comp_at(g).filter(|i| !self.sch.components[*i].kind.is_power_symbol()) {
            format!("I({})", self.sch.components[i].refdes)
        } else {
            self.status = "そこには何もありません。ピンや配線 (電圧) か、部品の本体 (電流) をクリックしてください".into();
            return;
        };
        let added = match self.wave_mode {
            WaveMode::Ac => self.ac.as_mut().is_some_and(|v| {
                let ok = v.outcome.as_ref().is_ok_and(|r| r.signal(&sig).is_some());
                if ok {
                    v.selected.insert(sig.clone());
                }
                ok
            }),
            _ => self.tran.as_mut().is_some_and(|v| {
                let ok = v.outcome.as_ref().is_ok_and(|r| r.signal(&sig).is_some());
                if ok {
                    v.selected.insert(sig.clone());
                }
                ok
            }),
        };
        self.status = if added {
            format!("{sig} を波形に追加しました。続けてクリックできます (Esc か右クリックで終了)")
        } else if sig.starts_with('I') {
            format!("{sig} (電流) は、いま表示中の解析結果にありません。ICの内部モデルなど、電流の波形を持たない部品もあります。先に過渡解析かAC解析を実行したか確かめてください")
        } else {
            format!("{sig} は、いま表示中の解析結果にありません。先に過渡解析かAC解析を実行してください (GND は 0V の基準なので波形はありません)")
        };
    }

    pub(super) fn run_noise(&mut self) {
        let (Some(f0), Some(f1), Some(ppd), Some(temp)) =
            (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd), self.noise_temp.trim().parse::<f64>().ok())
        else {
            self.status = "開始・終了周波数、点数/デケード、温度[℃]を数値で入力してください".into();
            return;
        };
        let (out, inp) = (self.noise_out.trim().to_string(), self.noise_in.trim().to_string());
        let outcome = if out.is_empty() {
            Err(vec!["出力ノードの名前を入力してください (ネットラベル名など。差動は A,B)".to_string()])
        } else {
            match from_schematic(&self.sch) {
                Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
                Ok(c) => {
                    let (out_p, out_n) = match out.split_once(',') {
                        Some((a, b)) => (a.trim().to_string(), Some(b.trim().to_string())),
                        None => (out.clone(), None),
                    };
                    let st = NoiseSettings {
                        sweep: AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) },
                        output: out_p,
                        output_ref: out_n,
                        input: if inp.is_empty() { None } else { Some(inp) },
                        temp_c: temp,
                    };
                    noise_analysis(&c, &st).map_err(|e| vec![e.to_string()])
                }
            }
        };
        let mut selected = BTreeSet::new();
        selected.insert("合計".to_string());
        self.status = match &outcome {
            Ok(r) => format!("ノイズ解析: 出力 {} rms ({} Hz〜{} Hz)", eng(r.out_rms, "V"), f0, f1),
            Err(e) => format!("ノイズ解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        self.noise = Some(NoiseView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi) });
        self.wave_mode = WaveMode::Noise;
    }

    pub(super) fn noise_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.noise.as_mut() else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong("ノイズ解析 (熱雑音・ショット雑音)");
            if ui.button("閉じる").clicked() {
                close = true;
            }
        });
        let NoiseView { outcome, selected, plot: st } = view;
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
                let to_db = |psd: &[f64]| -> Vec<f64> { psd.iter().map(|p| 10.0 * (p.max(1e-60) / 1e-18).log10()).collect() };
                let mut order: Vec<(String, f64)> = vec![("合計".to_string(), res.out_rms)];
                if let Some(r) = res.in_rms {
                    order.push(("入力換算".to_string(), r));
                }
                order.extend(res.contributions.iter().map(|c| (c.name.clone(), c.rms)));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(270.0);
                        ui.label(format!("出力合計: {}", eng(res.out_rms, "V rms")));
                        if let Some(r) = res.in_rms {
                            ui.label(format!("入力換算: {}", eng(r, "rms"))).on_hover_text("出力のノイズを回路の利得で割って入力側に直した値。入力信号がこれより小さいと、ノイズに埋もれます");
                        }
                        if let Some((top, share)) = res.dominant() {
                            let info = describe_source(&top.name);
                            ui.colored_label(Color32::from_rgb(220, 150, 40), format!("最大の原因: {} ({}) — 全体の約 {:.0}%", top.name, info.title, share * 100.0));
                            if !info.remedy.is_empty() {
                                ui.small(format!("対策: {}", info.remedy));
                            }
                        }
                        {
                            crate::icons::help_small(ui, "グラフの読み方", "縦軸は雑音の「密度」で、1Hz の幅あたりの電圧 (dB, 0 dB = 1 nV/√Hz)。60 dB で 1 µV/√Hz。rms 値は、グラフの帯域全体で雑音を足し合わせた実効値です。雑音は互いに無関係に出るので、足すときは二乗して足し (電力の和)、最後に √ します。だから一番大きい原因を減らすのが近道です。素子の名前にカーソルを合わせると、その雑音の原因と対策が出ます。");
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| {
                          // name (tick to draw) | rms | share of the total, one row per source
                          egui::Grid::new("noise_list").striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                            ui.label("");
                            ui.label("rms").on_hover_text("その原因だけが出す雑音の実効値 (1Hz 幅あたりではなく、グラフの帯域全体)");
                            ui.label("割合").on_hover_text("全体の雑音のうち、この原因が占める割合 (電力で足し算)");
                            ui.end_row();
                            for (name, rms) in &order {
                                let mut on = selected.contains(name);
                                let color = order.iter().filter(|(n, _)| selected.contains(n)).position(|(n, _)| n == name).map(|i| PALETTE[i % PALETTE.len()]);
                                let share = if name == "合計" || name == "入力換算" { None } else { Some(res.share(name)) };
                                let label = name.clone();
                                let text = match color {
                                    Some(c) if on => egui::RichText::new(label).color(c),
                                    _ => egui::RichText::new(label),
                                };
                                let resp = ui.checkbox(&mut on, text);
                                if share.is_some() {
                                    let info = describe_source(name);
                                    let kind = if info.kind == NoiseKind::Shot { "ショット雑音" } else { "熱雑音" };
                                    let msg = format!("{} [{kind}]\n{}\n対策: {}", info.title, info.why, info.remedy);
                                    resp.clone().on_hover_text(msg);
                                }
                                if resp.changed() {
                                    if on {
                                        selected.insert(name.clone());
                                    } else {
                                        selected.remove(name);
                                    }
                                }
                                ui.label(eng(*rms, "V"));
                                ui.label(share.map_or(String::new(), |sh| format!("{:.0}%", sh * 100.0)));
                                ui.end_row();
                            }
                          });
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    let chosen: Vec<(&str, Vec<f64>)> = order
                        .iter()
                        .filter(|(n, _)| selected.contains(n))
                        .map(|(n, _)| {
                            let v = match n.as_str() {
                                "合計" => to_db(&res.out_psd),
                                "入力換算" => to_db(res.in_psd.as_deref().unwrap_or(&[])),
                                _ => res.contribution(n).map(|c| to_db(&c.psd)).unwrap_or_default(),
                            };
                            (n.as_str(), v)
                        })
                        .collect();
                    if chosen.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から項目を選んでください", FontId::proportional(14.0), Color32::from_gray(150));
                        return;
                    }
                    let traces: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, v))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v }).collect();
                    let sty = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true };
                    plot::draw_group(ui, rect, egui::Id::new("noise_density"), &traces, st, 0, &sty, true);
                });
            }
        }
        if close {
            self.noise = None;
        }
    }
}
