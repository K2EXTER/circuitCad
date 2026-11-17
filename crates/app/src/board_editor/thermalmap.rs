//! The "熱の地図" window (candidate H7): the board's temperature worked out on a grid (`pcb_core::thermal`) from the schematic's
//! self-heating powers, a thermal-camera colour over the board in the editor, and a table of the parts. The App hands the heat sources
//! over when the window asks for them (`take_thermal_request`, `run_thermal_map`).

use super::*;
use pcb_core::thermal::{HeatSource, Orientation, ThermalMap, ThermalSpec, ThermalTransient};

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
    /// The picture of the shown layer: (layer, frame of the run over time or `None` for the steady map, texture).
    texture: Option<(usize, Option<usize>, egui::TextureHandle)>,
    /// The run over time (switched on at t = 0), how long it runs (s), and the frame shown on the board (`None`: the steady map).
    pub transient: Option<Result<ThermalTransient, String>>,
    pub tran_end: f64,
    pub show_frame: Option<usize>,
    /// Counts the maps and runs worked out (the 3D view paints the board again when it changes).
    pub serial: u64,
    /// Drive the run over time with the circuit's power over time (the schematic's "温度の時間変化" run) instead of a constant.
    pub use_circuit: bool,
    /// The circuit's power over time repeats (a PWM, a load that comes and goes) instead of holding its last value.
    pub repeat: bool,
    /// The window asked for a run with the circuit's power (the App hands the sources with their profiles to `run_thermal_transient`).
    tran_request: bool,
    /// What drove the last run over time, for the window ("回路の過渡解析の電力: Q1, RS1").
    pub driven_by: String,
    /// The wiring's own heat for the next map (from the schematic's self-heating with the board carried over; empty otherwise).
    pub traces: Vec<pcb_core::thermal::TraceHeat>,
    /// The board in a closed case (H10): the case, and what it came to with the last map.
    pub in_case: bool,
    pub case: pcb_core::case_heat::CaseHeat,
    pub case_air: Option<pcb_core::case_heat::CaseAir>,
    request: bool,
    /// Milliseconds the last map took.
    pub took_ms: f64,
    /// Parts that warm each other noticeably (`pcb_core::thermal::hot_pairs`).
    pub pairs: Vec<pcb_core::thermal::HotPair>,
    /// A "離す" requirement to add to the schematic's design spec (the App takes it).
    apart_request: Option<design_spec::KeepApart>,
}

