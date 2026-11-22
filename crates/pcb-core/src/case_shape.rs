//! The case itself as a solid (E2): a SolidCraft script for the box and its lid round the board, from what `case::inside` worked out
//! (the inside size, the screw posts with their pilot holes, the openings in the walls, the lid's posts), so that it can be printed or
//! taken on in a CAD. SolidCraft (`shape_script::find_solidcraft`) runs it into a STEP; the STEP is read back as the board's case
//! (`case_model::CaseModel`), standing where the 3D view draws the case's frame, and checked against the parts.
//!
//! The script works in the case's frame (x along the width and y along the depth from the inner left front corner, z up from the
//! floor's top face): the box is the outer block with its four upright edges rounded, the inside cut out to the walls' height, the posts
//! joined to the floor with their pilot holes, and the openings cut through the walls. The lid is a plate on the walls with its posts
//! reaching down to the board's top face.

use crate::case::{inside, seat, CaseSpec, Wall};
use crate::model::*;
use crate::shape_script::Script;
use serde_json::{json, Value};

/// The walls and floor of the case (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaseBody {
    pub wall: f64,
    pub floor: f64,
    pub lid: f64,
    /// The radius of the box's upright outer edges (0: square).
    pub corner: f64,
}

impl Default for CaseBody {
    fn default() -> Self {
        CaseBody { wall: 2.0, floor: 2.0, lid: 2.0, corner: 3.0 }
    }
}

/// The inside height of the walls: the posts, the board and the room above it (else the tallest part on top and 2 mm).
pub fn wall_height(board: &Board, spec: &CaseSpec) -> f64 {
    let raise = seat(board, spec).map_or(crate::case::DEFAULT_POST, |s| s.raise);
    let tallest = board.parts.iter().filter(|p| p.side != Side::Bottom).map(|p| crate::model3d::part_height_on(board, p)).fold(0.0, f64::max);
    raise + crate::model3d::BOARD_THICKNESS + if spec.top > 0.0 { spec.top } else { tallest + 2.0 }
}

/// The SolidCraft script of the case's box (`Case`) and lid (`Lid`).
pub fn script(board: &Board, spec: &CaseSpec, body: &CaseBody) -> Result<Value, String> {
    let seat = seat(board, spec).ok_or("ケースの内寸 (幅と奥行) を入れてください")?;
    if !(body.wall > 0.3 && body.floor > 0.3 && body.lid > 0.3) {
        return Err("壁・底・ふたの厚さは 0.3 mm より厚くしてください".into());
    }
    let ins = inside(board, spec);
    let (w, d, t) = (spec.inner_w, spec.inner_d, body.wall);
    let h = wall_height(board, spec);
    let mut s = Script { cmds: vec![] };
    // the box: the outer block, its upright edges rounded, the inside cut out (open at the top)
    s.block("Outer", "Case", "new", [-t, -t, -body.floor], [w + t, d + t, h]);
    let r = body.corner.min(2.0 * t);
    let mid = (h - body.floor) / 2.0;
    if r > 0.05 {
        let e = |x: f64, y: f64| json!([x, y, mid]);
        s.push("solid.fillet", json!({"edges": [e(-t, -t), e(w + t, -t), e(w + t, d + t), e(-t, d + t)], "radius": r, "body": "Case", "name": "Corners"}));
    }
    s.block("Inside", "Case", "cut", [0.0, 0.0, 0.0], [w, d, h + 1.0]);
    // the screw posts on the floor and their pilot holes (from 1 mm above the floor up)
    for (k, p) in ins.posts.iter().enumerate().filter(|(_, p)| p.outer_d > 0.0) {
        s.cylinder(&format!("Post{}", k + 1), "Case", "join", [p.at.x, p.at.y, -0.01], "Z", p.outer_d / 2.0, p.height + 0.01);
        s.cylinder(&format!("Pilot{}", k + 1), "Case", "cut", [p.at.x, p.at.y, 1.0_f64.min(p.height / 2.0)], "Z", p.pilot_d / 2.0, p.height);
    }
    // the openings, cut through their walls
    for (k, o) in ins.openings.iter().enumerate() {
        let (a, b) = (o.along, o.up);
        let (lo, hi) = match o.wall {
            Wall::Front => ([a.0, -t - 1.0, b.0], [a.1, 1.0, b.1]),
            Wall::Back => ([a.0, d - 1.0, b.0], [a.1, d + t + 1.0, b.1]),
            Wall::Left => ([-t - 1.0, a.0, b.0], [1.0, a.1, b.1]),
            Wall::Right => ([w - 1.0, a.0, b.0], [w + t + 1.0, a.1, b.1]),
        };
        // an opening reaching the top of the wall stays a notch (the lid closes it)
        s.block(&format!("Opening{}", k + 1), "Case", "cut", lo, [hi[0], hi[1], hi[2].min(h + 1.0)]);
    }
    s.color(&["Case"], "#3a3d42", false);
    // the lid: a plate on the walls, its edges rounded as the box's, and its posts down onto the board
    s.block("LidPlate", "Lid", "new", [-t, -t, h], [w + t, d + t, h + body.lid]);
    if r > 0.05 {
        let e = |x: f64, y: f64| json!([x, y, h + body.lid / 2.0]);
        s.push("solid.fillet", json!({"edges": [e(-t, -t), e(w + t, -t), e(w + t, d + t), e(-t, d + t)], "radius": r, "body": "Lid", "name": "LidCorners"}));
    }
    let board_top = seat.raise + crate::model3d::BOARD_THICKNESS;
    for (k, p) in ins.lid_posts.iter().enumerate().filter(|(_, p)| p.outer_d > 0.0 && p.problem.is_none()) {
        // a ring round the hole (the screw comes up from below through it)
        s.cylinder(&format!("LidPost{}", k + 1), "Lid", "join", [p.at.x, p.at.y, board_top], "Z", p.outer_d / 2.0, h - board_top + 0.01);
        s.cylinder(&format!("LidBore{}", k + 1), "Lid", "cut", [p.at.x, p.at.y, board_top - 0.01], "Z", p.pilot_d / 2.0, (h - board_top) * 0.8);
    }
    s.color(&["Lid"], "#5a5e66", false);
    Ok(Value::Array(s.cmds))
}

