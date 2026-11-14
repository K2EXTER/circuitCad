//! The table of the nets: one row per net with its class, pads, track length (in all and per layer), vias, the thinnest and
//! widest track, the current declared for it with the width that needs, the pours it has and the connections still open. What the editor's "ネットの一覧" window and `board nets` show.

use crate::drc::connectivity;
use crate::model::*;
use crate::quality::measure;
use crate::ratsnest::Airwire;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct NetRow {
    pub net: String,
    /// The net class that lists the net (empty: the board's rules).
    pub class: String,
    pub pads: usize,
    /// Track length (mm), in all and per layer name (`Side::label`).
    pub length: f64,
    pub by_layer: BTreeMap<String, f64>,
    /// Length of the shortest tree of straight lines over the pads (0 for one pad).
    pub shortest: f64,
    pub vias: usize,
    /// Thinnest and widest track (mm); 0 with no track.
    pub thinnest: f64,
    pub widest: f64,
    /// The current declared for the net (A, `Board.net_currents`; 0: none) and the track width it needs (mm, rounded up to 0.05 like the
    /// editor's "推奨の線幅"; 0: no current).
    pub current: f64,
    pub needed: f64,
    /// The layers with a pour of the net.
    pub pours: Vec<String>,
    /// Connections not yet made of copper (yellow lines).
    pub open: usize,
}

impl NetRow {
    /// Track length over the shortest tree: 1.0 is as short as it can be.
    pub fn detour(&self) -> Option<f64> {
        (self.shortest > 1e-9 && self.length > 0.0 && self.pours.is_empty()).then(|| self.length / self.shortest)
    }
}

/// How the rows can be sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Class,
    Pads,
    Length,
    Detour,
    Vias,
    Thinnest,
    Current,
    Open,
}

impl SortBy {
    pub fn parse(s: &str) -> Option<SortBy> {
        Some(match s {
            "name" | "net" => SortBy::Name,
            "class" => SortBy::Class,
            "pads" => SortBy::Pads,
            "length" => SortBy::Length,
            "detour" => SortBy::Detour,
            "vias" => SortBy::Vias,
            "width" | "thinnest" => SortBy::Thinnest,
            "current" => SortBy::Current,
            "open" => SortBy::Open,
            _ => return None,
        })
    }
}

/// The rows of every net that has a pad, by name. `airwires`: the open connections (the editor keeps them worked out already;
/// `None` works them out).
pub fn net_table(board: &Board, airwires: Option<&[Airwire]>) -> Vec<NetRow> {
    let (_, quality) = measure(board);
    let conn = connectivity(board);
    let own;
    let wires: &[Airwire] = match airwires {
        Some(w) => w,
        None => {
            own = crate::ratsnest::ratsnest(board);
            &own
        }
    };
    let mut rows: BTreeMap<String, NetRow> = BTreeMap::new();
    for p in board.pads.iter().filter(|p| !p.net.is_empty()) {
        rows.entry(p.net.clone())
            .or_insert_with(|| NetRow {
                net: p.net.clone(),
                class: board.class_of(&p.net).map(|c| c.name.clone()).unwrap_or_default(),
                pads: 0,
                length: 0.0,
                by_layer: BTreeMap::new(),
                shortest: 0.0,
                vias: 0,
                thinnest: 0.0,
                widest: 0.0,
                current: 0.0,
                needed: 0.0,
                pours: vec![],
                open: 0,
            })
            .pads += 1;
    }
    for q in quality {
        if let Some(r) = rows.get_mut(&q.net) {
            r.length = q.length;
            r.by_layer = q.by_layer;
            r.shortest = q.shortest;
            r.vias = q.vias;
        }
    }
    for (i, t) in board.tracks.iter().enumerate() {
        if let Some(r) = conn.track_net(i).and_then(|n| rows.get_mut(n)) {
            r.thinnest = if r.thinnest == 0.0 { t.width } else { r.thinnest.min(t.width) };
            r.widest = r.widest.max(t.width);
        }
    }
    for (net, &amps) in &board.net_currents {
        if let Some(r) = rows.get_mut(net).filter(|_| amps > 0.0) {
            r.current = amps;
            r.needed = crate::current::round_up_width(crate::current::width_for_current(amps, &crate::current::CopperSpec::default()));
        }
    }
    for z in &board.zones {
        if let Some(r) = rows.get_mut(&z.net) {
            let l = z.side.label();
            if !r.pours.contains(&l) {
                r.pours.push(l);
            }
        }
    }
    for w in wires {
        if let Some(r) = rows.get_mut(&w.net) {
            r.open += 1;
        }
    }
    rows.into_values().collect()
}

/// Sort the rows (`descending` for the biggest first); ties stay by name.
pub fn sort_rows(rows: &mut [NetRow], by: SortBy, descending: bool) {
    let key = |r: &NetRow| -> f64 {
        match by {
            SortBy::Pads => r.pads as f64,
            SortBy::Length => r.length,
            SortBy::Detour => r.detour().unwrap_or(0.0),
            SortBy::Vias => r.vias as f64,
            SortBy::Thinnest => r.thinnest,
            SortBy::Current => r.current,
            SortBy::Open => r.open as f64,
            SortBy::Name | SortBy::Class => 0.0,
        }
    };
    rows.sort_by(|a, b| {
        let o = match by {
            SortBy::Name => natural(&a.net).cmp(&natural(&b.net)),
            SortBy::Class => a.class.cmp(&b.class),
            _ => key(a).total_cmp(&key(b)),
        };
        let o = if descending { o.reverse() } else { o };
        o.then_with(|| natural(&a.net).cmp(&natural(&b.net)))
    });
}

