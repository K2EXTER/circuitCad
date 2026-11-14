//! The "断面と電界" window: the cross-section of a track (or a pair) as it is after etching, with the solder mask, the potential
//! the field solver found around it, and the impedance, εeff and delay it gives (`pcb_core::fieldsolve`). Also the layer-stack
//! fields shared with the left panel, and `in_background`: the solver takes 0.1-0.5 s, so the windows ask it on a thread and show
//! the closed-form estimate until it answers.

use super::*;
use pcb_core::fieldsolve::{self as fs, FieldMap, Section, Solved};
use pcb_core::impedance::{Kind, Stackup};
use std::collections::HashMap;
use std::sync::Mutex;

/// Questions put to the solver on a thread, by key: false while it runs, true once its answers are in the solver's memory.
static JOBS: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);

/// Runs `job` on a thread once for `key` and returns whether it has finished (then the same question to the solver is answered at
/// once from its memory). The window is redrawn when it finishes.
pub(super) fn in_background(ctx: &egui::Context, key: String, job: impl FnOnce() + Send + 'static) -> bool {
    {
        let mut g = JOBS.lock().unwrap();
        let m = g.get_or_insert_with(Default::default);
        if let Some(done) = m.get(&key) {
            return *done;
        }
        if m.len() > 4000 {
            m.retain(|_, done| !*done);
        }
        m.insert(key.clone(), false);
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        job();
        if let Some(m) = JOBS.lock().unwrap().as_mut() {
            m.insert(key, true);
        }
        ctx.request_repaint();
    });
    false
}

/// The impedance of one track of `width` on `kind` from the field, or `None` while the solver is still on it (asked on a thread).
pub(super) fn single_z(ctx: &egui::Context, s: &Stackup, kind: Kind, width: f64) -> Option<f64> {
    let sec = Section::single(kind, width);
    if fs::is_known(s, &sec) {
        return Some(fs::solve(s, &sec).main_z());
    }
    let s2 = *s;
    in_background(ctx, format!("z {s:?} {sec:?}"), move || {
        fs::solve(&s2, &sec);
    });
    None
}

/// The width of one track for `target` Ω from the field (`Some(None)`: none reaches it), or `None` while the solver is on it.
pub(super) fn width_for_z(ctx: &egui::Context, s: &Stackup, kind: Kind, target: f64) -> Option<Option<f64>> {
    let key = format!("w {s:?} {kind:?} {target}");
    let s2 = *s;
    in_background(ctx, key, move || {
        fs::width_for(&s2, &Section::single(kind, 0.0), target);
    })
    .then(|| fs::width_for(s, &Section::single(kind, 0.0), target))
}

