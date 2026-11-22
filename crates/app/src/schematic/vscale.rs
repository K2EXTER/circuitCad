//! The voltage colours of the wires (the steps, the legend with its two switches, the net the voltages are measured
//! from) and a DC result kept after the circuit changed: drawn faint, with a band that offers to compute it again.
//! `docs/code-notes/app/ui-rules.md` "2026-11-17 に決めた決まり".

use super::*;

/// How strongly a result from before the last change is drawn.
const STALE_OPACITY: f32 = 0.35;

impl SchEditor {
    /// The DC result is from before the last change to the circuit: it stays on the sheet, faint.
    pub(super) fn dc_stale(&self) -> bool {
        self.dc.as_ref().is_some_and(|d| d.signature != self.signature_cached())
    }

    /// The painter for the figures of a result: faint when the result is out of date.
    pub(super) fn result_painter(&self, painter: &egui::Painter) -> egui::Painter {
        let mut p = painter.clone();
        if self.dc_stale() {
            p.multiply_opacity(STALE_OPACITY);
        }
        p
    }

    /// The voltage of wire `i` in the result. After a change the list of wires may have changed, so the wire is found by
    /// its two ends (a wire that is new has no voltage).
    pub(super) fn wire_voltage(&self, d: &DcView, i: usize) -> Option<f64> {
        if d.signature == self.signature_cached() {
            return d.wire_v.get(i).copied().flatten();
        }
        let w = self.sch.wires.get(i)?;
        d.wire_ends.iter().position(|&(a, b)| (a, b) == (w.a, w.b) || (b, a) == (w.a, w.b)).and_then(|j| d.wire_v[j])
    }

    /// The voltage the colours are measured from (the reference net, or 0 V) and the range of the wires shown, measured
    /// from it (lo ≤ 0 ≤ hi). Only the wires count: the nodes inside a part's model (an op-amp's −5801 V) made every
    /// wire the top colour.
    pub(super) fn volt_scale_of(&self, d: &DcView) -> (f64, (f64, f64)) {
        let r = self.reference_voltage(d).unwrap_or(0.0);
        let range = (0..self.sch.wires.len()).filter_map(|i| self.wire_voltage(d, i)).map(|v| v - r).fold((0.0f64, 0.0f64), |(lo, hi), v| (lo.min(v), hi.max(v)));
        (r, range)
    }

    fn reference_voltage(&self, d: &DcView) -> Option<f64> {
        d.outcome.as_ref().ok()?.voltage(self.vscale.reference.as_ref()?)
    }

