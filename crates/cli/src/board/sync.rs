//! Board commands that go between the schematic and the board: new, import, swap, backannotate, renumber.

use super::*;

impl Session {
    pub(super) fn board_new(&mut self, rest: &[String]) -> Res {
        let size = match rest {
            [] => DEFAULT_SIZE,
            [w, h] => (num(w)?, num(h)?),
            _ => return Err("board new [WIDTH HEIGHT]  (mm)".into()),
        };
        self.board = Some(Board::new("board", size.0, size.1));
        Ok(obj(json!({"width": size.0, "height": size.1})))
    }

    pub(super) fn board_import(&mut self, _rest: &[String]) -> Res {
        let gate = self.erc_gate()?;
        let circuit_ok = gate.is_none();
        let (parts, nets) = self.netlist_of();
        if parts.is_empty() {
            return Err("the schematic has no parts to import".into());
        }
        let board = self.board.get_or_insert_with(|| Board::new("board", DEFAULT_SIZE.0, DEFAULT_SIZE.1));
        // library parts bring their own footprint; the footprints whose land is only roughly right are worked out again from the
        // library each time (a part whose land was checked since loses its mark)
        board.land_unverified.clear();
        let mut swaps: Vec<(String, pcb_core::swap::SwapSpec)> = vec![];
        for c in self.sch.flat().components.iter().filter(|c| c.custom.is_some()) {
            if let Some(spec) = c.custom.as_ref().and_then(|sym| part_lib::swap_for(&sym.name, &self.lib)) {
                swaps.push((c.refdes.clone(), spec));
            }
            if let Some(def) = self.lib.iter().find(|d| d.footprint_name() == c.footprint)
                && let Ok(fp) = def.footprint()
            {
                board.add_footprint(fp);
                if let Some(m) = &def.model3d {
                    board.models3d.insert(def.footprint_name(), m.clone());
                }
                if def.land_unverified {
                    board.land_unverified.insert(def.footprint_name(), def.name.clone());
                }
            }
        }
        let rep = netlist::import(board, &parts, &nets);
        netlist::import_shops(board, &shops_of(&self.sch));
        netlist::import_swaps(board, &swaps);
        let sheets: Vec<(String, String)> = self.sch.flat().components.iter().filter(|c| !c.kind.is_power_symbol() && !c.sim_only).map(|c| (c.refdes.clone(), self.sch.sheet_name(sch_core::sheet_of(c.pos).0))).collect();
        netlist::import_sheets(board, &sheets);
        let (_, _, req) = netlist::import_hints_report(board, &netlist::NetHints { currents: self.sch.net_currents.clone().into_iter().collect(), pairs: self.sch.diff_pairs.clone(), requirements: self.sch.requirements.clone() });
        Ok(obj(json!({"circuit_ok": circuit_ok, "added": rep.added, "removed": rep.removed, "updated": rep.updated, "warnings": rep.warnings.iter().cloned().chain(gate).chain(req.warnings()).collect::<Vec<_>>()})))
    }

    pub(super) fn board_swap(&mut self, rest: &[String]) -> Res {
        // board swap [dry=1]: trade the nets of alike gates and alike inputs of logic ICs so that the airwires get shorter
        // and cross less (only gates with no copper on their pads). The trade is kept in the board, so a new import keeps it.
        let (_, kv) = options(rest);
        let dry = opt(&kv, "dry") == Some("1");
        let board = self.board_mut()?;
        if board.swap_specs.is_empty() {
            return Err("no part with alike gates here: `board import` first (library parts such as 74HC32 carry the rule)".into());
        }
        let r = pcb_core::swap::run(board, dry);
        let plans: Vec<_> = r
            .plans
            .iter()
            .map(|p| json!({"part": p.refdes, "moves": p.moves, "length_mm": [p.length.0, p.length.1], "crossings": [p.crossings.0, p.crossings.1]}))
            .collect();
        Ok(obj(json!({"dry": dry, "swapped": plans, "locked": r.locked})))
    }

    pub(super) fn board_backannotate(&mut self, _rest: &[String]) -> Res {
        // board backannotate: the pin trades of `board swap` (kept in the board) go into the schematic, so that the drawing says what
        // the board says. On an error nothing is changed.
        let trades = self.board_mut()?.pin_map.clone();
        if trades.is_empty() {
            return Err("no pin trades on the board (`board swap` makes them)".into());
        }
        let rep = self.sch.apply_pin_trades(&trades)?;
        let board = self.board_mut()?;
        for r in &rep.parts {
            board.pin_map.remove(r);
        }
        Ok(obj(json!({"parts": rep.parts, "moved": rep.moved, "copies": rep.copies, "left": board.pin_map.keys().collect::<Vec<_>>()})))
    }

    pub(super) fn board_renumber(&mut self, rest: &[String]) -> Res {
        // board renumber [dry=1]: number the parts again by where they sit (the top row first, each row from the left; locked parts keep
        // theirs). The schematic, its table of requirements and the board change together. `dry=1` only answers what would change.
        let (_, kv) = options(rest);
        let plan = pcb_core::renumber::renumber_plan(self.board_mut()?);
        if opt(&kv, "dry") != Some("1") && !plan.is_empty() {
            self.sch.rename_parts(&plan)?;
            self.board_mut()?.rename_parts(&plan);
        }
        Ok(obj(json!({"renamed": plan.len(), "plan": plan, "dry": opt(&kv, "dry") == Some("1")})))
    }
}