/// The layer-stack fields (the left panel's "層構成" and the window's). True when one changed.
pub(super) fn stackup_fields(ui: &mut egui::Ui, s: &mut Stackup) -> bool {
    let before = *s;
    ui.horizontal_wrapped(|ui| {
        ui.add(egui::DragValue::new(&mut s.er).speed(0.05).range(2.0..=12.0).prefix(i18n::tr("誘電率 "))).on_hover_text(i18n::tr("基板の材料の比誘電率。FR-4 は 4.2〜4.6 ほどで、材料と周波数で変わります"));
        ui.add(egui::DragValue::new(&mut s.height).speed(0.01).range(0.05..=3.0).prefix(i18n::tr("外層から基準面まで ")).suffix(" mm")).on_hover_text(i18n::tr("外層の配線から、その下の銅 (2層基板なら反対の面、4層基板なら隣の内層のベタ) までの距離。2層の 1.6 mm 基板なら約 1.53 mm、4層基板なら 0.1〜0.2 mm ほどです"));
        ui.add(egui::DragValue::new(&mut s.plane_gap).speed(0.01).range(0.1..=3.0).prefix(i18n::tr("内層の上下のベタの間 ")).suffix(" mm")).on_hover_text(i18n::tr("内層の配線を上下から挟む2枚のベタの間の距離 (ストリップライン)"));
        ui.add(egui::DragValue::new(&mut s.copper_um).speed(1.0).range(9.0..=140.0).prefix(i18n::tr("銅箔 ")).suffix(" µm")).on_hover_text(i18n::tr("仕上がりの銅箔の厚さ。1 oz = 35 µm。外層は、めっきを含めた厚さです"));
    });
    ui.horizontal_wrapped(|ui| {
        ui.add(egui::DragValue::new(&mut s.etch_um).speed(1.0).range(0.0..=100.0).prefix(i18n::tr("エッチングで上が細る ")).suffix(" µm")).on_hover_text(i18n::tr("エッチング (銅を溶かして配線を作る工程) では、銅の上の面が下の面より細く、断面が台形になります。上の辺が下の辺より何 µm 細いか (両側の合計)。1 oz で 25 µm ほどが目安です。描いた線幅は下の辺の幅です。0 なら長方形で計算します"));
        ui.add(egui::DragValue::new(&mut s.mask_um).speed(1.0).range(0.0..=60.0).prefix(i18n::tr("レジスト (基板の上) ")).suffix(" µm")).on_hover_text(i18n::tr("ソルダーレジスト (基板の緑などの膜) の、配線のない所の厚さ。20 µm ほどが目安です。0 ならレジストなしで計算します。外層だけに効きます"));
        ui.add(egui::DragValue::new(&mut s.mask_top_um).speed(1.0).range(0.0..=60.0).prefix(i18n::tr("レジスト (銅の上) ")).suffix(" µm")).on_hover_text(i18n::tr("配線の上と横のレジストの厚さ。基板の上より薄く、10 µm ほどが目安です"));
        ui.add(egui::DragValue::new(&mut s.mask_er).speed(0.05).range(2.0..=6.0).prefix(i18n::tr("レジストの誘電率 "))).on_hover_text(i18n::tr("ソルダーレジストの比誘電率。3.5〜4 ほどです"));
    });
    *s != before
}

/// What the window shows: the section asked about, and the picture of its field.
pub struct FieldView {
    pub open: bool,
    pub inner: bool,
    pub width: f64,
    pub pair: bool,
    pub space: f64,
    pub coplanar: bool,
    pub gap: f64,
    pub target: f64,
    pending: Option<(String, std::sync::mpsc::Receiver<FieldMap>)>,
    picture: Option<(String, egui::TextureHandle, Picture)>,
}

impl Default for FieldView {
    fn default() -> Self {
        Self { open: false, inner: false, width: 0.3, pair: false, space: 0.2, coplanar: false, gap: 0.2, target: 50.0, pending: None, picture: None }
    }
}

/// The area drawn (mm, x from the middle) and the copper in it.
struct Picture {
    half: f64,
    top: f64,
    copper: Vec<(Vec<[f64; 2]>, bool)>,
    eps: Vec<([f64; 4], f64)>,
    nodes: usize,
}

impl FieldView {
    fn section(&self) -> Section {
        Section {
            kind: if self.inner { Kind::Stripline } else { Kind::Microstrip },
            width: self.width,
            space: self.pair.then_some(self.space),
            ground_gap: self.coplanar.then_some(self.gap),
        }
    }
}

impl BoardEditor {
    /// Opens the window on a track of `width` on `side` (from the net window).
    pub(super) fn open_field_view(&mut self, width: f64, side: Side) {
        let f = &mut self.field_view;
        f.open = true;
        f.width = (width * 1000.0).round() / 1000.0;
        f.inner = matches!(side, Side::Inner(_));
    }