    /// The legend in the canvas's bottom-right corner: what each colour means, and the two switches (steps of the
    /// largest voltage or steps of ten; from ground or from a chosen net).
    pub(super) fn volt_legend(&mut self, ui: &egui::Ui, rect: Rect) {
        let Some(d) = self.dc.as_ref().filter(|d| self.show_v && d.outcome.is_ok()) else { return };
        let (_, (lo, hi)) = self.volt_scale_of(d);
        if lo == 0.0 && hi == 0.0 && self.vscale.reference.is_none() {
            return; // nothing powers the circuit: every wire is gray, and the status line says why
        }
        let decades = self.vscale.decades;
        let cells = legend_cells((lo, hi), decades);
        let what = match &self.vscale.reference {
            Some(n) => i18n::trf!("{n} からの差 (V)"),
            None => i18n::tr("電圧 (V)").to_string(),
        };
        let how = if decades { i18n::tr("10 倍ごと (段の下の値)") } else { i18n::tr("比例 (段の上の値)") };
        let mut vscale = self.vscale.clone();
        let pos = rect.right_bottom() - Vec2::new(14.0, 14.0);
        egui::Area::new(egui::Id::new("sch_volt_legend")).order(egui::Order::Foreground).pivot(Align2::RIGHT_BOTTOM).fixed_pos(pos).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                let (cw, ch) = (34.0, 10.0);
                // as wide as the cells: the switches below would otherwise stretch the box across the canvas
                ui.set_max_width(cw * cells.len() as f32);
                ui.label(egui::RichText::new(format!("{what}  {how}")).small());
                let (r, _) = ui.allocate_exact_size(Vec2::new(cw * cells.len() as f32, ch + 16.0), Sense::hover());
                let p = ui.painter();
                for (k, (step, text)) in cells.iter().enumerate() {
                    let x = r.left() + cw * k as f32;
                    p.rect_filled(Rect::from_min_size(Pos2::new(x + 1.0, r.top()), Vec2::new(cw - 2.0, ch)), 2.0, step_colour(*step));
                    p.text(Pos2::new(x + cw / 2.0, r.top() + ch + 2.0), Align2::CENTER_TOP, text, FontId::proportional(12.0), ui.visuals().text_color());
                }
                ui.horizontal(|ui| {
                    let bar = crate::widgets::segment_bar(ui, &[("比例", None), ("10 倍ごと", None)], Some(decades as usize));
                    if bar[0].clone().on_hover_text(i18n::tr("いちばん高い電圧を 5 段に分けて色を付けます。どのネットがだいたい同じ電圧かが分かります")).clicked() {
                        vscale.decades = false;
                    }
                    if bar[1].clone().on_hover_text(i18n::tr("1 mV・10 mV・100 mV・1 V・10 V の 10 倍ごとに色を変えます。電源が大きく、信号が mV のときに、小さな信号にも色が付きます")).clicked() {
                        vscale.decades = true;
                    }
                    if vscale.reference.is_some()
                        && ui.small_button(i18n::tr("基準を GND に")).on_hover_text(i18n::tr("色を GND からの電圧に戻します")).clicked()
                    {
                        vscale.reference = None;
                    }
                });
                if vscale.reference.is_none() {
                    ui.label(egui::RichText::new(i18n::tr("ネットを右クリックで、そこを基準にできます")).small().weak());
                }
            });
        });
        self.vscale = vscale;
    }

    /// Choosing the reference net from the right-click on a wire, pin or label (in the Select tool, with a DC result).
    /// Measuring from a net also switches to steps of ten: the differences left are small.
    /// The menu also puts a review note there (always; the net choices only with a DC result shown).
    pub(super) fn open_net_menu(&mut self, at: Pt, pos: Pos2) -> bool {
        let with_dc = self.show_v && self.dc.as_ref().is_some_and(|d| d.outcome.is_ok());
        let net = if with_dc { net_at(&self.sch, at) } else { None };
        self.net_menu = Some((net, pos, at));
        true
    }

    pub(super) fn net_menu_ui(&mut self, ui: &egui::Ui) {
        let Some((net, pos, at)) = self.net_menu.clone() else { return };
        let mut keep = true;
        egui::Area::new(egui::Id::new("sch_net_menu")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                if ui.button(i18n::tr("ここに付箋を置く")).on_hover_text(i18n::tr("この場所にコメント (付箋) を置きます。返事を付けたり、解決済みにしたりできます。ファイルに保存されます")).clicked() {
                    crate::notes_ui::add(&mut self.sch.notes, &mut self.notes_ui, at.0 as f64, at.1 as f64, self.sch.active_sheet);
                    keep = false;
                }
                let Some(net) = &net else { return };
                let here = self.vscale.reference.as_deref() == Some(net.as_str());
                if !here
                    && ui
                        .button(i18n::trf!("「{net}」を基準にする"))
                        .on_hover_text(i18n::tr("線の色を、このネットからの電圧の差で付けます (オシロスコープのグラウンドを付け替えるのと同じです)。小さな電圧降下が見えます"))
                        .clicked()
                {
                    self.vscale = VoltScale { decades: true, reference: Some(net.clone()) };
                    keep = false;
                }
                if self.vscale.reference.is_some() && ui.button(i18n::tr("基準を GND に戻す")).on_hover_text(i18n::tr("色を GND からの電圧に戻します")).clicked() {
                    self.vscale.reference = None;
                    keep = false;
                }
            });
            if ui.input(|i| i.pointer.any_pressed()) && !ui.rect_contains_pointer(ui.min_rect()) {
                keep = false;
            }
        });
        if !keep || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.net_menu = None;
        }
    }

    /// The band over the sheet while the result is from before the last change.
    pub(super) fn stale_band(&mut self, ui: &egui::Ui, rect: Rect) {
        if !self.dc_stale() || !(self.show_v || self.show_i) || self.dc.as_ref().is_some_and(|d| d.outcome.is_err()) {
            return;
        }
        let mut again = false;
        egui::Area::new(egui::Id::new("sch_stale_band")).order(egui::Order::Foreground).pivot(Align2::CENTER_TOP).fixed_pos(rect.center_top() + Vec2::new(0.0, 10.0)).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(i18n::tr("回路を変えたので、薄い数字は変える前の結果です"));
                    again = ui.button(i18n::tr("再計算 (F5)")).on_hover_text(i18n::tr("いまの回路で、最後に動かした解析をもう一度計算します")).clicked();
                });
            });
        });
        if again {
            self.rerun_last();
        }
    }
}

