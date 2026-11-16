//! The board's temperature map (candidate H7): the board as a thin plate cut into square cells, one sheet of cells per copper layer.
//!
//! * In each layer's sheet, heat runs between neighbouring cells through the copper that covers the cell (pours as filled, tracks,
//!   pads, vias; `k` 390 W/m·K) and a share of the FR-4 (0.8 W/m·K along the board).
//! * Between the layers it crosses the FR-4 (0.3 W/m·K through the board) and the plated barrels of the vias.
//! * The outer faces give heat to the air by natural convection and radiation, with the heat transfer coefficient `h` worked out
//!   cell by cell from the cell's own temperature rise (`h_air`): air as an ideal gas at the board's altitude, the flat plate laws
//!   (`Nu = 0.54 Ra^¼` for a hot face upwards, `0.27 Ra^¼` downwards, `0.59 Ra^¼` standing), radiation `εσ(T⁴ − T∞⁴)` linearised.
//!   As `h` follows the temperature, the map and `h` are solved again in turn until they agree.
//! * The heat of a part goes into the copper under its pads, shared by pad area.
//!
//! Solved with conjugate gradients (Jacobi preconditioned) on the sparse system; about 60 000 unknowns for a 100 × 80 mm two-layer
//! board at 0.5 mm. Limits: no forced air, no neighbours' shading, the part's own body to the air is not here (it stays in θ), the FR-4
//! is one material and the inner layers sit evenly through the thickness. A guide, ±20–30 %.

use crate::model::*;
use crate::zonefill::{self, FillShape};

/// Thermal conductivities (W/m·K).
const K_CU: f64 = 390.0;
const K_FR4_PLANE: f64 = 0.8;
const K_FR4_THROUGH: f64 = 0.3;
/// Board thickness (mm) and via plating (mm).
const BOARD_MM: f64 = 1.6;
const PLATING_MM: f64 = 0.025;
const STEFAN: f64 = 5.670e-8;
/// Heat capacity per volume (J/m³·K): copper 8.96 g/cm³ × 0.385 J/g·K, FR-4 1.85 g/cm³ × 1.1 J/g·K.
const RHO_C_CU: f64 = 8.96e6 * 0.385;
const RHO_C_FR4: f64 = 1.85e6 * 1.1;
const G: f64 = 9.81;
/// Most nodes over all the copper layers: the pitch grows from 0.5 mm on big boards and boards of many layers to stay near this (a
/// 100 × 80 mm two-layer board keeps 0.5 mm; four layers of 100 × 76 mm get about 0.7 mm). Between 0.5 and 1 mm the answers move by
/// about 1.5 %, and the time goes with the nodes.
const MAX_NODES: f64 = 64_000.0;

/// How the board stands in the air.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Orientation {
    /// Lying flat, the top face upwards (on a desk, in a flat box).
    #[default]
    Flat,
    /// Standing on an edge (in a rack, on a wall); the height is the board's height in y.
    Standing,
}

/// What the map is worked out for.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ThermalSpec {
    /// The air around the board (°C).
    pub ambient_c: f64,
    pub orientation: Orientation,
    /// Height above the sea (m): thinner air carries less heat away.
    pub altitude_m: f64,
    /// How well the faces radiate (solder mask about 0.9, bare copper 0.05).
    pub emissivity: f64,
    /// The side of a cell (mm); 0 = 0.5 mm, or coarser on a big board.
    pub cell_mm: f64,
    /// In a case: the room from the top face to the lid and from the bottom face to the floor (mm; 0 = open air). Across a thin gap
    /// the still air carries the heat by conduction, k / gap, when that beats convection.
    pub gap_top_mm: f64,
    pub gap_bottom_mm: f64,
}

impl Default for ThermalSpec {
    fn default() -> Self {
        Self { ambient_c: 25.0, orientation: Orientation::Flat, altitude_m: 0.0, emissivity: 0.9, cell_mm: 0.0, gap_top_mm: 0.0, gap_bottom_mm: 0.0 }
    }
}

/// The air pressure (Pa) at an altitude (m), standard atmosphere.
pub fn pressure_at(altitude_m: f64) -> f64 {
    101_325.0 * (1.0 - 2.255_77e-5 * altitude_m.clamp(-500.0, 11_000.0)).powf(5.255_88)
}

/// The heat transfer coefficient (W/m²·K) from a face `rise_k` above the air at `ambient_c`: natural convection on a plate of
/// characteristic length `length_m` (a flat face: area / perimeter; a standing one: its height), `up` for a hot face upwards (`None`
/// for a standing one), plus radiation with `emissivity`, air at `pressure_pa`. Returns (convection, radiation).
pub fn h_air(rise_k: f64, ambient_c: f64, length_m: f64, up: Option<bool>, emissivity: f64, pressure_pa: f64) -> (f64, f64) {
    let dt = rise_k.max(0.05);
    let ta = ambient_c + 273.15;
    let film = ta + dt / 2.0;
    // air as an ideal gas: the density goes as P / T, so the kinematic viscosity and diffusivity as T^1.75 / P (μ ~ T^0.75)
    let nu = 1.57e-5 * (film / 300.0).powf(1.75) * (101_325.0 / pressure_pa);
    let alpha = nu / 0.71;
    let k = 0.0263 * (film / 300.0).powf(0.8);
    let beta = 1.0 / film;
    let l = length_m.max(1e-4);
    let ra = G * beta * dt * l.powi(3) / (nu * alpha);
    let nu_number = match up {
        Some(true) if ra > 1e7 => 0.15 * ra.powf(1.0 / 3.0),
        Some(true) => 0.54 * ra.powf(0.25),
        Some(false) => 0.27 * ra.powf(0.25),
        None if ra > 1e9 => 0.10 * ra.powf(1.0 / 3.0),
        None => 0.59 * ra.powf(0.25),
    };
    let ts = ta + dt;
    let rad = emissivity * STEFAN * (ts * ts + ta * ta) * (ts + ta);
    (nu_number * k / l, rad)
}

/// The grid: cell (i, j) has its middle at `(x0 + (i + ½)·d, y0 + (j + ½)·d)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub x0: f64,
    pub y0: f64,
    pub d: f64,
    pub w: usize,
    pub h: usize,
    /// Whether the cell's middle is on the board.
    pub inside: Vec<bool>,
}

impl Grid {
    pub fn centre(&self, c: usize) -> Point {
        Point::new(self.x0 + ((c % self.w) as f64 + 0.5) * self.d, self.y0 + ((c / self.w) as f64 + 0.5) * self.d)
    }

    /// The cell a point falls in.
    pub fn cell_at(&self, p: Point) -> Option<usize> {
        let (i, j) = (((p.x - self.x0) / self.d).floor(), ((p.y - self.y0) / self.d).floor());
        (i >= 0.0 && j >= 0.0 && (i as usize) < self.w && (j as usize) < self.h).then(|| j as usize * self.w + i as usize)
    }
}

/// A part's heat on the board.
#[derive(Clone, Debug, PartialEq)]
pub struct PartHeat {
    pub refdes: String,
    pub power: f64,
    /// The junction (or element) of the part (°C).
    pub junction: f64,
    /// The copper under its pads: the mean and the hottest temperature (°C).
    pub pad_temp: f64,
    pub pad_max: f64,
}

/// The worked out map.
#[derive(Clone, Debug, PartialEq)]
pub struct ThermalMap {
    pub grid: Grid,
    pub layers: Vec<Side>,
    /// Temperature (°C) per layer (in `layers` order), per cell; `NaN` off the board.
    pub temp: Vec<Vec<f64>>,
    pub ambient_c: f64,
    pub parts: Vec<PartHeat>,
    /// The heat transfer coefficients at the end (W/m²·K, convection + radiation), the mean over the top face and the bottom face.
    pub h_top: f64,
    pub h_bottom: f64,
    /// Rounds of map-and-h.
    pub rounds: usize,
    /// The heat of the wiring itself put into the map (W, `TraceHeat`).
    pub trace_power: f64,
}

/// The heat of a pad's wiring itself (its I²R, W), put into the copper along its way (`irdrop::PadSeries::path`; the pieces through
/// vias are left out), by length, on the layer of each piece's height.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceHeat {
    pub path: Vec<crate::irdrop::PathSeg>,
    pub power: f64,
}

impl ThermalMap {
    /// The hottest cell (°C) and where it is.
    pub fn hottest(&self) -> (f64, Point) {
        let mut best = (f64::NEG_INFINITY, Point::new(0.0, 0.0));
        for t in &self.temp {
            for (c, &v) in t.iter().enumerate() {
                if v > best.0 {
                    best = (v, self.grid.centre(c));
                }
            }
        }
        best
    }

    /// The temperature (°C) at a point on a layer, if it is on the board.
    pub fn at(&self, side: Side, p: Point) -> Option<f64> {
        let l = self.layers.iter().position(|s| *s == side)?;
        self.grid.cell_at(p).map(|c| self.temp[l][c]).filter(|v| v.is_finite())
    }
}

/// The physical model of the board: what conducts where, without the heat sources or the air.
pub(crate) struct Model {
    pub grid: Grid,
    pub layers: Vec<Side>,
    /// Node of (layer, cell), `usize::MAX` off the board.
    node: Vec<usize>,
    /// The cell of each node and its layer.
    cell_of: Vec<(usize, usize)>,
    /// Conductances (W/K) between two nodes.
    edges: Vec<(usize, usize, f64)>,
    /// Characteristic length (m) of the faces for the convection law.
    length_m: f64,
    /// The board's sheet conductance (W/K across a square, every layer added), the mean over the board: how well it spreads heat.
    mean_sheet: f64,
    /// Heat capacity (J/K) of each node: its copper and its share of the FR-4.
    cap: Vec<f64>,
}

impl Model {
    pub fn nodes(&self) -> usize {
        self.cell_of.len()
    }
}

