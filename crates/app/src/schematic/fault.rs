//! Fault simulation panel: break one part and see what changes at DC.

use super::*;
use sim_core::{breakable, simulate_fault, simulate_fault_ac, simulate_fault_tran, with_fault, AcFaultReport, Fault, FaultReport, TranFaultReport};

/// The last fault run.
pub(super) struct FaultView {
    pub(super) report: Result<FaultReport, Vec<String>>,
}

/// How a fault is compared: at DC (voltages), in time (waveform) or in frequency (gain).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FaultDomain {
    Dc,
    Tran,
    Ac,
}

pub(super) enum FaultWave {
    Tran(TranFaultReport),
    Ac(AcFaultReport),
}

/// The last waveform fault run (time or frequency domain): the node's waveform healthy and broken, drawn on top of each other.
pub(super) struct FaultWaveView {
    pub(super) report: Result<FaultWave, Vec<String>>,
    plot: PlotState,
}

/// One line of the "break every part" table.
pub(super) struct ScanRow {
    part: String,
    fault: Fault,
    /// Largest voltage change at any node (V); `f64::INFINITY` when the broken circuit cannot be solved.
    pub(super) severity: f64,
    hot: bool,
}

const ALL_FAULTS: [Fault; 4] = [Fault::Open, Fault::Short, Fault::Scale(10.0), Fault::Scale(0.1)];

impl SchEditor {
    pub fn set_fault(&mut self, part: &str, kind: usize) {
        self.fault_part = part.into();
        self.fault_kind = kind.min(ALL_FAULTS.len() - 1);
    }

    pub fn analyze_fault(&mut self) {
        self.run_fault();
    }

    pub fn analyze_fault_scan(&mut self) {
        self.run_fault_scan();
    }

    /// The node to watch: the one chosen, else a likely output (OUT / VOUT / VDC ...), else the first net.
    pub(super) fn fault_watch_node(&self) -> Option<String> {
        if !self.fault_node.trim().is_empty() {
            return Some(self.fault_node.trim().to_string());
        }
        let nets: Vec<String> = extract(&self.sch).into_iter().map(|n| n.name).filter(|n| !n.contains('.') && !n.eq_ignore_ascii_case("GND")).collect();
        ["OUT", "VOUT", "VDC", "OUTPUT"].iter().find_map(|w| nets.iter().find(|n| n.eq_ignore_ascii_case(w)).cloned()).or_else(|| nets.first().cloned())
    }

    pub fn set_fault_tran(&mut self, node: &str) {
        self.fault_domain = FaultDomain::Tran;
        self.fault_node = node.into();
    }

    pub fn set_fault_ac(&mut self, node: &str) {
        self.fault_domain = FaultDomain::Ac;
        self.fault_node = node.into();
    }

