//! A plain box for the board to go in (the inside size of the case) and what does not fit: the board's footprint on the floor of
//! the box (turned a quarter if that helps), and the parts taller than the room above and below the board. No STEP is needed:
//! these are the numbers that the case's drawing gives (`dxf_in` takes the outline from the drawing itself).

use crate::model::*;
use crate::model3d::part_height_on;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaseSpec {
    /// Inside width and depth of the box in mm (0: the floor is not checked).
    pub inner_w: f64,
    pub inner_d: f64,
    /// Room from the board's surface to the lid (top) and to the floor (bottom) in mm (0: not checked).
    pub top: f64,
    pub bottom: f64,
    /// Gap to keep between the board's edge and the wall in mm.
    pub clearance: f64,
    /// Where the board sits on the floor: in the middle, against walls, or against the walls its connectors go through.
    pub toward: Toward,
}

impl Default for CaseSpec {
    fn default() -> Self {
        CaseSpec { inner_w: 0.0, inner_d: 0.0, top: 0.0, bottom: 0.0, clearance: 0.5, toward: Toward::Middle }
    }
}

/// Where the board sits on the case's floor (E2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Toward {
    /// In the middle of the floor.
    #[default]
    Middle,
    /// Against the walls the connectors at its edges go through (on the side that has them; in the middle along an axis with
    /// connectors on both walls or none), so that the plugs reach in.
    Auto,
    /// Against these walls, `clearance` from them: x (−1 left, 0 middle, +1 right) and y (−1 front, 0 middle, +1 back).
    Walls(i8, i8),
}

impl Toward {
    /// From a word: `middle`, `auto`, or walls `left`, `right`, `front`, `back` joined by `,` (one per axis).
    pub fn parse(s: &str) -> Result<Toward, String> {
        match s.trim() {
            "" | "middle" => return Ok(Toward::Middle),
            "auto" => return Ok(Toward::Auto),
            _ => {}
        }
        let (mut x, mut y) = (0, 0);
        for w in s.split(',').map(str::trim) {
            match w {
                "left" => x = -1,
                "right" => x = 1,
                "front" => y = -1,
                "back" => y = 1,
                _ => return Err(format!("{w}: middle, auto, or left / right / front / back (two joined by a comma)")),
            }
        }
        Ok(Toward::Walls(x, y))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaseReport {
    /// The board's size (the box round its outline and parts).
    pub board_w: f64,
    pub board_d: f64,
    /// Is the floor checked, does the board go in, and is it turned a quarter to go in?
    pub floor_checked: bool,
    pub fits: bool,
    pub turned: bool,
    /// Room left on each side (width, depth) in the turn that was chosen (negative: how much too big).
    pub spare_w: f64,
    pub spare_d: f64,
    /// Parts above the room: (reference, side, height, how much over).
    pub too_tall: Vec<(String, Side, f64, f64)>,
}

impl CaseReport {
    pub fn ok(&self) -> bool {
        (!self.floor_checked || self.fits) && self.too_tall.is_empty()
    }
}

pub fn check(board: &Board, spec: &CaseSpec) -> CaseReport {
    let (lo, hi) = board.bounds().unwrap_or_default();
    let (bw, bd) = (hi.x - lo.x, hi.y - lo.y);
    let floor_checked = spec.inner_w > 0.0 && spec.inner_d > 0.0;
    let (aw, ad) = (spec.inner_w - 2.0 * spec.clearance, spec.inner_d - 2.0 * spec.clearance);
    let straight = (aw - bw, ad - bd);
    let turned_sp = (aw - bd, ad - bw);
    let min = |s: (f64, f64)| s.0.min(s.1);
    let turned = floor_checked && min(straight) < 0.0 && min(turned_sp) >= 0.0;
    let (spare_w, spare_d) = if turned || (floor_checked && min(turned_sp) > min(straight)) { turned_sp } else { straight };
    let fits = floor_checked && spare_w >= 0.0 && spare_d >= 0.0;
    let mut too_tall = vec![];
    for p in &board.parts {
        let room = if p.side == Side::Bottom { spec.bottom } else { spec.top };
        if room <= 0.0 {
            continue;
        }
        let h = part_height_on(board, p);
        if h > room + 1e-9 {
            too_tall.push((p.refdes.clone(), p.side, h, h - room));
        }
    }
    too_tall.sort_by(|a, b| b.3.total_cmp(&a.3));
    CaseReport { board_w: bw, board_d: bd, floor_checked, fits, turned, spare_w, spare_d, too_tall }
}

// ---------------------------------------------------------------- inside the case: posts and openings

/// How high the screw posts are when the room under the board is not given (mm).
pub const DEFAULT_POST: f64 = 5.0;
/// Room left round an opening in a wall, each side (mm): the part slides in, and a plug's sleeve has room.
pub const OPENING_MARGIN: f64 = 0.5;
/// A connector whose body comes this close to the board's edge (mm) is reached from outside, through a wall (automatic
/// placement leaves 3 mm round the parts, and a plug reaches a few mm in).
const NEAR_EDGE: f64 = 5.0;

/// A wall of the case, seen from above with x along the width and y along the depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wall {
    /// y = 0
    Front,
    /// y = depth
    Back,
    /// x = 0
    Left,
    /// x = width
    Right,
}

impl Wall {
    pub fn label(self) -> &'static str {
        match self {
            Wall::Front => "手前の壁",
            Wall::Back => "奥の壁",
            Wall::Left => "左の壁",
            Wall::Right => "右の壁",
        }
    }

    /// The wall's name in the DXF (ASCII: an R12 DXF has no sure way to carry Japanese text).
    pub fn code(self) -> &'static str {
        match self {
            Wall::Front => "FRONT wall (y = 0)",
            Wall::Back => "BACK wall",
            Wall::Left => "LEFT wall (x = 0)",
            Wall::Right => "RIGHT wall",
        }
    }

    /// What the position along this wall is measured from.
    pub fn along_from(self) -> &'static str {
        match self {
            Wall::Front | Wall::Back => "左の内側の面から",
            Wall::Left | Wall::Right => "手前の内側の面から",
        }
    }
}

