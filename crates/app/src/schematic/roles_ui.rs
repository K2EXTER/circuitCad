//! The bubble beside the one part picked on the schematic (B11): what it does with the parts around it (a divider, an RC filter, an
//! LED's resistor, a pull-up, a bypass capacitor, an op amp's feedback) and the figure that comes of it (`sch_core::roles`). The
//! voltages come from the DC result when it is for the circuit as it is, else from the supplies' values.

use super::*;

/// The roles worked out for one part (the key: the edit count, the part, whether the DC result was used).
#[derive(Default)]
pub(crate) struct RolesCache {
    key: String,
    roles: Vec<sch_core::roles::Role>,
    from_dc: bool,
}

impl SchEditor {
    pub(super) fn roles_bubble(&mut self, ui: &egui::Ui, rect: Rect) {
        if self.live_logic.is_some() || !self.multi.is_empty() {
            return;
        }
        let Some(Sel::Comp(i)) = self.selected else { return };
        let Some(c) = self.sch.components.get(i) else { return };
        let refdes = c.refdes.clone();
        if self.roles_hidden.as_deref() == Some(refdes.as_str()) {
            return;
        }
        let dc = self.bench_voltages();
        let key = format!("{}|{refdes}|{}", self.rev, dc.is_some());
        if self.roles_cache.key != key {
            let fixed = sch_core::roles::fixed_volts(&self.sch);
            let volts = |n: &str| -> Option<f64> {
                dc.as_ref().and_then(|v| v.iter().find(|(m, _)| m == n).map(|(_, x)| *x)).or_else(|| fixed.get(n).copied())
            };
            self.roles_cache = RolesCache { key, roles: sch_core::roles::roles_of(&self.sch, &refdes, &volts), from_dc: dc.is_some() };
        }
        if self.roles_cache.roles.is_empty() {
            return;
        }
        // to the right of the part, or to its left when the canvas ends first (not over the part)
        const W: f32 = 316.0;
        let right = self.to_screen(rect, (c.pos.0 as f32 + 3.0, c.pos.1 as f32 - 3.0));
        let x = if right.x + W <= rect.right() { right.x } else { self.to_screen(rect, (c.pos.0 as f32 - 3.0, 0.0)).x - W };
        let at = egui::pos2(x.clamp(rect.left() + 4.0, (rect.right() - W).max(rect.left() + 4.0)), right.y.clamp(rect.top() + 4.0, rect.bottom() - 120.0));
        let mut close = false;
        egui::Area::new(egui::Id::new("roles_bubble")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(i18n::trf!("{refdes} の役割")).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text(i18n::tr("この部品の吹き出しを閉じます (ほかの部品を選ぶと、また出ます)")).clicked() {
                            close = true;
                        }
                    });
                });
                for r in &self.roles_cache.roles {
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(i18n::tr_dyn(&r.name)).strong().color(crate::theme::info()));
                        if let Some(v) = &r.value {
                            ui.label(egui::RichText::new(v).strong());
                        }
                        if r.parts.len() > 1 {
                            ui.label(egui::RichText::new(i18n::trf!("({})", r.parts.join("・"))).small().weak());
                        }
                    });
                    ui.label(egui::RichText::new(i18n::tr_dyn(&r.explain)).small());
                }
                let source = if self.roles_cache.from_dc { "値は DC 動作点の結果から計算しました" } else { "値は電源の値と部品の値から計算しました (DC 解析をすると、その結果を使います)" };
                ui.label(egui::RichText::new(i18n::tr(source)).small().weak());
            });
        });
        if close {
            self.roles_hidden = Some(refdes);
        }
    }
}
