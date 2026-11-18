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
    pub fn board_circuit(&self, nets: &sch_core::breadboard::BoardNets) -> Result<sim_core::Circuit, String> {
        let wiring: Vec<sim_core::WiringR> = nets.off_pins.iter().map(|(r, p)| sim_core::WiringR { refdes: r.clone(), pin: p.clone(), ohm: BOARD_WIRE_OHM, henry: 0.0, farad: 0.0 }).collect();
        let temp = self.temp_c.trim().parse::<f64>().ok().filter(|t| *t > -273.0 && *t < 1000.0).unwrap_or(sim_core::T_NOM_C);
        let c = sim_core::from_schematic_with_nets(&self.sch, &nets.pins, &wiring).map_err(|e| e.first().map(|e| e.to_string()).unwrap_or_default())?;
        Ok(c.at_temperature(temp))
    }

    /// The DC operating point of the board's circuit: the current through each part and the parts past their ratings.
    pub fn board_dc(&self, nets: &sch_core::breadboard::BoardNets) -> Result<crate::breadboard::BoardDc, String> {
        let c = self.board_circuit(nets)?;
        let r = sim_core::dc_op(&c).map_err(|e| e.to_string())?;
        let mut ratings = sim_core::check_ratings(&self.with_library_ratings(), &r);
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
        Ok((r.elements.iter().map(|e| (e.name.clone(), e.current)).collect(), ratings))
    }

    /// The switches clicked on the breadboard while it runs: (part, the source in its model that holds the position, push button).
    pub fn live_switches(&self) -> Vec<(String, String, bool)> {
        self.sch.components.iter().filter_map(|c| super::live_logic::switch_of(c).map(|(k, s)| (c.refdes.clone(), s, k == super::live_logic::InputKind::Push))).collect()
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