/// Where the board sits in the case: in the middle of the floor, turned a quarter when that is how it goes in. Maps board
/// points to the case's inside (x along the width, y along the depth, from the inner corner at the left front; z from the floor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seat {
    pub turned: bool,
    /// The middle of the board's outline (board coordinates) and of the floor (case coordinates).
    board_mid: Point,
    floor_mid: Point,
    /// The height of the board's bottom face above the floor (the posts' height).
    pub raise: f64,
}

impl Seat {
    /// A board point on the case's floor. Turned: the board's +x runs along the case's +y.
    pub fn place(&self, p: Point) -> Point {
        let (dx, dy) = (p.x - self.board_mid.x, p.y - self.board_mid.y);
        let (dx, dy) = if self.turned { (-dy, dx) } else { (dx, dy) };
        Point::new(self.floor_mid.x + dx, self.floor_mid.y + dy)
    }

    /// A point on the case's floor back on the board (the inverse of `place`).
    pub fn unplace(&self, c: Point) -> Point {
        let (dx, dy) = (c.x - self.floor_mid.x, c.y - self.floor_mid.y);
        let (dx, dy) = if self.turned { (dy, -dx) } else { (dx, dy) };
        Point::new(self.board_mid.x + dx, self.board_mid.y + dy)
    }

    /// How a shape in the case's frame (x along the width, y along the depth from the inner left front corner, z up from the floor)
    /// stands in the board's frame (`Mesh3d::placed`): quarter turns about z, then the move. The inverse of `place`, with z down by the
    /// posts' height (the board's bottom face is z = 0).
    pub fn case_to_board(&self) -> (u8, [f64; 3]) {
        // turned: a case point goes to the board by (x, y) -> (y, -x), three quarter turns
        let rot = if self.turned { 3 } else { 0 };
        let f = if self.turned { Point::new(self.floor_mid.y, -self.floor_mid.x) } else { self.floor_mid };
        (rot, [self.board_mid.x - f.x, self.board_mid.y - f.y, -self.raise])
    }

    /// A direction on the board (±x, ±y) as the wall it points at.
    fn wall(&self, dir: (i8, i8)) -> Wall {
        let (x, y) = if self.turned { (-dir.1, dir.0) } else { dir };
        match (x, y) {
            (1, _) => Wall::Right,
            (-1, _) => Wall::Left,
            (_, 1) => Wall::Back,
            _ => Wall::Front,
        }
    }
}

/// A screw post standing on the floor under one of the board's mounting holes.
#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    /// The hole in the board (board coordinates) and the post's place on the floor (case coordinates).
    pub hole: Point,
    pub at: Point,
    /// From the floor to the board's bottom face (mm).
    pub height: f64,
    /// The screw that goes through the hole ("M3"), the post's outside diameter and the pilot hole for a self-tapping screw.
    pub screw: &'static str,
    pub outer_d: f64,
    pub pilot_d: f64,
    /// What the post runs into (a part or a lead under the board, the wall), if anything.
    pub problem: Option<String>,
}

/// An opening in a wall for a part that is reached from outside (a USB socket, a DC jack, a terminal block's wire entries).
#[derive(Clone, Debug, PartialEq)]
pub struct Opening {
    pub refdes: String,
    pub wall: Wall,
    /// Along the wall (see `Wall::along_from`) and up from the floor (mm), with `OPENING_MARGIN` round the part.
    pub along: (f64, f64),
    pub up: (f64, f64),
    /// From the part's face to the wall's inside face (mm): far means a plug may not reach in; negative, the part goes into the wall.
    pub to_wall: f64,
}

/// The posts, the openings and what the person should know (no mounting holes, the posts' height was assumed ...).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inside {
    pub posts: Vec<Post>,
    /// Posts hanging from the lid onto the board's top face round the same holes (pressing the board down; `height` from the lid to
    /// the board's top face). Only when the room above the board is given.
    pub lid_posts: Vec<Post>,
    pub openings: Vec<Opening>,
    /// When the board has no round mounting hole: free places for M3 holes near its corners (board coordinates), clear of the parts,
    /// pads and tracks and 3.5 mm or more inside the outline.
    pub suggested_holes: Vec<Point>,
    /// The walls the board was pushed against (`Toward::Auto` and `Walls`): x and y as in `Toward::Walls`.
    pub pushed: (i8, i8),
    pub notes: Vec<String>,
}

/// The screw for a mounting hole of this diameter: (name, post outside diameter, pilot hole for a self-tapping screw).
fn screw_for(drill: f64) -> (&'static str, f64, f64) {
    match drill {
        d if d <= 2.45 => ("M2", 4.5, 1.6),
        d if d <= 2.95 => ("M2.5", 5.0, 2.0),
        d if d <= 3.65 => ("M3", 6.0, 2.5),
        d if d <= 4.65 => ("M4", 8.0, 3.3),
        _ => ("?", 0.0, 0.0),
    }
}

/// Where the board sits in this case (`None` when the floor is not given): in the middle, or pushed against walls (`spec.toward`).
pub fn seat(board: &Board, spec: &CaseSpec) -> Option<Seat> {
    seat_pushed(board, spec).map(|s| s.0)
}

/// The seat and the walls it was pushed against.
fn seat_pushed(board: &Board, spec: &CaseSpec) -> Option<(Seat, (i8, i8))> {
    let push = match spec.toward {
        Toward::Middle => (0, 0),
        Toward::Walls(x, y) => (x.signum(), y.signum()),
        Toward::Auto => {
            // the walls the connectors go through with the board in the middle
            let mid = seat_at(board, spec, (0, 0))?;
            let walls: Vec<Wall> = openings(board, spec, &mid).iter().map(|o| o.wall).collect();
            let side = |neg: Wall, pos: Wall| match (walls.contains(&neg), walls.contains(&pos)) {
                (true, false) => -1,
                (false, true) => 1,
                _ => 0,
            };
            (side(Wall::Left, Wall::Right), side(Wall::Front, Wall::Back))
        }
    };
    Some((seat_at(board, spec, push)?, push))
}

