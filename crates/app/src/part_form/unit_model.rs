//! A model of one unit (候補 Q6): the pins of each other unit in the model's node order (`SimModel::units`; unit A's are the
//! model's own pin list). 「ピン表のユニットから作る」 finds them: each of unit A's model pins that is A's own (not a pin the
//! units share, like the supply) is matched in the other unit by its name without the unit's mark ("1IN-" and "2IN-", "-INA"
//! and "-INB"), else by its place among the unit's pins; a shared pin stays the same.

use super::PartForm;
use eframe::egui;
use part_lib::PinDef;

/// A pin's name without the unit's mark: the leading channel number ("1OUT", "2IN-") or a trailing channel letter ("OUTA",
/// "-INB"), upper case.
fn bare(name: &str) -> String {
    let n = name.trim().to_ascii_uppercase();
    let n = n.trim_start_matches(|c: char| c.is_ascii_digit());
    let n = match n.strip_suffix(['A', 'B', 'C', 'D']) {
        Some(rest) if rest.len() >= 2 => rest,
        _ => n,
    };
    n.trim_end_matches(|c: char| c.is_ascii_digit()).to_string()
}

/// Each unit's pins in the model's node order, unit A's first (from `a`, the model's pins), for every unit the pin list has.
pub fn infer_units(pins: &[PinDef], a: &[String]) -> Result<Vec<Vec<String>>, String> {
    let last = pins.iter().map(|p| p.unit).max().unwrap_or(0);
    if last == 0 {
        return Err("ピン表にユニット B 以降のピンがありません。「③ ピン」でピンに unit=B などを付けてから作ります".into());
    }
    let of = |n: &str| pins.iter().find(|p| p.number == n);
    let own_a: Vec<&PinDef> = pins.iter().filter(|p| p.unit == 0).collect();
    let mut out = vec![a.to_vec()];
    for u in 1..=last {
        let theirs: Vec<&PinDef> = pins.iter().filter(|p| p.unit == u).collect();
        let mut row = vec![];
        for n in a {
            let p = of(n).ok_or_else(|| format!("モデルの端子のピン {n} がピン表にありません"))?;
            // the units' shared pins (supply, ground): the same pin for every unit. A pin of A whose name no other unit has, when
            // A has more pins than the unit: shared
            let key = bare(&p.name);
            let by_name: Vec<&&PinDef> = theirs.iter().filter(|q| bare(&q.name) == key).collect();
            let got = match by_name.as_slice() {
                [q] => q.number.clone(),
                _ => {
                    let place = own_a.iter().position(|q| q.number == p.number);
                    match place.and_then(|k| theirs.get(k)) {
                        Some(q) if own_a.len() == theirs.len() => q.number.clone(),
                        _ => p.number.clone(),
                    }
                }
            };
            row.push(got);
        }
        out.push(row);
    }
    Ok(out)
}

impl PartForm {
    /// Under the model's pins: each other unit's pins in the model's order, and the button that finds them.
    pub(super) fn unit_model_ui(&mut self, ui: &mut egui::Ui) {
        let has_units = self.parse_pins().is_ok_and(|p| p.iter().any(|p| p.unit > 0));
        if !has_units && self.sim_units.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("ユニットごとの端子"))
                .on_hover_text(i18n::tr("モデルが 1 ユニット分 (デュアルのオペアンプの 1 チャネル) のとき、ユニットごとにモデルの端子の順のピン番号を 1 行ずつ (A、B、C …)。置いたユニットごとにモデルが 1 つ入ります。空なら、モデルは部品全体のもの"));
            if ui.small_button(i18n::tr("ピン表のユニットから作る")).on_hover_text(i18n::tr("「ノード順のピン番号」(ユニット A) から、ほかのユニットの同じ名前のピン (1IN- と 2IN-、-INA と -INB) を探して並べます。電源のように全部のユニットで同じピンは、そのままです")).clicked() {
                let a: Vec<String> = self.sim_pins.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(String::from).collect();
                self.unit_model_msg = Some(self.parse_pins().and_then(|p| infer_units(&p, &a)).map(|u| {
                    let n = u.len();
                    self.sim_units = u;
                    format!("{n} ユニットの端子を並べました。ピン表と見比べてください")
                }));
            }
            if !self.sim_units.is_empty() && ui.small_button(i18n::tr("消す (部品全体のモデル)")).clicked() {
                self.sim_units.clear();
                self.unit_model_msg = None;
            }
        });
        if !self.sim_units.is_empty() {
            let mut text = self.sim_units.iter().map(|u| u.join(" ")).collect::<Vec<_>>().join("\n");
            if ui.add(egui::TextEdit::multiline(&mut text).desired_rows(self.sim_units.len()).desired_width(200.0).font(egui::TextStyle::Monospace)).changed() {
                self.sim_units = text.lines().map(|l| l.split_whitespace().map(String::from).collect()).filter(|v: &Vec<String>| !v.is_empty()).collect();
            }
        }
        if let Some(m) = &self.unit_model_msg {
            let (ok, t) = match m {
                Ok(t) => (true, t),
                Err(e) => (false, e),
            };
            ui.colored_label(if ok { crate::theme::ok() } else { crate::theme::warn() }, egui::RichText::new(t).small());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(number: &str, name: &str, unit: u8) -> PinDef {
        PinDef { unit, number: number.into(), name: name.into(), side: None, sim_only: false, gap: 0, kind: None }
    }

    /// TI's LM358 names (1OUT, 1IN-, 1IN+ / 2OUT, 2IN-, 2IN+) and ADI's (OUTA, -INA, +INA / OUTB …): the model's order of unit A
    /// (IN+ IN- V+ V- OUT) found in B, the supply kept.
    #[test]
    fn units_by_their_pin_names() {
        let ti = [pin("1", "1OUT", 0), pin("2", "1IN-", 0), pin("3", "1IN+", 0), pin("4", "GND", 0), pin("8", "VCC", 0), pin("7", "2OUT", 1), pin("6", "2IN-", 1), pin("5", "2IN+", 1)];
        let a: Vec<String> = ["3", "2", "8", "4", "1"].iter().map(|s| s.to_string()).collect();
        assert_eq!(infer_units(&ti, &a).unwrap()[1], ["5", "6", "8", "4", "7"]);
        let adi = [pin("1", "OUTA", 0), pin("2", "-INA", 0), pin("3", "+INA", 0), pin("4", "V-", 0), pin("8", "V+", 0), pin("7", "OUTB", 1), pin("6", "-INB", 1), pin("5", "+INB", 1)];
        assert_eq!(infer_units(&adi, &a).unwrap()[1], ["5", "6", "8", "4", "7"]);
        assert!(infer_units(&ti[..5], &a).is_err(), "no unit B");
    }
}
