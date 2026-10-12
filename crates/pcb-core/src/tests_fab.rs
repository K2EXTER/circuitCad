//! Structural validation of the fabrication output. No external Gerber viewer is available in
//! CI, so this is a strict RS-274X / Excellon reader that rejects anything a CAM tool would
//! choke on: bad header order, undefined apertures, unbalanced regions, wild coordinates.

use crate::*;
use std::collections::HashSet;

struct Stats {
    flashes: usize,
}

type BBox = (f64, f64, f64, f64);

/// Parses a Gerber file produced by `gerber::*`; `bbox` is (x0, y0, x1, y1) in mm.
fn check_gerber(name: &str, g: &str, bbox: BBox) -> Result<Stats, String> {
    let err = |ln: usize, m: &str| Err(format!("{name}:{ln}: {m}"));
    let (mut fs, mut mo) = (false, false);
    let mut defined: HashSet<u32> = HashSet::new();
    let mut current: Option<u32> = None;
    let (mut in_region, mut ended, mut pen_set) = (false, false, false);
    let mut st = Stats { flashes: 0 };
    for (ln, l) in g.lines().enumerate().map(|(i, l)| (i + 1, l)) {
        if ended {
            return err(ln, "content after M02");
        }
        if let Some(rest) = l.strip_prefix('%') {
            let Some(body) = rest.strip_suffix("*%") else { return err(ln, "unterminated extended command") };
            if body.contains('*') || body.contains('%') {
                return err(ln, "stray delimiter in extended command");
            }
            if body == "FSLAX46Y46" {
                fs = true;
            } else if body == "MOMM" {
                mo = true;
            } else if let Some(a) = body.strip_prefix("ADD") {
                if !(fs && mo) {
                    return err(ln, "aperture defined before %FS / %MO");
                }
                let digits = a.chars().take_while(|c| c.is_ascii_digit()).count();
                let Ok(code) = a[..digits].parse::<u32>() else { return err(ln, "bad D-code") };
                if code < 10 || !defined.insert(code) {
                    return err(ln, "D-code < 10 or defined twice");
                }
                let def = &a[digits..];
                let Some((shape, dims)) = def.split_once(',') else { return err(ln, "missing comma") };
                let n = match shape {
                    "C" => 1,
                    "R" | "O" => 2,
                    _ => return err(ln, "unknown aperture shape"),
                };
                let vals: Vec<f64> = dims.split('X').map(|v| v.parse().unwrap_or(-1.0)).collect();
                if vals.len() != n || vals.iter().any(|v| *v <= 0.0) {
                    return err(ln, &format!("aperture needs {n} positive size(s): {def}"));
                }
            } else if body.starts_with("LP") {
                if body != "LPD" && body != "LPC" {
                    return err(ln, "bad polarity");
                }
            } else if !body.starts_with("TF.") {
                return err(ln, &format!("unknown extended command {body}"));
            }
            continue;
        }
        let Some(cmd) = l.strip_suffix('*') else { return err(ln, "command not terminated by *") };
        match cmd {
            "G01" => {}
            "G36" if !in_region => in_region = true,
            "G37" if in_region => in_region = false,
            "G36" | "G37" => return err(ln, "unbalanced region"),
            "M02" => {
                if in_region {
                    return err(ln, "M02 inside region");
                }
                ended = true;
            }
            c if c.starts_with("G04") => {}
            c if c.starts_with('D') => {
                let Ok(code) = c[1..].parse::<u32>() else { return err(ln, "bad aperture select") };
                if in_region {
                    return err(ln, "aperture change inside region");
                }
                if !defined.contains(&code) {
                    return err(ln, &format!("D{code} selected but not defined"));
                }
                current = Some(code);
            }
            c if c.starts_with('X') => {
                let Some((x, rest)) = c[1..].split_once('Y') else { return err(ln, "missing Y") };
                let Some(dpos) = rest.rfind('D') else { return err(ln, "missing D op") };
                let (y, op) = rest.split_at(dpos);
                let (Ok(x), Ok(y)) = (x.parse::<i64>(), y.parse::<i64>()) else { return err(ln, "bad coordinate") };
                let (xm, ym) = (x as f64 / 1e6, y as f64 / 1e6);
                if xm < bbox.0 - 1.0 || xm > bbox.2 + 1.0 || ym < bbox.1 - 1.0 || ym > bbox.3 + 1.0 {
                    return err(ln, &format!("coordinate ({xm}, {ym}) outside the board"));
                }
                match op {
                    "D02" => pen_set = true,
                    "D01" if !pen_set => return err(ln, "draw without a starting move"),
                    "D01" if !in_region && current.is_none() => return err(ln, "draw without aperture"),
                    "D01" => {}
                    "D03" => {
                        if current.is_none() || in_region {
                            return err(ln, "flash without aperture");
                        }
                        st.flashes += 1;
                    }
                    _ => return err(ln, "unknown operation"),
                }
            }
            _ => return err(ln, &format!("unrecognised command {cmd}")),
        }
    }
    if !(fs && mo) {
        return Err(format!("{name}: missing %FS or %MO"));
    }
    if !ended || in_region {
        return Err(format!("{name}: missing M02 or open region"));
    }
    Ok(st)
}