    pub fn field_window(&mut self, ctx: &egui::Context) {
        if !self.field_view.open {
            return;
        }
        let mut open = true;
        egui::Window::new(i18n::tr("断面と電界 (インピーダンス)")).id(egui::Id::new("field_window")).open(&mut open).default_width(640.0).default_pos([420.0, 120.0]).show(ctx, |ui| {
            ui.small(i18n::tr("配線の断面のまわりの電界を、断面を細かい格子に分けて解き (電界ソルバ)、特性インピーダンスを求めます。銅はエッチング後の台形、外層はレジストをかぶった、仕上がりの形で解きます"));
            let fv = &mut self.field_view;
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut fv.inner, false, i18n::tr("外層")).on_hover_text(i18n::tr("表面・裏面の配線。下の銅 (基準面) との間に基板、上は空気とレジスト (マイクロストリップ)"));
                ui.radio_value(&mut fv.inner, true, i18n::tr("内層")).on_hover_text(i18n::tr("4層以上の基板の内側の配線。上下を2枚のベタに挟まれ、まわりは全部基板 (ストリップライン)"));
                ui.separator();
                ui.add(egui::DragValue::new(&mut fv.width).speed(0.005).range(0.03..=10.0).prefix(i18n::tr("線幅 ")).suffix(" mm")).on_hover_text(i18n::tr("描いた配線の幅 (銅の下の辺)"));
                ui.checkbox(&mut fv.pair, i18n::tr("差動ペア")).on_hover_text(i18n::tr("同じ幅の2本を並べ、逆向きの信号を流す (USB・LVDS など)。差動インピーダンスを出します"));
                if fv.pair {
                    ui.add(egui::DragValue::new(&mut fv.space).speed(0.005).range(0.03..=10.0).prefix(i18n::tr("間隔 ")).suffix(" mm")).on_hover_text(i18n::tr("2本の配線の、銅の縁から縁までの距離"));
                }
                ui.checkbox(&mut fv.coplanar, i18n::tr("横に GND の銅")).on_hover_text(i18n::tr("配線の両横の、同じ層に GND のベタがある (グランド付きコプレーナ線路)。近いほどインピーダンスが下がります"));
                if fv.coplanar {
                    ui.add(egui::DragValue::new(&mut fv.gap).speed(0.005).range(0.05..=10.0).prefix(i18n::tr("すき間 ")).suffix(" mm")).on_hover_text(i18n::tr("配線の縁から、横の GND の銅までの距離"));
                }
            });
            let sec = fv.section();
            let mut s = self.board.stackup;
            egui::CollapsingHeader::new(i18n::tr("層構成 (この基板の値)")).id_salt("field_stackup").default_open(false).show(ui, |ui| {
                ui.small(i18n::tr("左パネルの「層構成 (層数・材料)」と同じ値です。ここで変えると基板の値も変わります。発注先の公式ページの積層構成に合わせてください"));
                stackup_fields(ui, &mut s);
            });
            if s != self.board.stackup {
                self.checkpoint_once();
                self.board.stackup = s;
            }
            ui.separator();
            // --- the answer (asked on a thread; the closed forms meanwhile)
            let key = format!("sec {s:?} {sec:?}");
            let s2 = s;
            let ready = fs::is_known(&s, &sec) || in_background(ctx, key, move || {
                fs::solve(&s2, &sec);
            });
            let formula = match sec.space {
                Some(sp) => pcb_core::impedance::differential(&s, sec.kind, sec.width, sp),
                None => pcb_core::impedance::single_ended(&s, sec.kind, sec.width),
            };
            if !ready {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(i18n::trf!("電界を解いています… (式による目安: 約 {formula:.1} Ω)"));
                });
            } else {
                let r = fs::solve(&s, &sec);
                let row = |ui: &mut egui::Ui, name: &str, tip: &str, v: String| {
                    ui.label(i18n::tr_dyn(name)).on_hover_text(i18n::tr_dyn(tip));
                    ui.label(egui::RichText::new(v).strong());
                    ui.end_row();
                };
                egui::Grid::new("field_results").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    let mode_rows = |ui: &mut egui::Ui, m: &fs::Mode| {
                        row(ui, "実効誘電率 εeff", "信号は、まわりが全部この誘電率の材料であるかのように伝わります。外層は空気とレジストがあるので、基板の誘電率より小さくなります", format!("{:.3}", m.er_eff));
                        row(ui, "遅延", "信号が 1 mm 進むのにかかる時間 (√εeff ÷ 光の速さ)。長さの合わせ込みの目安です", format!("{:.2} ps/mm", m.delay_ps_mm));
                        row(ui, "容量・インダクタンス", "配線 1 m あたりの、GND との間の容量と、インダクタンス。Z0 = √(L / C)", format!("{:.1} pF/m · {:.1} nH/m", m.c_pf_m, m.l_nh_m));
                    };
                    match r {
                        Solved::Single(m) => {
                            row(ui, "特性インピーダンス Z0", "この配線の特性インピーダンス (電界ソルバ)", format!("{:.1} Ω", m.z));
                            mode_rows(ui, &m);
                        }
                        Solved::Pair { odd, even } => {
                            row(ui, "差動インピーダンス", "2本に逆向きの信号を流したときの、2本の間のインピーダンス (= 2 × 奇モード)。USB は 90 Ω、LVDS・Ethernet は 100 Ω が目標です", format!("{:.1} Ω", r.main_z()));
                            row(ui, "同相インピーダンス", "2本に同じ向きの信号を流したときの、2本まとめたインピーダンス (= 偶モード ÷ 2)", format!("{:.1} Ω", r.common_z().unwrap_or(0.0)));
                            row(ui, "奇モード・偶モード", "1本あたりのインピーダンス。逆向き (奇) と同じ向き (偶)。近づけるほど開きます", format!("{:.1} Ω · {:.1} Ω", odd.z, even.z));
                            mode_rows(ui, &odd);
                        }
                    }
                    if sec.ground_gap.is_none() {
                        let d = (formula - r.main_z()) / r.main_z() * 100.0;
                        row(ui, "教科書の式では", "Hammerstad-Jensen・IPC-2141 の近似式 (銅は長方形、レジストなし)。速いかわりに ±10% ほどずれます。式と電界ソルバの差が、形の違い (台形・レジスト) と式の誤差です", format!("{formula:.1} Ω ({d:+.1}%)"));
                    }
                });
            }
            // --- a width (or space) for a wanted impedance
            ui.horizontal_wrapped(|ui| {
                ui.label(i18n::tr("目標"));
                ui.add(egui::DragValue::new(&mut self.field_view.target).speed(0.5).range(10.0..=200.0).suffix(" Ω")).on_hover_text(i18n::tr("求めたいインピーダンス。1本なら Z0 (50 Ω など)、差動ペアなら差動インピーダンス (90 Ω・100 Ω など)"));
                let t = self.field_view.target;
                if ui.button(i18n::tr("線幅を求める")).on_hover_text(i18n::tr("目標のインピーダンスになる線幅を、電界ソルバで探して、上の線幅に入れます (ペアなら間隔はそのまま)")).clicked() {
                    let w = match sec.space {
                        Some(sp) => fs::width_for_differential(&s, &sec, sp, t),
                        None => fs::width_for(&s, &sec, t),
                    };
                    match w {
                        Some(w) => self.field_view.width = (w * 1000.0).round() / 1000.0,
                        None => self.status = format!("この層構成では、{t:.0} Ω にできる線幅が見つかりません (0.03〜20 mm)。層の間隔や目標を見直してください"),
                    }
                }
                if sec.space.is_some() && ui.button(i18n::tr("間隔を求める")).on_hover_text(i18n::tr("目標の差動インピーダンスになる、2本の間隔を探します (線幅はそのまま)")).clicked() {
                    match fs::space_for_differential(&s, &sec, sec.width, t) {
                        Some(sp) => self.field_view.space = (sp * 1000.0).round() / 1000.0,
                        None => self.status = format!("この線幅では、間隔をどう変えても差動 {t:.0} Ω になりません。線幅を変えてください"),
                    }
                }
            });
            ui.separator();
            self.field_picture(ui, &s, &sec);
        });
        if !open {
            self.field_view.open = false;
        }
    }

    /// The cross-section with the potential (asked on a thread; drawn when it comes back).
    fn field_picture(&mut self, ui: &mut egui::Ui, s: &Stackup, sec: &Section) {
        let key = format!("{s:?} {sec:?}");
        let fv = &mut self.field_view;
        if fv.picture.as_ref().is_none_or(|p| p.0 != key) && fv.pending.as_ref().is_none_or(|p| p.0 != key) {
            let (tx, rx) = std::sync::mpsc::channel();
            let (s2, sec2, ctx) = (*s, *sec, ui.ctx().clone());
            std::thread::spawn(move || {
                let _ = tx.send(fs::field_map(&s2, &sec2));
                ctx.request_repaint();
            });
            fv.pending = Some((key.clone(), rx));
        }
        if let Some((k, rx)) = &fv.pending
            && let Ok(map) = rx.try_recv()
        {
            let k = k.clone();
            let (img, pic) = render(&map, s, sec);
            let tex = ui.ctx().load_texture("field_map", img, egui::TextureOptions::LINEAR);
            fv.picture = Some((k, tex, pic));
            fv.pending = None;
        }
        let Some((k, tex, pic)) = &fv.picture else {
            ui.spinner();
            return;
        };
        if *k != key {
            ui.small(i18n::tr("図を描き直しています…"));
        }
        let size = tex.size_vec2();
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
        let p = ui.painter_at(rect);
        p.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        let to = |x: f64, y: f64| egui::pos2(rect.left() + ((x + pic.half) / (2.0 * pic.half)) as f32 * rect.width(), rect.bottom() - (y / pic.top) as f32 * rect.height());
        // the material edges, then the copper on both halves
        for (r, _) in &pic.eps {
            let y = r[3];
            if y < pic.top {
                p.line_segment([to(-pic.half, y), to(pic.half, y)], egui::Stroke::new(1.0, Color32::from_black_alpha(70)));
            }
        }
        let copper = Color32::from_rgb(214, 140, 60);
        for (poly, track) in &pic.copper {
            for sign in [1.0, -1.0] {
                let pts: Vec<egui::Pos2> = poly.iter().map(|c| to(sign * c[0].min(pic.half * 1.01), c[1])).collect();
                p.add(egui::Shape::convex_polygon(pts, copper, egui::Stroke::new(1.0, if *track { Color32::from_rgb(120, 60, 10) } else { Color32::from_gray(80) })));
            }
        }
        p.line_segment([to(-pic.half, 0.0), to(pic.half, 0.0)], egui::Stroke::new(3.0, copper));
        if sec.kind == Kind::Stripline {
            p.line_segment([to(-pic.half, pic.top), to(pic.half, pic.top)], egui::Stroke::new(3.0, copper));
        }
        resp.on_hover_text(i18n::tr("配線の断面。色は電位で、赤がプラス、青がマイナス (差動ペアの逆側)、白が 0 V です。縞の1本ごとに 0.1 V 変わり、縞が詰まっている所ほど電界が強く、そこに電気のエネルギーが集まっています。横線は材料の境目 (基板・レジスト)、オレンジは銅です"));
        let mm = 2.0 * pic.half;
        ui.small(i18n::trf!("幅 {mm:.2} mm の範囲 · 格子 {} 点で解きました", pic.nodes));
    }
}

