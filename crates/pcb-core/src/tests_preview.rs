//! The preview rebuilds the board from the fabrication files.
use crate::gerber_view::*;
use crate::*;

fn sample() -> Board {
    let mut b = Board::new("demo", 30.0, 20.0);
    let mut smd = Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 2.0, h: 1.5 }, None, Side::Top);
    smd.net = "A".into();
    let mut th = Pad::new(Point::new(20.0, 10.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
    th.net = "A".into();
    b.pads.push(smd);
    b.pads.push(th);
    b.tracks.push(Track { side: Side::Top, a: Point::new(10.0, 10.0), b: Point::new(20.0, 10.0), width: 0.4 });
    b.vias.push(Via { pos: Point::new(15.0, 15.0), diameter: 0.6, drill: 0.3, net: String::new() });
    b.holes.push(Hole::new(Point::new(3.0, 3.0), 3.0));
    b
}

fn preview(b: &Board) -> Preview {
    Preview::from_files(&export::fab_files(b), 1000).expect("our own files must be readable")
}

#[test]
fn copper_holes_and_size_come_back_from_the_files() {
    let p = preview(&sample());
    assert!((p.size_mm.0 - 30.0).abs() < 1e-6 && (p.size_mm.1 - 20.0).abs() < 1e-6);
    assert!(p.copper_at(Side::Top, Point::new(10.0, 10.0)), "SMD pad");
    assert!(p.copper_at(Side::Top, Point::new(15.0, 10.0)), "track");
    assert!(!p.copper_at(Side::Top, Point::new(15.0, 12.0)), "nothing between");
    assert!(!p.copper_at(Side::Bottom, Point::new(10.0, 10.0)), "the SMD pad is only on top");
    assert!(p.copper_at(Side::Bottom, Point::new(20.0, 10.0)) && p.copper_at(Side::Top, Point::new(20.0, 10.0)), "through-hole pad on both sides");
    assert!(p.copper_at(Side::Bottom, Point::new(15.0, 15.0)), "via");
    assert!(p.hole_at(Point::new(20.0, 10.0)) && p.hole_at(Point::new(15.0, 15.0)) && p.hole_at(Point::new(3.0, 3.0)));
    assert!(!p.hole_at(Point::new(10.0, 10.0)));
}

#[test]
fn drill_summary_lists_each_size_with_its_count() {
    let p = preview(&sample());
    assert_eq!(p.drills, vec![(0.3, 1, true), (0.8, 1, true), (3.0, 1, false)]);
}

#[test]
fn a_zone_leaves_a_gap_around_other_nets_and_fills_the_rest() {
    let mut b = Board::new("z", 30.0, 20.0);
    b.zones.push(Zone::new("GND", Side::Bottom, vec![Point::new(2.0, 2.0), Point::new(28.0, 2.0), Point::new(28.0, 18.0), Point::new(2.0, 18.0)]));
    let mut sig = Pad::new(Point::new(15.0, 10.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
    sig.net = "SIG".into();
    b.pads.push(sig);
    // a GND pad keeps the pour (a pour that touches nothing of its net is left out)
    let mut gnd = Pad::new(Point::new(4.0, 4.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
    gnd.net = "GND".into();
    b.pads.push(gnd);
    let p = preview(&b);
    assert!(p.copper_at(Side::Bottom, Point::new(8.0, 10.0)), "the pour");
    assert!(p.copper_at(Side::Bottom, Point::new(15.0, 10.0)), "the pad itself");
    assert!(!p.copper_at(Side::Bottom, Point::new(16.0, 10.0)), "clearance ring between pad (r 0.8) and pour (gap 0.3)");
    assert!(p.copper_at(Side::Bottom, Point::new(17.0, 10.0)), "pour again beyond the ring");
    assert!(!p.copper_at(Side::Bottom, Point::new(1.0, 10.0)), "outside the zone polygon");
}

#[test]
fn rendering_colours_exposed_copper_holes_and_the_outside() {
    let b = sample();
    let p = preview(&b);
    let img = p.render(false, true, true);
    let at = |x: f64, y: f64| {
        let i = ((x - p.frame.x0) * p.frame.px) as usize;
        let j = ((p.frame.y1 - y) * p.frame.px) as usize;
        img[j * p.frame.w + i]
    };
    assert_eq!(at(10.0, 10.0)[3], 255);
    assert_eq!(at(10.0, 10.0)[..3], [214, 176, 58], "a pad has a mask opening, so it shows bare metal");
    assert_eq!(at(15.0, 10.0)[..3], [38, 128, 76], "the track is under solder mask");
    assert_eq!(at(20.0, 10.0)[..3], [22, 22, 22], "drilled hole");
    assert_eq!(at(25.0, 5.0)[..3], [14, 70, 44], "bare board");
    assert_eq!(at(-0.5, -0.5)[3], 0, "outside the board is transparent");
    // viewed from the back the SMD pad is not there
    let back = p.render(true, true, true);
    let mirrored = ((p.frame.y1 - 10.0) * p.frame.px) as usize * p.frame.w + (p.frame.w - 1 - ((10.0 - p.frame.x0) * p.frame.px) as usize);
    assert_eq!(back[mirrored][..3], [14, 70, 44]);
}

#[test]
fn the_reader_reports_what_it_cannot_read() {
    assert!(parse_gerber("%FSLAX46Y46*%\nD11*\nX1Y1D03*\n").is_err(), "undefined aperture");
    assert!(parse_gerber("%FSLAX46Y46*%\nHELLO\n").is_err());
    assert!(Preview::from_files(&[], 500).is_err());
    let prims = parse_gerber("%FSLAX46Y46*%\n%ADD10C,0.500000*%\nD10*\nX1000000Y2000000D02*\nX3000000Y2000000D01*\nM02*\n").unwrap();
    assert_eq!(prims, vec![Prim::Line { a: Point::new(1.0, 2.0), b: Point::new(3.0, 2.0), width: 0.5, clear: false }]);
}
