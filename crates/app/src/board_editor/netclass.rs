//! The "ネットクラス" panel: groups of nets with their own track width, clearance and via, kept in `Board.net_classes`.

use super::*;
use pcb_core::NetClass;

impl BoardEditor {
    /// The layer stack the impedance is worked out from (`Board.stackup`).
    pub(super) fn panel_stackup(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("層構成 (層数・材料)").default_open(false).show(ui, |ui| {
            self.layer_count_ui(ui);
            ui.separator();
            ui.small("基板の材料と、層の間隔です。配線の太さから特性インピーダンスの目安を出すのに使います。初期値は、ふつうの厚さ 1.6 mm の2層基板の値です。発注先の公式ページにある積層構成 (層の間隔・誘電率・銅箔の厚さ) に直すと、近づきます");
            let total = self.board.inner_layers as usize + 2;
            let mut s = self.board.stackup;
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(&mut s.er).speed(0.05).range(2.0..=12.0).prefix("誘電率 ")).on_hover_text("基板の材料の比誘電率。FR-4 は 4.2〜4.6 ほどで、材料と周波数で変わります");
                ui.add(egui::DragValue::new(&mut s.height).speed(0.01).range(0.05..=3.0).prefix("外層から基準面まで ").suffix(" mm")).on_hover_text("外層の配線から、その下の銅 (2層基板なら反対の面、4層基板なら隣の内層のベタ) までの距離。2層の 1.6 mm 基板なら約 1.53 mm、4層基板なら 0.1〜0.2 mm ほどです");
                ui.add(egui::DragValue::new(&mut s.plane_gap).speed(0.01).range(0.1..=3.0).prefix("内層の上下のベタの間 ").suffix(" mm")).on_hover_text("内層の配線を上下から挟む2枚のベタの間の距離 (ストリップライン)");
                ui.add(egui::DragValue::new(&mut s.copper_um).speed(1.0).range(9.0..=140.0).prefix("銅箔 ").suffix(" µm")).on_hover_text("銅箔の厚さ。1 oz = 35 µm");
            });
            if let Some(w) = pcb_core::impedance::width_for(&s, pcb_core::impedance::Kind::Microstrip, 50.0) {
                ui.small(format!("この構成で、外層の 50 Ω は、線幅 約 {w:.2} mm です"));
            }
            ui.horizontal(|ui| {
                if ui.small_button("この層数の目安を入れる").on_hover_text("2層なら、1.6 mm の基板の裏の銅までの距離 (1.53 mm)。4層以上なら、外層と隣のベタの間が薄い (0.2 mm) 例を入れます。層の間隔は発注先ごとに違うので、あくまで例です。公式ページの値に直してください").clicked() {
                    s.height = if total > 2 { 0.2 } else { pcb_core::impedance::Stackup::default().height };
                }
                if ui.small_button("初期値に戻す").on_hover_text("ふつうの 1.6 mm・2層基板の値に戻します").clicked() {
                    s = pcb_core::impedance::Stackup::default();
                }
            });
            if s != self.board.stackup {
                self.checkpoint_once();
                self.board.stackup = s;
            }
        }).header_response.on_hover_text("基板の層の数と、材料・層の間隔 (誘電率・層の間隔・銅箔)。インピーダンスの目安に使います");
    }

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
                    classes.push(NetClass { name: format!("クラス{}", classes.len() + 1), track_width: self.track_width, clearance: rules.min_clearance, via_diameter: self.via_dia, via_drill: self.via_drill, nets: vec![], impedance: None });
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
                        classes.push(NetClass { name: "電源".into(), nets: ns, track_width: 0.5_f64.max(rules.min_track_width), clearance: 0.25_f64.max(rules.min_clearance), via_diameter: 0.8, via_drill: 0.4, impedance: None });
                    }
                }
            });
            let down = ui.input(|i| i.pointer.any_down());
            self.commit_classes(classes, down);
        }).header_response.on_hover_text("ネットのグループごとに、線幅・間隔・ビアを決めます (中級者向け)");
    }
}