/// The potential as a picture of the area around the copper: red for +, blue for −, white for 0 V, a darker band every 0.1 V; the
/// board material tinted.
fn render(map: &FieldMap, s: &Stackup, sec: &Section) -> (egui::ColorImage, Picture) {
    let outer = map.copper.iter().filter(|c| c.1 > 0.0).map(|c| c.0[1][0]).fold(0.0, f64::max);
    let ground_edge = map.copper.iter().filter(|c| c.1 == 0.0).map(|c| c.0[0][0]).fold(f64::INFINITY, f64::min);
    let (half, top) = match sec.kind {
        Kind::Microstrip => {
            let h = s.height;
            let half = (outer + 1.5 * h).max(outer * 1.6).min(if ground_edge.is_finite() { ground_edge + h.max(0.5 * outer) } else { f64::INFINITY });
            (half, h + s.copper_um / 1000.0 + 1.2 * h)
        }
        Kind::Stripline => (outer + 1.2 * s.plane_gap, s.plane_gap),
    };
    let scale = (620.0 / (2.0 * half)).min(300.0 / top);
    let (w, h) = (((2.0 * half * scale) as usize).max(40), ((top * scale) as usize).max(30));
    let nx = map.xs.len();
    let at = |x: f64, y: f64| -> (f64, f64) {
        let (ax, sign) = (x.abs(), if x < 0.0 && map.odd { -1.0 } else { 1.0 });
        let i = map.xs.partition_point(|v| *v <= ax).clamp(1, nx - 1) - 1;
        let j = map.ys.partition_point(|v| *v <= y).clamp(1, map.ys.len() - 1) - 1;
        let fx = ((ax - map.xs[i]) / (map.xs[i + 1] - map.xs[i])).clamp(0.0, 1.0);
        let fy = ((y - map.ys[j]) / (map.ys[j + 1] - map.ys[j])).clamp(0.0, 1.0);
        let v = |i: usize, j: usize| map.v[j * nx + i];
        let pot = (v(i, j) * (1.0 - fx) + v(i + 1, j) * fx) * (1.0 - fy) + (v(i, j + 1) * (1.0 - fx) + v(i + 1, j + 1) * fx) * fy;
        (sign * pot, map.eps[j * (nx - 1) + i])
    };
    let samples: Vec<(f64, f64, f64)> = (0..h)
        .flat_map(|py| {
            let y = top * (1.0 - (py as f64 + 0.5) / h as f64);
            (0..w).map(move |px| (-half + 2.0 * half * (px as f64 + 0.5) / w as f64, y))
        })
        .map(|(x, y)| {
            let (v, e) = at(x, y);
            (v, e, y)
        })
        .collect();
    // an equipotential line every 0.1 V: where the step of the potential changes from a pixel to the one right of it or below it
    let step = |v: f64| (v * 10.0).floor() as i32;
    let mut pixels = Vec::with_capacity(w * h);
    for py in 0..h {
        for px in 0..w {
            let (v, e, y) = samples[py * w + px];
            // the material: air white, the mask a little green, the board a little yellow
            let base = if e <= 1.0 { [250.0, 250.0, 250.0] } else if sec.kind == Kind::Microstrip && y > s.height - 1e-9 { [225.0, 240.0, 222.0] } else { [236.0, 232.0, 210.0] };
            let a = v.abs().min(1.0) * 0.85;
            let tint = if v >= 0.0 { [220.0, 40.0, 40.0] } else { [40.0, 80.0, 220.0] };
            let line = (px + 1 < w && step(samples[py * w + px + 1].0) != step(v)) || (py + 1 < h && step(samples[(py + 1) * w + px].0) != step(v));
            let dark = if line { 0.45 } else { 0.0 };
            let c: Vec<u8> = (0..3).map(|k| ((base[k] * (1.0 - a) + tint[k] * a) * (1.0 - dark)) as u8).collect();
            pixels.push(Color32::from_rgb(c[0], c[1], c[2]));
        }
    }
    let img = egui::ColorImage::new([w, h], pixels);
    let copper = map.copper.iter().map(|(c, v)| (c.to_vec(), *v > 0.0)).collect();
    // the edges between materials, drawn as lines: the top of the board and of the mask (a stripline is board throughout)
    let mut eps = vec![];
    if sec.kind == Kind::Microstrip {
        eps.push(([0.0, 0.0, 0.0, s.height], s.er));
        if s.mask_um > 0.0 {
            eps.push(([0.0, 0.0, 0.0, s.height + s.mask_um / 1000.0], s.mask_er));
        }
    }
    (img, Picture { half, top, copper, eps, nodes: map.nodes })
}
