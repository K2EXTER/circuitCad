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

    /// Which IC each bypass capacitor serves that the table does not place yet (`design_spec::suggest_bypass`), from the circuit of all
    /// sheets: the nets of each part's pins, the pin count and the sheet it is drawn on.
    pub fn bypass_suggestions(&self) -> Vec<design_spec::Suggestion> {
        let flat = self.flat();
        let mut views: std::collections::BTreeMap<String, design_spec::PartView> = Default::default();
        for c in flat.components.iter().filter(|c| !c.kind.is_power_symbol() && !c.sim_only) {
            views.insert(c.refdes.clone(), design_spec::PartView { refdes: c.refdes.clone(), sheet: self.sheet_name(crate::sheet_of(c.pos).0), pin_count: c.pins().len(), nets: vec![] });
        }
        for net in extract(self) {
            for pin in &net.pins {
                if let Some(v) = views.get_mut(&pin.refdes) {
                    if !v.nets.contains(&net.name) {
                        v.nets.push(net.name.clone());
                    }
                }
            }
        }
        let parts: Vec<design_spec::PartView> = views.into_values().collect();
        design_spec::suggest_bypass(&self.requirements, &parts)
    }
}

