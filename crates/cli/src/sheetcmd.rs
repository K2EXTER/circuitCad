//! `sheet`: several sheets in one circuit. Nets join across sheets by the same net label (or power symbol), so a circuit built with
//! `net` (a label on every pin, no wires) can be split by moving parts: their labels go with them.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::json;

/// The box a part occupies on the grid: (min x, max x, min y, max y) over its pins and, for a library part, its body.
fn part_bounds(c: &sch_core::Component) -> (i32, i32, i32, i32) {
    let mut xs: Vec<i32> = c.pins().iter().map(|(_, p)| p.0).collect();
    let mut ys: Vec<i32> = c.pins().iter().map(|(_, p)| p.1).collect();
    if let Some(sym) = &c.custom {
        let l = sym.layout();
        for (cx, cy) in [(-l.hw, -l.hh), (l.hw, l.hh)] {
            let r = sch_core::rotate((cx, cy), c.rot);
            xs.push(c.pos.0 + r.0);
            ys.push(c.pos.1 + r.1);
        }
    }
    (*xs.iter().min().unwrap_or(&c.pos.0), *xs.iter().max().unwrap_or(&c.pos.0), *ys.iter().min().unwrap_or(&c.pos.1), *ys.iter().max().unwrap_or(&c.pos.1))
}

impl Session {
    /// Packs the parts of the active sheet into rows of about the same width as they are tall (a wide screen: 1.7 : 1), in the order
    /// they are in, and moves the net labels on their pins with them. Returns (parts, width, height) of the result.
    fn tidy_active(&mut self) -> Result<(usize, i32, i32), String> {
        if !self.sch.wires.is_empty() {
            return Err("this sheet has wires, which would be torn apart: tidy is for circuits joined by net labels (the 'net' command)".into());
        }
        const GAP: i32 = 8;
        let boxes: Vec<(i32, i32, i32, i32)> = self.sch.components.iter().map(part_bounds).collect();
        if boxes.is_empty() {
            return Ok((0, 0, 0));
        }
        let area: f64 = boxes.iter().map(|b| ((b.1 - b.0 + GAP) * (b.3 - b.2 + GAP)) as f64).sum();
        let widest = boxes.iter().map(|b| b.1 - b.0).max().unwrap_or(0);
        let row_w = ((area * 1.7).sqrt() as i32).max(widest + GAP).max(40);
        let (mut x, mut y, mut rowh, mut max_x) = (8, 8, 0, 0);
        let mut moves = vec![];
        for b in &boxes {
            let (w, h) = (b.1 - b.0, b.3 - b.2);
            if x > 8 && x + w > row_w {
                x = 8;
                y += rowh + GAP;
                rowh = 0;
            }
            moves.push((x - b.0, y - b.2));
            x += w + GAP;
            rowh = rowh.max(h);
            max_x = max_x.max(x - GAP);
        }
        let height = y + rowh;
        let old_pins: Vec<Vec<sch_core::Pt>> = self.sch.components.iter().map(|c| c.pins().into_iter().map(|(_, p)| p).collect()).collect();
        // each label belongs to the first part that has a pin where it sits; found before anything moves, so that a label is never moved twice
        for l in self.sch.labels.iter_mut() {
            if let Some(i) = old_pins.iter().position(|ps| ps.contains(&l.pos)) {
                l.pos = (l.pos.0 + moves[i].0, l.pos.1 + moves[i].1);
            }
        }
        for (c, (dx, dy)) in self.sch.components.iter_mut().zip(&moves) {
            c.pos = (c.pos.0 + dx, c.pos.1 + dy);
        }
        Ok((boxes.len(), max_x, height))
    }

    fn sheet_index(&self, what: &str) -> Result<usize, String> {
        let n = self.sch.sheet_count();
        if let Ok(k) = what.parse::<usize>() {
            return if (1..=n).contains(&k) { Ok(k - 1) } else { Err(format!("no sheet {k} (the circuit has {n}: 1..{n})")) };
        }
        (0..n).find(|&k| self.sch.sheet_name(k).eq_ignore_ascii_case(what)).ok_or_else(|| format!("no sheet called '{what}' (sheets: {})", self.sheet_names().join(", ")))
    }

    fn sheet_names(&self) -> Vec<String> {
        (0..self.sch.sheet_count()).map(|k| format!("{}={}", k + 1, self.sch.sheet_name(k))).collect()
    }