/// Builds the case with SolidCraft into `dir` (`case.step`, `case.json`) and reads it back standing round the board. `None` when
/// SolidCraft is not installed.
pub fn build(board: &Board, spec: &CaseSpec, body: &CaseBody, dir: &std::path::Path) -> Option<Result<(std::path::PathBuf, crate::case_model::CaseModel), String>> {
    let (cli, _) = crate::shape_script::find_solidcraft()?;
    Some((|| {
        let v = script(board, spec, body)?;
        let step = crate::shape_script::run_to_step(&cli, &v, dir, "case")?;
        let bytes = std::fs::read(&step).map_err(|e| format!("{}: {e}", step.display()))?;
        let mesh = crate::step::read_step(&bytes)?;
        let (rot, offset) = seat(board, spec).ok_or("ケースの内寸 (幅と奥行) を入れてください")?.case_to_board();
        let file = step.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        Ok((step, crate::case_model::CaseModel { file, mesh, rot, offset }))
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        let mut b = Board::new("x", 60.0, 40.0);
        for (x, y) in [(3.5, 3.5), (56.5, 3.5), (3.5, 36.5), (56.5, 36.5)] {
            b.holes.push(Hole::new(Point::new(x, y), 3.2));
        }
        for (i, (x, y)) in [(57.5, 18.0), (57.5, 25.0)].iter().enumerate() {
            let mut p = Pad::new(Point::new(*x, *y), PadShape::Circle { d: 1.7 }, Some(1.0), Side::Top);
            p.refdes = "J1".into();
            p.pin = (i + 1).to_string();
            b.pads.push(p);
        }
        b.parts.push(Part { refdes: "J1".into(), value: String::new(), footprint: "USB_DEMO_TH".into(), pos: Point::new(57.5, 21.5), rot: 0, side: Side::Top });
        b
    }

    /// The script: the box and the lid, a post and a pilot hole under every hole, a cut for the USB's opening, the lid's posts; no
    /// floor size, no script.
    #[test]
    fn the_case_script_has_its_posts_openings_and_lid() {
        let spec = CaseSpec { inner_w: 66.0, inner_d: 46.0, bottom: 5.0, top: 12.0, toward: crate::case::Toward::Auto, ..Default::default() };
        let v = script(&board(), &spec, &CaseBody::default()).unwrap();
        let names: Vec<&str> = v.as_array().unwrap().iter().filter_map(|c| c["params"]["name"].as_str()).collect();
        for n in ["Outer", "Corners", "Inside", "Post4", "Pilot4", "Opening1", "LidPlate", "LidCorners", "LidPost4", "LidBore4"] {
            assert!(names.contains(&n), "{n}: {names:?}");
        }
        assert!(script(&board(), &CaseSpec::default(), &CaseBody::default()).is_err());
        assert!(script(&board(), &spec, &CaseBody { wall: 0.1, ..Default::default() }).is_err());
        assert!((wall_height(&board(), &spec) - (5.0 + 1.6 + 12.0)).abs() < 1e-9);
    }

    /// Built by SolidCraft (`--ignored`): the case reads back round the board, the board and the parts clear of it (the USB through its
    /// opening), the lid on top.
    #[test]
    #[ignore]
    fn solidcraft_builds_a_case_the_board_fits() {
        let b = board();
        let spec = CaseSpec { inner_w: 66.0, inner_d: 46.0, bottom: 5.0, top: 12.0, toward: crate::case::Toward::Auto, ..Default::default() };
        let dir = std::env::temp_dir().join("circuitcad_case_shape");
        let (step, case) = build(&b, &spec, &CaseBody::default(), &dir).expect("SolidCraft is installed").unwrap();
        assert!(step.is_file());
        let hits = crate::case_model::collisions(&b, &case);
        assert!(hits.parts.is_empty() && !hits.board, "{hits:?}");
        let (lo, hi) = case.placed().bounds().unwrap();
        assert!(lo[2] < -5.0 && hi[2] > 1.6 + 12.0, "{lo:?} {hi:?}");
    }
}
