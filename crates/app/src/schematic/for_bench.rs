//! What the bench tab ("実機", `bench_tab.rs`) reads from the schematic: the DC voltages to expect at the test points, the nets and
//! parts the spec table measures, and the spec table itself (drawn in the tab's step "合否").

use super::*;

/// The resistance of a supply's lead to the breadboard (ohms): a wire and its clip.
const BOARD_WIRE_OHM: f64 = 0.05;
/// A supply giving more than this (amperes) on a breadboard is shorted.
const SHORT_AMPS: f64 = 2.0;

impl SchEditor {
    /// The DC operating point's voltage of every net (ground left out), when a DC result is there and was found for the circuit as it is.
    pub fn bench_voltages(&self) -> Option<Vec<(String, f64)>> {
        // a what-if result (a part broken on purpose) is not what the board should read
        let v = self.dc.as_ref().filter(|v| v.fault.is_none() && v.signature == self.signature())?;
        let r = v.outcome.as_ref().ok()?;
        Some(r.voltages.iter().filter(|(n, _)| n != "0" && !n.contains('.') && !n.contains('#')).cloned().collect())
    }

    /// The circuit as the breadboard joins it (B6: wired differently from the schematic, it runs, reads and burns that way), or why
    /// it cannot be built. The supplies' wires have a little resistance, so that a supply plugged straight across itself is a short
    /// circuit with a large current rather than an equation with no answer.
    pub fn board_circuit(&self, nets: &sch_core::breadboard::BoardNets, temp_c: Option<f64>) -> Result<sim_core::Circuit, String> {
        let wiring: Vec<sim_core::WiringR> = nets.off_pins.iter().map(|(r, p)| sim_core::WiringR { refdes: r.clone(), pin: p.clone(), ohm: BOARD_WIRE_OHM, henry: 0.0, farad: 0.0 }).collect();
        let temp = temp_c.unwrap_or_else(|| self.board_temp());
        let c = sim_core::from_schematic_with_nets(&self.sch, &nets.pins, &wiring).map_err(|e| e.first().map(|e| e.to_string()).unwrap_or_default())?;
        Ok(c.at_temperature(temp))
    }

    /// The temperature the schematic is simulated at (℃): the breadboard's temperature slider starts there.
    pub fn board_temp(&self) -> f64 {
        self.temp_c.trim().parse::<f64>().ok().filter(|t| *t > -273.0 && *t < 1000.0).unwrap_or(sim_core::T_NOM_C)
    }

    /// The DC operating point of the board's circuit: the current through each part and the parts past their ratings.
    pub fn board_dc(&self, nets: &sch_core::breadboard::BoardNets, temp_c: Option<f64>) -> Result<crate::breadboard::BoardDc, String> {
        let c = self.board_circuit(nets, temp_c)?;
        let r = sim_core::dc_op(&c).map_err(|e| e.to_string())?;
        let ratings = self.board_ratings(&self.rated_schematic(), &r);
        Ok((r.elements.iter().map(|e| (e.name.clone(), e.current)).collect(), ratings))
    }

    /// The schematic with the library's ratings on its parts, for `board_ratings` (made once per run, not every frame).
    pub fn rated_schematic(&self) -> Schematic {
        self.with_library_ratings()
    }

    /// The parts past (or near) their ratings at operating point `r` of the board (the DC point, or the running circuit now),
    /// and a supply that is shorted.
    pub fn board_ratings(&self, rated: &Schematic, r: &sim_core::DcResult) -> Vec<sim_core::RatingIssue> {
        let mut ratings = sim_core::check_ratings(rated, r);
        // a supply giving amperes on a breadboard: its + and − are joined (a bench supply would trip, a battery heat up)
        for e in &r.elements {
            let supply = self.sch.components.iter().any(|c| c.refdes == e.name && c.kind == Kind::VSource);
            if supply && e.current.abs() > SHORT_AMPS {
                ratings.push(sim_core::RatingIssue {
                    refdes: e.name.clone(),
                    level: sim_core::RatingLevel::Over,
                    message: format!("電源がショートしています ({} A 流れています)", sim_core::format_value(e.current.abs())),
                    hint: "電源の + と − (GND) が、ジャンパー線か部品の足で直接つながっていないか確かめます。実物なら電源の保護が働くか、電池と線が熱くなります".into(),
                });
            }
        }
        ratings
    }

