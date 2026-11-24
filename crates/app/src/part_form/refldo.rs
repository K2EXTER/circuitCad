//! "かんたん作成" for a voltage reference (REF50xx: NR・TRIM and TEMP pins) and for an LDO whose output a SET pin's current
//! sets (LT3045: RSET・CSET, ILIM, EN/UV, PG): the data sheet's typ values go into the same templates the bundled REF5025 and
//! LT3045 are made with (`part_lib::fit::reference`, `setldo`), the PSRR points fitted, and the model goes to the simulation
//! tab with its notes. The example buttons fill the boxes from the bundled parts' readings (`crates/part-lib/fits/`).

use super::*;
use part_lib::fit::{self, Fitted};
use part_lib::fit::regulator::{LimitAt, Noise};

/// A number as typed (SI prefixes: 2.5, 800u, 54, 10k).
fn value(label: &str, t: &str) -> Result<f64, String> {
    sim_core::parse_value(t.trim()).filter(|v| v.is_finite()).ok_or_else(|| {
        if t.trim().is_empty() { format!("{label}が空です。データシートの typ を入れてください") } else { format!("{label}「{}」が数として読めません (例: 2.5、800u、10k)", t.trim()) }
    })
}

fn opt(label: &str, t: &str) -> Result<Option<f64>, String> {
    if t.trim().is_empty() { Ok(None) } else { value(label, t).map(Some) }
}

/// The numbers of a box ("0.02 54", "1.2 10k 1k 0.5").
fn numbers(label: &str, t: &str) -> Result<Vec<f64>, String> {
    t.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(|s| value(label, s)).collect()
}

/// Exactly `n` numbers, or (`optional`) none at all.
fn fixed<const N: usize>(label: &str, t: &str, how: &str, optional: bool) -> Result<Option<[f64; N]>, String> {
    let v = numbers(label, t)?;
    if v.is_empty() && optional {
        return Ok(None);
    }
    v.try_into().map(Some).map_err(|_| format!("{label}は {how} で入れてください"))
}

fn show(v: f64) -> String {
    super::linear::show(v)
}

fn join(v: &[f64]) -> String {
    v.iter().map(|x| show(*x)).collect::<Vec<_>>().join(" ")
}

/// The PSRR box: "Hz dB" a line, 1 line or more (one: the same at every frequency; more: the shape fitted to them).
fn psrr_of(text: &str) -> Result<Vec<[f64; 2]>, String> {
    let how = "PSRR は 1 行に「周波数 [Hz] 除去 [dB]」で、1 行以上入れてください (例: 120 117 と 10k 90)。2 行以上なら、その点に周波数特性を合わせます";
    let mut out = vec![];
    for r in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let n: Vec<f64> = r.split_whitespace().map(|x| sim_core::parse_value(x).ok_or(())).collect::<Result<_, _>>().map_err(|_| how.to_string())?;
        match n.as_slice() {
            [f, db] if *f > 0.0 && *db > 0.0 => out.push([*f, *db]),
            _ => return Err(how.into()),
        }
    }
    if out.is_empty() {
        return Err(how.into());
    }
    Ok(out)
}

fn psrr_text(p: &[[f64; 2]]) -> String {
    p.iter().map(|q| format!("{} {}", show(q[0]), fit::num::g(q[1], 6))).collect::<Vec<_>>().join("\n")
}

/// A noise box: "rms f_low f_high" (V, Hz, Hz) or one density (V/√Hz; A for a current).
fn noise_of(label: &str, t: &str) -> Result<Noise, String> {
    match numbers(label, t)?.as_slice() {
        [d] => Ok(Noise::Density { density: *d }),
        [rms, fl, fh] if fh > fl => Ok(Noise::Rms { rms: *rms, f_low: *fl, f_high: *fh }),
        _ => Err(format!("{label}は「0.8u 10 100k」(rms と 帯域の下・上 [Hz]) か、密度 1 つ (20n = 20 nV/√Hz) で入れてください")),
    }
}

fn noise_text(n: &Noise) -> String {
    match *n {
        Noise::Rms { rms, f_low, f_high } => join(&[rms, f_low, f_high]),
        Noise::Density { density } => show(density),
    }
}

/// The temperature box: "°C percent" lines, as the regulator's.
fn temp_text(c: &fit::TempCurve) -> String {
    c.points.iter().map(|p| format!("{} {}", fit::num::g(p[0], 6), fit::num::g(p[1] * 100.0, 6))).collect::<Vec<_>>().join("\n")
}

/// A temperature box as a curve, with where it was read when the box is as it was filled.
fn temp_of(text: &str, from: &str) -> Result<Option<fit::TempCurve>, String> {
    let mut c = super::linear::temp_curve(text)?;
    if let Some(c) = c.as_mut()
        && !from.is_empty()
    {
        c.from = from.to_string();
    }
    Ok(c)
}

fn common(part: &str, source: &str) -> Result<fit::Common, String> {
    serde_json::from_value(serde_json::json!({"part": part, "name": part_lib::diode::model_name(part), "source": source})).map_err(|e| e.to_string())
}