/// What the board is painted with (`BoardEditor::shown_heat`): the grid, each layer's temperatures (°C, `NaN` off the board), the air,
/// the colour scale, the parts' (name, °C, W), and the moment of the run over time (s) and its frame when it is one.
pub(crate) struct ShownHeat {
    pub grid: pcb_core::thermal::Grid,
    pub layers: Vec<Vec<f64>>,
    pub ambient: f64,
    pub scale: (f64, f64),
    pub parts: Vec<(String, f64, f64)>,
    pub moment: Option<f64>,
    pub frame: Option<usize>,
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
        let copper = self.board.stackup.copper_um;
        let mut pairs = vec![];
        self.thermal.result = Some(sources.and_then(|(src, own, ambient)| {
            self.thermal.spec.ambient_c = ambient;
            self.thermal.own_temps = own;
            self.thermal.sources = src.clone();
            if src.is_empty() && self.thermal.traces.is_empty() {
                return Err("電力の出ている部品がありません。回路図の「自己発熱」で電力の出る部品があるか確かめてください".into());
            }
            // the parts that warm each other: those with real power only (as the schematic's heat runs take from the board)
            let top = src.iter().map(|s| s.power).fold(0.0, f64::max);
            let mut hot: Vec<HeatSource> = src.iter().filter(|s| s.power >= 1e-3 && s.power >= 0.01 * top).cloned().collect();
            hot.sort_by(|a, b| b.power.total_cmp(&a.power));
            hot.truncate(sim_core::BOARD_HEATERS_MAX);
            let spec = self.thermal.spec;
            self.thermal.case_air = None;
            if self.thermal.in_case {
                // in the case: the map with what the board sees inside, the pairs on the same surroundings
                let (map, air) = pcb_core::case_heat::thermal_map_in_case(&self.board, &src, &self.thermal.traces, copper, &spec, &self.thermal.case)?;
                let inside = pcb_core::thermal::ThermalSpec { ambient_c: air.board_ambient_c, gap_top_mm: self.thermal.case.top_mm, gap_bottom_mm: self.thermal.case.bottom_mm, ..spec };
                if hot.len() >= 2 {
                    pairs = pcb_core::thermal::hot_pairs(&self.board, &hot, copper, &inside).unwrap_or_default();
                }
                self.thermal.case_air = Some(air);
                return Ok(map);
            }
            if hot.len() == src.len() && self.thermal.traces.is_empty() {
                // every source is one of them: the matrix's own map is the map (one solve for both)
                let (r, map) = pcb_core::thermal::thermal_matrix(&self.board, &hot, copper, &spec)?;
                pairs = pcb_core::thermal::hot_pairs_with(&self.board, &hot, &r, &map, copper, &spec).unwrap_or_default();
                Ok(map)
            } else {
                let map = pcb_core::thermal::thermal_map_with(&self.board, &src, &self.thermal.traces, copper, &spec)?;
                if hot.len() >= 2 {
                    pairs = pcb_core::thermal::hot_pairs(&self.board, &hot, copper, &spec).unwrap_or_default();
                }
                Ok(map)
            }
        }));
        self.thermal.pairs = pairs;
        self.thermal.took_ms = t0.elapsed().as_secs_f64() * 1e3;
        self.thermal.serial += 1;
        self.thermal.transient = None;
        self.thermal.show_frame = None;
        if let Some(Ok(_)) = &self.thermal.result {
            self.thermal.overlay = true;
        }
    }

    /// Whether the window asked for a run over time with the circuit's power (the App then builds the sources with their profiles).
    pub(crate) fn take_thermal_tran_request(&mut self) -> bool {
        std::mem::take(&mut self.thermal.tran_request)
    }

    /// The parts of the last map (the App adds the circuit's power over time to them).
    pub(crate) fn thermal_sources(&self) -> &[HeatSource] {
        &self.thermal.sources
    }

    /// The board warming up from the air's temperature for `tran_end` seconds: the sources of the last map switched on at t = 0, or
    /// `sources` (with the circuit's power over time) when given, or the error that kept them from being made.
    pub(crate) fn run_thermal_transient(&mut self, sources: Option<Result<Vec<HeatSource>, String>>) {
        let repeat = self.thermal.repeat;
        let sources = sources.map(|r| r.map(|v| v.into_iter().map(|s| HeatSource { repeat: repeat && !s.profile.is_empty(), ..s }).collect::<Vec<_>>()));
        let end = if self.thermal.tran_end > 0.0 { self.thermal.tran_end } else { 600.0 };
        self.thermal.texture = None;
        let driven = sources.is_some();
        let src = match sources {
            Some(s) => s,
            None if self.thermal.sources.is_empty() => Err("先に「熱の地図を計算」を押してください".into()),
            None => Ok(self.thermal.sources.clone()),
        };
        self.thermal.driven_by = match &src {
            Ok(s) if driven => i18n::trf!("回路の過渡解析の電力: {}", s.iter().filter(|x| !x.profile.is_empty()).map(|x| x.refdes.as_str()).collect::<Vec<_>>().join(", ")),
            _ => String::new(),
        };
        let case = if self.thermal.in_case { self.thermal.case_air.clone() } else { None };
        let spec = match &case {
            Some(air) => pcb_core::thermal::ThermalSpec { ambient_c: air.board_ambient_c, gap_top_mm: self.thermal.case.top_mm, gap_bottom_mm: self.thermal.case.bottom_mm, ..self.thermal.spec },
            None => self.thermal.spec,
        };
        let room = self.thermal.spec.ambient_c;
        self.thermal.transient = Some(src.and_then(|s| {
            let tr = pcb_core::thermal::thermal_transient(&self.board, &s, &self.thermal.traces, self.board.stackup.copper_um, &spec, end, 8)?;
            Ok(match &case {
                Some(air) => pcb_core::case_heat::transient_in_case(tr, air, room),
                None => tr,
            })
        }));
        if let Some(Ok(tr)) = &self.thermal.transient {
            self.thermal.show_frame = Some(tr.frames.len().saturating_sub(1));
        }
        self.thermal.serial += 1;
    }

    /// What the board is painted with: the moment of the run over time picked in the window, or else the steady map; one colour scale
    /// for both (the air to the hottest of the steady map and of the whole run, at least 10 K), so a colour means the same temperature
    /// at every moment. `None` with nothing worked out.
    pub(crate) fn shown_heat(&self) -> Option<ShownHeat> {
        let map = match &self.thermal.result {
            Some(Ok(m)) => Some(m),
            _ => None,
        };
        let tr = match &self.thermal.transient {
            Some(Ok(tr)) => Some(tr),
            _ => None,
        };
        let mut top = f64::MIN;
        let mut ambient = f64::NAN;
        if let Some(m) = map {
            ambient = m.ambient_c;
            top = m.temp.iter().flatten().filter(|t| t.is_finite()).fold(top, |a, b| a.max(*b));
            top = m.parts.iter().map(|p| p.junction).fold(top, f64::max);
        }
        if let Some(tr) = tr {
            ambient = tr.ambient_c;
            top = tr.frames.iter().flat_map(|f| f.1.iter().flatten()).filter(|t| t.is_finite()).fold(top, |a, b| a.max(*b as f64));
            top = tr.parts.iter().flat_map(|p| p.1.iter().copied()).fold(top, f64::max);
        }
        if !ambient.is_finite() {
            return None;
        }
        let scale = (ambient, top.max(ambient + 10.0));
        match (tr, self.thermal.show_frame) {
            (Some(tr), Some(k)) if k < tr.frames.len() => {
                let (t, layers) = &tr.frames[k];
                let at = tr.times.iter().position(|x| (x - t).abs() <= 1e-9 * t.max(1.0)).unwrap_or(tr.times.len() - 1);
                let power = |n: &str| map.and_then(|m| m.parts.iter().find(|p| p.refdes == n)).map_or(0.0, |p| p.power);
                Some(ShownHeat {
                    grid: tr.grid.clone(),
                    layers: layers.iter().map(|l| l.iter().map(|x| *x as f64).collect()).collect(),
                    ambient,
                    scale,
                    parts: tr.parts.iter().map(|(n, v)| (n.clone(), v[at], power(n))).collect(),
                    moment: Some(*t),
                    frame: Some(k),
                })
            }
            _ => map.map(|m| ShownHeat { grid: m.grid.clone(), layers: m.temp.clone(), ambient, scale, parts: m.parts.iter().map(|p| (p.refdes.clone(), p.junction, p.power)).collect(), moment: None, frame: None }),
        }
    }

    /// The colour of the shown layer over the board (after the copper, under the silk and the marks).
    pub(super) fn draw_thermal(&mut self, painter: &egui::Painter, rect: Rect) {
        if !self.thermal.overlay {
            return;
        }
        let Some(sh) = self.shown_heat() else { return };
        let layer = self.thermal.layer.min(sh.layers.len() - 1);
        let (lo, hi) = sh.scale;
        if self.thermal.texture.as_ref().is_none_or(|(l, f, _)| *l != layer || *f != sh.frame) {
            let g = &sh.grid;
            let mut img = egui::ColorImage::new([g.w, g.h], vec![Color32::TRANSPARENT; g.w * g.h]);
            for (c, t) in sh.layers[layer].iter().enumerate() {
                if t.is_finite() {
                    let [r, gg, b] = super::view3d::heat_color((t - lo) / (hi - lo));
                    // rows go up in y on the board, down in the picture
                    let (i, j) = (c % g.w, g.h - 1 - c / g.w);
                    img.pixels[j * g.w + i] = Color32::from_rgba_unmultiplied(r, gg, b, 170);
                }
            }
            self.thermal.texture = Some((layer, sh.frame, painter.ctx().load_texture("thermal_map", img, egui::TextureOptions::LINEAR)));
        }
        let Some((_, _, tex)) = &self.thermal.texture else { return };
        let g = &sh.grid;
        let a = self.to_screen(rect, Point::new(g.x0, g.y0 + g.h as f64 * g.d));
        let b = self.to_screen(rect, Point::new(g.x0 + g.w as f64 * g.d, g.y0));
        painter.image(tex.id(), Rect::from_two_pos(a, b), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        // the scale, at the canvas's bottom left
        let bar = Rect::from_min_size(Pos2::new(rect.left() + 56.0, rect.bottom() - 34.0), egui::vec2(160.0, 10.0));
        for k in 0..32 {
            let f = k as f64 / 31.0;
            let [r, gg, b] = super::view3d::heat_color(f);
            let x0 = bar.left() + bar.width() * k as f32 / 32.0;
            painter.rect_filled(Rect::from_min_size(Pos2::new(x0, bar.top()), egui::vec2(bar.width() / 32.0 + 0.5, bar.height())), 0.0, Color32::from_rgb(r, gg, b));
        }
        let font = crate::theme::canvas_font(&painter, 13.0);
        painter.text(Pos2::new(bar.left(), bar.top() - 2.0), egui::Align2::LEFT_BOTTOM, format!("{lo:.0} ℃"), font.clone(), Color32::WHITE);
        painter.text(Pos2::new(bar.right(), bar.top() - 2.0), egui::Align2::RIGHT_BOTTOM, format!("{hi:.0} ℃"), font.clone(), Color32::WHITE);
        let label = match sh.moment {
            Some(t) => i18n::trf!("熱の地図 (目安)・電源を入れて {}", fmt_time(t)),
            None => i18n::tr("熱の地図 (目安)").to_string(),
        };
        painter.text(Pos2::new(bar.right() + 8.0, bar.center().y), egui::Align2::LEFT_CENTER, label, font, Color32::WHITE);
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
            self.case_heat_ui(ui);
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
                    // a circuit that only pulses has no steady power: its run over time still works
                    ui.checkbox(&mut self.thermal.overlay, i18n::tr("基板に色を重ねる"));
                    self.thermal_transient_ui(ui);
                }
                Some(Ok(m)) => {
                    if let Some(air) = &self.thermal.case_air {
                        let room = self.thermal.spec.ambient_c;
                        ui.label(i18n::trf!("ケースの中: 空気 {:.1} ℃・壁 {:.1} ℃ (周囲 {:.0} ℃ より {:+.1} K)。基板は {:.1} ℃ の中にいるのと同じです", air.air_c, air.wall_c, room, air.air_c - room, air.board_ambient_c))
                            .on_hover_text(i18n::trf!(
                                "熱 {:.2} W がケースを通って外へ出ます。熱抵抗: 中の空気 → 壁 {:.1} K/W (内側の対流 h {:.1} W/m²K)、壁 {:.2} K/W、外側 → 部屋 {:.1} K/W (対流と放射 h {:.1} W/m²K、内側の面積 {:.0} cm²)。基板の熱の約半分は空気へ、残りは放射で壁へ直接。ケースが温まる時定数 約 {} (壁の熱容量 {:.0} J/K)",
                                air.power, air.r_air, air.h_in, air.r_wall, air.r_out, air.h_out, air.area * 1e4, fmt_time(air.tau), air.cth
                            ));
                    }
                    let (t, at) = m.hottest();
                    ui.label(i18n::trf!("いちばん熱いのは ({:.1}, {:.1}) の {:.1} ℃ (周囲 {:.0} ℃ + {:.1} K)", at.x, at.y, t, m.ambient_c, t - m.ambient_c));
                    if m.trace_power >= 1e-3 {
                        ui.label(i18n::trf!("配線そのものの発熱 {:.0} mW も入れています (電流² × 配線の抵抗)", m.trace_power * 1e3))
                            .on_hover_text(i18n::tr("基板を「シミュレーションに反映」していると、パッドの配線の抵抗に流れる電流の熱を、配線の道すじに沿って基板に入れます。大電流の細い配線は、自分で温まります"));
                    }
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
                    self.thermal_transient_ui(ui);
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

impl BoardEditor {
    /// "ケースに入れる": the case round the board (inside size, room above and below, wall, material).
    fn case_heat_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.thermal.in_case, i18n::tr("ケースに入れる")).on_hover_text(i18n::tr(
                "閉じたケースに入れたときの温度を計算します。基板の熱は、中の空気と、放射で壁へ伝わり、壁を通って外の空気へ逃げます。ケースの中はその分だけ周囲より暖かくなり、基板はその中で温まります。通気口・ファンは入っていません",
            ));
            if self.thermal.in_case && self.thermal.case.inner_w <= 0.0
                && let Some((lo, hi)) = self.board.bounds() {
                    // a first guess: the board with 5 mm round it
                    self.thermal.case.inner_w = ((hi.x - lo.x) + 10.0).round();
                    self.thermal.case.inner_d = ((hi.y - lo.y) + 10.0).round();
                }
        });
        if !self.thermal.in_case {
            return;
        }
        let c = &mut self.thermal.case;
        egui::Grid::new("case_heat_grid").num_columns(4).show(ui, |ui| {
            ui.label(i18n::tr("内寸"));
            ui.add(egui::DragValue::new(&mut c.inner_w).range(5.0..=1000.0).speed(0.5).suffix(" mm")).on_hover_text(i18n::tr("ケースの内側の幅"));
            ui.add(egui::DragValue::new(&mut c.inner_d).range(5.0..=1000.0).speed(0.5).suffix(" mm")).on_hover_text(i18n::tr("ケースの内側の奥行"));
            ui.label("");
            ui.end_row();
            ui.label(i18n::tr("上・下の空き"));
            ui.add(egui::DragValue::new(&mut c.top_mm).range(0.5..=300.0).speed(0.5).suffix(" mm")).on_hover_text(i18n::tr("基板の表の面からふたまで。狭いと、空気は動かず、熱は空気の層を伝わって (伝導で) ふたへ渡ります"));
            ui.add(egui::DragValue::new(&mut c.bottom_mm).range(0.5..=300.0).speed(0.5).suffix(" mm")).on_hover_text(i18n::tr("基板の裏の面から底まで (ねじの柱の高さなど)"));
            ui.label("");
            ui.end_row();
            ui.label(i18n::tr("壁"));
            ui.add(egui::DragValue::new(&mut c.wall_mm).range(0.3..=20.0).speed(0.1).suffix(" mm")).on_hover_text(i18n::tr("壁の厚さ。樹脂でも薄い壁はあまり熱を妨げません。効くのはケースの外側の広さです"));
            egui::ComboBox::from_id_salt("case_material")
                .selected_text(match c.material {
                    pcb_core::case_heat::CaseMaterial::Plastic => i18n::tr("樹脂 (ABS)"),
                    pcb_core::case_heat::CaseMaterial::Aluminium => i18n::tr("アルミ (アルマイト)"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut c.material, pcb_core::case_heat::CaseMaterial::Plastic, i18n::tr("樹脂 (ABS)"));
                    ui.selectable_value(&mut c.material, pcb_core::case_heat::CaseMaterial::Aluminium, i18n::tr("アルミ (アルマイト)"))
                        .on_hover_text(i18n::tr("黒や色のアルマイト・塗装 (放射率 0.8)。磨いたままのアルミは放射が 1 割しかなく、かえって熱がこもります"));
                });
            ui.label("");
            ui.end_row();
        });
    }

    /// "時間の変化": how long, the button, the moment shown on the board, and each part's temperature over time.
    fn thermal_transient_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(egui::RichText::new(i18n::tr("時間の変化 (電源を入れてから)")).strong()).on_hover_text(i18n::tr("部品の電力を時刻 0 で入れたときに、基板が温まっていく様子です。部品は秒で、基板は分で温まります。銅と FR-4 の熱容量 (重さ × 比熱) と、部品の熱容量 (回路図の「温度の時間変化」の熱容量) で計算します"));
        ui.horizontal(|ui| {
            if self.thermal.tran_end <= 0.0 {
                self.thermal.tran_end = 600.0;
            }
            ui.label(i18n::tr("時間"));
            ui.add(egui::DragValue::new(&mut self.thermal.tran_end).range(0.1..=36000.0).speed(10.0).suffix(" s")).on_hover_text(i18n::tr("何秒後まで計算するか。基板全体が落ち着くには、ふつう 10〜30 分かかります"));
            if ui.button(i18n::tr("時間の変化を計算")).on_hover_text(i18n::tr("いま計算した地図の部品の電力を、時刻 0 で入れて、温まっていく様子を計算します (時間は対数で刻みます)")).clicked() {
                if self.thermal.use_circuit {
                    self.thermal.tran_request = true;
                } else {
                    self.run_thermal_transient(None);
                }
            }
        });
        ui.checkbox(&mut self.thermal.use_circuit, i18n::tr("回路の過渡解析の電力を使う")).on_hover_text(i18n::tr("回路図の「温度の時間変化」で計算した、部品ごとの電力の時間の変化 (パルス・起動時の突入など) で温めます。その計算に入れていない部品は、自己発熱の一定の電力です。過渡解析の時間より先は、最後の電力が続くものとします"));
        if self.thermal.use_circuit {
            ui.checkbox(&mut self.thermal.repeat, i18n::tr("くり返す")).on_hover_text(i18n::tr("回路の過渡解析の電力を、その長さを周期としてくり返します (PWM や、つないだり外したりする負荷)。長い刻みでは、周期の平均の電力で温まります。外すと、過渡解析の最後の電力が続きます"));
        }
        if !self.thermal.driven_by.is_empty() {
            ui.label(egui::RichText::new(&self.thermal.driven_by).small());
        }
        let Some(tr) = &self.thermal.transient else { return };
        let tr = match tr {
            Ok(tr) => tr,
            Err(e) => {
                ui.colored_label(crate::theme::ERROR, e);
                return;
            }
        };
        ui.label(egui::RichText::new(i18n::trf!("基板の熱容量 {:.1} J/K (銅と FR-4)、温まる時定数 約 {} (熱容量 ÷ 空気へ逃げる熱のしやすさ)", tr.board_cth, fmt_time(tr.board_tau))).small());
        // the moment shown on the board
        let mut k = self.thermal.show_frame.unwrap_or(tr.frames.len().saturating_sub(1)).min(tr.frames.len().saturating_sub(1));
        let mut steady = self.thermal.show_frame.is_none();
        ui.horizontal(|ui| {
            ui.checkbox(&mut steady, i18n::tr("落ち着いた温度を表示")).on_hover_text(i18n::tr("基板の色を、時間の変化でなく、上の落ち着いた (定常の) 地図にします"));
            ui.add_enabled(!steady && tr.frames.len() > 1, egui::Slider::new(&mut k, 0..=tr.frames.len().saturating_sub(1)).show_value(false))
                .on_hover_text(i18n::tr("基板に重ねる時刻を選びます"));
            if let Some((t, _)) = tr.frames.get(k) {
                ui.label(fmt_time(*t));
            }
        });
        let pick = if steady { None } else { Some(k) };
        if pick != self.thermal.show_frame {
            self.thermal.show_frame = pick;
        }
        draw_temp_chart(ui, tr);
    }
}

