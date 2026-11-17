//! "フィルタの設計" (candidate V8): puts the designed filter on the sheet (one op-amp per stage, the parts joined by net labels,
//! an AC 1 input source and ±5 V supplies), runs the AC analysis over it and draws the target response on the Bode plot as
//! points (a `bench_data` curve named 目標). The calculation is `sim_core::design_filter`; the window is the value wizard's
//! filter calculator.

use super::*;
use sch_core::Fragment;
use sim_core::{FilterDesign, FilterFamily, FilterShape};

/// The op-amps to use, the first the library has (TL072: 3 MHz GBW, enough for audio filters; LM358: 1 MHz).
const OPAMPS: [&str; 2] = ["TL072", "LM358"];

/// Net-name prefixes tried in turn, so a second filter on the sheet gets its own nets.
fn net_prefix(sch: &Schematic) -> String {
    let used = |p: &str| sch.labels.iter().any(|l| l.text == format!("{p}IN") || l.text == format!("{p}OUT"));
    (1..).map(|k| if k == 1 { "F".to_string() } else { format!("F{k}_") }).find(|p| !used(p)).unwrap_or_default()
}

/// A short wire out of pin `pin` of component `idx` and a net label at its end (the way the sample circuits join parts).
fn label_pin(s: &mut Schematic, idx: usize, pin: &str, text: &str) {
    let Some((_, pt)) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin) else { return };
    let c = s.components[idx].pos;
    let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
    let end = if dx.abs() >= 3 || (dx != 0 && dx.abs() >= dy.abs()) { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
    s.wires.push(Wire { a: pt, b: end });
    s.labels.push(Label { pos: end, text: text.into() });
}

/// Phase made continuous along the sweep, as the AC result's (`AcSignal::phase_deg`).
fn unwrap_deg(ph: impl Iterator<Item = f64>) -> Vec<f64> {
    let mut out: Vec<f64> = vec![];
    for mut p in ph {
        if let Some(&prev) = out.last() {
            p += 360.0 * ((prev - p) / 360.0).round();
        }
        out.push(p);
    }
    out
}

/// What the filter is, in a few words (the target curve's name).
pub(super) fn filter_title(d: &FilterDesign) -> String {
    let fam = match d.spec.family {
        FilterFamily::Butterworth => "バターワース".to_string(),
        FilterFamily::Bessel => "ベッセル".to_string(),
        FilterFamily::Chebyshev(r) => format!("チェビシェフ {r}dB"),
    };
    let (shape, n) = match d.spec.shape {
        FilterShape::LowPass => ("ローパス", d.spec.order),
        FilterShape::HighPass => ("ハイパス", d.spec.order),
        FilterShape::BandPass => ("バンドパス", d.spec.order * 2),
    };
    format!("{n}次 {fam} {shape} {}Hz", format_value(d.spec.freq))
}

/// The filter as sheet pieces: (fragment, input net, output net). `has_supplies`: the sheet already has VCC / VEE.
pub(super) fn filter_fragment(d: &FilterDesign, opamp: &part_lib::PartDef, prefix: &str, has_supplies: bool) -> Result<(Fragment, String, String), String> {
    let sym = opamp.symbol();
    let pin = |name: &str| sym.pins.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.number.clone()).ok_or_else(|| format!("{} に {name} のピンがありません", opamp.name));
    let (p_plus, p_minus, p_out, p_vp, p_vn) = (pin("IN+")?, pin("IN-")?, pin("OUT")?, pin("V+")?, pin("V-")?);
    let mut s = Schematic::default();
    let (input, output) = (format!("{prefix}IN"), format!("{prefix}OUT"));
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "SIN(0 1 1k) AC 1".into();
    label_pin(&mut s, v1, "+", &input);
    label_pin(&mut s, v1, "-", "GND");
    let mut prev = input.clone();
    let n = d.stages.len();
    for (k, st) in d.stages.iter().enumerate() {
        let x0 = 14 + 40 * k as i32;
        let out = if k + 1 == n { output.clone() } else { format!("{prefix}S{}", k + 1) };
        let (parts, op) = st.netlist(&prev, &out, &|x| format!("{prefix}S{}{x}", k + 1), true);
        for (j, p) in parts.iter().enumerate() {
            let i = s.add(if p.is_cap() { Kind::Capacitor } else { Kind::Resistor }, (x0 + 6, -12 + 6 * j as i32), 0);
            s.components[i].value = format_value(p.value);
            label_pin(&mut s, i, "1", &p.a);
            label_pin(&mut s, i, "2", &p.b);
        }
        let u = s.add_custom(sym.clone(), &opamp.default_value(), &opamp.footprint_name(), (x0 + 26, 0), 0);
        label_pin(&mut s, u, &p_plus, &op.plus);
        label_pin(&mut s, u, &p_minus, &op.minus);
        label_pin(&mut s, u, &p_out, &op.out);
        label_pin(&mut s, u, &p_vp, "VCC");
        label_pin(&mut s, u, &p_vn, "VEE");
        prev = out;
    }
    if !has_supplies {
        let x = 14 + 40 * n as i32;
        let vp = s.add(Kind::VSource, (x, -6), 0);
        s.components[vp].value = "5".into();
        let vn = s.add(Kind::VSource, (x, 10), 0);
        s.components[vn].value = "5".into();
        label_pin(&mut s, vp, "+", "VCC");
        label_pin(&mut s, vp, "-", "GND");
        label_pin(&mut s, vn, "+", "GND");
        label_pin(&mut s, vn, "-", "VEE");
    }
    Ok((Fragment { components: s.components, wires: s.wires, labels: s.labels }, input, output))
}

