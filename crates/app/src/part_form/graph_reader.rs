//! "グラフから点を読む" in the data sheet window (M10, step 2): the person clicks the tick labels of both axes (or a
//! grid line and types its value), then clicks on the curve; the points come out as numbers, to copy (lines or
//! the `.fit.json` form) or to put into the form's field that asked for them. The arithmetic is `datasheet::graph`.

use datasheet::graph::{Axis, Graph, Tick, tick_value};
use eframe::egui::{self, Color32};

/// What a click on the page does now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    XTick,
    YTick,
    Points,
}

/// The form's field that asked for the points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    /// The diode's forward points ("current voltage" lines).
    DiodeForward,
}

impl Dest {
    fn button(self) -> &'static str {
        match self {
            Dest::DiodeForward => "ダイオードの「順方向特性の点」に入れる",
        }
    }
}

/// The points the person sent to the form.
pub struct GraphOut {
    pub dest: Dest,
    /// (first, second) in the order the form wants (already swapped and scaled).
    pub points: Vec<(f64, f64)>,
    pub page: usize,
}

/// Units the axis labels may be printed in: a label "10" on a "mA" axis is 0.01 A.
const SCALES: &[(&str, f64)] = &[("そのまま", 1.0), ("m (1/1000)", 1e-3), ("µ (1/100万)", 1e-6), ("n (1/10億)", 1e-9), ("p (1/1兆)", 1e-12), ("k (1000 倍)", 1e3)];

/// One axis as the person sets it: tick positions with the label text as typed or clicked.
#[derive(Default)]
struct AxisInput {
    log: bool,
    ticks: Vec<(f32, String)>,
    /// Index into `SCALES`.
    scale: usize,
}

impl AxisInput {
    /// The axis, or why it is not ready.
    fn axis(&self) -> Result<Axis, String> {
        let mut ticks = vec![];
        for (n, (at, text)) in self.ticks.iter().enumerate() {
            let value = tick_value(text).ok_or_else(|| format!("目盛り {} の値「{text}」が数字として読めません", n + 1))?;
            ticks.push(Tick { at: *at, value });
        }
        let a = Axis { log: self.log, ticks };
        match a.problem() {
            Some(p) => Err(p),
            None => Ok(a),
        }
    }

    fn scale(&self) -> f64 {
        SCALES.get(self.scale).map_or(1.0, |s| s.1)
    }
}

pub struct GraphReader {
    pub page: usize,
    pub mode: Option<Mode>,
    x: AxisInput,
    y: AxisInput,
    /// Picked points (page points).
    points: Vec<(f32, f32)>,
    /// Write the vertical axis's value first (a diode's graph has the voltage across and the current up; its field
    /// wants "current voltage").
    swap: bool,
    dest: Option<Dest>,
    msg: Option<(bool, String)>,
    out: Option<GraphOut>,
}

impl GraphReader {
    pub fn new(page: usize, dest: Option<Dest>) -> GraphReader {
        GraphReader {
            page,
            mode: Some(Mode::XTick),
            x: AxisInput::default(),
            y: AxisInput { log: dest == Some(Dest::DiodeForward), ..Default::default() },
            points: vec![],
            swap: dest == Some(Dest::DiodeForward),
            dest,
            msg: None,
            out: None,
        }
    }

    /// The graph as far as it is set (axes that are not ready are left empty).
    fn graph(&self) -> Graph {
        Graph { page: self.page, x: self.x.axis().unwrap_or_default(), y: self.y.axis().unwrap_or_default(), points: self.points.clone() }
    }

    /// The points as values in the output order (swapped and scaled), or why there are none.
    pub fn values(&self) -> Result<Vec<(f64, f64)>, String> {
        let x = self.x.axis().map_err(|e| format!("横軸: {e}"))?;
        let y = self.y.axis().map_err(|e| format!("縦軸: {e}"))?;
        if self.points.is_empty() {
            return Err("点がまだありません。「点を取る」を押して、曲線の上をクリックしてください".into());
        }
        let g = Graph { page: self.page, x, y, points: self.points.clone() };
        let (sx, sy) = (self.x.scale(), self.y.scale());
        let mut v: Vec<(f64, f64)> = g.values().unwrap_or_default().into_iter().map(|(a, b)| if self.swap { (b * sy, a * sx) } else { (a * sx, b * sy) }).collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(v)
    }

