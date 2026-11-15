//! "SolidCraft で形を作る": the outline drawings cut out of the data sheet (seen from above and from the side) go to SolidCraft as
//! pictures to draw over (its canvases), scaled to millimetres, with a starting body from the form's dimensions; SolidCraft opens the
//! design, the person models the part and exports a STEP into the same folder, and the form takes it as the part's 3D shape.
//!
//! SolidCraft is an outside program (docs/integration-policy.md): found by `SOLIDCRAFT_DIR`, else the PATH, else its usual install
//! folder; without it the panel says how to get it and nothing else changes. The only thing particular to SolidCraft is the command
//! script (`script`) and the two program names: the drawings are plain PNG files in the folder, usable with any other CAD.

use super::*;
use std::path::{Path, PathBuf};

/// Which way a drawing looks at the part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// From above: the board's plane (x right, y up).
    Top,
    /// From the front: x right, z up.
    Side,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Top => "上から見た図",
            View::Side => "横から見た図",
        }
    }

    fn file(self) -> &'static str {
        match self {
            View::Top => "top.png",
            View::Side => "side.png",
        }
    }
}

/// A drawing cut out of a page of the data sheet, and the two points and the distance that set its scale.
pub struct Drawing {
    pub view: View,
    pub page: usize,
    pub rgba: Vec<u8>,
    /// Pixels (width, height).
    pub size: [usize; 2],
    /// The cut's width in PDF points (1/72 in): the drawing's size as printed, used until two points set the scale.
    pub width_pt: f64,
    /// Two points on the picture (pixels) and their true distance (mm).
    pub a: Option<[f32; 2]>,
    pub b: Option<[f32; 2]>,
    pub dist_mm: f64,
    tex: Option<egui::TextureHandle>,
}

impl Drawing {
    pub fn new(view: View, page: usize, rgba: Vec<u8>, size: [usize; 2], width_pt: f64) -> Drawing {
        Drawing { view, page, rgba, size, width_pt, a: None, b: None, dist_mm: 0.0, tex: None }
    }

    /// Millimetres per pixel, when two points and their distance are set.
    fn calibrated(&self) -> Option<f64> {
        let (a, b) = (self.a?, self.b?);
        let px = f64::from(((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt());
        (px > 2.0 && self.dist_mm > 0.0).then(|| self.dist_mm / px)
    }

    /// The picture's width in millimetres: from the two points, else as printed (data sheet drawings are rarely printed 1:1).
    pub fn width_mm(&self) -> f64 {
        match self.calibrated() {
            Some(mm_per_px) => mm_per_px * self.size[0] as f64,
            None => self.width_pt * 25.4 / 72.0,
        }
    }

    fn png(&self) -> Result<Vec<u8>, String> {
        let img = image::RgbaImage::from_raw(self.size[0] as u32, self.size[1] as u32, self.rgba.clone()).ok_or("絵の大きさが合いません")?;
        let mut out = std::io::Cursor::new(vec![]);
        img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        Ok(out.into_inner())
    }
}

/// The SolidCraft programs: (command line, app). `SOLIDCRAFT_DIR` names the folder; else the PATH; else the usual install folder.
pub fn find_solidcraft() -> Option<(PathBuf, PathBuf)> {
    let pair = |dir: &Path| {
        let (cli, app) = (dir.join("solidcraft-cli.exe"), dir.join("solidcraft.exe"));
        (cli.is_file() && app.is_file()).then_some((cli, app))
    };
    if let Some(d) = std::env::var_os("SOLIDCRAFT_DIR") {
        return pair(Path::new(&d));
    }
    if let Some(path) = std::env::var_os("PATH")
        && let Some(found) = std::env::split_paths(&path).find_map(|d| pair(&d))
    {
        return Some(found);
    }
    pair(Path::new(r"C:\Program Files\SolidCraft"))
}

/// What to tell someone without SolidCraft.
pub const SOLIDCRAFT_MISSING: &str = "SolidCraft が見つかりません。無料のオープンソースの CAD で、PowerShell で「winget install bherbruck.SolidCraft」と入れるか、github.com/bherbruck/solidcraft から入れてください。別の場所に入れたときは、環境変数 SOLIDCRAFT_DIR にそのフォルダを書きます。切り出した図 (PNG) は、ほかの CAD の下絵にも使えます";

/// A starting body: a box `w` (x) × `h` (y) × `height` (z) standing on the board's face, centred on the origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Starter {
    pub w: f64,
    pub h: f64,
    pub height: f64,
}

/// The SolidCraft command script: the drawings as canvases (the top view on the board's plane, the side view standing behind the part,
/// its picture's up along +z) and the starting body.
pub fn script(drawings: &[(View, PathBuf, f64)], starter: Option<Starter>) -> serde_json::Value {
    let mut cmds = vec![];
    // how far behind the part the side view stands: clear of the part and of the top view
    let behind = drawings.iter().filter(|d| d.0 == View::Top).map(|d| d.2 / 2.0).fold(starter.map_or(10.0, |s| s.h / 2.0 + 5.0), f64::max);
    if let Some(s) = starter {
        cmds.push(serde_json::json!({"command": "solid.box", "params": {"length": s.w, "width": s.h, "height": s.height, "corner": [-s.w / 2.0, -s.h / 2.0, 0.0], "body_name": "body"}}));
        cmds.push(serde_json::json!({"command": "appearance.assign", "params": {"bodies": ["body"], "color": "#3a3a3a", "opacity": 0.8}}));
    }
    for (view, path, width) in drawings {
        let path = path.display().to_string();
        cmds.push(match view {
            View::Top => serde_json::json!({"command": "canvas.insert", "params": {"path": path, "plane": "XY", "center": [0.0, 0.0], "width": width, "opacity": 0.6, "name": "上から見た図"}}),
            View::Side => serde_json::json!({"command": "canvas.insert", "params": {
                "path": path,
                "plane": {"origin": [0.0, behind, 0.0], "x_dir": [1.0, 0.0, 0.0], "y_dir": [0.0, 0.0, 1.0]},
                "center": [0.0, starter.map_or(width / 4.0, |s| s.height / 2.0)],
                "width": width, "opacity": 0.6, "name": "横から見た図"}}),
        });
    }
    serde_json::Value::Array(cmds)
}

/// The folder a part's SolidCraft work goes in (one per part name, under the system's temporary folder).
fn work_dir(name: &str) -> PathBuf {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    std::env::temp_dir().join("circuitcad_solidcraft").join(if safe.is_empty() { "part".to_string() } else { safe })
}

/// The STEP file the person is asked to export (and that the form watches for).
pub const STEP_NAME: &str = "part.step";

impl PartForm {
    /// Takes a drawing cut out of the data sheet.
    pub(super) fn take_pdf_drawing(&mut self) {
        if let Some(d) = self.sheet.take_drawing() {
            let view = d.view;
            self.sc_drawings.retain(|x| x.view != view);
            self.sc_drawings.push(d);
            self.sc_drawings.sort_by_key(|x| x.view == View::Side);
            self.sc_msg = Some(Ok(i18n::trf!("「{}」を切り出しました。図の中の寸法線の両端を 2 回クリックし、その長さ (mm) を入れると縮尺が合います", i18n::tr(view.label()))));
        }
    }

