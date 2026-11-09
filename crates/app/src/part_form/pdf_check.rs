//! Checking what was typed against the data sheet: the pins against the pin table that was cut out, and the dimensions
//! against the numbers of the pages with a drawing. A check says what it could not confirm; it never passes by silence.

use super::datasheet_view::Target;
use super::pin_import::parse_pin_lines;
use super::*;
use std::hash::{DefaultHasher, Hash, Hasher};

/// The pin table the pins in the form are compared with: where it was cut out and which columns were used.
#[derive(Clone)]
pub struct PinRef {
    pub page: usize,
    pub area: datasheet::Rect,
    pub num_col: usize,
    pub name_col: usize,
}

/// The last comparison, and what it was made for (it is made again only when that changes).
pub struct PinCheck {
    key: u64,
    /// None: the table could not be read again (the PDF was closed).
    pub result: Option<datasheet::PinCompare>,
}

/// The numbers of the pages with a drawing, read when the person asks for the check.
pub struct DimRef {
    pub pages: Vec<usize>,
    /// No page had a "land pattern" or "package" heading, so the page that was shown was used.
    pub fallback: bool,
    lengths: Vec<(usize, Vec<datasheet::PageLength>)>,
}

/// A dimension typed in: the field, its value (mm), and where the number was found (page and what was found).
type DimRow = (Target, f64, Option<(usize, datasheet::Found)>);

const OK: Color32 = Color32::from_rgb(90, 190, 120);

impl PartForm {
    /// Every frame: tells the data sheet window the pins, and compares them with the pin table again when the pins
    /// or the table changed.
    pub(super) fn sync_pdf_checks(&mut self) {
        self.sheet.set_current_pins(parse_pin_lines(&self.pins_text));
        let Some(r) = self.pin_ref.clone() else {
            self.pin_check = None;
            return;
        };
        let mut h = DefaultHasher::new();
        self.pins_text.hash(&mut h);
        (r.page, r.num_col, r.name_col, r.area.x0.to_bits(), r.area.y0.to_bits(), r.area.x1.to_bits(), r.area.y1.to_bits()).hash(&mut h);
        let key = h.finish();
        if self.pin_check.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let result = self.sheet.table_at(r.page, r.area).filter(|t| r.num_col < t.columns && r.name_col < t.columns).map(|t| {
            let pdf = datasheet::pins_from(&t, r.num_col, r.name_col);
            datasheet::compare_pins(&parse_pin_lines(&self.pins_text), &pdf)
        });
        self.pin_check = Some(PinCheck { key, result });
    }

    /// How many pins differ from the pin table (0 when there is no table to compare with).
    pub(super) fn pin_mismatches(&self) -> usize {
        self.pin_check.as_ref().and_then(|c| c.result.as_ref()).map_or(0, |r| r.diffs.len())
    }

    /// The result of comparing the pins with the pin table, under the pin box.
    pub(super) fn pin_check_ui(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.pin_ref.clone() else { return };
        ui.horizontal(|ui| {
            ui.small(i18n::trf!("データシート p.{} のピン表と比べています", r.page + 1));
            if ui.small_button(i18n::tr("表を見る")).on_hover_text(i18n::tr("比べている表のページを、データシートの窓に出します")).clicked() {
                self.sheet.go_to_page(r.page);
            }
            if ui.small_button(i18n::tr("比べるのをやめる")).on_hover_text(i18n::tr("この表との比べをやめます (ピン表は変わりません)")).clicked() {
                self.pin_ref = None;
                self.pin_check = None;
            }
        });
        match self.pin_check.as_ref().and_then(|c| c.result.as_ref()) {
            None => {
                ui.colored_label(crate::theme::WARN, i18n::tr("比べる表を読み直せませんでした (データシートを閉じましたか?)。確認できていません"));
            }
            Some(c) if c.diffs.is_empty() => {
                ui.colored_label(OK, i18n::trf!("✓ 表の {} 本すべてが、ピン表と一致しています", c.same));
            }
            Some(c) => {
                ui.colored_label(crate::theme::WARN, i18n::trf!("食い違いが {} 件あります (一致 {} 本)", c.diffs.len(), c.same));
                for d in c.diffs.iter().take(12) {
                    let (text, tip) = match d.kind {
                        datasheet::PinDiffKind::Differs => (
                            format!("{} 番: 名前が違います (ピン表 {} / データシート {})", d.number, d.form.as_deref().unwrap_or(""), d.pdf.as_deref().unwrap_or("")),
                            "どちらかの写し間違いです。データシートの表を見て、ピン表の名前を直します",
                        ),
                        datasheet::PinDiffKind::MissingInForm => (
                            format!("{} 番 {}: データシートにあるのに、ピン表にありません", d.number, d.pdf.as_deref().unwrap_or("")),
                            "ピンの入れ忘れです。ピン表にこのピンの行を足します。部品が持たないピンなら、読み取りの列が違っているかもしれません",
                        ),
                        datasheet::PinDiffKind::ExtraInForm => (
                            format!("{} 番 {}: ピン表にあるのに、データシートの表にありません", d.number, d.form.as_deref().unwrap_or("")),
                            "番号の写し間違いか、囲んだ表がこのピンを含んでいません (放熱パッドなど、別の表にあるピンかもしれません)",
                        ),
                    };
                    ui.small(text).on_hover_text(tip);
                }
                if c.diffs.len() > 12 {
                    ui.small(i18n::trf!("ほか {} 件", c.diffs.len() - 12));
                }
            }
        }
    }