    pub(super) fn run_fault_wave(&mut self) {
        let fault = ALL_FAULTS[self.fault_kind.min(ALL_FAULTS.len() - 1)];
        let ac = self.fault_domain == FaultDomain::Ac;
        let report = (|| {
            let node = self.fault_watch_node().ok_or_else(|| vec!["見る点 (ノード) がありません。回路に名前のついた点 (ラベルなど) を付けてください".to_string()])?;
            let c = self.sim_circuit()?;
            let part = self.fault_part.trim();
            if ac {
                let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
                    return Err(vec!["AC解析の開始・終了周波数と点数を数値で入力してください (例: 10, 1Meg, 20)".to_string()]);
                };
                let set = AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) };
                simulate_fault_ac(&c, part, fault, &set, &node).map(FaultWave::Ac).map_err(|e| vec![e.to_string()])
            } else {
                let (Some(tstop), Some(tstep)) = (parse_value(&self.tran_tstop), parse_value(&self.tran_tstep)) else {
                    return Err(vec!["過渡解析の終了時間と刻みを数値で入力してください (例: 10m, 10u)".to_string()]);
                };
                simulate_fault_tran(&c, part, fault, &TranSettings { tstop, tstep, uic: self.tran_uic }, &node).map(FaultWave::Tran).map_err(|e| vec![e.to_string()])
            }
        })();
        self.status = match &report {
            Ok(_) => format!("故障シミュレーション ({}): {} を{}", if ac { "AC" } else { "過渡" }, self.fault_part, fault.label()),
            Err(e) => format!("故障シミュレーションに失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let plot = if ac {
            let (f0, f1) = (parse_value(&self.ac_fstart).unwrap_or(10.0), parse_value(&self.ac_fstop).unwrap_or(1e6));
            PlotState::log(f0.max(1e-30), f1.max(f0 * 1.001))
        } else {
            PlotState::new(parse_value(&self.tran_tstop).unwrap_or(1.0))
        };
        self.fault_wave = Some(FaultWaveView { report, plot });
        self.wave_mode = WaveMode::Fault;
    }

    pub(super) fn fault_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.fault_wave.as_mut() else { return };
        let mut close = false;
        let FaultWaveView { report, plot: st } = view;
        ui.horizontal(|ui| {
            ui.strong(match report {
                Ok(FaultWave::Ac(_)) => "故障シミュレーション (AC): 周波数特性の比較",
                _ => "故障シミュレーション (過渡): 正常と故障の比較",
            });
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.small("白い線 = 正常な回路 / 色つきの線 = 部品を壊した回路。クリックで測定カーソル");
        });
        match report {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(r) => {
                let (lines, window) = match r {
                    FaultWave::Tran(t) => (t.summary(), Some(t.window)),
                    FaultWave::Ac(a) => (a.summary(), None),
                };
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        for (k, l) in lines.iter().enumerate() {
                            if k == 0 {
                                ui.strong(l);
                            } else {
                                ui.label(l);
                            }
                        }
                        if let Some(w) = window {
                            ui.small(format!("測った範囲: {} 〜 {}", eng(w.0, "s"), eng(w.1, "s")));
                        }
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    match r {
                        FaultWave::Tran(r) => {
                            let (h_name, f_name) = (format!("V({}) 正常", r.node), format!("V({}) {}", r.node, r.fault.label()));
                            let mut traces = vec![Trace { name: &h_name, color: Color32::from_gray(225), t: &r.healthy_time, v: &r.healthy }];
                            if let Ok((t, v)) = &r.faulted {
                                traces.push(Trace { name: &f_name, color: PALETTE[0], t, v });
                            }
                            let style = Style { unit: "V", x_unit: "s", x_name: "t", plain_y: false };
                            plot::draw_group(ui, rect, egui::Id::new("fault_plot"), &traces, st, 0, &style, true);
                        }
                        FaultWave::Ac(r) => {
                            let (h_name, f_name) = (format!("V({}) 正常", r.node), format!("V({}) {}", r.node, r.fault.label()));
                            let mut traces = vec![Trace { name: &h_name, color: Color32::from_gray(225), t: &r.freq, v: &r.healthy_db }];
                            if let Ok(v) = &r.faulted_db {
                                traces.push(Trace { name: &f_name, color: PALETTE[0], t: &r.freq, v });
                            }
                            let style = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true };
                            plot::draw_group(ui, rect, egui::Id::new("fault_plot_ac"), &traces, st, 0, &style, true);
                        }
                    }
                });
            }
        }
        if close {
            self.fault_wave = None;
        }
    }

    pub(super) fn run_fault(&mut self) {
        if self.fault_domain != FaultDomain::Dc {
            self.run_fault_wave();
            return;
        }
        let fault = ALL_FAULTS[self.fault_kind.min(ALL_FAULTS.len() - 1)];
        let report = match self.sim_circuit() {
            Err(errs) => Err(errs),
            Ok(c) => simulate_fault(&c, self.fault_part.trim(), fault).map_err(|e| vec![e.to_string()]),
        };
        self.status = match &report {
            Ok(r) => format!("故障シミュレーション: {} を{}", r.element, fault.label()),
            Err(e) => format!("故障シミュレーションに失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.fault_view = Some(FaultView { report });
    }

    pub(super) fn run_fault_scan(&mut self) {
        let Ok(c) = self.sim_circuit() else {
            self.status = "回路を計算できないため、故障を試せません。先に DC動作点を解析してエラーを直してください".into();
            return;
        };
        let mut rows = vec![];
        for part in breakable(&c) {
            for fault in ALL_FAULTS {
                if with_fault(&c, &part, fault).is_none() {
                    continue;
                }
                let Ok(r) = simulate_fault(&c, &part, fault) else { continue };
                let severity = match &r.faulted {
                    Err(_) => f64::INFINITY,
                    Ok(_) => r.node_changes.iter().map(|(_, a, b)| (b - a).abs()).fold(0.0, f64::max),
                };
                let hot = r.part_changes.iter().any(|p| p.power.1 > 0.25 && p.power.1 > 2.0 * p.power.0.max(0.0));
                rows.push(ScanRow { part: part.clone(), fault, severity, hot });
            }
        }
        rows.sort_by(|a, b| b.severity.total_cmp(&a.severity));
        self.status = format!("故障シミュレーション: {} 通りを試しました", rows.len());
        self.fault_scan = rows;
    }

    /// Shows the broken circuit's voltages on the sheet.
    pub(super) fn show_fault_on_sheet(&mut self) {
        let Some(FaultView { report: Ok(r) }) = &self.fault_view else { return };
        let Ok(f) = &r.faulted else { return };
        let label = format!("{} を{}", r.element, r.fault.label());
        let view = self.dc_view(Ok(f.clone()), Some(label));
        self.dc = Some(view);
        self.show_v = true;
    }

    pub(super) fn panel_fault(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("故障シミュレーション (壊れたらどうなる?)");
        ui.small("部品を1つ壊した状態 (断線・ショート・値の間違い) で DC を解き、元の回路と比べます。「どの部品が壊れると危ないか」を、実物を壊さずに学べます");
        let parts: Vec<String> = self.sim_circuit().map(|c| breakable(&c)).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("部品");
            let shown = if self.fault_part.is_empty() { "選ぶ".to_string() } else { self.fault_part.clone() };
            egui::ComboBox::from_id_salt("fault_part")
                .width(70.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for p in &parts {
                        ui.selectable_value(&mut self.fault_part, p.clone(), p);
                    }
                })
                .response
                .on_hover_text("壊す部品 (抵抗・コンデンサ・コイル・ダイオード/LED・トランジスタ)。IC の中の部品や電源は壊せません");
            if let Some(Sel::Comp(i)) = self.selected
                && let Some(c) = self.sch.components.get(i)
                && parts.contains(&c.refdes)
                && c.refdes != self.fault_part
                && ui.small_button("選択中の部品").on_hover_text("回路図で選んでいる部品を壊す対象にします").clicked()
            {
                self.fault_part = c.refdes.clone();
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (k, f) in ALL_FAULTS.iter().enumerate() {
                let tip = match f {
                    Fault::Open => "足が外れた・はんだ不良・パターン切れ。その部品が回路から消えた状態にします",
                    Fault::Short => "足同士がつながった (はんだブリッジ・部品の破壊)。ごく小さな抵抗でつなぎます",
                    Fault::Scale(x) if *x > 1.0 => "値の読み間違い・部品の取り違え。値を10倍にします (抵抗・コンデンサ・コイルのみ)",
                    _ => "値の読み間違い・部品の取り違え。値を1/10にします (抵抗・コンデンサ・コイルのみ)",
                };
                ui.selectable_value(&mut self.fault_kind, k, f.label()).on_hover_text(tip);
            }
        });
        ui.horizontal(|ui| {
            ui.label("比べる");
            ui.selectable_value(&mut self.fault_domain, FaultDomain::Dc, "DC (電圧)").on_hover_text("落ち着いたあとの電圧と電流を比べます。素早く、どこが危ないか分かります");
            ui.selectable_value(&mut self.fault_domain, FaultDomain::Tran, "過渡 (波形)").on_hover_text("時間を追った波形を、正常な回路と重ねて比べます。リップルや発振、点滅の変化など、DCでは見えない故障が分かります。過渡解析の終了時間・刻みの設定を使います");
            ui.selectable_value(&mut self.fault_domain, FaultDomain::Ac, "AC (周波数特性)").on_hover_text("周波数ごとのゲインを、正常な回路と重ねて比べます。フィルタのカットオフが動く、増幅度が変わる、共振が現れるなどが分かります。AC解析の周波数の設定を使います。入力の電源は「AC 1」にしておきます");
        });
        if self.fault_domain != FaultDomain::Dc {
            ui.horizontal(|ui| {
                ui.label("見る点");
                let nets: Vec<String> = extract(&self.sch).into_iter().map(|n| n.name).filter(|n| !n.contains('.')).collect();
                let shown = self.fault_watch_node().unwrap_or_else(|| "選ぶ".to_string());
                egui::ComboBox::from_id_salt("fault_node")
                    .width(80.0)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for n in &nets {
                            ui.selectable_value(&mut self.fault_node, n.clone(), n);
                        }
                    })
                    .response
                    .on_hover_text("波形を比べるノード (ネット) の名前。出力にあたる点を選びます。選ばないときは OUT などを自動で探します");
            });
        }
        ui.horizontal(|ui| {
            if ui.button("故障を試す").on_hover_text("選んだ部品を壊して DC 解析し、変わった電圧と電流を日本語で説明します。回路そのものは変わりません").clicked() {
                self.run_fault();
            }
            if ui.button("全部品を順に壊す").on_hover_text("すべての部品を、断線・ショート・10倍・1/10 で壊して、影響の大きい順に並べます。クリックで詳しく見られます").clicked() {
                self.run_fault_scan();
            }
        });
        let mut show_sheet = false;
        let mut restore = false;
        if let Some(view) = &self.fault_view {
            match &view.report {
                Err(msgs) => {
                    for m in msgs {
                        ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                    }
                }
                Ok(r) => {
                    let lines = r.summary();
                    for (k, l) in lines.iter().enumerate() {
                        if k == 0 {
                            ui.strong(l);
                        } else {
                            ui.label(l);
                        }
                    }
                    ui.horizontal(|ui| {
                        if r.faulted.is_ok() && ui.button("故障時の電圧を回路図に表示").on_hover_text("回路図の電圧表示を、壊れた状態の値に切り替えます。元に戻すには右のボタンか、DC解析を実行します").clicked() {
                            show_sheet = true;
                        }
                        if ui.button("元に戻す (DC解析)").clicked() {
                            restore = true;
                        }
                    });
                }
            }
        }
        if show_sheet {
            self.show_fault_on_sheet();
        }
        if restore {
            self.run_dc();
        }
        if !self.fault_scan.is_empty() {
            let mut pick = None;
            egui::CollapsingHeader::new("影響の大きい順").default_open(true).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    for (k, row) in self.fault_scan.iter().enumerate() {
                        let effect = if row.severity.is_infinite() {
                            "計算できない (大電流/不定)".to_string()
                        } else if row.severity < 0.01 {
                            "DCでは変化なし".to_string()
                        } else {
                            format!("電圧が最大 {:.2} V 変化", row.severity)
                        };
                        let text = format!("{} {}: {}{}", row.part, row.fault.label(), effect, if row.hot { " 🔥発熱" } else { "" });
                        let col = if row.severity.is_infinite() || row.hot { Color32::from_rgb(230, 90, 70) } else if row.severity < 0.01 { Color32::from_gray(140) } else { Color32::from_rgb(220, 190, 80) };
                        if ui.selectable_label(false, egui::RichText::new(text).color(col)).on_hover_text("クリックで、この故障の詳しい結果を表示します").clicked() {
                            pick = Some(k);
                        }
                    }
                });
            });
            if let Some(k) = pick {
                let (part, fault) = (self.fault_scan[k].part.clone(), self.fault_scan[k].fault);
                self.fault_part = part;
                self.fault_kind = ALL_FAULTS.iter().position(|f| *f == fault).unwrap_or(0);
                self.run_fault();
            }
        }
    }
}