/// Polygons as rows of covered subsamples: `mask[row][col]`, `s` subsamples per cell side, OR-ed in. Even-odd over the shape's
/// own contours (outer and holes).
fn paint(mask: &mut [bool], grid: &Grid, s: usize, shape: &FillShape) {
    let (sw, sh) = (grid.w * s, grid.h * s);
    let sd = grid.d / s as f64;
    let contours: Vec<&Vec<Point>> = std::iter::once(&shape.outer).chain(shape.holes.iter()).collect();
    let (ylo, yhi) = shape.outer.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    let r0 = (((ylo - grid.y0) / sd) - 0.5).floor().max(0.0) as usize;
    let r1 = ((((yhi - grid.y0) / sd) - 0.5).ceil().max(0.0) as usize + 1).min(sh);
    let mut xs: Vec<f64> = vec![];
    for r in r0..r1 {
        let y = grid.y0 + (r as f64 + 0.5) * sd;
        xs.clear();
        for c in &contours {
            let n = c.len();
            for k in 0..n {
                let (a, b) = (c[k], c[(k + 1) % n]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks_exact(2) {
            let c0 = (((pair[0] - grid.x0) / sd) - 0.5).ceil().max(0.0) as usize;
            let c1 = ((((pair[1] - grid.x0) / sd) - 0.5).floor() + 1.0).max(0.0) as usize;
            for col in c0..c1.min(sw) {
                mask[r * sw + col] = true;
            }
        }
    }
}

/// The copper pieces of each layer: pours as filled, tracks, pads (through-hole pads on every layer) and vias (every layer).
fn copper_pieces(board: &Board, layers: &[Side]) -> Vec<Vec<FillShape>> {
    let mut out: Vec<Vec<FillShape>> = vec![vec![]; layers.len()];
    let at = |s: Side| layers.iter().position(|l| *l == s);
    let conn = crate::drc::connectivity(board);
    for (side, _, fills) in zonefill::merged_fills(board, &conn) {
        if let Some(l) = at(side) {
            out[l].extend(fills);
        }
    }
    for t in &board.tracks {
        if let Some(l) = at(t.side) {
            out[l].push(zonefill::track_piece(t.a, t.b, t.width));
        }
    }
    for p in &board.pads {
        let piece = zonefill::pad_piece(p.pos, p.shape);
        if p.drill.is_some() {
            for o in out.iter_mut() {
                o.push(piece.clone());
            }
        } else if let Some(l) = at(p.side) {
            out[l].push(piece);
        }
    }
    for v in &board.vias {
        for o in out.iter_mut() {
            o.push(zonefill::via_piece(v.pos, v.diameter));
        }
    }
    out
}

/// The grid and the conductances of a board, its copper `copper_um` thick.
pub(crate) fn model(board: &Board, copper_um: f64, cell_mm: f64) -> Result<Model, String> {
    if board.outline.len() < 3 {
        return Err("基板の外形がありません".into());
    }
    let (lo, hi) = board.outline.iter().fold((Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN)), |(a, b), p| (Point::new(a.x.min(p.x), a.y.min(p.y)), Point::new(b.x.max(p.x), b.y.max(p.y))));
    let (wmm, hmm) = (hi.x - lo.x, hi.y - lo.y);
    if !(wmm > 0.0 && hmm > 0.0) {
        return Err("基板の外形に広さがありません".into());
    }
    let d = if cell_mm > 0.0 { cell_mm } else { 0.5f64.max((wmm * hmm * board.copper_layers().len() as f64 / MAX_NODES).sqrt()) };
    let (w, h) = ((wmm / d).ceil() as usize, (hmm / d).ceil() as usize);
    let mut grid = Grid { x0: lo.x, y0: lo.y, d, w, h, inside: vec![false; w * h] };
    for c in 0..w * h {
        grid.inside[c] = crate::geom::inside_polygon(&board.outline, grid.centre(c));
    }
    // drilled holes (mounting holes) take the board away
    for hole in &board.holes {
        for c in 0..w * h {
            if grid.inside[c] && grid.centre(c).dist(hole.pos) < hole.drill / 2.0 {
                grid.inside[c] = false;
            }
        }
    }
    let layers = board.copper_layers();
    let n_layers = layers.len();
    // copper coverage of every cell, from 3 × 3 subsamples
    const S: usize = 3;
    let pieces = copper_pieces(board, &layers);
    let cover: Vec<Vec<f64>> = crate::par::par_map(&pieces, |shapes| {
        let mut mask = vec![false; w * S * h * S];
        for s in shapes {
            paint(&mut mask, &grid, S, s);
        }
        (0..w * h)
            .map(|c| {
                let (i, j) = (c % w, c / w);
                let mut n = 0;
                for a in 0..S {
                    for b in 0..S {
                        n += mask[(j * S + a) * w * S + i * S + b] as usize;
                    }
                }
                n as f64 / (S * S) as f64
            })
            .collect()
    });
    // nodes
    let mut node = vec![usize::MAX; n_layers * w * h];
    let mut cell_of = vec![];
    let mut cap = vec![];
    let cell_area = (d * 1e-3) * (d * 1e-3);
    for l in 0..cover.len() {
        for c in 0..w * h {
            if grid.inside[c] {
                node[l * w * h + c] = cell_of.len();
                cell_of.push((l, c));
                cap.push(cell_area * (cover[l][c] * copper_um * 1e-6 * RHO_C_CU + BOARD_MM * 1e-3 / n_layers as f64 * RHO_C_FR4));
            }
        }
    }
    // sheet conductance (W/K across a square) of a node: its copper and its share of the FR-4
    let t_cu = copper_um * 1e-6;
    let t_fr4 = BOARD_MM * 1e-3 / n_layers as f64;
    let sheet = |f: f64| f * K_CU * t_cu + K_FR4_PLANE * t_fr4;
    let mut edges = vec![];
    for (l, cov) in cover.iter().enumerate() {
        for j in 0..h {
            for i in 0..w {
                let c = j * w + i;
                let a = node[l * w * h + c];
                if a == usize::MAX {
                    continue;
                }
                for (ni, nj) in [(i + 1, j), (i, j + 1)] {
                    if ni >= w || nj >= h {
                        continue;
                    }
                    let nc = nj * w + ni;
                    let b = node[l * w * h + nc];
                    if b == usize::MAX {
                        continue;
                    }
                    // two half cells in series
                    let (sa, sb) = (sheet(cov[c]), sheet(cov[nc]));
                    edges.push((a, b, 2.0 * sa * sb / (sa + sb)));
                }
            }
        }
    }
    // through the board: FR-4 between neighbouring layers, and the via barrels
    if n_layers > 1 {
        let gap = BOARD_MM * 1e-3 / (n_layers - 1) as f64;
        let area = (d * 1e-3) * (d * 1e-3);
        let mut barrel = vec![0.0; w * h];
        for v in &board.vias {
            if let Some(c) = grid.cell_at(v.pos) {
                barrel[c] += K_CU * std::f64::consts::PI * (v.drill.max(0.2) * 1e-3) * (PLATING_MM * 1e-3) / gap;
            }
        }
        for p in board.pads.iter().filter(|p| p.drill.is_some()) {
            if let Some(c) = grid.cell_at(p.pos) {
                barrel[c] += K_CU * std::f64::consts::PI * (p.drill.unwrap_or(0.8) * 1e-3) * (PLATING_MM * 1e-3) / gap;
            }
        }
        for l in 0..n_layers - 1 {
            for c in 0..w * h {
                let (a, b) = (node[l * w * h + c], node[(l + 1) * w * h + c]);
                if a != usize::MAX && b != usize::MAX {
                    edges.push((a, b, K_FR4_THROUGH * area / gap + barrel[c]));
                }
            }
        }
    }
    let inside_area: f64 = grid.inside.iter().filter(|x| **x).count() as f64 * d * d;
    let perimeter: f64 = (0..board.outline.len()).map(|k| board.outline[k].dist(board.outline[(k + 1) % board.outline.len()])).sum();
    let inside_cells = grid.inside.iter().filter(|x| **x).count().max(1) as f64;
    let mean_sheet = cover.iter().map(|cov| (0..w * h).filter(|c| grid.inside[*c]).map(|c| sheet(cov[c])).sum::<f64>() / inside_cells).sum();
    Ok(Model { grid, layers, node, cell_of, edges, length_m: inside_area / perimeter.max(1e-9) * 1e-3, mean_sheet, cap })
}

/// The grid nodes under a part's pads, each with its share (by count of covered cells, adding to 1).
fn pad_cells(board: &Board, m: &Model, refdes: &str) -> Vec<(usize, f64)> {
    let (w, h) = (m.grid.w, m.grid.h);
    {
        let mut cells: Vec<(usize, f64)> = vec![];
        for pad in board.pads.iter().filter(|x| x.refdes == refdes) {
            let piece = zonefill::pad_piece(pad.pos, pad.shape);
            let (lo, hi) = piece.outer.iter().fold((Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN)), |(a, b), p| (Point::new(a.x.min(p.x), a.y.min(p.y)), Point::new(b.x.max(p.x), b.y.max(p.y))));
            let layers: Vec<usize> = if pad.drill.is_some() { (0..m.layers.len()).collect() } else { m.layers.iter().position(|s| *s == pad.side).into_iter().collect() };
            let mut hit = vec![];
            let i0 = ((lo.x - m.grid.x0) / m.grid.d).floor().max(0.0) as usize;
            let j0 = ((lo.y - m.grid.y0) / m.grid.d).floor().max(0.0) as usize;
            let i1 = (((hi.x - m.grid.x0) / m.grid.d).ceil().max(0.0) as usize).min(w);
            let j1 = (((hi.y - m.grid.y0) / m.grid.d).ceil().max(0.0) as usize).min(h);
            for j in j0..j1 {
                for i in i0..i1 {
                    let c = j * w + i;
                    if crate::geom::inside_polygon(&piece.outer, m.grid.centre(c)) {
                        hit.push(c);
                    }
                }
            }
            if hit.is_empty() {
                hit.extend(m.grid.cell_at(pad.pos));
            }
            for c in hit {
                for &l in &layers {
                    let n = m.node[l * w * h + c];
                    if n != usize::MAX {
                        cells.push((n, 1.0));
                    }
                }
            }
        }
        let total = cells.len() as f64;
        for (_, f) in cells.iter_mut() {
            *f /= total;
        }
        cells
    }
}

/// The off-diagonal conductances of the system, row by row (compressed rows).
pub(crate) struct Csr {
    start: Vec<usize>,
    col: Vec<u32>,
    g: Vec<f64>,
    /// The sum of the conductances of each row (the diagonal without the air).
    pub sum: Vec<f64>,
    /// Where each node is, for the multigrid preconditioner (`Place`); empty = Jacobi alone.
    pub place: Vec<Place>,
}

/// Where a node is: a cell of a layer (column, row), or a part's junction (its own, never merged with another).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Place {
    Cell { layer: u8, i: u32, j: u32 },
    Junction(u32),
}

impl Csr {
    pub fn new(n: usize, edges: &[(usize, usize, f64)]) -> Csr {
        let mut count = vec![0usize; n + 1];
        for &(a, b, _) in edges {
            count[a + 1] += 1;
            count[b + 1] += 1;
        }
        for i in 0..n {
            count[i + 1] += count[i];
        }
        let mut fill = count.clone();
        let mut col = vec![0u32; count[n]];
        let mut g = vec![0.0; count[n]];
        let mut sum = vec![0.0; n];
        for &(a, b, w) in edges {
            for (x, y) in [(a, b), (b, a)] {
                col[fill[x]] = y as u32;
                g[fill[x]] = w;
                fill[x] += 1;
                sum[x] += w;
            }
        }
        Csr { start: count, col, g, sum, place: vec![] }
    }

    /// `out = diag·v − offdiag·v`.
    fn apply(&self, diag: &[f64], v: &[f64], out: &mut [f64]) {
        spmv(&self.start, &self.col, &self.g, diag, v, out);
    }
}

/// `out = diag·v − offdiag·v` for a matrix in compressed rows.
fn spmv(start: &[usize], col: &[u32], g: &[f64], diag: &[f64], v: &[f64], out: &mut [f64]) {
    for i in 0..diag.len() {
        let mut s = diag[i] * v[i];
        for k in start[i]..start[i + 1] {
            s -= g[k] * v[col[k] as usize];
        }
        out[i] = s;
    }
}

/// One level of the multigrid: its matrix (`diag` and the conductances between nodes, rows compressed) and how its nodes merge into the
/// next level's (`up`).
struct Level {
    diag: Vec<f64>,
    start: Vec<usize>,
    col: Vec<u32>,
    g: Vec<f64>,
    /// The coarser node each node of this level merges into (empty on the coarsest).
    up: Vec<u32>,
    /// On a coarser level: the conductances inside each merged node (taken from the sum of its parts' diagonals).
    inner: Vec<f64>,
}

