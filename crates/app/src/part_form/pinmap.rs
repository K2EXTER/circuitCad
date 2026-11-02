//! "ノード順のピン番号" by picking: each terminal of the SPICE model gets a pin of the pin table from a drop-down.

use super::*;

/// Shown in the list for a terminal that has no pin yet (also what is written into `sim_pins`, so the form says what is missing).
const OPEN: &str = "?";

impl PartForm {
    /// One row per terminal of the pasted model, each with a drop-down of the part's pins. Writes `sim_pins`.
    pub(super) fn pin_map_ui(&mut self, ui: &mut egui::Ui) {
        let Ok(g) = guess_spice(&self.sim_text) else { return };
        if g.roles.is_empty() {
            return;
        }
        let pins = self.parse_pins().unwrap_or_default();
        if pins.is_empty() {
            return;
        }
        let mut cur: Vec<String> = self.sim_pins.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(String::from).collect();
        cur.resize(g.roles.len().max(cur.len()), OPEN.into());
        let mut changed = false;
        ui.add_space(4.0);
        crate::icons::help_label(
            ui,
            "モデルの端子とピンの対応",
            "モデルの端子 (SPICE の文に書かれた順) が、部品のどのピンにあたるかを選びます。ピン表のすべてのピンを選ぶ必要はありません。選ばなかったピンは、シミュレーションではどこにもつながらない扱いです (8ピンのパッケージに3端子のモデルを使うときなど)",
        );
        egui::Grid::new("pin_map").num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
            for (i, role) in g.roles.iter().enumerate() {
                ui.label(format!("端子 {} ({})", i + 1, role));
                ui.label("→");
                let shown = match pins.iter().find(|p| p.number == cur[i]) {
                    Some(p) => format!("{}  {}", p.number, p.name),
                    None => "(ピンを選ぶ)".to_string(),
                };
                let twice = cur[i] != OPEN && cur.iter().filter(|c| **c == cur[i]).count() > 1;
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("pin_map_pick", i)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                        for p in &pins {
                            if ui.selectable_label(cur[i] == p.number, format!("{}  {}", p.number, p.name)).clicked() {
                                cur[i] = p.number.clone();
                                changed = true;
                            }
                        }
                    });
                    if twice {
                        ui.colored_label(crate::theme::WARN, "同じピンを2つの端子に選んでいます");
                    }
                });
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            if ui.small_button("名前で合わせる").on_hover_text("ピン表のピンの名前 (か番号) が、モデルの端子の名前と同じものを、自動で選びます。合うものがない端子は、選び直しになります").clicked() {
                for (i, role) in g.roles.iter().enumerate() {
                    cur[i] = pins.iter().find(|p| p.name.eq_ignore_ascii_case(role) || p.number.eq_ignore_ascii_case(role)).map_or(OPEN.into(), |p| p.number.clone());
                }
                changed = true;
            }
            let unused: Vec<&str> = pins.iter().filter(|p| !cur.contains(&p.number)).map(|p| p.name.as_str()).collect();
            if !unused.is_empty() {
                ui.small(format!("モデルに使わないピン: {}", unused.join(" "))).on_hover_text("これらのピンは、シミュレーションでは何にもつながりません (基板では、パッドとしてそのまま使えます)。つながない部品なら、回路図で「使わないピン (NC)」にします");
            }
        });
        if changed {
            cur.truncate(g.roles.len());
            self.sim_pins = cur.join(" ");
        }
    }
}