/// The cells of the legend, left to right: (step, the value written under it).
fn legend_cells((lo, hi): (f64, f64), decades: bool) -> Vec<(i32, String)> {
    let mut out = vec![];
    if decades {
        const NAMES: [&str; 5] = ["1m", "10m", "100m", "1", "10"];
        if lo < -DECADE_FLOOR {
            out.extend((1..=5).rev().map(|k| (-k, format!("-{}", NAMES[k as usize - 1]))));
        }
        out.push((0, "0".into()));
        if hi > DECADE_FLOOR || lo >= -DECADE_FLOOR {
            out.extend((1..=5).map(|k| (k, NAMES[k as usize - 1].to_string())));
        }
        return out;
    }
    if lo < 0.0 {
        out.extend((1..=5).rev().map(|k| (-k, short_volts(lo * k as f64 / 5.0))));
    }
    out.push((0, "0".into()));
    if hi > 0.0 {
        out.extend((1..=5).map(|k| (k, short_volts(hi * k as f64 / 5.0))));
    }
    out
}

/// A value for the legend: 3 figures at most, with m / µ (1.8, 240m, 12).
fn short_volts(v: f64) -> String {
    let a = v.abs();
    let (x, unit) = if a >= 1.0 || a == 0.0 { (v, "") } else if a >= 1e-3 { (v * 1e3, "m") } else { (v * 1e6, "µ") };
    let s = if x.abs() >= 100.0 { format!("{x:.0}") } else if x.abs() >= 10.0 { format!("{x:.1}") } else { format!("{x:.2}") };
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    format!("{s}{unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_in_proportion_and_in_decades() {
        // in proportion: 5 steps on each side of the largest voltage, a hundredth of it counts as 0 V
        assert_eq!(volt_step(5.0, (-5.0, 5.0), false), 5);
        assert_eq!(volt_step(1.0, (-5.0, 5.0), false), 1);
        assert_eq!(volt_step(-0.033, (-5.0, 5.0), false), 0, "a 33 mV signal next to ±5 V rails is gray in proportion");
        assert_eq!(volt_step(-5.0, (-5.0, 5.0), false), -5);
        // in decades the same signal has its own step
        assert_eq!(volt_step(-0.033, (-5.0, 5.0), true), -2);
        assert_eq!(volt_step(-0.003, (-5.0, 5.0), true), -1);
        assert_eq!(volt_step(0.0005, (0.0, 5.0), true), 0);
        assert_eq!(volt_step(5.0, (0.0, 5.0), true), 4);
        assert_eq!(volt_step(48.0, (0.0, 48.0), true), 5);
    }

    #[test]
    fn the_legend_reads_like_the_steps() {
        let cells = legend_cells((0.0, 5.0), false);
        assert_eq!(cells.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), ["0", "1", "2", "3", "4", "5"]);
        let cells = legend_cells((-0.006, 4.0), true);
        assert_eq!(cells.len(), 11, "both sides of ten steps and 0");
        assert_eq!(short_volts(0.24), "240m");
        assert_eq!(short_volts(1.8), "1.8");
        assert_eq!(short_volts(-3.3333), "-3.33");
    }
}

impl SchEditor {
    /// The review notes of this sheet as marks on the canvas, and their window (a note chosen in the list brings the view to it).
    pub(super) fn notes_on_canvas(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let (zoom, pan, c, sheet) = (self.zoom, self.pan, rect.center(), self.sch.active_sheet);
        let to_screen = |x: f64, y: f64| Pos2::new(c.x + (x as f32 - pan.x) * zoom, c.y + (y as f32 - pan.y) * zoom);
        crate::notes_ui::marks(ui, "sch", &self.sch.notes, &mut self.notes_ui, |n| n.sheet == sheet, to_screen);
        let many = self.sch.sheet_count() > 1;
        let names: Vec<String> = (0..self.sch.sheet_count()).map(|k| self.sch.sheet_name(k)).collect();
        let jump = crate::notes_ui::window(ui.ctx(), "回路図", &mut self.sch.notes, &mut self.notes_ui, |s| many.then(|| names.get(s).cloned().unwrap_or_default()));
        if let Some(crate::notes_ui::NoteAction::Jump(k)) = jump
            && let Some(n) = self.sch.notes.get(k)
        {
            let (x, y, s) = (n.x as f32, n.y as f32, n.sheet);
            if s != self.sch.active_sheet && s < self.sch.sheet_count() {
                self.show_sheet(s);
            }
            self.pan = Vec2::new(x, y);
        }
    }
}
