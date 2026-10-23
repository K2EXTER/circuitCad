//! Net settings made on the schematic (current, differential pair), by the net's label; the board import takes them over.

use super::*;

impl SchEditor {
    /// Settings of the net whose label is selected.
    pub(super) fn label_net_ui(&mut self, ui: &mut egui::Ui) {
        let Some(Sel::Label(i)) = self.selected else { return };
        let Some(net) = self.sch.labels.get(i).map(|l| l.text.clone()).filter(|t| !t.is_empty()) else { return };
        ui.label(format!("選択中のネット: {net}"));
        ui.horizontal(|ui| {
            ui.label("流れる電流");
            let mut amps = self.sch.net_currents.get(&net).copied().unwrap_or(0.0);
            let r = ui
                .add(egui::DragValue::new(&mut amps).speed(0.01).range(0.0..=100.0).suffix(" A").max_decimals(3))
                .on_hover_text("このネットに流れる電流の目安。基板に取り込むと、配線の太さの推奨や電圧降下の計算に使われます。0 にすると指定なし (基板側で決めた値はそのまま)");
            if r.changed() {
                if amps > 0.0 {
                    self.sch.net_currents.insert(net.clone(), amps);
                } else {
                    self.sch.net_currents.remove(&net);
                }
            }
        });
        let mut names: Vec<String> = self.sch.labels.iter().map(|l| l.text.clone()).filter(|t| !t.is_empty() && *t != net).collect();
        names.sort();
        names.dedup();
        let partner = |s: &Schematic| s.diff_pairs.iter().find_map(|(p, n)| if *p == net { Some(n.clone()) } else if *n == net { Some(p.clone()) } else { None });
        let now = partner(&self.sch);
        let mut pick = now.clone();
        ui.horizontal(|ui| {
            ui.label("差動ペアの相手");
            egui::ComboBox::from_id_salt("net_pair")
                .selected_text(pick.clone().unwrap_or_else(|| "なし".into()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut pick, None, "なし");
                    for n in &names {
                        ui.selectable_value(&mut pick, Some(n.clone()), n);
                    }
                })
                .response
                .on_hover_text("USB の D+ と D− のように、2本で1つの信号を運ぶネットの相手を選びます。基板に取り込むと差動ペアとして登録され、並べて配線・長さ合わせの対象になります");
        });
        if pick != now {
            self.sch.diff_pairs.retain(|(p, n)| *p != net && *n != net);
            if let Some(o) = pick {
                self.sch.diff_pairs.retain(|(p, n)| *p != o && *n != o);
                self.sch.diff_pairs.push((net, o));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_survive_a_round_trip_and_reach_the_board() {
        let mut s = Schematic::default();
        s.net_currents.insert("VIN".into(), 1.5);
        s.diff_pairs.push(("D+".into(), "D-".into()));
        let back: Schematic = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.net_currents, s.net_currents);
        assert_eq!(back.diff_pairs, s.diff_pairs);
        assert!(!serde_json::to_string(&Schematic::default()).unwrap().contains("diff_pairs"), "empty settings are not written");
    }
}