/// The seat with the board in the middle along an axis (0) or `clearance` from the wall on that side (−1 left / front, +1 right / back).
fn seat_at(board: &Board, spec: &CaseSpec, push: (i8, i8)) -> Option<Seat> {
    let r = check(board, spec);
    if !r.floor_checked {
        return None;
    }
    let (lo, hi) = board.bounds()?;
    // the board's size on the floor, turned as it goes in
    let (pw, pd) = if r.turned { (hi.y - lo.y, hi.x - lo.x) } else { (hi.x - lo.x, hi.y - lo.y) };
    let at = |k: i8, inner: f64, size: f64| match k {
        k if k < 0 => spec.clearance + size / 2.0,
        k if k > 0 => inner - spec.clearance - size / 2.0,
        _ => inner / 2.0,
    };
    Some(Seat {
        turned: r.turned,
        board_mid: Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0),
        floor_mid: Point::new(at(push.0, spec.inner_w, pw), at(push.1, spec.inner_d, pd)),
        raise: if spec.bottom > 0.0 { spec.bottom } else { DEFAULT_POST },
    })
}

/// The screw posts under the board's mounting holes and the openings in the walls for the parts reached from outside.
pub fn inside(board: &Board, spec: &CaseSpec) -> Inside {
    let mut out = Inside::default();
    let Some((seat, pushed)) = seat_pushed(board, spec) else {
        out.notes.push("ケースの内寸 (幅と奥行) を入れると、ねじ柱と壁の穴の位置を出します".into());
        return out;
    };
    out.pushed = pushed;
    if spec.bottom <= 0.0 {
        out.notes.push(format!("基板の下の高さが無いので、ねじ柱の高さを {DEFAULT_POST} mm として計算しました。裏の部品やはんだの足に合わせて決めてください"));
    }
    // posts
    let round: Vec<&Hole> = board.holes.iter().filter(|h| h.slot <= h.drill).collect();
    if round.is_empty() {
        out.suggested_holes = suggest_holes(board);
        if out.suggested_holes.is_empty() {
            out.notes.push("基板に取り付け穴がありません。角の近くに M3 の穴 (3.2 mm) を置ける空いた所も見つかりませんでした。部品を内側に寄せるか、ケースに溝を付けて基板を差し込む形にしましょう".into());
        } else {
            let list: Vec<String> = out.suggested_holes.iter().map(|p| format!("({:.1}, {:.1})", p.x, p.y)).collect();
            out.notes.push(format!(
                "基板に取り付け穴がありません。ねじで留めるなら、部品・パッド・配線から離れた角の近くに M3 の穴 (3.2 mm) を置けます: {}。穴を足すと、その下にねじ柱が立ちます",
                list.join("、")
            ));
        }
    }
    let room = |bottom: bool| -> Vec<(String, Point, Point)> { board.parts.iter().filter(|p| (p.side == Side::Bottom) == bottom).filter_map(|p| crate::autoplace::extent(board, &p.refdes).map(|(lo, hi)| (p.refdes.clone(), lo, hi))).collect() };
    let underneath = room(true);
    // above the board, what the 3D view draws (a guessed can is wider than its pads): a screw's head must clear it
    let solids = crate::model3d::part_solid_extents(board);
    let above: Vec<(String, Point, Point)> = room(false)
        .into_iter()
        .map(|(name, lo, hi)| match board.parts.iter().position(|p| p.refdes == name).and_then(|i| solids[i]) {
            Some((a, b)) => (name, Point::new(lo.x.min(a[0]), lo.y.min(a[1])), Point::new(hi.x.max(b[0]), hi.y.max(b[1]))),
            None => (name, lo, hi),
        })
        .collect();
    for h in round {
        let (screw, outer_d, pilot_d) = screw_for(h.drill);
        let at = seat.place(h.pos);
        let r = outer_d / 2.0;
        let mut problem = None;
        if screw == "?" {
            problem = Some(format!("穴 {:.1} mm に合うねじ (M2〜M4) がありません。柱は自分で決めてください", h.drill));
        }
        // a part under the board, or a lead coming through (the solder joint of a through-hole pad sits on the bottom face)
        if problem.is_none()
            && let Some((name, ..)) = underneath.iter().find(|(_, lo, hi)| h.pos.x + r > lo.x && h.pos.x - r < hi.x && h.pos.y + r > lo.y && h.pos.y - r < hi.y)
        {
            problem = Some(format!("裏の部品 {name} に当たります"));
        }
        // the screw's head on the top face (about as wide as the post)
        if problem.is_none()
            && let Some((name, ..)) = above.iter().find(|(_, lo, hi)| h.pos.x + r > lo.x && h.pos.x - r < hi.x && h.pos.y + r > lo.y && h.pos.y - r < hi.y)
        {
            problem = Some(format!("ねじの頭が表の部品 {name} に当たります。穴か部品を動かしましょう"));
        }
        if problem.is_none()
            && let Some(p) = board.pads.iter().find(|p| (p.drill.is_some() || p.side == Side::Bottom) && {
                let (w, hh) = p.shape.size();
                (p.pos.x - h.pos.x).abs() < r + w / 2.0 && (p.pos.y - h.pos.y).abs() < r + hh / 2.0
            })
        {
            problem = Some(format!("{}.{} のパッド (裏のはんだ) に当たります。柱を細くするか、座金で浮かせます", p.refdes, p.pin));
        }
        if problem.is_none() && (at.x - r < 0.0 || at.y - r < 0.0 || at.x + r > spec.inner_w || at.y + r > spec.inner_d) {
            problem = Some("柱が壁にかかります".into());
        }
        out.posts.push(Post { hole: h.pos, at, height: seat.raise, screw, outer_d, pilot_d, problem });
        // the lid's post: from the lid onto the board's top face round the hole, clear of the parts on top
        if spec.top > 0.0 {
            let problem = above
                .iter()
                .find(|(_, lo, hi)| h.pos.x + r > lo.x && h.pos.x - r < hi.x && h.pos.y + r > lo.y && h.pos.y - r < hi.y)
                .map(|(name, ..)| format!("ふたの柱が表の部品 {name} に当たります。柱を細くするか、穴か部品を動かしましょう"));
            out.lid_posts.push(Post { hole: h.pos, at, height: spec.top, screw, outer_d, pilot_d, problem });
        }
    }
    out.openings = openings(board, spec, &seat);
    let far: Vec<String> = out.openings.iter().filter(|o| o.to_wall > FAR_FROM_WALL).map(|o| format!("{} ({:.1} mm)", o.refdes, o.to_wall)).collect();
    if !far.is_empty() {
        let how = if spec.toward == Toward::Middle { "基板を壁に寄せる (寄せ方「コネクタの壁へ」)" } else { "基板を寄せる壁を変える" };
        out.notes.push(format!("壁から離れたコネクタがあります: {}。プラグが奥まで届かないことがあるので、{how}か、内寸の小さいケースにするか、コネクタを基板の縁に寄せましょう", far.join(", ")));
    }
    out
}

