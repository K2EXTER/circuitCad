//! What the simulation does not know about a circuit: parts without a model (left out of the calculation), parts that use a
//! general-purpose model instead of the data of their own part number, library models with their own caveats, and the test sources that are
//! not on the board. The results are only as good as this list is short.

use sch_core::{Kind, Schematic};

/// One part and what is to be said about it.
#[derive(Clone, Debug, PartialEq)]
pub struct PartNote {
    pub refdes: String,
    pub value: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelCoverage {
    /// Parts that take part in the circuit but have no model: they are left out, as if they were not there.
    pub ignored: Vec<PartNote>,
    /// Diodes, LEDs and NPN transistors that use the built-in general-purpose parameters, not the data of a part number.
    pub generic: Vec<PartNote>,
    /// Library parts whose model comes with caveats (what it leaves out, where its numbers come from).
    pub caveats: Vec<PartNote>,
    /// Parts only for the simulation (test sources, the thing to measure): they are not on the board.
    pub test_sources: Vec<PartNote>,
    /// How many parts the circuit has that are simulated (not counting symbols and pin headers).
    pub total: usize,
}

impl ModelCoverage {
    /// Nothing is left out and nothing is only roughly modelled.
    pub fn is_clean(&self) -> bool {
        self.ignored.is_empty() && self.generic.is_empty() && self.caveats.is_empty()
    }
}

/// The coverage of the whole schematic (all sheets).
pub fn model_coverage(s: &Schematic) -> ModelCoverage {
    let flat = s.flat();
    let mut out = ModelCoverage::default();
    for c in flat.components.iter() {
        if matches!(c.kind, Kind::Ground | Kind::Power | Kind::Header(_)) {
            continue;
        }
        out.total += 1;
        let note = |notes: Vec<String>| PartNote { refdes: c.refdes.clone(), value: c.value.clone(), notes };
        if c.sim_only {
            out.test_sources.push(note(vec!["シミュレーション専用です (基板には載りません)。実物では、この部品のかわりに測る相手や電源につながります".into()]));
        }
        match c.kind {
            Kind::Diode => out.generic.push(note(vec!["汎用のダイオードの標準値で計算します (IS・N など)。実物の型番の特性ではありません".into()])),
            Kind::Led => out.generic.push(note(vec!["汎用の LED の標準値で計算します (順方向電圧は 2 V 前後)。色や型番による違いは入っていません".into()])),
            Kind::Npn => out.generic.push(note(vec!["汎用の NPN (hFE 200) で計算します。実物の型番の特性ではありません".into()])),
            Kind::Custom => match c.custom.as_ref().and_then(|x| x.sim.as_ref()) {
                None => out.ignored.push(note(vec!["シミュレーションモデルがありません。計算では、回路にないものとして扱います。動作や電流に関わる部品なら、結果は実物と違います".into()])),
                Some(m) if !m.limits.is_empty() => out.caveats.push(note(m.limits.clone())),
                Some(_) => {}
            },
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sch_core::{CustomSymbol, PinSide, SimKind, SimModel, SymPin};

    fn part(name: &str, sim: Option<SimModel>) -> CustomSymbol {
        let pin = |n: &str, side| SymPin { number: n.into(), name: n.into(), side, sim_only: false };
        CustomSymbol { name: name.into(), prefix: "U".into(), pins: vec![pin("1", PinSide::Left), pin("2", PinSide::Right)], sim, ratings: None, glyph: String::new(), shops: Default::default() }
    }

    fn model(limits: &[&str]) -> SimModel {
        SimModel { kind: SimKind::Model, element: "D".into(), name: "X".into(), text: ".model X D".into(), pins: vec!["1".into(), "2".into()], params: Default::default(), limits: limits.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn it_tells_what_the_simulation_leaves_out_or_guesses() {
        let mut s = Schematic::default();
        s.add(Kind::Resistor, (0, 0), 0);
        s.add(Kind::Ground, (4, 0), 0);
        s.add(Kind::Header(2), (8, 0), 0);
        s.add(Kind::Led, (12, 0), 0);
        s.add(Kind::Npn, (16, 0), 0);
        s.add_custom(part("ADC", None), "ADC", "x", (20, 0), 0);
        s.add_custom(part("OK", Some(model(&[]))), "OK", "x", (24, 0), 0);
        s.add_custom(part("ROUGH", Some(model(&["ボディダイオードは含みません"]))), "ROUGH", "x", (28, 0), 0);
        let i = s.add(Kind::VSource, (32, 0), 0);
        s.components[i].sim_only = true;
        let c = model_coverage(&s);
        assert_eq!(c.total, 7, "the ground and the header are not counted: {c:?}");
        assert_eq!(c.ignored.iter().map(|p| p.value.as_str()).collect::<Vec<_>>(), ["ADC"]);
        assert_eq!(c.generic.len(), 2, "the LED and the NPN: {c:?}");
        assert_eq!(c.caveats.len(), 1);
        assert_eq!(c.caveats[0].notes, ["ボディダイオードは含みません"]);
        assert_eq!(c.test_sources.len(), 1);
        assert!(!c.is_clean());
    }

    #[test]
    fn a_circuit_of_plain_parts_and_a_modelled_part_is_clean() {
        let mut s = Schematic::default();
        s.add(Kind::Resistor, (0, 0), 0);
        s.add(Kind::Capacitor, (4, 0), 0);
        s.add_custom(part("OK", Some(model(&[]))), "OK", "x", (20, 0), 0);
        let c = model_coverage(&s);
        assert!(c.is_clean() && c.total == 3, "{c:?}");
    }
}
