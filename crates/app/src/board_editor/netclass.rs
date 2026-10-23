//! The "ネットクラス" panel: groups of nets with their own track width, clearance and via, kept in `Board.net_classes`.

use super::*;
use pcb_core::NetClass;

impl BoardEditor {
    pub(super) fn panel_net_classes(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("ネットクラス (線幅・間隔・ビア)").default_open(false).show(ui, |ui| {
            ui.small("電源は太く広く、信号は細く、のように、ネットのグループごとに配線のルールを決めます。配線ツールと自動配線は、そのネットをクラスの線幅・ビアで引き、DRC はクラスの間隔を守っているか調べます");
            let rules = self.board.rules.clone();
            let nets = self.board_nets();
            let mut classes = self.board.net_classes.clone();
            let mut remove = None;
            for (k, c) in classes.iter_mut().enumerate() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(120.0).hint_text("クラスの名前")).on_hover_text("クラスの名前 (例: 電源、信号、高速)");
                        if ui.small_button("このクラスを削除").on_hover_text("クラスをなくします。配線はそのままで、ネットは基板全体の設定に戻ります").clicked() {
                            remove = Some(k);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::DragValue::new(&mut c.track_width).speed(0.01).range(rules.min_track_width..=5.0).prefix("線幅 ").suffix(" mm")).on_hover_text("このクラスのネットを引く線の太さ。発注先の最小線幅より細くはできません");
                        ui.add(egui::DragValue::new(&mut c.clearance).speed(0.01).range(rules.min_clearance..=5.0).prefix("間隔 ").suffix(" mm")).on_hover_text("ほかのネットの銅が、このクラスの銅から離れる距離。高電圧や、ノイズを嫌う線は広く。基板全体の最小間隔より狭くはできません");
                        ui.add(egui::DragValue::new(&mut c.via_diameter).speed(0.05).range(0.3..=3.0).prefix("ビア径 ").suffix(" mm")).on_hover_text("このクラスのビアの外径");
                        ui.add(egui::DragValue::new(&mut c.via_drill).speed(0.05).range(0.2..=2.0).prefix("穴 ").suffix(" mm")).on_hover_text("このクラスのビアの穴径");
                    });
                    c.via_drill = c.via_drill.min(c.via_diameter - 0.1).max(0.2);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("ネット:");
                        for n in c.nets.clone() {
                            if ui.small_button(format!("{n} ×")).on_hover_text("このネットをクラスから外します").clicked() {
                                c.nets.retain(|x| *x != n);
                            }
                        }
                        egui::ComboBox::from_id_salt(("class_add", k)).selected_text("＋ ネットを足す").show_ui(ui, |ui| {
                            for n in &nets {
                                if !c.nets.contains(n) && ui.selectable_label(false, n).clicked() {
                                    c.nets.push(n.clone());
                                }
                            }
                        }).response.on_hover_text("クラスに入れるネットを選びます。1つのネットが入れるクラスは、先に並んだ1つだけです");
                    });
                });
            }
            if let Some(k) = remove {
                classes.remove(k);
            }
            ui.horizontal(|ui| {
                if ui.button("クラスを追加").on_hover_text("新しい空のクラスを作ります。ネットを足して、線幅・間隔・ビアを決めます").clicked() {
                    classes.push(NetClass { name: format!("クラス{}", classes.len() + 1), track_width: self.track_width, clearance: rules.min_clearance, via_diameter: self.via_dia, via_drill: self.via_drill, nets: vec![] });
                }
                if ui.button("電源のクラスを作る").on_hover_text("GND と、流れる電流を決めたネットを入れた「電源」クラスを作ります (線幅 0.5mm、間隔 0.25mm、ビア 0.8/0.4mm)。値はあとで直せます").clicked() {
                    let mut ns: Vec<String> = self.board.net_currents.keys().cloned().collect();
                    if nets.iter().any(|n| n == "GND") && !ns.iter().any(|n| n == "GND") {
                        ns.push("GND".into());
                    }
                    ns.retain(|n| !classes.iter().any(|c| c.nets.contains(n)));
                    if ns.is_empty() {
                        self.status = "電源のクラスに入れるネットがありません (GND が無いか、すでに別のクラスにあります)。ネットは、クラスの「ネットを足す」から選べます".into();
                    } else {
                        classes.push(NetClass { name: "電源".into(), nets: ns, track_width: 0.5_f64.max(rules.min_track_width), clearance: 0.25_f64.max(rules.min_clearance), via_diameter: 0.8, via_drill: 0.4 });
                    }
                }
            });
            let down = ui.input(|i| i.pointer.any_down());
            self.commit_classes(classes, down);
        }).header_response.on_hover_text("ネットのグループごとに、線幅・間隔・ビアを決めます (中級者向け)");
    }
}
