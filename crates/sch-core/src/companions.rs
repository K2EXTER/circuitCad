//! Placing the parts a library part's data sheet recommends beside it (`CustomSymbol::companions`: a regulator's input and output
//! capacitors): each next to its pin, out from the part's body, wired to the pin, and to a ground symbol or to the other pin.
//! An end may also be a net that is no pin of the part (`net:VOUT`: a buck's output, after its coil): it gets a label of that
//! name, and a part hanging from such a net stands in a column right of the part's body.

use crate::{Kind, Pt, Schematic, Wire};

impl Schematic {
    /// Places the recommended parts of the component `idx`; the reference names of the parts placed, or why none could be.
    pub fn place_companions(&mut self, idx: usize) -> Result<Vec<String>, String> {
        let c = self.components.get(idx).ok_or("部品がありません")?;
        let Some(sym) = &c.custom else { return Err("ライブラリ部品ではありません".into()) };
        if sym.companions.is_empty() {
            return Err(format!("{} には推奨の周辺部品が入っていません (部品作成の「推奨の周辺部品」で入れられます)", c.refdes));
        }
        let (center, pins, list) = (c.pos, c.pins(), sym.companions.clone());
        // a pin's net by name, for a part whose far end goes to another pin: "U1_SW"
        let pin_net = |no: &str| format!("{}_{}", c.refdes, sym.pins.iter().find(|p| p.number == no).map_or(no, |p| p.name.as_str()).replace(['/', ' '], "_"));
        let names: Vec<(String, String)> = list.iter().filter(|x| !x.to.eq_ignore_ascii_case("GND") && !x.to.starts_with("net:")).map(|x| (x.to.clone(), pin_net(&x.to))).collect();
        let pin_at = |n: &str| pins.iter().find(|(m, _)| m == n).map(|(_, at)| *at);
        // a column of its own for each part on a pin of the left or the right side: the lowest pin's parts nearest the body, so
        // that the wire from a pin above runs over them (parts of neighbouring pins in one column touched: a buck's coil on SW and
        // the capacitor on BST)
        let side = |p: Pt| {
            let (dx, dy) = (p.0 - center.0, p.1 - center.1);
            (dx.abs() >= dy.abs()).then_some(dx.signum())
        };
        let mut column = vec![0i32; list.len()];
        for sign in [-1, 1] {
            let mut on: Vec<(usize, i32)> = list.iter().enumerate().filter_map(|(k, c)| pin_at(&c.pin).filter(|p| side(*p) == Some(sign)).map(|p| (k, p.1))).collect();
            on.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            for (rank, (k, _)) in on.iter().enumerate() {
                column[*k] = rank as i32;
            }
        }
        let n_right = list.iter().filter(|c| pin_at(&c.pin).is_some_and(|p| side(p) == Some(1))).count() as i32;
        // the column for parts hanging from a net: right of the right side's columns
        let right = pins.iter().map(|(_, at)| at.0).max().unwrap_or(center.0) + 4 + 3 * n_right + 4;
        let mut on_net = 0;
        let mut placed = vec![];
        // how many have gone on each pin, to set the next one further out
        let mut on_pin: Vec<(String, i32)> = vec![];
        for (n, comp) in list.iter().enumerate() {
            let kind = match comp.kind.as_str() {
                "C" => Kind::Capacitor,
                "R" => Kind::Resistor,
                "L" => Kind::Inductor,
                k => return Err(format!("{} の種類「{k}」は C・R・L のどれかです", comp.name)),
            };
            if let Some(net) = comp.pin.strip_prefix("net:") {
                // standing in the column right of the part: a label of the net on its top end
                let x = right + 4 * on_net;
                on_net += 1;
                let i = self.add(kind, (x, center.1 + 2), 1);
                self.components[i].value = comp.value.clone();
                let ends = self.components[i].pins();
                let (p1, p2) = (ends[0].1, ends[1].1);
                let top = (p1.0, p1.1 - 2);
                self.wires.push(Wire { a: p1, b: top });
                self.labels.push(crate::Label { pos: top, text: net.to_string() });
                self.end_of(comp, p2, &pin_at, &names)?;
                placed.push(self.components[i].refdes.clone());
                continue;
            }
            let p = pin_at(&comp.pin).ok_or_else(|| format!("{} のピン「{}」が部品にありません", comp.name, comp.pin))?;
            let k = match on_pin.iter_mut().find(|(n, _)| *n == comp.pin) {
                Some((_, n)) => {
                    *n += 1;
                    *n
                }
                None => {
                    on_pin.push((comp.pin.clone(), 0));
                    0
                }
            };
            // out from the body: the pin's side
            let (dx, dy) = (p.0 - center.0, p.1 - center.1);
            let horizontal = dx.abs() >= dy.abs();
            // the point the part hangs from, level with the pin (or a little out from a top or bottom pin), and the part standing
            // below it: its pin 1 there, pin 2 four below
            let (q, top): (Pt, Pt) = if horizontal {
                let x = p.0 + dx.signum() * (4 + 3 * column[n]);
                (p, (x, p.1))
            } else {
                let q = (p.0, p.1 + dy.signum() * 2);
                (q, (p.0 + 4 + 3 * k, q.1))
            };
            let i = self.add(kind, (top.0, top.1 + 2), 1);
            self.components[i].value = comp.value.clone();
            let ends = self.components[i].pins();
            let (p1, p2) = (ends[0].1, ends[1].1);
            self.wires.push(Wire { a: p, b: q });
            self.wires.push(Wire { a: q, b: top });
            if top != p1 {
                self.wires.push(Wire { a: top, b: p1 });
            }
            self.end_of(comp, p2, &pin_at, &names)?;
            placed.push(self.components[i].refdes.clone());
        }
        Ok(placed)
    }

