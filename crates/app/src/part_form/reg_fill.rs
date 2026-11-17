//! The regulator's boxes filled from the electrical characteristics table of the open PDF. Tables name their rows by a
//! symbol (onsemi: VO, Regline, Regload, IB, RR, Vn) or only by the parameter's name (TI, AMS: "Line regulation",
//! "Dropout voltage"): both are looked up (`spec_table` takes the first word of a row as its symbol). A value whose
//! meaning needs the test conditions (a line regulation over "7.5 V ≤ VIN ≤ 25 V") goes in only when those are read.

use super::linear::{RegulatorInput, show, temperatures};
use datasheet::spec_table::{SpecValue, prefix};
use part_lib::fit;

/// The numbers of a conditions text that carry one of `units` ("7.5 Vdc ≤ Vin ≤ 25 Vdc" with the volt units: 7.5 and
/// 25), in SI. A number and its unit may be one word ("4.8V") or two ("4.8 V").
fn with_units(text: &str, units: &[(&str, f64)]) -> Vec<f64> {
    let t: String = text.chars().map(|c| if "≤≥<>=,;()·[]".contains(c) { ' ' } else { c }).collect();
    let toks: Vec<&str> = t.split_whitespace().collect();
    let mut out = vec![];
    for (i, tok) in toks.iter().enumerate() {
        let unit_of = |s: &str| units.iter().find(|(u, _)| s == *u).map(|(_, k)| *k);
        // "25Vdc" or "25" "Vdc"
        let split = tok.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '\u{2212}'));
        let (num, unit) = match split {
            Some(0) => continue,
            Some(j) => (&tok[..j], unit_of(&tok[j..])),
            None => (*tok, toks.get(i + 1).and_then(|n| unit_of(n))),
        };
        if let (Ok(v), Some(k)) = (num.replace('\u{2212}', "-").parse::<f64>(), unit) {
            out.push(v.abs() * k);
        }
    }
    out
}

const VOLTS: &[(&str, f64)] = &[("V", 1.0), ("Vdc", 1.0), ("mV", 1e-3)];
const AMPS: &[(&str, f64)] = &[("A", 1.0), ("Adc", 1.0), ("mA", 1e-3), ("mAdc", 1e-3), ("µA", 1e-6), ("μA", 1e-6), ("uA", 1e-6)];
const HERTZ: &[(&str, f64)] = &[("Hz", 1.0), ("kHz", 1e3), ("MHz", 1e6)];

/// The span of a range written in the conditions ("5.0 mA ≤ IO ≤ 1.0 A": 0.995), None without two values.
fn span(text: &str, units: &[(&str, f64)]) -> Option<f64> {
    let v = with_units(text, units);
    if v.len() < 2 {
        return None;
    }
    let (lo, hi) = (v.iter().copied().fold(f64::MAX, f64::min), v.iter().copied().fold(f64::MIN, f64::max));
    (hi > lo).then_some(hi - lo)
}

/// The output voltage a part's name tells: "MC7805" 5, "UA78L12" 12, "AMS1117-3.3" 3.3, "MCP1700-3302" 3.3, "LP5907-18"
/// 1.8.
pub fn nominal_from_name(name: &str) -> Option<f64> {
    let n = name.to_ascii_uppercase();
    // a decimal written out
    if let Some(i) = n.find(|c: char| c.is_ascii_digit())
        && let Some(d) = n[i..].split(|c: char| !(c.is_ascii_digit() || c == '.')).find(|s| s.contains('.'))
    {
        return d.parse().ok();
    }
    // 78xx / 79xx (with L or M between): the last two digits are volts
    for pre in ["78", "79"] {
        if let Some(i) = n.find(pre) {
            let rest = n[i + 2..].trim_start_matches(['L', 'M']);
            let d: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            // "7800" names the family, not a voltage; a 78xx is 24 V at most: "78L33" and "78L033" are 3.3 V (TI's UA78L)
            if d.len() == 2 {
                return d.parse::<f64>().ok().filter(|v| *v > 0.0).map(|v| if v > 24.0 { v / 10.0 } else { v });
            }
            if d.len() == 3 && d.starts_with('0') {
                return d.parse::<f64>().ok().filter(|v| *v > 0.0).map(|v| v / 10.0);
            }
        }
    }
    // a suffix after "-": "-33" 3.3, "-3302" 3.3 (Microchip: the first two digits), "-18" 1.8, "-50" 5.0
    let suffix = n.rsplit('-').next().filter(|_| n.contains('-'))?;
    let d: String = suffix.chars().take_while(|c| c.is_ascii_digit()).collect();
    (d.len() >= 2).then(|| d[..2].parse::<f64>().ok().map(|v| v / 10.0)).flatten()
}

