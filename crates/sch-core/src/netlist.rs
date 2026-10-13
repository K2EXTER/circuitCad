use crate::{on_segment, Kind, Label, Pt, Schematic};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The circuit will not work (or not simulate) as drawn.
    Error,
    /// Probably a slip, but the circuit can still be valid.
    Warning,
}

/// One finding of the electrical rule check, written for a beginner: what is wrong, where,
/// and (in `hint`) why it matters and how to fix it.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub message: String,
    pub at: Option<Pt>,
    pub hint: String,
    pub severity: Severity,
}

impl Problem {
    fn new(severity: Severity, message: String, at: Option<Pt>, hint: &str) -> Self {
        Self { message, at, hint: hint.to_string(), severity }
    }
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
    extract_groups(s, None).1.into_iter().map(|(_, n)| n).collect()
}

/// Name of the net that passes through grid point `p` (a pin, a wire, a label), as `extract`
/// would call it. `None` when nothing is connected there or the net has no component pin.
pub fn net_at(s: &Schematic, p: Pt) -> Option<String> {
    let mut probe = s.clone();
    probe.labels.push(Label { pos: p, text: String::new() });
    let skip = probe.labels.len() - 1;
    let (mut cn, groups) = extract_groups(&probe, Some(skip));
    let g = cn.dsu.find(cn.ids[&p]);
    groups.into_iter().find(|(root, _)| *root == g).map(|(_, n)| n.name)
}

/// Nets together with the connectivity group each one belongs to. A label index in `skip` is
/// ignored when naming (it is only a probe).
fn extract_groups(s: &Schematic, skip: Option<usize>) -> (Conn, Vec<(usize, Net)>) {
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
    for (i, l) in s.labels.iter().enumerate() {
        if Some(i) == skip {
            continue;
        }
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
        nets.push((g, Net { name, pins }));
    }
    (cn, nets)
}

