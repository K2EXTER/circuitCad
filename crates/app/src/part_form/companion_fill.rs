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

/// The divider that sets an adjustable output (候補 Q5): R1 from the output to the ADJ (or FB) pin, R2 from there to GND, for
/// `vout` with the reference `vref`. On an ADJ pin (LM317) the reference stands across R1: VOUT = VREF × (1 + R2 / R1), R1 `r1`
/// or 240 Ω and R2 the nearest E24. On an FB pin (a DC-DC) it stands across R2: VOUT = VREF × (1 + R1 / R2), R2 `r1` or 10 kΩ
/// (TI's RFBB) and R1 the nearest E24 (before 2026-11-24 the FB divider was worked out as the ADJ one: 5 V came out 0.96 V).
/// Without an output pin (a DC-DC's output is after its coil) R1 goes to the net `net:VOUT`. The two lines and the output.
pub fn divider_lines(pins: &[PinDef], vout: f64, vref: f64, r1: Option<f64>) -> Result<(Vec<String>, f64), String> {
    let find = |names: &[&str]| pins.iter().find(|p| names.iter().any(|n| p.name.eq_ignore_ascii_case(n)));
    let adj = find(&["ADJ", "ADJUST", "ADJUSTMENT", "FB", "SET", "VFB"]).ok_or("帰還のピン (ADJ・FB) がピン表にありません。出力電圧が決まった部品には分圧の抵抗は要りません")?;
    let fb = adj.name.to_ascii_uppercase().contains("FB");
    let out = match find(&["VOUT", "OUT", "OUTPUT", "VO"]) {
        Some(p) => p.name.clone(),
        None if fb => "net:VOUT".to_string(),
        None => return Err("出力のピン (VOUT・OUT・OUTPUT) がピン表にありません".into()),
    };
    if !(vref > 0.0 && vout > vref) {
        return Err(format!("出力電圧 {vout} V は基準電圧 {vref} V より高くします"));
    }
    let (top, bottom, got) = if fb {
        let bottom = r1.filter(|r| *r > 0.0).unwrap_or(10e3);
        let top = sim_core::e24(bottom * (vout / vref - 1.0), false);
        (top, bottom, vref * (1.0 + top / bottom))
    } else {
        let top = r1.filter(|r| *r > 0.0).unwrap_or(240.0);
        let bottom = sim_core::e24(top * (vout / vref - 1.0), false);
        (top, bottom, vref * (1.0 + bottom / top))
    };
    let ohms = |r: f64| part_lib::passive::eng(r);
    let note = format!("出力 {vout} V の分圧 (VREF {vref} V で {got:.3} V)");
    Ok((vec![format!("R1 | R | {} | {out} | {} | {note}", ohms(top), adj.name), format!("R2 | R | {} | {} | GND | {note}", ohms(bottom), adj.name)], got))
}

/// The nearest E6 value (1, 1.5, 2.2, 3.3, 4.7, 6.8 a decade): coils and large capacitors come in them.
fn e6(v: f64) -> f64 {
    let d = 10f64.powf(v.log10().floor());
    let m = [1.0, 1.5, 2.2, 3.3, 4.7, 6.8, 10.0].into_iter().min_by(|a, b| ((v / d) / a).ln().abs().total_cmp(&((v / d) / b).ln().abs())).unwrap_or(1.0);
    m * d
}

