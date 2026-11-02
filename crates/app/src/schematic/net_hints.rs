//! Net settings made on the schematic (current, differential pair), by the net's label; the board import takes them over.

use super::*;

impl SchEditor {
    /// Settings of the net whose label is selected.
    pub(super) fn label_net_ui(&mut self, ui: &mut egui::Ui) {
        let Some(Sel::Label(i)) = self.selected else { return };
        let Some(shown) = self.sch.labels.get(i).map(|l| l.text.clone()) else { return };
        ui.label("選択中のラベル");
        // edited in a buffer and applied on Enter / focus loss, so a half-typed name never merges two nets
        let id = egui::Id::new(("label_rename", i));
        let mut buf: String = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| shown.clone());
        let r = ui
            .horizontal(|ui| {
                ui.label("名前");
                ui.text_edit_singleline(&mut buf)
            })
            .inner
            .on_hover_text("ネットの名前を書き換えます。Enter で確定。同じ名前のラベルがほかにもあれば、まとめて同じ名前になります (つながりは変わりません)。すでにある別のネットの名前にすると、2つのネットがつながります");
        if r.lost_focus() {
            let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
            let new = buf.trim().to_string();
            if !cancelled && !new.is_empty() && new != shown {
                rename_net(&mut self.sch, &shown, &new);
            }
            ui.data_mut(|d| d.remove_temp::<String>(id));
        } else if r.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, buf));
        } else {
            ui.data_mut(|d| d.remove_temp::<String>(id));
        }
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

/// Give every label named `old` the name `new`, and carry the net's declared current and differential pair along.
fn rename_net(sch: &mut Schematic, old: &str, new: &str) {
    for l in sch.labels.iter_mut().filter(|l| l.text == old) {
        l.text = new.to_string();
    }
    if let Some(a) = sch.net_currents.remove(old) {
        sch.net_currents.entry(new.to_string()).or_insert(a);
    }
    for (p, n) in sch.diff_pairs.iter_mut() {
        for s in [p, n] {
            if s == old {
                *s = new.to_string();
            }
        }
    }
    // the design-requirements table: net names in the net rows and in the "keep apart" sets (row names stay)
    let req = &mut sch.requirements;
    let names = req.net_rules.iter_mut().flat_map(|r| r.nets.iter_mut()).chain(req.apart.iter_mut().flat_map(|k| k.a.iter_mut().chain(k.b.iter_mut())));
    for s in names.filter(|s| *s == old) {
        *s = new.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renaming_a_label_renames_the_whole_net() {
        let mut s = Schematic::default();
        s.labels.push(Label { pos: (0, 0), text: "A".into() });
        s.labels.push(Label { pos: (5, 5), text: "A".into() });
        s.labels.push(Label { pos: (9, 9), text: "B".into() });
        s.net_currents.insert("A".into(), 2.0);
        s.diff_pairs.push(("A".into(), "B".into()));
        s.requirements.net_rules.push(design_spec::NetRule { name: "A".into(), nets: vec!["A".into(), "GN*".into()], ..Default::default() });
        s.requirements.apart.push(design_spec::KeepApart { a: vec!["A".into()], b: vec!["B".into()], ..Default::default() });
        rename_net(&mut s, "A", "C");
        assert_eq!(s.requirements.net_rules[0].nets, ["C", "GN*"]);
        assert_eq!(s.requirements.net_rules[0].name, "A");
        assert_eq!(s.requirements.apart[0].a, ["C"]);
        assert_eq!(s.requirements.apart[0].b, ["B"]);
        assert_eq!(s.labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["C", "C", "B"]);
        assert_eq!(s.net_currents.get("C"), Some(&2.0));
        assert!(!s.net_currents.contains_key("A"));
        assert_eq!(s.diff_pairs, vec![("C".to_string(), "B".to_string())]);
    }

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
