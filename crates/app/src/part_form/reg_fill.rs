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
    // ROHM: "BD" or "BA" then the voltage's digits and a letter ("BD33HC5WEFJ" 3.3, "BA033CC0" 3.3; not "BA1117FP")
    for pre in ["BD", "BA"] {
        if let Some(rest) = n.strip_prefix(pre) {
            let d: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            let letter = rest[d.len()..].starts_with(|c: char| c.is_ascii_alphabetic());
            if letter && (d.len() == 2 || (d.len() == 3 && d.starts_with('0'))) {
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
    /// The output's range from a row of fractions of it (`r`: min 0.99, max 1.01, unit "×VO") and the output in the box.
    fn apply_output_fraction(&mut self, r: &SpecValue) {
        if let (Ok(vo), Some(lo), Some(hi)) = (self.vout.trim().parse::<f64>(), r.min, r.max) {
            self.vrange = format!("{} {}", show(vo * lo), show(vo * hi));
        }
    }

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
        // (volts: "V", onsemi's "Vdc", or no unit read)
        let typed: Vec<&SpecValue> = outs.iter().filter(|v| v.typ.is_some() && v.unit.as_deref().is_none_or(|u| u == "V" || u == "?" || u.eq_ignore_ascii_case("vdc"))).collect();
        let out = match want.filter(|_| !self.adjustable) {
            Some(w) => typed.iter().find(|v| (v.typ.unwrap_or_default() - w).abs() < 0.02 * w),
            None => typed.first(),
        };
        let page = out.map(|v| v.page);
        // the limits as fractions of the output (ROHM: "VO×0.99 VO VO×1.01"), for the output the name tells or the one in the box
        let fraction = if self.adjustable { None } else { outs.iter().find(|v| v.unit.as_deref() == Some("×VO")) };
        let nominal = want.or_else(|| self.vout.trim().parse::<f64>().ok().filter(|v| *v > 0.0));
        match out {
            Some(v) => {
                self.vout = show(v.typ.unwrap_or_default());
                if let (Some(lo), Some(hi)) = (v.min, v.max) {
                    self.vrange = format!("{} {}", show(lo), show(hi));
                }
                self.found.push((true, format!("{} {} V: {}", if self.adjustable { "基準電圧" } else { "出力電圧" }, self.vout, at(v))));
            }
            None if let (Some(r), Some(vo)) = (fraction, nominal) => {
                self.vout = show(vo);
                self.apply_output_fraction(r);
                let from = if want.is_some() { "型番から" } else { "入れてある値" };
                self.found.push((true, format!("出力電圧 {} V ({from})、範囲 {} V: 表の比 {}〜{} 倍 {}", self.vout, self.vrange, show(r.min.unwrap_or_default()), show(r.max.unwrap_or_default()), at(r))));
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
                    // a percentage per unit of the change ("0.001 %/mA", LP5907): times the change in that unit
                    let per = unit.split_once("%/").and_then(|(_, u)| datasheet::spec_table::prefix(u));
                    *field = match per {
                        Some(k) if k > 0.0 => format!("{}% {}", fit::num::g(typ * s / k, 6), show(s)),
                        _ if unit.contains('%') => format!("{pct}% {}", show(s)),
                        _ => format!("{} {}", show(si(v, typ, 1e-3)), show(s)),
                    };
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
                // (the output voltage from the table, else from the part number: MC7805's table writes "Vdc", a family's name none)
                [d] if c.contains("vin=") || c.contains("vi=") => self.vout.parse::<f64>().ok().or(want).map(|vo| d - vo).filter(|d| *d > 0.0),
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
        // a short-circuit current with no voltage in its row (TI's LP5907: "TA = 25°C", the input in the table's heading "VIN =
        // VOUT(NOM) + 1V"): the output shorted to 0 V with the input 1 V over the output, said as an assumption
        let mut assumed = None;
        if limits.is_empty()
            && let Some(vo) = self.vout.parse::<f64>().ok().or(want).filter(|v| *v > 0.0)
            && let Some(v) = rows(&["ISC", "Isc"]).into_iter().find(|v| v.typ.or(v.min).is_some() && with_units(&v.conditions, VOLTS).is_empty())
        {
            let a = v.typ.or(v.min).unwrap_or_default();
            limits.push((vo + 1.0, si(&v, a, 1.0), at(&v)));
            assumed = Some(vo + 1.0);
        }
        if limits.is_empty() {
            self.found.push((false, "電流制限: 入出力の差 (1 つの電圧) が条件にある行が見つかりません。自分で入れてください".into()));
        } else {
            limits.sort_by(|a, b| a.0.total_cmp(&b.0));
            self.ilim = limits.iter().map(|l| format!("{} {}", show(l.0), show(l.1))).collect::<Vec<_>>().join("\n");
            for l in &limits {
                self.found.push((true, format!("電流制限 {} V で {} A: {}", show(l.0), show(l.1), l.2)));
            }
            if let Some(d) = assumed {
                self.found.push((false, format!("電流制限: 短絡電流の行に電圧が無いので、出力を 0 V に短絡し入力は出力 + 1 V (入出力の差 {} V) として入れました。表の上の共通の条件と見比べてください", show(d))));
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
        // the packages the part numbers are ordered in (for making every version at once)
        self.reg.parts = parts.iter().filter_map(|(p, _)| nominal_from_name(p).map(|v| (v, p.clone()))).collect();
        self.reg.packages = self.sheet.part_packages(&self.name);
        self.reg.grades = self.sheet.grades();
        self.reg.temps = self.sheet.part_temperatures(&self.name);
        if !self.reg.variants.is_empty() && self.reg.vout.trim().is_empty() && !self.reg.adjustable {
            self.reg.found.push((true, format!("この 1 冊は出力電圧が {} 通りあります。下の「電圧違い」で型番を選ぶと、出力電圧が入ります", self.reg.variants.len())));
        }
    }

    /// One of the variants picked: its part number as the part's name, the table read again for it (the page of its
    /// voltage, MC7800), and its voltage as the output when the table has none (LP5907: "±2 %" only).
    pub(super) fn reg_choose_variant(&mut self, v: f64, part: &str) {
        self.name = part.to_string();
        let (sheet, name) = (&self.sheet, self.name.clone());
        self.reg.fill_from_pdf(&name, |s| sheet.spec_values_for(s, &name));
        if self.reg.vout.trim().is_empty() {
            self.reg.vout = show(v);
            self.reg.found.insert(0, (true, format!("出力電圧 {} V: 型番 {part} から", show(v))));
            // its range from the table's fractions of the output (ROHM's "VO×0.99 VO VO×1.01")
            if let Some(r) = ["VO", "VOUT"].iter().flat_map(|s| sheet.spec_values_for(s, &name)).find(|r| r.unit.as_deref() == Some("×VO")) {
                self.reg.apply_output_fraction(&r);
            }
        }
        self.reg.result = None;
    }
}

/// The versions to make of a family: per voltage, one part number per package and grade (the shortest), from the part
/// numbers (`parts`: voltage, part number), their packages (`packages`: part number, package name) and the grades the tables
/// print over their columns (`grades`: onsemi's MC7805B −40 to 125 °C, MC7805C 0 to 125 °C, MC7805AB ±2 %, each read in its
/// own columns; none: one grade). A voltage whose part numbers name no package is one version, named as `variants` names
/// it, in the form's package (`None`).
pub fn versions(variants: &[(f64, String)], parts: &[(f64, String)], packages: &[(String, String)], grades: &[String]) -> Vec<(f64, String, Option<String>)> {
    let mut out: Vec<(f64, String, Option<String>)> = vec![];
    // the family's part numbers name their packages: a voltage only in a package not named (TI's LP5907YKGR-2.825, a DSBGA)
    // is not made in the form's package; it is left to the caller to say so (an empty package name)
    let named = parts.iter().any(|(_, p)| packages.iter().any(|(n, _)| n.eq_ignore_ascii_case(p)));
    for (v, first) in variants {
        // (part number, package, grade)
        let mut here: Vec<(String, String, Option<String>)> = vec![];
        for (_, p) in parts.iter().filter(|(pv, _)| (pv - v).abs() < 1e-9) {
            let Some(pkg) = packages.iter().find(|(n, _)| n.eq_ignore_ascii_case(p)).map(|x| x.1.clone()) else { continue };
            let grade = datasheet::spec_table::grade_of(grades, p);
            match here.iter_mut().find(|h| h.1 == pkg && h.2 == grade) {
                Some(h) if p.len() < h.0.len() => h.0 = p.clone(),
                Some(_) => {}
                None => here.push((p.clone(), pkg, grade)),
            }
        }
        if here.is_empty() {
            out.push((*v, first.clone(), named.then(String::new)));
        } else {
            out.extend(here.into_iter().map(|(p, pkg, _)| (*v, p, Some(pkg))));
        }
    }
    out
}

impl super::PartForm {
    /// Every voltage (and package) of the data sheet made at once, as a family (`part_lib::variants::VariantSeries`): for each
    /// version the table read again for its part number (`reg_choose_variant`), the regulator fitted at its voltage, put into
    /// 「② シミュレーション」, and its package's built-in footprint chosen (a package with none is left out and said so). The form
    /// is put back as it was. Returns the family and one line per version left out.
    pub(super) fn reg_series(&mut self) -> Result<(part_lib::variants::VariantSeries, Vec<String>), String> {
        if self.reg.adjustable || self.reg.variants.is_empty() {
            return Err("電圧違いの型番がありません (「PDF の表から読む」で注文情報の型番を探します)".into());
        }
        let list = versions(&self.reg.variants, &self.reg.parts, &self.reg.packages, &self.reg.grades);
        // what is changed for each version, to put back
        let keep = (self.name.clone(), self.reg.clone(), self.sim, self.sim_name.clone(), self.sim_text.clone(), self.sim_pins.clone(), self.sim_limits.clone(), self.sim_origin.clone(), self.description.clone(), self.fp_preset.clone());
        let keep_pins = self.pins_text.clone();
        // the pin table's column of each package (TI's LP5907: X2SON 1 OUT … 4 IN, SOT-23 1 IN … 5 OUT): each version takes its
        // own package's pins, not the form's (the X2SON's five numbers fit a SOT-23-5 too)
        let columns: Vec<datasheet::draft::FoundPins> = self.sheet.datasheet().map(|ds| ds.find_pin_tables()).unwrap_or_default();
        let mut made: Vec<(part_lib::PartDef, part_lib::variants::Variant)> = vec![];
        let mut left = vec![];
        for (v, part, pkg) in &list {
            self.name = keep.0.clone();
            self.reg = keep.1.clone();
            self.description = keep.8.clone();
            self.fp_preset = keep.9.clone();
            // the version's own output: its table's (page of its voltage), else its part number's. What the table does not
            // give again for it follows its voltage: the output's range in proportion, the test input by the difference
            let old = keep.1.vout.trim().parse::<f64>().ok().filter(|o| *o > 0.0);
            self.reg.vout.clear();
            self.reg.vrange.clear();
            self.reg_choose_variant(*v, part);
            if let Some(o) = old {
                let nums = |t: &str| t.split_whitespace().map(str::parse::<f64>).collect::<Result<Vec<_>, _>>().ok();
                if self.reg.vrange.trim().is_empty()
                    && let Some(r) = nums(&keep.1.vrange).filter(|r| r.len() == 2)
                {
                    self.reg.vrange = format!("{} {}", show(r[0] * v / o), show(r[1] * v / o));
                }
                if self.reg.vin0 == keep.1.vin0
                    && let Ok(vin) = keep.1.vin0.trim().parse::<f64>()
                {
                    self.reg.vin0 = show(vin + v - o);
                }
            }
            self.pins_text = keep_pins.clone();
            if let Some(pkg) = pkg {
                // (a voltage whose part numbers name no package the data sheet's names tell: `versions`)
                let Some((family, pins)) = datasheet::draft::package_of(pkg) else {
                    left.push(format!("{part}: 注文情報の表のパッケージが読めないので作りませんでした (DSBGA など。形を選んで 1 つずつ作ってください)"));
                    continue;
                };
                let name = datasheet::draft::PackageName { name: pkg.clone(), family, pins, count: 1, page: 0 };
                if columns.len() > 1
                    && let Some(i) = super::draft::pins_of_package(&columns, &name)
                {
                    self.pins_text = super::pin_import::lines_with_kinds(&columns[i].pins, &columns[i].io);
                }
                let numbers: Vec<String> = self.pins_text.lines().filter_map(|l| l.split_whitespace().next().map(str::to_string)).collect();
                match super::draft::builtin_footprint(&name, Some(&numbers)) {
                    Some(fp) => self.fp_preset = fp,
                    None => {
                        left.push(match super::draft::builtin_footprint(&name, None) {
                            Some(fp) => format!("{part} ({pkg}): ピン表の番号 ({}) が組み込みの {fp} のパッドと合わないので作りませんでした (「③ ピン」を直してください)", numbers.join(" ")),
                            None => format!("{part} ({pkg}): 組み込みの形が無いので作りませんでした"),
                        });
                        continue;
                    }
                }
            }
            self.regulator_make();
            let fitted = match self.reg.result.clone() {
                Some(Ok(f)) => f,
                Some(Err(e)) => {
                    left.push(format!("{part}: {e}"));
                    continue;
                }
                None => continue,
            };
            let def = self.linear_apply(&fitted, self.reg_roles()).and_then(|_| self.to_def());
            match def {
                Ok(d) => {
                    let mut note = match pkg {
                        Some(p) => format!("出力 {} V、{p}", show(*v)),
                        None => format!("出力 {} V", show(*v)),
                    };
                    // the temperature range it is ordered for (onsemi's B and C grades differ in it)
                    if let Some((_, (lo, hi))) = self.reg.temps.iter().find(|t| t.0.eq_ignore_ascii_case(part)) {
                        note += &format!("、{}〜{} ℃", show(*lo), show(*hi));
                    }
                    let variant = part_lib::variants::Variant { name: d.name.clone(), value: format!("{}V", show(*v)), sim: d.sim.clone(), package: Some((d.footprint_name(), d.package.clone())), note };
                    made.push((d, variant));
                }
                Err(e) => left.push(format!("{part}: {e}")),
            }
        }
        (self.name, self.reg, self.sim, self.sim_name, self.sim_text, self.sim_pins, self.sim_limits, self.sim_origin, self.description, self.fp_preset) = keep;
        self.pins_text = keep_pins;
        let Some(base) = made.first().map(|m| m.0.clone()) else { return Err(format!("1 つも作れませんでした: {}", left.join(" / "))) };
        // the base's description is the form's own (each version adds its voltage and package)
        let base = part_lib::PartDef { description: self.description.clone(), ..base };
        let series = part_lib::variants::VariantSeries { series: datasheet::ordering::family(&self.name), base, variants: made.into_iter().map(|m| m.1).collect() };
        Ok((series, left))
    }

    /// The versions read from the open PDF (keeping the regulator's values) and all made (`--form-variants`).
    pub fn reg_variants_and_make_all(&mut self) {
        self.reg_find_variants();
        self.reg_make_all();
    }

    /// The regulator section's last message (for the command line).
    pub fn reg_message(&self) -> String {
        self.reg.message.as_ref().map_or(String::new(), |m| m.1.clone())
    }

    /// The button's work: the family made and its parts handed to the editor to save (`PartForm::batch`).
    pub(super) fn reg_make_all(&mut self) {
        match self.reg_series() {
            Ok((series, left)) => {
                let parts = series.expand();
                let names: Vec<String> = parts.iter().map(|p| p.name.clone()).collect();
                let mut msg = format!("{} 部品を作ってライブラリに保存します: {}", parts.len(), names.join("、"));
                if !left.is_empty() {
                    msg += &format!(" (作らなかったもの: {})", left.join(" / "));
                }
                self.reg.message = Some((true, msg));
                self.batch = parts;
            }
            Err(e) => self.reg.message = Some((false, e)),
        }
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
        for (n, want) in [("MC7805", Some(5.0)), ("UA78L12", Some(12.0)), ("AMS1117-3.3", Some(3.3)), ("MCP1700-3302", Some(3.3)), ("LP5907-18", Some(1.8)), ("LM317", None), ("MC7800", None), ("UA78L33", Some(3.3)), ("UA78L033", Some(3.3)), ("MCP1700T-3302E", Some(3.3)), ("LP5907MFX-2.85", Some(2.85)), ("BD33HC5WEFJ", Some(3.3)), ("BD50HC5WEFJ", Some(5.0)), ("BA033CC0", Some(3.3)), ("BA1117FP", None), ("BDxxHC5WEFJ", None)] {
            assert_eq!(nominal_from_name(n), want, "{n}");
        }
    }

    /// ROHM's fixed outputs: the output's limits as fractions of it ("VO×0.99 VO VO×1.01", read as 0.99 1 1.01 "×VO"), the
    /// output from the part number, or from the box when the name gives none.
    #[test]
    fn an_output_range_as_fractions_of_the_output() {
        let table = [v(4, "VO", "", Some(1.5), 0.0, Some(7.0), Some("V")), v(4, "VO", "IO=0A", Some(0.99), 1.0, Some(1.01), Some("×VO"))];
        let table = [SpecValue { typ: None, ..table[0].clone() }, table[1].clone()];
        let values = |s: &str| table.iter().filter(|r| r.symbol == s).cloned().collect::<Vec<_>>();
        let mut r = RegulatorInput::default();
        r.fill_from_pdf("BD33HC5WEFJ", values);
        assert_eq!((r.vout.as_str(), r.vrange.as_str()), ("3.3", "3.267 3.333"), "{:?}", r.found);
        let mut r = RegulatorInput { vout: "5".into(), ..Default::default() };
        r.fill_from_pdf("BDxxHC5WEFJ", values);
        assert_eq!((r.vout.as_str(), r.vrange.as_str()), ("5", "4.95 5.05"), "{:?}", r.found);
        // nothing to multiply: said so, as before
        let mut r = RegulatorInput::default();
        r.fill_from_pdf("BDxxHC5WEFJ", values);
        assert!(r.vout.is_empty() && r.found.iter().any(|f| !f.0), "{:?}", r.found);
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
        // a load regulation per milliamp (LP5907's 0.001 %/mA over 1 mA to 250 mA): 0.249 % over the change
        let t3 = [v(4, "Load", "IOUT = 1mA to 250mA", None, 0.001, None, Some("%/mA"))];
        r.fill_from_pdf("LP5907", |s| t3.iter().filter(|x| x.symbol == s).cloned().collect());
        assert_eq!(r.load, "0.249% 249m");
        // onsemi's MC7805 writes its output in "Vdc"; the short-circuit row's "Vin = 35 Vdc" less it is the difference
        let t4 = [v(2, "VO", "Output Voltage (TJ = 25°C)", Some(4.8), 5.0, Some(5.2), Some("Vdc")), v(2, "ISC", "Vin = 35 Vdc", None, 0.2, None, Some("A"))];
        r.fill_from_pdf("MC7805BTG", |s| t4.iter().filter(|x| x.symbol == s).cloned().collect());
        assert_eq!((r.vout.as_str(), r.ilim.as_str()), ("5", "30 200m"));
        // without its output in the table, the part number's voltage (MC7812)
        let t5 = [v(2, "ISC", "Vin = 35 Vdc", None, 0.2, None, Some("A"))];
        r.vout.clear();
        r.fill_from_pdf("MC7812BTG", |s| t5.iter().filter(|x| x.symbol == s).cloned().collect());
        assert_eq!(r.ilim, "23 200m");
        // TI's LP5907: a short-circuit row with no voltage, taken at the output + 1 V and said to be assumed
        let t6 = [v(4, "ISC", "TA = 25°C(8)", Some(250.0), 500.0, None, Some("mA"))];
        r.vout.clear();
        r.fill_from_pdf("LP5907MFX-3.3", |s| t6.iter().filter(|x| x.symbol == s).cloned().collect());
        assert_eq!(r.ilim, "4.3 500m");
        assert!(r.found.iter().any(|(ok, l)| !ok && l.contains("短絡")), "{:?}", r.found);
    }

    /// Per voltage one part number per package (the shortest); a voltage whose part numbers name no package is one version,
    /// in the form's package when no part number names one, else with no package (not made: TI's LP5907YKGR-2.825, a DSBGA).
    #[test]
    fn versions_by_voltage_and_package() {
        let s = |x: &str| x.to_string();
        let variants = vec![(1.8, s("LP5907MFX-1.8")), (3.3, s("LP5907MFX-3.3")), (5.0, s("LP5907-5"))];
        let parts = vec![(1.8, s("LP5907MFX-1.8")), (1.8, s("LP5907SNX-1.8")), (3.3, s("LP5907MFX-3.3")), (3.3, s("LP5907MFXQ-3.3"))];
        let packages = vec![(s("LP5907MFX-1.8"), s("SOT-23")), (s("LP5907SNX-1.8"), s("X2SON")), (s("LP5907MFX-3.3"), s("SOT-23")), (s("LP5907MFXQ-3.3"), s("SOT-23"))];
        assert_eq!(
            versions(&variants, &parts, &packages, &[]),
            vec![(1.8, s("LP5907MFX-1.8"), Some(s("SOT-23"))), (1.8, s("LP5907SNX-1.8"), Some(s("X2SON"))), (3.3, s("LP5907MFX-3.3"), Some(s("SOT-23"))), (5.0, s("LP5907-5"), Some(String::new()))]
        );
        // no part number names its package: every voltage in the form's
        assert_eq!(versions(&variants, &parts, &[], &[]).iter().map(|v| v.2.clone()).collect::<Vec<_>>(), vec![None, None, None]);
        // grades printed over the tables' columns (onsemi's MC7800): one version per grade in each package, a part number of
        // no grade its own (MC7805AEBTG)
        let variants = vec![(5.0, s("MC7805BTG"))];
        let parts = vec![(5.0, s("MC7805BTG")), (5.0, s("MC7805CTG")), (5.0, s("MC7805ABTG")), (5.0, s("MC7805CDTG")), (5.0, s("MC7805BDTRKG")), (5.0, s("MC7805BDTG")), (5.0, s("MC7805AEBTG"))];
        let packages: Vec<(String, String)> = parts.iter().map(|p| (p.1.clone(), s(if p.1.contains("DT") { "DPAK" } else { "TO-220" }))).collect();
        let grades = vec![s("MC7805B"), s("NCV7805B"), s("MC7805C"), s("MC7805AB")];
        let names: Vec<String> = versions(&variants, &parts, &packages, &grades).into_iter().map(|v| v.1).collect();
        assert_eq!(names, ["MC7805BTG", "MC7805CTG", "MC7805ABTG", "MC7805CDTG", "MC7805BDTG", "MC7805AEBTG"]);
    }

    /// Every version made at once (no PDF: the LM7805 example's values): a part per voltage in its package's built-in
    /// footprint, each with its own model at its voltage; a package with no built-in outline left out and said so; the form as
    /// it was afterwards.
    #[test]
    fn every_voltage_is_made_at_once() {
        let mut f = super::super::PartForm { name: "MC7800".into(), pins_text: "1 IN\n2 GND\n3 OUT".into(), fp_preset: "TO-220-3".into(), ..Default::default() };
        f.regulator_example_and_make(false);
        let s = |x: &str| x.to_string();
        f.reg.variants = vec![(5.0, s("MC7805CT")), (12.0, s("MC7812CT"))];
        f.reg.parts = vec![(5.0, s("MC7805CT")), (5.0, s("MC7805BDT")), (12.0, s("MC7812CT"))];
        f.reg.packages = vec![(s("MC7805CT"), s("TO-220")), (s("MC7805BDT"), s("DPAK")), (s("MC7812CT"), s("TO-220"))];
        let before = (f.name.clone(), f.reg.vout.clone(), f.sim_text.clone(), f.fp_preset.clone());
        let (series, left) = f.reg_series().unwrap();
        assert_eq!(series.series, "MC78");
        let parts = series.expand();
        let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["MC7805CT", "MC7812CT"], "{left:?}");
        assert!(left.len() == 1 && left[0].contains("MC7805BDT") && left[0].contains("DPAK"), "{left:?}");
        assert_eq!(parts[0].footprint_name.as_deref(), Some("TO-220-3"));
        assert_eq!(parts[1].value.as_deref(), Some("12V"));
        let (a, b) = (parts[0].sim.as_ref().unwrap(), parts[1].sim.as_ref().unwrap());
        assert!(a.text.contains("MC7805CT") && b.text.contains("MC7812CT") && a.text != b.text);
        assert!(parts[1].description.starts_with("MC78 の 出力 12 V、TO-220"), "{}", parts[1].description);
        assert_eq!((f.name.clone(), f.reg.vout.clone(), f.sim_text.clone(), f.fp_preset.clone()), before, "the form put back");
        // the button hands them over for saving
        f.reg_make_all();
        assert_eq!(f.batch.len(), 2);
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