    pub fn take_out(&mut self) -> Option<GraphOut> {
        self.out.take()
    }

    /// A click on the page at `p` (page points). `label` is the word under the pointer when it reads as a number
    /// (a tick label: its centre is the tick's position).
    pub fn click(&mut self, p: (f32, f32), label: Option<(&str, (f32, f32))>) {
        match self.mode {
            Some(Mode::XTick) => match label {
                Some((t, c)) => self.x.ticks.push((c.0, t.to_string())),
                None => self.x.ticks.push((p.0, String::new())),
            },
            Some(Mode::YTick) => match label {
                Some((t, c)) => self.y.ticks.push((c.1, t.to_string())),
                None => self.y.ticks.push((p.1, String::new())),
            },
            Some(Mode::Points) => self.points.push(p),
            None => {}
        }
        // two ticks on the horizontal axis: go on with the vertical one; then with the points
        if self.mode == Some(Mode::XTick) && self.x.ticks.len() >= 2 && self.y.ticks.is_empty() {
            self.mode = Some(Mode::YTick);
        } else if self.mode == Some(Mode::YTick) && self.y.ticks.len() >= 2 && self.points.is_empty() && self.x.axis().is_ok() && self.y.axis().is_ok() {
            self.mode = Some(Mode::Points);
        }
    }

    /// Adds a point at values of the axes as printed (for `--form-graph`, screenshots): placed where the axes put it.
    pub fn add_point_at_values(&mut self, xv: f64, yv: f64) {
        if let (Ok(x), Ok(y)) = (self.x.axis(), self.y.axis())
            && let (Some(px), Some(py)) = (x.position(xv), y.position(yv))
        {
            self.points.push((px, py));
        }
    }

    /// The value under the pointer, for the tooltip while picking points.
    pub fn value_at(&self, p: (f32, f32)) -> Option<(f64, f64)> {
        let g = self.graph();
        let (a, b) = g.value_at(p.0, p.1)?;
        Some((a * self.x.scale(), b * self.y.scale()))
    }

    /// Draws the ticks (as lines across the page) and the points over the page picture.
    pub fn paint(&self, painter: &egui::Painter, to_screen: impl Fn(f32, f32) -> egui::Pos2, page_size: (f32, f32)) {
        let tick = Color32::from_rgba_unmultiplied(40, 120, 230, 170);
        for (at, text) in &self.x.ticks {
            let (a, b) = (to_screen(*at, 0.0), to_screen(*at, page_size.1));
            painter.line_segment([a, b], egui::Stroke::new(1.0, tick));
            painter.text(a + egui::vec2(3.0, 2.0), egui::Align2::LEFT_TOP, text, egui::FontId::proportional(14.0), tick);
        }
        for (at, text) in &self.y.ticks {
            let (a, b) = (to_screen(0.0, *at), to_screen(page_size.0, *at));
            painter.line_segment([a, b], egui::Stroke::new(1.0, tick));
            painter.text(a + egui::vec2(2.0, -2.0), egui::Align2::LEFT_BOTTOM, text, egui::FontId::proportional(14.0), tick);
        }
        let red = Color32::from_rgb(230, 40, 40);
        for (n, &(x, y)) in self.points.iter().enumerate() {
            let c = to_screen(x, y);
            painter.circle_stroke(c, 4.0, egui::Stroke::new(1.5, red));
            painter.line_segment([c - egui::vec2(6.0, 0.0), c + egui::vec2(6.0, 0.0)], egui::Stroke::new(1.0, red));
            painter.line_segment([c - egui::vec2(0.0, 6.0), c + egui::vec2(0.0, 6.0)], egui::Stroke::new(1.0, red));
            painter.text(c + egui::vec2(6.0, -6.0), egui::Align2::LEFT_BOTTOM, format!("{}", n + 1), egui::FontId::proportional(14.0), red);
        }
    }

