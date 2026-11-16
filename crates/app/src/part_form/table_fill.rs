//! Filling the optocoupler's, the photodiode's and the Darlington's boxes from the open PDF's electrical
//! characteristics table (`datasheet::spec_table`): the values a table holds (typ, min / max and their conditions).
//! The curves (an LED's forward points, the output curves, a Darlington's hFE) are seldom in a table and are read off
//! the figures; each fill says, line by line, what it took from where and what it did not find.

use super::bjt::lines;
use super::darlington::DarlingtonInput;
use super::graph_reader::short;
use super::opto::{OptoInput, PhotodiodeInput, exact};
use datasheet::spec_table::SpecValue;

fn at(v: &SpecValue) -> String {
    format!("p.{}「{} {}」", v.page + 1, v.symbol, v.conditions.trim())
}

/// The rows of every one of `symbols` (in that order).
fn rows(values: &impl Fn(&str) -> Vec<SpecValue>, symbols: &[&str]) -> Vec<SpecValue> {
    let mut out: Vec<SpecValue> = vec![];
    for s in symbols {
        for v in values(s) {
            // a symbol asked for twice in another case ("CD" / "Cd") gives the same rows
            if !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

/// A value in SI units: by the printed unit's prefix, or (no unit read) by `usual`, and whether it was guessed.
fn si(v: &SpecValue, x: f64, usual: f64) -> (f64, bool) {
    match v.unit.as_deref().and_then(datasheet::spec_table::prefix) {
        Some(k) => (x * k, false),
        None => (x * usual, true),
    }
}

/// " (単位が読めないので mA とした)" when the unit was guessed.
fn guessed(g: bool, unit: &str) -> String {
    if g { format!(" (単位が読めないので {unit} とした)") } else { String::new() }
}

/// A frequency as a plain number of 6 digits ("80000", not "8e+04": the box is read as typed).
fn plain(f: f64) -> String {
    let g = part_lib::fit::num::g(f, 6);
    g.parse::<f64>().map_or(g, |x| format!("{x}"))
}

/// The irradiance of a row's conditions in mW/cm² ("Ee = 1 mW/cm2"), when it gives one.
fn irradiance(conditions: &str) -> Option<f64> {
    let i = conditions.find("mW/cm")?;
    let before = conditions[..i].trim_end();
    let start = before.char_indices().rev().take_while(|(_, c)| c.is_ascii_digit() || *c == '.').last().map(|(j, _)| j)?;
    before[start..].parse().ok()
}

/// A row measured in the dark (no light in its conditions, or a light of 0).
fn dark(v: &SpecValue) -> bool {
    irradiance(&v.conditions).is_none_or(|e| e == 0.0) && !v.conditions.contains("lx")
}

impl OptoInput {
    /// Fills the table's values: VF typ, the LED's capacitance, the CTR range (from a CTR row, else from the
    /// collector current at its IF), VCE(sat) with its IF and IC, and the cut-off frequency.
    pub fn fill_from_pdf(&mut self, values: impl Fn(&str) -> Vec<SpecValue>) {
        let mut found = vec![];
        match rows(&values, &["VF"]).iter().find(|v| v.typ.is_some()) {
            Some(v) => {
                let (vf, g) = si(v, v.typ.unwrap_or_default().abs(), 1.0);
                let i_f = v.condition("IF");
                self.led_typ = format!("{} {}", short(i_f.unwrap_or(0.02)), short(vf));
                let cond = if i_f.is_some() { String::new() } else { " (IF が読めないので 20 mA とした)".into() };
                found.push((true, format!("VF typ {} V{}{cond}: {}。曲線と比べるだけです", short(vf), guessed(g, "V"), at(v))));
            }
            None => found.push((false, "VF の typ が表に見つかりません (比べるだけなので空でも作れます)".into())),
        }
        if let Some(v) = rows(&values, &["Ct", "CT", "Cin", "CIN"]).iter().find(|v| v.typ.is_some()) {
            let (c, g) = si(v, v.typ.unwrap_or_default(), 1e-12);
            self.led_cap = short(c);
            found.push((true, format!("LED の容量 {}F{}: {}", short(c), guessed(g, "pF"), at(v))));
        }
        // the CTR in %: its own row, or the collector current's range at the row's IF
        let ctr = rows(&values, &["CTR"]);
        let from_ic = || {
            rows(&values, &["IC"]).into_iter().find_map(|v| {
                let i_f = v.condition("IF")?;
                let lo = v.min.map(|m| si(&v, m, 1e-3).0 / i_f * 100.0);
                let hi = v.max.map(|m| si(&v, m, 1e-3).0 / i_f * 100.0);
                (lo.is_some() || hi.is_some()).then(|| (lo, hi, format!("{} の IC ÷ IF", at(&v))))
            })
        };
        let range = ctr.iter().find(|v| v.min.is_some() || v.max.is_some()).map(|v| (v.min, v.max, at(v))).or_else(from_ic);
        match range {
            Some((lo, hi, from)) => {
                if let Some(lo) = lo {
                    self.ctr_min = short(lo);
                }
                if let Some(hi) = hi {
                    self.ctr_max = short(hi);
                }
                let show = |x: Option<f64>| x.map_or("—".to_string(), short);
                let missing = if lo.is_none() || hi.is_none() { "。片方しか無いので、もう片方は自分で入れてください" } else { "" };
                found.push((true, format!("CTR {}〜{}%: {from}{missing}", show(lo), show(hi))));
            }
            None => found.push((false, "CTR (電流伝達率) の範囲が表に見つかりません".into())),
        }
        let sat = rows(&values, &["VCE(sat)", "VCE(SAT)", "VCE"]);
        match sat.iter().filter(|v| v.condition("IF").is_some() && v.condition("IC").is_some()).find_map(|v| Some((v.typ.map(|t| (t, true)).or(v.max.map(|m| (m, false)))?, v))) {
            Some(((x, is_typ), v)) => {
                let (x, g) = si(v, x.abs(), 1.0);
                self.vce_sat = short(x);
                self.sat_if = short(v.condition("IF").unwrap_or_default());
                self.sat_ic = short(v.condition("IC").unwrap_or_default());
                let kind = if is_typ { "typ" } else { "max (typ が無いので保証値)" };
                found.push((true, format!("VCE(sat) {} V ({kind}){}: {}", self.vce_sat, guessed(g, "V"), at(v))));
            }
            None => found.push((false, "VCE(sat) (IF と IC の条件つき) が表に見つかりません".into())),
        }
        match rows(&values, &["fc", "fC", "FC", "f-3dB", "BW"]).iter().find(|v| v.typ.is_some()) {
            Some(v) => {
                let (f, g) = si(v, v.typ.unwrap_or_default(), 1e3);
                self.fc = plain(f);
                found.push((true, format!("カットオフ周波数 {}Hz{}: {}", part_lib::passive::eng(f), guessed(g, "kHz"), at(v))));
            }
            None => found.push((false, "カットオフ周波数 (fc) の typ が表に見つかりません。応答時間 tr しか無ければ 0.35 ÷ tr を入れます".into())),
        }
        if self.led.trim().is_empty() || self.output.trim().is_empty() {
            found.push((false, "LED の順方向の点と出力の点は表に無いので、図から「グラフから読む」で入れてください".into()));
        }
        self.found = found;
        self.result = None;
    }
}

impl PhotodiodeInput {
    /// Fills the table's values: the light current per mW/cm², the short-circuit current, the open-circuit voltage and
    /// its temperature coefficient, the dark current at its VR, the breakdown voltage and the capacitance points.
    pub fn fill_from_pdf(&mut self, values: impl Fn(&str) -> Vec<SpecValue>) {
        let mut found = vec![];
        // a current per mW/cm² from the first row that gives the light in mW/cm² (rows in lx are another light)
        let per_light = |symbols: &[&str]| -> (Option<(f64, String)>, bool) {
            let r = rows(&values, symbols);
            let only_lx = !r.is_empty() && r.iter().all(|v| irradiance(&v.conditions).is_none());
            let got = r.iter().find_map(|v| {
                let e = irradiance(&v.conditions).filter(|e| *e > 0.0)?;
                let (x, is_typ) = v.typ.map(|t| (t, true)).or(v.min.map(|m| (m, false)))?;
                let (i, g) = si(v, x.abs(), 1e-6);
                Some((i / e, format!("{}{}{}: {}", if is_typ { "typ" } else { "min (typ が無いので)" }, if e == 1.0 { String::new() } else { format!("、{} mW/cm² で割った", short(e)) }, guessed(g, "µA"), at(v))))
            });
            (got, only_lx)
        };
        match per_light(&["Ira", "IRA", "IL", "Iph", "IPH", "IP"]) {
            (Some((k, how)), _) => {
                self.sensitivity = short(k);
                found.push((true, format!("感度 {}A/(mW/cm²) ({how})", short(k))));
            }
            (None, true) => found.push((false, "光電流が照度 (lx) でしか書かれていません。mW/cm² あたりの値を、図か別の表から入れてください".into())),
            (None, false) => found.push((false, "光電流 (Ira・IL) の mW/cm² での値が表に見つかりません".into())),
        }
        if let (Some((i, how)), _) = per_light(&["Ik", "IK", "Isc", "ISC"]) {
            self.isc = short(i);
            found.push((true, format!("短絡電流 {}A ({how})。比べるだけです", short(i))));
        }
        let vo = rows(&values, &["Vo", "VO", "Voc", "VOC"]);
        match vo.iter().filter(|v| v.typ.is_some()).max_by_key(|v| irradiance(&v.conditions).is_some()) {
            Some(v) => {
                let raw = v.typ.unwrap_or_default().abs();
                // a bare number above a few volts is in mV
                let in_mv = raw > 5.0;
                let (x, g) = si(v, raw, if in_mv { 1e-3 } else { 1.0 });
                self.voc = short(x);
                found.push((true, format!("開放電圧 {} V{}: {}", self.voc, guessed(g, if in_mv { "mV" } else { "V" }), at(v))));
            }
            None => found.push((false, "開放電圧 (Vo・VOC) の typ が表に見つかりません".into())),
        }
        if let Some(v) = rows(&values, &["TKVo", "TKVO", "TCVo", "TCVO", "TKVoc", "TCVoc"]).iter().find(|v| v.typ.is_some()) {
            self.voc_tc = short(v.typ.unwrap_or_default().abs());
            found.push((true, format!("開放電圧の温度係数 −{} mV/K: {}。比べるだけです (マイナスとして扱います)", self.voc_tc, at(v))));
        }
        let dk = rows(&values, &["Iro", "IRO", "Idark", "ID", "Id", "IR"]);
        match dk.iter().filter(|v| dark(v)).find_map(|v| Some((v.typ.map(|t| (t, true)).or(v.max.map(|m| (m, false)))?, v))) {
            Some(((x, is_typ), v)) => {
                let (i, g) = si(v, x.abs(), 1e-9);
                self.dark_i = short(i);
                if let Some(vr) = v.condition("VR") {
                    self.dark_v = short(vr);
                }
                let kind = if is_typ { "typ" } else { "max (typ が無いので保証値。実物はふつうもっと小さい)" };
                found.push((true, format!("暗電流 {}A ({kind}、VR = {} V){}: {}", self.dark_i, self.dark_v, guessed(g, "nA"), at(v))));
            }
            None => found.push((false, "暗電流 (Iro・ID) が表に見つかりません".into())),
        }
        match rows(&values, &["V(BR)", "VBR", "V(BR)R", "BVR", "VR(BR)"]).iter().find_map(|v| Some((v.min.or(v.typ)?, v))) {
            Some((x, v)) => {
                self.bv = short(si(v, x.abs(), 1.0).0);
                if let Some(i) = v.condition("IR") {
                    self.ibv = short(i);
                }
                found.push((true, format!("降伏電圧 {} V (IR = {}A): {}", self.bv, self.ibv, at(v))));
            }
            None => found.push((false, "降伏電圧 V(BR) が表に見つかりません。絶対最大定格の逆電圧 VR を入れても構いません".into())),
        }
        // the capacitance at each reverse voltage the table gives (a row without VR is taken at 0 V)
        let mut cap: Vec<[f64; 2]> = vec![];
        let mut from = None;
        for v in rows(&values, &["CD", "Cd", "Cj", "CJ", "CT", "Ct"]).iter().filter(|v| v.typ.is_some()) {
            let vr = v.condition("VR").unwrap_or(0.0);
            if !cap.iter().any(|p| (p[0] - vr).abs() < 1e-9) {
                cap.push([vr, si(v, v.typ.unwrap_or_default(), 1e-12).0]);
                from.get_or_insert(v.page);
            }
        }
        cap.sort_by(|a, b| a[0].total_cmp(&b[0]));
        match from {
            Some(page) => {
                self.cap = exact(&cap);
                let pts: Vec<String> = cap.iter().map(|p| format!("{} V で {}F", short(p[0]), short(p[1]))).collect();
                found.push((true, format!("容量 {}: p.{}。図があれば点を足すと、逆電圧による変化が入ります", pts.join("、"), page + 1)));
            }
            None => found.push((false, "容量 (CD・Cj) の typ が表に見つかりません".into())),
        }
        self.found = found;
        self.result = None;
    }
}

/// (IC, value) points of the rows with a typ and an IC, the value by the unit (`usual` when none is read; `None`: the
/// value has no unit, as hFE), with the rows' IC/IB.
fn ic_points(rows: &[SpecValue], usual: Option<f64>) -> Vec<([f64; 2], Option<f64>)> {
    let mut out: Vec<([f64; 2], Option<f64>)> = vec![];
    for v in rows {
        let (Some(t), Some(ic)) = (v.typ, v.condition("IC")) else { continue };
        let y = usual.map_or(t.abs(), |u| si(v, t.abs(), u).0);
        if !out.iter().any(|p| (p.0[0] / ic - 1.0).abs() < 0.01) {
            out.push(([ic, y], v.condition("IB").map(|ib| ic / ib)));
        }
    }
    out.sort_by(|a, b| a.0[0].total_cmp(&b.0[0]));
    out
}

impl DarlingtonInput {
    /// Fills the table's values: hFE, VBE(on), VBE(sat) and VCE(sat) where the table gives typ values with their IC
    /// (most give only guarantees: then the figures are needed), fT (or |hfe| at its frequency) and Cob / Cib.
    pub fn fill_from_pdf(&mut self, values: impl Fn(&str) -> Vec<SpecValue>) {
        let mut found = vec![];
        // one curve: its typ points, or a line saying why there are none
        let mut curve = |symbols: &[&str], label: &str, usual: Option<f64>, sat: bool, field: &mut String, vce: Option<&mut String>, ratio: Option<&mut String>| {
            let r = rows(&values, symbols);
            let mut pts = ic_points(&r, usual);
            if sat {
                // the saturation rows of one IC/IB (the figure's condition)
                let first = pts.iter().find_map(|p| p.1);
                pts.retain(|p| match (p.1, first) {
                    (Some(a), Some(b)) => (a / b - 1.0).abs() < 0.1,
                    _ => first.is_none(),
                });
                if let (Some(k), Some(ratio)) = (first, ratio) {
                    *ratio = short(k);
                }
            }
            if pts.is_empty() {
                found.push((false, if r.is_empty() {
                    format!("{label} が表に見つかりません。図から読んでください")
                } else {
                    format!("{label} は表に保証値 (min・max) しか無いか、IC の条件が読めません。図から読んでください")
                }));
                return;
            }
            let p: Vec<[f64; 2]> = pts.iter().map(|p| p.0).collect();
            *field = lines(&p);
            if let Some(vce) = vce
                && let Some(x) = r.iter().find_map(|v| v.condition("VCE"))
            {
                *vce = short(x);
            }
            let few = if p.len() < 3 { "。点が少ないので図からも読んでください" } else { "" };
            found.push((true, format!("{label} {} 点: {}{few}", p.len(), at(&r[0]))));
        };
        curve(&["hFE", "HFE"], "hFE", None, false, &mut self.hfe, Some(&mut self.hfe_vce), None);
        curve(&["VBE(on)", "VBE(ON)", "VBE"], "VBE(on)", Some(1.0), false, &mut self.vbe_on, Some(&mut self.on_vce), None);
        curve(&["VBE(sat)", "VBE(SAT)"], "VBE(sat)", Some(1.0), true, &mut self.vbe_sat, None, Some(&mut self.sat_ratio));
        curve(&["VCE(sat)", "VCE(SAT)"], "VCE(sat)", Some(1.0), true, &mut self.vce_sat, None, Some(&mut self.sat_ratio));
        // fT, or the small-signal gain |hfe| at a frequency (the gain falls as 1/f, so fT = |hfe| × f)
        let ft = rows(&values, &["fT", "FT"]).into_iter().find_map(|v| {
            let (x, is_typ) = v.typ.map(|t| (t, true)).or(v.min.map(|m| (m, false)))?;
            Some((si(&v, x.abs(), 1e6).0, format!("fT の {}: {}", if is_typ { "typ" } else { "min" }, at(&v))))
        });
        let ft = ft.or_else(|| {
            rows(&values, &["hfe", "|hfe|"]).into_iter().find_map(|v| {
                let f = v.condition("f")?;
                let (x, is_typ) = v.typ.map(|t| (t, true)).or(v.min.map(|m| (m, false)))?;
                Some((x.abs() * f, format!("|hfe| の {} {} × {}Hz: {}", if is_typ { "typ" } else { "min" }, short(x.abs()), part_lib::passive::eng(f), at(&v))))
            })
        });
        match ft {
            Some((f, how)) => {
                self.ft = plain(f);
                found.push((true, format!("fT {}Hz ({how})", part_lib::passive::eng(f))));
            }
            None => found.push((false, "fT が表に見つかりません (空でも作れます)".into())),
        }
        for (syms, cond, label, field) in [(&["Cob", "Cobo"][..], "VCB", "Cob", &mut self.cob), (&["Cib", "Cibo"][..], "VEB", "Cib", &mut self.cib)] {
            if let Some(v) = rows(&values, syms).iter().find(|v| v.typ.is_some()) {
                let c = si(v, v.typ.unwrap_or_default(), 1e-12).0;
                let vr = v.condition(cond).unwrap_or(0.0);
                *field = lines(&[[vr, c]]);
                found.push((true, format!("{label} {}F ({} V): {}", short(c), short(vr), at(v))));
            }
        }
        if self.r1.trim().is_empty() || self.r2.trim().is_empty() {
            found.push((false, "内蔵の抵抗 R1・R2 は表に無いので、データシートの回路の図から入れてください".into()));
        }
        self.found = found;
        self.result = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(symbol: &str, conditions: &str, min: Option<f64>, typ: Option<f64>, max: Option<f64>, unit: Option<&str>) -> SpecValue {
        SpecValue { page: 3, symbol: symbol.into(), conditions: conditions.into(), min, typ, max, unit: unit.map(Into::into), rect: datasheet::Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 } }
    }

    fn lookup(table: &[SpecValue]) -> impl Fn(&str) -> Vec<SpecValue> + '_ {
        |s| table.iter().filter(|x| x.symbol == s).cloned().collect()
    }

    #[test]
    fn the_light_currents_irradiance_is_read() {
        assert_eq!(irradiance("Ee = 1 mW/cm2, λ = 950 nm"), Some(1.0));
        assert_eq!(irradiance("Ee=0.5mW/cm²"), Some(0.5));
        assert_eq!(irradiance("EA = 1 klx"), None);
    }

    /// The rows as PC817's table reads (Sharp: no CTR row, the collector current at IF = 5 mA instead).
    #[test]
    fn an_optocoupler_table_gives_ctr_from_the_collector_current() {
        let table = [
            v("VF", "IF=20mA", None, Some(1.2), Some(1.4), Some("V")),
            v("Ct", "V=0, f=1kHz", None, Some(30.0), Some(250.0), Some("pF")),
            v("IC", "IF=5mA, VCE=5V", Some(2.5), None, Some(30.0), Some("mA")),
            v("VCE", "IF=20mA, IC=1mA", None, Some(0.1), Some(0.2), Some("V")),
            v("fc", "IC=2mA, RL=100Ω, −3dB", None, Some(80.0), None, Some("kHz")),
        ];
        let mut o = OptoInput::default();
        o.fill_from_pdf(lookup(&table));
        assert_eq!((o.led_typ.as_str(), o.led_cap.as_str()), ("0.02 1.2", "30p"));
        assert_eq!((o.ctr_min.as_str(), o.ctr_max.as_str()), ("50", "600"));
        assert_eq!((o.vce_sat.as_str(), o.sat_if.as_str(), o.sat_ic.as_str()), ("0.1", "0.02", "0.001"));
        assert_eq!(o.fc, "80000");
        // the curves are not in a table: said, and nothing else is missing
        let missing: Vec<&String> = o.found.iter().filter(|f| !f.0).map(|f| &f.1).collect();
        assert!(missing.len() == 1 && missing[0].contains("出力の点"), "{missing:?}");
    }

    /// The rows as BPW34's table reads (Vishay): light currents in lx and in mW/cm², the TKVo without its sign.
    #[test]
    fn a_photodiode_table_gives_the_light_current_per_mw() {
        let table = [
            v("V(BR)", "IR = 100 μA, E = 0", Some(60.0), None, None, Some("V")),
            v("Iro", "VR = 10 V, E = 0", None, Some(2.0), Some(30.0), Some("nA")),
            v("Ira", "EA = 1 klx, VR = 5 V", None, Some(75.0), None, Some("μA")),
            v("Ira", "Ee = 1 mW/cm2, λ = 950", Some(40.0), Some(50.0), None, Some("μA")),
            v("Ik", "Ee = 1 mW/cm2, λ = 950 nm", None, Some(47.0), None, Some("μA")),
            v("Vo", "Ee = 1 mW/cm2, λ = 950 nm", None, Some(350.0), None, Some("mV")),
            v("TKVo", "Ee = 1 mW/cm2, λ = 950 nm", None, Some(2.6), None, None),
            v("CD", "VR = 0 V, f = 1 MHz, E = 0", None, Some(70.0), None, Some("pF")),
            v("CD", "VR = 3 V, f = 1 MHz, E = 0", None, Some(25.0), Some(40.0), Some("pF")),
            v("CD", "VR = 0 V, f = 1 MHz, E = 0", None, Some(70.0), None, Some("pF")),
        ];
        let mut p = PhotodiodeInput::default();
        p.fill_from_pdf(lookup(&table));
        assert_eq!((p.sensitivity.as_str(), p.isc.as_str(), p.voc.as_str(), p.voc_tc.as_str()), ("50u", "47u", "0.35", "2.6"));
        assert_eq!((p.dark_i.as_str(), p.dark_v.as_str(), p.bv.as_str(), p.ibv.as_str()), ("2n", "10", "60", "100u"));
        assert_eq!(p.cap, "0 70p\n3 25p");
        assert!(p.found.iter().all(|f| f.0), "{:?}", p.found);
        // what was read makes a model (the light points and N are optional)
        assert!(p.spec("PD", "s").is_ok());
    }

    /// TIP120's table (onsemi) gives guarantees only: the curves are left to the figures, fT comes from |hfe| at 1 MHz.
    #[test]
    fn a_darlington_table_of_guarantees_leaves_the_curves_to_the_figures() {
        let table = [
            v("hFE", "", Some(1000.0), None, None, None),
            v("VCE(sat)", "", None, None, Some(2.0), None),
            v("hfe", "f = 1.0 MHz)", Some(4.0), None, None, None),
        ];
        let mut d = DarlingtonInput::default();
        d.fill_from_pdf(lookup(&table));
        assert_eq!(d.ft, "4000000");
        assert!(d.hfe.is_empty());
        let says = |w: &str| d.found.iter().any(|f| !f.0 && f.1.contains(w));
        assert!(says("保証値") && says("R1・R2"), "{:?}", d.found);
        // typ rows with their currents go in, the saturation condition with them
        let table = [
            v("hFE", "IC = 0.5 A, VCE = 3 V", None, Some(2000.0), None, None),
            v("hFE", "IC = 3 A, VCE = 3 V", None, Some(4000.0), None, None),
            v("VCE(sat)", "IC = 3 A, IB = 12 mA", None, Some(1.1), Some(2.0), Some("V")),
            v("VCE(sat)", "IC = 5 A, IB = 50 mA", None, Some(1.5), Some(4.0), Some("V")),
        ];
        let mut d = DarlingtonInput::default();
        d.fill_from_pdf(lookup(&table));
        assert_eq!((d.hfe.as_str(), d.hfe_vce.as_str()), ("0.5 2000\n3 4000", "3"));
        assert_eq!((d.vce_sat.as_str(), d.sat_ratio.as_str()), ("3 1.1", "250"), "IC/IB = 100 is another condition");
    }
}
