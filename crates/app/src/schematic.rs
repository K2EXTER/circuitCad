//! Schematic editor: place parts, wire them together, run ERC, export netlist.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use crate::part_form::PartForm;
use part_lib::PartDef;
use crate::plot::{self, PlotState, Style, Trace, PALETTE};
use sim_core::{
    ac_analysis, dc_op, from_schematic, parse_value, spectrum as fft_spectrum, stats as signal_stats, transient, AcResult, AcSettings, AcSweep, DcResult, Spectrum,
    TranResult, TranSettings,
};
use std::collections::BTreeSet;
use sch_core::{erc, extract, on_segment, to_spice, Component, CustomSymbol, Kind, Label, PinSide, Problem, Pt, Schematic, Wire};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Place,
    Wire,
    Label,
}

#[derive(Clone, Copy, PartialEq)]
enum Sel {
    Comp(usize),
    Wire(usize),
    Label(usize),
}

pub struct SchEditor {
    pub sch: Schematic,
    path: Option<PathBuf>,
    tool: Tool,
    place: Kind,
    rot: u8,
    label_text: String,
    pending: Option<Pt>,
    selected: Option<Sel>,
    drag: Option<(usize, Pt)>,
    zoom: f32, // pixels per grid step
    pan: Vec2, // grid position at the screen centre
    problems: Vec<Problem>,
    pub status: String,
    pub library: Vec<PartDef>,
    lib_errors: Vec<String>,
    lib_dir: PathBuf,
    lib_filter: String,
    place_lib: Option<usize>,
    form: Option<PartForm>,
    dc: Option<DcView>,
    show_v: bool,
    sig_cache: String,
    tran: Option<TranView>,
    tran_tstop: String,
    tran_tstep: String,
    tran_uic: bool,
    ac: Option<AcView>,
    ac_fstart: String,
    ac_fstop: String,
    ac_ppd: String,
    wave_mode: WaveMode,
    fit_pending: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum WaveMode {
    Tran,
    Ac,
}

/// A finished AC sweep (Bode plot).
struct AcView {
    outcome: Result<AcResult, Vec<String>>,
    selected: BTreeSet<String>,
    plot: PlotState,
}

/// A finished transient run with the waveform viewer state.
struct TranView {
    outcome: Result<TranResult, Vec<String>>,
    selected: BTreeSet<String>,
    plot: PlotState,
    /// Show the amplitude spectrum instead of the waveform.
    spectrum: bool,
    spec_t0: String,
    spec_t1: String,
    spec_plot: Option<PlotState>,
    spec_cache: Option<(String, Vec<(String, Spectrum)>)>,
}

/// A finished DC analysis, remembered with the schematic it was computed from.
struct DcView {
    outcome: Result<DcResult, Vec<String>>,
    signature: String,
    /// Voltage at every pin position.
    pin_v: Vec<(Pt, f64)>,
}

/// Engineering notation: 4.700 mA, 12.00 V, 250.0 nA.
fn eng(v: f64, unit: &str) -> String {
    let a = v.abs();
    let (scale, prefix) = if a == 0.0 {
        (1.0, "")
    } else if a >= 1e9 {
        (1e-9, "G")
    } else if a >= 1e6 {
        (1e-6, "M")
    } else if a >= 1e3 {
        (1e-3, "k")
    } else if a >= 1.0 {
        (1.0, "")
    } else if a >= 1e-3 {
        (1e3, "m")
    } else if a >= 1e-6 {
        (1e6, "µ")
    } else if a >= 1e-9 {
        (1e9, "n")
    } else {
        (1e12, "p")
    };
    format!("{:.3} {prefix}{unit}", v * scale)
}

impl Default for SchEditor {
    fn default() -> Self {
        let lib_dir = part_lib::default_dir();
        let _ = part_lib::install_examples(&lib_dir);
        let (library, lib_errors) = part_lib::load_dir(&lib_dir);
        Self {
            library,
            lib_errors,
            lib_dir,
            lib_filter: String::new(),
            place_lib: None,
            form: None,
            dc: None,
            show_v: true,
            sig_cache: String::new(),
            tran: None,
            tran_tstop: "10m".into(),
            tran_tstep: "10u".into(),
            tran_uic: false,
            ac: None,
            ac_fstart: "10".into(),
            ac_fstop: "1Meg".into(),
            ac_ppd: "20".into(),
            wave_mode: WaveMode::Tran,
            fit_pending: false,
            sch: Schematic::default(),
            path: None,
            tool: Tool::Place,
            place: Kind::Resistor,
            rot: 0,
            label_text: "SIG".into(),
            pending: None,
            selected: None,
            drag: None,
            zoom: 22.0,
            pan: Vec2::new(10.0, 6.0),
            problems: vec![],
            status: "回路図: 部品を選んで配置し、配線ツールで線をつなぎます".into(),
        }
    }
}

// ------------------------------------------------------------- symbol drawing

enum Sh {
    L(f32, f32, f32, f32),
    C(f32, f32, f32),
}

fn symbol(kind: Kind) -> Vec<Sh> {
    use Sh::*;
    match kind {
        Kind::Resistor => vec![
            L(-2., 0., -1., 0.),
            L(1., 0., 2., 0.),
            L(-1., -0.4, 1., -0.4),
            L(-1., 0.4, 1., 0.4),
            L(-1., -0.4, -1., 0.4),
            L(1., -0.4, 1., 0.4),
        ],
        Kind::Capacitor => vec![
            L(-2., 0., -0.25, 0.),
            L(0.25, 0., 2., 0.),
            L(-0.25, -0.8, -0.25, 0.8),
            L(0.25, -0.8, 0.25, 0.8),
        ],
        Kind::Inductor => {
            let mut v = vec![L(-2., 0., -1.2, 0.), L(1.2, 0., 2., 0.)];
            let n = 48;
            let pt = |i: i32| {
                let x = -1.2 + 2.4 * i as f32 / n as f32;
                (x, -0.5 * (std::f32::consts::PI * (x + 1.2) / 0.6).sin().abs())
            };
            for i in 0..n {
                let (a, b) = (pt(i), pt(i + 1));
                v.push(L(a.0, a.1, b.0, b.1));
            }
            v
        }
        Kind::Diode | Kind::Led => {
            let mut v = vec![
                L(-2., 0., -0.6, 0.),
                L(0.6, 0., 2., 0.),
                L(-0.6, -0.6, -0.6, 0.6),
                L(-0.6, -0.6, 0.6, 0.),
                L(-0.6, 0.6, 0.6, 0.),
                L(0.6, -0.6, 0.6, 0.6),
            ];
            if kind == Kind::Led {
                v.extend([L(0., -0.9, 0.6, -1.5), L(0.6, -0.9, 1.2, -1.5)]);
            }
            v
        }
        Kind::VSource => vec![
            L(0., -2., 0., -1.2),
            L(0., 1.2, 0., 2.),
            C(0., 0., 1.2),
            L(-0.25, -0.6, 0.25, -0.6),
            L(0., -0.85, 0., -0.35),
            L(-0.25, 0.6, 0.25, 0.6),
        ],
        Kind::Npn => vec![
            C(0.5, 0., 1.7),
            L(-2., 0., -0.5, 0.),
            L(-0.5, -1., -0.5, 1.),
            L(-0.5, -0.4, 2., -2.),
            L(-0.5, 0.4, 2., 2.),
            L(1.2, 1.5, 2., 2.),
            L(1.5, 1.1, 2., 2.),
        ],
        Kind::Header(n) => {
            let mut v = vec![
                L(0., -1., 1.2, -1.),
                L(0., -1., 0., 2. * n as f32 - 1.),
                L(1.2, -1., 1.2, 2. * n as f32 - 1.),
                L(0., 2. * n as f32 - 1., 1.2, 2. * n as f32 - 1.),
            ];
            for i in 0..n as i32 {
                v.push(L(1.2, 2. * i as f32, 2., 2. * i as f32));
                v.push(C(0.6, 2. * i as f32, 0.25));
            }
            v
        }
        Kind::Ground => vec![
            L(0., 0., 0., 0.5),
            L(-0.8, 0.5, 0.8, 0.5),
            L(-0.5, 0.8, 0.5, 0.8),
            L(-0.2, 1.1, 0.2, 1.1),
        ],
        Kind::Power => vec![L(0., 0., 0., -0.6), L(-0.6, -0.6, 0.6, -0.6)],
        Kind::Custom => vec![],
    }
}

/// Local bounding box (xmin, xmax, ymin, ymax).
fn extents(kind: Kind) -> (f32, f32, f32, f32) {
    match kind {
        Kind::VSource => (-1.4, 1.4, -2., 2.),
        Kind::Npn => (-2., 2., -2., 2.),
        Kind::Header(n) => (0., 2., -1., 2. * n as f32 - 1.),
        Kind::Custom => (-2., 2., -2., 2.),
        Kind::Ground => (-1., 1., 0., 1.2),
        Kind::Power => (-1., 1., -1., 0.),
        _ => (-2., 2., -1., 1.),
    }
}

fn comp_extents(c: &Component) -> (f32, f32, f32, f32) {
    match &c.custom {
        Some(sym) => {
            let l = sym.layout();
            let (w, h) = ((l.hw + 2) as f32, (l.hh + 2) as f32);
            (-w, w, -h, h)
        }
        None => extents(c.kind),
    }
}

fn rot_f(p: (f32, f32), rot: u8) -> (f32, f32) {
    let mut q = p;
    for _ in 0..(rot % 4) {
        q = (-q.1, q.0);
    }
    q
}

const SCH_BG: Color32 = Color32::from_rgb(250, 248, 235);
const SYM: Color32 = Color32::from_rgb(120, 20, 20);
const WIRE: Color32 = Color32::from_rgb(0, 110, 30);

impl SchEditor {
    fn to_screen(&self, rect: Rect, p: (f32, f32)) -> Pos2 {
        let c = rect.center();
        Pos2::new(c.x + (p.0 - self.pan.x) * self.zoom, c.y + (p.1 - self.pan.y) * self.zoom)
    }