    /// Takes a drawing cut out just now (`--form-drawing`, before the first frame).
    pub fn take_drawing_now(&mut self) {
        self.take_pdf_drawing();
    }

    /// Writes the drawings and the script into the part's folder, makes the design with SolidCraft's command line and opens it.
    fn solidcraft_open(&mut self) -> Result<String, String> {
        let (cli, app) = find_solidcraft().ok_or(SOLIDCRAFT_MISSING)?;
        let dir = work_dir(&self.name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut drawn = vec![];
        for d in &self.sc_drawings {
            let p = dir.join(d.view.file());
            std::fs::write(&p, d.png()?).map_err(|e| format!("{}: {e}", p.display()))?;
            drawn.push((d.view, p, d.width_mm()));
        }
        let starter = (self.sc_starter && self.body_w > 0.0 && self.body_h > 0.0).then(|| Starter { w: self.body_w, h: self.body_h, height: if self.height > 0.0 { self.height } else { 1.0 } });
        let script_path = dir.join("script.json");
        std::fs::write(&script_path, serde_json::to_string_pretty(&script(&drawn, starter)).unwrap_or_default()).map_err(|e| e.to_string())?;
        let design = dir.join("part.sc");
        let out = std::process::Command::new(&cli).arg("run").arg(&script_path).arg("--save").arg(&design).arg("--quiet").output().map_err(|e| format!("{}: {e}", cli.display()))?;
        if !out.status.success() {
            return Err(format!("SolidCraft が下絵の設計を作れませんでした: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")));
        }
        std::process::Command::new(&app).arg(&design).spawn().map_err(|e| format!("{}: {e}", app.display()))?;
        self.sc_dir = Some(dir.clone());
        self.sc_seen = std::fs::metadata(dir.join(STEP_NAME)).and_then(|m| m.modified()).ok();
        Ok(i18n::trf!("SolidCraft を開きました。形を作ったら「ファイル → 書き出し → STEP」で、このフォルダに {STEP_NAME} という名前で保存してください。保存すると、ここに自動で読み込みます: {}", dir.display()))
    }

