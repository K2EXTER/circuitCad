//! The "熱の地図" window (candidate H7): the board's temperature worked out on a grid (`pcb_core::thermal`) from the schematic's
//! self-heating powers, a thermal-camera colour over the board in the editor, and a table of the parts. The App hands the heat sources
//! over when the window asks for them (`take_thermal_request`, `run_thermal_map`).

use super::*;
use pcb_core::thermal::{HeatSource, Orientation, ThermalMap, ThermalSpec};

#[derive(Default)]
pub struct ThermalView {
    pub open: bool,
    /// Colour the board in the editor.
    pub overlay: bool,
    /// The layer shown (index in the map's layers).
    pub layer: usize,
    pub spec: ThermalSpec,
    pub result: Option<Result<ThermalMap, String>>,
    /// The self-heating run's temperature of each part (°C), to compare.
    pub own_temps: Vec<(String, f64)>,
    /// The heat sources of the last map.
    sources: Vec<HeatSource>,
    /// The picture of the shown layer: (layer, texture).
    texture: Option<(usize, egui::TextureHandle)>,
    request: bool,
    /// Milliseconds the last map took.
    pub took_ms: f64,
    /// Parts that warm each other noticeably (`pcb_core::thermal::hot_pairs`).
    pub pairs: Vec<pcb_core::thermal::HotPair>,
    /// A "離す" requirement to add to the schematic's design spec (the App takes it).
    apart_request: Option<design_spec::KeepApart>,
}

impl BoardEditor {
    pub fn open_thermal_view(&mut self) {
        self.thermal.open = true;
    }

    /// Whether the window asked for a map (the App then works out the self-heating and hands the sources to `run_thermal_map`).
    pub(crate) fn take_thermal_request(&mut self) -> bool {
        std::mem::take(&mut self.thermal.request)
    }

    /// Asks for the "離す" requirement of the `k`-th hot pair (the App puts it into the schematic's design spec).
    pub(crate) fn suggest_apart(&mut self, k: usize) {
        let Some(p) = self.thermal.pairs.get(k) else { return };
        let (hot, from) = if p.a_from_b >= p.b_from_a { (&p.a, p.a_from_b) } else { (&p.b, p.b_from_a) };
        self.thermal.apart_request = Some(design_spec::KeepApart {
            name: format!("熱 {}-{}", p.a, p.b),
            a: vec![p.a.clone()],
            b: vec![p.b.clone()],
            min_dist: p.suggest_mm,
            note: i18n::trf!("熱の地図: {:.1} mm で {} が +{:.1} K 受けていた", p.dist_mm, hot, from),
        });
    }

    /// A "離す" requirement the window asked for, to go into the schematic's design spec.
    pub(crate) fn take_apart_request(&mut self) -> Option<design_spec::KeepApart> {
        self.thermal.apart_request.take()
    }

    /// The map with these heat sources (from the schematic's self-heating run) and the air's temperature.
    pub(crate) fn run_thermal_map(&mut self, sources: Result<(Vec<HeatSource>, Vec<(String, f64)>, f64), String>) {
        let t0 = std::time::Instant::now();
        self.thermal.texture = None;
        self.thermal.result = Some(sources.and_then(|(src, own, ambient)| {
            self.thermal.spec.ambient_c = ambient;
            self.thermal.own_temps = own;
            self.thermal.sources = src.clone();
            if src.is_empty() {
                return Err("電力の出ている部品がありません。回路図の「自己発熱」で電力の出る部品があるか確かめてください".into());
            }
            pcb_core::thermal::thermal_map(&self.board, &src, self.board.stackup.copper_um, &self.thermal.spec)
        }));
        // the pairs of parts that warm each other: the parts with real power only (as the schematic's heat runs take from the board)
        self.thermal.pairs = match &self.thermal.result {
            Some(Ok(m)) => {
                let top = m.parts.iter().map(|p| p.power).fold(0.0, f64::max);
                let mut hot: Vec<HeatSource> = m.parts.iter().filter(|p| p.power >= 1e-3 && p.power >= 0.01 * top).filter_map(|p| self.thermal.sources.iter().find(|s| s.refdes == p.refdes).cloned()).collect();
                hot.sort_by(|a, b| b.power.total_cmp(&a.power));
                hot.truncate(sim_core::BOARD_HEATERS_MAX);
                pcb_core::thermal::hot_pairs(&self.board, &hot, self.board.stackup.copper_um, &self.thermal.spec).unwrap_or_default()
            }
            _ => vec![],
        };
        self.thermal.took_ms = t0.elapsed().as_secs_f64() * 1e3;
        if let Some(Ok(_)) = &self.thermal.result {
            self.thermal.overlay = true;
        }
    }