    /// The package fields that have a value from the data sheet's drawings, for the kind of package chosen.
    pub(super) fn package_targets(&self) -> Vec<Target> {
        use Target as T;
        let mut targets = match self.pkg {
            PkgKind::Chip2 => vec![T::Pitch, T::PadLen, T::PadWidth],
            PkgKind::Dual if self.tht => vec![T::Pitch, T::Row, T::PadLen, T::PadWidth, T::Drill],
            PkgKind::Dual => vec![T::Pitch, T::Row, T::PadLen, T::PadWidth],
            PkgKind::Quad => vec![T::Pitch, T::Span, T::PadLen, T::PadWidth],
            PkgKind::Row => vec![T::Pitch, T::PadDia, T::Drill],
        };
        if matches!(self.pkg, PkgKind::Dual | PkgKind::Quad) && self.thermal_on {
            targets.extend([T::ThermalW, T::ThermalH]);
        }
        targets.push(T::Height);
        if self.body_on {
            targets.push(T::BodyW);
            if !self.body_circle {
                targets.push(T::BodyH);
            }
        }
        targets
    }

    /// The value in the field of a package target (mm).
    fn field_value(&self, t: Target) -> f64 {
        match t {
            Target::Pitch => self.pitch,
            Target::Row => self.row,
            Target::PadLen => self.len,
            Target::PadWidth => self.width,
            Target::Span => self.span,
            Target::PadDia => self.pad_d,
            Target::Drill => self.drill,
            Target::BodyW => self.body_w,
            Target::BodyH => self.body_h,
            Target::Height => self.height,
            Target::ThermalW => self.thermal_w,
            Target::ThermalH => self.thermal_h,
            _ => 0.0,
        }
    }

    /// For each dimension typed in: (field, value, where it was found: the page and the text of the number). None where
    /// the number is not on the pages that were read.
    fn dim_rows(&self) -> Vec<DimRow> {
        let Some(d) = &self.dim_ref else { return vec![] };
        self.package_targets()
            .into_iter()
            .map(|t| (t, self.field_value(t)))
            .filter(|(_, v)| *v > 0.0)
            .map(|(t, v)| {
                // a number that matches as it is, on any page, comes before one that matches as inches
                let found = d.lengths.iter().find_map(|(page, l)| datasheet::find_length(l, v).filter(|f| !f.as_inch).map(|f| (*page, f)));
                (t, v, found.or_else(|| d.lengths.iter().find_map(|(page, l)| datasheet::find_length(l, v).map(|f| (*page, f)))))
            })
            .collect()
    }

