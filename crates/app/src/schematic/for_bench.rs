//! What the bench tab ("実機", `bench_tab.rs`) reads from the schematic: the DC voltages to expect at the test points, the nets and
//! parts the spec table measures, and the spec table itself (drawn in the tab's step "合否").

use super::*;

impl SchEditor {
    /// The DC operating point's voltage of every net (ground left out), when a DC result is there and was found for the circuit as it is.
    pub fn bench_voltages(&self) -> Option<Vec<(String, f64)>> {
        // a what-if result (a part broken on purpose) is not what the board should read
        let v = self.dc.as_ref().filter(|v| v.fault.is_none() && v.signature == self.signature())?;
        let r = v.outcome.as_ref().ok()?;
        Some(r.voltages.iter().filter(|(n, _)| n != "0" && !n.contains('.') && !n.contains('#')).cloned().collect())
    }

    /// The DC result for the circuit as it is (not a what-if), when there is one.
    fn fresh_dc(&self) -> Option<&DcView> {
        self.dc.as_ref().filter(|v| v.fault.is_none() && v.signature == self.signature())
    }

    /// The current through each part at the DC operating point (A, by reference designator), for the breadboard's LEDs.
    pub fn dc_part_currents(&self) -> Vec<(String, f64)> {
        self.fresh_dc().and_then(|v| v.outcome.as_ref().ok()).map(|r| r.elements.iter().map(|e| (e.name.clone(), e.current)).collect()).unwrap_or_default()
    }

    /// The parts past (or near) what they stand at the DC operating point, for the breadboard's broken parts.
    pub fn dc_ratings(&self) -> Vec<sim_core::RatingIssue> {
        self.fresh_dc().map(|v| v.ratings.clone()).unwrap_or_default()
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