impl RegulatorInput {
    /// Fills the boxes from the table. `name` picks the output voltage of a sheet for several (MC7800: 5 to 24 V, a
    /// page each), and the other values are taken from that page first. Adjustable when the table has an ADJ current.
    pub fn fill_from_pdf(&mut self, name: &str, values: impl Fn(&str) -> Vec<SpecValue>) {
        self.found.clear();
        let room = |v: &SpecValue| temperatures(&v.conditions).iter().all(|t| (t - 25.0).abs() < 0.5);
        let all = |symbols: &[&str]| -> Vec<SpecValue> { symbols.iter().flat_map(|s| values(s)).filter(room).collect() };
        let at = |v: &SpecValue| format!("p.{}「{} {}」", v.page + 1, v.symbol, v.conditions.trim());
        let si = |v: &SpecValue, x: f64, usual: f64| v.unit.as_deref().and_then(prefix).map_or(x * usual, |k| x * k);
        // adjustable: an ADJ current in the table
        let iadj = all(&["IAdj", "IADJ", "Adjustment", "Adjust"]);
        self.adjustable = iadj.iter().any(|v| v.typ.is_some());
        // the output (or reference) voltage, the one the name tells when there are several
        let want = nominal_from_name(name);
        let outs = all(if self.adjustable { &["VREF", "Vref", "Reference"] } else { &["VO", "VOUT", "Vout", "Output"] });
        let typed: Vec<&SpecValue> = outs.iter().filter(|v| v.typ.is_some() && v.unit.as_deref().is_none_or(|u| u == "V" || u == "?")).collect();
        let out = match want.filter(|_| !self.adjustable) {
            Some(w) => typed.iter().find(|v| (v.typ.unwrap_or_default() - w).abs() < 0.02 * w),
            None => typed.first(),
        };
        let page = out.map(|v| v.page);
        match out {
            Some(v) => {
                self.vout = show(v.typ.unwrap_or_default());
                if let (Some(lo), Some(hi)) = (v.min, v.max) {
                    self.vrange = format!("{} {}", show(lo), show(hi));
                }
                self.found.push((true, format!("{} {} V: {}", if self.adjustable { "基準電圧" } else { "出力電圧" }, self.vout, at(v))));
            }
            None => self.found.push((false, match want {
                Some(w) => format!("出力電圧 {w} V の行が表に見つかりません。自分で入れてください"),
                None => "出力電圧の typ が表に見つかりません。自分で入れてください".into(),
            })),
        }
        // the rows of a value: on the output voltage's page first (a sheet with a table per voltage)
        let rows = |symbols: &[&str]| -> Vec<SpecValue> {
            let mut r = all(symbols);
            r.sort_by_key(|v| (Some(v.page) != page, v.page));
            r
        };
        // line and load regulation: the change over the range in the conditions
        for (what, symbols, units, field, hint) in [
            ("ライン・レギュレーション", &["Regline", "REGLINE", "Line", "ΔVOLINE"][..], VOLTS, &mut self.line, "入力の変化"),
            ("ロード・レギュレーション", &["Regload", "REGLOAD", "Load", "ΔVOLOAD"][..], AMPS, &mut self.load, "電流の変化"),
        ] {
            let r = rows(symbols);
            let mut done = false;
            for v in r.iter().filter(|v| v.typ.is_some()) {
                let typ = v.typ.unwrap_or_default();
                let unit = v.unit.as_deref().unwrap_or("");
                // a percentage is written as a plain number ("0.01%/V": the box reads no prefix before "%")
                let pct = fit::num::g(typ, 6);
                if unit.contains("%/V") {
                    *field = format!("{pct}%/V");
                } else if let Some(s) = span(&v.conditions, units) {
                    *field = if unit.contains('%') { format!("{pct}% {}", show(s)) } else { format!("{} {}", show(si(v, typ, 1e-3)), show(s)) };
                } else {
                    continue;
                }
                self.found.push((true, format!("{what} {field}: {}", at(v))));
                done = true;
                break;
            }
            if !done {
                self.found.push((false, match r.first() {
                    Some(v) => format!("{what}: 行はありますが、条件から{hint}が読めません ({})。自分で入れてください", at(v)),
                    None => format!("{what}の行が表に見つかりません"),
                }));
            }
        }
        // one typ value with its usual unit
        // (`sane`: the value in SI is one such a part can have; a row read off by one line of the table is skipped)
        let mut one = |what: &str, symbols: &[&str], usual: f64, unit: &str, field: &mut String, max: Option<&mut String>, sane: fn(f64) -> bool| {
            match rows(symbols).into_iter().find(|v| v.typ.is_some_and(|t| sane(si(v, t, usual)))) {
                Some(v) => {
                    *field = show(si(&v, v.typ.unwrap_or_default(), usual));
                    let mut line = format!("{what} {field}: {}", at(&v));
                    if v.unit.as_deref().and_then(prefix).is_none() {
                        line += &format!(" (単位が読めないので {unit} とした)");
                    }
                    if let (Some(m), Some(x)) = (max, v.max) {
                        *m = show(si(&v, x, usual));
                        line += &format!("、max {m}");
                    }
                    self.found.push((true, line));
                }
                None => self.found.push((false, format!("{what}の typ が表に見つかりません"))),
            }
        };
        one("ドロップアウト", &["VI-VO", "VI−VO", "VDO", "VDROP", "Vdrop", "Dropout"], 1.0, "V", &mut self.vdo0, None, |v| (0.03..=5.0).contains(&v));
        if self.adjustable {
            one("ADJ の電流", &["IAdj", "IADJ", "Adjustment", "Adjust"], 1e-6, "µA", &mut self.iadj, Some(&mut self.iadj_max), |v| v > 0.0 && v < 1e-3);
            one("最小負荷電流", &["ILmin", "IL(min)", "IOmin", "Minimum"], 1e-3, "mA", &mut self.min_load, None, |v| v > 0.0 && v < 0.1);
        } else {
            one("グラウンド電流", &["IB", "IQ", "Iq", "IGND", "Quiescent", "Ground", "Bias"], 1e-3, "mA", &mut self.iq, None, |v| v > 0.0 && v < 0.1);
        }
        // the current limit at one input-output difference a row (two rows: the limit falling with the difference)
        let mut limits: Vec<(f64, f64, String)> = vec![];
        for v in rows(&["ISC", "Isc", "Ipk", "IPK", "ILIM", "ICL", "Current"]) {
            let Some(a) = v.typ.or(v.min) else { continue };
            // the difference the row is at: the one voltage of its conditions, written as the difference ("VIN − VOUT =
            // 40 V") or as the input voltage ("Vin = 35 Vdc": less the output voltage)
            let volts = with_units(&v.conditions, VOLTS);
            let c: String = datasheet::normalize(&v.conditions).chars().filter(|c| !c.is_whitespace()).collect();
            let diff = match volts.as_slice() {
                [d] if c.contains("vin-vout") || c.contains("vi-vo") => Some(*d),
                [d] if c.contains("vin=") || c.contains("vi=") => self.vout.parse::<f64>().ok().map(|vo| d - vo).filter(|d| *d > 0.0),
                // the row's own words with one voltage, the difference's name borrowed from the next row ("≤ 15V · (VIN −
                // VOUT) = 40V", TI's LM317)
                _ if c.contains("vin-vout") || c.contains("vi-vo") => v.conditions.split(" · ").next().map(|own| with_units(own, VOLTS)).filter(|o| o.len() == 1 && v.conditions.contains(" · ")).map(|o| o[0]),
                _ => None,
            };
            if let Some(d) = diff
                && !limits.iter().any(|l| (l.0 - d).abs() < 1e-9)
            {
                limits.push((d, si(&v, a, 1.0), at(&v)));
            }
        }
        limits.truncate(2);
        if limits.is_empty() {
            self.found.push((false, "電流制限: 入出力の差 (1 つの電圧) が条件にある行が見つかりません。自分で入れてください".into()));
        } else {
            limits.sort_by(|a, b| a.0.total_cmp(&b.0));
            self.ilim = limits.iter().map(|l| format!("{} {}", show(l.0), show(l.1))).collect::<Vec<_>>().join("\n");
            for l in &limits {
                self.found.push((true, format!("電流制限 {} V で {} A: {}", show(l.0), show(l.1), l.2)));
            }
            // one row: the limit is taken as the same at every difference
            if limits.len() == 1 {
                self.found.push((false, "電流制限は 1 行だけ読めました。入出力の差で制限が変わる部品 (78xx・LM317) なら、表のもう 1 行 (差の小さい所) を 2 行目に足してください".into()));
            }
        }
        // ripple rejection in dB at the frequency of its conditions
        match rows(&["RR", "PSRR", "Ripple"]).into_iter().find(|v| v.typ.is_some()) {
            Some(v) => {
                self.rr = show(v.typ.unwrap_or_default());
                let f = with_units(&v.conditions, HERTZ);
                let mut line = format!("リップル除去 {} dB: {}", self.rr, at(&v));
                match f.first() {
                    Some(f) => self.rr_f = show(*f),
                    None => line += " (周波数が条件に無いので、欄を確かめてください)",
                }
                if self.adjustable
                    && let Some(vo) = v.condition("VOUT").or_else(|| v.condition("VO"))
                {
                    self.rr_vout = show(vo);
                }
                self.found.push((true, line));
            }
            None => self.found.push((false, "リップル除去の typ が表に見つかりません".into())),
        }
        // output noise: a density (per √Hz) or an rms over the band of its conditions
        match rows(&["Vn", "VN", "eN", "en", "Noise"]).into_iter().find(|v| v.typ.is_some()) {
            Some(v) => {
                let unit = v.unit.as_deref().unwrap_or("");
                // "µV/VO": per volt of output (onsemi's 78xx)
                let per_volt = unit.contains("/VO") || unit.ends_with("/V");
                let k = if unit.starts_with('m') { 1e-3 } else if unit.starts_with('n') { 1e-9 } else { 1e-6 };
                let x = v.typ.unwrap_or_default() * k * if per_volt { self.vout.parse::<f64>().unwrap_or(1.0) } else { 1.0 };
                let band = with_units(&v.conditions, HERTZ);
                if !per_volt && (unit.contains('√') || unit.contains("rtHz") || unit.contains("Hz")) {
                    self.noise = show(x);
                    self.found.push((true, format!("雑音 {}V/√Hz: {}", self.noise, at(&v))));
                } else if let [lo, hi, ..] = band.as_slice() {
                    self.noise = format!("{} {} {}", show(x), show(lo.min(*hi)), show(lo.max(*hi)));
                    self.found.push((true, format!("雑音 {} V rms ({}〜{} Hz): {}", show(x), show(lo.min(*hi)), show(lo.max(*hi)), at(&v))));
                } else {
                    self.found.push((false, format!("雑音: 帯域が条件から読めません ({})。自分で入れてください", at(&v))));
                }
            }
            None => self.found.push((false, "雑音の typ が表に見つかりません".into())),
        }
        self.found.push((true, "表の行の読みは会社ごとにまちまちです。入れた値は表と見比べて確かめてください".into()));
    }