/// A time for people: "0.5 秒", "40 秒", "3.2 分".
fn fmt_time(t: f64) -> String {
    if t < 1.0 {
        format!("{:.0} ms", t * 1e3)
    } else if t < 120.0 {
        i18n::trf!("{:.1} 秒", t)
    } else {
        i18n::trf!("{:.1} 分", t / 60.0)
    }
}

/// Each part's temperature over time, time on a log scale.
fn draw_temp_chart(ui: &mut egui::Ui, tr: &ThermalTransient) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(300.0), 150.0), Sense::hover());
    let plot = Rect::from_min_max(rect.min + egui::vec2(46.0, 6.0), rect.max - egui::vec2(8.0, 18.0));
    let painter = ui.painter_at(rect);
    let weak = ui.visuals().weak_text_color();
    painter.rect_stroke(plot, 0.0, egui::Stroke::new(1.0, weak), egui::StrokeKind::Inside);
    let top = tr.parts.iter().flat_map(|p| p.1.iter().copied()).fold(tr.ambient_c + 1.0, f64::max);
    let t0 = tr.times.get(1).copied().unwrap_or(1e-3).max(1e-6);
    let t1 = tr.times.last().copied().unwrap_or(1.0).max(t0 * 10.0);
    let x = |t: f64| plot.left() + plot.width() * (((t.max(t0)).ln() - t0.ln()) / (t1.ln() - t0.ln())) as f32;
    let y = |c: f64| plot.bottom() - plot.height() * ((c - tr.ambient_c) / (top - tr.ambient_c)) as f32;
    let font = egui::FontId::proportional(11.0);
    painter.text(Pos2::new(plot.left() - 3.0, plot.top()), egui::Align2::RIGHT_TOP, format!("{top:.0} ℃"), font.clone(), weak);
    painter.text(Pos2::new(plot.left() - 3.0, plot.bottom()), egui::Align2::RIGHT_BOTTOM, format!("{:.0} ℃", tr.ambient_c), font.clone(), weak);
    painter.text(Pos2::new(plot.left(), plot.bottom() + 2.0), egui::Align2::LEFT_TOP, fmt_time(t0), font.clone(), weak);
    painter.text(Pos2::new(plot.right(), plot.bottom() + 2.0), egui::Align2::RIGHT_TOP, fmt_time(t1), font.clone(), weak);
    for (k, (name, temps)) in tr.parts.iter().enumerate() {
        let col = crate::theme::series_color(k);
        let pts: Vec<Pos2> = tr.times.iter().zip(temps).skip(1).map(|(t, c)| Pos2::new(x(*t), y(*c))).collect();
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, col)));
        if let Some(c) = temps.last() {
            // kept inside the chart: the hottest line ends at its top edge
            painter.text(Pos2::new(plot.right() - 2.0, (y(*c) - 1.0).max(plot.top() + 13.0)), egui::Align2::RIGHT_BOTTOM, format!("{name} {c:.0} ℃"), font.clone(), col);
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