    /// Loads the STEP that SolidCraft saved into the part's folder, when it is new or has changed.
    pub(super) fn solidcraft_poll(&mut self, ctx: &egui::Context) {
        let Some(dir) = &self.sc_dir else { return };
        let path = dir.join(STEP_NAME);
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if modified.is_some() && modified != self.sc_seen {
            self.sc_seen = modified;
            self.step_load(&path);
            self.sc_msg = Some(if self.step_msg.is_empty() { Ok(i18n::trf!("SolidCraft の {STEP_NAME} を、3D の形に読み込みました。上から・横から見た図で向きを確かめてください")) } else { Err(self.step_msg.clone()) });
        }
        // look again in a while, without the person moving the mouse
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    /// The "SolidCraft で形を作る" panel inside the 3D section.
    pub(super) fn solidcraft_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(i18n::tr("STEP が無いとき: SolidCraft で形を作る")).id_salt("part_solidcraft").default_open(!self.sc_drawings.is_empty()).show(ui, |ui| {
            ui.label(egui::RichText::new(i18n::tr("データシートの外形図 (Package Outline・Dimensions) を切り出して、無料の CAD「SolidCraft」に下絵として渡します。SolidCraft で形を作って STEP で保存すると、ここに読み込みます")).weak());
            let has_doc = self.sheet.has_doc();
            ui.horizontal_wrapped(|ui| {
                for view in [View::Top, View::Side] {
                    if ui.add_enabled(has_doc, egui::Button::new(i18n::trf!("{} を切り出す", i18n::tr(view.label())))).on_hover_text(i18n::tr("データシートの窓で、外形図を囲むようにドラッグします。寸法の数字と線が入るように少し広めに")).on_disabled_hover_text(i18n::tr("先に「データシート (PDF) を開く…」でデータシートを開いてください")).clicked() {
                        self.sheet.start_drawing_cut(view);
                    }
                }
            });
            let mut remove = None;
            for (k, d) in self.sc_drawings.iter_mut().enumerate() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(i18n::tr(d.view.label())).strong());
                    ui.small(i18n::trf!("p.{}  幅 {:.1} mm{}", d.page + 1, d.width_mm(), if d.calibrated().is_some() { "" } else { " (縮尺は未設定: 印刷の大きさ)" }));
                    if ui.small_button(i18n::tr("消す")).on_hover_text(i18n::tr("この図を下絵から外します")).clicked() {
                        remove = Some(k);
                    }
                });
                drawing_calibrate_ui(ui, d);
            }
            if let Some(k) = remove {
                self.sc_drawings.remove(k);
            }
            ui.checkbox(&mut self.sc_starter, i18n::trf!("たたき台に本体の箱を入れる ({:.2} × {:.2} × 高さ {:.2} mm)", self.body_w, self.body_h, self.height))
                .on_hover_text(i18n::tr("「① 基本」「④ 部品の形」に入れた本体の幅・奥行き・高さで、半透明の箱を置きます。下絵と大きさを見比べたり、そのまま形を作り始めたりできます"));
            let found = find_solidcraft().is_some();
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.sc_drawings.is_empty() || self.sc_starter, egui::Button::new(i18n::tr("SolidCraft で開く"))).on_hover_text(i18n::tr("下絵とたたき台を入れた設計を作って、SolidCraft を開きます。作った形を STEP で保存すると、自動で読み込みます")).clicked() {
                    self.sc_msg = Some(self.solidcraft_open());
                }
                if !found {
                    ui.colored_label(crate::theme::WARN, i18n::tr("SolidCraft が入っていません")).on_hover_text(i18n::tr(SOLIDCRAFT_MISSING));
                }
                if let Some(dir) = self.sc_dir.clone()
                    && ui.small_button(i18n::tr("作業フォルダを開く")).on_hover_text(i18n::tr("下絵の PNG・SolidCraft の設計・保存した STEP が入っているフォルダを開きます")).clicked()
                {
                    let _ = std::process::Command::new("explorer").arg(&dir).spawn();
                }
            });
            match &self.sc_msg {
                Some(Ok(m)) => {
                    ui.colored_label(crate::theme::OK, m);
                }
                Some(Err(e)) => {
                    ui.colored_label(crate::theme::ERROR, e);
                }
                None => {}
            }
        });
        self.solidcraft_poll(ui.ctx());
    }
}

