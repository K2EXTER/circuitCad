//! The board's heat in the schematic's heat runs (candidate H7, "回路図に反映"): once the board is carried over, the parts that give
//! off heat are put on the board's temperature grid (`pcb_core::thermal::thermal_matrix`), and the matrix it gives replaces their θ
//! (its diagonal) and adds how they warm each other (the rest of it). The matrix is kept until the board, the chosen parts or their θ
//! change.

use super::*;

/// The board carried over for the heat, and the last matrix worked out on it.
pub(crate) struct BoardHeat {
    pub board: pcb_core::model::Board,
    /// (key of what it was worked out for, the parts, the matrix in K/W).
    cache: Option<(u64, Vec<String>, Vec<Vec<f64>>)>,
}

impl BoardHeat {
    pub fn new(board: pcb_core::model::Board) -> Self {
        Self { board, cache: None }
    }
}

/// The parts on the board's matrix, and the matrix (`r[i][j]`: K of part i per W in part j).
pub(crate) type BoardMatrix = (Vec<String>, Vec<Vec<f64>>);

impl SchEditor {
    /// The board's matrix for these heaters (`None` without a board carried over, or no part worth it).
    pub(super) fn board_matrix(&mut self, c: &sim_core::Circuit, heaters: &[sim_core::Heater], ambient: f64) -> Result<Option<BoardMatrix>, String> {
        let Some(bh) = &self.board_heat else { return Ok(None) };
        let on_board = |n: &str| bh.board.pads.iter().any(|p| p.refdes == n);
        let parts = sim_core::board_heaters(&self.sch, c, heaters, on_board).map_err(|e| e.to_string())?;
        if parts.is_empty() {
            return Ok(None);
        }
        // the key: the board's copper, which parts, their θ, and the air (the powers only set h, so they are left out)
        use std::hash::{Hash, Hasher};
        let mut hs = std::collections::hash_map::DefaultHasher::new();
        pcb_core::zonefill::geometry_key(&bh.board).hash(&mut hs);
        for p in &parts {
            p.name.hash(&mut hs);
            for x in [p.own_theta, p.theta_jc, p.heatsink] {
                x.to_bits().hash(&mut hs);
            }
        }
        ambient.round().to_bits().hash(&mut hs);
        let key = hs.finish();
        if let Some((k, names, r)) = &bh.cache
            && *k == key {
                return Ok(Some((names.clone(), r.clone())));
            }
        let sources: Vec<pcb_core::thermal::HeatSource> = parts.iter().map(|p| pcb_core::thermal::HeatSource::new(&p.name, p.power, p.own_theta, p.theta_jc, p.heatsink)).collect();
        let spec = pcb_core::thermal::ThermalSpec { ambient_c: ambient, ..Default::default() };
        let (r, _) = pcb_core::thermal::thermal_matrix(&bh.board, &sources, bh.board.stackup.copper_um, &spec)?;
        let names: Vec<String> = parts.into_iter().map(|p| p.name).collect();
        if let Some(bh) = &mut self.board_heat {
            bh.cache = Some((key, names.clone(), r.clone()));
        }
        Ok(Some((names, r)))
    }

    /// The heaters with the board's θ, and the board's couplings between them (a, b, K of a per W in b), for `self_heat_coupled`.
    pub(super) fn heaters_on_board(&mut self, c: &sim_core::Circuit, mut heaters: Vec<sim_core::Heater>, ambient: f64) -> Result<(Vec<sim_core::Heater>, Vec<(String, String, f64)>), String> {
        let Some((names, r)) = self.board_matrix(c, &heaters, ambient)? else {
            self.board_heat_parts.clear();
            return Ok((heaters, vec![]));
        };
        let mutual = sim_core::heaters_with_matrix(&mut heaters, &names, &r);
        self.board_heat_parts = names;
        Ok((heaters, mutual))
    }

    /// For the temperature over time: the board's matrix as each part's θ to the air and the links between the parts run
    /// (`sim_core::nodes_with_matrix`).
    pub(super) fn tran_nodes_on_board(&mut self, c: &sim_core::Circuit, nodes: &mut [sim_core::ThermalNode], links: Vec<sim_core::ThermalLink>, ambient: f64) -> Result<Vec<sim_core::ThermalLink>, String> {
        let heaters = sim_core::heaters_from_schematic(&self.sch, c, &self.copper_g());
        let Some((names, r)) = self.board_matrix(c, &heaters, ambient)? else {
            self.board_heat_parts.clear();
            return Ok(links);
        };
        let links = sim_core::nodes_with_matrix(nodes, links, &names, &r);
        self.board_heat_parts = names;
        Ok(links)
    }
}
