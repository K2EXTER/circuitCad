use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn dist(self, o: Point) -> f64 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

/// A face of the board, or (for copper only: tracks and zones) an inner copper layer.
/// `Inner(n)` counts from the top, 1-based; it is only valid up to `Board::inner_layers`.
/// Parts, pads and silkscreen are always `Top` or `Bottom`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Top,
    Bottom,
    Inner(u8),
}

impl Side {
    /// The other face. An inner layer has no other face, so it stays.
    pub fn flip(self) -> Side {
        match self {
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
            inner => inner,
        }
    }

    /// The face a part, pad or silkscreen goes on when this layer is being worked on (inner layers have none: the top).
    pub fn outer(self) -> Side {
        if self.is_inner() { Side::Top } else { self }
    }

    /// Name for the screen: 表面 / 内層1 / 裏面.
    pub fn label(self) -> String {
        match self {
            Side::Top => "表面".into(),
            Side::Bottom => "裏面".into(),
            Side::Inner(n) => format!("内層{n}"),
        }
    }

    pub fn is_inner(self) -> bool {
        matches!(self, Side::Inner(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PadShape {
    Circle { d: f64 },
    Rect { w: f64, h: f64 },
    Oval { w: f64, h: f64 },
}

impl PadShape {
    /// Bounding box size (w, h).
    pub fn size(&self) -> (f64, f64) {
        match *self {
            PadShape::Circle { d } => (d, d),
            PadShape::Rect { w, h } | PadShape::Oval { w, h } => (w, h),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub side: Side,
    pub a: Point,
    pub b: Point,
    pub width: f64,
}

/// A pad. `drill: Some(_)` makes it a plated through-hole pad present on both
/// copper layers; `drill: None` is an SMD pad on `side`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pad {
    pub pos: Point,
    pub shape: PadShape,
    pub drill: Option<f64>,
    /// With a drill: a plated slot `slot` long (and `drill` wide), along x or, with `slot_v`, along y. 0 = round hole.
    #[serde(default)]
    pub slot: f64,
    #[serde(default)]
    pub slot_v: bool,
    pub side: Side,
    /// Owning part (empty for a loose pad) and its pin name.
    #[serde(default)]
    pub refdes: String,
    #[serde(default)]
    pub pin: String,
    /// Net name from the schematic (empty = unassigned).
    #[serde(default)]
    pub net: String,
}

impl Pad {
    pub fn new(pos: Point, shape: PadShape, drill: Option<f64>, side: Side) -> Self {
        Self { pos, shape, drill, slot: 0.0, slot_v: false, side, refdes: String::new(), pin: String::new(), net: String::new() }
    }
}

/// A placed footprint instance; its pads and silkscreen live in `Board::pads` / `Board::silk`
/// tagged with the same `refdes`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Part {
    pub refdes: String,
    pub value: String,
    pub footprint: String,
    pub pos: Point,
    /// Quarter turns counter-clockwise (0..=3), as seen from the top.
    pub rot: u8,
    /// Mounted on the top or the bottom of the board. A bottom part is mirrored left-to-right
    /// (the board is turned over about its vertical axis).
    #[serde(default = "default_top")]
    pub side: Side,
}

fn default_top() -> Side {
    Side::Top
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Via {
    pub pos: Point,
    pub diameter: f64,
    pub drill: f64,
}

/// Non-plated hole: a mounting screw hole, or a locating peg hole that belongs to a part (`refdes` set).
/// With `slot` > `drill` it is a slot (a routed hole, `slot` long and `drill` wide), along x or, with `slot_v`, along y.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hole {
    pub pos: Point,
    pub drill: f64,
    #[serde(default)]
    pub slot: f64,
    #[serde(default)]
    pub slot_v: bool,
    /// Owning part (empty for a loose hole). A part's holes follow it when it moves or turns.
    #[serde(default)]
    pub refdes: String,
}

impl Hole {
    pub fn new(pos: Point, drill: f64) -> Self {
        Self { pos, drill, slot: 0.0, slot_v: false, refdes: String::new() }
    }

    pub fn is_slot(&self) -> bool {
        self.slot > self.drill + 1e-9
    }

    /// The two ends of the slot's centre line (both the centre for a round hole).
    pub fn ends(&self) -> (Point, Point) {
        slot_ends(self.pos, self.drill, self.slot, self.slot_v)
    }
}

/// Centres of the two round ends of a slot `slot` long and `drill` wide at `pos` (both `pos` when it is round).
pub fn slot_ends(pos: Point, drill: f64, slot: f64, vertical: bool) -> (Point, Point) {
    let half = ((slot - drill) / 2.0).max(0.0);
    if vertical { (Point::new(pos.x, pos.y - half), Point::new(pos.x, pos.y + half)) } else { (Point::new(pos.x - half, pos.y), Point::new(pos.x + half, pos.y)) }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SilkLine {
    pub side: Side,
    pub a: Point,
    pub b: Point,
    pub width: f64,
    #[serde(default)]
    pub refdes: String,
}

/// Silkscreen text. Texts with a `refdes` belong to a part (generated, moves with it).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SilkText {
    pub side: Side,
    /// Centre of the text.
    pub pos: Point,
    pub height: f64,
    pub text: String,
    #[serde(default)]
    pub refdes: String,
}

fn default_zone_clearance() -> f64 {
    0.3
}

fn default_true() -> bool {
    true
}

fn default_thermal_gap() -> f64 {
    0.3
}

fn default_spoke() -> f64 {
    0.3
}

/// A copper pour. It is output as a filled region with clear-polarity cut-outs around every
/// item of another net, so it follows the routing automatically.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Zone {
    pub net: String,
    pub side: Side,
    pub polygon: Vec<Point>,
    #[serde(default = "default_zone_clearance")]
    pub clearance: f64,
    /// Connect through-hole pads of the zone's net with four spokes instead of solid copper.
    #[serde(default = "default_true")]
    pub thermal: bool,
    #[serde(default = "default_thermal_gap")]
    pub thermal_gap: f64,
    #[serde(default = "default_spoke")]
    pub spoke_width: f64,
    /// Drop pieces of the pour that touch no pad, via or track of its net (isolated fragments).
    #[serde(default)]
    pub remove_islands: bool,
}

impl Zone {
    pub fn new(net: &str, side: Side, polygon: Vec<Point>) -> Self {
        Self {
            net: net.to_string(),
            side,
            polygon,
            clearance: default_zone_clearance(),
            thermal: true,
            thermal_gap: default_thermal_gap(),
            spoke_width: default_spoke(),
            remove_islands: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub min_track_width: f64,
    pub min_clearance: f64,
    /// Minimum copper-to-board-edge distance.
    pub edge_clearance: f64,
    pub min_drill: f64,
    pub min_annular_ring: f64,
    pub mask_expansion: f64,
    pub outline_width: f64,
}

impl Default for Rules {
    fn default() -> Self {
        // Conservative values that most low-cost 2-layer fabs accept.
        Self {
            min_track_width: 0.15,
            min_clearance: 0.15,
            edge_clearance: 0.3,
            min_drill: 0.3,
            min_annular_ring: 0.13,
            mask_expansion: 0.05,
            outline_width: 0.1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Board {
    pub name: String,
    /// Closed polygon (last point implicitly connects to first).
    pub outline: Vec<Point>,
    pub tracks: Vec<Track>,
    pub pads: Vec<Pad>,
    pub vias: Vec<Via>,
    pub holes: Vec<Hole>,
    pub silk: Vec<SilkLine>,
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub texts: Vec<SilkText>,
    #[serde(default)]
    pub zones: Vec<Zone>,
    /// User-defined footprints used by this board (embedded so the file is self-contained).
    #[serde(default)]
    pub footprints: Vec<crate::footprints::Footprint>,
    /// Current (A) each net is meant to carry, set by the user: the track widths are checked against it.
    #[serde(default)]
    pub net_currents: std::collections::BTreeMap<String, f64>,
    /// Parts (by reference designator) that were fixed in place: the editor does not move, turn or flip them.
    #[serde(default)]
    pub locked_parts: Vec<String>,
    /// Number of inner copper layers (0 = a two-layer board). The copper layers are
    /// `Top`, `Inner(1)` .. `Inner(inner_layers)`, `Bottom`.
    #[serde(default)]
    pub inner_layers: u8,
    /// Differential pairs: two nets routed side by side with the same length.
    #[serde(default)]
    pub diff_pairs: Vec<crate::diffpair::DiffPair>,
    /// Finishing of the copper (rounded corners, teardrops ...) applied when the board is shown and made, not to the tracks.
    #[serde(default)]
    pub finish: crate::smooth::Finish,
    /// Nets whose tracks the finishing leaves as they are (a controlled-impedance line, a differential pair ...).
    #[serde(default)]
    pub finish_exclude: Vec<String>,
    /// Shop part numbers of library parts (reference designator -> shop id -> number), taken from the schematic on import.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub part_shops: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// Where the user moved a part's silkscreen designator to: reference designator -> offset (mm) from the automatic spot.
    /// The text follows its part and keeps the offset; an entry is removed when the text is put back.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub label_offsets: std::collections::BTreeMap<String, Point>,
    pub rules: Rules,
}

impl Board {
    /// All copper layers from the top face to the bottom face.
    pub fn copper_layers(&self) -> Vec<Side> {
        let mut v = vec![Side::Top];
        v.extend((1..=self.inner_layers).map(Side::Inner));
        v.push(Side::Bottom);
        v
    }

    pub fn is_locked(&self, refdes: &str) -> bool {
        self.locked_parts.iter().any(|r| r == refdes)
    }

    pub fn new(name: &str, width: f64, height: f64) -> Self {
        Self {
            name: name.to_string(),
            outline: vec![
                Point::new(0.0, 0.0),
                Point::new(width, 0.0),
                Point::new(width, height),
                Point::new(0.0, height),
            ],
            tracks: vec![],
            pads: vec![],
            vias: vec![],
            holes: vec![],
            silk: vec![],
            parts: vec![],
            texts: vec![],
            zones: vec![],
            footprints: vec![],
            net_currents: Default::default(),
            locked_parts: vec![],
            inner_layers: 0,
            diff_pairs: vec![],
            finish: Default::default(),
            finish_exclude: vec![],
            part_shops: Default::default(),
            label_offsets: Default::default(),
            rules: Rules::default(),
        }
    }

    /// Replace the outline by an axis-aligned rectangle.
    pub fn set_rect_outline(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.outline = vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)];
    }

    /// Moves pour `i` to the front (drawn last, so it wins where pours of other nets overlap) or to the back.
    /// Returns its new index.
    pub fn move_zone(&mut self, i: usize, to_front: bool) -> usize {
        if i >= self.zones.len() {
            return i;
        }
        let z = self.zones.remove(i);
        if to_front {
            self.zones.push(z);
            self.zones.len() - 1
        } else {
            self.zones.insert(0, z);
            0
        }
    }

    pub fn bounds(&self) -> Option<(Point, Point)> {
        let mut it = self.outline.iter();
        let first = *it.next()?;
        let (mut lo, mut hi) = (first, first);
        for p in it {
            lo.x = lo.x.min(p.x);
            lo.y = lo.y.min(p.y);
            hi.x = hi.x.max(p.x);
            hi.y = hi.y.max(p.y);
        }
        Some((lo, hi))
    }
}