    /// The panel. `shown` is the page on screen; returns false when the reading should be dropped.
    pub fn ui(&mut self, ui: &mut egui::Ui, shown: usize) -> bool {
        let mut keep = true;
        ui.label(egui::RichText::new(i18n::tr("1. 横軸の目盛りの数字を 2 つ以上クリック → 2. 縦軸の目盛りの数字を 2 つ以上クリック → 3. 曲線の上をクリックして点を取る。数字が文字でない図は、目盛りの線をクリックして値を入れます。拡大すると正確に取れます")).small());
        if shown != self.page {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(crate::theme::WARN, i18n::trf!("このグラフは p.{} のものです", self.page + 1));
                if ui.button(i18n::tr("このページで始め直す")).on_hover_text(i18n::tr("目盛りと点を消して、いま出ているページのグラフを読み始めます")).clicked() {
                    *self = GraphReader::new(shown, self.dest);
                }
            });
        }
        let mode = self.mode;
        let set_mode = |ui: &mut egui::Ui, m: Mode, label: &str, tip: &str, me: &mut Option<Mode>| {
            if ui.selectable_label(mode == Some(m), i18n::tr(label)).on_hover_text(i18n::tr(tip)).clicked() {
                *me = if mode == Some(m) { None } else { Some(m) };
            }
        };
        for (axis, m, name, tip) in [
            (&mut self.x, Mode::XTick, "横軸の目盛りを足す", "ページの横軸の目盛りの数字をクリックすると、その位置と値を目盛りにします (数字の真ん中が目盛りの位置)。数字でない所をクリックしたら、下の欄に値を入れます"),
            (&mut self.y, Mode::YTick, "縦軸の目盛りを足す", "ページの縦軸の目盛りの数字をクリックすると、その位置と値を目盛りにします。格子が対数どおりでない図は、数字の付いた目盛りを全部足すと、隣の目盛りの間で読みます"),
        ] {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                set_mode(ui, m, name, tip, &mut self.mode);
                ui.checkbox(&mut axis.log, i18n::tr("対数")).on_hover_text(i18n::tr("目盛りが 1・10・100 のように 10 倍ずつの軸なら入れます"));
                egui::ComboBox::from_id_salt((name, "scale")).selected_text(i18n::tr(SCALES[axis.scale].0)).show_ui(ui, |ui| {
                    for (i, (s, _)) in SCALES.iter().enumerate() {
                        ui.selectable_value(&mut axis.scale, i, i18n::tr(s));
                    }
                })
                .response
                .on_hover_text(i18n::tr("軸の単位の接頭語。軸が mA なら m を選ぶと、目盛りの「10」を 0.01 (A) として読みます"));
            });
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (i, (_, text)) in axis.ticks.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(text).desired_width(66.0).hint_text(i18n::tr("値"))).on_hover_text(i18n::tr("目盛りの値。ページの数字をそのまま (10、0.1、1E-3、10m など)"));
                    if ui.small_button("×").on_hover_text(i18n::tr("この目盛りを消します")).clicked() {
                        remove = Some(i);
                    }
                }
            });
            if let Some(i) = remove {
                axis.ticks.remove(i);
            }
            if !axis.ticks.is_empty()
                && let Err(e) = axis.axis()
            {
                ui.colored_label(crate::theme::WARN, e);
            }
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            set_mode(ui, Mode::Points, "点を取る", "曲線の上をクリックすると点を足します。ポインタの所の値が、そばに出ます", &mut self.mode);
            ui.label(i18n::trf!("{} 点", self.points.len()));
            if !self.points.is_empty() && ui.small_button(i18n::tr("最後の点を消す")).on_hover_text(i18n::tr("最後に取った点を消します")).clicked() {
                self.points.pop();
            }
            if !self.points.is_empty() && ui.small_button(i18n::tr("点を全部消す")).on_hover_text(i18n::tr("取った点を全部消します (目盛りは残します)")).clicked() {
                self.points.clear();
            }
        });
        ui.checkbox(&mut self.swap, i18n::tr("縦軸の値を先に書く")).on_hover_text(i18n::tr("ダイオードの順方向特性は横軸が電圧・縦軸が電流ですが、入れる欄は「電流 電圧」の順です。そのときは入れます"));
        match self.values() {
            Ok(v) => {
                let lines: Vec<String> = v.iter().map(|(a, b)| format!("{} {}", short(*a), short(*b))).collect();
                egui::ScrollArea::vertical().id_salt("graph_points").max_height(120.0).show(ui, |ui| {
                    ui.monospace(lines.join("\n"));
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button(i18n::tr("写す (1 行 1 点)")).on_hover_text(i18n::tr("「値 値」の行にして、クリップボードに写します")).clicked() {
                        ui.ctx().copy_text(lines.join("\n"));
                        self.msg = Some((true, i18n::trf!("{} 点を写しました", v.len())));
                    }
                    if ui.button(i18n::tr("写す (.fit.json の形)")).on_hover_text(i18n::tr("[[値, 値], ...] の形で写します。データシートの読みのファイル (crates/part-lib/fits/*.fit.json) に貼れます")).clicked() {
                        let items: Vec<String> = v.iter().map(|(a, b)| format!("[{}, {}]", json_number(*a), json_number(*b))).collect();
                        ui.ctx().copy_text(format!("[{}]", items.join(", ")));
                        self.msg = Some((true, i18n::trf!("{} 点を .fit.json の形で写しました", v.len())));
                    }
                    if let Some(d) = self.dest
                        && ui.button(i18n::tr(d.button())).on_hover_text(i18n::tr("取った点を、部品作成フォームの欄に入れます (前の中身は置き換えます)")).clicked()
                    {
                        self.out = Some(GraphOut { dest: d, points: v.clone(), page: self.page });
                        self.msg = Some((true, i18n::trf!("{} 点をフォームに入れました", v.len())));
                    }
                });
            }
            Err(e) => {
                ui.colored_label(crate::theme::INFO, e);
            }
        }
        if let Some((ok, m)) = &self.msg {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::ERROR }, m);
        }
        if ui.button(i18n::tr("グラフの読み取りを終える")).on_hover_text(i18n::tr("目盛りと点を消して、この窓を閉じます")).clicked() {
            keep = false;
        }
        keep
    }
}

