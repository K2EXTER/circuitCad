//! Heat colours for the 3D view (E9 ③): every part painted by its temperature from the schematic's "自己発熱" run, like a thermal
//! camera. The App hands the temperatures over (`BoardEditor::set_part_temps`); parts the run did not warm are at the ambient
//! temperature, the cold end of the scale. The scale runs from the ambient to the hottest part (at least `MIN_SPAN` K, so a
//! barely warm part does not look red hot).

use super::*;

/// The temperatures of the schematic's last self-heating run, matched to the board's parts by reference.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct PartTemps {
    /// The surrounding air (°C).
    pub ambient: f64,
    /// `(refdes, temperature °C, power W)` of the parts the run warmed.
    pub parts: Vec<(String, f64, f64)>,
    /// The hottest spot of the board's heat map when the board is painted too (°C; 0 = no map): the scale reaches it.
    pub hottest: f64,
    /// Where the temperatures come from, shown under the legend ("熱の地図", and on a second line "38 秒後" for a moment of the map over
    /// time); empty = the self-heating run.
    pub source: String,
}

/// The narrowest scale (K): the hot end is at least this far above the ambient.
pub(super) const MIN_SPAN: f64 = 10.0;

/// The ends of the colour scale (°C).
pub(super) fn scale(t: &PartTemps) -> (f64, f64) {
    let top = t.parts.iter().map(|p| p.1).fold(t.ambient.max(t.hottest), f64::max);
    (t.ambient, top.max(t.ambient + MIN_SPAN))
}

/// A thermal camera's palette ("ironbow"): dark blue at `f` = 0 through purple, red and orange to pale yellow at 1.
pub(crate) fn heat_color(f: f64) -> [u8; 3] {
    const STOPS: [[f64; 3]; 6] = [[20.0, 24.0, 90.0], [110.0, 20.0, 150.0], [200.0, 30.0, 90.0], [240.0, 100.0, 20.0], [255.0, 190.0, 40.0], [255.0, 250.0, 200.0]];
    let f = if f.is_finite() { f.clamp(0.0, 1.0) } else { 0.0 };
    let x = f * (STOPS.len() - 1) as f64;
    let i = (x.floor() as usize).min(STOPS.len() - 2);
    let t = x - i as f64;
    let (a, b) = (STOPS[i], STOPS[i + 1]);
    [0, 1, 2].map(|k| (a[k] + (b[k] - a[k]) * t).round() as u8)
}

impl Scene {
    /// Paints the parts by temperature (`None`: their own colours). Returns whether anything changed, so the picture is redrawn only then.
    pub(super) fn set_heat(&mut self, temps: Option<&PartTemps>) -> bool {
        if self.heat_applied.as_ref() == temps {
            return false;
        }
        self.heat_applied = temps.cloned();
        let Some(t) = temps else {
            self.heat = vec![None; self.parts.len()];
            self.temps = vec![None; self.parts.len()];
            return true;
        };
        let (lo, hi) = scale(t);
        self.temps = self.parts.iter().map(|(r, ..)| t.parts.iter().find(|p| &p.0 == r).map(|p| (p.1, p.2))).collect();
        self.heat = self.temps.iter().map(|tp| Some(heat_color((tp.map_or(t.ambient, |p| p.0) - lo) / (hi - lo)))).collect();
        true
    }

    /// The colour a part is painted with: its temperature, red over the case height or running into the case read from its STEP, or its own.
    pub(super) fn part_color(&self, part: usize, own: [u8; 3]) -> [u8; 3] {
        if let Some(Some(c)) = self.heat.get(part) {
            return *c;
        }
        if self.over.get(part).is_some_and(|o| *o > 1e-6) || self.case_hits.get(part).copied().unwrap_or(false) { [225, 50, 40] } else { own }
    }

    /// Whether the part keeps its colour in the white look (painted by temperature or over the case height).
    pub(super) fn part_marked(&self, part: usize) -> bool {
        self.heat.get(part).is_some_and(Option::is_some) || self.over.get(part).is_some_and(|o| *o > 1e-6) || self.case_hits.get(part).copied().unwrap_or(false)
    }
}