fn natural(net: &str) -> (String, u64, String) {
    let split = net.find(|c: char| c.is_ascii_digit()).unwrap_or(net.len());
    let (head, rest) = net.split_at(split);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    (head.to_string(), digits.parse().unwrap_or(0), rest[digits.len()..].to_string())
}

/// The rows as CSV (for a spreadsheet). Layer columns are the layers that carry tracks.
pub fn table_csv(rows: &[NetRow]) -> String {
    let mut layers: Vec<String> = rows.iter().flat_map(|r| r.by_layer.keys().cloned()).collect();
    layers.sort();
    layers.dedup();
    let mut out = String::from("net,class,pads,length_mm");
    for l in &layers {
        out.push_str(&format!(",{l}_mm"));
    }
    out.push_str(",detour,vias,thinnest_mm,widest_mm,current_a,recommended_mm,pours,open\n");
    for r in rows {
        out.push_str(&format!("{},{},{},{:.2}", quote(&r.net), quote(&r.class), r.pads, r.length));
        for l in &layers {
            out.push_str(&format!(",{:.2}", r.by_layer.get(l).copied().unwrap_or(0.0)));
        }
        let detour = r.detour().map_or(String::new(), |d| format!("{d:.2}"));
        // no current: both left empty (not 0, which would read as "0 A, needs nothing")
        let (current, needed) = if r.current > 0.0 { (format!("{:.3}", r.current), format!("{:.2}", r.needed)) } else { (String::new(), String::new()) };
        out.push_str(&format!(",{detour},{},{:.2},{:.2},{current},{needed},{},{}\n", r.vias, r.thinnest, r.widest, quote(&r.pours.join(" ")), r.open));
    }
    out
}

fn quote(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        for (x, y, net) in [(5.0, 5.0, "SIG"), (25.0, 5.0, "SIG"), (5.0, 20.0, "VCC"), (25.0, 20.0, "VCC"), (15.0, 25.0, "N10"), (15.0, 12.0, "N2")] {
            let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
            p.net = net.into();
            b.pads.push(p);
        }
        // SIG is routed with a dog-leg on two widths, VCC is open
        b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 5.0), b: Point::new(15.0, 8.0), width: 0.25 });
        b.tracks.push(Track { side: Side::Top, a: Point::new(15.0, 8.0), b: Point::new(25.0, 5.0), width: 0.5 });
        b.net_classes.push(NetClass { name: "Power".into(), nets: vec!["VCC".into()], ..NetClass::default() });
        b
    }

    #[test]
    fn a_row_per_net_with_its_length_widths_class_and_open_connections() {
        let b = board();
        let rows = net_table(&b, None);
        assert_eq!(rows.iter().map(|r| r.net.as_str()).collect::<Vec<_>>(), ["N10", "N2", "SIG", "VCC"]);
        let sig = &rows[2];
        assert_eq!(sig.pads, 2);
        assert!((sig.length - 2.0 * 109f64.sqrt()).abs() < 1e-6, "{}", sig.length);
        assert_eq!((sig.thinnest, sig.widest, sig.open), (0.25, 0.5, 0));
        assert!(sig.detour().unwrap() > 1.0);
        assert_eq!(sig.by_layer.get("表面").copied().map(|l| l.round()), Some(21.0));
        let vcc = &rows[3];
        assert_eq!((vcc.class.as_str(), vcc.open, vcc.length), ("Power", 1, 0.0));
        assert_eq!(vcc.detour(), None);
    }

    #[test]
    fn rows_sort_by_any_column_and_names_in_natural_order() {
        let b = board();
        let mut rows = net_table(&b, None);
        sort_rows(&mut rows, SortBy::Name, false);
        assert_eq!(rows.iter().map(|r| r.net.as_str()).collect::<Vec<_>>(), ["N2", "N10", "SIG", "VCC"]);
        sort_rows(&mut rows, SortBy::Length, true);
        assert_eq!(rows[0].net, "SIG");
        sort_rows(&mut rows, SortBy::Open, true);
        assert_eq!(rows[0].net, "VCC");
        let csv = table_csv(&rows);
        assert!(csv.starts_with("net,class,pads,length_mm,表面_mm,detour"), "{csv}");
        assert!(csv.contains("VCC,Power,2,0.00,0.00,,0,0.00,0.00,,,,1"), "{csv}");
    }

    /// A declared current is a column of the table and of its CSV, with the width it needs; a net without one leaves both empty.
    #[test]
    fn the_current_and_the_width_it_needs_are_in_the_table() {
        let mut b = board();
        b.net_currents.insert("VCC".into(), 2.0);
        let mut rows = net_table(&b, None);
        let vcc = rows.iter().find(|r| r.net == "VCC").unwrap();
        let need = crate::current::round_up_width(crate::current::width_for_current(2.0, &crate::current::CopperSpec::default()));
        assert_eq!((vcc.current, vcc.needed), (2.0, need));
        assert!(need > 0.5, "2 A needs more than a thin signal track: {need}");
        sort_rows(&mut rows, SortBy::Current, true);
        assert_eq!(rows[0].net, "VCC");
        let csv = table_csv(&rows);
        assert!(csv.lines().next().unwrap().contains("widest_mm,current_a,recommended_mm,pours"), "{csv}");
        assert!(csv.contains(&format!(",2.000,{need:.2},")), "{csv}");
        assert!(csv.lines().any(|l| l.starts_with("SIG,") && l.contains(",,,")), "no current: empty cells\n{csv}");
    }
}
