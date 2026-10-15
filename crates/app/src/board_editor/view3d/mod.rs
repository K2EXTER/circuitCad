//! "3Dで見る" window: the board as the fab will make it (the picture rebuilt from the exported
//! fabrication files, put on the top and bottom faces) with solids for every part. Drawn by a small
//! software rasteriser (`raster3d`): perspective projection, back-face culling and a depth buffer, so what is
//! nearer always hides what is behind it. Hovering a part names it, clicking selects it in the board editor.

use super::raster3d::{outline_part, Buffers, Paint, Picture, Vert, NO_PART};
use super::*;
use pcb_core::gerber_view::Preview;
use pcb_core::model3d::{part_boxes, part_height_on, BodyShape, PartBox, BOARD_THICKNESS};

mod camera;
mod glow;
mod picture;
mod scene;
#[cfg(test)]
mod tests;
mod window;

use camera::*;
use glow::*;
use picture::*;
use scene::*;

/// Colour of the highlighted net, and how far (mm) it floats above the board so it wins the depth test.
const GLOW: [u8; 3] = [255, 60, 170];
const GLOW_LIFT: f64 = 0.05;
/// Largest zoom (the scroll wheel and "選択部品へ寄る" share it).
const MAX_ZOOM: f32 = 30.0;

pub(super) struct View3d {
    pub(super) open: bool,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    pan: Vec2,
    spin: bool,
    show_parts: bool,
    show_names: bool,
    mask: bool,
    silk: bool,
    scene: Option<Scene>,
    error: Option<String>,
    /// Rebuild the scene on the next frame (set when the window opens).
    stale: bool,
    /// The part under the pointer in the last picture (drawn brighter in the next one).
    hover: Option<usize>,
    /// Changes whenever the scene or its pictures change, so a kept picture is not reused for another.
    generation: u32,
    /// The last picture, kept while nothing that shows in it changes.
    cache: Option<Cached>,
    /// The net drawn in colour on the board, and the net of the editor's selection last followed (so picking
    /// another net in the list is not undone by the same selection every frame).
    net: Option<String>,
    followed: Option<String>,
    /// A part to move the camera to on the next frame.
    focus: Option<usize>,
    /// The case height check: on/off and the room above the top face and below the bottom face (mm).
    check_height: bool,
    limit_top: f64,
    limit_bottom: f64,
}

/// A finished picture of the scene: what it was drawn for, the image, and the part at each pixel.
struct Cached {
    key: PictureKey,
    tex: egui::TextureHandle,
    ids: Vec<u32>,
    size: [usize; 2],
}

#[derive(PartialEq)]
struct PictureKey {
    camera: [u32; 5],
    size: [usize; 2],
    parts: bool,
    hover: Option<usize>,
    selected: Option<usize>,
    generation: u32,
    /// Drawn coarsely (while the view is being dragged or zoomed) or fully.
    draft: bool,
}

impl Default for View3d {
    fn default() -> Self {
        Self { open: false, yaw: 0.0, pitch: 0.9, zoom: 1.0, pan: Vec2::ZERO, spin: false, show_parts: true, show_names: false, mask: true, silk: true, scene: None, error: None, stale: true, hover: None, generation: 0, cache: None, net: None, followed: None, focus: None, check_height: false, limit_top: 15.0, limit_bottom: 5.0 }
    }
}

impl BoardEditor {
    /// Opens (and rebuilds) the 3D window.
    pub fn open_view3d(&mut self) {
        self.view3d.open = true;
        self.view3d.stale = true;
    }

    /// Recomputes the copper of the highlighted net (after the scene was rebuilt or another net was picked).
    fn apply_net_glow(&mut self) {
        // with the finish on, the highlighted copper is the finished one
        let finished = self.finished();
        let glow = match (&self.view3d.net, &finished) {
            (Some(n), Some(f)) => net_glow(&f.board, &pcb_core::drc::connectivity(&f.board), n, &f.patches),
            (Some(n), None) => net_glow(&self.board, &self.cached_connectivity(), n, &[]),
            (None, _) => vec![],
        };
        if let Some(sc) = &mut self.view3d.scene {
            sc.glow = glow;
        }
        self.view3d.generation += 1;
    }
}
