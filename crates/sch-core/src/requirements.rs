//! The table of requirements (`DesignSpec`) read against the circuit: what it names that the circuit does not have.

use crate::{extract, Schematic};

impl Schematic {
    /// Sentences about the rows of the requirements table that name a part or a net the circuit (all sheets) does not have, or a role
    /// that is not one of `ROLES`. Empty when the table is empty or all right.
    pub fn requirement_problems(&self) -> Vec<String> {
        if self.requirements.is_empty() {
            return vec![];
        }
        let flat = self.flat();
        let parts: Vec<String> = flat.components.iter().filter(|c| !c.kind.is_power_symbol()).map(|c| c.refdes.clone()).collect();
        let nets: Vec<String> = extract(self).into_iter().map(|n| n.name).collect();
        self.requirements.unknown_names(&parts, &nets)
    }
}
