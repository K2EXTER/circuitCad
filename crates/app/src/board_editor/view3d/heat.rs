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
}

/// The narrowest scale (K): the hot end is at least this far above the ambient.
pub(super) const MIN_SPAN: f64 = 10.0;

/// The ends of the colour scale (°C).
pub(super) fn scale(t: &PartTemps) -> (f64, f64) {
    let top = t.parts.iter().map(|p| p.1).fold(t.ambient, f64::max);
    (t.ambient, top.max(t.ambient + MIN_SPAN))
}

/// A thermal camera's palette ("ironbow"): dark blue at `f` = 0 through purple, red and orange to pale yellow at 1.
pub(super) fn heat_color(f: f64) -> [u8; 3] {
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
    painter.rect_filled(Rect::from_min_max(bar.min - Vec2::new(8.0, 28.0), Pos2::new(rect.right() - 6.0, bar.bottom() + 10.0)), 4.0, Color32::from_black_alpha(150));
    const N: usize = 32;
    for k in 0..N {
        let f = k as f32 / N as f32;
        let c = heat_color(1.0 - (k as f64 + 0.5) / N as f64);
        let r = Rect::from_min_max(Pos2::new(bar.left(), bar.top() + f * h), Pos2::new(bar.right(), bar.top() + (f + 1.0 / N as f32) * h + 0.5));
        painter.rect_filled(r, 0.0, Color32::from_rgb(c[0], c[1], c[2]));
    }
    let font = egui::FontId::proportional(11.0);
    painter.text(Pos2::new(bar.left() - 2.0, bar.top() - 22.0), egui::Align2::LEFT_TOP, i18n::tr("温度 (目安)"), font.clone(), Color32::WHITE);
    painter.text(Pos2::new(bar.right() + 4.0, bar.top()), egui::Align2::LEFT_TOP, format!("{hi:.0} ℃"), font.clone(), Color32::WHITE);
    painter.text(Pos2::new(bar.right() + 4.0, bar.bottom()), egui::Align2::LEFT_BOTTOM, format!("{lo:.0} ℃"), font, Color32::WHITE);
}

impl BoardEditor {
    /// The temperatures of the schematic's self-heating run (`None`: no run, or it failed), for the heat colours of the 3D view.
    pub(crate) fn set_part_temps(&mut self, temps: Option<PartTemps>) {
        self.view3d.temps = temps;
    }

    /// Paints the parts by temperature in the 3D view (`--3d-heat`, for screenshots).
    pub fn view3d_heat(&mut self, on: bool) {
        self.view3d.heat_on = on;
    }
}
