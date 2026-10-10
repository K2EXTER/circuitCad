//! "配線の太さ" section of the board panel: the current each net carries, the track width that
//! needs (IPC-2221), and a button that widens the tracks to it.

use super::*;
use pcb_core::current::{net_widths, round_up_width, widen_net, CopperSpec};

impl BoardEditor {
    /// Net names that appear on pads, in order of first appearance.
    pub(super) fn board_nets(&self) -> Vec<String> {
        let mut nets: Vec<String> = vec![];
        for p in &self.board.pads {
            if !p.net.is_empty() && !nets.contains(&p.net) {
                nets.push(p.net.clone());
            }
        }
        nets
    }

    /// Widens every track of `net` to the width its declared current needs. Returns how many were changed.
    pub(super) fn widen_net_to_need(&mut self, net: &str) -> usize {
        let Some(&amps) = self.board.net_currents.get(net) else { return 0 };
        let width = round_up_width(pcb_core::current::width_for_current(amps, &CopperSpec::default()));
        let conn = drc::connectivity(&self.board);
        let would = self.board.tracks.iter().enumerate().filter(|(i, t)| conn.track_net(*i) == Some(net) && t.width + 1e-9 < width).count();
        if would == 0 {
            return 0;
        }
        self.checkpoint();
        let n = widen_net(&mut self.board, &conn, net, width);
        self.status = format!("{net}: 配線 {n} 本を {width:.2} mm に太くしました。DRC で間隔が足りているか確かめましょう");
        n
    }

    pub(super) fn panel_widths(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("配線の太さ (電流から)").default_open(false).show(ui, |ui| {
            let nets = self.board_nets();
            if nets.is_empty() {
                ui.small("回路図から部品を取り込むと、ネットの一覧が出ます");
                return;
            }
            let spec = CopperSpec::default();
            let conn = drc::connectivity(&self.board);
            let report = net_widths(&self.board, &conn, &spec);
            let mut widen: Vec<String> = vec![];
            let mut use_width = None;
            egui::Grid::new("net_widths").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                for net in &nets {
                    ui.label(net);
                    let mut a = self.board.net_currents.get(net).copied().unwrap_or(0.0);
                    let r = ui.add(egui::DragValue::new(&mut a).speed(0.05).range(0.0..=30.0).suffix(" A").max_decimals(2));
                    r.on_hover_text("このネットに流れる最大の電流。0 のままなら調べません。分からなければ、つなぐ部品のデータシートの最大電流を見ます");
                    if a > 0.0 {
                        self.board.net_currents.insert(net.clone(), a);
                    } else {
                        self.board.net_currents.remove(net);
                    }
                    match report.iter().find(|w| &w.net == net) {
                        Some(w) => {
                            let rec = round_up_width(w.needed);
                            ui.horizontal(|ui| {
                                let (txt, col) = match w.thinnest {
                                    None => (format!("必要 {rec:.2} mm (配線なし)"), egui::Color32::from_gray(160)),
                                    Some(t) if w.too_thin() => (format!("必要 {rec:.2} mm / 今 {t:.2} mm"), egui::Color32::from_rgb(230, 90, 70)),
                                    Some(t) => (format!("必要 {rec:.2} mm / 今 {t:.2} mm ✔"), egui::Color32::from_rgb(90, 190, 110)),
                                };
                                ui.colored_label(col, txt).on_hover_text(format!("IPC-2221 の式で、{:.2} A を温度上昇 10℃ に収めて流すのに必要な幅は {:.3} mm です。丸めて {rec:.2} mm", w.amps, w.needed));
                                if w.too_thin() && ui.small_button("推奨幅にする").on_hover_text("このネットの細い配線を、必要な幅まで太くします (Ctrl+Z で戻せます)。太くした結果、隣と近づきすぎることがあるので、あとで DRC を実行してください").clicked() {
                                    widen.push(net.clone());
                                }
                                if ui.small_button("線幅に").on_hover_text("これから引く配線の線幅 (上の「線幅」) を、この値にします").clicked() {
                                    use_width = Some(rec);
                                }
                            });
                        }
                        None => {
                            ui.small("");
                        }
                    }
                    ui.end_row();
                }
            });
            if let Some(w) = use_width {
                self.track_width = w;
            }
            for n in widen {
                self.widen_net_to_need(&n);
            }
            if report.iter().any(|w| w.too_thin()) && ui.button("足りないネットをすべて推奨幅にする").clicked() {
                let thin: Vec<String> = report.iter().filter(|w| w.too_thin()).map(|w| w.net.clone()).collect();
                for n in thin {
                    self.widen_net_to_need(&n);
                }
            }
        }).header_response.on_hover_text("ネットごとに流す電流を入れると、必要な線幅を計算し、足りない配線を教えます (銅箔 35µm・温度上昇 10℃ の外層)。電源やモーターの線は、信号線より太くします");
    }
}