    fn to_grid(&self, rect: Rect, s: Pos2) -> (f32, f32) {
        let c = rect.center();
        ((s.x - c.x) / self.zoom + self.pan.x, (s.y - c.y) / self.zoom + self.pan.y)
    }

    fn draw_symbol(&self, painter: &egui::Painter, rect: Rect, kind: Kind, pos: Pt, rot: u8, color: Color32) {
        let tr = |x: f32, y: f32| {
            let r = rot_f((x, y), rot);
            self.to_screen(rect, (pos.0 as f32 + r.0, pos.1 as f32 + r.1))
        };
        let stroke = Stroke::new(1.6, color);
        for s in symbol(kind) {
            match s {
                Sh::L(a, b, c, d) => {
                    painter.line_segment([tr(a, b), tr(c, d)], stroke);
                }
                Sh::C(x, y, r) => {
                    painter.circle_stroke(tr(x, y), r * self.zoom, stroke);
                }
            }
        }
    }

    fn draw_component(&self, painter: &egui::Painter, rect: Rect, c: &Component, color: Color32) {
        match &c.custom {
            Some(sym) => self.draw_custom(painter, rect, sym, c.pos, c.rot, color),
            None => self.draw_symbol(painter, rect, c.kind, c.pos, c.rot, color),
        }
    }

    fn draw_custom(&self, painter: &egui::Painter, rect: Rect, sym: &CustomSymbol, pos: Pt, rot: u8, color: Color32) {
        let l = sym.layout();
        let tr = |x: f32, y: f32| {
            let r = rot_f((x, y), rot);
            self.to_screen(rect, (pos.0 as f32 + r.0, pos.1 as f32 + r.1))
        };
        let stroke = Stroke::new(1.6, color);
        let (hw, hh) = (l.hw as f32, l.hh as f32);
        let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            painter.line_segment([tr(a.0, a.1), tr(b.0, b.1)], stroke);
        }
        let name_font = FontId::proportional((self.zoom * 0.42).clamp(7.0, 14.0));
        let num_font = FontId::proportional((self.zoom * 0.34).clamp(6.0, 11.0));
        for p in &l.pins {
            let (tx, ty, ex, ey) = (p.tip.0 as f32, p.tip.1 as f32, p.edge.0 as f32, p.edge.1 as f32);
            painter.line_segment([tr(tx, ty), tr(ex, ey)], stroke);
            // name inside the body, anchored so it stays upright after rotation
            let inward = match p.side {
                PinSide::Left => (1.0, 0.0),
                PinSide::Right => (-1.0, 0.0),
                PinSide::Top => (0.0, 1.0),
                PinSide::Bottom => (0.0, -1.0),
            };
            let r = rot_f(inward, rot);
            let anchor = if r.0 > 0.5 {
                Align2::LEFT_CENTER
            } else if r.0 < -0.5 {
                Align2::RIGHT_CENTER
            } else if r.1 > 0.5 {
                Align2::CENTER_TOP
            } else {
                Align2::CENTER_BOTTOM
            };
            painter.text(tr(ex + inward.0 * 0.3, ey + inward.1 * 0.3), anchor, &p.name, name_font.clone(), color);
            // pin number beside the lead
            let (mx, my) = ((tx + ex) / 2.0, (ty + ey) / 2.0);
            let off = if matches!(p.side, PinSide::Left | PinSide::Right) { (0.0, -0.45) } else { (0.45, 0.0) };
            painter.text(tr(mx + off.0, my + off.1), Align2::CENTER_CENTER, &p.number, num_font.clone(), Color32::from_gray(110));
        }
    }