    /// Puts a curve read off a graph in its field (「表とグラフから全部読む」 and 「グラフから読む」): the dropout curve
    /// ((current, voltage) points) as the straight line vdo0 + rdo × I through them; the ripple rejection ((Hz, dB))
    /// at the field's frequency (120 Hz when empty) and the noise density ((Hz, V/√Hz)) at 1 kHz, each only when the
    /// table gave none (the table's value is the one the data sheet stands by; the graph's is said beside it). The line
    /// of what was put in, or why not.
    pub fn take_graph(&mut self, dest: super::graph_reader::Dest, points: &[(f64, f64)], page: usize) -> Result<String, String> {
        use super::graph_reader::Dest;
        let p = page + 1;
        match dest {
            Dest::RegDropout => {
                let pts: Vec<(f64, f64)> = points.iter().map(|&(i, v)| (i.abs(), v.abs())).filter(|&(i, v)| i.is_finite() && v.is_finite()).collect();
                let (vdo0, rdo) = dropout_line(&pts).ok_or_else(|| format!("p.{p} の図の点が少なすぎます ({} 点)", pts.len()))?;
                let top = pts.iter().map(|q| q.0).fold(0.0, f64::max);
                if !(0.0..=5.0).contains(&vdo0) || !(0.0..100.0).contains(&rdo) || vdo0 + rdo * top > 5.0 {
                    return Err(format!("p.{p} の図の値がドロップアウトらしくありません ({}V + {}Ω × I)。目盛りの単位を確かめてください", show(three(vdo0)), show(three(rdo))));
                }
                self.vdo0 = show(three(vdo0));
                self.rdo = show(three(rdo));
                Ok(format!("ドロップアウト: p.{p} の図の {} 点 (0〜{}A) に直線を当てて {}V + {}Ω × 電流", pts.len(), show(three(top)), self.vdo0, self.rdo))
            }
            Dest::RegRipple => {
                let f = if self.rr_f.trim().is_empty() {
                    120.0
                } else {
                    sim_core::parse_value(self.rr_f.trim()).filter(|f| *f > 0.0).ok_or_else(|| format!("リップル除去の周波数「{}」が数として読めません", self.rr_f))?
                };
                let pts: Vec<(f64, f64)> = points.iter().map(|&(hz, db)| (hz, db.abs())).collect();
                let db = at_frequency(&pts, f, false).ok_or_else(|| format!("p.{p} の図に {} Hz がありません (図は {})", show(f), range_text(&pts)))?;
                let db = (db * 10.0).round() / 10.0;
                if !(1.0..=200.0).contains(&db) {
                    return Err(format!("p.{p} の図の {} Hz の値 {db} dB はリップル除去らしくありません", show(f)));
                }
                if !self.rr.trim().is_empty() {
                    return Ok(format!("リップル除去: 表の {} dB のまま (p.{p} の図では {} Hz で {db} dB)", self.rr.trim(), show(f)));
                }
                self.rr = show(db);
                self.rr_f = show(f);
                Ok(format!("リップル除去: p.{p} の図から {} Hz で {db} dB", show(f)))
            }
            Dest::RegNoise => {
                let d = at_frequency(points, 1e3, true).ok_or_else(|| format!("p.{p} の図に 1 kHz がありません (図は {})", range_text(points)))?;
                // more than 1 mV/√Hz is no regulator's: the axis' unit (nV/√Hz, µV/√Hz) was not read
                if !(d > 0.0 && d < 1e-4) {
                    return Err(format!("p.{p} の図の縦軸の単位 (nV/√Hz か µV/√Hz) が読めません。1 kHz の値を読んで「出力雑音」に密度 (例: 3u) を入れてください"));
                }
                let d = show(three(d));
                if !self.noise.trim().is_empty() {
                    return Ok(format!("雑音: 表の「{}」のまま (p.{p} の図では 1 kHz で {d}V/√Hz)", self.noise.trim()));
                }
                self.noise = d.clone();
                Ok(format!("雑音: p.{p} の図から 1 kHz の密度 {d}V/√Hz"))
            }
            _ => Err("レギュレータの欄ではありません".into()),
        }
    }
}

