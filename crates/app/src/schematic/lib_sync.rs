//! "ライブラリの部品が新しくなっています" (candidate Y1): the placed parts whose copy of their library part is older than the library
//! (`part_lib::placed`: a model added or corrected since), listed in a window and brought up to date part by part or all at once
//! (an ordinary edit: Undo takes it back).

use super::*;
use part_lib::placed::{refresh, stale, Stale};

/// The list worked out, by the schematic's edit count and the library's (scanning ~15 000 parts each frame would be slow), and
/// whether the window is open.
#[derive(Default)]
pub(super) struct LibSync {
    found: Option<(u64, u64, Vec<Stale>)>,
    pub open: bool,
}

impl SchEditor {
    /// The placed parts older than the library now.
    pub(super) fn stale_parts(&mut self) -> Vec<Stale> {
        let key = (self.rev, self.lib_rev);
        if !self.lib_sync.found.as_ref().is_some_and(|(r, l, _)| (*r, *l) == key && self.settled()) {
            self.lib_sync.found = Some((key.0, key.1, stale(&self.sch, &self.library)));
        }
        self.lib_sync.found.as_ref().map(|x| x.2.clone()).unwrap_or_default()
    }

    /// The button under the checks, only when some placed part is older than the library.
    pub(super) fn lib_sync_button(&mut self, ui: &mut egui::Ui) {
        let n = self.stale_parts().len();
        if n == 0 {
            return;
        }
        if ui
            .add_sized([ui.available_width(), 22.0], egui::Button::new(egui::RichText::new(i18n::trf!("ライブラリの部品が新しくなっています ({} 個)…", n)).color(crate::theme::warn())))
            .on_hover_text(i18n::tr("回路図に置いた部品は、置いたときのライブラリの部品の写し (ピン・モデル・定格・店の型番) を持っています。そのあとライブラリの部品にモデルが付いたり直ったりした部品を並べ、置いた部品を新しくできます"))
            .clicked()
        {
            self.lib_sync.open = true;
        }
    }

    /// The window: each part, what differs, and the buttons to bring it up to date.
    pub(super) fn lib_sync_window(&mut self, ctx: &egui::Context) {
        if !self.lib_sync.open {
            return;
        }
        let found = self.stale_parts();
        let mut open = true;
        let mut update: Option<Vec<String>> = None;
        egui::Window::new(i18n::tr("ライブラリの部品の更新")).open(&mut open).default_pos(crate::widgets::beside_panel()).default_width(480.0).show(ctx, |ui| {
            if found.is_empty() {
                ui.colored_label(crate::theme::ok(), i18n::tr("✔ 置いた部品は、どれもライブラリと同じです"));
                return;
            }
            ui.label(egui::RichText::new(i18n::tr("置いたあとでライブラリの部品が変わった部品です。新しくすると、部品の場所・向き・部品番号・値はそのままで、モデル・定格・店の型番などがライブラリの今のものになります (元に戻すで戻せます)。ピンの並びが違う部品は、配線が外れないよう、ピンだけ前のままにします")).small());
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                egui::Grid::new("lib_sync_grid").striped(true).num_columns(4).show(ui, |ui| {
                    for h in ["部品", "ライブラリの部品", "変わった所", ""] {
                        ui.strong(i18n::tr(h));
                    }
                    ui.end_row();
                    for s in &found {
                        ui.monospace(&s.refdes);
                        ui.label(&s.part);
                        let mut what: Vec<String> = s.changes.iter().map(|c| i18n::tr_dyn(c).to_string()).collect();
                        if s.pins_differ {
                            what.push(i18n::tr("ピンは前のまま (置き直すと新しいピンに)").to_string());
                        }
                        ui.label(what.join("、"));
                        if ui.small_button(i18n::tr("新しくする")).on_hover_text(i18n::tr("この部品だけ、ライブラリの今の版にします")).clicked() {
                            update = Some(vec![s.refdes.clone()]);
                        }
                        ui.end_row();
                    }
                });
            });
            if ui.button(i18n::trf!("全部を新しくする ({} 個)", found.len())).on_hover_text(i18n::tr("並んだ部品を全部、ライブラリの今の版にします")).clicked() {
                update = Some(found.iter().map(|s| s.refdes.clone()).collect());
            }
        });
        self.lib_sync.open = open;
        if let Some(refs) = update {
            let library = self.library.clone();
            let done = refresh(&mut self.sch, &library, &refs);
            self.status = i18n::trf!("ライブラリの今の版にしました: {} (元に戻すで戻せます)", done.join("、"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A relay placed before its library part had a model is listed; brought up to date it has the model, the list is empty, and
    /// Undo puts the old copy back.
    #[test]
    fn a_part_older_than_the_library_is_listed_updated_and_undone() {
        let mut e = SchEditor::default();
        let relay = e.library.iter().find(|p| p.name == "TQ2-L2-5V").expect("bundled").clone();
        let k = e.sch.add(Kind::Custom, (10, 10), 0);
        let mut old = relay.symbol();
        old.sim = None;
        e.sch.components[k].custom = Some(old);
        e.sch.components[k].footprint = relay.footprint_name();
        e.commit_history_for_test();
        let found = e.stale_parts();
        assert_eq!(found.len(), 1, "{found:?}");
        let library = e.library.clone();
        assert_eq!(refresh(&mut e.sch, &library, &[]).len(), 1);
        e.commit_history_for_test();
        assert!(e.sch.components[k].custom.as_ref().unwrap().sim.is_some());
        assert!(e.stale_parts().is_empty());
        e.undo();
        assert!(e.sch.components[k].custom.as_ref().unwrap().sim.is_none(), "Undo takes it back");
        assert_eq!(e.stale_parts().len(), 1);
    }
}
