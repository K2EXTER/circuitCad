//! Repeated structures: a circuit often has the same few parts connected the same way several times (eight range switches, ten LEDs with
//! their resistors, four identical filter stages). A person lays such copies out the same way, in a regular row or grid. This finds them,
//! for the placement to lay the first one out and repeat it.
//!
//! The parts that count are the small ones: parts with many pads (ICs, headers) tie everything together and are left out, and so are
//! the nets that touch many parts (grounds, supplies, buses). What is left falls apart into small groups of parts joined by their own nets.
//! Two groups are copies of each other when their parts (the kind of designator and the number of pads; the values and footprints may
//! differ) and the way their pins are joined are the same; that is looked for with the label of each part refined twice by what its pins touch.

use crate::model::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Parts with this many pads or more are anchors (ICs, headers): they do not join the groups.
const ANCHOR_PADS: usize = 8;
/// A net that touches more parts than this is a hub (a ground, a supply, a bus): it does not join the groups.
const HUB_NET: usize = 6;
/// A group has at least this many parts, and at most.
const MIN_PARTS: usize = 2;
const MAX_PARTS: usize = 12;

/// A part as the placement knows it: refdes, and the size of its cell.
pub(crate) type Item = (String, Point, f64, f64);

/// Groups of copies. Each group is a list of copies, and each copy lists its parts (indexes into `items`) in the same role order, so
/// that part `k` of one copy stands where part `k` of the first copy stands. A group has at least two copies; every copy lies on one
/// schematic sheet (`sheet`).
pub(crate) fn find(board: &Board, items: &[Item], poured: &[String], sheet: &dyn Fn(usize) -> String) -> Vec<Vec<Vec<usize>>> {
    let index: HashMap<&str, usize> = items.iter().enumerate().map(|(i, it)| (it.0.as_str(), i)).collect();
    let mut pads = vec![0usize; items.len()];
    // net -> the pins on it (item, pin)
    let mut nets: BTreeMap<&str, Vec<(usize, &str)>> = BTreeMap::new();
    for p in board.pads.iter().filter(|p| !p.refdes.is_empty()) {
        let Some(&i) = index.get(p.refdes.as_str()) else { continue };
        pads[i] += 1;
        if !p.net.is_empty() && !poured.contains(&p.net) {
            nets.entry(p.net.as_str()).or_default().push((i, p.pin.as_str()));
        }
    }
    let anchor: Vec<bool> = pads.iter().map(|&n| n >= ANCHOR_PADS).collect();
    // the nets that join parts into groups: not a hub, and at least two small parts
    let private: BTreeMap<&str, &Vec<(usize, &str)>> = nets
        .iter()
        .filter(|(_, pins)| {
            let parts: BTreeSet<usize> = pins.iter().map(|p| p.0).collect();
            parts.len() <= HUB_NET && parts.iter().filter(|i| !anchor[**i]).count() >= 2
        })
        .map(|(n, p)| (*n, p))
        .collect();
    // groups: the small parts joined by private nets
    let mut parent: Vec<usize> = (0..items.len()).collect();
    fn find_root(parent: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while parent[r] != r {
            r = parent[r];
        }
        let mut c = i;
        while parent[c] != r {
            let next = parent[c];
            parent[c] = r;
            c = next;
        }
        r
    }
    for pins in private.values() {
        // (parts of different sheets are not joined: a copy lies on one sheet)
        let small: Vec<usize> = pins.iter().map(|p| p.0).filter(|i| !anchor[*i]).collect();
        for (k, &i) in small.iter().enumerate() {
            let Some(&first) = small[..k].iter().find(|&&j| sheet(j) == sheet(i)) else { continue };
            let (a, b) = (find_root(&mut parent, i), find_root(&mut parent, first));
            if a != b {
                parent[a] = b;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..items.len() {
        if !anchor[i] && pads[i] > 0 {
            let r = find_root(&mut parent, i);
            groups.entry(r).or_default().push(i);
        }
    }
    let groups: Vec<Vec<usize>> = groups.into_values().filter(|g| (MIN_PARTS..=MAX_PARTS).contains(&g.len())).collect();
    let in_group: HashMap<usize, usize> = groups.iter().enumerate().flat_map(|(k, g)| g.iter().map(move |i| (*i, k))).collect();

    // labels: what a part is, then what it is joined to (twice). What a part is: the kind of designator and the number of pads. The
    // value and the footprint are not part of it: the stages of a range switch are the same circuit with other resistors and capacitors.
    let base = |i: usize| -> String { format!("{}|{}", natural(&items[i].0).0, pads[i]) };
    let mut label: Vec<String> = (0..items.len()).map(base).collect();
    for _ in 0..2 {
        let mut next = label.clone();
        for g in &groups {
            for &i in g {
                // what each pin of the part touches: the labels and pins of the other small parts of the net, whether an anchor is on it
                let mut pins: Vec<String> = vec![];
                for p in board.pads.iter().filter(|p| p.refdes == items[i].0) {
                    let touch = match private.get(p.net.as_str()) {
                        Some(list) => {
                            let mut others: Vec<String> = list.iter().filter(|(j, pin)| !(*j == i && *pin == p.pin) && !anchor[*j]).map(|(j, pin)| format!("{}:{pin}", label[*j])).collect();
                            others.sort();
                            let anchored = list.iter().any(|(j, _)| anchor[*j]);
                            format!("[{}{}]", others.join(","), if anchored { "+A" } else { "" })
                        }
                        None if p.net.is_empty() => "free".to_string(),
                        None => "hub".to_string(),
                    };
                    pins.push(format!("{}={touch}", p.pin));
                }
                pins.sort();
                next[i] = format!("{}<{}>", label[i], pins.join(";"));
            }
        }
        label = next;
    }
    // copies: groups with the same set of labels (and on one sheet)
    let mut by_signature: BTreeMap<String, Vec<Vec<usize>>> = BTreeMap::new();
    for g in &groups {
        let sheets: BTreeSet<String> = g.iter().map(|&i| sheet(i)).collect();
        if sheets.len() != 1 {
            continue;
        }
        let mut members = g.clone();
        // the role order: by label, then by the number in the designator (the same parts of different copies come out in the same place)
        members.sort_by(|&a, &b| label[a].cmp(&label[b]).then(natural(&items[a].0).cmp(&natural(&items[b].0))));
        let sig = format!("{}#{}", members.iter().map(|&i| label[i].as_str()).collect::<Vec<_>>().join("/"), sheets.iter().next().unwrap());
        by_signature.entry(sig).or_default().push(members);
    }
    let _ = in_group;
    by_signature.into_values().filter(|copies| copies.len() >= 2).collect()
}

/// A designator as (letters, number): "R12" sorts after "R9".
pub(crate) fn natural(r: &str) -> (String, u64) {
    let letters: String = r.chars().take_while(|c| !c.is_ascii_digit()).collect();
    let number = r[letters.len()..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
    (letters, number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netlist::*;

    fn part(r: &str, f: &str) -> NetlistPart {
        NetlistPart { refdes: r.into(), value: "1k".into(), footprint: f.into() }
    }

    fn net(n: &str, pins: &[(&str, &str)]) -> NetlistNet {
        NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() }
    }

    /// Four copies of "a resistor and a capacitor in series between IN<k> and OUT<k>", and a ground and a supply that touch all of them.
    fn channels(n: usize, supply: bool) -> Board {
        let mut b = Board::new("t", 10.0, 10.0);
        let mut parts = vec![];
        let mut nets: Vec<NetlistNet> = vec![];
        let (mut gnd, mut vcc) = (vec![], vec![]);
        for k in 1..=n {
            parts.push(part(&format!("R{k}"), "R_0805"));
            parts.push(part(&format!("C{k}"), "C_0805"));
            nets.push(net(&format!("MID{k}"), &[(&format!("R{k}"), "2"), (&format!("C{k}"), "1")]));
            nets.push(net(&format!("IN{k}"), &[(&format!("R{k}"), "1")]));
            gnd.push((format!("C{k}"), "2".to_string()));
            vcc.push((format!("R{k}"), "1".to_string()));
        }
        nets.push(NetlistNet { name: "GND".into(), pins: gnd });
        // the supply joins the first pad of every resistor: a hub when it touches many parts
        if supply {
            nets.push(NetlistNet { name: "VCC".into(), pins: vcc });
        }
        nets.retain(|n| !n.name.starts_with("IN"));
        import(&mut b, &parts, &nets);
        b
    }

    fn items_of(b: &Board) -> Vec<Item> {
        b.parts.iter().map(|p| (p.refdes.clone(), Point::new(0.0, 0.0), 2.0, 1.0)).collect()
    }

    #[test]
    fn identical_stages_are_found_as_copies_in_the_same_role_order() {
        let b = channels(8, true);
        let items = items_of(&b);
        let found = find(&b, &items, &["GND".to_string()], &|_| "s".to_string());
        assert_eq!(found.len(), 1, "{found:?}");
        let copies = &found[0];
        assert_eq!(copies.len(), 8, "the supply touches eight parts: a hub, it does not join the stages");
        // each copy is its resistor and its capacitor, the same role in the same place
        let role0: Vec<&str> = copies.iter().map(|c| items[c[0]].0.as_str()).collect();
        let role1: Vec<&str> = copies.iter().map(|c| items[c[1]].0.as_str()).collect();
        assert!(role0.iter().all(|r| r.starts_with(role0[0].chars().next().unwrap())), "{role0:?}");
        assert!(role1.iter().all(|r| r.starts_with(role1[0].chars().next().unwrap())), "{role1:?}");
        assert_ne!(role0[0].chars().next(), role1[0].chars().next());
    }

    #[test]
    fn stages_that_differ_are_not_copies() {
        let mut b = channels(3, false);
        // another footprint (and value) does not make a stage another circuit
        b.parts.iter_mut().find(|p| p.refdes == "C3").unwrap().footprint = "C_1206".into();
        b.parts.iter_mut().find(|p| p.refdes == "C3").unwrap().value = "47n".into();
        let items = items_of(&b);
        assert_eq!(find(&b, &items, &["GND".to_string()], &|_| "s".to_string()).iter().map(|g| g.len()).collect::<Vec<_>>(), vec![3]);
        // a third part on the stage's net does
        b.set_part(Part { refdes: "R9".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(0.0, 0.0), rot: 0, side: Side::Top }).unwrap();
        b.pads.iter_mut().find(|p| p.refdes == "R9" && p.pin == "1").unwrap().net = "MID3".into();
        let items = items_of(&b);
        let found = find(&b, &items, &["GND".to_string()], &|_| "s".to_string());
        assert_eq!(found.iter().map(|g| g.len()).collect::<Vec<_>>(), vec![2], "only stages 1 and 2: {found:?}");
        // copies on two sheets are not copies of one another
        let b = channels(4, false);
        let items = items_of(&b);
        let found = find(&b, &items, &["GND".to_string()], &|i| if items[i].0.ends_with('1') || items[i].0.ends_with('2') { "a".to_string() } else { "b".to_string() });
        assert_eq!(found.iter().map(|g| g.len()).collect::<Vec<_>>(), vec![2, 2], "{found:?}");
    }

    #[test]
    fn a_single_stage_or_unconnected_parts_make_no_group() {
        let b = channels(1, false);
        let items = items_of(&b);
        assert!(find(&b, &items, &["GND".to_string()], &|_| "s".to_string()).is_empty());
    }

    #[test]
    fn designators_sort_by_their_number() {
        assert!(natural("R9") < natural("R12"));
        assert!(natural("C3") < natural("R1"));
    }

    /// What is found on the microohm meter board (a file outside the repository): printed, for looking at.
    #[test]
    #[ignore]
    fn print_the_copies_of_a_real_board() {
        let Ok(text) = std::fs::read_to_string("../../microohm-meter/mmBase.pcbjson") else { return };
        let b: Board = serde_json::from_str(&text).unwrap();
        let items: Vec<Item> = b
            .parts
            .iter()
            .map(|p| {
                let (lo, hi) = crate::autoplace::extent(&b, &p.refdes).unwrap_or((Point::default(), Point::default()));
                (p.refdes.clone(), lo, hi.x - lo.x + 1.2, hi.y - lo.y + 1.2)
            })
            .collect();
        let poured: Vec<String> = b.zones.iter().map(|z| z.net.clone()).collect();
        let sheet = |i: usize| b.part_sheets.get(&items[i].0).cloned().unwrap_or_default();
        let found = find(&b, &items, &poured, &sheet);
        for g in &found {
            eprintln!("{} copies of {} parts: {:?}", g.len(), g[0].len(), g.iter().map(|c| c.iter().map(|&i| items[i].0.as_str()).collect::<Vec<_>>().join("+")).collect::<Vec<_>>());
        }
    }
}