/// Free places for M3 mounting holes near the board's corners: from 3.5 mm inside each corner, the nearest to it (in 0.5 mm steps up
/// to 15 mm along each edge) where a post of 6 mm and a screw head as wide keep clear of the parts on both faces (their 3D bodies),
/// the pads and the tracks, and stay 3.5 mm inside the outline.
pub fn suggest_holes(board: &Board) -> Vec<Point> {
    let Some((lo, hi)) = board.bounds() else { return vec![] };
    let r = 3.0;
    let solids = crate::model3d::part_solid_extents(board);
    let mut boxes: Vec<(Point, Point)> = board.parts.iter().filter_map(|p| crate::autoplace::extent(board, &p.refdes)).collect();
    boxes.extend(solids.iter().flatten().map(|(a, b)| (Point::new(a[0], a[1]), Point::new(b[0], b[1]))));
    let seg = |p: Point, a: Point, b: Point| {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let l2 = dx * dx + dy * dy;
        let t = if l2 < 1e-12 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
        p.dist(Point::new(a.x + t * dx, a.y + t * dy))
    };
    let n = board.outline.len();
    let free = |c: Point| {
        n >= 3
            && crate::geom::inside_polygon(&board.outline, c)
            && (0..n).all(|i| seg(c, board.outline[i], board.outline[(i + 1) % n]) >= 3.5)
            && boxes.iter().all(|(a, b)| c.x + r <= a.x || c.x - r >= b.x || c.y + r <= a.y || c.y - r >= b.y)
            && board.pads.iter().all(|p| { let (w, h) = p.shape.size(); (p.pos.x - c.x).abs() >= r + w / 2.0 || (p.pos.y - c.y).abs() >= r + h / 2.0 })
            && board.tracks.iter().all(|t| seg(c, t.a, t.b) >= r + t.width / 2.0)
            && board.holes.iter().all(|h| h.pos.dist(c) >= r + h.drill / 2.0 + 1.0)
    };
    let mut out = vec![];
    for (cx, cy, sx, sy) in [(lo.x, lo.y, 1.0, 1.0), (hi.x, lo.y, -1.0, 1.0), (hi.x, hi.y, -1.0, -1.0), (lo.x, hi.y, 1.0, -1.0)] {
        let mut best: Option<(f64, Point)> = None;
        for i in 0..=23 {
            for j in 0..=23 {
                let (a, b) = (3.5 + 0.5 * i as f64, 3.5 + 0.5 * j as f64);
                let c = Point::new(cx + sx * a, cy + sy * b);
                if best.is_none_or(|(d, _)| a + b < d) && free(c) && out.iter().all(|q: &Point| q.dist(c) > 2.0 * r) {
                    best = Some((a + b, c));
                }
            }
        }
        out.extend(best.map(|b| b.1));
    }
    out
}

