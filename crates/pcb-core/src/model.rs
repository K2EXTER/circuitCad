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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Top,
    Bottom,
}

impl Side {
    pub fn flip(self) -> Side {
        match self {
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
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
        Self { pos, shape, drill, side, refdes: String::new(), pin: String::new(), net: String::new() }
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

/// Non-plated mounting hole.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hole {
    pub pos: Point,
    pub drill: f64,
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
    pub rules: Rules,
}

impl Board {
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
            rules: Rules::default(),
        }
    }

    /// Replace the outline by an axis-aligned rectangle.
    pub fn set_rect_outline(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.outline = vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)];
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
