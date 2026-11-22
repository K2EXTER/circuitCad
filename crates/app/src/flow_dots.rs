//! Dots that move along the wires with the current (X1): on the schematic's DC result and on the breadboard while it runs.
//!
//! Which way a dot goes is the way the (conventional) current flows; how fast is on a log scale, so a µA and an amp are both seen
//! moving on one sheet (1 µA slowly, 1 A quickly; below 0.1 µA no dots). Whether dots are shown and how fast they go are settings
//! (the settings window's 表示 tab, and the check box beside 「電流表示」), kept here so both views read the same ones.

use eframe::egui::{self, Color32, Pos2};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Below this no dots move (a capacitor at DC, the leakage of an idle pin).
pub const MIN_AMPS: f64 = 1e-7;

/// The speeds offered: (factor, label, hover text).
pub const SPEEDS: &[(f32, &str, &str)] = &[(0.5, "遅い", "点がゆっくり動きます。電流の違いを落ち着いて見比べたいときに"), (1.0, "ふつう", "はじめの速さです"), (2.0, "速い", "点が速く動きます。小さな電流でも動きが分かりやすくなります")];

static ON: AtomicBool = AtomicBool::new(true);
static SPEED: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0f32

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn set_on(v: bool) {
    ON.store(v, Ordering::Relaxed);
}

pub fn speed() -> f32 {
    f32::from_bits(SPEED.load(Ordering::Relaxed))
}

pub fn set_speed(v: f32) {
    SPEED.store(v.clamp(0.1, 10.0).to_bits(), Ordering::Relaxed);
}

/// How many dot spacings a dot moves in a second at `amps`: 0.35 at 1 µA, 1.4 at 1 mA, 2.45 at 1 A (times the speed setting).
pub fn rate(amps: f64) -> f32 {
    let a = amps.abs();
    if a < MIN_AMPS {
        return 0.0;
    }
    (0.35 * (a.log10() + 7.0).max(0.0) * speed() as f64) as f32
}

/// The dots of one straight piece from `a` to `b` carrying `amps` (positive: from `a` to `b`) at time `t` seconds, `spacing`
/// pixels apart. The phase is counted from `a` so pieces that meet end to end keep a steady stream.
pub fn dots_on(a: Pos2, b: Pos2, amps: f64, t: f64, spacing: f32) -> Vec<Pos2> {
    dots_along(&[a, b], amps, t, spacing)
}

/// The same along a line of several points (a wire drawn as a curve), from its first point to its last.
pub fn dots_along(pts: &[Pos2], amps: f64, t: f64, spacing: f32) -> Vec<Pos2> {
    let r = rate(amps);
    let len: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    if r == 0.0 || len < 1.0 || spacing < 2.0 {
        return vec![];
    }
    let shift = ((t * r as f64).fract() as f32) * spacing;
    let start = if amps > 0.0 { shift } else { spacing - shift };
    let mut out = vec![];
    // walk the line: `s` the distance of the next dot from the start, `done` the length of the pieces passed
    let (mut s, mut done) = (start % spacing, 0.0f32);
    for w in pts.windows(2) {
        let l = (w[1] - w[0]).length();
        while l > 0.0 && s < done + l {
            out.push(w[0] + (w[1] - w[0]) * ((s - done) / l));
            s += spacing;
        }
        done += l;
    }
    out
}

/// Draws the dots of a piece: a small disc with a dark rim, so it shows on a light and a dark wire alike.
pub fn paint(painter: &egui::Painter, dots: &[Pos2], radius: f32, col: Color32) {
    for &p in dots {
        painter.circle_filled(p, radius + 1.0, Color32::from_black_alpha(140));
        painter.circle_filled(p, radius, col);
    }
}

/// Keep the picture moving while dots are shown (about 30 frames a second, not as fast as the screen can go).
pub fn keep_moving(ctx: &egui::Context) {
    ctx.request_repaint_after(std::time::Duration::from_millis(33));
}

/// The settings' rows: on / off and the speed (the settings window's 表示 tab).
pub fn settings_rows(ui: &mut egui::Ui, on: &mut bool, speed: &mut f32) {
    ui.checkbox(on, i18n::tr("電流の流れを動く点で見せる")).on_hover_text(i18n::tr("DC動作点の結果と、試作のタブで電源を入れて動かしている間、電線の上を点が電流の向きに動きます。速さは電流の大きさ (1 µA でゆっくり、1 A で速く)。0.1 µA より小さい所は動きません。慣れて邪魔に感じたら切ります (回路図の「電流表示」のとなりでも切れます)"));
    crate::widgets::unless_ui(ui, (!*on).then_some("「電流の流れを動く点で見せる」を入れると変えられます"), |ui| {
        ui.horizontal(|ui| {
            ui.label(i18n::tr("点の速さ"));
            for (v, label, tip) in SPEEDS {
                if ui.selectable_label((*speed - v).abs() < 1e-3, i18n::tr(label)).on_hover_text(i18n::tr(tip)).clicked() {
                    *speed = *v;
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faster_for_more_current_and_none_below_the_floor() {
        set_speed(1.0);
        assert_eq!(rate(5e-8), 0.0);
        assert!(rate(1e-6) < rate(1e-3) && rate(1e-3) < rate(1.0));
        assert!((rate(1e-3) - 1.4).abs() < 1e-6 && (rate(-1e-3) - 1.4).abs() < 1e-6);
    }

    #[test]
    fn dots_move_the_way_the_current_flows() {
        set_speed(1.0);
        let (a, b) = (Pos2::new(0.0, 0.0), Pos2::new(100.0, 0.0));
        let first = |amps: f64, t: f64| dots_on(a, b, amps, t, 20.0)[0].x;
        // a little later, a dot of a forward current is further along; of a backward current, further back
        let (x0, x1) = (first(1e-3, 0.01), first(1e-3, 0.05));
        assert!(x1 > x0, "{x0} -> {x1}");
        let (y0, y1) = (first(-1e-3, 0.01), first(-1e-3, 0.05));
        assert!(y1 < y0, "{y0} -> {y1}");
        // 100 px at 20 px apart: 5 dots, none past the end
        assert_eq!(dots_on(a, b, 1e-3, 0.3, 20.0).len(), 5);
        assert!(dots_on(a, b, 1e-9, 0.3, 20.0).is_empty());
    }
}