    /// The switches clicked on the breadboard while it runs: (part, the source in its model that holds the position, push button).
    pub fn live_switches(&self) -> Vec<(String, String, bool)> {
        self.sch.components.iter().filter_map(|c| super::live_logic::switch_of(c).map(|(k, s)| (c.refdes.clone(), s, k == super::live_logic::InputKind::Push))).collect()
    }

    /// A transient run of the circuit as it is now, for the bench tab: a circuit that keeps moving (an oscillator, a pulse source)
    /// does not sit at its DC point, and the board shows what it does while it runs. The last run when it was of this circuit,
    /// else one run with the sheet's times (not kept: the waveform panel is left as it was). None when it cannot run.
    pub fn running_tran(&self) -> Option<std::borrow::Cow<'_, sim_core::TranResult>> {
        if let Some(v) = self.tran.as_ref().filter(|v| v.signature == self.signature()) {
            return v.outcome.as_ref().ok().map(std::borrow::Cow::Borrowed);
        }
        let (tstop, tstep) = self.tran_times()?;
        let c = self.sim_circuit().ok()?;
        sim_core::transient(&c, &sim_core::TranSettings { tstop, tstep, uic: self.tran_uic }).ok().map(std::borrow::Cow::Owned)
    }

    /// The nets that move while the circuit runs (`running_tran`'s steady part is not their DC voltage): (net, mean, min, max) in
    /// volts. A tester shows about the mean, and its reading may wander.
    pub fn running_voltages(&self) -> Vec<(String, f64, f64, f64)> {
        let Some(dc) = self.bench_voltages() else { return vec![] };
        let Some(r) = self.running_tran() else { return vec![] };
        dc.iter()
            .filter_map(|(net, v)| {
                let sig = r.signal(&format!("V({net})"))?;
                let (mean, lo, hi) = sim_core::steady_stats(&r.time, &sig.values)?;
                sim_core::runs_away_from_dc(*v, mean, lo, hi, 0.05).then(|| (net.clone(), mean, lo, hi))
            })
            .collect()
    }

    /// A signal of the last transient run (times, values), with the run's circuit unchanged since.
    pub fn tran_signal(&self, name: &str) -> Option<(Vec<f64>, Vec<f64>)> {
        let r = self.tran.as_ref()?.outcome.as_ref().ok()?;
        let sig = r.signal(name).or_else(|| r.signals.iter().find(|s| s.name.eq_ignore_ascii_case(name)))?;
        Some((r.time.clone(), sig.values.clone()))
    }

    /// The spec table with its "実測" column, as the analysis tab shows it.
    pub fn spec_table_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_spec(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Found by a usability test (2026-11-19): for a 555 blinker the bench tab gave the DC point (the timer stopped half way, LED
    // off) as what a working board draws and reads, so a correct board was "a little high" and its timing capacitor "1.67 V".
    #[test]
    fn a_blinking_555_is_judged_by_its_running_current_and_voltages() {
        let mut e = SchEditor::default();
        e.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/ne555_blinker.schjson"));
        e.tran_tstop = "3".into();
        e.analyze_dc();
        let mut g = crate::power_on_guide::PowerOnGuide::default();
        g.open_for(&e);
        let s = g.result_for_test().unwrap().as_ref().unwrap().supplies[0].clone();
        let run = s.running.unwrap_or_else(|| panic!("the blinker keeps moving: {s:?}"));
        assert!(run.max > run.min && (s.nominal - run.mean).abs() < 1e-12, "{s:?}");
        assert_eq!(s.judge(run.mean), sim_core::PowerOnReading::Normal, "the mean current of a working board is normal: {s:?}");
        let moving = e.running_voltages();
        assert!(!moving.is_empty(), "some nets swing while it blinks");
        for (net, mean, lo, hi) in &moving {
            assert!(lo < mean && mean < hi, "{net}: {lo} {mean} {hi}");
        }
        // the supply itself does not move
        assert!(moving.iter().all(|(n, ..)| !n.eq_ignore_ascii_case("VCC")), "{moving:?}");
    }
}
