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

/// The way a part goes onto a board built by hand (L12), as a box to choose from: 自動 (from the footprint) and the choices that fit
/// the part (`MountChoice::fits`). Shown in the part's properties and in the prototype panel's list of parts.
pub fn mount_choice_ui(ui: &mut egui::Ui, c: &mut sch_core::Component, id: impl std::hash::Hash + std::fmt::Debug) {
    use sch_core::mount::{mount_of, MountChoice};
    let leads = c.pins().len();
    let fits: Vec<MountChoice> = MountChoice::ALL.into_iter().filter(|m| m.fits(c, leads)).collect();
    let auto = mount_of(c, leads).describe();
    // a part with nothing to choose (straight in already, no other way) shows nothing
    if c.mount.is_none() && auto.is_empty() && fits.iter().all(|m| *m == MountChoice::Direct) {
        return;
    }
    // a word for the automatic way (the sentence in the hover text: a long one in the box widened the left panel)
    use sch_core::mount::Mount;
    let short = match mount_of(c, leads) {
        Mount::Direct => "そのまま",
        Mount::Socket { .. } => "ソケット",
        Mount::Leaded => "リード部品",
        Mount::Wired => "リード線",
        Mount::Adapter { .. } => "変換基板",
        Mount::Module { .. } => "ピンヘッダ",
        Mount::Back { .. } => "裏のランド",
    };
    let auto_label = i18n::trf!("自動 ({})", i18n::tr(short));
    let auto_tip = if auto.is_empty() { i18n::tr("フットプリントから: そのまま差します").to_string() } else { i18n::trf!("フットプリントから: {}", i18n::tr_dyn(&auto)) };
    let shown = match c.mount {
        None => auto_label.clone(),
        Some(m) => i18n::tr(m.label()).to_string(),
    };
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(180.0).show_ui(ui, |ui| {
        ui.selectable_value(&mut c.mount, None, auto_label).on_hover_text(&auto_tip);
        for m in &fits {
            let tip = match m {
                MountChoice::Direct => "ソケットや変換基板を使わず、部品をそのまま基板にはんだ付けします",
                MountChoice::Leaded => "チップ部品の代わりに、同じ値のリード部品を買って差します (購入リストにも出ます)",
                MountChoice::Wired => "チップ部品の両端にリード線 (すずめっき線) をはんだ付けし、リード部品のように 2 つの穴に差します。部品表はチップ部品のままです",
                MountChoice::Lands => "チップ部品を裏 (はんだの面) で、隣り合う 2 つの穴のランドにまたがせて直接はんだ付けします (1206・0805)",
            };
            ui.selectable_value(&mut c.mount, Some(*m), i18n::tr(m.label())).on_hover_text(i18n::tr(tip));
        }
    }).response.on_hover_text(i18n::tr("試作 (ブレッドボード・ストリップ基板・ユニバーサル基板) で、この部品をどう付けるか。自動はフットプリントから決めます (DIP の IC はソケット、SOP は変換基板、チップ部品は同じ値のリード部品)。組み立て表と買う物の一覧に出ます"));
}
