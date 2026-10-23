//! Wires stay attached to a part that moves or turns: a wire end sitting on one of its pins follows the pin.

use super::*;

/// What a drag needs to re-stretch the wires every frame: the wires as they were when the drag began,
/// and the pins of the moved parts (part, pin name, position then).
pub(super) struct StretchBase {
    wires: Vec<Wire>,
    pins: Vec<(usize, String, Pt)>,
}

/// `base` with every wire end that sits on an old pin position (first of a `moved` pair) put on the new one.
/// A wire that would turn diagonal gets one bend, so it stays horizontal/vertical: the bend segment is
/// appended at the end (the wire keeps its index). Wires in `keep` are left as they are.
pub(super) fn stretch_wires(base: &[Wire], moved: &[(Pt, Pt)], keep: &[usize]) -> Vec<Wire> {
    let to = |p: Pt| moved.iter().find(|m| m.0 == p && m.0 != m.1).map(|m| m.1);
    let mut out = base.to_vec();
    for (i, w) in base.iter().enumerate() {
        if keep.contains(&i) {
            continue;
        }
        let (na, nb) = (to(w.a), to(w.b));
        if na.is_none() && nb.is_none() {
            continue;
        }
        let horizontal = w.a.1 == w.b.1;
        let vertical = w.a.0 == w.b.0;
        let (a, b) = (na.unwrap_or(w.a), nb.unwrap_or(w.b));
        let mut path = vec![a];
        if a.0 != b.0 && a.1 != b.1 && (horizontal || vertical) {
            path.push(match (na.is_some(), nb.is_some()) {
                // only the end `a` moved: keep the far end's line, bend next to the moved end
                (true, false) => if horizontal { (a.0, b.1) } else { (b.0, a.1) },
                (false, true) => if horizontal { (b.0, a.1) } else { (a.0, b.1) },
                // both moved: leave `a` along the wire's own direction first
                _ => if horizontal { (b.0, a.1) } else { (a.0, b.1) },
            });
        }
        path.push(b);
        let mut segs = path.windows(2).filter(|s| s[0] != s[1]).map(|s| Wire { a: s[0], b: s[1] });
        out[i] = segs.next().unwrap_or(Wire { a, b });
        out.extend(segs);
    }
    out
}

impl SchEditor {
    /// Remembers the wires and the pins of parts `comps` before they are moved.
    pub(super) fn begin_stretch(&mut self, comps: &[usize]) {
        let pins = comps
            .iter()
            .flat_map(|&i| self.sch.components[i].pins().into_iter().map(move |(n, p)| (i, n, p)))
            .collect();
        self.stretch = Some(StretchBase { wires: self.sch.wires.clone(), pins });
    }

    /// Puts the wires where they belong for the parts' current places (all computed from the start of the drag,
    /// so dragging back leaves no trace). Wires in `keep` are moved by the caller.
    pub(super) fn apply_stretch(&mut self, keep: &[usize]) {
        let Some(base) = &self.stretch else { return };
        let moved: Vec<(Pt, Pt)> = base
            .pins
            .iter()
            .filter_map(|(i, n, old)| self.sch.components.get(*i)?.pins().into_iter().find(|p| &p.0 == n).map(|p| (*old, p.1)))
            .collect();
        let mut out = stretch_wires(&base.wires, &moved, keep);
        for &k in keep {
            if let (Some(w), Some(slot)) = (self.sch.wires.get(k), out.get_mut(k)) {
                *slot = w.clone();
            }
        }
        self.sch.wires = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(a: Pt, b: Pt) -> Wire {
        Wire { a, b }
    }

    fn pin(e: &SchEditor, i: usize, k: usize) -> Pt {
        e.sch.components[i].pins()[k].1
    }

    #[test]
    fn wires_follow_a_turning_part_and_stay_connected() {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        e.sch.add(Kind::Resistor, (20, 20), 0);
        let p = pin(&e, 0, 1);
        e.sch.wires.push(Wire { a: p, b: (p.0 + 6, p.1) });
        e.rotate_comp(0);
        let p2 = pin(&e, 0, 1);
        assert!(e.sch.wires.iter().any(|w| w.a == p2 || w.b == p2), "a wire end sits on the moved pin");
        assert!(e.sch.wires.iter().all(|w| w.a.0 == w.b.0 || w.a.1 == w.b.1), "no diagonal wires");
        assert!(e.sch.wires.iter().any(|w| w.a == (p.0 + 6, p.1) || w.b == (p.0 + 6, p.1)), "the far end did not move");
    }

    #[test]
    fn dragging_a_part_there_and_back_restores_the_wires() {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        e.sch.add(Kind::Resistor, (20, 20), 0);
        let p = pin(&e, 0, 1);
        e.sch.wires.push(Wire { a: p, b: (p.0 + 6, p.1) });
        let before = e.sch.wires.clone();
        e.begin_stretch(&[0]);
        e.sch.components[0].pos = (22, 25);
        e.apply_stretch(&[]);
        assert!(e.sch.wires.len() > 1 && e.sch.wires.iter().any(|w| w.a == pin(&e, 0, 1) || w.b == pin(&e, 0, 1)));
        e.sch.components[0].pos = (20, 20);
        e.apply_stretch(&[]);
        assert_eq!(e.sch.wires, before);
    }

    #[test]
    fn straight_move_just_stretches() {
        let out = stretch_wires(&[w((0, 0), (10, 0))], &[((10, 0), (13, 0))], &[]);
        assert_eq!(out, vec![w((0, 0), (13, 0))]);
    }

    #[test]
    fn sideways_move_of_a_horizontal_wire_adds_a_bend_at_the_pin() {
        let out = stretch_wires(&[w((0, 0), (10, 0))], &[((10, 0), (10, 4))], &[]);
        assert_eq!(out, vec![w((0, 0), (10, 0)), w((10, 0), (10, 4))]);
    }

    #[test]
    fn moving_the_start_keeps_the_far_end_in_place() {
        let out = stretch_wires(&[w((0, 0), (10, 0))], &[((0, 0), (2, 3))], &[]);
        assert!(out.contains(&w((2, 3), (2, 0))) && out.contains(&w((2, 0), (10, 0))));
    }

    #[test]
    fn dragging_back_restores_the_wire() {
        let base = [w((0, 0), (10, 0))];
        assert_eq!(stretch_wires(&base, &[((10, 0), (10, 0))], &[]), base.to_vec());
    }

    #[test]
    fn kept_wires_and_wires_off_the_pins_are_untouched() {
        let base = [w((0, 0), (10, 0)), w((10, 0), (10, 5)), w((20, 20), (30, 20))];
        let out = stretch_wires(&base, &[((10, 0), (12, 0))], &[1]);
        assert_eq!(out[1], base[1]);
        assert_eq!(out[2], base[2]);
        assert_eq!(out[0], w((0, 0), (12, 0)));
    }
}