/// Returns the hole count. Hits must be inside the board and use a declared tool.
fn check_excellon(name: &str, d: &str, bbox: BBox) -> Result<usize, String> {
    let mut lines = d.lines();
    if lines.next() != Some("M48") {
        return Err(format!("{name}: must start with M48"));
    }
    let mut tools: HashSet<u32> = HashSet::new();
    let mut in_header = true;
    let (mut cur, mut hits, mut ended) = (None, 0, false);
    for l in lines {
        if ended {
            return Err(format!("{name}: content after M30"));
        }
        if in_header {
            if l == "%" {
                in_header = false;
            } else if let Some(t) = l.strip_prefix('T') {
                let Some((n, dia)) = t.split_once('C') else { return Err(format!("{name}: bad tool def {l}")) };
                let (Ok(n), Ok(dia)) = (n.parse::<u32>(), dia.parse::<f64>()) else {
                    return Err(format!("{name}: bad tool def {l}"));
                };
                if dia <= 0.0 || !tools.insert(n) {
                    return Err(format!("{name}: bad/duplicate tool {l}"));
                }
            } else if !(l.starts_with(';') || l == "FMAT,2" || l == "METRIC") {
                return Err(format!("{name}: unknown header line {l}"));
            }
        } else if l == "G90" || l == "G05" {
        } else if l == "M30" {
            ended = true;
        } else if let Some(t) = l.strip_prefix('T') {
            let Ok(n) = t.parse::<u32>() else { return Err(format!("{name}: bad tool select {l}")) };
            if n != 0 && !tools.contains(&n) {
                return Err(format!("{name}: T{n} not declared"));
            }
            cur = Some(n);
        } else if let Some(c) = l.strip_prefix('X') {
            let Some((x, y)) = c.split_once('Y') else { return Err(format!("{name}: missing Y")) };
            let (Ok(x), Ok(y)) = (x.parse::<f64>(), y.parse::<f64>()) else { return Err(format!("{name}: bad coordinate {l}")) };
            if matches!(cur, None | Some(0)) {
                return Err(format!("{name}: hit with no tool"));
            }
            if x < bbox.0 || x > bbox.2 || y < bbox.1 || y > bbox.3 {
                return Err(format!("{name}: hole ({x}, {y}) outside the board"));
            }
            hits += 1;
        } else {
            return Err(format!("{name}: unrecognised line {l}"));
        }
    }
    if !ended || in_header {
        return Err(format!("{name}: missing M30 or %"));
    }
    Ok(hits)
}

fn bbox(b: &Board) -> BBox {
    let xs = b.outline.iter().map(|p| p.x);
    let ys = b.outline.iter().map(|p| p.y);
    (
        xs.clone().fold(f64::MAX, f64::min),
        ys.clone().fold(f64::MAX, f64::min),
        xs.fold(f64::MIN, f64::max),
        ys.fold(f64::MIN, f64::max),
    )
}

/// Validates every file in the package and the hole counts against the board.
fn validate(b: &Board) {
    let bb = bbox(b);
    let files = export::fab_files(b);
    assert_eq!(files.len(), 9);
    let mut flashes = 0;
    for (name, data) in &files {
        if name.ends_with(".DRL") {
            let hits = check_excellon(name, data, bb).unwrap_or_else(|e| panic!("{e}"));
            let expect = if name.contains("NPTH") {
                b.holes.len()
            } else {
                b.pads.iter().filter(|p| p.drill.is_some()).count() + b.vias.len()
            };
            assert_eq!(hits, expect, "{name}: hole count");
        } else {
            flashes += check_gerber(name, data, bb).unwrap_or_else(|e| panic!("{e}")).flashes;
        }
    }
    assert!(flashes > 0 || b.pads.is_empty());
}