/// The output voltages of a data sheet for several, each once with its shortest part number, lowest first: the voltage
/// each part number tells (`nominal_from_name`: "LP5907MFX-3.3", "MC7812CTG", "MCP1700T-3302E"), then the listed
/// options with no part number of their own, named after the family ("MCP1700-2.8"). None for a part of one voltage.
pub fn variants(parts: &[(String, usize)], options: &[f64], name: &str) -> Vec<(f64, String)> {
    let mut out: Vec<(f64, String)> = vec![];
    for (p, _) in parts {
        let Some(v) = nominal_from_name(p).filter(|v| *v > 0.0 && *v <= 60.0) else { continue };
        // the shortest part number of a voltage names it ("AMS1117-3.3" before "AMS1117CD-3.3", "MC7805" before "MC7805B")
        match out.iter_mut().find(|o| (o.0 - v).abs() < 1e-9) {
            Some(o) if p.len() < o.1.len() => o.1 = p.clone(),
            Some(_) => {}
            None => out.push((v, p.clone())),
        }
    }
    let family = datasheet::ordering::family(name);
    for &v in options {
        if !out.iter().any(|o| (o.0 - v).abs() < 1e-9) {
            out.push((v, format!("{family}-{}", show(v))));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    if out.len() < 2 { vec![] } else { out }
}

impl super::PartForm {
    /// The regulator's voltage variants from the open PDF's part numbers (shown as buttons in its section).
    pub(super) fn reg_find_variants(&mut self) {
        let (parts, options) = self.sheet.part_numbers(&self.name);
        self.reg.variants = variants(&parts, &options, &self.name);
        if !self.reg.variants.is_empty() && self.reg.vout.trim().is_empty() && !self.reg.adjustable {
            self.reg.found.push((true, format!("この 1 冊は出力電圧が {} 通りあります。下の「電圧違い」で型番を選ぶと、出力電圧が入ります", self.reg.variants.len())));
        }
    }

    /// One of the variants picked: its part number as the part's name, the table read again for it (the page of its
    /// voltage, MC7800), and its voltage as the output when the table has none (LP5907: "±2 %" only).
    pub(super) fn reg_choose_variant(&mut self, v: f64, part: &str) {
        self.name = part.to_string();
        let (sheet, name) = (&self.sheet, self.name.clone());
        self.reg.fill_from_pdf(&name, |s| sheet.spec_values(s));
        if self.reg.vout.trim().is_empty() {
            self.reg.vout = show(v);
            self.reg.found.insert(0, (true, format!("出力電圧 {} V: 型番 {part} から", show(v))));
        }
        self.reg.result = None;
    }
}

/// A number to 3 significant digits (a value read off a graph).
fn three(v: f64) -> f64 {
    format!("{v:.2e}").parse().unwrap_or(v)
}

/// The straight line V = vdo0 + rdo × I that fits the dropout points (current, voltage) by least squares: (vdo0, rdo).
/// A line that would start below 0 V is drawn from 0 (a MOSFET pass device: the dropout is its resistance); one
/// falling with the current is flat at the mean. None with fewer than 3 points or one current only.
fn dropout_line(pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    if pts.len() < 3 {
        return None;
    }
    let n = pts.len() as f64;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let b = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f64>() / sxx;
    let a = my - b * mx;
    if b < 0.0 {
        return Some((my, 0.0));
    }
    if a < 0.0 {
        let sxx0: f64 = pts.iter().map(|p| p.0 * p.0).sum();
        return Some((0.0, pts.iter().map(|p| p.0 * p.1).sum::<f64>() / sxx0));
    }
    Some((a, b))
}

/// The curve's value at frequency `f`, between its points on a log frequency axis (and log values when `log`). None
/// when `f` is outside the points' frequencies.
fn at_frequency(pts: &[(f64, f64)], f: f64, log: bool) -> Option<f64> {
    let mut v: Vec<(f64, f64)> = pts.iter().copied().filter(|p| p.0 > 0.0 && (!log || p.1 > 0.0)).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let w = v.windows(2).find(|w| w[0].0 <= f && f <= w[1].0)?;
    let t = if w[1].0 > w[0].0 { (f.ln() - w[0].0.ln()) / (w[1].0.ln() - w[0].0.ln()) } else { 0.0 };
    Some(if log { (w[0].1.ln() + t * (w[1].1.ln() - w[0].1.ln())).exp() } else { w[0].1 + t * (w[1].1 - w[0].1) })
}

/// The frequencies a curve covers, for a message ("10〜100k Hz").
fn range_text(pts: &[(f64, f64)]) -> String {
    let lo = pts.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let hi = pts.iter().map(|p| p.0).fold(0.0, f64::max);
    if lo.is_finite() { format!("{}〜{} Hz", show(three(lo)), show(three(hi))) } else { "点なし".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(page: usize, symbol: &str, conditions: &str, min: Option<f64>, typ: f64, max: Option<f64>, unit: Option<&str>) -> SpecValue {
        SpecValue { page, symbol: symbol.into(), conditions: conditions.into(), min, typ: Some(typ), max, unit: unit.map(str::to_string), rect: datasheet::Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 } }
    }

    #[test]
    fn ranges_and_names_are_read() {
        assert_eq!(span("7.5 Vdc ≤ Vin ≤ 25 Vdc, IO = 500 mA", VOLTS), Some(17.5));
        assert!((span("5.0 mA ≤ IO ≤ 1.0 A", AMPS).unwrap() - 0.995).abs() < 1e-12);
        assert_eq!(span("TJ = 25°C", VOLTS), None);
        assert_eq!(with_units("f = 120Hz", HERTZ), vec![120.0]);
        for (n, want) in [("MC7805", Some(5.0)), ("UA78L12", Some(12.0)), ("AMS1117-3.3", Some(3.3)), ("MCP1700-3302", Some(3.3)), ("LP5907-18", Some(1.8)), ("LM317", None), ("MC7800", None), ("UA78L33", Some(3.3)), ("UA78L033", Some(3.3)), ("MCP1700T-3302E", Some(3.3)), ("LP5907MFX-2.85", Some(2.85))] {
            assert_eq!(nominal_from_name(n), want, "{n}");
        }
    }

    #[test]
    fn a_fixed_regulator_from_an_onsemi_table() {
        // MC7800-like: a page per voltage
        let table = [
            v(2, "VO", "Output Voltage (TJ = 25°C)", Some(4.8), 5.0, Some(5.2), None),
            v(4, "VO", "Output Voltage (TJ = 25°C)", Some(7.7), 8.0, Some(8.3), None),
            v(4, "Regline", "10.5 Vdc ≤ Vin ≤ 25 Vdc", None, 6.0, Some(32.0), Some("mV")),
            v(2, "Regline", "7.5 20 1.0 5.0 100", None, 0.5, Some(20.0), Some("mV")),
            v(2, "Regline", "7.5 Vdc ≤ Vin ≤ 25 Vdc, IO = 500 mA", None, 0.5, Some(10.0), Some("mV")),
            v(2, "Regload", "5.0 mA ≤ IO ≤ 1.0 A", None, 1.3, Some(25.0), Some("mV")),
            v(2, "IB", "", None, 3.2, Some(8.0), Some("mA")),
            v(2, "VI-VO", "IO = 1.0 A, TJ = 25°C", None, 2.0, None, Some("Vdc")),
            v(2, "ISC", "Vin = 35 Vdc", None, 0.2, None, Some("A")),
            v(2, "Ipk", "TJ = 25°C", None, 2.2, None, Some("A")),
            v(2, "RR", "f = 120 Hz, 8.0 V ≤ Vin ≤ 18 V", Some(62.0), 83.0, None, Some("dB")),
            v(2, "Vn", "TA = 25°C, 10 Hz ≤ f ≤ 100 kHz", None, 10.0, None, Some("µV/VO")),
        ];
        let mut r = RegulatorInput::default();
        r.fill_from_pdf("MC7805", |s| table.iter().filter(|x| x.symbol == s).cloned().collect());
        assert!(!r.adjustable);
        assert_eq!((r.vout.as_str(), r.vrange.as_str()), ("5", "4.8 5.2"));
        assert_eq!(r.line, "500u 17.5", "the 7805's page, the row with its input range");
        assert_eq!(r.load, "1.3m 995m");
        assert_eq!(r.iq, "3.2m");
        assert_eq!(r.vdo0, "2");
        assert_eq!(r.ilim, "30 200m", "Vin = 35 V less the 5 V out; the peak current's row has no voltage");
        assert_eq!(r.noise, "50u 10 100k", "10 µV per volt of output, over the band of its conditions");
        assert_eq!((r.rr.as_str(), r.rr_f.as_str()), ("83", "120"));
        assert!(r.found.iter().any(|(ok, l)| *ok && l.contains("リップル除去")));
    }

    #[test]
    fn an_adjustable_regulator_from_a_table_of_names() {
        // LM317-like: rows named by their first word
        let table = [
            v(7, "Reference", "TJ = 25°C", None, 1.25, None, Some("V")),
            v(7, "Line", "VI − VO = 3 V to 40 V, TJ = 25°C", None, 0.01, Some(0.04), Some("%/V")),
            v(7, "Load", "IO = 10mA to 1.5A, TJ = 25°C", None, 0.1, Some(0.5), Some("%")),
            v(7, "Adjustment", "", None, 50.0, Some(100.0), Some("μA")),
            v(7, "Minimum", "(VIN − VOUT) = 40V", None, 3.5, Some(10.0), Some("mA")),
            v(7, "Current", "(VIN − VOUT) = 40V PD < PMAX, TJ = 25°C", Some(0.15), 0.4, None, Some("A")),
            v(7, "Ripple", "VOUT = 10V, f = 120Hz, CADJ = 10μF", Some(62.0), 64.0, None, None),
            // the row at the small difference, its own words with one voltage and the next row's name borrowed (TI's LM317)
            v(7, "Current", "15V (3) · (VIN − VOUT) = 40V PD < PMAX (3), TJ =", Some(1.5), 2.2, None, Some("A")),
        ];
        let mut r = RegulatorInput::default();
        r.fill_from_pdf("LM317", |s| table.iter().filter(|x| x.symbol == s).cloned().collect());
        assert!(r.adjustable);
        assert_eq!(r.vout, "1.25");
        assert_eq!(r.line, "0.01%/V");
        assert_eq!(r.load, "0.1% 1.49");
        assert_eq!((r.iadj.as_str(), r.iadj_max.as_str(), r.min_load.as_str()), ("50u", "100u", "3.5m"));
        assert_eq!(r.ilim, "15 2.2\n40 400m");
        assert_eq!((r.rr.as_str(), r.rr_f.as_str(), r.rr_vout.as_str()), ("64", "120", "10"));
        assert!(r.found.iter().any(|(ok, l)| !ok && l.contains("ドロップアウト")), "no dropout row: said");
        // a dropout row read off by a line (5 mV) is passed over for the next
        let t2 = [v(2, "Dropout", "VIN = 6.5V", None, 5.0, Some(20.0), Some("mV")), v(2, "Dropout", "IOUT = 0.8A", None, 1.1, Some(1.3), Some("V"))];
        r.fill_from_pdf("AMS1117", |s| t2.iter().filter(|x| x.symbol == s).cloned().collect());
        assert_eq!(r.vdo0, "1.1");
    }

    #[test]
    fn voltage_variants_from_part_numbers() {
        let parts: Vec<(String, usize)> = ["LP5907SN", "LP5907A33YKMR", "LP5907MFX1.2NOPBG4", "LP5907MFX-1.2", "LP5907MFX-3.3", "LP5907SNX-2.75"].iter().map(|p| (p.to_string(), 30)).collect();
        let v = variants(&parts, &[], "LP5907");
        assert_eq!(v, vec![(1.2, "LP5907MFX-1.2".to_string()), (2.75, "LP5907SNX-2.75".to_string()), (3.3, "LP5907MFX-3.3".to_string())]);
        // the options listed with no part number of their own
        let parts = vec![("MCP1700T-3302E".to_string(), 20)];
        let v = variants(&parts, &[1.2, 3.3], "MCP1700");
        assert_eq!(v, vec![(1.2, "MCP1700-1.2".to_string()), (3.3, "MCP1700T-3302E".to_string())]);
        // one voltage: nothing to pick
        assert!(variants(&[("LM317KCS".to_string(), 3)], &[], "LM317").is_empty());
        // picked: the part's name and, the table having none, its voltage
        let mut f = crate::part_form::PartForm { name: "LP5907".into(), ..Default::default() };
        f.reg_choose_variant(3.3, "LP5907MFX-3.3");
        assert_eq!((f.name.as_str(), f.reg.vout.as_str()), ("LP5907MFX-3.3", "3.3"));
        assert!(f.reg.found[0].1.contains("LP5907MFX-3.3"), "{:?}", f.reg.found);
    }

    #[test]
    fn regulator_graphs_go_into_their_fields() {
        use crate::part_form::graph_reader::Dest;
        let mut r = RegulatorInput::default();
        // a MOSFET pass device (LP5907's Figure 5-18): through 0, 120 mV at 250 mA
        let pts: Vec<(f64, f64)> = (0..=10).map(|k| (0.025 * k as f64, 0.48 * 0.025 * k as f64)).collect();
        let line = r.take_graph(Dest::RegDropout, &pts, 8).unwrap();
        assert_eq!((r.vdo0.as_str(), r.rdo.as_str()), ("0", "480m"), "{line}");
        // a bipolar one: 1.5 V and 0.25 Ω
        let pts: Vec<(f64, f64)> = (1..=6).map(|k| (0.25 * k as f64, 1.5 + 0.25 * 0.25 * k as f64)).collect();
        r.take_graph(Dest::RegDropout, &pts, 8).unwrap();
        assert_eq!((r.vdo0.as_str(), r.rdo.as_str()), ("1.5", "250m"));
        // a dropout in volts read as millivolts is refused
        let big: Vec<(f64, f64)> = pts.iter().map(|p| (p.0, p.1 * 1e3)).collect();
        assert!(r.take_graph(Dest::RegDropout, &big, 8).is_err() && r.vdo0 == "1.5");

        // the ripple rejection at 120 Hz on a falling curve (dB may be read below 0)
        let rr = [(10.0, -80.0), (100.0, -78.0), (1e3, -70.0), (1e4, -55.0), (1e5, -40.0)];
        let line = r.take_graph(Dest::RegRipple, &rr, 9).unwrap();
        assert_eq!((r.rr.as_str(), r.rr_f.as_str()), ("77.4", "120"), "{line}");
        // the table's value stays
        r.rr = "83".into();
        let line = r.take_graph(Dest::RegRipple, &rr, 9).unwrap();
        assert!(r.rr == "83" && line.contains("77.4"), "{line}");

        // the noise density at 1 kHz; a density whose unit was not read (nV taken as V) is refused
        let nd = [(10.0, 100e-9), (100.0, 30e-9), (1e4, 10e-9)];
        r.take_graph(Dest::RegNoise, &nd, 11).unwrap();
        assert_eq!(r.noise, "17.3n");
        let raw: Vec<(f64, f64)> = nd.iter().map(|p| (p.0, p.1 * 1e9)).collect();
        r.noise.clear();
        assert!(r.take_graph(Dest::RegNoise, &raw, 11).unwrap_err().contains("単位") && r.noise.is_empty());
    }
}
