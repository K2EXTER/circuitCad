//! The window of a net: click copper of a net with the Select tool and this opens, with the current the net carries, the track width
//! that current needs, the net class of the net and its values. What the "配線の太さ" and "ネットクラス" parts of the panel do for every
//! net, for the one that was clicked.

use super::*;
use pcb_core::current::{net_widths, round_up_width, width_for_current, CopperSpec};
use pcb_core::NetClass;

impl BoardEditor {
    /// Put the classes in the board when they differ (one undo step for a whole drag of a value).
    pub(super) fn commit_classes(&mut self, classes: Vec<NetClass>, pointer_down: bool) {
        if classes != self.board.net_classes {
            self.checkpoint_once();
            self.board.net_classes = classes;
        }
        if !pointer_down {
            self.drag_edit = false;
        }
    }

    pub fn net_window(&mut self, ctx: &egui::Context) {
        let Some(net) = self.net_win.clone() else { return };
        if !self.board.pads.iter().any(|p| p.net == net) {
            self.net_win = None;
            return;
        }
        let mut open = true;
        let down = ctx.input(|i| i.pointer.any_down());
        let spec = CopperSpec::default();
        egui::Window::new(format!("ネット {net} の設定")).id(egui::Id::new("net_window")).open(&mut open).default_width(360.0).default_pos([420.0, 120.0]).show(ctx, |ui| {
            ui.small(self.net_caption(&net));
            ui.separator();
            // --- the current and the width it needs
            ui.strong("電流と推奨の線幅");
            let mut amps = self.board.net_currents.get(&net).copied().unwrap_or(0.0);
            ui.horizontal(|ui| {
                ui.label("流れる電流");
                ui.add(egui::DragValue::new(&mut amps).speed(0.05).range(0.0..=30.0).suffix(" A").max_decimals(2)).on_hover_text("このネットに流れる最大の電流。分からなければ、つなぐ部品のデータシートの最大電流を見ます。0 のままなら、推奨の線幅は出しません");
            });
            if amps > 0.0 {
                self.board.net_currents.insert(net.clone(), amps);
            } else {
                self.board.net_currents.remove(&net);
            }
            let mut widen = false;
            let mut set_width: Option<f64> = None;
            if amps > 0.0 {
                let rec = round_up_width(width_for_current(amps, &spec));
                let conn = drc::connectivity(&self.board);
                let now = net_widths(&self.board, &conn, &spec).into_iter().find(|w| w.net == net).and_then(|w| w.thinnest);
                ui.label(egui::RichText::new(format!("推奨の線幅: {rec:.2} mm")).strong()).on_hover_text("銅箔 35µm・温度上昇 10℃ の外層で、この電流を流せる太さ (IPC-2221 の式) を、0.05 mm 単位に切り上げたものです。内層や、厚い銅箔、温度上昇を許す場合は、もっと細くできます");
                match now {
                    None => {
                        ui.small("このネットの配線は、まだありません。配線するときの線幅に使えます");
                    }
                    Some(t) if t + 1e-9 < rec => {
                        ui.colored_label(Color32::from_rgb(230, 90, 70), format!("今の最も細い配線は {t:.2} mm で、足りません"));
                    }
                    Some(t) => {
                        ui.colored_label(Color32::from_rgb(90, 190, 110), format!("今の最も細い配線は {t:.2} mm で、足ります ✔"));
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    if now.is_some_and(|t| t + 1e-9 < rec) && ui.button("配線を推奨幅にする").on_hover_text("このネットの、推奨より細い配線を、推奨の太さまで太くします (元に戻せます)。太くしたあと、DRC で間隔が足りているか確かめましょう").clicked() {
                        widen = true;
                    }
                    if ui.button("この幅で引く").on_hover_text("これから引く、このネットの配線の線幅を、推奨の太さにします (ネットクラスがあれば、そのクラスの線幅を変えます)").clicked() {
                        set_width = Some(rec);
                    }
                });
            } else {
                ui.small("電流を入れると、推奨の線幅が出ます:");
                ui.horizontal(|ui| {
                    for a in [0.5, 1.0, 2.0, 3.0] {
                        if ui.small_button(format!("{a} A")).on_hover_text("この電流を入れます").clicked() {
                            self.board.net_currents.insert(net.clone(), a);
                        }
                    }
                });
            }
            if widen {
                self.widen_net_to_need(&net);
            }
            ui.separator();
            // --- the net class
            ui.strong("このネットの配線ルール (ネットクラス)");
            let rules = self.board.rules.clone();
            let mut classes = self.board.net_classes.clone();
            let current = classes.iter().position(|c| c.nets.contains(&net));
            let label = current.map_or("なし (基板全体の設定)".to_string(), |k| classes[k].name.clone());
            let mut choose: Option<Option<usize>> = None;
            ui.horizontal(|ui| {
                ui.label("クラス");
                egui::ComboBox::from_id_salt("net_window_class").selected_text(label).show_ui(ui, |ui| {
                    if ui.selectable_label(current.is_none(), "なし (基板全体の設定)").clicked() {
                        choose = Some(None);
                    }
                    for (k, c) in classes.iter().enumerate() {
                        if ui.selectable_label(current == Some(k), &c.name).clicked() {
                            choose = Some(Some(k));
                        }
                    }
                }).response.on_hover_text("このネットを入れるクラスを選びます。クラスの線幅・間隔・ビアで、このネットの配線を引き、DRC がチェックします");
            });
            if let Some(c) = choose {
                for cl in classes.iter_mut() {
                    cl.nets.retain(|n| *n != net);
                }
                if let Some(k) = c {
                    classes[k].nets.push(net.clone());
                }
            }
            let current = classes.iter().position(|c| c.nets.contains(&net));
            match current {
                Some(k) => {
                    let others: Vec<String> = classes[k].nets.iter().filter(|n| **n != net).cloned().collect();
                    if !others.is_empty() {
                        ui.small(format!("この値は、同じクラスのネット ({}) にも効きます", others.join("・")));
                    }
                    let c = &mut classes[k];
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::DragValue::new(&mut c.track_width).speed(0.01).range(rules.min_track_width..=5.0).prefix("線幅 ").suffix(" mm")).on_hover_text("このクラスのネットを引く線の太さ。発注先の最小線幅より細くはできません");
                        ui.add(egui::DragValue::new(&mut c.clearance).speed(0.01).range(rules.min_clearance..=5.0).prefix("間隔 ").suffix(" mm")).on_hover_text("ほかのネットの銅が、このクラスの銅から離れる距離");
                        ui.add(egui::DragValue::new(&mut c.via_diameter).speed(0.05).range(0.3..=3.0).prefix("ビア径 ").suffix(" mm")).on_hover_text("このクラスのビアの外径");
                        ui.add(egui::DragValue::new(&mut c.via_drill).speed(0.05).range(0.2..=2.0).prefix("穴 ").suffix(" mm")).on_hover_text("このクラスのビアの穴径");
                    });
                    c.via_drill = c.via_drill.min(c.via_diameter - 0.1).max(0.2);
                    if let Some(w) = set_width {
                        c.track_width = w;
                    }
                }
                None => {
                    ui.small(format!("いまは、基板全体の設定 (線幅 {:.2} mm、ビア {:.2}/{:.2} mm) で引きます", self.track_width, self.via_dia, self.via_drill));
                    if ui.button("このネット専用のクラスを作る").on_hover_text("このネットだけのクラスを作って、入れます。線幅は、推奨の線幅 (電流を入れていれば) か、いまの設定の線幅です。値は、あとで直せます").clicked() {
                        let width = set_width.or_else(|| self.board.net_currents.get(&net).map(|a| round_up_width(width_for_current(*a, &spec)))).unwrap_or(self.track_width).max(rules.min_track_width);
                        classes.push(NetClass { name: net.clone(), nets: vec![net.clone()], track_width: width, clearance: rules.min_clearance, via_diameter: self.via_dia, via_drill: self.via_drill });
                    } else if let Some(w) = set_width {
                        self.track_width = w;
                        self.status = format!("線幅を {w:.2} mm にしました");
                    }
                }
            }
            self.commit_classes(classes, down);
            ui.separator();
            ui.small("基板で、別のネットの配線やパッドをクリックすると、そのネットの設定に切り替わります。何もない所をクリックすると閉じます");
        });
        if !open {
            self.net_win = None;
        }
    }
}