impl Level {
    fn n(&self) -> usize {
        self.diag.len()
    }

    /// `out = A·v`.
    fn apply(&self, v: &[f64], out: &mut [f64]) {
        spmv(&self.start, &self.col, &self.g, &self.diag, v, out);
    }
}

/// The multigrid preconditioner: the cells of each layer merged 2 × 2 into coarser cells, level after level (a part's junction stays a
/// node of its own), until a few hundred nodes are left, which are solved directly (Cholesky). One V-cycle per CG step: damped Jacobi
/// sweeps smooth the error from cell to cell on each level, and the coarser levels take out what spreads over the board (heat that has
/// to cross the copper to reach the air far away), which plain Jacobi needs hundreds of steps for. The coarse matrices are `Pᵀ A P`
/// with P the piecewise-constant spreading of a merged node to its parts; with as many sweeps before as after, the V-cycle is symmetric
/// and positive definite, so CG still applies, and the steps needed hardly grow with the board.
struct Multigrid {
    levels: Vec<Level>,
    /// The coarsest matrix factored (L, row by row).
    chol: Vec<f64>,
}

/// Damping and number of the Jacobi sweeps before and after the coarser levels, and how much of the coarse correction is taken (more
/// than all of it: a merged node spreads its value evenly, which falls short of a smooth error). Chosen on three boards (thermal_bench,
/// thermal_bench_board): one sweep and 1.6 were as quick as any other choice tried, within the ±20 % the timings move from run to run.
const MG_OMEGA: f64 = 0.7;
const MG_SWEEPS: usize = 1;
const MG_OVER: f64 = 1.6;
/// The coarsest level is solved directly once it has at most this many nodes (its factor is redone on every refresh).
const MG_COARSEST: usize = 200;
/// The multigrid is used from this many nodes (about 100 × 80 mm of one layer at 1 mm, or 40 × 50 mm at 0.5 mm).
const MG_FROM: usize = 16_000;

/// The dense Cholesky factor (L, row by row) of a level's matrix; `None` if it is not positive definite.
fn factor(level: &Level) -> Option<Vec<f64>> {
    let nc = level.n();
    let mut c = vec![0.0; nc * nc];
    for i in 0..nc {
        c[i * nc + i] += level.diag[i];
        for k in level.start[i]..level.start[i + 1] {
            c[i * nc + level.col[k] as usize] -= level.g[k];
        }
    }
    for j in 0..nc {
        let mut d = c[j * nc + j];
        for k in 0..j {
            d -= c[j * nc + k] * c[j * nc + k];
        }
        if !(d > 0.0) {
            return None;
        }
        let d = d.sqrt();
        c[j * nc + j] = d;
        for i in j + 1..nc {
            let mut v = c[i * nc + j];
            for k in 0..j {
                v -= c[i * nc + k] * c[j * nc + k];
            }
            c[i * nc + j] = v / d;
        }
    }
    Some(c)
}

impl Multigrid {
    fn new(a: &Csr, diag: &[f64]) -> Option<Multigrid> {
        // a small grid: Jacobi alone is quick, and building the levels would cost more than it saves
        if a.place.len() != diag.len() || diag.len() < MG_FROM {
            return None;
        }
        let mut place = a.place.clone();
        let mut level = Level { diag: diag.to_vec(), start: a.start.clone(), col: a.col.clone(), g: a.g.clone(), up: vec![], inner: vec![] };
        let mut levels = vec![];
        while level.n() > MG_COARSEST {
            // merge: cells 2 × 2 within a layer, junctions alone
            let mut index: std::collections::HashMap<Place, u32> = std::collections::HashMap::new();
            let mut coarse_place = vec![];
            let up: Vec<u32> = place
                .iter()
                .map(|p| {
                    let key = match *p {
                        Place::Cell { layer, i, j } => Place::Cell { layer, i: i / 2, j: j / 2 },
                        junction => junction,
                    };
                    *index.entry(key).or_insert_with(|| {
                        coarse_place.push(key);
                        (coarse_place.len() - 1) as u32
                    })
                })
                .collect();
            let nc = coarse_place.len();
            if nc as f64 > 0.9 * level.n() as f64 {
                break; // nothing left to merge (all junctions)
            }
            // Pᵀ A P: the diagonal takes the node's own and loses the conductances inside a merged node
            let mut cdiag = vec![0.0; nc];
            let mut inner = vec![0.0; nc];
            let mut rows: Vec<std::collections::HashMap<u32, f64>> = vec![std::collections::HashMap::new(); nc];
            for i in 0..level.n() {
                let ci = up[i];
                cdiag[ci as usize] += level.diag[i];
                for k in level.start[i]..level.start[i + 1] {
                    let cj = up[level.col[k] as usize];
                    if cj == ci {
                        cdiag[ci as usize] -= level.g[k];
                        inner[ci as usize] += level.g[k];
                    } else {
                        *rows[ci as usize].entry(cj).or_insert(0.0) += level.g[k];
                    }
                }
            }
            let mut start = vec![0usize; nc + 1];
            let (mut col, mut g) = (vec![], vec![]);
            for (r, row) in rows.iter().enumerate() {
                let mut entries: Vec<(&u32, &f64)> = row.iter().collect();
                entries.sort_by_key(|e| *e.0);
                for (c, v) in entries {
                    col.push(*c);
                    g.push(*v);
                }
                start[r + 1] = col.len();
            }
            level.up = up;
            levels.push(level);
            level = Level { diag: cdiag, start, col, g, up: vec![], inner };
            place = coarse_place;
        }
        let chol = factor(&level)?;
        levels.push(level);
        Some(Multigrid { levels, chol })
    }

    /// The same network with another diagonal (the air's h and the heat capacity over the step change only the diagonal): each coarser
    /// diagonal is the sum of its parts' less what lies inside, and the coarsest is factored again. `false` if it cannot be.
    fn refresh(&mut self, diag: &[f64]) -> bool {
        if diag.len() != self.levels[0].n() {
            return false;
        }
        self.levels[0].diag.copy_from_slice(diag);
        for l in 1..self.levels.len() {
            let (fine, coarse) = self.levels.split_at_mut(l);
            let (f, c) = (&fine[l - 1], &mut coarse[0]);
            for (ci, inner) in c.diag.iter_mut().zip(&c.inner) {
                *ci = -inner;
            }
            for i in 0..f.n() {
                c.diag[f.up[i] as usize] += f.diag[i];
            }
        }
        match factor(self.levels.last().unwrap()) {
            Some(c) => {
                self.chol = c;
                true
            }
            None => false,
        }
    }

    /// One V-cycle from zero on level `l` for the right-hand side `r`.
    fn cycle(&self, l: usize, r: &[f64]) -> Vec<f64> {
        let lv = &self.levels[l];
        let n = lv.n();
        if l + 1 == self.levels.len() {
            // L y = r, Lᵀ x = y
            let mut y = r.to_vec();
            for i in 0..n {
                let mut v = y[i];
                for k in 0..i {
                    v -= self.chol[i * n + k] * y[k];
                }
                y[i] = v / self.chol[i * n + i];
            }
            for i in (0..n).rev() {
                let mut v = y[i];
                for k in i + 1..n {
                    v -= self.chol[k * n + i] * y[k];
                }
                y[i] = v / self.chol[i * n + i];
            }
            return y;
        }
        let mut z = vec![0.0; n];
        let mut az = vec![0.0; n];
        let sweep = |z: &mut Vec<f64>, az: &mut Vec<f64>| {
            lv.apply(z, az);
            for i in 0..n {
                z[i] += MG_OMEGA * (r[i] - az[i]) / lv.diag[i];
            }
        };
        for _ in 0..MG_SWEEPS {
            sweep(&mut z, &mut az);
        }
        lv.apply(&z, &mut az);
        let nc = self.levels[l + 1].n();
        let mut rc = vec![0.0; nc];
        for i in 0..n {
            rc[lv.up[i] as usize] += r[i] - az[i];
        }
        let zc = self.cycle(l + 1, &rc);
        for i in 0..n {
            z[i] += MG_OVER * zc[lv.up[i] as usize];
        }
        for _ in 0..MG_SWEEPS {
            sweep(&mut z, &mut az);
        }
        z
    }
}

/// Solves the same network again and again with other diagonals (the rounds of h, the steps of time), keeping the multigrid's levels
/// and refreshing only their diagonals.
pub(crate) struct Solver<'a> {
    a: &'a Csr,
    mg: Option<Multigrid>,
}

impl<'a> Solver<'a> {
    pub fn new(a: &'a Csr) -> Self {
        Solver { a, mg: None }
    }

    /// Makes the preconditioner fit `diag` (built the first time, refreshed after), and returns it.
    fn ready(&mut self, diag: &[f64]) -> Option<&Multigrid> {
        let fresh = match &mut self.mg {
            Some(m) => m.refresh(diag),
            None => false,
        };
        if !fresh {
            self.mg = Multigrid::new(self.a, diag);
        }
        self.mg.as_ref()
    }

    /// `solve` trying Jacobi first: a step of time short against the network's own (the heat capacity over the step outweighs the
    /// conductances) is solved by Jacobi in a few steps, cheaper than building the levels' diagonals and cycling through them; only if
    /// that does not settle within `JACOBI_FIRST` steps does the multigrid take over from where it got.
    pub fn solve_easy_first(&mut self, diag: &[f64], b: &[f64], x: &mut [f64], tol: f64) -> usize {
        const JACOBI_FIRST: usize = 15;
        let its = cg_upto(self.a, diag, b, x, tol, None, JACOBI_FIRST);
        if its < JACOBI_FIRST {
            return its;
        }
        its + self.solve(diag, b, x, tol)
    }

    /// `A x = b` from `x` until the residual is `tol` of `b`. Returns the iterations.
    pub fn solve(&mut self, diag: &[f64], b: &[f64], x: &mut [f64], tol: f64) -> usize {
        let a = self.a;
        let mg = self.ready(diag);
        cg_with(a, diag, b, x, tol, mg)
    }
}


fn cg_with(a: &Csr, diag: &[f64], b: &[f64], x: &mut [f64], tol: f64, mg: Option<&Multigrid>) -> usize {
    cg_upto(a, diag, b, x, tol, mg, 20_000)
}

/// `cg_with` stopping after `max_iter` steps (returns `max_iter` then, the answer as far as it got in `x`).
fn cg_upto(a: &Csr, diag: &[f64], b: &[f64], x: &mut [f64], tol: f64, mg: Option<&Multigrid>, max_iter: usize) -> usize {
    let n = b.len();
    let inv: Vec<f64> = diag.iter().map(|d| 1.0 / d).collect();
    let precond = |r: &[f64], z: &mut Vec<f64>| match mg {
        Some(m) => *z = m.cycle(0, r),
        None => {
            for i in 0..r.len() {
                z[i] = r[i] * inv[i];
            }
        }
    };
    let mut r = vec![0.0; n];
    a.apply(diag, x, &mut r);
    for i in 0..n {
        r[i] = b[i] - r[i];
    }
    let mut z = vec![0.0; n];
    precond(&r, &mut z);
    let mut p = z.clone();
    let mut rz: f64 = (0..n).map(|i| r[i] * z[i]).sum();
    let limit = (tol * b.iter().map(|v| v * v).sum::<f64>().sqrt()).powi(2).max(1e-300);
    let mut ap = vec![0.0; n];
    for it in 0..max_iter {
        a.apply(diag, &p, &mut ap);
        let pap: f64 = p.iter().zip(&ap).map(|(a, b)| a * b).sum();
        if !(pap > 0.0) {
            return it;
        }
        let alpha = rz / pap;
        let mut rr = 0.0;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
            rr += r[i] * r[i];
        }
        if rr < limit {
            return it + 1;
        }
        precond(&r, &mut z);
        let rz2: f64 = (0..n).map(|i| r[i] * z[i]).sum();
        let beta = rz2 / rz;
        rz = rz2;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
    }
    max_iter
}

