//! The first power-on of a built board (candidate B1): from the DC operating point, how much current each supply of the schematic
//! should deliver, the range a working board falls in (the parts' tolerances and the models' data sheet spreads at their ends, plus the
//! self-heating), the current limit to set on a bench supply, and what a reading outside the range points at.
//!
//! The range is found like `worst_case`: each toleranced part alone at both ends of its tolerance, then the corner where every part
//! sits at the end that lowers (raises) the supply's current is solved as a real circuit. The self-heating shifts the answer by the
//! difference between the warm and the cold operating point (`self_heat`), added on the side it moves. This is the steady current; the
//! first milliseconds charge the capacitors (inrush) and can touch the limit for a moment.

use crate::circuit::{Circuit, Element};
use crate::dc::{dc_op, DcResult, SimError};
use crate::montecarlo::{spread_swings, swing_to, Swing};
use crate::psrr::supply_sources;
use crate::selfheat::{self_heat, Heater};
use crate::worstcase::{tolerance, WcSettings};

#[derive(Clone, Debug)]
pub struct PowerOnSettings {
    /// Tolerances as fractions (0.05 = ±5 %), as `WcSettings`. `tol_v` moves the supplies themselves.
    pub tol_r: f64,
    pub tol_c: f64,
    pub tol_l: f64,
    pub tol_v: f64,
    /// The models' own min / max (`Circuit::spreads`: a regulator's quiescent current, an op amp's offset) at their ends too.
    pub spreads: bool,
    /// Surrounding temperature for the self-heating (°C).
    pub ambient: f64,
    /// The limit is the highest normal current times this (1.5: half again), rounded up to a value a supply's knob can show.
    pub margin: f64,
}

impl Default for PowerOnSettings {
    fn default() -> Self {
        Self { tol_r: 0.05, tol_c: 0.2, tol_l: 0.2, tol_v: 0.0, spreads: true, ambient: 25.0, margin: 1.5 }
    }
}

/// The smallest limit suggested (A): most bench supplies cannot be set much finer, and a few mA hurt nothing.
pub const LIMIT_FLOOR: f64 = 0.01;

/// One supply's expected current.
#[derive(Clone, Debug, PartialEq)]
pub struct SupplyCurrent {
    /// The source (`V1`) and its voltage.
    pub name: String,
    pub volts: f64,
    /// The current it delivers with every part at its printed value, cold (A, never negative: a source that takes current in is left out).
    pub nominal: f64,
    /// The same once the parts have warmed by their own power (`None` without heated parts).
    pub warm: Option<f64>,
    /// The range a working board falls in (A).
    pub min: f64,
    pub max: f64,
    /// The current limit to set (A).
    pub limit: f64,
    /// The parts that move this current most, alone at their low / high end: (name, change at low end, change at high end) in A.
    pub parts: Vec<(String, f64, f64)>,
    /// The circuit does not sit at its DC point (it oscillates or keeps changing): the current of a transient run's steady part.
    /// The range and the limit are then moved to it (`PowerOn::apply_running`).
    pub running: Option<Running>,
}

/// A supply's current while the circuit runs (A): a transient run's steady part, and the DC point's current it replaced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Running {
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    pub dc: f64,
}

/// The steady part of a transient signal: its second half (the start-up left out), as (time-weighted mean, min, max).
pub fn steady_stats(t: &[f64], v: &[f64]) -> Option<(f64, f64, f64)> {
    if t.len() < 3 || t.len() != v.len() {
        return None;
    }
    let half = t[0] + (t[t.len() - 1] - t[0]) / 2.0;
    let (mut area, mut span, mut lo, mut hi) = (0.0, 0.0, f64::INFINITY, f64::NEG_INFINITY);
    for i in 1..t.len() {
        if t[i] < half {
            continue;
        }
        let dt = t[i] - t[i - 1].max(half);
        area += (v[i] + v[i - 1]) / 2.0 * dt;
        span += dt;
        lo = lo.min(v[i]);
        hi = hi.max(v[i]);
    }
    (span > 0.0).then(|| (area / span, lo, hi))
}