    /// "Check the dimensions against the data sheet": which of the numbers typed in are on the pages with a drawing.
    pub(super) fn dim_check_ui(&mut self, ui: &mut egui::Ui) {
        if !self.sheet.has_doc() {
            return;
        }
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("寸法をデータシートと照らす")).on_hover_text(i18n::tr("入れた寸法が、データシートの「ランドパターン」「外形寸法」のページに書かれている数字の中にあるかを調べます (インチの数字は mm に直して比べます)。数字があることの確認で、その数字がピッチなのかパッド幅なのかは、図面を見て確かめてください")).clicked() {
                self.dim_ref = self.sheet.dimension_lengths().map(|d| DimRef { pages: d.pages, fallback: d.fallback, lengths: d.lengths });
            }
            if self.dim_ref.is_some() && ui.small_button(i18n::tr("消す")).clicked() {
                self.dim_ref = None;
            }
        });
        let Some(d) = &self.dim_ref else { return };
        let pages = d.pages.iter().map(|p| format!("p.{}", p + 1)).collect::<Vec<_>>().join(" ");
        let note = if d.fallback {
            format!("「ランドパターン」「外形寸法」の見出しのあるページが見つかりませんでした。窓で見ていたページ ({pages}) の数字と照らしました")
        } else {
            format!("照らしたページ: {pages}")
        };
        let rows = self.dim_rows();
        ui.small(note);
        for (t, v, found) in &rows {
            match found {
                Some((page, f)) => {
                    let how = if f.as_inch { " (インチの数字として)" } else { "" };
                    ui.colored_label(OK, i18n::trf!("✓ {} {} mm: p.{} の「{}」{how}にあります", t.label(), super::pdf_input::number_text(*v), page + 1, f.text))
                }
                None => ui.colored_label(crate::theme::WARN, i18n::trf!("✗ {} {} mm: データシートの図面のページに見つかりません", t.label(), super::pdf_input::number_text(*v))),
            }
            .on_hover_text(if found.as_ref().is_some_and(|f| f.1.as_inch) { i18n::tr("単位のない小さな数字を、インチとして mm に直すと一致しました。偶然の一致もあるので、図面でインチの寸法かを確かめてください") } else if found.is_some() { i18n::tr("その数字が書かれていることの確認です。同じ数字が別の寸法のこともあるので、図面で確かめてください") } else { i18n::tr("写し間違いか、インチ・mil で書かれているか、図面に書かれていない計算値かもしれません。データシートの図面を見て、確かめてください") });
        }
        let missing = rows.iter().filter(|r| r.2.is_none()).count();
        if rows.is_empty() {
            ui.small(i18n::tr("照らす寸法がまだ入っていません"));
        } else if missing > 0 {
            ui.colored_label(crate::theme::WARN, i18n::trf!("{missing} 件が見つかっていません。「ランドパターンは概算」の印は、そのままにしておきます"));
        } else if self.land_unverified {
            ui.colored_label(OK, i18n::tr("入れた寸法は、すべて図面のページにありました"));
            if ui.button(i18n::tr("図面と見比べて確認した: 「概算」の印を外す")).on_hover_text(i18n::tr("数字があることまでは確かめました。ピッチやパッドの寸法が、図面のどの数字かを人が見比べて、合っていると確認したときに押してください")).clicked() {
                self.land_unverified = false;
            }
        } else {
            ui.colored_label(OK, i18n::tr("入れた寸法は、すべて図面のページにありました"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datasheet::PageLength;

    fn length(text: &str, mm: &[f64]) -> PageLength {
        PageLength { text: text.into(), mm: mm.to_vec(), as_inch: vec![] }
    }

    #[test]
    fn dimensions_are_looked_for_on_the_pages_that_were_read() {
        let mut f = PartForm::default();
        f.pkg = PkgKind::Dual;
        f.tht = false;
        f.thermal_on = false;
        f.body_on = false;
        (f.pitch, f.row, f.len, f.width, f.height) = (1.27, 5.4, 1.55, 0.6, 0.0);
        f.dim_ref = Some(DimRef { pages: vec![2], fallback: false, lengths: vec![(2, vec![length("1.27", &[1.27]), length("5.4", &[5.4]), length("0.60", &[0.6])])] });
        let rows = f.dim_rows();
        // the height is 0 (not typed in): not asked about
        assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), [Target::Pitch, Target::Row, Target::PadLen, Target::PadWidth]);
        assert_eq!(rows[0].2.as_ref().map(|r| (r.0, r.1.text.as_str())), Some((2, "1.27")));
        assert_eq!(rows[2].2, None, "1.55 is not on the page");
        assert!(rows[3].2.is_some());
    }

    #[test]
    fn the_targets_follow_the_package() {
        let mut f = PartForm::default();
        f.pkg = PkgKind::Row;
        f.body_on = false;
        assert_eq!(f.package_targets(), [Target::Pitch, Target::PadDia, Target::Drill, Target::Height]);
        f.pkg = PkgKind::Quad;
        f.thermal_on = true;
        f.body_on = true;
        f.body_circle = false;
        assert_eq!(
            f.package_targets(),
            [Target::Pitch, Target::Span, Target::PadLen, Target::PadWidth, Target::ThermalW, Target::ThermalH, Target::Height, Target::BodyW, Target::BodyH]
        );
    }

    #[test]
    fn a_table_that_cannot_be_read_again_is_not_a_pass() {
        // no data sheet is open: the comparison has no result, and nothing is counted as a mismatch
        let mut f = PartForm::default();
        f.pin_ref = Some(PinRef { page: 0, area: datasheet::Rect { x0: 0.0, y0: 0.0, x1: 10.0, y1: 10.0 }, num_col: 0, name_col: 1 });
        f.sync_pdf_checks();
        assert!(f.pin_check.as_ref().is_some_and(|c| c.result.is_none()));
        assert_eq!(f.pin_mismatches(), 0);
        // without a table to compare with there is no check at all
        f.pin_ref = None;
        f.sync_pdf_checks();
        assert!(f.pin_check.is_none());
    }
}