impl Model {
    /// Where each node is (for the multigrid), the model's nodes then `extra` junctions.
    fn places(&self, extra: usize) -> Vec<Place> {
        let mut out: Vec<Place> = self.cell_of.iter().map(|&(l, c)| Place::Cell { layer: l as u8, i: (c % self.grid.w) as u32, j: (c / self.grid.w) as u32 }).collect();
        out.extend((0..extra).map(|e| Place::Junction(e as u32)));
        out
    }
}

/// The matrix of a model with its junctions, knowing where its nodes are (for the multigrid).
fn system(m: &Model, extra: &[(usize, usize, f64)], junctions: usize) -> Csr {
    let n = m.nodes() + junctions;
    let edges: Vec<(usize, usize, f64)> = m.edges.iter().chain(extra).copied().collect();
    let mut csr = Csr::new(n, &edges);
    csr.place = m.places(junctions);
    csr
}

/// The steady temperature rise (K above the air) of every node for heat `q` (W per node), and the per-node conductance to the air
/// that agrees with it.
/// `extra` holds the conductances (W/K) of nodes past the grid's (the parts' junctions) and `extra_air` their own way to the air (W/K, fixed).
/// Each grid node's conductance to the air (W/K) at its rise `rise[k]` (the faces it carries: the top face on the first layer, the
/// bottom face on the last, both on a board of one layer); the nodes past the grid's get 0.
fn air_conductance(m: &Model, spec: &ThermalSpec, rise: &[f64], air: &mut [f64]) {
    let last = m.layers.len() - 1;
    let face_area = (m.grid.d * 1e-3).powi(2);
    let pressure = pressure_at(spec.altitude_m);
    let standing = spec.orientation == Orientation::Standing;
    let length = if standing { m.grid.h as f64 * m.grid.d * 1e-3 } else { m.length_m };
    for (k, &(l, _)) in m.cell_of.iter().enumerate() {
        let mut g = 0.0;
        for (face_layer, is_top) in [(0, true), (last, false)] {
            if l == face_layer {
                let facing = if standing { None } else { Some(is_top) };
                let (mut c, r) = h_air(rise[k], spec.ambient_c, length, facing, spec.emissivity, pressure);
                // a thin layer of still air to the lid or the floor: conduction across it
                let gap = if is_top { spec.gap_top_mm } else { spec.gap_bottom_mm };
                if gap > 0.0 {
                    c = c.max(0.0263 / (gap * 1e-3));
                }
                g += (c + r) * face_area;
            }
        }
        air[k] = g;
    }
}

fn solve_rise(m: &Model, spec: &ThermalSpec, extra: &[(usize, usize, f64)], extra_air: &[f64], q: &[f64], rise: &mut Vec<f64>) -> (Vec<f64>, usize) {
    let n = m.nodes() + extra_air.len();
    let csr = system(m, extra, extra_air.len());
    let mut solver = Solver::new(&csr);
    if rise.len() != n {
        *rise = vec![10.0; n];
    }
    let mut air = vec![0.0; n];
    let mut rounds = 0;
    // h follows the map: loose solves while it settles, then one tight solve with the h it settled at
    let mut settled = false;
    for round in 0..16 {
        rounds = round + 1;
        let mut diag = csr.sum.clone();
        air_conductance(m, spec, rise, &mut air);
        for k in 0..m.nodes() {
            diag[k] += air[k];
        }
        for (e, g) in extra_air.iter().enumerate() {
            diag[m.nodes() + e] += g;
        }
        let before = rise.clone();
        solver.solve(&diag, q, rise, if settled { 1e-6 } else { 1e-3 });
        if settled {
            break;
        }
        let moved = rise.iter().zip(&before).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        let top = rise.iter().fold(0.0f64, |a, b| a.max(*b));
        settled = moved < 0.005 * top.max(0.1);
    }
    (air, rounds)
}

/// A part that heats the board: its power and the two ways out of its junction (or element), to the copper under its pads through
/// `theta_jc` and to the air through its own body and leads `theta_body` (0 = none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeatSource {
    pub refdes: String,
    pub power: f64,
    pub theta_jc: f64,
    pub theta_body: f64,
    /// The part's own heat capacity (J/K) for the map over time; 0 = it follows its power at once.
    pub cth: f64,
    /// For the map over time: its power over time `(s, W)`, straight between the points, the last value held after them (the
    /// circuit's transient run); empty = `power` from t = 0.
    pub profile: Vec<(f64, f64)>,
    /// The profile repeats with its own length as the period (a PWM or a load that comes and goes), instead of holding its last value.
    pub repeat: bool,
}

/// The energy (J) of a profile from its first point to `t` (negative before it): straight between the points, the first value before
/// them; after the last, the last value held, or with `repeat` the profile again and again (period = its length).
pub fn profile_energy(profile: &[(f64, f64)], t: f64, repeat: bool) -> f64 {
    let Some(&(t0, p0)) = profile.first() else { return 0.0 };
    let &(t1, p1) = profile.last().unwrap();
    // the integral from t0 to x, within the points
    let within = |x: f64| -> f64 {
        let mut sum = 0.0;
        for w in profile.windows(2) {
            let ((a, pa), (b, pb)) = (w[0], w[1]);
            let hi = x.min(b);
            if hi > a && b > a {
                let at = |t: f64| pa + (pb - pa) * (t - a) / (b - a);
                sum += 0.5 * (at(a) + at(hi)) * (hi - a);
            }
        }
        sum
    };
    if t <= t0 {
        return -p0 * (t0 - t);
    }
    if t <= t1 {
        return within(t);
    }
    let period = t1 - t0;
    if repeat && period > 0.0 {
        let n = ((t - t0) / period).floor();
        n * within(t1) + within(t0 + (t - t0 - n * period))
    } else {
        within(t1) + p1 * (t - t1)
    }
}

/// The mean (W) of a power profile (straight between its points, the first value before them and the last after) over `[a, b]`.
pub fn profile_mean(profile: &[(f64, f64)], a: f64, b: f64) -> f64 {
    match profile.first() {
        None => 0.0,
        Some(&(_, p0)) if !(b > a) => p0,
        Some(_) => (profile_energy(profile, b, false) - profile_energy(profile, a, false)) / (b - a),
    }
}

/// The share of a part's own case-to-air θ (its data sheet's minimum pad) that its body and leads carry to the air by themselves: what
/// is left once the grid's minimum-pad board is taken out, fitted to NDT3055L (98 K/W from the data sheet, 127.5 K/W from the bare
/// grid: 1 − 98/127.5). With it, the grid gives that part's 0.066 in² and 1 in² points within 12 %.
pub const BODY_SHARE: f64 = 0.231;

/// θ (K/W) of a part's body and leads to the air, from its own case-to-air θ on the minimum pad (`θ − θjc`).
pub fn body_theta(case_to_air: f64) -> f64 {
    if case_to_air > 0.0 { case_to_air / BODY_SHARE } else { 0.0 }
}

impl HeatSource {
    /// A part with its power, its own θ on the minimum pad (`own_theta`, K/W), its θjc and a heat sink on its case (K/W, 0 = none):
    /// the body takes `BODY_SHARE` of the case's own way to the air, and the heat sink sits beside it.
    pub fn new(refdes: &str, power: f64, own_theta: f64, theta_jc: f64, heatsink: f64) -> HeatSource {
        let jc = theta_jc.clamp(0.0, own_theta);
        let mut g = 0.0;
        let body = body_theta(own_theta - jc);
        if body > 0.0 {
            g += 1.0 / body;
        }
        if heatsink > 0.0 {
            g += 1.0 / heatsink;
        }
        HeatSource { refdes: refdes.into(), power, theta_jc: jc, theta_body: if g > 0.0 { 1.0 / g } else { 0.0 }, cth: 0.0, profile: vec![], repeat: false }
    }
}

/// The temperature map of `board` with the parts' heat `sources`, its copper `copper_um` thick.
pub fn thermal_map(board: &Board, sources: &[HeatSource], copper_um: f64, spec: &ThermalSpec) -> Result<ThermalMap, String> {
    thermal_map_with(board, sources, &[], copper_um, spec)
}

/// `thermal_map` with the wiring's own heat (`TraceHeat`) too.
pub fn thermal_map_with(board: &Board, sources: &[HeatSource], traces: &[TraceHeat], copper_um: f64, spec: &ThermalSpec) -> Result<ThermalMap, String> {
    let m = model(board, copper_um, spec.cell_mm)?;
    Ok(map_of(board, &m, sources, traces, spec).0)
}

/// The heat (W) of the wiring into the grid's nodes: each piece of a way sampled at the cell size, its share of the power by length,
/// on the layer whose height is nearest the piece's (`PathSeg::z`, mm above the top copper).
fn trace_heat_into(m: &Model, traces: &[TraceHeat], q: &mut [f64]) -> f64 {
    let (w, h) = (m.grid.w, m.grid.h);
    let last = m.layers.len().saturating_sub(1);
    let mut put = 0.0;
    for t in traces.iter().filter(|t| t.power > 0.0) {
        let pieces: Vec<&crate::irdrop::PathSeg> = t.path.iter().filter(|s| s.a.dist(s.b) > 1e-9).collect();
        let total: f64 = pieces.iter().map(|s| s.a.dist(s.b)).sum();
        if !(total > 0.0) {
            continue;
        }
        for s in pieces {
            let l = s.a.dist(s.b);
            let layer = if last == 0 { 0 } else { ((s.z / BOARD_MM) * last as f64).round().clamp(0.0, last as f64) as usize };
            let n = (l / m.grid.d).ceil().max(1.0) as usize;
            for k in 0..n {
                let f = (k as f64 + 0.5) / n as f64;
                let p = Point::new(s.a.x + f * (s.b.x - s.a.x), s.a.y + f * (s.b.y - s.a.y));
                let Some(c) = m.grid.cell_at(p) else { continue };
                let node = m.node[layer * w * h + c];
                if node != usize::MAX {
                    let share = t.power * l / total / n as f64;
                    q[node] += share;
                    put += share;
                }
            }
        }
    }
    put
}