    fn sheet_list(&self) -> Res {
        let rows: Vec<_> = (0..self.sch.sheet_count())
            .map(|k| {
                let active = k == self.sch.active_sheet.min(self.sch.sheet_count() - 1);
                let (parts, labels) = if active || self.sch.sheets.is_empty() {
                    (self.sch.components.len(), self.sch.labels.len())
                } else {
                    (self.sch.sheets[k].components.len(), self.sch.sheets[k].labels.len())
                };
                json!({"n": k + 1, "name": self.sch.sheet_name(k), "parts": parts, "labels": labels, "active": active})
            })
            .collect();
        Ok(obj(json!({"sheets": rows, "hint": "add, use, name and move edit the sheets; add / net / remove / list act on the active sheet"})))
    }

    /// `sheet` (list), `sheet add [NAME]`, `sheet use N|NAME`, `sheet name NAME`, `sheet move REF.. to=N|NAME`.
    pub fn sheet(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let Some(sub) = pos.first().map(|s| s.to_ascii_lowercase()) else { return self.sheet_list() };
        match sub.as_str() {
            "add" => {
                let k = self.sch.add_sheet();
                if let Some(name) = pos.get(1) {
                    self.sch.rename_sheet(k, name);
                }
                self.cursor = self.cursor_below();
                self.sheet_list()
            }
            "use" => {
                let k = self.sheet_index(pos.get(1).ok_or("sheet use N|NAME")?)?;
                self.sch.switch_sheet(k);
                self.cursor = self.cursor_below();
                self.sheet_list()
            }
            "name" => {
                let name = pos.get(1).ok_or("sheet name NAME   (renames the active sheet)")?;
                if self.sch.sheets.is_empty() {
                    return Err("the circuit has one sheet: 'sheet add' first".into());
                }
                self.sch.rename_sheet(self.sch.active_sheet, name);
                self.sheet_list()
            }
            "move" => {
                let to = opt(&kv, "to").ok_or("sheet move REF [REF ...] to=N|NAME")?;
                if pos.len() < 2 {
                    return Err("sheet move REF [REF ...] to=N|NAME".into());
                }
                if self.sch.sheets.len() < 2 {
                    return Err("the circuit has one sheet: 'sheet add' makes the other".into());
                }
                let target = self.sheet_index(to)?;
                let here = self.sch.active_sheet;
                if target == here {
                    return Err("that is the active sheet already".into());
                }
                if !self.sch.wires.is_empty() {
                    return Err("this sheet has wires, which cannot be split between sheets: moving is for circuits joined by net labels (the 'net' command)".into());
                }
                let mut moving = vec![];
                for r in &pos[1..] {
                    let c = self.sch.components.iter().find(|c| c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r} on the active sheet {}", self.sch.sheet_name(here)))?;
                    moving.push(c.refdes.clone());
                }
                let points: Vec<_> = self.sch.components.iter().filter(|c| moving.contains(&c.refdes)).flat_map(|c| c.pins().into_iter().map(|(_, p)| p)).collect();
                let (go, stay): (Vec<_>, Vec<_>) = std::mem::take(&mut self.sch.components).into_iter().partition(|c| moving.contains(&c.refdes));
                self.sch.components = stay;
                let (lgo, lstay): (Vec<_>, Vec<_>) = std::mem::take(&mut self.sch.labels).into_iter().partition(|l| points.contains(&l.pos));
                self.sch.labels = lstay;
                let (n_parts, n_labels) = (go.len(), lgo.len());
                let dest = &mut self.sch.sheets[target];
                dest.components.extend(go);
                dest.labels.extend(lgo);
                let mut out = self.sheet_list()?;
                out.insert("moved".into(), json!({"parts": n_parts, "labels": n_labels, "to": self.sch.sheet_name(target)}));
                Ok(out)
            }
            "tidy" => {
                // the labels that sit on pins are looked up by place, so two parts must never share a pin position: tidy one sheet at a time
                let all = pos.get(1).is_some_and(|s| s.eq_ignore_ascii_case("all"));
                let here = self.sch.active_sheet;
                let mut sizes = vec![];
                let ks: Vec<usize> = if all { (0..self.sch.sheet_count()).collect() } else { vec![here] };
                for k in ks {
                    if all && self.sch.sheet_count() > 1 {
                        self.sch.switch_sheet(k);
                    }
                    let (n, w, h) = self.tidy_active()?;
                    sizes.push(json!({"sheet": self.sch.sheet_name(self.sch.active_sheet), "parts": n, "width": w, "height": h}));
                }
                if all && self.sch.sheet_count() > 1 {
                    self.sch.switch_sheet(here);
                }
                self.cursor = self.cursor_below();
                Ok(obj(json!({"tidied": sizes, "hint": "the parts are in rows about 1.7 times as wide as tall; nets and labels are unchanged"})))
            }
            _ => Err("sheet [add [NAME] | use N|NAME | name NAME | move REF.. to=N|NAME | tidy [all]]".into()),
        }
    }
}