/// A synchronous buck's parts (候補 Q5): the input capacitor, the bootstrap capacitor (BST to SW), the soft start's capacitor
/// (1 ms, from the SS current), the coil (a 30 % ripple at `iout`), the output capacitor (the ripple at most 0.5 % of `vout`, 22 µF
/// or more) and the divider; the lines and a sentence of how each was chosen.
pub fn buck_lines(pins: &[PinDef], vin: f64, vout: f64, iout: f64, fsw: f64, vref: f64, iss: Option<f64>) -> Result<(Vec<String>, String), String> {
    if !(vin > vout && vout > 0.0 && iout > 0.0 && fsw > 0.0) {
        return Err("入力電圧 > 出力電圧 > 0、出力電流と周波数は 0 より大きくしてください".into());
    }
    let find = |names: &[&str]| pins.iter().find(|p| names.iter().any(|n| p.name.eq_ignore_ascii_case(n))).map(|p| p.name.clone());
    let sw = find(&["SW", "LX", "PH"]).ok_or("スイッチのピン (SW・LX) がピン表にありません。降圧の DC-DC の部品で使います")?;
    let vin_pin = find(&["VIN", "IN", "PVIN"]).ok_or("入力のピン (VIN) がピン表にありません")?;
    let eng = part_lib::passive::eng;
    let ripple = 0.3 * iout;
    let l = e6((vin - vout) * vout / (vin * fsw * ripple));
    let di = (vin - vout) * vout / (vin * fsw * l);
    let cout = e6((di / (8.0 * fsw * 0.005 * vout)).max(22e-6));
    let mut lines = vec![format!("CIN | C | 10u | {vin_pin} | GND | 入力のすぐそばに (セラミック、入力電圧の 1.5 倍以上の耐圧)")];
    if let Some(bst) = find(&["BST", "BOOT", "CB", "CBOOT"]) {
        lines.push(format!("CBST | C | 100n | {bst} | {sw} | ブートストラップ (無いと上側のスイッチが入らない)"));
    }
    if let Some(ss) = find(&["SS", "SS/TR", "TR"]) {
        let i = iss.unwrap_or(5.5e-6);
        let css = sim_core::e24(1e-3 * i / vref, false);
        lines.push(format!("CSS | C | {} | {ss} | GND | ソフトスタート 約 {:.1} ms (CSS × {} V ÷ {} µA)", eng(css), css * vref / i * 1e3, crate::part_form::linear_show(vref), crate::part_form::linear_show(i * 1e6)));
    }
    lines.push(format!("L1 | L | {} | {sw} | net:VOUT | リップル電流 約 {di:.2} A (出力電流の {:.0} %)、飽和電流は {:.1} A 以上の品を", eng(l), di / iout * 100.0, iout + di / 2.0));
    lines.push(format!("COUT | C | {} | net:VOUT | GND | 出力のリップル 約 {:.1} mV。セラミックは直流電圧で容量が減るので、実効でこれだけ残る品か 2 個に", eng(cout), di / (8.0 * fsw * cout) * 1e3));
    let (div, got) = divider_lines(pins, vout, vref, None)?;
    lines.extend(div);
    let how = format!(
        "入力 {} V → 出力 {} V・{} A・{} kHz: コイル {}H (リップル {:.2} A)、出力 {}F、分圧で {:.3} V",
        crate::part_form::linear_show(vin),
        crate::part_form::linear_show(vout),
        crate::part_form::linear_show(iout),
        crate::part_form::linear_show(fsw / 1e3),
        eng(l),
        di,
        eng(cout),
        got
    );
    Ok((lines, how))
}

impl PartForm {
    /// 「DC-DC の周辺部品を足す」: the buck's lines from the boxes (the input, the output, its current, the frequency; the reference
    /// and the SS current from the buck box when made, else the data sheet's VFB and 0.8 V), replacing lines of the same names.
    pub fn add_buck_companions(&mut self) -> Result<String, String> {
        let num = |label: &str, t: &str| sim_core::parse_value(t.trim().trim_end_matches(['V', 'v', 'A', 'a'])).filter(|v| *v > 0.0).ok_or(format!("{label}を入れてください"));
        let vin = num("入力電圧", &self.companions_vin)?;
        let vout = num("出力電圧", &self.companions_vout)?;
        let iout = num("出力電流", &self.companions_iout)?;
        let fsw = sim_core::parse_value(self.companions_fsw.trim().trim_end_matches("Hz")).filter(|v| *v > 0.0).unwrap_or(500e3);
        let buck = self.buck.spec("X", "s").ok();
        let vref = buck.as_ref().map(|b| b.vref).or_else(|| ["VFB", "VREF"].iter().find_map(|s| self.sheet.spec_values(s).iter().find_map(|v| v.typ)).filter(|v| *v > 0.1 && *v < 5.0)).unwrap_or(0.8);
        let iss = buck.as_ref().and_then(|b| b.iss);
        let pins = self.parse_pins()?;
        let (lines, how) = buck_lines(&pins, vin, vout, iout, fsw, vref, iss)?;
        let names: Vec<String> = lines.iter().filter_map(|l| l.split('|').next().map(|n| n.trim().to_string())).collect();
        let mut keep: Vec<String> = self.companions_text.lines().filter(|l| !l.trim().is_empty() && !l.split('|').next().is_some_and(|n| names.iter().any(|x| x == n.trim()))).map(str::to_string).collect();
        keep.extend(lines);
        self.companions_text = keep.join("\n");
        Ok(format!("{how}。データシートの応用回路の値があれば、それに直してください"))
    }

