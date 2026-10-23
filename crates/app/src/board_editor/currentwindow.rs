//! "ネットの電流一覧" window: the current of every net (declared, or from the schematic's simulation), the track width each needs and
//! what is drawn, in one table. The left panel only has a button for it.

use super::*;
use pcb_core::current::{net_widths, round_up_width, CopperSpec};

impl BoardEditor {
    /// The schematic's DC current of `net` came back (`App` answers `net_current_request`): it is declared for that net alone, and
    /// only if the net has none yet.
    pub fn fill_net_current(&mut self, net: &str, currents: Result<Vec<(String, f64)>, String>) {
        let note = match currents {
            Err(e) => format!("回路図のシミュレーションから電流を取れませんでした: {e}。電流を自分で入れると、推奨の線幅が出ます"),
            Ok(list) => match list.iter().find(|(n, _)| n == net).map(|(_, a)| *a) {
                Some(a) if a > 1e-6 && !self.board.net_currents.contains_key(net) => {
                    let a = (a * 1000.0).round() / 1000.0;
                    self.board.net_currents.insert(net.to_string(), a.max(0.001));
                    format!("回路のDC解析の電流を入れました ({a} A)。ピーク電流 (起動時・スイッチング) は入らないので、余裕を見てください")
                }
                Some(_) => "回路のDC解析では、このネットに流れる電流が、ほとんどありません。電流を自分で入れると、推奨の線幅が出ます".into(),
                None => "回路のDC解析に、このネットが見つかりません (回路図とつながりが合っているか、基板の「回路図から取り込み」を確かめてください)".into(),
            },
        };
        self.net_win_note = Some((net.to_string(), note));
    }

    pub fn current_window(&mut self, ctx: &egui::Context) {
        if !self.current_win_open {
            return;
        }
        let mut open = true;
        egui::Window::new("ネットの電流一覧").id(egui::Id::new("current_window")).open(&mut open).default_width(620.0).default_pos([420.0, 160.0]).show(ctx, |ui| {
            let nets = self.board_nets();
            if nets.is_empty() {
                ui.label("回路図から部品を取り込むと、ネットの一覧が出ます");
                return;
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("DC解析の電流を入れる").on_hover_text("回路図のDC解析 (動作点) で各ネットに流れる電流を求めて、全部のネットに入れます (いま入れてある電流は置き換わります)").clicked() {
                    self.currents_request = Some(CurrentSource::Dc);
                }
                if ui.button("過渡解析の電流を入れる").on_hover_text("回路図で最後に実行した過渡解析の、後半の電流の実効値 (RMS) を、各ネットに入れます。先に回路図で過渡解析を実行してください").clicked() {
                    self.currents_request = Some(CurrentSource::Tran);
                }
            });
            let spec = CopperSpec::default();
            let conn = drc::connectivity(&self.board);
            let report = net_widths(&self.board, &conn, &spec);
            let stats = !self.current_stats.is_empty();
            let mut open_net: Option<String> = None;
            egui::ScrollArea::both().max_height(420.0).show(ui, |ui| {
                egui::Grid::new("current_table").striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                    for h in ["ネット", "電流", "推奨の線幅", "今の最も細い配線"] {
                        ui.strong(h);
                    }
                    if stats {
                        ui.strong("実効 (過渡)").on_hover_text("過渡解析の後半の電流の実効値。太さに使うのは、これです");
                        ui.strong("10 ms の最大").on_hover_text("短くても繰り返す山の熱");
                        ui.strong("ピーク").on_hover_text("一瞬の最大値。参考です");
                    }
                    ui.end_row();
                    for net in &nets {
                        if ui.selectable_label(false, net).on_hover_text("クリックで、このネットの設定の窓を開きます").clicked() {
                            open_net = Some(net.clone());
                        }
                        let mut a = self.board.net_currents.get(net).copied().unwrap_or(0.0);
                        ui.add(egui::DragValue::new(&mut a).speed(0.05).range(0.0..=30.0).suffix(" A").max_decimals(3)).on_hover_text("このネットに流れる最大の電流。0 なら調べません");
                        if a > 0.0 {
                            self.board.net_currents.insert(net.clone(), a);
                        } else {
                            self.board.net_currents.remove(net);
                        }
                        match report.iter().find(|w| &w.net == net) {
                            Some(w) => {
                                ui.label(format!("{:.2} mm", round_up_width(w.needed)));
                                match w.thinnest {
                                    None => ui.colored_label(Color32::from_gray(160), "配線なし"),
                                    Some(t) if w.too_thin() => ui.colored_label(Color32::from_rgb(230, 90, 70), format!("{t:.2} mm ✖ 足りません")),
                                    Some(t) => ui.colored_label(Color32::from_rgb(90, 190, 110), format!("{t:.2} mm ✔")),
                                };
                            }
                            None => {
                                ui.label("—");
                                ui.label("—");
                            }
                        }
                        if stats {
                            match self.current_stats.iter().find(|s| &s.net == net) {
                                Some(s) => {
                                    ui.label(format!("{:.3} A", s.rms));
                                    ui.label(format!("{:.3} A", s.burst_rms));
                                    let txt = format!("{:.3} A{}", s.peak, if s.spiky() { " ⚠" } else { "" });
                                    if s.spiky() {
                                        ui.colored_label(Color32::from_rgb(220, 160, 40), txt).on_hover_text("ピークが実効値の10倍以上です。起動時のサージやスパイクかもしれません。実物では電源の内部抵抗・ESR・ソフトスタートが抑えるので、そのまま太さにすると必要以上に太くなります");
                                    } else {
                                        ui.label(txt);
                                    }
                                }
                                None => {
                                    ui.label("—");
                                    ui.label("—");
                                    ui.label("—");
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
            });
            let thin: Vec<String> = report.iter().filter(|w| w.too_thin()).map(|w| w.net.clone()).collect();
            if !thin.is_empty() && ui.button(format!("足りない {} ネットを推奨幅にする", thin.len())).on_hover_text("電流に対して細いネットの配線を、すべて必要な太さまで太くします (Ctrl+Z で戻せます)。太くしたあと、DRC で間隔が足りるか確かめましょう").clicked() {
                for n in thin {
                    self.widen_net_to_need(&n);
                }
            }
            ui.small("太さの目安は、銅箔 35µm・温度上昇 10℃ の外層 (IPC-2221) です。ピーク電流 (起動時・スイッチング) は入らないので、余裕を見てください");
            if let Some(n) = open_net {
                self.open_net_window(&n);
            }
        });
        if !open {
            self.current_win_open = false;
        }
    }
}