/// The cut-out picture, small, where two clicks set the scale.
fn drawing_calibrate_ui(ui: &mut egui::Ui, d: &mut Drawing) {
    let tex = d.tex.get_or_insert_with(|| ui.ctx().load_texture(format!("sc_drawing_{:?}", d.view), egui::ColorImage::from_rgba_unmultiplied(d.size, &d.rgba), egui::TextureOptions::LINEAR)).clone();
    let shown = (ui.available_width().min(320.0)) / d.size[0] as f32;
    let size = egui::vec2(d.size[0] as f32, d.size[1] as f32) * shown;
    let resp = ui.add(egui::Image::new((tex.id(), size)).sense(egui::Sense::click())).on_hover_text(i18n::tr("寸法の分かる線 (寸法線の両端の矢印など) の、片方の端をクリックし、もう片方の端をクリックします。3 回目のクリックでやり直します"));
    let to_screen = |p: [f32; 2]| resp.rect.min + egui::vec2(p[0], p[1]) * shown;
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let p = (pos - resp.rect.min) / shown;
        let p = [p.x, p.y];
        match (d.a, d.b) {
            (Some(_), None) => d.b = Some(p),
            _ => {
                d.a = Some(p);
                d.b = None;
            }
        }
    }
    let red = egui::Color32::from_rgb(230, 40, 40);
    for p in [d.a, d.b].into_iter().flatten() {
        ui.painter().circle_stroke(to_screen(p), 4.0, egui::Stroke::new(1.5, red));
    }
    if let (Some(a), Some(b)) = (d.a, d.b) {
        ui.painter().line_segment([to_screen(a), to_screen(b)], egui::Stroke::new(1.5, red));
    }
    ui.horizontal(|ui| {
        ui.label(i18n::tr("2 点の間の長さ"));
        ui.add_enabled(d.b.is_some(), egui::DragValue::new(&mut d.dist_mm).range(0.0..=500.0).speed(0.05).suffix(" mm")).on_hover_text(i18n::tr("クリックした 2 点の、本当の長さ (図の寸法の数字)。これで図の縮尺が決まります"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_puts_the_top_view_flat_and_the_side_view_standing_behind_the_part() {
        let s = script(&[(View::Top, PathBuf::from("top.png"), 12.0), (View::Side, PathBuf::from("side.png"), 12.0)], Some(Starter { w: 6.0, h: 4.0, height: 3.0 }));
        let cmds = s.as_array().unwrap();
        assert_eq!(cmds[0]["command"], "solid.box");
        assert_eq!(cmds[0]["params"]["corner"], serde_json::json!([-3.0, -2.0, 0.0]));
        let top = &cmds[2]["params"];
        assert_eq!((top["plane"].as_str(), top["width"].as_f64()), (Some("XY"), Some(12.0)));
        let side = &cmds[3]["params"];
        // standing up: its picture's up is +z, behind the part (half its 4 mm depth + 5 mm, more than half the 12 mm top view)
        assert_eq!(side["plane"]["y_dir"], serde_json::json!([0.0, 0.0, 1.0]));
        assert_eq!(side["plane"]["origin"][1].as_f64(), Some(7.0));
        assert_eq!(side["center"][1].as_f64(), Some(1.5));
    }

    /// When SolidCraft is installed, the script it is given makes a design with the two pictures and the body (skipped without it).
    #[test]
    fn solidcraft_takes_the_script() {
        let Some((cli, _)) = find_solidcraft() else { return };
        let dir = std::env::temp_dir().join("circuitcad_solidcraft_test");
        let _ = std::fs::create_dir_all(&dir);
        let d = Drawing::new(View::Top, 0, vec![200; 80 * 40 * 4], [80, 40], 60.0);
        let (top, side) = (dir.join("top.png"), dir.join("side.png"));
        std::fs::write(&top, d.png().unwrap()).unwrap();
        std::fs::write(&side, d.png().unwrap()).unwrap();
        let s = script(&[(View::Top, top, 20.0), (View::Side, side, 20.0)], Some(Starter { w: 6.0, h: 4.0, height: 3.0 }));
        let (sp, design) = (dir.join("script.json"), dir.join("part.sc"));
        std::fs::write(&sp, serde_json::to_string(&s).unwrap()).unwrap();
        let _ = std::fs::remove_file(&design);
        let out = std::process::Command::new(cli).arg("run").arg(&sp).arg("--save").arg(&design).arg("--quiet").output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        // the design is a zip whose document.json lists the canvases
        let bytes = std::fs::read(&design).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("document.json"), "a SolidCraft design");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_points_set_the_scale() {
        let mut d = Drawing::new(View::Top, 0, vec![0; 200 * 100 * 4], [200, 100], 72.0);
        // as printed: 72 points = 25.4 mm
        assert!((d.width_mm() - 25.4).abs() < 1e-9);
        // 50 pixels are 5 mm: the 200-pixel picture is 20 mm wide
        d.a = Some([10.0, 10.0]);
        d.b = Some([60.0, 10.0]);
        d.dist_mm = 5.0;
        assert!((d.width_mm() - 20.0).abs() < 1e-9);
        assert!(d.png().unwrap().starts_with(&[0x89, b'P', b'N', b'G']));
    }
}
