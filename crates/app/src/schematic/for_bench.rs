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

    /// The spec table with its "実測" column, as the analysis tab shows it.
    pub fn spec_table_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_spec(ui);
    }
}