/// The openings in the walls for the parts reached from outside, with the board where `seat` puts it.
fn openings(board: &Board, spec: &CaseSpec, seat: &Seat) -> Vec<Opening> {
    let mut out = vec![];
    let solids = crate::model3d::part_solid_extents(board);
    let Some((blo, bhi)) = board.bounds() else { return out };
    for (i, part) in board.parts.iter().enumerate() {
        let Some((lo, hi)) = solids[i] else { continue };
        let kind = crate::model3d::part_kind(&part.refdes, &part.footprint);
        let reached = matches!(kind, crate::model3d::Kind::Shell | crate::model3d::Kind::Terminal);
        let gaps = [(lo[0] - blo.x, (-1, 0)), (bhi.x - hi[0], (1, 0)), (lo[1] - blo.y, (0, -1)), (bhi.y - hi[1], (0, 1))];
        // the way its mouth faces, read from a maker's mesh (turned and mirrored with the part); else the board edge it is nearest
        let mouth = board.models3d.get(&part.footprint).and_then(crate::model3d::mouth).map(|(dx, dy)| {
            let d = crate::parts::rotate(Point::new(if part.side == Side::Bottom { -f64::from(dx) } else { f64::from(dx) }, f64::from(dy)), part.rot);
            (d.x.round() as i8, d.y.round() as i8)
        });
        let (gap, dir) = match mouth {
            Some(m) => gaps.iter().copied().find(|g| g.1 == m).unwrap_or(gaps[0]),
            None => gaps.iter().copied().min_by(|a, b| a.0.total_cmp(&b.0)).unwrap(),
        };
        // a connector near the edge, or anything that sticks out past it
        if !(reached && gap <= NEAR_EDGE || gap < -0.05) {
            continue;
        }
        let wall = seat.wall(dir);
        // the part's face towards the wall and its two sides along it, on the floor
        let face = Point::new(if dir.0 < 0 { lo[0] } else if dir.0 > 0 { hi[0] } else { (lo[0] + hi[0]) / 2.0 }, if dir.1 < 0 { lo[1] } else if dir.1 > 0 { hi[1] } else { (lo[1] + hi[1]) / 2.0 });
        let (s0, s1) = if dir.0 != 0 { (Point::new(face.x, lo[1]), Point::new(face.x, hi[1])) } else { (Point::new(lo[0], face.y), Point::new(hi[0], face.y)) };
        let (a, b, f) = (seat.place(s0), seat.place(s1), seat.place(face));
        let (along, to_wall) = match wall {
            Wall::Front => ((a.x.min(b.x), a.x.max(b.x)), f.y),
            Wall::Back => ((a.x.min(b.x), a.x.max(b.x)), spec.inner_d - f.y),
            Wall::Left => ((a.y.min(b.y), a.y.max(b.y)), f.x),
            Wall::Right => ((a.y.min(b.y), a.y.max(b.y)), spec.inner_w - f.x),
        };
        let m = OPENING_MARGIN;
        // the body only: the legs through the board (a mesh's, below the top face) need no opening
        let t = crate::model3d::BOARD_THICKNESS;
        let (z0, z1) = if part.side == Side::Bottom { (lo[2], hi[2].min(0.0)) } else { (lo[2].max(t), hi[2]) };
        out.push(Opening {
            refdes: part.refdes.clone(),
            wall,
            along: (along.0 - m, along.1 + m),
            up: ((seat.raise + z0 - m).max(0.0), seat.raise + z1 + m),
            to_wall,
        });
    }
    out
}

/// A connector further than this from its wall (mm) may be out of reach of a plug through the opening.
const FAR_FROM_WALL: f64 = 4.0;

