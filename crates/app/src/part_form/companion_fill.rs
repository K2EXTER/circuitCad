//! 「推奨の周辺部品」 of the part form: the parts the data sheet asks for beside the part (a regulator's CIN and COUT), one per
//! line as `name | C | value | pin | to | note` (the pin by its name or number, `to` GND or another pin), read from the open
//! PDF (`datasheet::companions`: "CIN = 1µF" and the like) and saved as `PartDef::companions` for the schematic to place.

use super::PartForm;
use eframe::egui;
use part_lib::PinDef;
use sch_core::Companion;

/// The pin names a capacitor's name points to: CIN → the input, COUT → the output, CADJ → ADJ …; and where its other end goes
/// (GND, or for a boot capacitor the SW pin). None for a name that says no pin (C1, C2: the figure tells).
fn pin_for(cap: &str, pins: &[PinDef]) -> Option<(String, String)> {
    let find = |names: &[&str]| pins.iter().find(|p| names.iter().any(|n| p.name.eq_ignore_ascii_case(n))).map(|p| p.number.clone());
    let tail = cap.trim_start_matches(['C', 'c']).trim_end_matches(|c: char| c.is_ascii_digit()).to_ascii_uppercase();
    let gnd = || "GND".to_string();
    match tail.as_str() {
        "IN" | "I" => find(&["VIN", "IN", "INPUT", "VI", "VCC", "VDD", "V+"]).map(|p| (p, gnd())),
        "OUT" | "O" => find(&["VOUT", "OUT", "OUTPUT", "VO"]).map(|p| (p, gnd())),
        "BOOT" | "BST" => Some((find(&["BOOT", "BST", "CB"])?, find(&["SW", "LX", "PH"])?)),
        "FF" => Some((find(&["FB"])?, find(&["VOUT", "OUT", "OUTPUT", "VO"])?)),
        "" => None,
        // the name itself, or a longer name it starts ("ADJ" of TI's "ADJUST", "ADJUSTMENT")
        t => pins
            .iter()
            .find(|p| p.name.to_ascii_uppercase().split('/').any(|part| part == t))
            .or_else(|| pins.iter().find(|p| t.len() >= 2 && p.name.to_ascii_uppercase().starts_with(t)))
            .map(|p| (p.number.clone(), gnd())),
    }
}