/// The map on a built model, and the grid's conductance to the air it settled at.
fn map_of(board: &Board, m: &Model, sources: &[HeatSource], traces: &[TraceHeat], spec: &ThermalSpec) -> (ThermalMap, Vec<f64>) {
    let (rise, air, under, rounds, trace_power) = solve_with_parts(board, m, sources, traces, spec, &mut vec![]);
    let (w, h) = (m.grid.w, m.grid.h);
    let mut temp = vec![vec![f64::NAN; w * h]; m.layers.len()];
    for (k, &(l, c)) in m.cell_of.iter().enumerate() {
        temp[l][c] = spec.ambient_c + rise[k];
    }
    let parts = sources
        .iter()
        .zip(&under)
        .enumerate()
        .map(|(e, (s, cells))| {
            let mean = cells.iter().map(|(n, f)| rise[*n] * f).sum::<f64>();
            let max = cells.iter().map(|(n, _)| rise[*n]).fold(0.0, f64::max);
            PartHeat { refdes: s.refdes.clone(), power: s.power, junction: spec.ambient_c + rise[m.nodes() + e], pad_temp: spec.ambient_c + mean, pad_max: spec.ambient_c + max }
        })
        .collect();
    let face_h = |face: usize| {
        let ks: Vec<usize> = (0..m.nodes()).filter(|k| m.cell_of[*k].0 == face).collect();
        let a = (m.grid.d * 1e-3).powi(2) * if m.layers.len() == 1 { 2.0 } else { 1.0 };
        ks.iter().map(|k| air[*k] / a).sum::<f64>() / ks.len().max(1) as f64
    };
    (ThermalMap { h_top: face_h(0), h_bottom: face_h(m.layers.len() - 1), grid: m.grid.clone(), layers: m.layers.clone(), temp, ambient_c: spec.ambient_c, parts, rounds, trace_power }, air)
}

/// The rise of every node (the grid's, then one junction per source), the grid's conductance to the air, the pad cells of each source
/// The junction nodes of the sources (numbered after the grid's): their conductances to the pads, their own way to the air, and the
/// pad cells of each.
#[allow(clippy::type_complexity)]
fn junctions(board: &Board, m: &Model, sources: &[HeatSource]) -> (Vec<(usize, usize, f64)>, Vec<f64>, Vec<Vec<(usize, f64)>>) {
    let n0 = m.nodes();
    let under: Vec<Vec<(usize, f64)>> = sources.iter().map(|s| pad_cells(board, m, &s.refdes)).collect();
    let mut extra = vec![];
    let mut extra_air = vec![];
    for (e, (s, cells)) in sources.iter().zip(&under).enumerate() {
        let jc = s.theta_jc.max(1e-3);
        for (n, f) in cells {
            extra.push((n0 + e, *n, f / jc));
        }
        // a part with no pads on the board keeps its heat to its body
        let body = if s.theta_body > 0.0 { 1.0 / s.theta_body } else { 0.0 };
        extra_air.push(if cells.is_empty() && body == 0.0 { 1e-9 } else { body });
    }
    (extra, extra_air, under)
}

/// The rise of every node (the grid's, then one junction per source), the grid's conductance to the air, the pad cells of each source
/// and the rounds; `rise` is where to start from (empty: anywhere).
#[allow(clippy::type_complexity)]
fn solve_with_parts(board: &Board, m: &Model, sources: &[HeatSource], traces: &[TraceHeat], spec: &ThermalSpec, rise: &mut Vec<f64>) -> (Vec<f64>, Vec<f64>, Vec<Vec<(usize, f64)>>, usize, f64) {
    let (extra, extra_air, under) = junctions(board, m, sources);
    let mut q = vec![0.0; m.nodes() + sources.len()];
    for (e, s) in sources.iter().enumerate() {
        q[m.nodes() + e] = s.power;
    }
    let trace_power = trace_heat_into(m, traces, &mut q);
    let (air, rounds) = solve_rise(m, spec, &extra, &extra_air, &q, rise);
    (rise.clone(), air, under, rounds, trace_power)
}

/// How the parts warm each other through the board: `r[i][j]` is the rise (K) of part i's junction for 1 W in part j (`r[i][i]` its
/// own θ with the board). The air's `h` is the one of the map at the sources' powers (it follows the temperature), then held: the
/// matrix is the board's answer near that point, and heat adds up in it (`ΔT = R·P`). One solve per source, in parallel. The map at
/// those powers comes back with it.
pub fn thermal_matrix(board: &Board, sources: &[HeatSource], copper_um: f64, spec: &ThermalSpec) -> Result<(Vec<Vec<f64>>, ThermalMap), String> {
    let m = model(board, copper_um, spec.cell_mm)?;
    let (map, air) = map_of(board, &m, sources, &[], spec);
    let (extra, extra_air, _) = junctions(board, &m, sources);
    let n0 = m.nodes();
    let n = n0 + sources.len();
    let csr = system(&m, &extra, sources.len());
    let mut diag = csr.sum.clone();
    for k in 0..n0 {
        diag[k] += air[k];
    }
    for (e, g) in extra_air.iter().enumerate() {
        diag[n0 + e] += g;
    }
    // one hierarchy for every source: the network and its diagonal are the same, only the right-hand side moves
    let mg = Multigrid::new(&csr, &diag);
    let ks: Vec<usize> = (0..sources.len()).collect();
    let cols: Vec<Vec<f64>> = crate::par::par_map(&ks, |&k| {
        let mut q = vec![0.0; n];
        q[n0 + k] = 1.0;
        let mut x = vec![0.0; n];
        cg_with(&csr, &diag, &q, &mut x, 1e-6, mg.as_ref());
        (0..sources.len()).map(|i| x[n0 + i]).collect()
    });
    let r = (0..sources.len()).map(|i| (0..sources.len()).map(|j| cols[j][i]).collect()).collect();
    Ok((r, map))
}

/// The board warming up after its parts are switched on (their powers held from t = 0): pictures of the map at some moments and every
/// part's junction over time.
#[derive(Clone, Debug, PartialEq)]
pub struct ThermalTransient {
    pub grid: Grid,
    pub layers: Vec<Side>,
    pub ambient_c: f64,
    /// The moments (s) of the steps.
    pub times: Vec<f64>,
    /// Each source's junction (°C) at every step.
    pub parts: Vec<(String, Vec<f64>)>,
    /// Pictures of the map: (moment s, temperature °C per layer per cell, `NaN` off the board), at about `frames` moments spread
    /// evenly on a log scale of time, the last at the end.
    pub frames: Vec<(f64, Vec<Vec<f32>>)>,
    /// The board's own heat capacity (J/K, copper and FR-4) and its time to warm (s): the capacity over its whole conductance to the air.
    pub board_cth: f64,
    pub board_tau: f64,
}

/// How the board warms from the air's temperature when the sources' powers are switched on at t = 0 (or follow their `profile`), until
/// `t_end` seconds: backward Euler (stable whatever the step) with steps growing from `t_end`/10⁵ (at least 1 ms) by a constant factor,
/// about 60 of them, and a step end at every point of the profiles too; a step's power is the profile's mean over it (the energy of a
/// pulse shorter than a step is kept; a repeating profile is averaged over the periods a long step holds). The wiring's own heat
/// (`traces`) is held from t = 0. Each cell's h follows its rise of the step before. Each node's heat capacity is its copper and FR-4; a part's junction has its own `cth`
/// (0: a little, so it follows its power within a step).
pub fn thermal_transient(board: &Board, sources: &[HeatSource], traces: &[TraceHeat], copper_um: f64, spec: &ThermalSpec, t_end: f64, frames: usize) -> Result<ThermalTransient, String> {
    if !(t_end > 0.0) {
        return Err("時間は 0 より長くしてください".into());
    }
    let m = model(board, copper_um, spec.cell_mm)?;
    let (extra, extra_air, _) = junctions(board, &m, sources);
    let n0 = m.nodes();
    let n = n0 + sources.len();
    let csr = system(&m, &extra, sources.len());
    let mut solver = Solver::new(&csr);
    let mut cap = m.cap.clone();
    cap.extend(sources.iter().map(|s| s.cth.max(1e-6)));
    // the wiring's own heat, held from t = 0; the parts' power is set step by step
    let mut q = vec![0.0; n];
    trace_heat_into(&m, traces, &mut q);
    // steps: geometric from dt0 to t_end
    let steps = 60usize;
    let dt0 = (t_end * 1e-5).max(1e-3).min(t_end / steps as f64);
    let growth = if t_end / dt0 > steps as f64 {
        // dt0 · (g^steps − 1) / (g − 1) = t_end
        let (mut lo, mut hi): (f64, f64) = (1.0 + 1e-9, 2.0);
        for _ in 0..100 {
            let mid = 0.5 * (lo + hi);
            if dt0 * (mid.powi(steps as i32) - 1.0) / (mid - 1.0) < t_end { lo = mid } else { hi = mid }
        }
        hi
    } else {
        1.0
    };
    // where the steps end: the geometric ones, and the points of the sources' power profiles (at most 2000 of them, evenly thinned),
    // so a pulse of the circuit is stepped through
    let mut ends: Vec<f64> = vec![];
    let (mut t, mut dt) = (0.0, dt0);
    while t < t_end * (1.0 - 1e-9) {
        t = (t + dt).min(t_end);
        ends.push(t);
        dt *= growth;
    }
    let mut points: Vec<f64> = sources.iter().flat_map(|s| s.profile.iter().map(|p| p.0)).filter(|t| *t > 0.0 && *t < t_end).collect();
    points.sort_by(f64::total_cmp);
    points.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let every = points.len().div_ceil(2000).max(1);
    ends.extend(points.into_iter().step_by(every));
    ends.sort_by(f64::total_cmp);
    ends.dedup_by(|a, b| (*a - *b).abs() <= 1e-9 * b.abs().max(1e-6));
    let frame_at: Vec<f64> = (1..=frames.max(1)).map(|k| t_end * 10f64.powf(-3.0 * (frames.max(1) - k) as f64 / frames.max(1) as f64)).collect();
    let mut rise = vec![0.0; n];
    let mut air = vec![0.0; n];
    let mut times = vec![0.0];
    let mut parts: Vec<(String, Vec<f64>)> = sources.iter().map(|s| (s.refdes.clone(), vec![spec.ambient_c])).collect();
    let mut shots = vec![];
    let (w, h) = (m.grid.w, m.grid.h);
    let picture = |rise: &[f64]| -> Vec<Vec<f32>> {
        let mut t = vec![vec![f32::NAN; w * h]; m.layers.len()];
        for (k, &(l, c)) in m.cell_of.iter().enumerate() {
            t[l][c] = (spec.ambient_c + rise[k]) as f32;
        }
        t
    };
    let mut t = 0.0;
    let mut next_frame = 0;
    for &end in &ends {
        let step = end - t;
        if !(step > 0.0) {
            continue;
        }
        for (e, s) in sources.iter().enumerate() {
            q[n0 + e] = if s.profile.is_empty() { s.power } else { (profile_energy(&s.profile, end, s.repeat) - profile_energy(&s.profile, t, s.repeat)) / step };
        }
        air_conductance(&m, spec, &rise, &mut air);
        let mut diag = csr.sum.clone();
        let mut b = vec![0.0; n];
        for k in 0..n {
            if k < n0 {
                diag[k] += air[k];
            } else {
                diag[k] += extra_air[k - n0];
            }
            diag[k] += cap[k] / step;
            b[k] = q[k] + cap[k] / step * rise[k];
        }
        solver.solve_easy_first(&diag, &b, &mut rise, 1e-5);
        t = end;
        times.push(t);
        for (e, p) in parts.iter_mut().enumerate() {
            p.1.push(spec.ambient_c + rise[n0 + e]);
        }
        while next_frame < frame_at.len() && t >= frame_at[next_frame] * (1.0 - 1e-9) {
            shots.push((t, picture(&rise)));
            next_frame += 1;
        }
    }
    let board_cth: f64 = m.cap.iter().sum();
    let g_air: f64 = air[..n0].iter().sum();
    Ok(ThermalTransient { grid: m.grid.clone(), layers: m.layers.clone(), ambient_c: spec.ambient_c, times, parts, frames: shots, board_cth, board_tau: board_cth / g_air.max(1e-12) })
}