    /// Zoom and pan so that the whole drawing is visible.
    fn fit_view(&mut self, rect: Rect) {
        let mut pts: Vec<Pt> = vec![];
        for c in &self.sch.components {
            let (x0, x1, y0, y1) = comp_extents(c);
            for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
                let r = rot_f((x, y), c.rot);
                pts.push((c.pos.0 + r.0.round() as i32, c.pos.1 + r.1.round() as i32));
            }
        }
        for w in &self.sch.wires {
            pts.push(w.a);
            pts.push(w.b);
        }
        for l in &self.sch.labels {
            pts.push(l.pos);
        }
        if pts.is_empty() {
            return;
        }
        let (x0, x1) = (pts.iter().map(|p| p.0).min().unwrap() as f32, pts.iter().map(|p| p.0).max().unwrap() as f32);
        let (y0, y1) = (pts.iter().map(|p| p.1).min().unwrap() as f32, pts.iter().map(|p| p.1).max().unwrap() as f32);
        let (w, h) = ((x1 - x0).max(4.0) + 6.0, (y1 - y0).max(4.0) + 6.0);
        self.zoom = (rect.width() / w).min(rect.height() / h).clamp(6.0, 40.0);
        self.pan = Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    }

    pub fn has_wave(&self) -> bool {
        match self.wave_mode {
            WaveMode::Tran => self.tran.is_some(),
            WaveMode::Ac => self.ac.is_some(),
        }
    }

    pub fn analyze_ac(&mut self) {
        self.run_ac();
    }

    pub fn set_ac_settings(&mut self, f0: &str, f1: &str, ppd: &str) {
        self.ac_fstart = f0.into();
        self.ac_fstop = f1.into();
        self.ac_ppd = ppd.into();
    }

    fn run_ac(&mut self) {
        let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
            self.status = "開始・終了周波数と点数/デケードを数値で入力してください (例: 10, 1Meg, 20)".into();
            return;
        };
        let outcome = match from_schematic(&self.sch) {
            Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
            Ok(c) => ac_analysis(&c, &AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) }).map_err(|e| vec![e.to_string()]),
        };
        let mut selected = BTreeSet::new();
        if let Ok(r) = &outcome {
            for sg in r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).take(3) {
                selected.insert(sg.name.clone());
            }
        }
        self.status = match &outcome {
            Ok(r) => format!("AC解析: 完了 ({} 点)", r.freq.len()),
            Err(e) => format!("AC解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        self.ac = Some(AcView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi) });
        self.wave_mode = WaveMode::Ac;
    }

    /// Bottom panel: the waveform viewer for the most recent analysis.
    pub fn wave_panel(&mut self, ui: &mut egui::Ui) {
        match self.wave_mode {
            WaveMode::Tran => self.tran_wave_panel(ui),
            WaveMode::Ac => self.ac_wave_panel(ui),
        }
    }

    fn ac_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.ac.as_mut() else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong("AC解析 (ボード線図)");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.small("ゲインは 0dB = 1 V(または 1 A)。入力の電源を「AC 1」にすると、そのまま伝達関数になります");
        });
        let AcView { outcome, selected, plot: st } = view;
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(res) => {
                for w in &res.warnings {
                    ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                }
                ui.horizontal_top(|ui| {
                    let order: Vec<&str> = res.signals.iter().filter(|s| !s.name.contains('.')).map(|s| s.name.as_str()).collect();
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        if ui.button("選択をクリア").clicked() {
                            selected.clear();
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for name in &order {
                                let mut on = selected.contains(*name);
                                let color = order.iter().filter(|n| selected.contains(**n)).position(|n| n == name).map(|i| PALETTE[i % PALETTE.len()]);
                                let text = match color {
                                    Some(c) if on => egui::RichText::new(*name).color(c),
                                    _ => egui::RichText::new(*name),
                                };
                                if ui.checkbox(&mut on, text).changed() {
                                    if on {
                                        selected.insert(name.to_string());
                                    } else {
                                        selected.remove(*name);
                                    }
                                }
                            }
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    let chosen: Vec<(&str, Vec<f64>, Vec<f64>)> = order
                        .iter()
                        .filter(|n| selected.contains(**n))
                        .filter_map(|n| res.signal(n).map(|sg| (*n, sg.db(), sg.phase_deg())))
                        .collect();
                    if chosen.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から信号を選んでください", FontId::proportional(14.0), Color32::from_gray(150));
                        return;
                    }
                    let mag: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, db, _))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v: db }).collect();
                    let phase: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, _, ph))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v: ph }).collect();
                    let h = rect.height() / 2.0;
                    let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), h - 2.0));
                    let bottom = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h), Vec2::new(rect.width(), h - 2.0));
                    let ms = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true };
                    let ps = Style { unit: "°", x_unit: "Hz", x_name: "f", plain_y: true };
                    plot::draw_group(ui, top, egui::Id::new("bode_mag"), &mag, st, 0, &ms, false);
                    plot::draw_group(ui, bottom, egui::Id::new("bode_phase"), &phase, st, 1, &ps, true);
                });
            }
        }
        if close {
            self.ac = None;
        }
    }


    /// Switch the waveform viewer to the amplitude spectrum (command line `--spectrum`).
    pub fn show_spectrum(&mut self) {
        if let Some(v) = self.tran.as_mut() {
            v.spectrum = true;
        }
    }

    pub fn analyze_tran(&mut self) {
        self.run_tran();
    }

    pub fn set_tran_settings(&mut self, tstop: &str, tstep: &str) {
        self.tran_tstop = tstop.into();
        self.tran_tstep = tstep.into();
    }

    fn run_tran(&mut self) {
        let (Some(tstop), Some(tstep)) = (parse_value(&self.tran_tstop), parse_value(&self.tran_tstep)) else {
            self.status = "終了時間と時間刻みを数値で入力してください (例: 10m, 10u)".into();
            return;
        };
        let outcome = match from_schematic(&self.sch) {
            Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
            Ok(c) => transient(&c, &TranSettings { tstop, tstep, uic: self.tran_uic }).map_err(|e| vec![e.to_string()]),
        };
        let mut selected = BTreeSet::new();
        if let Ok(r) = &outcome {
            for sg in r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).take(4) {
                selected.insert(sg.name.clone());
            }
        }
        self.status = match &outcome {
            Ok(r) => format!("過渡解析: 完了 ({} 点, 再試行 {} 回)", r.time.len(), r.retries),
            Err(e) => format!("過渡解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.tran = Some(TranView {
            outcome,
            selected,
            plot: PlotState::new(tstop),
            spectrum: false,
            spec_t0: format!("{:.6e}", tstop * 0.1),
            spec_t1: format!("{:.6e}", tstop),
            spec_plot: None,
            spec_cache: None,
        });
        self.wave_mode = WaveMode::Tran;
    }

    /// Transient waveforms.
    fn tran_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.tran.as_mut() else { return };
        let mut close = false;
        let TranView { outcome, selected, plot: st, spectrum, spec_t0, spec_t1, spec_plot, spec_cache } = view;
        ui.horizontal(|ui| {
            ui.strong("過渡解析の波形");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.checkbox(spectrum, "スペクトル表示");
            if *spectrum {
                ui.label("解析範囲");
                ui.add(egui::TextEdit::singleline(spec_t0).desired_width(70.0));
                ui.label("〜");
                ui.add(egui::TextEdit::singleline(spec_t1).desired_width(70.0));
                ui.small("秒 / 0 dB = 1 V(ピーク値)。立ち上がり直後を除いた範囲を指定してください");
            } else {
                ui.small("ドラッグ: 移動 / ホイール: 時間軸ズーム (Ctrl: 縦軸) / ダブルクリック: 全体表示 / 統計値は表示中の範囲");
            }
        });
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(res) => {
                for w in &res.warnings {
                    ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                }
                ui.horizontal_top(|ui| {
                    let order: Vec<&str> = res.signals.iter().filter(|s| !s.name.contains('.')).map(|s| s.name.as_str()).collect();
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        if ui.button("選択をクリア").clicked() {
                            selected.clear();
                        }
                        // mean and AC RMS (= noise / ripple amplitude) over the visible window
                        let (w0, w1) = if *spectrum {
                            (parse_value(spec_t0).unwrap_or(0.0), parse_value(spec_t1).unwrap_or(f64::MAX))
                        } else {
                            st.x
                        };
                        for name in order.iter().filter(|n| selected.contains(**n)).take(6) {
                            if let Some(sg) = res.signal(name) {
                                if let Some(sx) = signal_stats(&res.time, &sg.values, w0, w1) {
                                    let u = sg.unit;
                                    ui.small(format!("{name}\n  平均 {}{u}  AC実効値 {}{u}", plot::axis_label(sx.mean, sx.mean.abs() / 1000.0), plot::axis_label(sx.rms_ac, sx.rms_ac.abs() / 1000.0)));
                                }
                            }
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for name in &order {
                                let mut on = selected.contains(*name);
                                let color = order
                                    .iter()
                                    .filter(|n| selected.contains(**n))
                                    .position(|n| n == name)
                                    .map(|i| PALETTE[i % PALETTE.len()]);
                                let text = match color {
                                    Some(c) if on => egui::RichText::new(*name).color(c),
                                    _ => egui::RichText::new(*name),
                                };
                                if ui.checkbox(&mut on, text).changed() {
                                    if on {
                                        selected.insert(name.to_string());
                                    } else {
                                        selected.remove(*name);
                                    }
                                }
                            }
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    if *spectrum {
                        let (Some(t0), Some(t1)) = (parse_value(spec_t0), parse_value(spec_t1)) else {
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "解析範囲を数値で入力してください", FontId::proportional(14.0), Color32::from_rgb(230, 60, 60));
                            return;
                        };
                        let key = format!("{t0:e}|{t1:e}|{selected:?}");
                        if spec_cache.as_ref().map(|c| &c.0) != Some(&key) {
                            let list: Vec<(String, Spectrum)> = order
                                .iter()
                                .filter(|n| selected.contains(**n))
                                .filter_map(|n| res.signal(n).and_then(|sg| fft_spectrum(&res.time, &sg.values, t0, t1)).map(|sp| (n.to_string(), sp)))
                                .collect();
                            *spec_plot = list.first().map(|(_, sp)| PlotState::log(sp.freq[0], *sp.freq.last().unwrap()));
                            *spec_cache = Some((key, list));
                        }
                        let list = &spec_cache.as_ref().unwrap().1;
                        let (Some(sp_state), false) = (spec_plot.as_mut(), list.is_empty()) else {
                            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から信号を選んでください (範囲が不正な場合も表示されません)", FontId::proportional(14.0), Color32::from_gray(150));
                            return;
                        };
                        let dbs: Vec<Vec<f64>> = list.iter().map(|(_, sp)| sp.amp.iter().map(|a| 20.0 * a.max(1e-12).log10()).collect()).collect();
                        let traces: Vec<Trace> = list
                            .iter()
                            .zip(&dbs)
                            .enumerate()
                            .map(|(k, ((n, sp), db))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &sp.freq, v: db })
                            .collect();
                        let style = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true };
                        plot::draw_group(ui, rect, egui::Id::new("spectrum"), &traces, sp_state, 0, &style, true);
                        return;
                    }
                    // traces per unit group, colours assigned in list order
                    let mut groups: [Vec<Trace>; 2] = [vec![], vec![]];
                    let mut k = 0;
                    for name in &order {
                        if !selected.contains(*name) {
                            continue;
                        }
                        if let Some(sg) = res.signal(name) {
                            let g = if sg.unit == "V" { 0 } else { 1 };
                            groups[g].push(Trace { name: &sg.name, color: PALETTE[k % PALETTE.len()], t: &res.time, v: &sg.values });
                            k += 1;
                        }
                    }
                    let used: Vec<usize> = (0..2).filter(|g| !groups[*g].is_empty()).collect();
                    if used.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から波形を選んでください", FontId::proportional(14.0), Color32::from_gray(150));
                    }
                    let h = rect.height() / used.len().max(1) as f32;
                    for (n, g) in used.iter().enumerate() {
                        let sub = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h * n as f32), Vec2::new(rect.width(), h - 2.0));
                        let style = Style { unit: if *g == 0 { "V" } else { "A" }, x_unit: "s", x_name: "t", plain_y: false };
                        plot::draw_group(ui, sub, egui::Id::new(("wave", *g)), &groups[*g], st, *g, &style, n + 1 == used.len());
                    }
                });
            }
        }
        if close {
            self.tran = None;
        }
    }

    fn signature_cached(&self) -> &str {
        &self.sig_cache
    }

    fn signature(&self) -> String {
        serde_json::to_string(&self.sch).unwrap_or_default()
    }

    fn run_dc(&mut self) {
        let outcome = match from_schematic(&self.sch) {
            Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
            Ok(c) => dc_op(&c).map_err(|e| vec![e.to_string()]),
        };
        let mut pin_v = vec![];
        if let Ok(res) = &outcome {
            for net in extract(&self.sch) {
                let Some(v) = res.voltage(&net.name) else { continue };
                for p in &net.pins {
                    if let Some(c) = self.sch.components.iter().find(|c| c.refdes == p.refdes) {
                        if let Some((_, pt)) = c.pins().into_iter().find(|(n, _)| *n == p.pin) {
                            pin_v.push((pt, v));
                        }
                    }
                }
            }
            // ground symbols and named power symbols show their net voltage too
            for c in self.sch.components.iter().filter(|c| c.kind.is_power_symbol()) {
                let name = if c.kind == Kind::Ground { "GND".to_string() } else { c.value.clone() };
                if let Some(v) = res.voltage(&name) {
                    pin_v.push((c.pins()[0].1, v));
                }
            }
        }
        self.status = match &outcome {
            Ok(r) => format!("DC動作点: 収束 ({} 回, {})", r.iterations, r.strategy),
            Err(e) => format!("DC解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.dc = Some(DcView { outcome, signature: self.signature(), pin_v });
    }

    fn reload_library(&mut self) {
        let (lib, errs) = part_lib::load_dir(&self.lib_dir);
        self.library = lib;
        self.lib_errors = errs;
        self.place_lib = None;
    }

    fn import_part_json(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("部品定義 JSON", &["json"]).pick_file() else { return };
        let def: Result<PartDef, String> =
            std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()));
        self.status = match def {
            Err(e) => format!("読み込み失敗: {e}"),
            Ok(d) => match d.validate().into_iter().find(|i| i.error) {
                Some(i) => format!("{} に問題があります: {}", d.name, i.message),
                None => match part_lib::save(&self.lib_dir, &d) {
                    Ok(_) => {
                        self.reload_library();
                        format!("ライブラリに追加しました: {}", d.name)
                    }
                    Err(e) => format!("保存失敗: {e}"),
                },
            },
        };
    }

    fn comp_at(&self, g: (f32, f32)) -> Option<usize> {
        self.sch.components.iter().enumerate().rev().find_map(|(i, c)| {
            let rel = (g.0 - c.pos.0 as f32, g.1 - c.pos.1 as f32);
            let l = rot_f(rel, (4 - c.rot % 4) % 4);
            let (x0, x1, y0, y1) = comp_extents(c);
            (l.0 >= x0 && l.0 <= x1 && l.1 >= y0 && l.1 <= y1).then_some(i)
        })
    }

    fn pick(&self, g: (f32, f32)) -> Option<Sel> {
        if let Some(i) = self.comp_at(g) {
            return Some(Sel::Comp(i));
        }
        if let Some(i) = self.sch.labels.iter().position(|l| (l.pos.0 as f32 - g.0).abs() < 0.8 && (l.pos.1 as f32 - g.1).abs() < 0.6) {
            return Some(Sel::Label(i));
        }
        let d = |w: &Wire| {
            let (a, b) = ((w.a.0 as f32, w.a.1 as f32), (w.b.0 as f32, w.b.1 as f32));
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 == 0.0 { 0.0 } else { (((g.0 - a.0) * dx + (g.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
            ((g.0 - a.0 - t * dx).powi(2) + (g.1 - a.1 - t * dy).powi(2)).sqrt()
        };
        self.sch.wires.iter().position(|w| d(w) < 0.3).map(Sel::Wire)
    }

    fn delete_selected(&mut self) {
        match self.selected.take() {
            Some(Sel::Comp(i)) => drop(self.sch.components.remove(i)),
            Some(Sel::Wire(i)) => drop(self.sch.wires.remove(i)),
            Some(Sel::Label(i)) => drop(self.sch.labels.remove(i)),
            None => {}
        }
    }

    fn add_wire(&mut self, a: Pt, b: Pt) {
        if a == b {
            return;
        }
        if a.0 == b.0 || a.1 == b.1 {
            self.sch.wires.push(Wire { a, b });
        } else {
            let m = (b.0, a.1);
            self.sch.wires.push(Wire { a, b: m });
            self.sch.wires.push(Wire { a: m, b });
        }
    }

    // ------------------------------------------------------------- canvas

    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, SCH_BG);
        if self.dc.is_some() {
            self.sig_cache = self.signature();
        }
        if self.fit_pending {
            self.fit_pending = false;
            self.fit_view(rect);
        }

        if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
            let d = resp.drag_delta();
            self.pan.x -= d.x / self.zoom;
            self.pan.y -= d.y / self.zoom;
        }
        let hover = resp.hover_pos();
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let cur = hover.unwrap_or(rect.center());
                let before = self.to_grid(rect, cur);
                self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(6.0, 120.0);
                let after = self.to_grid(rect, cur);
                self.pan.x += before.0 - after.0;
                self.pan.y += before.1 - after.1;
            }
        }

        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing {
            if ui.input(|i| i.key_pressed(egui::Key::R)) {
                match self.selected {
                    Some(Sel::Comp(i)) => self.sch.components[i].rot = (self.sch.components[i].rot + 1) % 4,
                    _ => self.rot = (self.rot + 1) % 4,
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
                self.delete_selected();
            }
            if ui.input(|i| i.key_pressed(egui::Key::F)) {
                self.fit_pending = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.pending = None;
                self.selected = None;
                self.tool = Tool::Select;
            }
        }

        // grid dots
        let tl = self.to_grid(rect, rect.left_top());
        let br = self.to_grid(rect, rect.right_bottom());
        if self.zoom >= 10.0 {
            for x in (tl.0.floor() as i32)..=(br.0.ceil() as i32) {
                for y in (tl.1.floor() as i32)..=(br.1.ceil() as i32) {
                    painter.circle_filled(self.to_screen(rect, (x as f32, y as f32)), 0.9, Color32::from_gray(190));
                }
            }
        }

        // wires
        for (i, w) in self.sch.wires.iter().enumerate() {
            let sel = self.selected == Some(Sel::Wire(i));
            painter.line_segment(
                [self.to_screen(rect, (w.a.0 as f32, w.a.1 as f32)), self.to_screen(rect, (w.b.0 as f32, w.b.1 as f32))],
                Stroke::new(if sel { 3.0 } else { 1.8 }, if sel { Color32::from_rgb(0, 80, 220) } else { WIRE }),
            );
        }
        // junction dots
        let pins: Vec<Pt> = self.sch.components.iter().flat_map(|c| c.pins().into_iter().map(|p| p.1)).collect();
        for w in &self.sch.wires {
            for p in [w.a, w.b] {
                let ends = self.sch.wires.iter().map(|x| (x.a == p) as u32 + (x.b == p) as u32).sum::<u32>()
                    + pins.iter().filter(|&&q| q == p).count() as u32;
                let on_other = self.sch.wires.iter().any(|x| x.a != p && x.b != p && on_segment(p, x.a, x.b));
                if ends >= 3 || on_other {
                    painter.circle_filled(self.to_screen(rect, (p.0 as f32, p.1 as f32)), 3.5, WIRE);
                }
            }
        }

        // components
        for (i, c) in self.sch.components.iter().enumerate() {
            let sel = self.selected == Some(Sel::Comp(i));
            let col = if sel { Color32::from_rgb(0, 80, 220) } else { SYM };
            self.draw_component(&painter, rect, c, col);
            for (_, p) in c.pins() {
                painter.rect_filled(
                    Rect::from_center_size(self.to_screen(rect, (p.0 as f32, p.1 as f32)), Vec2::splat(4.0)),
                    0.0,
                    Color32::from_rgb(190, 40, 40),
                );
            }
            let f = FontId::proportional((self.zoom * 0.55).clamp(8.0, 18.0));
            let (x0, x1, y0, y1) = comp_extents(c);
            let ctr = (c.pos.0 as f32 + (x0 + x1) / 2.0, c.pos.1 as f32);
            if c.kind.is_power_symbol() {
                if c.kind == Kind::Power {
                    painter.text(self.to_screen(rect, (c.pos.0 as f32, c.pos.1 as f32 - 0.8)), Align2::CENTER_BOTTOM, &c.value, f.clone(), SYM);
                }
            } else {
                let top = c.pos.1 as f32 + if c.rot % 2 == 0 { y0.min(-1.0) - 0.3 } else { -x1.max(2.0) - 0.3 };
                painter.text(self.to_screen(rect, (ctr.0, top)), Align2::CENTER_BOTTOM, &c.refdes, f.clone(), Color32::from_rgb(20, 60, 140));
                let bot = c.pos.1 as f32 + if c.rot % 2 == 0 { y1.max(1.0) + 0.3 } else { x1.max(2.0) + 0.3 };
                painter.text(self.to_screen(rect, (ctr.0, bot)), Align2::CENTER_TOP, &c.value, f, Color32::from_gray(60));
            }
        }
        for (i, l) in self.sch.labels.iter().enumerate() {
            let sel = self.selected == Some(Sel::Label(i));
            let p = self.to_screen(rect, (l.pos.0 as f32, l.pos.1 as f32));
            painter.circle_filled(p, 3.0, WIRE);
            painter.text(
                p + Vec2::new(5.0, -3.0),
                Align2::LEFT_BOTTOM,
                &l.text,
                FontId::proportional(13.0),
                if sel { Color32::from_rgb(0, 80, 220) } else { Color32::from_rgb(0, 100, 100) },
            );
        }
        if self.show_v {
            if let Some(dc) = self.dc.as_ref().filter(|d| d.signature == self.signature_cached()) {
                let f = FontId::proportional((self.zoom * 0.5).clamp(9.0, 14.0));
                let mut seen: Vec<Pt> = vec![];
                for (pt, v) in &dc.pin_v {
                    if seen.contains(pt) {
                        continue;
                    }
                    seen.push(*pt);
                    let p = self.to_screen(rect, (pt.0 as f32, pt.1 as f32)) + Vec2::new(5.0, -4.0);
                    let galley = painter.layout_no_wrap(eng(*v, "V"), f.clone(), Color32::from_rgb(10, 60, 200));
                    painter.rect_filled(Rect::from_min_size(p - Vec2::new(0.0, galley.size().y), galley.size()).expand(1.0), 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 190));
                    painter.galley(p - Vec2::new(0.0, galley.size().y), galley, Color32::from_rgb(10, 60, 200));
                }
            }
        }
        for pr in &self.problems {
            if let Some(a) = pr.at {
                painter.circle_stroke(self.to_screen(rect, (a.0 as f32, a.1 as f32)), 11.0, Stroke::new(2.0, Color32::from_rgb(230, 0, 200)));
            }
        }

        // interaction
        let Some(pos) = hover else { return };
        let g = self.to_grid(rect, pos);
        let snapped: Pt = (g.0.round() as i32, g.1.round() as i32);
        let sp = self.to_screen(rect, (snapped.0 as f32, snapped.1 as f32));

        match self.tool {
            Tool::Place => {
                let ghost = Color32::from_rgba_unmultiplied(120, 20, 20, 120);
                match self.place_lib.and_then(|k| self.library.get(k)) {
                    Some(d) => self.draw_custom(&painter, rect, &d.symbol(), snapped, self.rot, ghost),
                    None => self.draw_symbol(&painter, rect, self.place, snapped, self.rot, ghost),
                }
            }
            Tool::Wire => {
                painter.circle_stroke(sp, 4.0, Stroke::new(1.5, WIRE));
                if let Some(a) = self.pending {
                    let a_s = self.to_screen(rect, (a.0 as f32, a.1 as f32));
                    let m = self.to_screen(rect, (snapped.0 as f32, a.1 as f32));
                    let st = Stroke::new(1.5, Color32::from_rgba_unmultiplied(0, 110, 30, 150));
                    painter.line_segment([a_s, m], st);
                    painter.line_segment([m, sp], st);
                }
            }
            Tool::Label => {
                painter.circle_stroke(sp, 4.0, Stroke::new(1.5, Color32::from_rgb(0, 100, 100)));
            }
            Tool::Select => {}
        }
        self.status = format!("({}, {})  部品 {}  配線 {}", snapped.0, snapped.1, self.sch.components.len(), self.sch.wires.len());

        if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select {
            if let Some(i) = self.comp_at(g) {
                let c = &self.sch.components[i];
                self.drag = Some((i, (snapped.0 - c.pos.0, snapped.1 - c.pos.1)));
                self.selected = Some(Sel::Comp(i));
            }
        }
        if let Some((i, off)) = self.drag {
            if resp.dragged_by(egui::PointerButton::Primary) {
                self.sch.components[i].pos = (snapped.0 - off.0, snapped.1 - off.1);
            } else {
                self.drag = None;
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }

        if resp.clicked_by(egui::PointerButton::Primary) {
            match self.tool {
                Tool::Select => self.selected = self.pick(g),
                Tool::Place => {
                    match self.place_lib.and_then(|k| self.library.get(k)).cloned() {
                        Some(d) => {
                            self.sch.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), snapped, self.rot);
                        }
                        None => {
                            self.sch.add(self.place, snapped, self.rot);
                        }
                    }
                }
                Tool::Wire => match self.pending.take() {
                    None => self.pending = Some(snapped),
                    Some(a) => {
                        self.add_wire(a, snapped);
                        self.pending = Some(snapped);
                    }
                },
                Tool::Label => {
                    if !self.label_text.is_empty() {
                        self.sch.labels.push(Label { pos: snapped, text: self.label_text.clone() });
                    }
                }
            }
        }
        if resp.double_clicked() && self.tool == Tool::Wire {
            self.pending = None;
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            self.pending = None;
        }
    }

    // ------------------------------------------------------------- side panel

    pub fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("回路図");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Select, "選択");
            ui.selectable_value(&mut self.tool, Tool::Wire, "配線");
            ui.selectable_value(&mut self.tool, Tool::Label, "ラベル");
        });
        if self.tool == Tool::Label {
            ui.text_edit_singleline(&mut self.label_text);
        }
        ui.separator();
        ui.label("部品 (クリックして配置 / Rキーで回転)");
        for k in Kind::PALETTE {
            if ui.selectable_label(self.tool == Tool::Place && self.place_lib.is_none() && self.place == *k, k.label()).clicked() {
                self.tool = Tool::Place;
                self.place_lib = None;
                self.place = *k;
                self.pending = None;
            }
        }
        ui.separator();
        let mut chosen = None;
        egui::CollapsingHeader::new(format!("ライブラリ部品 ({})", self.library.len())).default_open(true).show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.lib_filter).hint_text("検索"));
            let f = self.lib_filter.to_lowercase();
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                // grouped by category, then by name
                let cat = |d: &PartDef| if d.category.is_empty() { "その他".to_string() } else { d.category.clone() };
                let mut order: Vec<usize> = (0..self.library.len()).collect();
                order.sort_by_key(|&i| (cat(&self.library[i]), self.library[i].name.to_lowercase()));
                let mut last = String::new();
                for i in order {
                    let d = &self.library[i];
                    let c = cat(d);
                    if !f.is_empty() && ![&d.name, &d.description, &c].iter().any(|t| t.to_lowercase().contains(&f)) {
                        continue;
                    }
                    if c != last {
                        ui.label(egui::RichText::new(&c).small().weak());
                        last = c;
                    }
                    let mark = if d.sim.is_some() { " ◎" } else { "" };
                    let selected = self.tool == Tool::Place && self.place_lib == Some(i);
                    if ui.selectable_label(selected, format!("  {}{mark}", d.name)).on_hover_text(&d.description).clicked() {
                        chosen = Some(i);
                    }
                }
            });
            ui.small("◎ = シミュレーションモデルあり");
            ui.horizontal(|ui| {
                if ui.button("新規作成…").clicked() {
                    self.form = Some(PartForm::default());
                }
                if ui.button("JSONから追加…").clicked() {
                    self.import_part_json();
                }
                if ui.button("再読み込み").clicked() {
                    self.reload_library();
                }
            });
            ui.small(format!("保存先: {}", self.lib_dir.display()));
            for e in &self.lib_errors {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
            }
        });
        if let Some(i) = chosen {
            self.tool = Tool::Place;
            self.place_lib = Some(i);
            self.pending = None;
        }
        ui.separator();
        if let Some(Sel::Comp(i)) = self.selected {
            if i < self.sch.components.len() {
                let c = &mut self.sch.components[i];
                ui.label("選択中の部品");
                if !c.kind.is_power_symbol() {
                    ui.horizontal(|ui| {
                        ui.label("番号");
                        ui.text_edit_singleline(&mut c.refdes);
                    });
                }
                if c.kind != Kind::Ground {
                    ui.horizontal(|ui| {
                        ui.label("値");
                        ui.text_edit_singleline(&mut c.value);
                    });
                }
                if c.kind == Kind::VSource {
                    ui.small("例: 5 / SIN(0 1 1k) / PULSE(0 5 1m 1u 1u 2m 5m) / PWL(0 0 1m 5) / AC 1 (AC解析の入力) / 5 AC 1");
                    ui.small("ノイズ: NOISE(実効値 最高周波数 [シード]) 例: 5 + NOISE(0.05 1Meg) + SIN(0 0.02 100k) (+ の前後は空白)");
                }
                if c.kind == Kind::Capacitor {
                    ui.small("例: 100n / 100n ESR=20m ESL=5n (等価直列抵抗・インダクタンス)");
                }
                if c.kind == Kind::Inductor {
                    ui.small("例: 10u / 10u DCR=0.1 (巻線抵抗)");
                }
                if let Some(sym) = &c.custom {
                    ui.label(format!("フットプリント: {}", c.footprint));
                    match &sym.sim {
                        Some(m) => ui.label(format!("モデル: {}", m.name)),
                        None => ui.colored_label(Color32::from_rgb(220, 160, 40), "シミュレーションモデルなし"),
                    };
                } else if !c.kind.is_power_symbol() {
                    let current = c.footprint_name();
                    egui::ComboBox::from_label("フットプリント").selected_text(current).show_ui(ui, |ui| {
                        for n in pcb_core::footprints::names() {
                            ui.selectable_value(&mut c.footprint, n.clone(), n);
                        }
                    });
                }
                if ui.button("回転 (R)").clicked() {
                    c.rot = (c.rot + 1) % 4;
                }
            }
        }
        if self.selected.is_some() && ui.button("削除 (Delete)").clicked() {
            self.delete_selected();
        }
        ui.separator();
        if ui.button("ERC (電気ルールチェック)").clicked() {
            self.problems = erc(&self.sch);
            self.status = format!("ERC: 問題 {} 件", self.problems.len());
        }
        for p in &self.problems {
            ui.colored_label(Color32::from_rgb(230, 0, 200), &p.message);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("DC動作点を解析").clicked() {
                self.run_dc();
            }
            ui.checkbox(&mut self.show_v, "電圧表示");
        });
        ui.separator();
        ui.label("過渡解析");
        ui.horizontal(|ui| {
            ui.label("終了");
            ui.add(egui::TextEdit::singleline(&mut self.tran_tstop).desired_width(56.0));
            ui.label("刻み");
            ui.add(egui::TextEdit::singleline(&mut self.tran_tstep).desired_width(56.0));
        });
        ui.horizontal(|ui| {
            if ui.button("過渡解析を実行").clicked() {
                self.run_tran();
            }
            ui.checkbox(&mut self.tran_uic, "ゼロ状態から");
        });
        ui.separator();
        ui.label("AC解析 (周波数特性)");
        ui.horizontal(|ui| {
            ui.label("開始");
            ui.add(egui::TextEdit::singleline(&mut self.ac_fstart).desired_width(48.0));
            ui.label("終了");
            ui.add(egui::TextEdit::singleline(&mut self.ac_fstop).desired_width(48.0));
            ui.label("点/dec");
            ui.add(egui::TextEdit::singleline(&mut self.ac_ppd).desired_width(30.0));
        });
        if ui.button("AC解析を実行").clicked() {
            self.run_ac();
        }
        ui.small("入力にする電源の値を「AC 1」にしてください");
        ui.separator();
        let stale = self.dc.as_ref().is_some_and(|d| d.signature != self.signature());
        if let Some(dc) = &self.dc {
            if stale {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "回路が変更されました。再度解析してください");
            }
            match &dc.outcome {
                Err(msgs) => {
                    for m in msgs {
                        ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                    }
                }
                Ok(res) => {
                    for w in &res.warnings {
                        ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                    }
                    egui::CollapsingHeader::new("解析結果").default_open(true).show(ui, |ui| {
                        egui::Grid::new("dc_nodes").striped(true).show(ui, |ui| {
                            for (n, v) in res.voltages.iter().filter(|(n, _)| !n.contains('.')) {
                                ui.label(n);
                                ui.label(eng(*v, "V"));
                                ui.end_row();
                            }
                        });
                        ui.separator();
                        egui::Grid::new("dc_elems").striped(true).show(ui, |ui| {
                            ui.strong("素子");
                            ui.strong("電流");
                            ui.strong("電圧");
                            ui.strong("電力");
                            ui.end_row();
                            for e in res.elements.iter().filter(|e| !e.name.contains('.')) {
                                ui.label(&e.name);
                                ui.label(eng(e.current, "A"));
                                ui.label(eng(e.voltage, "V"));
                                ui.label(eng(e.power, "W"));
                                ui.end_row();
                            }
                        });
                        ui.small("電流は素子の第1端子→第2端子の向き。電源は電力を供給すると負");
                    });
                }
            }
        }
        ui.collapsing("ネットリスト", |ui| {
            for n in extract(&self.sch) {
                let pins: Vec<String> = n.pins.iter().map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
                ui.label(format!("{}: {}", n.name, pins.join(", ")));
            }
        });
        if ui.button("SPICEネットリストを書き出し…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("SPICE", &["cir"]).set_file_name("circuit.cir").save_file() {
                self.status = match std::fs::write(&p, to_spice(&self.sch, "circuitCad")) {
                    Ok(()) => format!("書き出しました: {}", p.display()),
                    Err(e) => format!("書き出し失敗: {e}"),
                };
            }
        }
        ui.separator();
        if let Some(mut form) = self.form.take() {
            let mut open = true;
            let mut saved = None;
            egui::Window::new("部品を作成").open(&mut open).default_width(560.0).vscroll(true).show(ui.ctx(), |ui| {
                saved = form.ui(ui);
            });
            match saved {
                Some(def) => match part_lib::save(&self.lib_dir, &def) {
                    Ok(p) => {
                        self.reload_library();
                        self.status = format!("ライブラリに保存しました: {}", p.display());
                    }
                    Err(e) => {
                        self.status = format!("保存失敗: {e}");
                        self.form = Some(form);
                    }
                },
                None if open => self.form = Some(form),
                None => {}
            }
        }
        ui.small("左クリック: 配置 / 配線 (斜めは自動でL字)\n右クリック・ダブルクリック: 配線を終了\n中・右ドラッグ: 移動 / ホイール: 拡大縮小");
    }

    // ------------------------------------------------------------- files

    pub fn new_file(&mut self) {
        *self = SchEditor::default();
    }

    pub fn save(&mut self, as_new: bool) {
        let path = if as_new || self.path.is_none() {
            rfd::FileDialog::new().add_filter("circuitCad 回路図", &["schjson"]).set_file_name("circuit.schjson").save_file()
        } else {
            self.path.clone()
        };
        if let Some(p) = path {
            self.status = match serde_json::to_string_pretty(&self.sch).map(|s| std::fs::write(&p, s)) {
                Ok(Ok(())) => {
                    self.path = Some(p.clone());
                    format!("保存しました: {}", p.display())
                }
                Ok(Err(e)) => format!("保存失敗: {e}"),
                Err(e) => format!("保存失敗: {e}"),
            };
        }
    }

    pub fn open(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("circuitCad 回路図", &["schjson"]).pick_file() {
            self.load_path(p);
        }
    }

    pub fn load_path(&mut self, p: PathBuf) {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string())) {
            Ok(s) => {
                self.sch = s;
                self.path = Some(p.clone());
                self.selected = None;
                self.pending = None;
                self.problems.clear();
                self.dc = None;
                self.fit_pending = true;
                self.status = format!("開きました: {}", p.display());
            }
            Err(e) => self.status = format!("読み込み失敗: {e}"),
        }
    }

    pub fn analyze_dc(&mut self) {
        self.run_dc();
    }

    #[cfg(test)]
    pub fn pin_voltage(&self, refdes: &str, pin: &str) -> Option<f64> {
        let c = self.sch.components.iter().find(|c| c.refdes == refdes)?;
        let (_, pt) = c.pins().into_iter().find(|(n, _)| n == pin)?;
        self.dc.as_ref()?.pin_v.iter().find(|(p, _)| *p == pt).map(|(_, v)| *v)
    }
}