/// Whether a running signal (its steady part: mean, min, max) is not the DC value `dc`: the mean is off by more than 5 % (and
/// `floor`, for values near zero), or it swings by more than 10 % of its size (and `floor`).
pub fn runs_away_from_dc(dc: f64, mean: f64, lo: f64, hi: f64, floor: f64) -> bool {
    let size = dc.abs().max(mean.abs());
    (mean - dc).abs() > (0.05 * size).max(floor) || (hi - lo) > (0.1 * size).max(floor)
}

#[derive(Clone, Debug)]
pub struct PowerOn {
    pub supplies: Vec<SupplyCurrent>,
    /// Parts whose own runs did not converge (left at their printed value).
    pub unsolved: Vec<String>,
    /// Whether the self-heating was taken in (some part warms).
    pub heated: bool,
}

/// A rounded-up value a supply can be set to: 1, 1.2, 1.5, 2, 2.5, 3, 4, 5, 6, 8 × 10ⁿ.
pub fn nice_up(x: f64) -> f64 {
    if !(x > 0.0) || !x.is_finite() {
        return 0.0;
    }
    let decade = 10f64.powf(x.log10().floor());
    for m in [1.0, 1.2, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0] {
        let v = m * decade;
        if v >= x * (1.0 - 1e-9) {
            return v;
        }
    }
    10.0 * decade
}

/// The current a source delivers (A): its current when it gives power, 0 when it takes power in (a cell being charged).
fn delivered(r: &DcResult, name: &str) -> Option<f64> {
    let e = r.element(name)?;
    Some(if e.power < 0.0 { e.current.abs() } else { 0.0 })
}