/// Copper's resistance rises by this share per kelvin (0.393 %/K), from the 25 °C its resistivity is taken at (`irdrop`).
pub const CU_TEMPCO: f64 = 0.00393;

/// A pad's wiring warmed by the board: its mean temperature along the way from the supply pad, and how much its resistance grows.
#[derive(Clone, Debug, PartialEq)]
pub struct WarmWiring {
    pub refdes: String,
    pub pin: String,
    pub net: String,
    pub temp_c: f64,
    /// The resistance at that temperature over the one at 25 °C.
    pub factor: f64,
}

/// The temperature of each pad's wiring (`irdrop::wiring_resistances`) on the map: the mean along its tracks, weighted by length
/// (the layers at a point averaged), or the temperature at the pad when its way has no tracks (a pour). Copper's resistance follows
/// as `1 + CU_TEMPCO·(T − 25)`.
pub fn wiring_heat_factors(board: &Board, series: &[crate::irdrop::PadSeries], map: &ThermalMap) -> Vec<WarmWiring> {
    let at = |p: Point| -> Option<f64> {
        let c = map.grid.cell_at(p)?;
        let v: Vec<f64> = map.temp.iter().map(|t| t[c]).filter(|x| x.is_finite()).collect();
        (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
    };
    series
        .iter()
        .filter_map(|s| {
            let (mut sum, mut len) = (0.0, 0.0);
            for seg in s.path.iter().filter(|g| g.a.dist(g.b) > 1e-9) {
                let l = seg.a.dist(seg.b);
                let n = (l / map.grid.d).ceil().max(1.0) as usize;
                for k in 0..n {
                    let f = (k as f64 + 0.5) / n as f64;
                    if let Some(t) = at(Point::new(seg.a.x + f * (seg.b.x - seg.a.x), seg.a.y + f * (seg.b.y - seg.a.y))) {
                        sum += t * l / n as f64;
                        len += l / n as f64;
                    }
                }
            }
            let temp = if len > 0.0 { sum / len } else { at(board.pads.iter().find(|p| p.refdes == s.refdes && p.pin == s.pin)?.pos)? };
            Some(WarmWiring { refdes: s.refdes.clone(), pin: s.pin.clone(), net: s.net.clone(), temp_c: temp, factor: 1.0 + CU_TEMPCO * (temp - 25.0) })
        })
        .collect()
}

/// Two parts that warm each other noticeably through the board.
#[derive(Clone, Debug, PartialEq)]
pub struct HotPair {
    pub a: String,
    pub b: String,
    /// The nearest pads of the two (mm, centre to centre, as the "離す" requirement measures).
    pub dist_mm: f64,
    /// The rise (K) each gets from the other's heat, and its whole rise.
    pub a_from_b: f64,
    pub b_from_a: f64,
    pub rise_a: f64,
    pub rise_b: f64,
    /// A distance (mm) at which the heat from the other falls to half the level that flags it (a guide from the board's spreading
    /// length).
    pub suggest_mm: f64,
}

/// The heat a part gets from a neighbour flags the pair when it is at least this (K) and this share of the part's rise.
pub const HOT_PAIR_K: f64 = 3.0;
pub const HOT_PAIR_SHARE: f64 = 0.15;

/// The pairs of parts that warm each other noticeably (`HOT_PAIR_K` and `HOT_PAIR_SHARE`), the strongest first, with a distance to
/// keep them apart. From the board's matrix at the sources' powers. The distance: a plate cooled by the air spreads heat over a length
/// λ = √(sheet conductance / h) (the board's mean copper and FR-4, both faces' h), and far from a source the heat falls as
/// e^(−r/λ)/√r; the coupling as it is at today's distance is scaled by that until it is half the flagging level.
pub fn hot_pairs(board: &Board, sources: &[HeatSource], copper_um: f64, spec: &ThermalSpec) -> Result<Vec<HotPair>, String> {
    let (r, map) = thermal_matrix(board, sources, copper_um, spec)?;
    hot_pairs_with(board, sources, &r, &map, copper_um, spec)
}

/// `hot_pairs` from a matrix and its map already worked out for these `sources` (`thermal_matrix`): no solve of its own.
pub fn hot_pairs_with(board: &Board, sources: &[HeatSource], r: &[Vec<f64>], map: &ThermalMap, copper_um: f64, spec: &ThermalSpec) -> Result<Vec<HotPair>, String> {
    let m = model(board, copper_um, spec.cell_mm)?;
    let h = (map.h_top + map.h_bottom).max(1.0);
    let lambda_mm = (m.mean_sheet / h).sqrt() * 1e3;
    let pads = |n: &str| board.pads.iter().filter(|p| p.refdes == n).map(|p| p.pos).collect::<Vec<Point>>();
    let mut out = vec![];
    for i in 0..sources.len() {
        for j in i + 1..sources.len() {
            let (pi, pj) = (pads(&sources[i].refdes), pads(&sources[j].refdes));
            let Some(d) = pi.iter().flat_map(|a| pj.iter().map(move |b| a.dist(*b))).min_by(f64::total_cmp) else { continue };
            let (rise_i, rise_j) = (map.parts[i].junction - map.ambient_c, map.parts[j].junction - map.ambient_c);
            let (ij, ji) = (r[i][j] * sources[j].power, r[j][i] * sources[i].power);
            let level = |rise: f64| HOT_PAIR_K.max(HOT_PAIR_SHARE * rise);
            let over = (ij / level(rise_i)).max(ji / level(rise_j));
            if over < 1.0 {
                continue;
            }
            // the distance where the coupling is half the level: c(r) = c(d) · √(d/r) · e^(−(r − d)/λ)
            let want = 0.5 / over;
            let d0 = d.max(0.5);
            let fall = |x: f64| (d0 / x).sqrt() * (-(x - d0) / lambda_mm.max(0.5)).exp();
            let (mut lo, mut hi) = (d0, d0 + 40.0 * lambda_mm.max(0.5));
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if fall(mid) > want { lo = mid } else { hi = mid }
            }
            let suggest_mm = hi.ceil().max(d + 2.0).ceil();
            out.push(HotPair { a: sources[i].refdes.clone(), b: sources[j].refdes.clone(), dist_mm: d, a_from_b: ij, b_from_a: ji, rise_a: rise_i, rise_b: rise_j, suggest_mm });
        }
    }
    out.sort_by(|x, y| y.a_from_b.max(y.b_from_a).total_cmp(&x.a_from_b.max(x.b_from_a)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mean of a power profile over a step is its exact integral: a ramp's middle, a pulse narrower than the step by its area, the
    /// held ends outside the points.
    #[test]
    fn a_profile_gives_its_exact_mean_over_a_step() {
        let ramp = [(0.0, 0.0), (10.0, 10.0)];
        assert!((profile_mean(&ramp, 0.0, 10.0) - 5.0).abs() < 1e-12);
        assert!((profile_mean(&ramp, 10.0, 20.0) - 10.0).abs() < 1e-12, "held after the end");
        // 2 W for 1 ms in the middle of a 10 ms step: 0.2 W on average
        let pulse = [(0.0, 0.0), (4.5e-3, 0.0), (4.5e-3, 2.0), (5.5e-3, 2.0), (5.5e-3, 0.0), (1.0, 0.0)];
        assert!((profile_mean(&pulse, 0.0, 0.01) - 0.2).abs() < 1e-9, "{}", profile_mean(&pulse, 0.0, 0.01));
        assert_eq!(profile_mean(&[], 0.0, 1.0), 0.0);
    }

    /// The worked example of the candidate: an 80 mm plate 30 K above the air gives about 6.6 W/m²K of convection and 6.7 of radiation.
    #[test]
    fn the_air_takes_heat_by_convection_and_radiation_alike() {
        let (c, r) = h_air(30.0, 25.0, 0.08, None, 0.9, pressure_at(0.0));
        assert!((5.0..7.5).contains(&c), "convection {c}");
        assert!((6.0..7.5).contains(&r), "radiation {r}");
        // a hot face downwards loses half of what an upward one does
        let (up, _) = h_air(30.0, 25.0, 0.02, Some(true), 0.9, pressure_at(0.0));
        let (down, _) = h_air(30.0, 25.0, 0.02, Some(false), 0.9, pressure_at(0.0));
        assert!((up / down - 2.0).abs() < 1e-9);
        // at 3000 m the air is thinner: less convection, the same radiation
        let (c3, r3) = h_air(30.0, 25.0, 0.08, None, 0.9, pressure_at(3000.0));
        assert!(c3 < 0.9 * c && (r3 - r).abs() < 1e-12);
        assert!((pressure_at(3000.0) / 101_325.0 - 0.69).abs() < 0.01);
    }
}

#[cfg(test)]
mod datasheet {
    use super::*;
    use crate::model::{Pad, PadShape, Side, Track, Zone};

    /// NDT3055L's test board as the data sheet leaves it to guess: a JEDEC-sized one-layer FR-4 board (76.2 × 114.3 mm), the SOT-223 in the
    /// middle, the drain copper a square of `copper_mm2` around the tab (0 = the pads alone).
    pub(crate) fn sot223_board(copper_mm2: f64) -> Board {
        let mut b = Board::new("t", 76.2, 114.3);
        let (cx, cy) = (38.1, 57.15);
        let pad = |x: f64, y: f64, w: f64, h: f64, pin: &str, net: &str| {
            let mut p = Pad::new(Point::new(cx + x, cy + y), PadShape::Rect { w, h }, None, Side::Top);
            p.refdes = "Q1".into();
            p.pin = pin.into();
            p.net = net.into();
            p
        };
        b.pads.push(pad(-3.15, 2.3, 2.0, 1.5, "1", "G"));
        b.pads.push(pad(-3.15, 0.0, 2.0, 1.5, "2", "DRN"));
        b.pads.push(pad(-3.15, -2.3, 2.0, 1.5, "3", "S"));
        b.pads.push(pad(3.15, 0.0, 2.0, 3.8, "2", "DRN"));
        if copper_mm2 > 0.0 {
            let s = copper_mm2.sqrt() / 2.0;
            let (x, y) = (cx + 3.15, cy);
            let mut z = Zone::new("DRN", Side::Top, vec![Point::new(x - s, y - s), Point::new(x + s, y - s), Point::new(x + s, y + s), Point::new(x - s, y + s)]);
            z.clearance = 0.3;
            b.zones.push(z);
        }
        b
    }

    /// The matrix of two parts on one board: R·P gives back the map's junctions (the same held h), it is symmetric (heat from i to j
    /// is heat from j to i), a part warms itself more than its neighbour, and the nearer neighbour gets more.
    #[test]
    fn the_matrix_adds_the_heat_of_the_parts_as_the_map_does() {
        let mut b = sot223_board(645.16);
        for (name, x) in [("R1", 20.0), ("R2", 60.0)] {
            for dx in [-1.6, 1.6] {
                let mut p = Pad::new(Point::new(x + dx, 57.15), PadShape::Rect { w: 1.2, h: 1.6 }, None, Side::Top);
                p.refdes = name.into();
                b.pads.push(p);
            }
        }
        let src = vec![
            HeatSource::new("Q1", 1.0, 110.0, 12.0, 0.0),
            HeatSource::new("R1", 0.3, 300.0, 0.0, 0.0),
            HeatSource::new("R2", 0.2, 300.0, 0.0, 0.0),
        ];
        let spec = ThermalSpec { cell_mm: 1.0, ..Default::default() };
        let (r, map) = thermal_matrix(&b, &src, 70.0, &spec).unwrap();
        for i in 0..3 {
            let sum: f64 = (0..3).map(|j| r[i][j] * src[j].power).sum();
            assert!((sum - (map.parts[i].junction - 25.0)).abs() < 1e-3 * sum, "{i}: R·P {sum} against the map {}", map.parts[i].junction - 25.0);
            for j in 0..3 {
                assert!((r[i][j] - r[j][i]).abs() < 1e-4 * r[i][i], "R[{i}][{j}] {} R[{j}][{i}] {}", r[i][j], r[j][i]);
                if i != j {
                    assert!(r[i][j] > 0.0 && r[i][j] < r[i][i], "{r:?}");
                }
            }
        }
        // the nearer the more: R1, about 20 mm from Q1, gets more of Q1's watt than R2 gets of R1's, 40 mm away
        assert!(r[1][0] > r[2][1], "{r:?}");
    }

    /// Two parts of 0.5 W each, 6 mm apart on a bare board, warm each other by more than the flagging level: a pair, with a distance
    /// further than now at which the coupling is about half that level. The same two 60 mm apart are not a pair.
    #[test]
    fn hot_neighbours_are_found_and_a_distance_is_suggested() {
        let pair = |gap: f64| {
            let mut b = Board::new("t", 100.0, 60.0);
            for (name, x) in [("R1", 20.0), ("R2", 20.0 + gap)] {
                for dy in [-1.6, 1.6] {
                    let mut p = Pad::new(Point::new(x, 30.0 + dy), PadShape::Rect { w: 1.6, h: 1.2 }, None, Side::Top);
                    p.refdes = name.into();
                    b.pads.push(p);
                }
            }
            b
        };
        let src = vec![HeatSource::new("R1", 0.5, 150.0, 0.0, 0.0), HeatSource::new("R2", 0.5, 150.0, 0.0, 0.0)];
        let spec = ThermalSpec { cell_mm: 1.0, ..Default::default() };
        let near = hot_pairs(&pair(6.0), &src, 35.0, &spec).unwrap();
        assert_eq!(near.len(), 1, "{near:?}");
        let p = &near[0];
        assert!((p.dist_mm - 6.0).abs() < 1e-9 && p.a_from_b > HOT_PAIR_K && p.suggest_mm > 8.0, "{p:?}");
        // at the suggested distance the coupling is what the spreading length promised: about half the level that flagged it
        // (15.1 K at 6 mm; at 10 mm the grid gives 7.1 K against the aimed 7.4 K)
        let level = HOT_PAIR_K.max(HOT_PAIR_SHARE * p.rise_a);
        let (r, _) = thermal_matrix(&pair(p.suggest_mm), &src, 35.0, &spec).unwrap();
        let there = r[0][1] * 0.5;
        assert!(there < 0.6 * level && there > 0.3 * level, "{there} K at {} mm, the level {level}", p.suggest_mm);
        assert!(hot_pairs(&pair(60.0), &src, 35.0, &spec).unwrap().is_empty());
    }

    /// The map over time: a 100 × 80 mm two-layer board holds about 26 J/K of FR-4 (the candidate's figure; its copper pads add
    /// little) and warms in minutes; run long, it ends on the steady map; a part with its own heat capacity rises at first as
    /// P·t/C, and later than one without.
    #[test]
    fn the_board_warms_over_minutes_to_its_steady_map() {
        let mut b = Board::new("t", 100.0, 80.0);
        for dx in [-1.6, 1.6] {
            let mut p = Pad::new(Point::new(50.0 + dx, 40.0), PadShape::Rect { w: 1.6, h: 1.2 }, None, Side::Top);
            p.refdes = "R1".into();
            b.pads.push(p);
        }
        let spec = ThermalSpec { cell_mm: 2.0, ..Default::default() };
        let mut src = HeatSource::new("R1", 1.0, 120.0, 0.0, 0.0);
        let steady = thermal_map(&b, std::slice::from_ref(&src), 35.0, &spec).unwrap();
        let tr = thermal_transient(&b, std::slice::from_ref(&src), &[], 35.0, &spec, 3000.0, 6).unwrap();
        assert!((25.0..28.0).contains(&tr.board_cth), "FR-4 100 × 80 × 1.6 mm: {} J/K", tr.board_cth);
        assert!((60.0..400.0).contains(&tr.board_tau), "minutes: {} s", tr.board_tau);
        let end = *tr.parts[0].1.last().unwrap();
        assert!((end - steady.parts[0].junction).abs() < 0.02 * (steady.parts[0].junction - 25.0), "the end {end}, the steady map {}", steady.parts[0].junction);
        assert_eq!(tr.frames.len(), 6);
        assert!((tr.frames.last().unwrap().0 - 3000.0).abs() < 1e-6);
        // the hottest cell rises frame after frame
        let hot = |f: &Vec<Vec<f32>>| f.iter().flatten().filter(|x| x.is_finite()).fold(0f32, |a, b| a.max(*b));
        assert!(tr.frames.windows(2).all(|w| hot(&w[1].1) >= hot(&w[0].1) - 1e-3));
        // its own 0.05 J/K: after 10 ms it has risen by about P·t/C = 0.2 K, far below where the part without one already is
        src.cth = 0.05;
        let slow = thermal_transient(&b, std::slice::from_ref(&src), &[], 35.0, &spec, 1.0, 4).unwrap();
        let at = |tr: &ThermalTransient, t: f64| tr.times.iter().position(|x| *x >= t).map(|k| tr.parts[0].1[k] - 25.0).unwrap();
        let early = at(&slow, 0.01);
        assert!((0.1..0.3).contains(&early), "{early} K after 10 ms");
        assert!(at(&tr, 0.01) > 5.0 * early);
    }

    /// A pad's wiring takes the mean temperature along its way: a track from the cool corner to the hot part is between the two, and
    /// warmer than the air by less than the part's pads; a wiring with no track (a pour) takes the pad's own temperature.
    #[test]
    fn wiring_takes_the_temperature_along_its_way() {
        let b = sot223_board(0.0);
        let map = thermal_map(&b, &[HeatSource::new("Q1", 1.0, 110.0, 12.0, 0.0)], 70.0, &ThermalSpec { cell_mm: 1.0, ..Default::default() }).unwrap();
        let pad = b.pads.iter().find(|p| p.pin == "2" && matches!(p.shape, PadShape::Rect { h, .. } if h > 3.0)).unwrap().pos;
        let seg = |a: Point, z: Point| crate::irdrop::PathSeg { a, b: z, z: 0.0, z2: 0.0, width: 0.5, plane: None };
        let series = |path: Vec<crate::irdrop::PathSeg>| crate::irdrop::PadSeries { net: "DRN".into(), refdes: "Q1".into(), pin: "2".into(), ohm: 0.01, henry: 0.0, farad: 0.0, path };
        let w = wiring_heat_factors(&b, &[series(vec![seg(Point::new(5.0, 5.0), pad)]), series(vec![])], &map);
        let pad_t = map.at(Side::Top, pad).unwrap();
        assert!(w[0].temp_c > 25.5 && w[0].temp_c < pad_t, "{w:?}, the pad {pad_t}");
        assert!((w[1].temp_c - pad_t).abs() < 0.5 * (pad_t - 25.0), "{w:?}");
        assert!((w[1].factor - (1.0 + CU_TEMPCO * (w[1].temp_c - 25.0))).abs() < 1e-12);
    }

    /// The circuit's power over time drives the map: a profile that holds 1 W gives the plain run; 2 W for the first 10 s then nothing
    /// heats the part until 10 s and lets it cool after, far below where 2 W held would take it.
    #[test]
    fn a_power_profile_drives_the_map_over_time() {
        let mut b = Board::new("t", 60.0, 40.0);
        for dx in [-1.6, 1.6] {
            let mut p = Pad::new(Point::new(30.0 + dx, 20.0), PadShape::Rect { w: 1.6, h: 1.2 }, None, Side::Top);
            p.refdes = "R1".into();
            b.pads.push(p);
        }
        let spec = ThermalSpec { cell_mm: 2.0, ..Default::default() };
        let plain = HeatSource { cth: 0.05, ..HeatSource::new("R1", 1.0, 120.0, 0.0, 0.0) };
        let held = HeatSource { profile: vec![(0.0, 1.0), (300.0, 1.0)], ..plain.clone() };
        let a = thermal_transient(&b, std::slice::from_ref(&plain), &[], 35.0, &spec, 300.0, 4).unwrap();
        let c = thermal_transient(&b, std::slice::from_ref(&held), &[], 35.0, &spec, 300.0, 4).unwrap();
        let (ea, ec) = (*a.parts[0].1.last().unwrap(), *c.parts[0].1.last().unwrap());
        assert!((ea - ec).abs() < 1e-6 * ea, "{ea} {ec}");
        let pulse = HeatSource { profile: vec![(0.0, 2.0), (10.0, 2.0), (10.0, 0.0), (300.0, 0.0)], ..plain };
        let p = thermal_transient(&b, &[pulse], &[], 35.0, &spec, 300.0, 4).unwrap();
        let (k, top) = p.parts[0].1.iter().enumerate().fold((0, f64::MIN), |m, (k, v)| if *v > m.1 { (k, *v) } else { m });
        assert!((p.times[k] - 10.0).abs() < 1e-6, "the peak at the end of the pulse: {} s", p.times[k]);
        let end = *p.parts[0].1.last().unwrap();
        assert!(end < top && end < 25.0 + 0.5 * (top - 25.0), "cooled after it: {end} from {top}");
        assert!(top < 25.0 + 2.0 * (ea - 25.0), "10 s of 2 W is far from its steady rise: {top}");
    }

    /// A wiring's own heat: 1 W along a 40 mm track warms the cells along it (more than a point 10 mm beside it), the map counts the
    /// watt, and in the steady state the air takes all of it (Σ G_air·rise = the power put in).
    #[test]
    fn the_wirings_own_heat_warms_the_copper_along_it() {
        let mut b = Board::new("t", 80.0, 40.0);
        b.tracks.push(Track { side: Side::Top, a: Point::new(20.0, 20.0), b: Point::new(60.0, 20.0), width: 0.5 });
        let path = vec![crate::irdrop::PathSeg { a: Point::new(20.0, 20.0), b: Point::new(60.0, 20.0), z: 0.0, z2: 0.0, width: 0.5, plane: None }];
        let spec = ThermalSpec { cell_mm: 1.0, ..Default::default() };
        let map = thermal_map_with(&b, &[], &[TraceHeat { path: path.clone(), power: 1.0 }], 35.0, &spec).unwrap();
        assert!((map.trace_power - 1.0).abs() < 1e-9, "{}", map.trace_power);
        let (on, beside) = (map.at(Side::Top, Point::new(40.0, 20.0)).unwrap(), map.at(Side::Top, Point::new(40.0, 30.0)).unwrap());
        assert!(on > beside && beside > 25.0, "{on} on the track, {beside} beside it");
        // the energy balance of the steady state
        let m = model(&b, 35.0, 1.0).unwrap();
        let mut rise = vec![];
        let (r, air, _, _, put) = solve_with_parts(&b, &m, &[], &[TraceHeat { path, power: 1.0 }], &spec, &mut rise);
        let out: f64 = (0..m.nodes()).map(|k| air[k] * r[k]).sum();
        assert!((out - put).abs() < 0.02 * put, "{out} W leaves, {put} W in");
    }

    /// A repeating profile: 2 W for half of every 1 s period averages to 1 W over a long step, and over minutes the part ends where
    /// 1 W held takes it (the board is far slower than the period); the energy of n periods is n times one.
    #[test]
    fn a_repeating_profile_warms_like_its_mean() {
        let pwm = [(0.0, 2.0), (0.5, 2.0), (0.5, 0.0), (1.0, 0.0)];
        assert!((profile_energy(&pwm, 10.0, true) - 10.0).abs() < 1e-9);
        assert!((profile_energy(&pwm, 10.25, true) - 10.5).abs() < 1e-9);
        assert!((profile_energy(&pwm, 10.0, false) - 1.0).abs() < 1e-9, "held at its last value 0 W");
        let mut b = Board::new("t", 60.0, 40.0);
        for dx in [-1.6, 1.6] {
            let mut p = Pad::new(Point::new(30.0 + dx, 20.0), PadShape::Rect { w: 1.6, h: 1.2 }, None, Side::Top);
            p.refdes = "R1".into();
            b.pads.push(p);
        }
        let spec = ThermalSpec { cell_mm: 2.0, ..Default::default() };
        let mean = HeatSource { cth: 0.5, ..HeatSource::new("R1", 1.0, 120.0, 0.0, 0.0) };
        let chopped = HeatSource { profile: pwm.to_vec(), repeat: true, ..mean.clone() };
        let a = thermal_transient(&b, std::slice::from_ref(&mean), &[], 35.0, &spec, 600.0, 4).unwrap();
        let c = thermal_transient(&b, std::slice::from_ref(&chopped), &[], 35.0, &spec, 600.0, 4).unwrap();
        let (ea, ec) = (*a.parts[0].1.last().unwrap(), *c.parts[0].1.last().unwrap());
        assert!((ea - ec).abs() < 0.02 * (ea - 25.0), "1 W held {ea}, the PWM {ec}");
    }

    /// The time each step takes on a board of the usual size (100 × 80 mm, two layers, a ground pour on the bottom, 8 parts that heat):
    /// `cargo test --release -p pcb-core thermal_bench -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn thermal_bench() {
        let mut b = Board::new("t", 100.0, 80.0);
        let mut src = vec![];
        for k in 0..8 {
            let name = format!("R{}", k + 1);
            let (x, y) = (15.0 + 10.0 * k as f64, if k % 2 == 0 { 25.0 } else { 55.0 });
            for dx in [-1.6, 1.6] {
                let mut p = Pad::new(Point::new(x + dx, y), PadShape::Rect { w: 1.6, h: 1.2 }, None, Side::Top);
                p.refdes = name.clone();
                p.net = if dx < 0.0 { format!("N{k}") } else { "GND".into() };
                b.pads.push(p);
            }
            src.push(HeatSource::new(&name, 0.3 + 0.05 * k as f64, 150.0, 0.0, 0.0));
        }
        let mut z = Zone::new("GND", Side::Bottom, vec![Point::new(1.0, 1.0), Point::new(99.0, 1.0), Point::new(99.0, 79.0), Point::new(1.0, 79.0)]);
        z.clearance = 0.3;
        b.zones.push(z);
        let spec = ThermalSpec::default();
        let time = |what: &str, f: &mut dyn FnMut()| {
            let t0 = std::time::Instant::now();
            f();
            eprintln!("BENCH {what}: {:.0} ms", t0.elapsed().as_secs_f64() * 1e3);
        };
        time("model", &mut || drop(model(&b, 35.0, 0.0).unwrap()));
        time("map", &mut || drop(thermal_map(&b, &src, 35.0, &spec).unwrap()));
        time("matrix (8 parts)", &mut || drop(thermal_matrix(&b, &src, 35.0, &spec).unwrap()));
        time("hot pairs", &mut || drop(hot_pairs(&b, &src, 35.0, &spec).unwrap()));
        time("warm-up 600 s", &mut || drop(thermal_transient(&b, &src, &[], 35.0, &spec, 600.0, 8).unwrap()));
        time("window: matrix and its map, pairs (one solve)", &mut || {
            let (r, map) = thermal_matrix(&b, &src, 35.0, &spec).unwrap();
            drop(hot_pairs_with(&b, &src, &r, &map, 35.0, &spec).unwrap());
        });
    }

    /// The steps on a real board with many parts: `THERMAL_BENCH_BOARD=path.pcbjson|.ccproj` (else the sample `mixed_signal`),
    /// every part a heat source of a made-up power (50 mW, 300 mW with more than 4 pads):
    /// `cargo test --release -p pcb-core thermal_bench_board -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn thermal_bench_board() {
        let path = std::env::var("THERMAL_BENCH_BOARD").unwrap_or_else(|_| format!("{}/../../docs/examples/mixed_signal.ccproj", env!("CARGO_MANIFEST_DIR")));
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let bv = if v.get("pads").is_some() { v } else if let Some(d) = v.get("designs") { d[0]["board"].clone() } else { v["board"].clone() };
        let b: Board = serde_json::from_value(bv).unwrap();
        let mut src: Vec<HeatSource> = b
            .parts
            .iter()
            .map(|p| {
                let pads = b.pads.iter().filter(|x| x.refdes == p.refdes).count();
                HeatSource::new(&p.refdes, if pads > 4 { 0.3 } else { 0.05 }, 120.0, 10.0, 0.0)
            })
            .collect();
        src.sort_by(|a, c| c.power.total_cmp(&a.power));
        let hot: Vec<HeatSource> = src.iter().take(24).cloned().collect();
        let spec = ThermalSpec::default();
        eprintln!("BENCH board {path}: {} parts, {} pads, {} tracks, {} pours, {} layers", b.parts.len(), b.pads.len(), b.tracks.len(), b.zones.len(), b.copper_layers().len());
        let time = |what: &str, f: &mut dyn FnMut()| {
            let t0 = std::time::Instant::now();
            f();
            eprintln!("BENCH {what}: {:.0} ms", t0.elapsed().as_secs_f64() * 1e3);
        };
        let m = model(&b, 35.0, 0.0).unwrap();
        eprintln!("BENCH grid {} × {} at {:.2} mm, {} nodes", m.grid.w, m.grid.h, m.grid.d, m.nodes());
        // the steps of one tight solve: 1 W into the hottest junction, the air's h of a 10 K rise
        {
            let (extra, extra_air, _) = junctions(&b, &m, &hot);
            let csr = system(&m, &extra, hot.len());
            let mut air = vec![0.0; m.nodes() + hot.len()];
            air_conductance(&m, &spec, &vec![10.0; m.nodes()], &mut air);
            let mut diag = csr.sum.clone();
            for k in 0..diag.len() {
                diag[k] += if k < m.nodes() { air[k] } else { extra_air[k - m.nodes()] };
            }
            let mut q = vec![0.0; diag.len()];
            q[m.nodes()] = 1.0;
            let t0 = std::time::Instant::now();
            let mg = Multigrid::new(&csr, &diag);
            let setup = t0.elapsed();
            let mut x = vec![0.0; diag.len()];
            let its = cg_with(&csr, &diag, &q, &mut x, 1e-6, mg.as_ref());
            eprintln!("BENCH one solve: multigrid {} levels, setup {:.0} ms, {its} steps, {:.0} ms", mg.as_ref().map_or(0, |m| m.levels.len()), setup.as_secs_f64() * 1e3, t0.elapsed().as_secs_f64() * 1e3);
            let t1 = std::time::Instant::now();
            let mut x = vec![0.0; diag.len()];
            let its = cg_with(&csr, &diag, &q, &mut x, 1e-6, None);
            eprintln!("BENCH one solve: Jacobi {its} steps, {:.0} ms", t1.elapsed().as_secs_f64() * 1e3);
        }
        // what the automatic pitch gives against 0.5 mm: each part's rise
        if m.grid.d > 0.51 {
            let auto = thermal_map(&b, &src, 35.0, &spec).unwrap();
            let fine = thermal_map(&b, &src, 35.0, &ThermalSpec { cell_mm: 0.5, ..spec }).unwrap();
            let (mut worst, mut at) = (0.0f64, String::new());
            for (a, f) in auto.parts.iter().zip(&fine.parts) {
                let (ra, rf) = (a.junction - 25.0, f.junction - 25.0);
                if (ra - rf).abs() > worst {
                    worst = (ra - rf).abs();
                    at = format!("{} {:.1} K at {:.2} mm, {:.1} K at 0.5 mm", a.refdes, ra, m.grid.d, rf);
                }
            }
            eprintln!("BENCH the pitch: largest change of a part's rise {worst:.2} K ({at})");
        }
        time("model (pours filled)", &mut || drop(model(&b, 35.0, 0.0).unwrap()));
        time(&format!("map, all {} parts", src.len()), &mut || drop(thermal_map(&b, &src, 35.0, &spec).unwrap()));
        time("matrix, 24 hottest", &mut || drop(thermal_matrix(&b, &hot, 35.0, &spec).unwrap()));
        time("window: map of all and pairs of the 24", &mut || {
            drop(thermal_map(&b, &src, 35.0, &spec).unwrap());
            drop(hot_pairs(&b, &hot, 35.0, &spec).unwrap());
        });
        time("warm-up 600 s, all parts", &mut || drop(thermal_transient(&b, &src, &[], 35.0, &spec, 600.0, 8).unwrap()));
    }

    /// NDT3055L on its three data sheet points (110 K/W on the minimum pad, 95 on 0.066 in², 42 on 1 in², 2 oz copper, θjc 12): the
    /// grid with the part's body to the air (`BODY_SHARE`, fitted to the first point only) gives all three within 12 %.
    #[test]
    fn the_grid_follows_the_datasheet_curve_of_a_sot223() {
        let q1 = HeatSource { refdes: "Q1".into(), power: 1.0, theta_jc: 12.0, theta_body: body_theta(110.0 - 12.0), ..Default::default() };
        let spec = ThermalSpec { cell_mm: 1.0, ..Default::default() };
        for (a, want) in [(0.0, 110.0), (0.066 * 645.16, 95.0), (645.16, 42.0)] {
            let m = thermal_map(&sot223_board(a), std::slice::from_ref(&q1), 70.0, &spec).unwrap();
            let theta = m.parts[0].junction - 25.0;
            if std::env::var("THERMAL_DEBUG").is_ok() {
                eprintln!("{a:.0} mm²: {theta:.1} K/W (data sheet {want}), pads {:.1}, h {:.1} / {:.1}", m.parts[0].pad_temp - 25.0, m.h_top, m.h_bottom);
            }
            assert!((theta / want - 1.0).abs() < 0.12, "{a:.0} mm²: {theta:.1} K/W, the data sheet says {want}");
            // the junction is above the pads by about θjc times what goes into the board
            assert!(m.parts[0].junction > m.parts[0].pad_temp);
        }
    }
}