    /// 「分圧の抵抗を足す」: R1 and R2 for the output voltage in its box (the reference from the open PDF's VREF / VFB row, else the
    /// regulator box's), replacing lines named R1 and R2.
    pub fn add_divider(&mut self) -> Result<String, String> {
        let vout = sim_core::parse_value(self.companions_vout.trim().trim_end_matches(['V', 'v'])).ok_or("出力電圧を V で入れてください (例: 5)")?;
        let vref = ["VREF", "Vref", "VFB", "VADJ"].iter().find_map(|s| self.sheet.spec_values(s).iter().find_map(|v| v.typ)).filter(|v| *v > 0.1 && *v < 5.0);
        let (vref, from) = match vref {
            Some(v) => (v, "データシートの表"),
            None => (1.25, "表に無いので LM317 の 1.25 V"),
        };
        let pins = self.parse_pins()?;
        let (lines, got) = divider_lines(&pins, vout, vref, None)?;
        let mut keep: Vec<String> = self.companions_text.lines().filter(|l| !l.trim().is_empty() && !matches!(l.split('|').next().map(str::trim), Some("R1" | "R2"))).map(str::to_string).collect();
        keep.extend(lines);
        self.companions_text = keep.join("\n");
        Ok(format!("R1・R2 を足しました: VREF {vref} V ({from}) で {got:.3} V。応用回路の図に R1 の値があれば、その値に直してください"))
    }

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
            // a net that is no pin of the part: "net:VOUT" (a buck's output, after its coil)
            let end = |t: &str| -> Result<String, String> { if t.len() > 4 && t[..4].eq_ignore_ascii_case("net:") { Ok(format!("net:{}", &t[4..])) } else { pin_no(t, n) } };
            let to = if f[4].eq_ignore_ascii_case("GND") { "GND".to_string() } else { end(f[4])? };
            out.push(Companion { name: f[0].to_string(), kind, value: f[2].to_string(), pin: end(f[3])?, to, note: f.get(5).map(|s| s.to_string()).unwrap_or_default() });
        }
        Ok(out)
    }

    /// The lines of a saved part's companions, the pins by name.
    pub(super) fn companions_text_of(list: &[Companion], pins: &[PinDef]) -> String {
        let name = |no: &str| if no.starts_with("net:") { no.to_string() } else { pins.iter().find(|p| p.number == no).map_or(no.to_string(), |p| p.name.clone()) };
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
            ui.horizontal(|ui| {
                ui.label(i18n::tr("出力電圧 [V]")).on_hover_text(i18n::tr("出力電圧を抵抗で決める部品 (LM317 の ADJ、DC-DC の FB) の、作りたい出力電圧"));
                ui.add(egui::TextEdit::singleline(&mut self.companions_vout).desired_width(50.0).hint_text("5"));
                if ui.button(i18n::tr("分圧の抵抗 R1・R2 を足す")).on_hover_text(i18n::tr("出力と ADJ (FB) の間に R1、ADJ と GND の間に R2 を足します。基準電圧はデータシートの表の VREF (VFB)、R1 は ADJ なら 240 Ω、FB なら 10 kΩ、R2 は E24 のいちばん近い値")).clicked() {
                    let m = self.add_divider();
                    self.companions_msg = vec![match m {
                        Ok(t) => (true, t),
                        Err(e) => (false, e),
                    }];
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(i18n::tr("降圧 DC-DC: 入力 [V]")).on_hover_text(i18n::tr("スイッチ内蔵の降圧 DC-DC (SW・BST・FB のピン) の、入力電圧"));
                ui.add(egui::TextEdit::singleline(&mut self.companions_vin).desired_width(40.0).hint_text("12"));
                ui.label(i18n::tr("出力電流 [A]")).on_hover_text(i18n::tr("使う最大の出力電流。コイルのリップル電流をその 30 % にします"));
                ui.add(egui::TextEdit::singleline(&mut self.companions_iout).desired_width(40.0).hint_text("2"));
                ui.label(i18n::tr("周波数 [Hz]")).on_hover_text(i18n::tr("スイッチング周波数 (空なら 500k)"));
                ui.add(egui::TextEdit::singleline(&mut self.companions_fsw).desired_width(50.0).hint_text("500k"));
                if ui.button(i18n::tr("DC-DC の周辺部品を足す")).on_hover_text(i18n::tr("上の「出力電圧」とこの欄から、入力のコンデンサ・BST と SW の間の 100 nF・ソフトスタートのコンデンサ (1 ms)・コイル (リップル 30 %)・出力のコンデンサ (リップル 0.5 % 以下、22 µF 以上)・帰還の分圧を足します。コイルと出力のコンデンサは「net:VOUT」(コイルの先の出力) につながります")).clicked() {
                    let m = self.add_buck_companions();
                    self.companions_msg = vec![match m {
                        Ok(t) => (true, t),
                        Err(e) => (false, e),
                    }];
                }
            });
            for (ok, line) in &self.companions_msg {
                ui.label(egui::RichText::new(line).small().color(if *ok { crate::theme::ok() } else { crate::theme::warn() }));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LM317 (OUTPUT, ADJUSTMENT, VREF 1.25 V) for 5 V: R1 240 Ω, R2 the E24 nearest 720 Ω (750 Ω: 5.16 V); an FB pin takes 10 kΩ.
    #[test]
    fn a_divider_for_an_adjustable_output() {
        let pin = |n: &str, name: &str| PinDef { unit: 0, number: n.into(), name: name.into(), side: None, sim_only: false, gap: 0, kind: None };
        let lm317 = [pin("1", "ADJUSTMENT"), pin("2", "OUTPUT"), pin("3", "INPUT")];
        let (lines, got) = divider_lines(&lm317, 5.0, 1.25, None).unwrap();
        assert!(lines[0].starts_with("R1 | R | 240 | OUTPUT | ADJUSTMENT") && lines[1].starts_with("R2 | R | 750 | ADJUSTMENT | GND"), "{lines:?}");
        assert!((got - 5.156).abs() < 0.01);
        // an FB pin: 10 kΩ at the bottom, the top 31.25 kΩ → E24 30 kΩ (3.2 V); without an output pin, the top to net:VOUT
        let buck = [pin("1", "FB"), pin("2", "VOUT")];
        let (lines, got) = divider_lines(&buck, 3.3, 0.8, None).unwrap();
        assert!(lines[0].starts_with("R1 | R | 30k | VOUT | FB") && lines[1].starts_with("R2 | R | 10k | FB | GND"), "{lines:?}");
        assert!((got - 3.2).abs() < 1e-9, "{got}");
        let (lines, _) = divider_lines(&buck[..1], 5.0, 0.8, None).unwrap();
        assert!(lines[0].starts_with("R1 | R | 51k | net:VOUT | FB"), "{lines:?}");
        assert!(divider_lines(&lm317[1..], 5.0, 1.25, None).is_err(), "no ADJ pin");
    }

    /// TPS62933 from 12 V to 5 V at 3 A, 500 kHz: a 30 % ripple asks 7.4 µH → E6 6.8 µH (the data sheet's design example has
    /// 6.8 µH too), the ripple 0.98 A; the output 22 µF; CSS 6.8 nF for 1 ms at 5.5 µA; the coil and COUT on net:VOUT.
    #[test]
    fn a_bucks_parts_from_its_voltages_and_current() {
        let pin = |n: &str, name: &str| PinDef { unit: 0, number: n.into(), name: name.into(), side: None, sim_only: false, gap: 0, kind: None };
        let pins = [pin("1", "RT"), pin("2", "EN"), pin("3", "VIN"), pin("4", "GND"), pin("5", "SW"), pin("6", "BST"), pin("7", "SS"), pin("8", "FB")];
        let (lines, how) = buck_lines(&pins, 12.0, 5.0, 3.0, 500e3, 0.8, Some(5.5e-6)).unwrap();
        let line = |name: &str| lines.iter().find(|l| l.starts_with(&format!("{name} |"))).unwrap_or_else(|| panic!("{name}: {lines:?}")).clone();
        assert!(line("CIN").starts_with("CIN | C | 10u | VIN | GND"));
        assert!(line("CBST").starts_with("CBST | C | 100n | BST | SW"));
        assert!(line("CSS").starts_with("CSS | C | 6.8n | SS | GND"), "{}", line("CSS"));
        assert!(line("L1").starts_with("L1 | L | 6.8u | SW | net:VOUT"), "{}", line("L1"));
        assert!(line("COUT").starts_with("COUT | C | 22u | net:VOUT | GND"), "{}", line("COUT"));
        assert!(line("R1").starts_with("R1 | R | 51k | net:VOUT | FB") && line("R2").starts_with("R2 | R | 10k | FB | GND"));
        assert!(how.contains("4.880"), "{how}");
        // the lines become parts with the net ends
        let mut f = PartForm::default();
        f.pins_text = "1 RT\n2 EN\n3 VIN\n4 GND\n5 SW\n6 BST\n7 SS\n8 FB".into();
        f.companions_text = lines.join("\n");
        let list = f.parse_companions(&f.parse_pins().unwrap()).unwrap();
        let l1 = list.iter().find(|c| c.name == "L1").unwrap();
        assert_eq!((l1.pin.as_str(), l1.to.as_str()), ("5", "net:VOUT"));
        assert!(buck_lines(&pins[..4], 12.0, 5.0, 3.0, 500e3, 0.8, None).unwrap_err().contains("SW"));
    }

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
