//! `sheet`: several sheets in one circuit. Nets join across sheets by the same net label (or power symbol), so a circuit built with
//! `net` (a label on every pin, no wires) can be split by moving parts: their labels go with them.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::json;

impl Session {
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
                self.cursor = (8, 8, 0);
                self.sheet_list()
            }
            "use" => {
                let k = self.sheet_index(pos.get(1).ok_or("sheet use N|NAME")?)?;
                self.sch.switch_sheet(k);
                self.cursor = (8, 8, 0);
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
            _ => Err("sheet [add [NAME] | use N|NAME | name NAME | move REF.. to=N|NAME]".into()),
        }
    }
}