/// A number for a "value value" line: plain for ordinary sizes, with an SI prefix for small ones (`10u`).
pub fn short(v: f64) -> String {
    if v != 0.0 && v.abs() < 1e-3 { part_lib::passive::eng(v) } else { part_lib::fit::num::g(v, 4) }
}

/// A number for JSON (no SI prefix).
fn json_number(v: f64) -> String {
    part_lib::fit::num::g(v, 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicking_labels_then_the_curve_gives_the_points_in_the_forms_order() {
        // a diode graph: VF across (0.4 V at x = 100, 1.2 V at x = 500), IF up on a log axis in mA (1 at y = 400, 1000 at y = 100)
        let mut g = GraphReader::new(2, Some(Dest::DiodeForward));
        assert_eq!(g.mode, Some(Mode::XTick));
        g.click((99.0, 600.0), Some(("0.4", (100.0, 600.0))));
        g.click((501.0, 600.0), Some(("1.2", (500.0, 600.0))));
        assert_eq!(g.mode, Some(Mode::YTick), "two ticks across: on to the vertical axis");
        g.click((40.0, 401.0), Some(("1", (40.0, 400.0))));
        g.click((40.0, 99.0), Some(("1000", (40.0, 100.0))));
        assert_eq!(g.mode, Some(Mode::Points));
        g.y.scale = 1; // mA
        g.click((300.0, 200.0), None);
        g.click((200.0, 300.0), None);
        let v = g.values().unwrap();
        // swapped (current first) and sorted by current: 10 mA at 0.6 V, 100 mA at 0.8 V
        assert!((v[0].0 - 0.01).abs() < 1e-12 && (v[0].1 - 0.6).abs() < 1e-9, "{v:?}");
        assert!((v[1].0 - 0.1).abs() < 1e-12 && (v[1].1 - 0.8).abs() < 1e-9, "{v:?}");
        assert_eq!(short(v[0].0), "0.01");
        assert_eq!(short(2e-5), "20u");
    }

    #[test]
    fn a_tick_off_the_labels_waits_for_its_value() {
        let mut g = GraphReader::new(0, None);
        g.click((10.0, 0.0), None);
        g.click((110.0, 0.0), None);
        let e = g.values().unwrap_err();
        assert!(e.contains("横軸") && e.contains("読めません"), "{e}");
        g.x.ticks[0].1 = "0".into();
        g.x.ticks[1].1 = "5".into();
        assert!(g.values().unwrap_err().contains("縦軸"));
    }
}