/// The boxes of the voltage reference section.
#[derive(Clone, Default)]
pub struct ReferenceInput {
    pub vout: String,
    /// "min max" (optional).
    pub vrange: String,
    pub vin0: String,
    /// ppm/V.
    pub line: String,
    /// ppm/mA: "sourcing sinking", or one for both.
    pub load: String,
    pub iq: String,
    pub iq_max: String,
    /// "V Ω": the dropout at no load and its growth with the current.
    pub dropout: String,
    /// "sourcing sinking" (A).
    pub isc: String,
    pub psrr: String,
    pub noise: String,
    /// "vbg r_bg r_pin noise_left" (the NR・TRIM pin), empty: none.
    pub nr: String,
    /// "V at 25 °C, V/°C, Ω" (the TEMP pin), empty: none.
    pub temp_pin: String,
    pub temp: String,
    /// What the example's values were read off (figures, assumptions), kept while its boxes are as the example put them.
    pub from_figures: String,
    pub temp_from: String,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl ReferenceInput {
    pub fn from_spec(s: &fit::ReferenceSpec) -> ReferenceInput {
        ReferenceInput {
            vout: show(s.vout),
            vrange: s.vrange.map(|r| join(&r)).unwrap_or_default(),
            vin0: s.vin0.map(show).unwrap_or_default(),
            line: fit::num::g(s.line_ppm_v, 6),
            load: if s.load_src_ppm_ma == s.load_snk_ppm_ma { fit::num::g(s.load_src_ppm_ma, 6) } else { format!("{} {}", fit::num::g(s.load_src_ppm_ma, 6), fit::num::g(s.load_snk_ppm_ma, 6)) },
            iq: show(s.iq),
            iq_max: s.iq_max.map(show).unwrap_or_default(),
            dropout: join(&[s.vdo0, s.rdo]),
            isc: join(&[s.isc_src, s.isc_snk]),
            psrr: psrr_text(&s.psrr),
            noise: noise_text(&s.noise),
            nr: s.nr.as_ref().map(|n| join(&[n.vbg, n.r_bg, n.r_pin, n.noise_left])).unwrap_or_default(),
            temp_pin: s.temp_pin.as_ref().map(|t| join(&[t.v25, t.slope, t.r_out])).unwrap_or_default(),
            temp: s.temp.as_ref().map(temp_text).unwrap_or_default(),
            from_figures: s.from_figures.clone(),
            temp_from: s.temp.as_ref().map(|c| c.from.clone()).unwrap_or_default(),
            ..Default::default()
        }
    }

    pub fn spec(&self, part: &str, source: &str) -> Result<fit::ReferenceSpec, String> {
        let vrange = fixed::<2>("出力電圧の min・max", &self.vrange, "「2.49875 2.50125」の 2 つの数", true)?;
        let load = match numbers("ロード・レギュレーション", &self.load)?.as_slice() {
            [b] => [*b, *b],
            [a, b] => [*a, *b],
            _ => return Err("ロード・レギュレーションは ppm/mA で、吐き出しと吸い込みの 2 つ (20 20) か、両方同じなら 1 つで入れてください".into()),
        };
        let [vdo0, rdo] = fixed::<2>("ドロップアウト", &self.dropout, "「0.02 54」(無負荷の電圧 [V] と 電流による増え [Ω])", false)?.unwrap_or_default();
        let [isc_src, isc_snk] = fixed::<2>("短絡電流", &self.isc, "「25m 20m」(吐き出しと吸い込み [A])", false)?.unwrap_or_default();
        let nr = fixed::<4>("NR・TRIM ピン", &self.nr, "「1.2 10k 1k 0.5」(バンドギャップの電圧・そこから増幅器までの抵抗・ピンまでの抵抗・大きなコンデンサで残る雑音の割合) か空", true)?
            .map(|[vbg, r_bg, r_pin, noise_left]| fit::reference::NrPin { vbg, r_bg, r_pin, noise_left });
        let temp_pin = fixed::<3>("TEMP ピン", &self.temp_pin, "「0.575 2.64m 60k」(25 ℃ の電圧・1 ℃ あたりの変化・出力抵抗) か空", true)?
            .map(|[v25, slope, r_out]| fit::reference::TempPin { v25, slope, r_out });
        let mut temp = super::linear::temp_curve(&self.temp)?;
        if let Some(c) = temp.as_mut()
            && !self.temp_from.is_empty()
        {
            c.from = self.temp_from.clone();
        }
        let s = fit::ReferenceSpec {
            common: common(part, source)?,
            cite: source.into(),
            from_figures: self.from_figures.clone(),
            vout: value("出力電圧", &self.vout)?,
            vrange,
            vin0: opt("表の測定の入力電圧", &self.vin0)?,
            line_ppm_v: value("ライン・レギュレーション", self.line.trim().trim_end_matches("ppm/V"))?,
            load_src_ppm_ma: load[0],
            load_snk_ppm_ma: load[1],
            iq: value("電源電流", &self.iq)?,
            iq_max: opt("電源電流の max", &self.iq_max)?,
            vdo0,
            rdo,
            isc_src,
            isc_snk,
            psrr: psrr_of(&self.psrr)?,
            noise: noise_of("出力雑音", &self.noise)?,
            nr,
            temp_pin,
            temp,
        };
        fit::reference::check_reference(&s)?;
        Ok(s)
    }
}

/// The boxes of the SET-pin LDO section.
#[derive(Clone, Default)]
pub struct SetLdoInput {
    pub iset: String,
    /// "min max" of the SET current and of the offset (optional).
    pub iset_range: String,
    pub vos_range: String,
    pub vin0: String,
    /// "A/V V/V": the SET current's and the offset's change with the input.
    pub line: String,
    /// Ω, or "ΔVOS ΔI".
    pub load: String,
    /// "V Ω".
    pub dropout: String,
    pub vin_min: String,
    /// "on, A per A of output, off" (A).
    pub iq: String,
    /// "difference current" lines (1 or 2).
    pub ilim: String,
    pub en: String,
    /// "ratio threshold", empty: no ILIM pin.
    pub ilim_pin: String,
    /// "threshold r_on fast", empty: no PG.
    pub pg: String,
    pub psrr: String,
    pub noise: String,
    pub iset_noise: String,
    /// The SET current's and the quiescent current's change with the temperature ("°C percent" lines), and where they were
    /// read (kept while the box is unchanged).
    pub iset_temp: String,
    pub iset_temp_from: String,
    pub iq_temp: String,
    pub iq_temp_from: String,
    pub from_figures: String,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl SetLdoInput {
    pub fn from_spec(s: &fit::SetLdoSpec) -> SetLdoInput {
        SetLdoInput {
            iset: show(s.iset),
            iset_range: s.iset_range.map(|r| join(&r)).unwrap_or_default(),
            vos_range: s.vos_range.map(|r| join(&r)).unwrap_or_default(),
            vin0: show(s.vin0),
            line: join(&[s.line_iset, s.line_vos]),
            load: show(s.rout),
            dropout: join(&[s.vdo0, s.rdo]),
            vin_min: show(s.vin_min),
            iq: join(&[s.iq, s.iq_per_a, s.iq_off]),
            ilim: s.ilim.iter().map(|l| join(&[l.diff, l.amps])).collect::<Vec<_>>().join("\n"),
            en: s.en_threshold.map(show).unwrap_or_default(),
            ilim_pin: s.ilim_pin.as_ref().map(|p| join(&[p.ratio, p.threshold])).unwrap_or_default(),
            pg: s.pg.as_ref().map(|p| join(&[p.threshold, p.r_on, p.fast])).unwrap_or_default(),
            psrr: psrr_text(&s.psrr),
            noise: noise_text(&s.noise),
            iset_noise: noise_text(&s.iset_noise),
            iset_temp: s.iset_temp.as_ref().map(temp_text).unwrap_or_default(),
            iset_temp_from: s.iset_temp.as_ref().map(|c| c.from.clone()).unwrap_or_default(),
            iq_temp: s.iq_temp.as_ref().map(temp_text).unwrap_or_default(),
            iq_temp_from: s.iq_temp.as_ref().map(|c| c.from.clone()).unwrap_or_default(),
            from_figures: s.from_figures.clone(),
            ..Default::default()
        }
    }

    pub fn spec(&self, part: &str, source: &str) -> Result<fit::SetLdoSpec, String> {
        let line = numbers("ライン・レギュレーション", &self.line)?;
        let (line_iset, line_vos) = match line.as_slice() {
            [] => (0.0, 0.0),
            [a, b] => (*a, *b),
            _ => return Err("ライン・レギュレーションは「0.5n 0.5u」(SET の電流の変化 [A/V] と オフセットの変化 [V/V]) で入れてください (空でも作れます)".into()),
        };
        let rout = match numbers("ロード・レギュレーション", &self.load)?.as_slice() {
            [r] => *r,
            [dv, di] if *di != 0.0 => (dv / di).abs(),
            _ => return Err("ロード・レギュレーションは出力抵抗 [Ω] 1 つか、「0.1m 0.499」(オフセットの変化 [V] と 電流の変化 [A]) で入れてください".into()),
        };
        let [vdo0, rdo] = fixed::<2>("ドロップアウト", &self.dropout, "「0.21 0.1」(小さい電流の電圧 [V] と 電流による増え [Ω])", false)?.unwrap_or_default();
        let (iq, iq_per_a, iq_off) = match numbers("電源電流", &self.iq)?.as_slice() {
            [a] => (*a, 0.0, 0.0),
            [a, b] => (*a, *b, 0.0),
            [a, b, c] => (*a, *b, *c),
            _ => return Err("電源電流は「2.2m 0.02 0.3u」(動作中 [A]・出力電流 1 A あたりの増え・止めたとき [A]) で、1〜3 つ入れてください".into()),
        };
        let ilim = match numbers("電流制限", &self.ilim.replace('\n', " "))?.as_slice() {
            [d, i] => [LimitAt { diff: *d, amps: *i }, LimitAt { diff: d + 1.0, amps: *i }],
            [d1, i1, d2, i2] => [LimitAt { diff: *d1, amps: *i1 }, LimitAt { diff: *d2, amps: *i2 }],
            _ => return Err("電流制限は 1 行に「入出力の差 [V] 電流 [A]」、1 行か 2 行で入れてください (例: 12 0.7 と 20 0.33)".into()),
        };
        let ilim_pin = fixed::<2>("ILIM ピン", &self.ilim_pin, "「500 0.3」(出力電流とピンの電流の比・制限が効くピンの電圧 [V]) か空", true)?
            .map(|[ratio, threshold]| fit::setldo::IlimPin { ratio, threshold });
        let pg = match numbers("PG", &self.pg)?.as_slice() {
            [] => None,
            [t, r] => Some(fit::setldo::PowerGood { threshold: *t, r_on: *r, fast: 0.0 }),
            [t, r, f] => Some(fit::setldo::PowerGood { threshold: *t, r_on: *r, fast: *f }),
            _ => return Err("PG は「0.3 300 1.9m」(PGFB のしきい値 [V]・PG が Low のときの抵抗 [Ω]・速い起動で SET に足す電流 [A]、無ければ 2 つ) か空で入れてください".into()),
        };
        let s = fit::SetLdoSpec {
            common: common(part, source)?,
            cite: source.into(),
            from_figures: self.from_figures.clone(),
            iset: value("SET ピンの電流", &self.iset)?,
            iset_range: fixed::<2>("SET ピンの電流の min・max", &self.iset_range, "「99u 101u」の 2 つの数", true)?,
            vos_range: fixed::<2>("オフセットの min・max", &self.vos_range, "「-1m 1m」の 2 つの数", true)?,
            vin0: value("表の測定の入力電圧", &self.vin0)?,
            line_iset,
            line_vos,
            rout,
            vdo0,
            rdo,
            vin_min: value("最小の入力電圧", &self.vin_min)?,
            iq,
            iq_per_a,
            iq_off,
            ilim,
            en_threshold: opt("EN のしきい値", &self.en)?,
            ilim_pin,
            pg,
            psrr: psrr_of(&self.psrr)?,
            noise: noise_of("出力雑音", &self.noise)?,
            iset_noise: noise_of("SET の電流の雑音", &self.iset_noise)?,
            iset_temp: temp_of(&self.iset_temp, &self.iset_temp_from)?,
            iq_temp: temp_of(&self.iq_temp, &self.iq_temp_from)?,
        };
        fit::setldo::check_setldo(&s)?;
        Ok(s)
    }
}

/// The bundled parts' readings: the examples.
fn ref5025() -> fit::ReferenceSpec {
    match serde_json::from_str::<fit::Spec>(include_str!("../../../part-lib/fits/REF5025_SOIC-8.fit.json")) {
        Ok(fit::Spec::Reference(s)) => s,
        _ => unreachable!("the bundled REF5025 readings are a reference"),
    }
}

fn lt3045() -> fit::SetLdoSpec {
    match serde_json::from_str::<fit::Spec>(include_str!("../../../part-lib/fits/LT3045_MSOP-12.fit.json")) {
        Ok(fit::Spec::SetLdo(s)) => s,
        _ => unreachable!("the bundled LT3045 readings are a SET-pin LDO"),
    }
}

/// The pins by role, found by name; the optional pins only when the model has them.
fn reference_roles(s: &ReferenceInput) -> Vec<(&'static str, &'static [&'static str])> {
    let mut r: Vec<(&str, &[&str])> = vec![("入力", &["VIN", "IN", "INPUT"]), ("GND", &["GND", "COM"]), ("出力", &["VOUT", "OUT", "OUTPUT"])];
    if !s.nr.trim().is_empty() {
        r.push(("NR・TRIM", &["TRIM/NR", "NR", "TRIM", "NR/TRIM"]));
    }
    if !s.temp_pin.trim().is_empty() {
        r.push(("TEMP", &["TEMP", "VTEMP"]));
    }
    r
}

fn setldo_roles(s: &SetLdoInput) -> Vec<(&'static str, &'static [&'static str])> {
    let mut r: Vec<(&str, &[&str])> = vec![
        ("入力", &["IN", "VIN", "INPUT"]),
        ("GND", &["GND"]),
        ("出力", &["OUT", "VOUT", "OUTPUT"]),
        ("出力の検出", &["OUTS", "SENSE", "OUT_S"]),
        ("SET", &["SET"]),
    ];
    if !s.en.trim().is_empty() {
        r.push(("EN", &["EN/UV", "EN", "SHDN", "EN/UVLO"]));
    }
    if !s.pg.trim().is_empty() {
        r.push(("PG", &["PG", "PGOOD"]));
        r.push(("PGFB", &["PGFB"]));
    }
    if !s.ilim_pin.trim().is_empty() {
        r.push(("ILIM", &["ILIM"]));
    }
    r
}

#[derive(Clone, Copy, PartialEq)]
pub enum RefLdo {
    Reference,
    SetLdo,
}

impl PartForm {
    fn refldo_make(&mut self, k: RefLdo) {
        let source = self.linear_source();
        match k {
            RefLdo::Reference => {
                let name = self.linear_name("VREF");
                self.vref.result = Some(self.vref.spec(&name, &source).and_then(|s| fit::fit(&fit::Spec::Reference(s))));
                self.vref.message = None;
            }
            RefLdo::SetLdo => {
                let name = self.linear_name("LDO");
                self.setldo.result = Some(self.setldo.spec(&name, &source).and_then(|s| fit::fit(&fit::Spec::SetLdo(s))));
                self.setldo.message = None;
            }
        }
    }

    /// The example (REF5025 or LT3045) in the boxes, made, and put into ② (`--form-reference-example` / `--form-setldo-example`).
    pub fn refldo_example_and_make(&mut self, k: RefLdo) {
        match k {
            RefLdo::Reference => self.vref = ReferenceInput::from_spec(&ref5025()),
            RefLdo::SetLdo => self.setldo = SetLdoInput::from_spec(&lt3045()),
        }
        self.refldo_make(k);
    }

    /// The pins of the example's package, named for the roles (REF5025 SOIC-8, LT3045 MSOP-12 with its tied pins as one).
    pub fn refldo_pins(&mut self, k: RefLdo) {
        self.pins_text = match k {
            RefLdo::Reference => "2 VIN left\n4 GND bottom\n6 VOUT right\n5 TRIM/NR left\n3 TEMP right",
            RefLdo::SetLdo => "1 IN left\n4 EN/UV left\n6 ILIM left\n8 SET left\n9 GND bottom\n11 OUT right\n10 OUTS right\n5 PG right\n7 PGFB right",
        }
        .into();
        self.prefix = "U".into();
    }

    pub fn refldo_apply(&mut self, k: RefLdo) -> Result<String, String> {
        let (result, roles) = match k {
            RefLdo::Reference => (self.vref.result.clone(), reference_roles(&self.vref)),
            RefLdo::SetLdo => (self.setldo.result.clone(), setldo_roles(&self.setldo)),
        };
        let Some(Ok(f)) = result else { return Err("モデルができていません".into()) };
        self.linear_apply(&f, &roles)
    }

    fn refldo_buttons(&mut self, ui: &mut egui::Ui, k: RefLdo) {
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作る")).on_hover_text(i18n::tr("入れた値をひな形に入れてモデルを作り、PSRR の点とモデルの値を並べます")).clicked() {
                self.refldo_make(k);
            }
            let made = matches!(if k == RefLdo::Reference { &self.vref.result } else { &self.setldo.result }, Some(Ok(_)));
            if made && ui.button(i18n::tr("「② シミュレーション」に入れる")).on_hover_text(i18n::tr("作ったモデルと注記を、シミュレーションのタブに入れます。ピンは名前で探します (使わないピンの欄が空なら、そのピンは要りません)")).clicked() {
                let msg = Some(match self.refldo_apply(k) {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
                if k == RefLdo::Reference { self.vref.message = msg } else { self.setldo.message = msg }
            }
        });
        let (result, message) = if k == RefLdo::Reference { (&self.vref.result, &self.vref.message) } else { (&self.setldo.result, &self.setldo.message) };
        if let Some(Ok(f)) = result
            && !f.rows.is_empty()
        {
            super::mosfet::fit_table(ui, if k == RefLdo::Reference { "vref_rows" } else { "setldo_rows" }, f);
        }
        Self::result_box(ui, result, message);
    }

    /// A row of the grid: the label with its tip, then the boxes. Returns whether a box was changed.
    fn refldo_row(ui: &mut egui::Ui, label: &str, tip: &str, fields: Vec<(&mut String, &str, f32)>) -> bool {
        ui.label(i18n::tr(label)).on_hover_text(i18n::tr(tip));
        let mut changed = false;
        ui.horizontal(|ui| {
            for (f, hint, w) in fields {
                changed |= ui.add(egui::TextEdit::singleline(f).desired_width(w).hint_text(hint)).changed();
            }
        });
        ui.end_row();
        changed
    }

    fn refldo_lines(ui: &mut egui::Ui, label: &str, tip: &str, text: &mut String, hint: &str) -> bool {
        ui.label(i18n::tr(label)).on_hover_text(i18n::tr(tip));
        let changed = ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(160.0).font(egui::TextStyle::Monospace).hint_text(hint)).changed();
        ui.end_row();
        changed
    }

    pub(super) fn reference_ui(&mut self, ui: &mut egui::Ui) {
        crate::icons::help_small(ui, "表の typ の値からモデルを作ります", "データシートの電気的特性の表の typ の値を、同梱の REF5025 と同じひな形に入れてモデルを作ります。中は、内部のバンドギャップ (1.2 V など) を増幅して出す形 (REF50xx のブロック図) で、NR・TRIM ピンにコンデンサを付けると雑音が下がり、抵抗で出力を少し動かせます。PSRR は図の点に合わせます");
        egui::CollapsingHeader::new(i18n::tr("考え方: どの値を使う? (はじめての人はここから)")).id_salt("vref_howto").show(ui, |ui| {
            for t in [
                "電圧基準は、ADC・DAC の物差しになる「正確でゆれない電圧」の IC です。出力の初期精度 (min・max) と温度による変化がいちばん大事な値です。",
                "ライン・レギュレーション [ppm/V]: 入力を 1 V 変えたときの出力の変化。ロード・レギュレーション [ppm/mA]: 1 mA 流したときの変化 (吐き出しと吸い込み)。",
                "ドロップアウト: 入力が出力よりこれだけ高くないと正確に出せない電圧。電流で増えるなら「0.02 54」のように増え方 [Ω] も。",
                "PSRR: 入力のゆれがどれだけ出力に漏れないか。表の 1 点だけでもよく、「PSRR vs Frequency」の図から何点か読むと、周波数による下がり方も合わせます。",
                "NR・TRIM ピン・TEMP ピンの欄は、そのピンの無い部品なら空にします (REF50xx の値はブロック図とピンの説明から)。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(i18n::tr("REF5025 の例を入れる")).on_hover_text(i18n::tr("同梱の REF5025 を作ったときの値 (TI のデータシートの表と図) を入れます")).clicked() {
                self.refldo_example_and_make(RefLdo::Reference);
            }
            if ui.button(i18n::tr("ピンを REF50xx の並びにする")).on_hover_text(i18n::tr("ピン表を SOIC-8 の REF50xx の並び (2 VIN・4 GND・6 VOUT・5 TRIM/NR・3 TEMP) にします")).clicked() {
                self.refldo_pins(RefLdo::Reference);
            }
        });
        let r = &mut self.vref;
        let mut changed = false;
        egui::Grid::new("vref_inputs").num_columns(2).show(ui, |ui| {
            changed |= Self::refldo_row(ui, "出力電圧 typ [V]・min max", "min・max は初期精度 (部品ごとの違い。2.5 V ± 0.05 % なら 2.49875 2.50125。空でも可)", vec![(&mut r.vout, "2.5", 60.0), (&mut r.vrange, "2.49875 2.50125", 130.0)]);
            changed |= Self::refldo_row(ui, "表の測定の入力電圧 [V]", "表の値を測ったときの入力電圧 (空なら出力 + 2 V)", vec![(&mut r.vin0, "4.5", 60.0)]);
            changed |= Self::refldo_row(ui, "ライン [ppm/V]・ロード [ppm/mA]", "ライン・レギュレーションと、ロード・レギュレーション (吐き出しと吸い込みの 2 つか、同じなら 1 つ)", vec![(&mut r.line, "1", 60.0), (&mut r.load, "20 20", 90.0)]);
            changed |= Self::refldo_row(ui, "電源電流 typ・max [A]", "自分で使う電流 (無負荷)。max はばらつきの解析で使います (空でも可)", vec![(&mut r.iq, "800u", 60.0), (&mut r.iq_max, "1m", 60.0)]);
            changed |= Self::refldo_row(ui, "ドロップアウト [V]・増え [Ω]", "無負荷のドロップアウトと、電流による増え (10 mA で 0.54 V 増えるなら 54)", vec![(&mut r.dropout, "0.02 54", 130.0)]);
            changed |= Self::refldo_row(ui, "短絡電流 [A]: 吐き出し 吸い込み", "出力を GND や電源につないだときの電流", vec![(&mut r.isc, "25m 20m", 130.0)]);
            changed |= Self::refldo_lines(ui, "PSRR: Hz dB", "1 行に「周波数 [Hz] 除去 [dB]」。1 行なら全部の周波数で同じ、2 行以上なら周波数特性をその点に合わせます", &mut r.psrr, "100 140\n10k 85");
            changed |= Self::refldo_row(ui, "出力雑音", "「rms [V] 帯域の下 上 [Hz]」(2.25u 10 1k) か、密度 1 つ (70n = 70 nV/√Hz)。NR にコンデンサを付けないときの値", vec![(&mut r.noise, "2.25u 10 1k", 130.0)]);
            changed |= Self::refldo_row(ui, "NR・TRIM ピン (空でも可)", "「バンドギャップの電圧 そこから増幅器までの抵抗 ピンまでの抵抗 大きなコンデンサで残る雑音の割合」。REF50xx は 1.2 10k 1k 0.5 (ブロック図と、1 µF で雑音が半分の文)", vec![(&mut r.nr, "1.2 10k 1k 0.5", 130.0)]);
            changed |= Self::refldo_row(ui, "TEMP ピン (空でも可)", "「25 ℃ の電圧 1 ℃ あたりの変化 出力抵抗」。REF50xx は 0.575 2.64m 60k", vec![(&mut r.temp_pin, "0.575 2.64m 60k", 130.0)]);
            let t = Self::refldo_lines(ui, "温度: ℃ 変化 [%]", "出力の温度による変化。1 行に「温度 [℃] 25 ℃ からの変化 [%]」を、低い温度から 2 行以上 (図の「Output Voltage vs Temperature」から)。空なら温度で変わりません", &mut r.temp, "-40 0.011\n125 0.010");
            if t {
                r.temp_from.clear();
            }
            changed |= t;
        });
        if changed {
            r.from_figures.clear();
        }
        self.refldo_buttons(ui, RefLdo::Reference);
    }

    pub(super) fn setldo_ui(&mut self, ui: &mut egui::Ui) {
        crate::icons::help_small(ui, "表の typ の値からモデルを作ります", "SET ピンから出る正確な電流 (LT3045 は 100 µA) を使う人の抵抗 RSET に流し、その電圧を出力に写す LDO のモデルを、同梱の LT3045 と同じひな形で作ります。出力電圧は RSET × 電流、起動の速さと雑音は SET のコンデンサ CSET で決まります。ILIM・EN/UV・PG の欄は、そのピンの無い部品なら空にします");
        egui::CollapsingHeader::new(i18n::tr("考え方: どの値を使う? (はじめての人はここから)")).id_salt("setldo_howto").show(ui, |ui| {
            for t in [
                "SET ピンの電流: 出力電圧を決める電流。3.3 V なら RSET = 3.3 V ÷ 100 µA = 33 kΩ。min・max は部品ごとの違い。",
                "オフセット: 出力と SET ピンの電圧の差 (typ 0)。min・max だけ入れます。",
                "電源電流: 「動作中 出力 1 A あたりの増え 止めたとき」。LT3045 は 2.2 mA、500 mA で約 12.5 mA (2 %)、止めて 0.3 µA。",
                "電流制限: 「入出力の差 電流」を 1〜2 行。差が大きいと制限が小さくなる部品は 2 行に。",
                "PSRR: 表の周波数ごとの値を 1 行ずつ。点の間の形を、高域通過を 2 つまで重ねて合わせます。",
                "雑音: 出力の増幅器の雑音 (CSET を大きくしたときの値) と、SET の電流の雑音 (RSET に流れて出力に出ます) を分けて入れます。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(i18n::tr("LT3045 の例を入れる")).on_hover_text(i18n::tr("同梱の LT3045 を作ったときの値 (ADI のデータシートの表と図) を入れます")).clicked() {
                self.refldo_example_and_make(RefLdo::SetLdo);
            }
            if ui.button(i18n::tr("ピンを LT3045 の並びにする")).on_hover_text(i18n::tr("ピン表を MSOP-12 の LT3045 の並び (1 IN・4 EN/UV・6 ILIM・8 SET・9 GND・11 OUT・10 OUTS・5 PG・7 PGFB) にします。IN・OUT の 2〜3 本のピンは 1 つにまとめた番号です")).clicked() {
                self.refldo_pins(RefLdo::SetLdo);
            }
        });
        let r = &mut self.setldo;
        let mut changed = false;
        egui::Grid::new("setldo_inputs").num_columns(2).show(ui, |ui| {
            changed |= Self::refldo_row(ui, "SET ピンの電流 typ [A]・min max", "min・max は部品ごとの違い (空でも可)", vec![(&mut r.iset, "100u", 60.0), (&mut r.iset_range, "99u 101u", 110.0)]);
            changed |= Self::refldo_row(ui, "オフセット min max [V]", "出力と SET ピンの電圧の差 (VOS) の min・max (空でも可)", vec![(&mut r.vos_range, "-1m 1m", 110.0)]);
            changed |= Self::refldo_row(ui, "表の測定の入力電圧 [V]・最小の入力 [V]", "表の値を測ったときの入力電圧と、これより下では止まる入力 (UVLO)", vec![(&mut r.vin0, "2", 60.0), (&mut r.vin_min, "1.78", 60.0)]);
            changed |= Self::refldo_row(ui, "ライン: SET の電流 [A/V] オフセット [V/V]", "入力を 1 V 変えたときの SET の電流とオフセットの変化 (空でも可)", vec![(&mut r.line, "0.5n 0.5u", 110.0)]);
            changed |= Self::refldo_row(ui, "ロード: 出力抵抗 [Ω]", "出力抵抗 1 つか、「オフセットの変化 [V] 電流の変化 [A]」(0.1m 0.499)", vec![(&mut r.load, "200u", 110.0)]);
            changed |= Self::refldo_row(ui, "ドロップアウト [V]・増え [Ω]", "小さい電流のドロップアウトと、電流による増え (0.21 V + 0.1 Ω × 電流)", vec![(&mut r.dropout, "0.21 0.1", 110.0)]);
            changed |= Self::refldo_row(ui, "電源電流 [A]: 動作中 増え 止めたとき", "動作中の GND の電流・出力電流 1 A あたりの増え (A/A)・EN で止めたときの電流。1〜3 つ", vec![(&mut r.iq, "2.2m 0.02 0.3u", 130.0)]);
            changed |= Self::refldo_lines(ui, "電流制限: 差 [V] 電流 [A]", "1 行に「入出力の差 電流」。1 行か 2 行 (差で変わる部品)", &mut r.ilim, "12 0.7\n20 0.33");
            changed |= Self::refldo_row(ui, "EN のしきい値 [V] (空でも可)", "EN/UV ピンで動き始める電圧。空なら EN ピンなし", vec![(&mut r.en, "1.24", 60.0)]);
            changed |= Self::refldo_row(ui, "ILIM ピン: 比 しきい値 [V] (空でも可)", "ILIM から出る電流が出力電流の何分の 1 か、と制限が効くピンの電圧。LT3045 は 500 0.3 (1 kΩ で 150 mA)", vec![(&mut r.ilim_pin, "500 0.3", 110.0)]);
            changed |= Self::refldo_row(ui, "PG: しきい値 [V] 抵抗 [Ω] 速い起動 [A] (空でも可)", "PGFB のしきい値、PG が Low のときの抵抗、PGFB がしきい値より下の間 SET に足す電流 (速い起動、無ければ 2 つだけ)", vec![(&mut r.pg, "0.3 300 1.9m", 130.0)]);
            changed |= Self::refldo_lines(ui, "PSRR: Hz dB", "1 行に「周波数 [Hz] 除去 [dB]」。表の周波数ごとの値を入れます", &mut r.psrr, "120 117\n10k 90");
            changed |= Self::refldo_row(ui, "出力雑音 (増幅器)", "「rms [V] 帯域の下 上 [Hz]」(0.8u 10 100k) か密度 1 つ。CSET を大きくしたときの値", vec![(&mut r.noise, "0.8u 10 100k", 130.0)]);
            changed |= Self::refldo_row(ui, "SET の電流の雑音", "「rms [A] 帯域の下 上 [Hz]」(6n 10 100k) か密度 1 つ [A/√Hz]。RSET に流れて出力の雑音になります", vec![(&mut r.iset_noise, "6n 10 100k", 130.0)]);
            let t = Self::refldo_lines(ui, "SET の電流の温度: ℃ 変化 [%]", "SET の電流の温度による変化 (出力も同じ割合で動きます)。1 行に「温度 [℃] 25 ℃ からの変化 [%]」を、低い温度から 2 行以上 (図の「SET Pin Current vs Temperature」から)。空なら温度で変わりません", &mut r.iset_temp, "-50 -0.098
150 0.13");
            if t {
                r.iset_temp_from.clear();
            }
            changed |= t;
            let t = Self::refldo_lines(ui, "電源電流の温度: ℃ 変化 [%]", "動作中の電源電流の温度による変化。1 行に「温度 [℃] 25 ℃ からの変化 [%]」(図の「Quiescent Current vs Temperature」から)。空なら温度で変わりません", &mut r.iq_temp, "-50 -7.7
150 24.9");
            if t {
                r.iq_temp_from.clear();
            }
            changed |= t;
        });
        if changed {
            r.from_figures.clear();
        }
        self.refldo_buttons(ui, RefLdo::SetLdo);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled(part: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../part-lib/examples").join(format!("{part}.part.json"));
        let d: part_lib::PartDef = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        d.sim.unwrap().text
    }

    #[test]
    fn the_examples_give_the_bundled_models() {
        // the boxes as the example button fills them give the bundled model again
        let s = ReferenceInput::from_spec(&ref5025()).spec("REF5025", "s").unwrap();
        assert_eq!(fit::fit(&fit::Spec::Reference(s)).unwrap().text, bundled("REF5025_SOIC-8"));
        let s = SetLdoInput::from_spec(&lt3045()).spec("LT3045", "s").unwrap();
        assert_eq!(fit::fit(&fit::Spec::SetLdo(s)).unwrap().text, bundled("LT3045_MSOP-12"));
    }

    #[test]
    fn missing_or_wrong_values_say_which_and_how() {
        let mut r = ReferenceInput::from_spec(&ref5025());
        r.isc = "25m".into();
        assert!(r.spec("X", "s").unwrap_err().contains("短絡電流"));
        r = ReferenceInput::from_spec(&ref5025());
        r.psrr = "100".into();
        assert!(r.spec("X", "s").unwrap_err().contains("PSRR"));
        // the optional pins may be left empty, and the load as one number for both ways
        r = ReferenceInput::from_spec(&ref5025());
        (r.nr, r.temp_pin, r.load) = (String::new(), String::new(), "20".into());
        let s = r.spec("X", "s").unwrap();
        assert!(s.nr.is_none() && s.temp_pin.is_none() && s.load_snk_ppm_ma == 20.0);
        assert_eq!(reference_roles(&r).len(), 3, "no NR or TEMP pin wanted");
        let mut l = SetLdoInput::from_spec(&lt3045());
        l.pg = "0.3".into();
        assert!(l.spec("X", "s").unwrap_err().contains("PG"));
        l = SetLdoInput::from_spec(&lt3045());
        l.load = "0.1m 0.499".into();
        assert!((l.spec("X", "s").unwrap().rout - 0.1e-3 / 0.499).abs() < 1e-12, "a change over a step is a resistance");
        (l.en, l.pg, l.ilim_pin) = (String::new(), String::new(), String::new());
        assert_eq!(setldo_roles(&l).len(), 5);
    }
}
