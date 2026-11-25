//! The perfboard's check against the other boards of the project plugged into it (L4): the holes of the connectors that join them make
//! the same pattern on both layouts (`sch_core::stripboard::mate_check`), and "相手に合わせて並べ直す" places this board's connectors to
//! match the other's and lays the rest out round them (`mate_parts`, then the layout round the locked parts).

use super::*;
use sch_core::stripboard::{mate_check, mate_parts, Mate, StripLayout};

impl BreadboardView {
    /// The other board's perfboard layout: as saved with it, else laid out now (once; kept for the next check).
    fn mate_layout(&mut self, k: usize) -> StripLayout {
        if let Some(l) = &self.mates[k].layout {
            return l.clone();
        }
        let m = &self.mates[k];
        let l = sch_core::perfboard::perf_layout_with(&m.sch, m.opt);
        self.mates[k].layout = Some(l.clone());
        l
    }

    /// The section of the panel: one line per board plugged into this one, its check and the button that fits this board to it.
    pub(super) fn mate_ui(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, l: &StripLayout) {
        if self.mates.is_empty() {
            return;
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(i18n::tr("つないだ基板のコネクタ")).strong()).on_hover_text(i18n::tr(
            "プロジェクトで、この基板とコネクタでつないだ基板 (差し込む小基板など) です。両方のユニバーサル基板の並べ方で、つないだピンの穴が同じ並び (間隔・向き) になっているかを照合します。合っていないと、実物では差せません",
        ));
        for k in 0..self.mates.len() {
            let name = self.mates[k].name.clone();
            let conns: Vec<String> = self.mates[k].links.iter().map(|x| format!("{}↔{}", x.b, x.a)).collect();
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{name} ({})", conns.join("、")));
                if std::mem::take(&mut self.mate_on_start) | ui.small_button(i18n::tr("照合")).on_hover_text(i18n::tr("この基板と相手の基板の並べ方で、つないだピンの穴の並びを比べます。相手の並べ方が保存されていなければ、相手を自動で並べた形と比べます (少し時間がかかります)")).clicked() {
                    let other = self.mate_layout(k);
                    self.mates[k].found = Some(mate_check(&other, l, &self.mates[k].links));
                }
                if ui
                    .small_button(i18n::tr("相手に合わせて並べ直す"))
                    .on_hover_text(i18n::tr("この基板のコネクタを、相手の基板のコネクタと同じ穴の並びに置いて固定し、ほかの部品をそのまわりに並べ直します (部品を上にして差し込む向き)。元に戻すで戻せます"))
                    .clicked()
                    && self.strip.around.is_none()
                {
                    let other = self.mate_layout(k);
                    match mate_parts(&other, &sch.sch, &self.mates[k].links, 2) {
                        Ok(parts) => {
                            let mut from = l.clone();
                            for p in parts {
                                from.parts.retain(|x| x.refdes != p.refdes);
                                from.locked.insert(p.refdes.clone());
                                from.parts.push(p);
                            }
                            // the boxes put by hand are let go: the connectors decide where things go now
                            from.pinned.clear();
                            self.mates[k].found = None;
                            sch.status = i18n::trf!("「{}」のコネクタに合わせて、コネクタを置いて固定し、ほかを並べ直しています…", name);
                            self.strip_relayout(sch, &from);
                        }
                        Err(e) => sch.status = e,
                    }
                }
            });
            if let Some(m) = &self.mates[k].found {
                let here = i18n::tr("この基板").to_string();
                let line = m.message(&format!("「{name}」"), &here);
                let color = if m.ok() { crate::theme::ok() } else { crate::theme::warn() };
                ui.colored_label(color, if m.ok() { format!("✔ {line}") } else { line });
                if matches!(m, Mate::Differs { .. } | Mate::Flipped { .. }) {
                    ui.small(i18n::tr("「相手に合わせて並べ直す」で、この基板のコネクタを相手と同じ並びにできます"));
                }
            }
        }
    }
}
