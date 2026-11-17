//! What the first power-on guide (`power_on_guide.rs`) needs from the schematic: the circuit as the analyses see it, and the parts
//! that warm themselves (with the board copper carried over from the board, when there is one).

use super::SchEditor;

impl SchEditor {
    /// The circuit and its self-heating parts; an error says what to fix in the schematic.
    pub fn power_on_inputs(&self) -> Result<(sim_core::Circuit, Vec<sim_core::Heater>), String> {
        let c = self.sim_circuit().map_err(|e| e.into_iter().next().unwrap_or_default())?;
        let heaters = sim_core::heaters_from_schematic(&self.sch, &c, &self.copper_g());
        Ok((c, heaters))
    }

    /// Changes whenever the circuit does (to tell that an answer is from an older circuit).
    pub fn circuit_signature(&self) -> String {
        self.signature()
    }
}