pub fn power_on(circuit: &Circuit, heaters: &[Heater], s: &PowerOnSettings) -> Result<PowerOn, SimError> {
    if [s.tol_r, s.tol_c, s.tol_l, s.tol_v].iter().any(|t| !(0.0..1.0).contains(t)) {
        return Err(SimError::Invalid("許容差は 0〜100 % 未満にしてください".into()));
    }
    if !(s.margin >= 1.0) {
        return Err(SimError::Invalid("制限の余裕は 1 倍以上にしてください".into()));
    }
    let names = supply_sources(circuit);
    if names.is_empty() {
        return Err(SimError::Invalid("回路図に直流の電源 (電圧源) がありません。電源を置いて、電圧を入れてください".into()));
    }
    let base = dc_op(circuit)?;
    let volts = |n: &str| {
        circuit.elements.iter().find_map(|e| match e {
            Element::VSource { name, wave: crate::waveform::Waveform::Dc(v), .. } if name == n => Some(*v),
            _ => None,
        })
    };
    // the supplies that give power (a source that only takes current in is a load, not a supply)
    let supplies: Vec<(String, f64, f64)> = names.iter().filter_map(|n| Some((n.clone(), volts(n)?, delivered(&base, n)?))).filter(|(_, _, i)| *i > 0.0).collect();
    if supplies.is_empty() {
        return Err(SimError::Invalid("電源から電流が流れ出していません。負荷がつながっているか、電源の向きを確かめてください".into()));
    }
    let currents = |r: &DcResult| -> Vec<Option<f64>> { supplies.iter().map(|(n, _, _)| delivered(r, n)).collect() };

    // every part alone at both ends: one solve gives every supply's current
    let ws = WcSettings { tol_r: s.tol_r, tol_c: s.tol_c, tol_l: s.tol_l, tol_v: s.tol_v, output: String::new(), measure: crate::McMeasure::DcVoltage };
    let mut named: Vec<(usize, Swing, String)> =
        circuit.elements.iter().enumerate().filter_map(|(i, e)| tolerance(e, &ws).map(|t| (i, Swing::Rel(t), e.name().to_string()))).collect();
    if s.spreads {
        named.extend(spread_swings(circuit));
    }
    let jobs: Vec<(usize, bool)> = (0..named.len()).flat_map(|k| [(k, false), (k, true)]).collect();
    let solve = |c: &Circuit| dc_op(c).ok().map(|r| currents(&r));
    let alone = crate::par_map(&jobs, |&(k, high)| {
        let mut c = circuit.clone();
        swing_to(&mut c.elements[named[k].0], named[k].1, if high { 1.0 } else { -1.0 });
        solve(&c)
    });
    let mut unsolved: Vec<String> = vec![];
    for (k, (_, _, name)) in named.iter().enumerate() {
        if alone[2 * k].is_none() || alone[2 * k + 1].is_none() {
            unsolved.push(name.clone());
        }
    }

    // the warm operating point
    let warm = if heaters.is_empty() { None } else { self_heat(circuit, heaters, s.ambient).ok().map(|h| currents(&h.warm)) };
    let heated = warm.is_some();

    let mut out = vec![];
    for (j, (name, v, nominal)) in supplies.iter().enumerate() {
        let at = |k: usize, high: bool| alone[2 * k + high as usize].as_ref().and_then(|r| r[j]).map(|i| i - nominal);
        // per part: the change at each end, and which end raises the current
        let mut parts: Vec<(String, f64, f64)> = vec![];
        let mut up_high = vec![None; named.len()];
        for (k, (_, _, pname)) in named.iter().enumerate() {
            if let (Some(lo), Some(hi)) = (at(k, false), at(k, true)) {
                up_high[k] = Some(hi >= lo);
                parts.push((pname.clone(), lo, hi));
            }
        }
        // the two corners, solved
        let corner = |raise: bool| -> Option<f64> {
            let mut c = circuit.clone();
            for (k, &(i, swing, _)) in named.iter().enumerate() {
                if let Some(h) = up_high[k] {
                    let high = if raise { h } else { !h };
                    swing_to(&mut c.elements[i], swing, if high { 1.0 } else { -1.0 });
                }
            }
            solve(&c).and_then(|r| r[j])
        };
        let alone_min = parts.iter().map(|p| nominal + p.1.min(p.2)).fold(*nominal, f64::min);
        let alone_max = parts.iter().map(|p| nominal + p.1.max(p.2)).fold(*nominal, f64::max);
        let (mut min, mut max) = if parts.is_empty() { (*nominal, *nominal) } else { (corner(false).unwrap_or(alone_min).min(alone_min), corner(true).unwrap_or(alone_max).max(alone_max)) };
        let warm_i = warm.as_ref().and_then(|w| w[j]);
        if let Some(w) = warm_i {
            let d = w - nominal;
            if d < 0.0 {
                min += d;
            } else {
                max += d;
            }
        }
        min = min.max(0.0);
        parts.retain(|p| p.1.abs().max(p.2.abs()) > 1e-12);
        parts.sort_by(|a, b| (b.1 - b.2).abs().total_cmp(&(a.1 - a.2).abs()));
        let limit = nice_up(max * s.margin).max(LIMIT_FLOOR);
        out.push(SupplyCurrent { name: name.clone(), volts: *v, nominal: *nominal, warm: warm_i, min, max, limit, parts, running: None });
    }
    out.sort_by(|a, b| (b.volts.abs() * b.nominal).total_cmp(&(a.volts.abs() * a.nominal)));
    Ok(PowerOn { supplies: out, unsolved, heated })
}

impl PowerOn {
    /// A supply's current from a transient run (the source's current signal `t`, `i`, in the simulator's sign: negative while it
    /// delivers). When the steady part is not the DC current (a 555 that blinks: its DC point is the timer stopped half way, with
    /// the LED off), the supply's numbers move to the running circuit: the mean replaces the DC current, the range keeps its width
    /// relative to it (the tolerances move the running current about as they move the DC one), and the limit also covers the
    /// highest current of the run. True when they moved. A bench supply shows about the mean.
    pub fn apply_running(&mut self, name: &str, t: &[f64], i: &[f64], margin: f64) -> bool {
        let Some(s) = self.supplies.iter_mut().find(|s| s.name == name) else { return false };
        let delivered: Vec<f64> = i.iter().map(|x| -x).collect();
        let Some((mean, lo, hi)) = steady_stats(t, &delivered) else { return false };
        if !(mean > 0.0) || !runs_away_from_dc(s.nominal, mean, lo, hi, 1e-6) {
            return false;
        }
        let scale = if s.nominal > 0.0 { mean / s.nominal } else { 1.0 };
        let (min, max) = if s.nominal > 0.0 { (s.min * scale, s.max * scale) } else { (lo.max(0.0), hi) };
        let dc = s.nominal;
        s.min = min.min(mean);
        s.max = max.max(mean);
        s.nominal = mean;
        s.warm = s.warm.map(|w| w * scale);
        s.limit = nice_up(s.max.max(hi) * margin).max(LIMIT_FLOOR);
        s.running = Some(Running { mean, min: lo.max(0.0), max: hi, dc });
        true
    }
}

