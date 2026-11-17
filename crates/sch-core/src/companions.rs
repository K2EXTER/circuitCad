//! Placing the parts a library part's data sheet recommends beside it (`CustomSymbol::companions`: a regulator's input and output
//! capacitors): each next to its pin, out from the part's body, wired to the pin, and to a ground symbol or to the other pin.

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
        let pin_at = |n: &str| pins.iter().find(|(m, _)| m == n).map(|(_, at)| *at);
        let mut placed = vec![];
        // how many have gone on each pin, to set the next one further out
        let mut on_pin: Vec<(String, i32)> = vec![];
        for comp in &list {
            let kind = match comp.kind.as_str() {
                "C" => Kind::Capacitor,
                "R" => Kind::Resistor,
                "L" => Kind::Inductor,
                k => return Err(format!("{} の種類「{k}」は C・R・L のどれかです", comp.name)),
            };
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
                let x = p.0 + dx.signum() * (4 + 3 * k);
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
            if comp.to.eq_ignore_ascii_case("GND") {
                let g = self.add(Kind::Ground, (p2.0, p2.1 + 2), 0);
                let gp = self.components[g].pins()[0].1;
                self.wires.push(Wire { a: p2, b: gp });
            } else {
                let other = pin_at(&comp.to).ok_or_else(|| format!("{} のつなぎ先のピン「{}」が部品にありません", comp.name, comp.to))?;
                let bend = (other.0, p2.1);
                self.wires.push(Wire { a: p2, b: bend });
                self.wires.push(Wire { a: bend, b: other });
            }
            self.wires.retain(|w| w.a != w.b);
            placed.push(self.components[i].refdes.clone());
        }
        Ok(placed)
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn regulator() -> CustomSymbol {
        let pin = |n: &str, name: &str, side| SymPin { number: n.into(), name: name.into(), side, sim_only: false, gap: 0, kind: None };
        let c = |name: &str, value: &str, pin: &str| Companion { name: name.into(), kind: "C".into(), value: value.into(), pin: pin.into(), to: "GND".into(), note: String::new() };
        CustomSymbol {
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
}
