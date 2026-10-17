//! A bigger board than the shipped examples, to look at how placement and routing hold up with many parts:
//! `cargo test -p pcb-core --release large_led_board -- --ignored --nocapture`
//! (`CIRCUITCAD_QUALITY_OUT=file.pcbjson` also writes the board).

use crate::footprints::Footprint;
use crate::netlist::{import, NetlistNet, NetlistPart};
use crate::package::PackageSpec;
use crate::ratsnest::ratsnest;
use crate::*;
use std::time::Instant;

/// `channels` LED channels (resistor + LED each) driven by 74HC595-style shift registers (SOIC-16) in a chain.
fn led_board(registers: usize) -> (Board, usize, usize) {
    let mut b = Board::new("led", 60.0, 40.0);
    let soic16: Footprint = PackageSpec::Dual { pins: 16, pitch: 1.27, row_spacing: 5.4, pad_len: 1.55, pad_width: 0.6, drill: None }.build("SOIC-16").unwrap();
    b.add_footprint(soic16);
    let part = |r: &str, v: &str, f: &str| NetlistPart { refdes: r.into(), value: v.into(), footprint: f.into() };
    let mut parts = vec![part("J1", "IN", "PinHeader_1x5")];
    let mut nets: Vec<(String, Vec<(String, String)>)> = vec![];
    let mut net = |name: &str, pins: &[(String, &str)]| nets.push((name.to_string(), pins.iter().map(|(r, p)| (r.clone(), p.to_string())).collect()));
    let (mut vcc, mut gnd, mut clk, mut latch) = (vec![("J1".to_string(), "1")], vec![("J1".to_string(), "2")], vec![("J1".to_string(), "3")], vec![("J1".to_string(), "4")]);
    let mut chain: Option<String> = None;
    let outputs = ["15", "1", "2", "3", "4", "5", "6", "7"];
    for r in 0..registers {
        let u = format!("U{}", r + 1);
        parts.push(part(&u, "74HC595", "SOIC-16"));
        vcc.push((u.clone(), "16"));
        vcc.push((u.clone(), "10"));
        gnd.push((u.clone(), "8"));
        gnd.push((u.clone(), "13"));
        clk.push((u.clone(), "11"));
        latch.push((u.clone(), "12"));
        // the data input: the header's first register, then the previous one's carry-out
        match chain.take() {
            Some(prev) => net(&format!("CARRY{r}"), &[(prev, "9"), (u.clone(), "14")]),
            None => net("SER", &[("J1".to_string(), "5"), (u.clone(), "14")]),
        }
        chain = Some(u.clone());
        // bypass capacitor
        let c = format!("C{}", r + 1);
        parts.push(part(&c, "100n", "C_0805"));
        vcc.push((c.clone(), "1"));
        gnd.push((c.clone(), "2"));
        for (k, pin) in outputs.iter().enumerate() {
            let n = r * 8 + k + 1;
            let (res, led) = (format!("R{n}"), format!("D{n}"));
            parts.push(part(&res, "330", "R_0805"));
            parts.push(part(&led, "LED", "LED_0805"));
            net(&format!("Q{n}"), &[(u.clone(), pin), (res.clone(), "1")]);
            net(&format!("L{n}"), &[(res.clone(), "2"), (led.clone(), "1")]);
            gnd.push((led.clone(), "2"));
        }
    }
    net("VCC", &vcc);
    net("GND", &gnd);
    net("CLK", &clk);
    net("LATCH", &latch);
    let npart = parts.len();
    let nets: Vec<NetlistNet> = nets.into_iter().map(|(name, pins)| NetlistNet { name, pins }).collect();
    let nnet = nets.len();
    import(&mut b, &parts, &nets);
    (b, npart, nnet)
}

#[test]
#[ignore]
fn large_led_board() {
    let (mut b, parts, nets) = led_board(4);
    let t = Instant::now();
    let placed = autoplace::autoplace(&mut b).unwrap();
    let t_place = t.elapsed();
    let open = ratsnest(&b);
    let ideal: f64 = open.iter().map(|w| w.a.dist(w.b)).sum();
    let t = Instant::now();
    let r = autoroute::autoroute(&mut b, &autoroute::RouteOptions { grid: 0.25, pad_lead: Some(0.5), ..Default::default() }, None);
    let t_route = t.elapsed();
    let len: f64 = b.tracks.iter().map(|t| t.a.dist(t.b)).sum();
    let odd = b.tracks.iter().filter(|t| { let (dx, dy) = ((t.b.x - t.a.x).abs(), (t.b.y - t.a.y).abs()); dx > 1e-6 && dy > 1e-6 && (dx - dy).abs() > 1e-6 }).count();
    let steps = b.tracks.iter().filter(|t| { let (dx, dy) = ((t.b.x - t.a.x).abs(), (t.b.y - t.a.y).abs()); t.a.dist(t.b) < 0.25 && dx > 1e-6 && dy > 1e-6 }).count();
    println!("QUALITY parts {parts} nets {nets} airwires {} placed {} board {}x{} (place {:?})", open.len(), placed.placed, placed.width, placed.height, t_place);
    println!("QUALITY routed {} failed {} in {:?}; unrouted left {}; drc {}", r.routed, r.failed.len(), t_route, ratsnest(&b).len(), drc::check(&b).len());
    println!("QUALITY track length {len:.0} mm vs straight-line {ideal:.0} mm (x{:.2}); vias {}; tracks {}; odd-angle {odd}; tiny steps {steps}", len / ideal.max(1e-9), b.vias.len(), b.tracks.len());
    assert!(r.failed.len() <= 2, "{} connections left open", r.failed.len());
    assert_eq!(odd, 0);
    assert!(steps <= 3, "{steps} tiny steps");
    for w in &r.failed { println!("QUALITY failed {} {:?} -> {:?}", w.net, w.a, w.b); }
    if let Ok(out) = std::env::var("CIRCUITCAD_QUALITY_OUT") {
        std::fs::write(out, serde_json::to_string_pretty(&b).unwrap()).unwrap();
    }
}