/// Where a measured current sits against the expected range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// Almost nothing flows (under a tenth of the lowest normal current).
    Nothing,
    /// Clearly less than the range (under half of its low end).
    MuchLow,
    /// A little under the range.
    Low,
    Normal,
    /// A little over the range.
    High,
    /// Clearly more than the range (over twice its high end), or the supply is at its limit.
    MuchHigh,
}

impl Reading {
    /// A short Japanese name of the verdict.
    pub fn title(self) -> &'static str {
        match self {
            Reading::Nothing => "ほとんど流れていない",
            Reading::MuchLow => "ずっと少ない",
            Reading::Low => "少し少ない",
            Reading::Normal => "正常な範囲",
            Reading::High => "少し多い",
            Reading::MuchHigh => "ずっと多い (すぐ電源を切る)",
        }
    }

    /// Whether the power should be cut at once.
    pub fn cut_now(self) -> bool {
        self == Reading::MuchHigh
    }

    /// The causes to look at, likeliest first (Japanese: what it is, and how to check it).
    pub fn causes(self) -> &'static [&'static str] {
        match self {
            Reading::Nothing => &[
                "電源の線・スイッチ・ヒューズ・コネクタが切れている (電源の端子と基板の電源のパッドの電圧をテスタで比べる)",
                "電流計のつなぎ方・レンジが違う (電源の表示と比べる)",
                "電源の部品 (レギュレータ・ダイオード) が付いていない・向きが逆",
                "電源の出力が OFF のまま",
            ],
            Reading::MuchLow => &[
                "部品の付け忘れ (電流をたくさん使う部品: LED・レギュレータ・IC・負荷の抵抗)",
                "はんだ不良: 足がパッドから浮いている・つやのない冷たいはんだ (拡大鏡で見て、テスタで導通を確かめる)",
                "IC が動いていない (イネーブル・リセットのピン、水晶)",
                "値の違う部品 (抵抗が 10 倍大きい: 色の帯の読み違い)",
            ],
            Reading::Low => &[
                "部品の値の違い (抵抗が大きめ、LED の色違い)",
                "電源の電圧が設定より低い (電源の端子で測る)",
                "計器の誤差 (小さい電流はレンジを下げて測る)",
            ],
            Reading::Normal => &["正常な範囲です。電源の電圧が設定どおりか、熱い部品がないかも確かめてから、次の確認 (各部の電圧) に進みます"],
            Reading::High => &[
                "部品の値の違い (抵抗が小さめ: 色の帯の読み違い)",
                "部品が熱くなっていないか (触る前に、手をかざすか温度計で)",
                "わずかな漏れ: フラックスの残り・湿り (IPA でふき取る)",
                "電源の電圧が設定より高い",
            ],
            Reading::MuchHigh => &[
                "ショート: はんだのブリッジ・電源と GND がつながっている (電源を切って、テスタの抵抗レンジで電源と GND の間を測る)",
                "部品の向き違い: 電解コンデンサ・ダイオード・LED・IC の 1 番ピン (向きの印を基板の印と見比べる)",
                "違う部品が付いている (似た形の IC・トランジスタの型番)",
                "電源の + と − を逆につないだ",
                "熱い部品がその場所 (触らずに、手をかざすか温度計・サーモカメラで探す)",
            ],
        }
    }
}

impl SupplyCurrent {
    /// Where `measured` (A) sits: inside the range, a little or clearly out. At 95 % of the limit the supply is limiting (CC), which
    /// counts as clearly too much.
    pub fn judge(&self, measured: f64) -> Reading {
        let m = measured.abs();
        if m >= self.limit * 0.95 || m > self.max * 2.0 {
            Reading::MuchHigh
        } else if m > self.max {
            Reading::High
        } else if m < self.min * 0.1 {
            Reading::Nothing
        } else if m < self.min * 0.5 {
            Reading::MuchLow
        } else if m < self.min {
            Reading::Low
        } else {
            Reading::Normal
        }
    }
}