/// Electrical rule check.
pub fn erc(s: &Schematic) -> Vec<Problem> {
    use Severity::{Error, Warning};
    let mut out = vec![];
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for c in &s.components {
        if !c.kind.is_power_symbol() {
            *seen.entry(&c.refdes).or_default() += 1;
        }
    }
    let mut dups: Vec<&str> = seen.iter().filter(|(_, n)| **n > 1).map(|(r, _)| *r).collect();
    dups.sort();
    for r in dups {
        let at = s.components.iter().filter(|c| c.refdes == r).map(|c| c.pos).nth(1);
        out.push(Problem::new(
            Error,
            format!("部品番号 {r} が重複しています"),
            at,
            "同じ番号の部品が2つあります。シミュレーションや基板で区別できなくなります。片方を選んで、左の「番号」を別の番号に変えましょう",
        ));
    }

    let nets = extract(s);
    let mut connected: HashMap<(String, String), usize> = HashMap::new();
    for n in &nets {
        for p in &n.pins {
            connected.insert((p.refdes.clone(), p.pin.clone()), n.pins.len());
        }
    }
    let dangling = dangling_wire_ends(s);
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
                    let near = dangling
                        .iter()
                        .filter(|d| (d.0 - at.0).abs() <= 3 && (d.1 - at.1).abs() <= 3)
                        .min_by_key(|d| (d.0 - at.0).abs() + (d.1 - at.1).abs());
                    let (hint, sev) = if let Some(d) = near {
                        (format!("すぐ近く ({}, {}) に、どこにもつながっていない線の端があります。線をこのピン (赤い点) まで延ばしてつなぎましょう。線の端は、ピンかほかの線の上に置いたときだけつながります", d.0, d.1), Error)
                    } else if c.custom.is_some() {
                        ("IC の入力ピンが浮いたままだと、電圧が定まらず動作が不安定になります。使わない入力は GND か電源につなぎましょう。本当に使わない端子 (NC) なら、そのままで構いません".to_string(), Warning)
                    } else {
                        ("この端子には何もつながっていないので、電流が流れません。配線ツールで相手の部品までつなぎましょう。使わない部品なら削除します".to_string(), Error)
                    };
                    out.push(Problem::new(sev, format!("{} ピン{} が未接続です", c.refdes, pin), Some(at), &hint));
                }
            }
        }
    }

    for at in dangling {
        out.push(Problem::new(
            Warning,
            "線の端がどこにもつながっていません".into(),
            Some(at),
            "ピン (赤い点) や別の線の上で終わっていない線です。つなぎ忘れかもしれません。ピンまで延ばすか、不要なら選んで Delete で消しましょう",
        ));
    }

    // a voltage source whose two terminals sit on one net
    for c in s.components.iter().filter(|c| c.kind == Kind::VSource) {
        let pins = c.pins();
        if pins.len() == 2 {
            let net = |pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == c.refdes && p.pin == pin));
            if let (Some(a), Some(b)) = (net(&pins[0].0), net(&pins[1].0))
                && a.name == b.name {
                    out.push(Problem::new(
                        Error,
                        format!("電源 {} の+と−が同じ配線につながっています (短絡)", c.refdes),
                        Some(c.pos),
                        "電源の両端が直接つながると、理論上は無限大の電流が流れ、シミュレーションできません。電源の+と−の間には、必ず抵抗などの負荷を入れましょう",
                    ));
                }
        }
    }

    // two different power names on one net (e.g. GND shorted to VCC)
    let mut cn = connectivity(s);
    let mut group_names: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut group_at: BTreeMap<usize, Pt> = BTreeMap::new();
    for c in s.components.iter().filter(|c| c.kind.is_power_symbol()) {
        let p = c.pins()[0].1;
        let g = cn.dsu.find(cn.ids[&p]);
        let name = if c.kind == Kind::Ground { "GND".to_string() } else { c.value.clone() };
        group_names.entry(g).or_default().push(name);
        group_at.entry(g).or_insert(p);
    }
    for (g, mut v) in group_names {
        v.sort();
        v.dedup();
        if v.len() > 1 {
            out.push(Problem::new(
                Error,
                format!("電源ネットが短絡しています: {}", v.join(" と ")),
                group_at.get(&g).copied(),
                "違う名前の電源 (GND と VCC など) が1本の配線でつながっています。そのまま電源を入れると大電流が流れて危険です。意図せずつながっている線や、重なって置いた記号を探しましょう",
            ));
        }
    }

    // a power symbol only names a net; something must actually drive it
    for c in s.components.iter().filter(|c| c.kind == Kind::Power) {
        let Some(net) = nets.iter().find(|n| n.name == c.value) else { continue };
        let driven = net.pins.iter().any(|p| {
            s.components.iter().find(|x| x.refdes == p.refdes).is_some_and(|x| matches!(x.kind, Kind::VSource | Kind::Header(_) | Kind::Custom))
        });
        if !driven {
            out.push(Problem::new(
                Error,
                format!("電源ネット {} に電圧源(または電源コネクタ)がつながっていません。電源記号は名前を付けるだけで、電圧は出しません", c.value),
                Some(c.pins()[0].1),
                "電源記号 (VCC など) は「この線の名前」を付けるだけの記号です。電圧を出すには、「電源」部品を置いて、その+側を同じ名前の電源記号につなぎましょう",
            ));
        }
    }

    // ---- checks that need the whole netlist
    let comp = |r: &str| s.components.iter().find(|c| c.refdes == r);
    let net_index = |r: &str, pin: &str| nets.iter().position(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin));
    let driven_by_source = |ni: usize| nets[ni].pins.iter().any(|p| comp(&p.refdes).is_some_and(|c| matches!(c.kind, Kind::VSource | Kind::Header(_))));
    // a supply net may also be fed by the output pin of another library part (a regulator)
    let fed_by_output = |ni: usize, me: &str| {
        nets[ni].pins.iter().any(|p| {
            p.refdes != me
                && comp(&p.refdes).and_then(|c| c.custom.as_ref()).and_then(|sym| sym.pins.iter().find(|q| q.number == p.pin)).is_some_and(|q| {
                    let n = q.name.to_ascii_uppercase();
                    n.starts_with("OUT") || n.starts_with("VOUT") || n == "VO"
                })
        })
    };

    // an LED straight across a supply burns out: it needs a resistor in series
    for c in s.components.iter().filter(|c| c.kind == Kind::Led) {
        let (Some(a), Some(k)) = (net_index(&c.refdes, "A"), net_index(&c.refdes, "K")) else { continue };
        let k_is_return = nets[k].name == "GND" || driven_by_source(k);
        if driven_by_source(a) && k_is_return && a != k {
            out.push(Problem::new(
                Warning,
                format!("{} に電流を制限する抵抗がありません", c.refdes),
                Some(c.pos),
                "LED は電圧がわずかに上がるだけで電流が急に増え、壊れてしまいます。電源と LED の間に抵抗 (330Ω〜1kΩ くらい) を直列に入れて、電流を決めましょう",
            ));
        }
    }

    // library parts: supply and ground pins must reach a supply
    for c in s.components.iter().filter(|c| c.kind == Kind::Custom) {
        let Some(sym) = &c.custom else { continue };
        for (pin, at) in c.pins() {
            let Some(name) = sym.pins.iter().find(|p| p.number == pin).map(|p| p.name.to_ascii_uppercase()) else { continue };
            let is_vcc = ["VCC", "VDD", "V+", "VIN"].iter().any(|n| name.starts_with(n));
            let is_gnd = ["GND", "VSS", "V-", "VEE"].iter().any(|n| name == *n);
            let Some(ni) = net_index(&c.refdes, &pin) else { continue };
            if nets[ni].pins.len() < 2 {
                continue; // already reported as an unconnected pin
            }
            if is_vcc && !driven_by_source(ni) && !fed_by_output(ni, &c.refdes) {
                out.push(Problem::new(
                    Warning,
                    format!("{} の電源ピン {} ({}) が電源につながっていません", c.refdes, pin, name),
                    Some(at),
                    "IC は電源ピンに電圧がかからないと動きません。電源の+側 (または VCC などの名前を付けた線) につなぎましょう",
                ));
            }
            if is_gnd && nets[ni].name != "GND" && !driven_by_source(ni) {
                out.push(Problem::new(
                    Warning,
                    format!("{} のグランドピン {} ({}) が GND につながっていません", c.refdes, pin, name),
                    Some(at),
                    "IC のグランドピンは、回路の基準 (GND) につなぎます。つながっていないと電流の戻り道がなく、IC が動きません",
                ));
            }
        }
    }

    // parts that are not connected to GND through any chain of parts
    let mut parent: Vec<usize> = (0..nets.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        r
    }
    for c in s.components.iter().filter(|c| !c.kind.is_power_symbol()) {
        let idx: Vec<usize> = c.pins().iter().filter_map(|(pin, _)| net_index(&c.refdes, pin)).collect();
        for w in idx.windows(2) {
            let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            parent[a] = b;
        }
    }
    if let Some(gnd) = nets.iter().position(|n| n.name == "GND") {
        let gnd_root = find(&mut parent, gnd);
        let mut reported: Vec<usize> = vec![];
        for c in s.components.iter().filter(|c| !c.kind.is_power_symbol()) {
            let Some(ni) = c.pins().iter().find_map(|(pin, _)| net_index(&c.refdes, pin)) else { continue };
            let root = find(&mut parent, ni);
            if root != gnd_root && !reported.contains(&root) {
                reported.push(root);
                out.push(Problem::new(
                    Warning,
                    format!("{} を含む部分が GND とつながっていません", c.refdes),
                    Some(c.pos),
                    "この部分は、どの部品をたどっても GND に届きません。電圧は GND を基準に決まるので、シミュレーションで値が定まらず、失敗することがあります。どこか1か所を GND につなぎましょう",
                ));
            }
        }
    }

    // labels that differ only in upper / lower case are different nets
    let mut first_seen: HashMap<String, &str> = HashMap::new();
    let mut warned: Vec<String> = vec![];
    for l in &s.labels {
        let key = l.text.to_ascii_lowercase();
        match first_seen.get(&key) {
            Some(prev) if *prev != l.text && !warned.contains(&key) => {
                warned.push(key);
                out.push(Problem::new(
                    Warning,
                    format!("ラベル「{}」と「{}」は大文字・小文字が違うので、別の配線になります", prev, l.text),
                    Some(l.pos),
                    "ラベルの名前は大文字・小文字も区別されます。同じ配線にしたいなら、同じ綴りにそろえましょう (別の配線のつもりなら、このままで構いません)",
                ));
            }
            Some(_) => {}
            None => {
                first_seen.insert(key, &l.text);
            }
        }
    }

    // a label named GND is the reference too (the simulator treats a net of that name as 0 V)
    let has_ground = s.components.iter().any(|c| c.kind == Kind::Ground) || s.labels.iter().any(|l| l.text.eq_ignore_ascii_case("GND"));
    if !has_ground && !s.components.is_empty() {
        out.push(Problem::new(
            Error,
            "GNDシンボルがありません".into(),
            None,
            "電圧は「どこから測るか」の基準 (0V) が必要です。GND 記号を、電源の−側など基準にしたい所に置きましょう。これがないとシミュレーションできません",
        ));
    }
    out
}

/// Wire ends that touch nothing: no pin, no other wire (end or middle), no label.
fn dangling_wire_ends(s: &Schematic) -> Vec<Pt> {
    let pins: Vec<Pt> = s.components.iter().flat_map(|c| c.pins().into_iter().map(|p| p.1)).collect();
    let mut out: Vec<Pt> = vec![];
    for (i, w) in s.wires.iter().enumerate() {
        for p in [w.a, w.b] {
            if w.a == w.b || out.contains(&p) {
                continue;
            }
            let touches = pins.contains(&p)
                || s.labels.iter().any(|l| l.pos == p)
                || s.wires.iter().enumerate().any(|(j, o)| j != i && on_segment(p, o.a, o.b));
            if !touches {
                out.push(p);
            }
        }
    }
    out
}