impl PartForm {
    /// The companion lines as parts, with their pins as numbers (a pin may be written by its name).
    pub(super) fn parse_companions(&self, pins: &[PinDef]) -> Result<Vec<Companion>, String> {
        let pin_no = |t: &str, n: usize| -> Result<String, String> {
            pins.iter()
                .find(|p| p.number == t)
                .or_else(|| pins.iter().find(|p| p.name.eq_ignore_ascii_case(t)))
                .map(|p| p.number.clone())
                .ok_or_else(|| format!("推奨の周辺部品 {n} 行目: ピン「{t}」がピン表にありません (番号か名前で書きます)"))
        };
        let mut out = vec![];
        for (n, line) in self.companions_text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let n = n + 1;
            let f: Vec<&str> = line.split('|').map(str::trim).collect();
            if f.len() < 5 {
                return Err(format!("推奨の周辺部品 {n} 行目: 「名前 | C | 値 | ピン | GND か相手のピン | 注」の形で書きます"));
            }
            let kind = f[1].to_ascii_uppercase();
            if !["C", "R", "L"].contains(&kind.as_str()) {
                return Err(format!("推奨の周辺部品 {n} 行目: 種類「{}」は C・R・L のどれかです", f[1]));
            }
            if sim_core::parse_value(f[2]).is_none() {
                return Err(format!("推奨の周辺部品 {n} 行目: 値「{}」が読めません (例: 1u、100n、10k)", f[2]));
            }
            let to = if f[4].eq_ignore_ascii_case("GND") { "GND".to_string() } else { pin_no(f[4], n)? };
            out.push(Companion { name: f[0].to_string(), kind, value: f[2].to_string(), pin: pin_no(f[3], n)?, to, note: f.get(5).map(|s| s.to_string()).unwrap_or_default() });
        }
        Ok(out)
    }

    /// The lines of a saved part's companions, the pins by name.
    pub(super) fn companions_text_of(list: &[Companion], pins: &[PinDef]) -> String {
        let name = |no: &str| pins.iter().find(|p| p.number == no).map_or(no.to_string(), |p| p.name.clone());
        list.iter()
            .map(|c| {
                let to = if c.to == "GND" { c.to.clone() } else { name(&c.to) };
                let line = format!("{} | {} | {} | {} | {to}", c.name, c.kind, c.value, name(&c.pin));
                if c.note.is_empty() { line } else { format!("{line} | {}", c.note) }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 「PDF から読む」: the capacitors the open data sheet names with a value, on the pins their names point to; a line for each
    /// (put in, or why not).
    pub fn fill_companions_from_pdf(&mut self) {
        let caps = self.sheet.capacitors();
        // the ESR asked of the output capacitor (and of the input one when the sheet says so): in the line's note
        let esr = self.sheet.esr();
        let esr_note = |name: &str| -> Option<String> {
            let e = esr.as_ref()?;
            let tail = name.trim_start_matches(['C', 'c']).trim_end_matches(|c: char| c.is_ascii_digit()).to_ascii_uppercase();
            let fits = matches!(tail.as_str(), "OUT" | "O") || (e.input_too && matches!(tail.as_str(), "IN" | "I"));
            let ohm = |v: f64| format!("{}Ω", part_lib::passive::eng(v));
            let range = match (e.lo, e.hi) {
                (Some(lo), Some(hi)) => format!("{}〜{}", ohm(lo), ohm(hi)),
                (None, Some(hi)) => format!("{} 以下", ohm(hi)),
                (Some(lo), None) => format!("{} 以上", ohm(lo)),
                (None, None) => return None,
            };
            fits.then(|| format!("ESR {range} (p.{})", e.page + 1))
        };
        let pins = self.parse_pins().unwrap_or_default();
        let mut lines: Vec<String> = self.companions_text.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect();
        let mut msg = vec![];
        if caps.is_empty() {
            msg.push((false, "「CIN = 1µF」のように名前と値の書かれたコンデンサが見つかりません。応用回路の図を見て、行を足してください".to_string()));
        }
        for c in &caps {
            let value = part_lib::passive::eng(c.farads);
            let how = format!("{}{value}F (p.{}、{} か所)", if c.at_least { "≥ " } else { "" }, c.page + 1, c.count);
            if lines.iter().any(|l| l.split('|').next().is_some_and(|n| n.trim().eq_ignore_ascii_case(&c.name))) {
                msg.push((true, format!("{} {how}: もう行があるので、そのままにしました", c.name)));
                continue;
            }
            match pin_for(&c.name, &pins) {
                Some((pin, to)) => {
                    let pin_name = pins.iter().find(|p| p.number == pin).map_or(pin.clone(), |p| p.name.clone());
                    let to_name = if to == "GND" { to.clone() } else { pins.iter().find(|p| p.number == to).map_or(to.clone(), |p| p.name.clone()) };
                    let mut note = format!("{}データシートの p.{}", if c.at_least { "以上。" } else { "" }, c.page + 1);
                    if let Some(e) = esr_note(&c.name) {
                        note += &format!("。{e}");
                    }
                    lines.push(format!("{} | C | {value} | {pin_name} | {to_name} | {note}", c.name));
                    msg.push((true, format!("{} {how}: {pin_name} と {to_name} の間", c.name)));
                }
                None => msg.push((false, format!("{} {how}: 名前からはどのピンか分からないので入れません。応用回路の図を見て、行を足してください", c.name))),
            }
        }
        self.companions_text = lines.join("\n");
        self.companions_msg = msg;
    }

    pub(super) fn companions_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(i18n::tr("推奨の周辺部品 (入力・出力のコンデンサなど)")).id_salt("companions").default_open(!self.companions_text.trim().is_empty()).show(ui, |ui| {
            ui.small(i18n::tr("データシートが部品のそばに付けるように言う部品です。1 行に「名前 | 種類 (C・R・L) | 値 | ピン | GND か相手のピン | 注」。回路図でこの部品を選び「推奨の周辺部品を置く」を押すと、ピンのそばに置いて線をつなぎます"));
            ui.add(egui::TextEdit::multiline(&mut self.companions_text).desired_rows(3).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace))
                .on_hover_text(i18n::tr("例: CIN | C | 0.33u | VIN | GND | 入力の近くに"));
            if self.sheet.has_doc()
                && ui
                    .button(i18n::tr("PDF から読む"))
                    .on_hover_text(i18n::tr("開いているデータシートから「CIN = 1µF」「COUT ≥ 2.2µF」のように名前と値の書かれたコンデンサを探し、いちばん多く書かれた値を、名前の指すピン (CIN は入力、COUT は出力、CADJ は ADJ) と GND の間に入れます。C1・C2 のような名前は、どのピンか分からないので知らせるだけです"))
                    .clicked()
            {
                self.fill_companions_from_pdf();
            }
            for (ok, line) in &self.companions_msg {
                ui.label(egui::RichText::new(line).small().color(if *ok { crate::theme::OK } else { crate::theme::WARN }));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_become_parts_on_pins_named_or_numbered() {
        let mut f = PartForm::default();
        f.pins_text = "1 VIN\n2 GND\n3 VOUT\n4 ADJ".into();
        let pins = f.parse_pins().unwrap();
        f.companions_text = "CIN | C | 0.33u | VIN | GND | 入力の近くに\nCOUT | c | 1u | 3 | GND".into();
        let list = f.parse_companions(&pins).unwrap();
        assert_eq!((list[0].pin.as_str(), list[0].note.as_str(), list[1].pin.as_str(), list[1].kind.as_str()), ("1", "入力の近くに", "3", "C"));
        // and back, the pins by name
        assert_eq!(PartForm::companions_text_of(&list, &pins), "CIN | C | 0.33u | VIN | GND | 入力の近くに\nCOUT | C | 1u | VOUT | GND");
        for bad in ["CIN | C | 1u | VX | GND", "CIN | Q | 1u | VIN | GND", "CIN | C | abc | VIN | GND", "CIN | C | 1u"] {
            f.companions_text = bad.into();
            assert!(f.parse_companions(&pins).is_err(), "{bad}");
        }
        // which pin a capacitor's name points to
        assert_eq!(pin_for("CIN", &pins), Some(("1".into(), "GND".into())));
        assert_eq!(pin_for("COUT", &pins), Some(("3".into(), "GND".into())));
        assert_eq!(pin_for("CADJ", &pins), Some(("4".into(), "GND".into())));
        assert_eq!(pin_for("C1", &pins), None);
        // TI's LM317 names its pin "ADJUST"
        let mut f = PartForm::default();
        f.pins_text = "1 ADJUST\n2 OUTPUT\n3 INPUT".into();
        let pins = f.parse_pins().unwrap();
        assert_eq!(pin_for("CADJ", &pins), Some(("1".into(), "GND".into())));
        assert_eq!(pin_for("CIN", &pins), Some(("3".into(), "GND".into())));
    }
}