impl SchEditor {
    /// Puts the filter on free ground of this sheet, runs the AC analysis over its range and draws the target on it.
    pub(super) fn place_filter(&mut self, d: &FilterDesign) -> Result<(), String> {
        let opamp = OPAMPS.iter().find_map(|n| self.library.iter().find(|p| p.name == *n)).cloned().ok_or("ライブラリにオペアンプ (TL072 か LM358) が見つかりません")?;
        let prefix = net_prefix(&self.sch);
        let has_supplies = self.sch.labels.iter().any(|l| l.text == "VCC") && self.sch.labels.iter().any(|l| l.text == "VEE");
        let (frag, input, output) = filter_fragment(d, &opamp, &prefix, has_supplies)?;
        let kept = std::mem::replace(&mut self.clipboard, frag);
        let before = self.sch.components.len();
        self.paste_clipboard();
        self.clipboard = kept;
        if self.sch.components.len() == before {
            return Err(self.status.clone());
        }
        // the target as points on the Bode plot, 10 a decade over the plotted range
        let (f0, f1) = d.plot_range();
        let decades = (f1 / f0).log10();
        let n = (decades * 10.0).round() as usize;
        let x: Vec<f64> = (0..=n).map(|i| f0 * 10f64.powf(decades * i as f64 / n as f64)).collect();
        let resp: Vec<sim_core::C64> = x.iter().map(|f| d.response(*f, false)).collect();
        let name = format!("目標 ({})", filter_title(d));
        let signal = format!("V({output})");
        self.bench.ac.retain(|c| c.signal != signal || !c.name.starts_with("目標"));
        self.bench.ac.push(bench_data::BenchCurve { name, signal: signal.clone(), y: resp.iter().map(|h| 20.0 * h.abs().log10()).collect(), phase: unwrap_deg(resp.iter().map(|h| h.arg_deg())), x, shift: 0.0 });
        self.set_ac_settings(&format_value(f0), &format_value(f1), "20");
        self.analyze_ac();
        // the input and the output (the AC run adds the target's signal itself; the input is left out of its first three)
        if let Some(view) = self.ac.as_mut()
            && let Ok(r) = &view.outcome
        {
            let want: BTreeSet<String> = [format!("V({input})"), signal].into_iter().filter(|n| r.signal(n).is_some()).collect();
            if !want.is_empty() {
                view.selected = want;
            }
        }
        self.status = format!("{} を置き、AC 解析をしました。点が目標の特性です (部品を選んで値を変え、F5 で比べられます)", filter_title(d));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{design_filter, FilterSpec, FilterTopology};

    fn editor() -> SchEditor {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        e
    }

    #[test]
    fn a_placed_filter_follows_its_target() {
        // a 4th-order Butterworth low-pass at 1 kHz, Sallen-Key, E96, on TL072s: within 0.5 dB of the target up to 2 kHz
        let d = design_filter(&FilterSpec { family: FilterFamily::Butterworth, shape: FilterShape::LowPass, order: 4, freq: 1e3, bandwidth: 0.0, topology: FilterTopology::SallenKey, e96: true }).unwrap();
        let mut e = editor();
        e.place_filter(&d).unwrap();
        assert_eq!(e.sch.components.iter().filter(|c| c.value == "TL072").count(), 2, "one op-amp a stage");
        let view = e.ac.as_ref().unwrap();
        let r = view.outcome.as_ref().unwrap();
        assert!(view.selected.contains("V(FOUT)"), "{:?}", view.selected);
        let out = r.signal("V(FOUT)").unwrap();
        for (i, f) in r.freq.iter().enumerate().filter(|(_, f)| **f <= 2e3) {
            let sim = 20.0 * out.mag()[i].log10();
            assert!((sim - d.db(*f, false)).abs() < 0.5, "at {f} Hz: {sim:.2} dB, target {:.2} dB", d.db(*f, false));
        }
        assert_eq!(e.bench.ac.len(), 1, "the target is drawn");
        let ph = &e.bench.ac[0].phase;
        assert!(ph.windows(2).all(|w| (w[1] - w[0]).abs() < 90.0) && ph.last().unwrap() < &-300.0, "continuous down to −360°: {ph:?}");
        // a second one gets its own nets and does not add supplies again
        let d2 = design_filter(&FilterSpec { shape: FilterShape::HighPass, order: 1, ..d.spec }).unwrap();
        e.place_filter(&d2).unwrap();
        assert!(e.sch.labels.iter().any(|l| l.text == "F2_OUT"));
        assert_eq!(e.sch.components.iter().filter(|c| c.kind == Kind::VSource).count(), 4, "two inputs, one pair of supplies");
        assert_eq!(e.bench.ac.len(), 2);
    }

    /// Generates docs/examples/active_filter.schjson (the calculator's default: 4th-order Butterworth low-pass at 1 kHz,
    /// Sallen-Key, E96, on TL072, with the target on the Bode plot): `cargo test -p app write_active_filter_example -- --ignored`
    #[test]
    #[ignore]
    fn write_active_filter_example() {
        let d = design_filter(&FilterSpec { family: FilterFamily::Butterworth, shape: FilterShape::LowPass, order: 4, freq: 1e3, bandwidth: 0.0, topology: FilterTopology::SallenKey, e96: true }).unwrap();
        let mut e = editor();
        e.place_filter(&d).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        std::fs::write(root.join("active_filter.schjson"), e.sheet_json().unwrap()).unwrap();
    }
}
