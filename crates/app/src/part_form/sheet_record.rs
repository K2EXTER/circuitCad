//! The data sheet a part was made from (候補 Q9): its file, edition and fingerprint are saved with the part
//! (`part_lib::SheetRecord`, from the open PDF: `SheetView::sheet_record`). Opening another edition of the sheet for a part
//! that has one offers to compare the table values kept with the part against the new edition's (`datasheet::revision::compare`).

use super::PartForm;
use datasheet::revision::{Change, SheetValue};
use eframe::egui;

fn value(v: &part_lib::SheetValueRecord) -> SheetValue {
    SheetValue { symbol: v.symbol.clone(), conditions: v.conditions.clone(), min: v.min, typ: v.typ, max: v.max, unit: v.unit.clone(), page: v.page }
}

/// "LM317.pdf (Rev SLVS044Z, APRIL 2025)".
pub fn shown(r: &part_lib::SheetRecord) -> String {
    let what: Vec<String> = [(!r.rev.is_empty()).then(|| format!("Rev {}", r.rev)), (!r.date.is_empty()).then(|| r.date.clone())].into_iter().flatten().collect();
    if what.is_empty() { r.file.clone() } else { format!("{} ({})", r.file, what.join(", ")) }
}

/// The lines of the changes between the values kept from `old` and those of `new`, one a change.
pub fn change_lines(old: &part_lib::SheetRecord, new: &part_lib::SheetRecord) -> Vec<String> {
    let (o, n): (Vec<SheetValue>, Vec<SheetValue>) = (old.values.iter().map(value).collect(), new.values.iter().map(value).collect());
    let cond = |v: &SheetValue| if v.conditions.is_empty() { String::new() } else { format!(" [{}]", v.conditions) };
    datasheet::revision::compare(&o, &n)
        .into_iter()
        .map(|c| match c {
            Change::Changed { old, new } => format!("変わった: {}{}  {} (p.{}) → {} (p.{})", old.symbol, cond(&old), old.shown(), old.page, new.shown(), new.page),
            Change::Missing(v) => format!("新しい版に無い: {}{}  {} (p.{})", v.symbol, cond(&v), v.shown(), v.page),
            Change::Added(v) => format!("新しい版だけ: {}{}  {} (p.{})", v.symbol, cond(&v), v.shown(), v.page),
        })
        .collect()
}

impl PartForm {
    /// Under the data sheet's URL: the edition the part was made from, and the comparison with the open PDF when it is another file.
    pub(super) fn sheet_record_ui(&mut self, ui: &mut egui::Ui) {
        let made = self.orig.as_ref().and_then(|o| o.sheet.clone());
        let open = self.sheet.sheet_record();
        if let Some(m) = &made {
            ui.label(egui::RichText::new(i18n::trf!("作った元のデータシート: {} — 表の値 {} 個を記録", shown(m), m.values.len())).small())
                .on_hover_text(i18n::tr("この部品を作ったときに開いていたデータシートの名前・版・日付と、表から読んだ値 (ページ付き)。新しい版の PDF を開くと、値を比べられます"));
        } else if let Some(o) = &open {
            ui.label(egui::RichText::new(i18n::trf!("保存すると、開いているデータシートの版 ({}) と表の値 {} 個を部品に記録します", shown(o), o.values.len())).small())
                .on_hover_text(i18n::tr("あとでメーカーが改版したとき、新しい版の PDF を開いて値を比べられます。PDF そのものは記録しません"));
        }
        if let (Some(m), Some(o)) = (&made, &open)
            && !m.fingerprint.is_empty()
            && m.fingerprint != o.fingerprint
            && ui.small_button(i18n::tr("作った元の版と表の値を比べる")).on_hover_text(i18n::tr("開いている PDF は作った元と別のファイルです。記録した表の値と、この PDF の表の値を並べ、変わった・無くなった・増えた行を出します")).clicked()
        {
            let lines = change_lines(m, o);
            self.sheet_diff = Some(if lines.is_empty() { vec![i18n::trf!("{} と {} で、記録した表の値は同じです", shown(m), shown(o))] } else { lines });
        }
        if let Some(lines) = &self.sheet_diff {
            for l in lines {
                ui.label(egui::RichText::new(l).small());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_value_is_listed() {
        let v = |typ: f64| part_lib::SheetValueRecord { symbol: "VF".into(), conditions: "IF = 10 mA".into(), typ: Some(typ), unit: "V".into(), page: 3, ..Default::default() };
        let old = part_lib::SheetRecord { file: "a.pdf".into(), rev: "7".into(), date: "April 2024".into(), fingerprint: "1".into(), values: vec![v(0.70)] };
        let new = part_lib::SheetRecord { file: "a.pdf".into(), rev: "8".into(), fingerprint: "2".into(), values: vec![v(0.72)], ..Default::default() };
        assert_eq!(shown(&old), "a.pdf (Rev 7, April 2024)");
        let l = change_lines(&old, &new);
        assert_eq!(l, vec!["変わった: VF [IF = 10 mA]  typ 0.7 V (p.3) → typ 0.72 V (p.3)".to_string()]);
    }
}