/// The case's drawing for its maker (DXF, mm): the floor seen from above with the board's outline and the posts (outside circle,
/// pilot hole, screw and height), and the four walls folded out flat round it, seen from inside, with their openings. Layers
/// `CASE` (the floor and walls), `BOARD`, `POSTS`, `OPENINGS` and `NOTES`.
pub fn drawing(board: &Board, spec: &CaseSpec) -> Result<String, String> {
    let seat = seat(board, spec).ok_or("ケースの内寸 (幅と奥行) を入れてください")?;
    let ins = inside(board, spec);
    let (w, d) = (spec.inner_w, spec.inner_d);
    // the walls' height: the posts, the board and the room above it (else the tallest part and 2 mm)
    let tallest = board.parts.iter().filter(|p| p.side != Side::Bottom).map(|p| crate::model3d::part_height_on(board, p)).fold(0.0, f64::max);
    let wall_h = seat.raise + crate::model3d::BOARD_THICKNESS + if spec.top > 0.0 { spec.top } else { tallest + 2.0 };
    let mut x = crate::dxf::Dxf::new();
    let rect = |x: &mut crate::dxf::Dxf, layer: &str, a: Point, b: Point| {
        let c = [a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)];
        for i in 0..4 {
            x.line(layer, c[i], c[(i + 1) % 4]);
        }
    };
    rect(&mut x, "CASE", Point::new(0.0, 0.0), Point::new(w, d));
    let n = board.outline.len();
    for i in 0..n {
        x.line("BOARD", seat.place(board.outline[i]), seat.place(board.outline[(i + 1) % n]));
    }
    for p in &ins.posts {
        x.circle("POSTS", p.at, p.outer_d);
        x.circle("POSTS", p.at, p.pilot_d);
        x.text("NOTES", Point::new(p.at.x + p.outer_d / 2.0 + 0.5, p.at.y), 1.5, &format!("{} h{:.1}", p.screw, p.height));
    }
    // the walls folded out: a point (along, up) on each wall drawn on the paper
    let on_paper = |wall: Wall, along: f64, up: f64| match wall {
        Wall::Front => Point::new(along, -up),
        Wall::Back => Point::new(along, d + up),
        Wall::Left => Point::new(-up, along),
        Wall::Right => Point::new(w + up, along),
    };
    for (wall, len) in [(Wall::Front, w), (Wall::Back, w), (Wall::Left, d), (Wall::Right, d)] {
        rect(&mut x, "CASE", on_paper(wall, 0.0, 0.0), on_paper(wall, len, wall_h));
        let mid = on_paper(wall, len / 2.0, wall_h + 3.0);
        x.text("NOTES", mid, 2.0, wall.code());
    }
    for o in &ins.openings {
        rect(&mut x, "OPENINGS", on_paper(o.wall, o.along.0, o.up.0), on_paper(o.wall, o.along.1, o.up.1));
        x.text("NOTES", on_paper(o.wall, o.along.0, o.up.1 + 1.0), 1.5, &o.refdes);
    }
    // the holes the board could have (no mounting hole yet): where the posts would stand
    for p in &ins.suggested_holes {
        let at = seat.place(*p);
        x.circle("SUGGESTED", at, 6.0);
        x.circle("SUGGESTED", at, 3.2);
        x.text("NOTES", Point::new(at.x + 3.5, at.y), 1.5, "M3 hole?");
    }
    // the lid, seen from inside (from below: mirrored left to right), right of the right wall, with its posts
    if !ins.lid_posts.is_empty() {
        let x0 = w + wall_h + 10.0;
        let lid = |p: Point| Point::new(x0 + (w - p.x), p.y);
        rect(&mut x, "CASE", lid(Point::new(w, 0.0)), lid(Point::new(0.0, d)));
        x.text("NOTES", Point::new(x0, d + 3.0), 2.0, "LID (seen from inside)");
        for p in &ins.lid_posts {
            x.circle("POSTS", lid(p.at), p.outer_d);
            x.text("NOTES", Point::new(lid(p.at).x + p.outer_d / 2.0 + 0.5, p.at.y), 1.5, &format!("h{:.1}", p.height));
        }
    }
    x.text("NOTES", Point::new(0.0, -wall_h - 8.0), 2.0, &format!("inside {w:.1} x {d:.1} mm, walls {wall_h:.1} mm high (seen from inside, folded out). posts h{:.1} mm", seat.raise));
    Ok(x.finish(&[("CASE", 7), ("BOARD", 3), ("POSTS", 1), ("OPENINGS", 6), ("SUGGESTED", 8), ("NOTES", 2)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_goes_in_a_box_or_does_not_and_a_quarter_turn_can_save_it() {
        let b = Board::new("x", 60.0, 40.0);
        let spec = |w, d| CaseSpec { inner_w: w, inner_d: d, ..Default::default() };
        let r = check(&b, &spec(70.0, 50.0));
        assert!(r.fits && !r.turned && (r.spare_w - 9.0).abs() < 1e-9, "{r:?}");
        let r = check(&b, &spec(45.0, 65.0));
        assert!(r.fits && r.turned, "turned a quarter it goes in: {r:?}");
        let r = check(&b, &spec(55.0, 45.0));
        assert!(!r.fits && r.spare_w < 0.0, "{r:?}");
        assert!(!r.ok());
        // nothing given: nothing is checked
        assert!(check(&b, &CaseSpec::default()).ok());
    }

    #[test]
    fn a_part_taller_than_the_room_is_named_with_the_excess() {
        let mut b = Board::new("x", 30.0, 20.0);
        // a pin header: 2.5 mm body + 6 mm of pins sticks out
        b.parts.push(Part { refdes: "J1".into(), value: String::new(), footprint: "PinHeader_1x4".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top });
        let r = check(&b, &CaseSpec { top: 5.0, ..Default::default() });
        assert_eq!(r.too_tall.len(), 1, "{r:?}");
        assert!(r.too_tall[0].3 > 0.0);
        let r = check(&b, &CaseSpec { top: 50.0, ..Default::default() });
        assert!(r.too_tall.is_empty());
        let r = check(&b, &CaseSpec { bottom: 1.0, ..Default::default() });
        assert!(r.too_tall.is_empty(), "the part is on the top: the bottom room does not matter");
    }

    fn board_with_holes() -> Board {
        let mut b = Board::new("x", 60.0, 40.0);
        for (x, y) in [(3.5, 3.5), (56.5, 3.5), (3.5, 36.5), (56.5, 36.5)] {
            b.holes.push(Hole::new(Point::new(x, y), 3.2));
        }
        b
    }

    /// A post stands under every mounting hole: M3 for a 3.2 mm hole, as high as the room under the board, placed on the floor
    /// with the board in the middle of the case.
    #[test]
    fn posts_stand_under_the_holes_with_their_screw_and_height() {
        let b = board_with_holes();
        let spec = CaseSpec { inner_w: 70.0, inner_d: 50.0, bottom: 6.0, ..Default::default() };
        let ins = inside(&b, &spec);
        assert_eq!(ins.posts.len(), 4);
        let p = &ins.posts[0];
        assert_eq!((p.screw, p.outer_d, p.pilot_d, p.height), ("M3", 6.0, 2.5, 6.0));
        // the board (60 x 40) in the middle of the 70 x 50 floor: the hole at (3.5, 3.5) is 5 mm further in
        assert!((p.at.x - 8.5).abs() < 1e-9 && (p.at.y - 8.5).abs() < 1e-9, "{p:?}");
        assert!(ins.posts.iter().all(|p| p.problem.is_none()), "{ins:?}");
        assert!(ins.notes.is_empty(), "the height was given: {:?}", ins.notes);
        // no room under the board given: 5 mm, and said so
        let ins = inside(&b, &CaseSpec { inner_w: 70.0, inner_d: 50.0, ..Default::default() });
        assert_eq!(ins.posts[0].height, DEFAULT_POST);
        assert!(ins.notes[0].contains("5 mm"));
    }

    /// A post that would sit on a through-hole pad's solder or under a bottom part is named; a board without holes is told
    /// where to put them; a board turned a quarter keeps its posts under its holes.
    #[test]
    fn posts_that_collide_and_boards_without_holes_are_told() {
        let mut b = board_with_holes();
        let mut pad = Pad::new(Point::new(6.0, 3.5), PadShape::Circle { d: 1.8 }, Some(1.0), Side::Top);
        pad.refdes = "J1".into();
        pad.pin = "1".into();
        b.pads.push(pad);
        let ins = inside(&b, &CaseSpec { inner_w: 70.0, inner_d: 50.0, bottom: 5.0, ..Default::default() });
        assert!(ins.posts[0].problem.as_deref().unwrap_or("").contains("J1.1"), "{:?}", ins.posts[0]);
        // a top part next to the hole: the screw's head hits it
        let mut c = board_with_holes();
        let mut q = Pad::new(Point::new(54.0, 36.5), PadShape::Rect { w: 1.5, h: 1.5 }, None, Side::Top);
        q.refdes = "U1".into();
        c.pads.push(q);
        c.parts.push(Part { refdes: "U1".into(), value: String::new(), footprint: "x".into(), pos: Point::new(54.0, 36.5), rot: 0, side: Side::Top });
        let ins = inside(&c, &CaseSpec { inner_w: 70.0, inner_d: 50.0, bottom: 5.0, ..Default::default() });
        assert!(ins.posts.iter().any(|p| p.problem.as_deref().is_some_and(|m| m.contains("U1"))), "{ins:?}");
        let bare = Board::new("x", 60.0, 40.0);
        let ins = inside(&bare, &CaseSpec { inner_w: 70.0, inner_d: 50.0, bottom: 5.0, ..Default::default() });
        assert!(ins.posts.is_empty() && ins.notes.iter().any(|n| n.contains("取り付け穴がありません")), "{ins:?}");
        // turned a quarter (the case is narrow and deep): the hole at the board's left front is at the case's right front
        let ins = inside(&board_with_holes(), &CaseSpec { inner_w: 50.0, inner_d: 70.0, bottom: 5.0, ..Default::default() });
        let p = ins.posts.iter().find(|p| p.hole == Point::new(3.5, 3.5)).unwrap();
        assert!((p.at.x - 41.5).abs() < 1e-9 && (p.at.y - 8.5).abs() < 1e-9, "{p:?}");
        let s = seat(&board_with_holes(), &CaseSpec { inner_w: 50.0, inner_d: 70.0, ..Default::default() }).unwrap();
        assert_eq!(s.unplace(p.at), p.hole, "unplace undoes place");
    }

    /// A USB socket at the board's right edge gets an opening in the right wall, at its height above the floor; turned a quarter,
    /// the same edge faces the back wall. A part in the middle of the board gets none.
    #[test]
    fn a_connector_at_the_edge_gets_an_opening_in_the_wall_it_faces() {
        let mut b = board_with_holes();
        for (i, (x, y)) in [(57.5, 18.0), (57.5, 20.5), (57.5, 22.5), (57.5, 25.0)].iter().enumerate() {
            let mut p = Pad::new(Point::new(*x, *y), PadShape::Circle { d: 1.7 }, Some(1.0), Side::Top);
            p.refdes = "J1".into();
            p.pin = (i + 1).to_string();
            b.pads.push(p);
        }
        b.parts.push(Part { refdes: "J1".into(), value: String::new(), footprint: "USB_DEMO_TH".into(), pos: Point::new(57.5, 21.5), rot: 0, side: Side::Top });
        let mut r = Pad::new(Point::new(30.0, 20.0), PadShape::Circle { d: 1.7 }, Some(1.0), Side::Top);
        r.refdes = "R1".into();
        b.pads.push(r);
        b.parts.push(Part { refdes: "R1".into(), value: String::new(), footprint: "R_Axial_10.16".into(), pos: Point::new(30.0, 20.0), rot: 0, side: Side::Top });
        let spec = CaseSpec { inner_w: 70.0, inner_d: 50.0, bottom: 5.0, ..Default::default() };
        let ins = inside(&b, &spec);
        assert_eq!(ins.openings.len(), 1, "{:?}", ins.openings);
        let o = &ins.openings[0];
        assert_eq!((o.refdes.as_str(), o.wall), ("J1", Wall::Right));
        // above the board's top face (5 + 1.6), up to the 7 mm shell, with the margin; along the wall round y = 21.5 + 5
        assert!((o.up.0 - (5.0 + 1.6 - 0.5)).abs() < 1e-9 && (o.up.1 - (5.0 + 1.6 + 7.0 + 0.5)).abs() < 1e-9, "{o:?}");
        assert!(o.along.0 < 26.5 && o.along.1 > 26.5, "{o:?}");
        assert!(o.to_wall > 0.0 && o.to_wall < 10.0, "{o:?}");
        assert!(ins.notes.iter().any(|n| n.contains("J1")), "the socket is {:.1} mm from the wall: {:?}", o.to_wall, ins.notes);
        // in a case the board's size, the socket is near its wall and nothing is said
        let snug = inside(&b, &CaseSpec { inner_w: 62.0, inner_d: 42.0, bottom: 5.0, ..Default::default() });
        assert!(snug.openings[0].to_wall < 4.0 && snug.notes.is_empty(), "{snug:?}");
        let turned = inside(&b, &CaseSpec { inner_w: 50.0, inner_d: 70.0, bottom: 5.0, ..Default::default() });
        assert_eq!(turned.openings[0].wall, Wall::Back);
        // the drawing: the floor, the four walls, the posts and the opening
        let dxf = drawing(&b, &spec).unwrap();
        assert!(dxf.contains("OPENINGS") && dxf.contains("POSTS") && dxf.contains("M3 h5.0"));
        let circles = dxf.lines().collect::<Vec<_>>().windows(2).filter(|w| w[0] == "0" && w[1] == "CIRCLE").count();
        assert_eq!(circles, 8, "outside and pilot circle of four posts");
        assert!(drawing(&b, &CaseSpec::default()).is_err());
        assert!(dxf.is_ascii(), "an R12 DXF in ASCII only");
    }

    /// The USB board of the test above: the board pushed to the connector's wall (auto) brings the socket within the clearance of the
    /// right wall and says nothing more; pushed against the left and front walls by hand, it sits 0.5 mm from them.
    #[test]
    fn the_board_is_pushed_against_walls() {
        let mut b = board_with_holes();
        for (i, (x, y)) in [(57.5, 18.0), (57.5, 25.0)].iter().enumerate() {
            let mut p = Pad::new(Point::new(*x, *y), PadShape::Circle { d: 1.7 }, Some(1.0), Side::Top);
            p.refdes = "J1".into();
            p.pin = (i + 1).to_string();
            b.pads.push(p);
        }
        b.parts.push(Part { refdes: "J1".into(), value: String::new(), footprint: "USB_DEMO_TH".into(), pos: Point::new(57.5, 21.5), rot: 0, side: Side::Top });
        let base = CaseSpec { inner_w: 80.0, inner_d: 50.0, bottom: 5.0, ..Default::default() };
        let mid = inside(&b, &base);
        let auto = inside(&b, &CaseSpec { toward: Toward::Auto, ..base });
        assert_eq!(auto.pushed, (1, 0), "{auto:?}");
        assert!(auto.openings[0].to_wall < mid.openings[0].to_wall - 9.0, "{} vs {}", auto.openings[0].to_wall, mid.openings[0].to_wall);
        assert!(!auto.notes.iter().any(|n| n.contains("壁から離れた")), "{:?}", auto.notes);
        let corner = seat(&b, &CaseSpec { toward: Toward::Walls(-1, -1), ..base }).unwrap();
        let lo = b.bounds().unwrap().0;
        let p = corner.place(lo);
        assert!((p.x - 0.5).abs() < 1e-9 && (p.y - 0.5).abs() < 1e-9, "{p:?}");
        assert_eq!(Toward::parse("auto"), Ok(Toward::Auto));
        assert_eq!(Toward::parse("right,back"), Ok(Toward::Walls(1, 1)));
        assert!(Toward::parse("up").is_err());
    }

    /// A board without holes gets free places near its corners (clear of a part in one corner); the lid's posts stand on the board's
    /// top round the holes when the room above is given, and one that lands on a part says so; the drawing has the lid.
    #[test]
    fn holes_are_suggested_and_the_lid_has_posts() {
        let mut bare = Board::new("x", 60.0, 40.0);
        let mut pad = Pad::new(Point::new(5.0, 5.0), PadShape::Rect { w: 3.0, h: 3.0 }, None, Side::Top);
        pad.refdes = "U1".into();
        bare.pads.push(pad);
        bare.parts.push(Part { refdes: "U1".into(), value: String::new(), footprint: "x".into(), pos: Point::new(5.0, 5.0), rot: 0, side: Side::Top });
        let ins = inside(&bare, &CaseSpec { inner_w: 70.0, inner_d: 50.0, bottom: 5.0, ..Default::default() });
        assert_eq!(ins.suggested_holes.len(), 4, "{ins:?}");
        let near = ins.suggested_holes.iter().find(|p| p.x < 30.0 && p.y < 20.0).unwrap();
        assert!(near.x.max(near.y) > 6.0 + 3.0, "clear of the part in that corner: {near:?}");
        let far = ins.suggested_holes.iter().find(|p| p.x > 30.0 && p.y > 20.0).unwrap();
        assert!((far.x - 56.5).abs() < 1e-9 && (far.y - 36.5).abs() < 1e-9, "3.5 mm in from a free corner: {far:?}");
        assert!(ins.notes.iter().any(|n| n.contains("M3")), "{:?}", ins.notes);
        // the lid
        let mut b = board_with_holes();
        b.parts.push(Part { refdes: "C1".into(), value: String::new(), footprint: "x".into(), pos: Point::new(56.0, 36.0), rot: 0, side: Side::Top });
        let mut q = Pad::new(Point::new(56.0, 36.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        q.refdes = "C1".into();
        b.pads.push(q);
        let none = inside(&b, &CaseSpec { inner_w: 70.0, inner_d: 50.0, bottom: 5.0, ..Default::default() });
        assert!(none.lid_posts.is_empty(), "no room above given");
        let spec = CaseSpec { inner_w: 70.0, inner_d: 50.0, bottom: 5.0, top: 12.0, ..Default::default() };
        let ins = inside(&b, &spec);
        assert_eq!(ins.lid_posts.len(), 4);
        assert!(ins.lid_posts.iter().all(|p| p.height == 12.0));
        assert_eq!(ins.lid_posts.iter().filter(|p| p.problem.as_deref().is_some_and(|m| m.contains("C1"))).count(), 1, "{:?}", ins.lid_posts);
        let dxf = drawing(&b, &spec).unwrap();
        assert!(dxf.contains("LID (seen from inside)") && dxf.is_ascii());
    }

    /// A connector's mouth read from its mesh: a block with a bore open on +x faces +x, whichever edge of the board it is nearest; a
    /// solid block faces nowhere.
    #[test]
    fn a_meshs_mouth_is_read() {
        use crate::mesh::Mesh3d;
        // a box 0..8 x 0..6 x 0..5 with a square bore 4 x 3 from x = 8 to x = 2 (open on +x): its faces as quads
        let mut m = Mesh3d { palette: vec![[0, 0, 0]], ..Default::default() };
        let mut quad = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| {
            let k = m.vertices.len() as u32;
            m.vertices.extend([a, b, c, d]);
            m.triangles.extend([[k, k + 1, k + 2], [k, k + 2, k + 3]]);
            m.tri_color.extend([0, 0]);
        };
        let (w, d, h) = (8.0f32, 6.0f32, 5.0f32);
        quad([0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, d, 0.0], [0.0, d, 0.0]);
        quad([0.0, 0.0, h], [w, 0.0, h], [w, d, h], [0.0, d, h]);
        quad([0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, 0.0, h], [0.0, 0.0, h]);
        quad([0.0, d, 0.0], [w, d, 0.0], [w, d, h], [0.0, d, h]);
        quad([0.0, 0.0, 0.0], [0.0, d, 0.0], [0.0, d, h], [0.0, 0.0, h]);
        // the +x face with the bore's opening (y 1.5..4.5, z 1..4) cut out, and the bore's walls and bottom
        let (y0, y1, z0, z1) = (1.5f32, 4.5f32, 1.0f32, 4.0f32);
        quad([w, 0.0, 0.0], [w, y0, 0.0], [w, y0, h], [w, 0.0, h]);
        quad([w, y1, 0.0], [w, d, 0.0], [w, d, h], [w, y1, h]);
        quad([w, y0, 0.0], [w, y1, 0.0], [w, y1, z0], [w, y0, z0]);
        quad([w, y0, z1], [w, y1, z1], [w, y1, h], [w, y0, h]);
        quad([2.0, y0, z0], [2.0, y1, z0], [2.0, y1, z1], [2.0, y0, z1]);
        // the same box with its +x face whole (no bore)
        let mut solid = m.clone();
        solid.triangles.truncate(10);
        solid.tri_color.truncate(10);
        let k = solid.vertices.len() as u32;
        solid.vertices.extend([[w, 0.0, 0.0], [w, d, 0.0], [w, d, h], [w, 0.0, h]]);
        solid.triangles.extend([[k, k + 1, k + 2], [k, k + 2, k + 3]]);
        solid.tri_color.extend([0, 0]);
        assert_eq!(crate::model3d::mouth(&m), Some((1, 0)));
        assert_eq!(crate::model3d::mouth(&solid), None);
    }
}
