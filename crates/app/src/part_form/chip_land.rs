//! 「データシートの寸法表から」 for a two-terminal chip (候補 Q3): the open data sheet's chip dimension table
//! (`datasheet::land::chip_sizes`: a row per size with L, W and the terminal underneath) made into lands by IPC-7351's chip
//! formula (`pcb_core::land::chip`), for a size the built-in shapes do not have or a maker whose chip is not the usual one.

use super::{PartForm, PkgKind};
use eframe::egui;
use datasheet::land::ChipSize;

impl PartForm {
    /// The sizes of the open PDF's chip tables, one button each (only those with the terminal's length, which the lands need).
    pub(super) fn chip_land_ui(&mut self, ui: &mut egui::Ui) {
        if self.pkg != PkgKind::Chip2 || !self.sheet.has_doc() {
            return;
        }
        let sizes: Vec<ChipSize> = self.sheet.chip_sizes().into_iter().filter(|c| c.term.is_some()).take(16).collect();
        if sizes.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("データシートの寸法表から:"))
                .on_hover_text(i18n::tr("開いているデータシートの外形寸法の表 (L・W と下面の電極の長さ) から、IPC-7351 の式でパッドを作ります。標準の形 (0603 など) の代わりに、このメーカーの寸法に合わせたいときに使います"));
            for c in &sizes {
                let tip = format!("L {}・W {}・電極 {} ({}) mm、p.{}", range(c.len), range(c.width), c.term.map_or("—".into(), range), c.term_from, c.page + 1);
                if ui.small_button(&c.code).on_hover_text(tip).clicked() {
                    self.chip_land_msg = Some(self.apply_chip_size(c));
                }
            }
        });
        if let Some(m) = &self.chip_land_msg {
            let (ok, text) = match m {
                Ok(t) => (true, t),
                Err(e) => (false, e),
            };
            ui.colored_label(if ok { crate::theme::ok() } else { crate::theme::warn() }, egui::RichText::new(text).small());
        }
    }

    /// The lands of one size into the pad fields (the standard shape let go).
    pub fn apply_chip_size(&mut self, c: &ChipSize) -> Result<String, String> {
        let term = c.term.ok_or_else(|| format!("{} の下面の電極の長さが表に無いので、パッドを計算できません", c.code))?;
        let g = pcb_core::land::chip(c.len, term, c.width).ok_or_else(|| format!("{} の寸法 (L {}・電極 {}) では、両側のパッドが重なります。表の読みを確かめてください", c.code, range(c.len), range(term)))?;
        self.fp_preset.clear();
        self.pkg = PkgKind::Chip2;
        (self.pitch, self.len, self.width) = (g.row, g.pad_l, g.pad_w);
        Ok(format!(
            "{} (p.{}) の寸法から IPC の式で、パッド {:.2} × {:.2} mm・中心の間 {:.2} mm にしました (電極は {})。推奨のランドの図があれば見比べてください",
            c.code,
            c.page + 1,
            g.pad_l,
            g.pad_w,
            g.row,
            c.term_from
        ))
    }
}

/// "1.50–1.70" (or "0.20 以上" for a minimum only).
fn range((lo, hi): (f64, f64)) -> String {
    if hi.is_infinite() {
        format!("{lo:.2} 以上")
    } else if (hi - lo).abs() < 1e-9 {
        format!("{lo:.2}")
    } else {
        format!("{lo:.2}–{hi:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Yageo's RT0603 row into the pad fields; the standard shape is let go.
    #[test]
    fn a_chip_size_sets_the_pads() {
        let mut f = PartForm { fp_preset: "R_0603".into(), ..Default::default() };
        let c = ChipSize { page: 3, code: "RT0603".into(), len: (1.5, 1.7), width: (0.7, 0.9), term: Some((0.1, 0.4)), term_from: "I2".into() };
        let m = f.apply_chip_size(&c).unwrap();
        assert!(f.fp_preset.is_empty() && f.pkg == PkgKind::Chip2 && (f.pitch - 1.56).abs() < 0.03 && (f.len - 0.87).abs() < 0.03, "{m}");
        assert!(f.apply_chip_size(&ChipSize { term: None, ..c }).is_err());
    }
}