    /// The far end `p2` of a placed part: to a ground symbol, a label of a net (`net:VOUT`), or the other pin.
    fn end_of(&mut self, comp: &crate::Companion, p2: Pt, pin_at: &dyn Fn(&str) -> Option<Pt>, names: &[(String, String)]) -> Result<(), String> {
        if comp.to.eq_ignore_ascii_case("GND") {
            let g = self.add(Kind::Ground, (p2.0, p2.1 + 2), 0);
            let gp = self.components[g].pins()[0].1;
            self.wires.push(Wire { a: p2, b: gp });
        } else if let Some(net) = comp.to.strip_prefix("net:") {
            let end = (p2.0, p2.1 + 2);
            self.wires.push(Wire { a: p2, b: end });
            self.labels.push(crate::Label { pos: end, text: net.to_string() });
        } else {
            // the other pin by a label on it and one on this end (wires back to it ran along the other parts' ends and joined them)
            let other = pin_at(&comp.to).ok_or_else(|| format!("{} のつなぎ先のピン「{}」が部品にありません", comp.name, comp.to))?;
            let net = names.iter().find(|(no, _)| *no == comp.to).map_or_else(|| comp.to.clone(), |(_, n)| n.clone());
            if !self.labels.iter().any(|l| l.pos == other && l.text == net) {
                self.labels.push(crate::Label { pos: other, text: net.clone() });
            }
            let end = (p2.0, p2.1 + 2);
            self.wires.push(Wire { a: p2, b: end });
            self.labels.push(crate::Label { pos: end, text: net });
        }
        self.wires.retain(|w| w.a != w.b);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn regulator() -> CustomSymbol {
        let pin = |n: &str, name: &str, side| SymPin { unit: 0, number: n.into(), name: name.into(), side, sim_only: false, gap: 0, kind: None };
        let c = |name: &str, value: &str, pin: &str| Companion { name: name.into(), kind: "C".into(), value: value.into(), pin: pin.into(), to: "GND".into(), note: String::new() };
        CustomSymbol {
            unit: 0,
            name: "REG".into(),
            prefix: "U".into(),
            pins: vec![pin("1", "VIN", PinSide::Left), pin("2", "GND", PinSide::Bottom), pin("3", "VOUT", PinSide::Right)],
            sim: None,
            ratings: None,
            glyph: String::new(),
            shops: Default::default(),
            programming: vec![],
            companions: vec![c("CIN", "330n", "1"), c("COUT", "100n", "3"), c("COUT2", "10u", "3")],
        }
    }

    #[test]
    fn the_recommended_capacitors_go_between_their_pins_and_ground() {
        let mut s = Schematic::default();
        let u = s.add_custom(regulator(), "REG", "SOT-223", (20, 20), 0);
        let placed = s.place_companions(u).unwrap();
        assert_eq!(placed, ["C1", "C2", "C3"]);
        let nets = extract(&s);
        let net_of = |r: &str, p: &str| nets.iter().find(|n| n.pins.iter().any(|q| q.refdes == r && q.pin == p)).map(|n| n.name.clone());
        // each capacitor between its pin and GND, the two on VOUT side by side
        assert_eq!(net_of("C1", "1"), net_of("U1", "1"));
        assert_eq!(net_of("C2", "1"), net_of("U1", "3"));
        assert_eq!(net_of("C3", "1"), net_of("U1", "3"));
        for c in ["C1", "C2", "C3"] {
            assert_eq!(net_of(c, "2").as_deref(), Some("GND"), "{c}");
        }
        assert_ne!(net_of("U1", "1"), net_of("U1", "3"), "input and output stay apart");
        let value = |r: &str| s.components.iter().find(|c| c.refdes == r).unwrap().value.clone();
        assert_eq!((value("C1"), value("C3")), ("330n".to_string(), "10u".to_string()));
        // a part with none says so
        let mut bare = regulator();
        bare.companions.clear();
        let v = s.add_custom(bare, "REG", "SOT-223", (60, 20), 0);
        assert!(s.place_companions(v).unwrap_err().contains("推奨の周辺部品"));
    }

    /// A buck's parts (候補 Q5): the bootstrap capacitor between two pins, the coil from SW to the output net (no pin of the part),
    /// the output capacitor and the top feedback resistor on that net, the bottom one from FB to GND.
    #[test]
    fn a_bucks_coil_and_output_parts_hang_on_the_output_net() {
        let pin = |n: &str, name: &str, side| SymPin { unit: 0, number: n.into(), name: name.into(), side, sim_only: false, gap: 0, kind: None };
        let part = |name: &str, kind: &str, value: &str, pin: &str, to: &str| Companion { name: name.into(), kind: kind.into(), value: value.into(), pin: pin.into(), to: to.into(), note: String::new() };
        let mut sym = regulator();
        sym.pins = vec![pin("3", "VIN", PinSide::Left), pin("4", "GND", PinSide::Bottom), pin("5", "SW", PinSide::Right), pin("6", "BST", PinSide::Right), pin("8", "FB", PinSide::Right)];
        sym.companions = vec![
            part("CIN", "C", "10u", "3", "GND"),
            part("CBST", "C", "100n", "6", "5"),
            part("L1", "L", "6.8u", "5", "net:VOUT"),
            part("COUT", "C", "44u", "net:VOUT", "GND"),
            part("RFBT", "R", "52.5k", "net:VOUT", "8"),
            part("RFBB", "R", "10k", "8", "GND"),
        ];
        let mut s = Schematic::default();
        let u = s.add_custom(sym, "TPS62933", "SOT583", (20, 20), 0);
        assert_eq!(s.place_companions(u).unwrap().len(), 6);
        let nets = extract(&s);
        let net_of = |r: &str, p: &str| nets.iter().find(|n| n.pins.iter().any(|q| q.refdes == r && q.pin == p)).map(|n| n.name.clone());
        assert_eq!(net_of("L1", "1"), net_of("U1", "5"), "the coil on SW");
        assert_eq!(net_of("L1", "2").as_deref(), Some("VOUT"));
        assert_eq!(net_of("C2", "1"), net_of("U1", "6"), "CBST from BST");
        assert_eq!(net_of("C2", "2"), net_of("U1", "5"), "to SW");
        assert_eq!(net_of("C3", "1").as_deref(), Some("VOUT"));
        assert_eq!(net_of("C3", "2").as_deref(), Some("GND"));
        assert_eq!(net_of("R1", "1").as_deref(), Some("VOUT"));
        assert_eq!(net_of("R1", "2"), net_of("U1", "8"), "RFBT to FB");
        assert_eq!(net_of("R2", "1"), net_of("U1", "8"));
        assert_eq!(net_of("R2", "2").as_deref(), Some("GND"));
        assert_ne!(net_of("U1", "5"), Some("VOUT".into()), "SW and the output stay apart");
    }

    /// An adjustable regulator's divider (候補 Q5): R1 from OUT to ADJ, R2 from ADJ to GND; the nets join only where they should.
    #[test]
    fn a_divider_between_two_pins_and_ground() {
        let mut sym = regulator();
        sym.pins[1].name = "ADJ".into();
        let r = |name: &str, value: &str, pin: &str, to: &str| Companion { name: name.into(), kind: "R".into(), value: value.into(), pin: pin.into(), to: to.into(), note: String::new() };
        sym.companions = vec![r("R1", "240", "3", "2"), r("R2", "750", "2", "GND")];
        let mut s = Schematic::default();
        let u = s.add_custom(sym, "LM317", "SOT-223", (20, 20), 0);
        assert_eq!(s.place_companions(u).unwrap(), ["R1", "R2"]);
        let nets = extract(&s);
        let net_of = |r: &str, p: &str| nets.iter().find(|n| n.pins.iter().any(|q| q.refdes == r && q.pin == p)).map(|n| n.name.clone());
        assert_eq!(net_of("R1", "1"), net_of("U1", "3"));
        assert_eq!(net_of("R1", "2"), net_of("U1", "2"));
        assert_eq!(net_of("R2", "1"), net_of("U1", "2"));
        assert_eq!(net_of("R2", "2").as_deref(), Some("GND"));
        assert_ne!(net_of("U1", "2"), net_of("U1", "3"), "ADJ and OUT stay apart");
        assert_ne!(net_of("U1", "2"), Some("GND".into()), "ADJ is not grounded");
    }
}