/// The legend over the picture: the colour bar from the ambient to the hottest part, in its top right corner.
pub(super) fn legend(painter: &egui::Painter, rect: Rect, t: &PartTemps) {
    let (lo, hi) = scale(t);
    let (w, h) = (16.0, (rect.height() * 0.4).clamp(80.0, 220.0));
    let bar = Rect::from_min_size(Pos2::new(rect.right() - 70.0, rect.top() + 34.0), Vec2::new(w, h));
    let foot = 10.0 + 14.0 * t.source.lines().count() as f32;
    painter.rect_filled(Rect::from_min_max(bar.min - Vec2::new(8.0, 28.0), Pos2::new(rect.right() - 6.0, bar.bottom() + foot)), 4.0, Color32::from_black_alpha(150));
    const N: usize = 32;
    for k in 0..N {
        let f = k as f32 / N as f32;
        let c = heat_color(1.0 - (k as f64 + 0.5) / N as f64);
        let r = Rect::from_min_max(Pos2::new(bar.left(), bar.top() + f * h), Pos2::new(bar.right(), bar.top() + (f + 1.0 / N as f32) * h + 0.5));
        painter.rect_filled(r, 0.0, Color32::from_rgb(c[0], c[1], c[2]));
    }
    let font = egui::FontId::proportional(11.0);
    painter.text(Pos2::new(bar.left() - 2.0, bar.top() - 22.0), egui::Align2::LEFT_TOP, i18n::tr("温度 (目安)"), font.clone(), Color32::WHITE);
    if !t.source.is_empty() {
        painter.text(Pos2::new(bar.left() - 2.0, bar.bottom() + 8.0), egui::Align2::LEFT_TOP, &t.source, font.clone(), Color32::WHITE);
    }
    painter.text(Pos2::new(bar.right() + 4.0, bar.top()), egui::Align2::LEFT_TOP, format!("{hi:.0} ℃"), font.clone(), Color32::WHITE);
    painter.text(Pos2::new(bar.right() + 4.0, bar.bottom()), egui::Align2::LEFT_BOTTOM, format!("{lo:.0} ℃"), font, Color32::WHITE);
}

/// Paints a face's picture (`bottom`: the mirrored one) with the board's heat map: each board pixel takes the colour of its cell's
/// temperature on `temps` (one layer of the map), mixed with what was drawn there so the copper and the silk still show.
pub(super) fn tint(img: &mut [[u8; 4]], frame: pcb_core::gerber_view::Frame, bottom: bool, grid: &pcb_core::thermal::Grid, temps: &[f64], lo: f64, hi: f64) {
    for j in 0..frame.h {
        for i in 0..frame.w {
            let px = &mut img[j * frame.w + i];
            if px[3] == 0 {
                continue;
            }
            let col = if bottom { frame.w as f64 - i as f64 - 0.5 } else { i as f64 + 0.5 };
            let p = pcb_core::model::Point::new(frame.x0 + col / frame.px, frame.y1 - (j as f64 + 0.5) / frame.px);
            let Some(t) = grid.cell_at(p).map(|c| temps[c]).filter(|t| t.is_finite()) else { continue };
            let c = heat_color((t - lo) / (hi - lo));
            for k in 0..3 {
                px[k] = (0.7 * c[k] as f64 + 0.3 * px[k] as f64).round() as u8;
            }
        }
    }
}

impl BoardEditor {
    /// With the heat colours on and the board's heat map worked out: the parts take their junctions from the map (or the moment of the
    /// map over time picked in its window), and the board's two faces are painted with its top and bottom layers on the same scale.
    /// Without a map: the self-heating run's temperatures and the faces as they are. The faces are painted again only when the map,
    /// the moment or the look changes.
    pub(super) fn sync_heat_map(&mut self) {
        let shown = if self.view3d.heat_on { self.shown_heat() } else { None };
        let key = shown.as_ref().map(|sh| (self.thermal.serial, sh.frame, self.view3d.mask, self.view3d.silk));
        let Some(sh) = shown else {
            if self.view3d.board_heat.is_some() {
                self.view3d.board_heat = None;
                if let Some(s) = &mut self.view3d.scene {
                    s.img = Scene::images(&s.pv, self.view3d.mask, self.view3d.silk);
                    self.view3d.generation += 1;
                }
            }
            return;
        };
        let source = match sh.moment {
            Some(t) => format!("{}\n{}", i18n::tr("熱の地図"), i18n::trf!("{:.0} 秒後", t)),
            None => i18n::tr("熱の地図").to_string(),
        };
        let temps = PartTemps { ambient: sh.ambient, parts: sh.parts.clone(), hottest: sh.scale.1, source };
        if self.view3d.board_heat != key {
            let (lo, hi) = sh.scale;
            if let Some(s) = &mut self.view3d.scene {
                let mut img = Scene::images(&s.pv, self.view3d.mask, self.view3d.silk);
                let last = sh.layers.len() - 1;
                tint(&mut img[0], s.pv.frame, false, &sh.grid, &sh.layers[0], lo, hi);
                tint(&mut img[1], s.pv.frame, true, &sh.grid, &sh.layers[last], lo, hi);
                s.img = img;
                self.view3d.generation += 1;
                self.view3d.board_heat = key;
            }
        }
        self.view3d.temps = Some(temps);
    }

    /// The temperatures of the schematic's self-heating run (`None`: no run, or it failed), for the heat colours of the 3D view.
    pub(crate) fn set_part_temps(&mut self, temps: Option<PartTemps>) {
        self.view3d.temps = temps;
    }

    /// Paints the parts by temperature in the 3D view (`--3d-heat`, for screenshots).
    pub fn view3d_heat(&mut self, on: bool) {
        self.view3d.heat_on = on;
    }
}