    /// The scale of the colours (°C): the air to the hottest cell (at least 10 K).
    fn thermal_scale(m: &ThermalMap) -> (f64, f64) {
        let top = m.temp.iter().flatten().filter(|t| t.is_finite()).fold(m.ambient_c, |a, b| a.max(*b));
        (m.ambient_c, top.max(m.ambient_c + 10.0))
    }

    /// The colour of the shown layer over the board (after the copper, under the silk and the marks).
    pub(super) fn draw_thermal(&mut self, painter: &egui::Painter, rect: Rect) {
        if !self.thermal.overlay {
            return;
        }
        let Some(Ok(m)) = &self.thermal.result else { return };
        let layer = self.thermal.layer.min(m.layers.len() - 1);
        if self.thermal.texture.as_ref().is_none_or(|(l, _)| *l != layer) {
            let (lo, hi) = Self::thermal_scale(m);
            let g = &m.grid;
            let mut img = egui::ColorImage::new([g.w, g.h], vec![Color32::TRANSPARENT; g.w * g.h]);
            for (c, t) in m.temp[layer].iter().enumerate() {
                if t.is_finite() {
                    let [r, gg, b] = super::view3d::heat_color((t - lo) / (hi - lo));
                    // rows go up in y on the board, down in the picture
                    let (i, j) = (c % g.w, g.h - 1 - c / g.w);
                    img.pixels[j * g.w + i] = Color32::from_rgba_unmultiplied(r, gg, b, 170);
                }
            }
            self.thermal.texture = Some((layer, painter.ctx().load_texture("thermal_map", img, egui::TextureOptions::LINEAR)));
        }
        let Some((_, tex)) = &self.thermal.texture else { return };
        let g = &m.grid;
        let a = self.to_screen(rect, Point::new(g.x0, g.y0 + g.h as f64 * g.d));
        let b = self.to_screen(rect, Point::new(g.x0 + g.w as f64 * g.d, g.y0));
        painter.image(tex.id(), Rect::from_two_pos(a, b), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        // the scale, at the canvas's bottom left
        let (lo, hi) = Self::thermal_scale(m);
        let bar = Rect::from_min_size(Pos2::new(rect.left() + 56.0, rect.bottom() - 34.0), egui::vec2(160.0, 10.0));
        for k in 0..32 {
            let f = k as f64 / 31.0;
            let [r, gg, b] = super::view3d::heat_color(f);
            let x0 = bar.left() + bar.width() * k as f32 / 32.0;
            painter.rect_filled(Rect::from_min_size(Pos2::new(x0, bar.top()), egui::vec2(bar.width() / 32.0 + 0.5, bar.height())), 0.0, Color32::from_rgb(r, gg, b));
        }
        let font = egui::FontId::proportional(13.0);
        painter.text(Pos2::new(bar.left(), bar.top() - 2.0), egui::Align2::LEFT_BOTTOM, format!("{lo:.0} ℃"), font.clone(), Color32::WHITE);
        painter.text(Pos2::new(bar.right(), bar.top() - 2.0), egui::Align2::RIGHT_BOTTOM, format!("{hi:.0} ℃"), font.clone(), Color32::WHITE);
        painter.text(Pos2::new(bar.right() + 8.0, bar.center().y), egui::Align2::LEFT_CENTER, i18n::tr("熱の地図 (目安)"), font, Color32::WHITE);
    }

    pub fn thermal_window(&mut self, ctx: &egui::Context) {
        if !self.thermal.open {
            return;
        }
        let mut open = true;
        egui::Window::new(i18n::tr("熱の地図 (基板の温度)")).id(egui::Id::new("thermal_window")).open(&mut open).default_width(480.0).default_pos([420.0, 120.0]).show(ctx, |ui| {
            ui.label(egui::RichText::new(i18n::tr("回路図の「自己発熱」の電力で、基板のどこが何 ℃ になるかを、基板を細かい升目に分けて計算します。銅 (ベタ・配線・パッド・ビア) は熱を広げ、表と裏の面から空気へ、対流と放射で逃げます")).small());
            ui.horizontal(|ui| {
                ui.label(i18n::tr("置き方"));
                let s = &mut self.thermal.spec;
                egui::ComboBox::from_id_salt("thermal_orient")
                    .selected_text(match s.orientation {
                        Orientation::Flat => i18n::tr("平らに置く"),
                        Orientation::Standing => i18n::tr("立てる"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.orientation, Orientation::Flat, i18n::tr("平らに置く")).on_hover_text(i18n::tr("机の上や平たい箱の中。熱い面が上を向くと空気がよく動き、下を向くと半分ほどしか冷えません"));
                        ui.selectable_value(&mut s.orientation, Orientation::Standing, i18n::tr("立てる")).on_hover_text(i18n::tr("ラックや壁に立てる。両面とも空気が下から上へ流れます (高さは基板の縦の長さ)"));
                    });
                ui.label(i18n::tr("標高"));
                ui.add(egui::DragValue::new(&mut s.altitude_m).range(0.0..=5000.0).speed(10.0).suffix(" m")).on_hover_text(i18n::tr("高い所では空気が薄く、対流で運べる熱が減ります (3000 m で約 2 割)。放射は変わりません"));
            });
            ui.horizontal(|ui| {
                if crate::widgets::primary_button_wide(ui, true, "熱の地図を計算").on_hover_text(i18n::tr("回路図の自己発熱を計算し、その電力を部品のパッドから基板に入れて、温度の分布を出します。周囲の温度は回路図の「自己発熱」の値です")).clicked() {
                    self.thermal.request = true;
                }
            });
            match &self.thermal.result {
                None => {
                    ui.label(egui::RichText::new(i18n::tr("まだ計算していません")).weak());
                }
                Some(Err(e)) => {
                    ui.colored_label(crate::theme::ERROR, e);
                }
                Some(Ok(m)) => {
                    let (t, at) = m.hottest();
                    ui.label(i18n::trf!("いちばん熱いのは ({:.1}, {:.1}) の {:.1} ℃ (周囲 {:.0} ℃ + {:.1} K)", at.x, at.y, t, m.ambient_c, t - m.ambient_c));
                    ui.label(egui::RichText::new(i18n::trf!("空気へ逃げる熱のしやすさ h: 表 {:.1}・裏 {:.1} W/m²K (対流と放射)。升目 {:.1} mm、{:.0} ms", m.h_top, m.h_bottom, m.grid.d, self.thermal.took_ms)).small())
                        .on_hover_text(i18n::tr("h は温度差で変わる (温度差の 1/4 乗) ので、地図と h を交互に計算して合わせています。放射 (ソルダーレジストの放射率 0.9) は、対流と同じくらい効きます"));
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.thermal.overlay, i18n::tr("基板に色を重ねる")).on_hover_text(i18n::tr("サーマルカメラのような色で、基板の温度を重ねて描きます"));
                        let names: Vec<String> = m.layers.iter().map(|s| side_name(*s)).collect();
                        egui::ComboBox::from_id_salt("thermal_layer").selected_text(names.get(self.thermal.layer).cloned().unwrap_or_default()).show_ui(ui, |ui| {
                            for (k, n) in names.iter().enumerate() {
                                if ui.selectable_value(&mut self.thermal.layer, k, n).changed() {
                                    self.thermal.texture = None;
                                }
                            }
                        });
                    });
                    egui::Grid::new("thermal_parts").striped(true).num_columns(5).show(ui, |ui| {
                        for (h, tip) in [("部品", ""), ("電力", ""), ("部品の温度", "接合部 (部品の中) の温度。基板の銅と、部品の本体から空気への熱で決まります"), ("パッドの銅", "部品のパッドの下の銅の平均の温度"), ("自己発熱", "回路図の「自己発熱」の温度 (熱抵抗 θ の式から)。地図の値と比べます")] {
                            let r = ui.label(egui::RichText::new(i18n::tr(h)).small().strong());
                            if !tip.is_empty() {
                                r.on_hover_text(i18n::tr(tip));
                            }
                        }
                        ui.end_row();
                        for p in &m.parts {
                            ui.label(&p.refdes);
                            ui.label(if p.power >= 1.0 { format!("{:.2} W", p.power) } else { format!("{:.0} mW", p.power * 1e3) });
                            ui.label(format!("{:.1} ℃", p.junction));
                            ui.label(format!("{:.1} ℃", p.pad_temp));
                            ui.label(self.thermal.own_temps.iter().find(|(n, _)| *n == p.refdes).map_or("—".into(), |(_, t)| format!("{t:.1} ℃")));
                            ui.end_row();
                        }
                    });
                    // parts that warm each other: a "離す" requirement, one click away
                    if !self.thermal.pairs.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new(i18n::tr("熱い部品どうしが近い")).strong()).on_hover_text(i18n::tr("基板を通ってほかの部品から受ける熱が 3 K 以上で、その部品の上昇の 15 % 以上の組です。離すと、お互いに温め合う分が減ります"));
                        let mut add = None;
                        for (k, p) in self.thermal.pairs.iter().enumerate() {
                            let (hot, from, rise) = if p.a_from_b >= p.b_from_a { (&p.a, p.a_from_b, p.rise_a) } else { (&p.b, p.b_from_a, p.rise_b) };
                            let have = self.board.keep_apart.iter().any(|k| (k.a.contains(&p.a) && k.b.contains(&p.b)) || (k.a.contains(&p.b) && k.b.contains(&p.a)));
                            ui.horizontal_wrapped(|ui| {
                                ui.label(i18n::trf!("{} と {}: {:.1} mm。{} はもう一方から +{:.1} K (上昇の {:.0} %)", p.a, p.b, p.dist_mm, hot, from, 100.0 * from / rise.max(1e-9)));
                                if have {
                                    ui.label(egui::RichText::new(i18n::tr("離す要件あり")).small().weak());
                                } else if ui.small_button(i18n::trf!("{:.0} mm 離す要件に足す", p.suggest_mm)).on_hover_text(i18n::tr("回路図の「設計の要件」に「離す」組を足します (パッドの中心どうしの距離の下限)。自動配置はこの距離を守るように並べ、DRC の要件の点検が近すぎる組を知らせます。距離は、基板が熱を広げる長さからの目安で、もう一方から受ける熱が、知らせる目安の半分になる所です")).clicked() {
                                    add = Some(k);
                                }
                            });
                        }
                        if let Some(k) = add {
                            self.suggest_apart(k);
                        }
                    }
                    ui.label(egui::RichText::new(i18n::tr("目安です: 風・まわりの部品・ケースで ±20〜30 % ずれます。部品の本体から空気への熱は、SOT-223 のデータシートの曲線に合わせた割合で入れています")).small().weak());
                }
            }
        });
        if !open {
            self.thermal.open = false;
            self.thermal.overlay = false;
        }
    }
}

fn side_name(s: Side) -> String {
    match s {
        Side::Top => i18n::tr("表").to_string(),
        Side::Bottom => i18n::tr("裏").to_string(),
        Side::Inner(k) => i18n::trf!("内層 {k}"),
    }
}
