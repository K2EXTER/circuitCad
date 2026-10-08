use crate::{on_segment, Kind, Pt, Schematic};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinRef {
    pub refdes: String,
    pub pin: String,
}

#[derive(Clone, Debug)]
pub struct Net {
    pub name: String,
    pub pins: Vec<PinRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub message: String,
    pub at: Option<Pt>,
}

struct Dsu(Vec<usize>);

impl Dsu {
    fn find(&mut self, i: usize) -> usize {
        let mut r = i;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut c = i;
        while self.0[c] != r {
            let n = self.0[c];
            self.0[c] = r;
            c = n;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        self.0[a] = b;
    }
}

/// Connectivity result shared by `extract` and `erc`.
struct Conn {
    /// point -> node index
    ids: HashMap<Pt, usize>,
    dsu: Dsu,
}

fn connectivity(s: &Schematic) -> Conn {
    let mut ids: HashMap<Pt, usize> = HashMap::new();
    let mut add = |p: Pt| {
        let n = ids.len();
        *ids.entry(p).or_insert(n)
    };
    for c in &s.components {
        for (_, p) in c.pins() {
            add(p);
        }
    }
    for w in &s.wires {
        add(w.a);
        add(w.b);
    }
    for l in &s.labels {
        add(l.pos);
    }
    let mut dsu = Dsu((0..ids.len()).collect());
    for w in &s.wires {
        let (a, b) = (ids[&w.a], ids[&w.b]);
        dsu.union(a, b);
        // any known point lying on the wire (pin, endpoint of another wire, label) joins it
        for (&p, &id) in &ids {
            if on_segment(p, w.a, w.b) {
                dsu.union(id, a);
            }
        }
    }
    Conn { ids, dsu }
}

/// Extract the netlist. Power symbols and labels name nets; same names merge.
/// Unnamed nets are called `N$1`, `N$2`, ... Nets without any component pin are omitted.
pub fn extract(s: &Schematic) -> Vec<Net> {
    let mut cn = connectivity(s);

    // names attached to a connectivity group
    let mut names: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for c in &s.components {
        if c.kind.is_power_symbol() {
            let p = c.pins()[0].1;
            let g = cn.dsu.find(cn.ids[&p]);
            let name = if c.kind == Kind::Ground { "GND".to_string() } else { c.value.clone() };
            names.entry(g).or_default().push(name);
        }
    }
    for l in &s.labels {
        let g = cn.dsu.find(cn.ids[&l.pos]);
        names.entry(g).or_default().push(l.text.clone());
    }
    // groups sharing a name are one net
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let groups: Vec<usize> = names.keys().copied().collect();
    for g in groups {
        for n in names[&g].clone() {
            match by_name.get(&n) {
                Some(&other) => cn.dsu.union(g, other),
                None => {
                    by_name.insert(n, g);
                }
            }
        }
    }

    let mut pins_by_group: BTreeMap<usize, Vec<PinRef>> = BTreeMap::new();
    for c in &s.components {
        if c.kind.is_power_symbol() {
            continue;
        }
        for (pin, p) in c.pins() {
            let g = cn.dsu.find(cn.ids[&p]);
            pins_by_group.entry(g).or_default().push(PinRef { refdes: c.refdes.clone(), pin });
        }
    }

    let mut name_of: HashMap<usize, String> = HashMap::new();
    for (n, g) in &by_name {
        let root = cn.dsu.find(*g);
        let e = name_of.entry(root).or_insert_with(|| n.clone());
        // deterministic and tidy: prefer GND, otherwise the alphabetically first
        if n == "GND" || (e != "GND" && n < e) {
            *e = n.clone();
        }
    }

    let mut auto = 0;
    let mut nets = vec![];
    for (g, pins) in pins_by_group {
        let name = name_of.get(&g).cloned().unwrap_or_else(|| {
            auto += 1;
            format!("N${auto}")
        });
        nets.push(Net { name, pins });
    }
    nets
}

/// Electrical rule check.
pub fn erc(s: &Schematic) -> Vec<Problem> {
    let mut out = vec![];
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for c in &s.components {
        if !c.kind.is_power_symbol() {
            *seen.entry(&c.refdes).or_default() += 1;
        }
    }
    for (r, n) in seen {
        if n > 1 {
            out.push(Problem { message: format!("部品番号 {r} が重複しています"), at: None });
        }
    }

    let nets = extract(s);
    let mut connected: HashMap<(String, String), usize> = HashMap::new();
    for n in &nets {
        for p in &n.pins {
            connected.insert((p.refdes.clone(), p.pin.clone()), n.pins.len());
        }
    }
    for c in &s.components {
        if c.kind.is_power_symbol() {
            continue;
        }
        for (pin, at) in c.pins() {
            let nc = c
                .custom
                .as_ref()
                .and_then(|s| s.pins.iter().find(|p| p.number == pin))
                .is_some_and(|p| p.name.eq_ignore_ascii_case("NC"));
            if nc {
                continue;
            }
            let alone = connected.get(&(c.refdes.clone(), pin.clone())).is_none_or(|&n| n < 2);
            if alone {
                // a pin alone on a net is only fine when it is explicitly named (e.g. a header pin on a labelled net)
                let named = nets.iter().any(|n| {
                    !n.name.starts_with("N$") && n.pins.iter().any(|p| p.refdes == c.refdes && p.pin == pin)
                });
                if !named {
                    out.push(Problem { message: format!("{} ピン{} が未接続です", c.refdes, pin), at: Some(at) });
                }
            }
        }
    }

    // two different power names on one net (e.g. GND shorted to VCC)
    let mut cn = connectivity(s);
    let mut group_names: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for c in s.components.iter().filter(|c| c.kind.is_power_symbol()) {
        let p = c.pins()[0].1;
        let g = cn.dsu.find(cn.ids[&p]);
        let name = if c.kind == Kind::Ground { "GND".to_string() } else { c.value.clone() };
        group_names.entry(g).or_default().push(name);
    }
    for (_, mut v) in group_names {
        v.sort();
        v.dedup();
        if v.len() > 1 {
            out.push(Problem { message: format!("電源ネットが短絡しています: {}", v.join(" と ")), at: None });
        }
    }

    if !s.components.iter().any(|c| c.kind == Kind::Ground) && !s.components.is_empty() {
        out.push(Problem { message: "GNDシンボルがありません".into(), at: None });
    }
    out
}