fn rich_board() -> Board {
    let mut b = Board::new("rich", 50.0, 40.0);
    let mut th = Pad::new(Point::new(10.0, 10.0), PadShape::Circle { d: 1.8 }, Some(0.9), Side::Top);
    th.net = "GND".into();
    b.pads.push(th);
    let mut th2 = Pad::new(Point::new(40.0, 30.0), PadShape::Oval { w: 2.2, h: 1.4 }, Some(0.8), Side::Top);
    th2.net = "GND".into();
    b.pads.push(th2);
    b.pads.push(Pad::new(Point::new(20.0, 20.0), PadShape::Rect { w: 1.0, h: 1.5 }, None, Side::Top));
    b.pads.push(Pad::new(Point::new(30.0, 20.0), PadShape::Rect { w: 1.0, h: 1.5 }, None, Side::Bottom));
    for (a, c, side) in [((20.0, 20.0), (30.0, 20.0), Side::Top), ((30.0, 20.0), (30.0, 30.0), Side::Bottom)] {
        b.tracks.push(Track { side, a: Point::new(a.0, a.1), b: Point::new(c.0, c.1), width: 0.25 });
    }
    b.vias.push(Via { pos: Point::new(30.0, 25.0), diameter: 0.6, drill: 0.3 });
    b.holes.push(Hole::new(Point::new(3.0, 3.0), 3.2));
    b.holes.push(Hole::new(Point::new(47.0, 37.0), 3.2));
    b.silk.push(SilkLine { side: Side::Top, a: Point::new(5.0, 5.0), b: Point::new(15.0, 5.0), width: 0.15, refdes: String::new() });
    for side in [Side::Top, Side::Bottom] {
        b.zones.push(Zone::new(
            "GND",
            side,
            vec![Point::new(2.0, 2.0), Point::new(48.0, 2.0), Point::new(48.0, 38.0), Point::new(2.0, 38.0)],
        ));
    }
    b
}

#[test]
fn a_board_with_every_feature_produces_well_formed_fab_files() {
    validate(&rich_board());
}

#[test]
fn an_empty_board_produces_well_formed_fab_files() {
    validate(&Board::new("empty", 20.0, 20.0));
}

#[test]
fn the_shipped_example_boards_produce_well_formed_fab_files() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "pcbjson") {
            let b: Board = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            validate(&b);
            n += 1;
        }
    }
    assert!(n >= 1);
}

#[test]
fn the_validator_rejects_broken_files() {
    let bb = (0.0, 0.0, 10.0, 10.0);
    let good = gerber::outline(&Board::new("x", 10.0, 10.0));
    assert!(check_gerber("good", &good, bb).is_ok());
    assert!(check_gerber("no-end", &good.replace("M02*\n", ""), bb).is_err());
    assert!(check_gerber("undefined-ap", &good.replace("D10*", "D11*"), bb).is_err());
    assert!(check_gerber("outside", &good.replace("X10000000", "X90000000"), bb).is_err());
    assert!(check_gerber("region", &good.replace("G01*\n", "G01*\nG36*\n"), bb).is_err());
    let drl = excellon::plated(&rich_board());
    assert!(check_excellon("drl", &drl, (0.0, 0.0, 50.0, 40.0)).is_ok());
    assert!(check_excellon("drl-small", &drl, (0.0, 0.0, 5.0, 5.0)).is_err());
    assert!(check_excellon("no-end", &drl.replace("M30\n", ""), (0.0, 0.0, 50.0, 40.0)).is_err());
}

/// A note that is about several places marks all of them (the first is `at`, the rest are in `also`).
#[test]
fn a_note_about_several_places_lists_them_all() {
    let mut b = Board::new("t", 40.0, 30.0);
    for (x, y, net_a, net_b) in [(10.0, 10.0, "A", "B"), (25.0, 20.0, "C", "D")] {
        for (dx, net) in [(0.0, net_a), (0.75, net_b)] {
            let mut p = Pad::new(Point::new(x + dx, y), PadShape::Rect { w: 0.6, h: 1.0 }, None, Side::Top);
            p.net = net.into();
            b.pads.push(p);
        }
    }
    let checks = crate::fab::preflight(&b, None);
    let dam = checks.iter().find(|c| c.title.contains("ソルダーマスク")).expect("the narrow mask strips are noted");
    assert!(dam.at.is_some() && dam.also.len() == 1, "{:?}", dam.also);
}

#[test]
fn a_board_with_inner_layers_gets_a_note_about_ordering() {
    let mut b = Board::new("m", 30.0, 20.0);
    assert!(!fab::preflight(&b, None).iter().any(|c| c.title.contains("層の基板")));
    b.inner_layers = 2;
    let c = fab::preflight(&b, None);
    let note = c.iter().find(|c| c.title.contains("4 層の基板")).expect("note");
    assert_eq!(note.level, fab::Level::Warn);
}
