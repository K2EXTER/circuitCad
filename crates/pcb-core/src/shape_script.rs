//! Templates of common part bodies (DC jack, tactile switch, relay, screw terminal block) as SolidCraft command scripts
//! (`solidcraft-cli run script.json --out part.obj`). circuitCad has no CAD of its own (`docs/integration-policy.md`): a
//! template writes the commands, SolidCraft builds the solid with its rounds and holes, and the triangles come back as the
//! part's 3D shape (`Mesh3d`). Without SolidCraft the 3D view keeps the guessed shapes of `model3d.rs`.
//!
//! Everything is in the footprint's frame (millimetres, x right, y up, z up from the board face), so the mesh needs no moving:
//! the legs stand in the footprint's own through-holes and the body sits where the template's dimensions put it relative to
//! them. The numbers are the usual sizes of each family; give the data sheet's when they matter.

use crate::mesh::Mesh3d;
use crate::footprints::FpPad as Pad;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A body template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    DcJack,
    Tact,
    Relay,
    Terminal,
}

/// One dimension of a template: its key (`length=14.5` on the command line), what it is, the usual value.
#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub key: &'static str,
    pub label: &'static str,
    pub default: f64,
}

const fn p(key: &'static str, label: &'static str, default: f64) -> Param {
    Param { key, label, default }
}

const DC_JACK: &[Param] = &[
    p("length", "本体の長さ (口から後ろの端まで、mm)", 14.5),
    p("back", "ピン 1 から後ろの端まで (mm)", 0.8),
    p("width", "本体の幅 (mm)", 10.0),
    p("height", "高さ (基板の面から、mm)", 11.0),
    p("bore", "口の直径 (mm。外径 5.5 のプラグなら 6.4)", 6.4),
    p("pin", "中心ピンの直径 (mm。2.1 か 2.5 のプラグに合わせる)", 2.0),
    p("depth", "口の深さ (mm)", 10.3),
];
const TACT: &[Param] = &[
    p("size", "本体の一辺 (mm。6 か 12)", 6.0),
    p("body", "本体の高さ (金属の板の上まで、mm)", 3.5),
    p("height", "ボタンの上までの高さ (mm)", 4.3),
    p("button", "ボタンの直径 (mm)", 3.5),
];
const RELAY: &[Param] = &[
    p("length", "長さ (x、mm)", 19.0),
    p("width", "幅 (y、mm)", 15.5),
    p("height", "高さ (mm)", 15.3),
    p("blue", "青いケース (1) か黒 (0)", 1.0),
];
const TERMINAL: &[Param] = &[
    p("depth", "奥行き (電線を差す向き、mm)", 8.0),
    p("height", "高さ (mm)", 10.0),
    p("open", "電線の口の向き (+x なら 1、−x なら −1)", 1.0),
];

impl Template {
    pub const ALL: [Template; 4] = [Template::DcJack, Template::Tact, Template::Relay, Template::Terminal];

    /// The name on the command line (`ccad shape dc_jack`).
    pub fn key(self) -> &'static str {
        match self {
            Template::DcJack => "dc_jack",
            Template::Tact => "tact",
            Template::Relay => "relay",
            Template::Terminal => "terminal",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Template::DcJack => "DC ジャック (横向き、口は −x)",
            Template::Tact => "タクトスイッチ (スルーホール)",
            Template::Relay => "リレー (箱形)",
            Template::Terminal => "ねじ端子台",
        }
    }

    pub fn from_key(key: &str) -> Option<Template> {
        Template::ALL.into_iter().find(|t| t.key() == key)
    }

    pub fn params(self) -> &'static [Param] {
        match self {
            Template::DcJack => DC_JACK,
            Template::Tact => TACT,
            Template::Relay => RELAY,
            Template::Terminal => TERMINAL,
        }
    }

    /// The template that fits a footprint, by the same names the 3D view's guess uses.
    pub fn for_footprint(refdes: &str, footprint: &str) -> Option<Template> {
        use crate::model3d::{Kind, part_kind};
        let fp = footprint.to_ascii_lowercase();
        match part_kind(refdes, footprint) {
            Kind::Shell if fp.contains("jack") || fp.contains("barrel") => Some(Template::DcJack),
            Kind::Tact => Some(Template::Tact),
            Kind::Relay => Some(Template::Relay),
            Kind::Terminal => Some(Template::Terminal),
            _ => None,
        }
    }
}

/// The value of each of the template's dimensions: the given one, else the usual one. An unknown key is refused (a typo
/// would otherwise be ignored), and every value must be a finite number.
pub fn values(t: Template, given: &BTreeMap<String, f64>) -> Result<BTreeMap<&'static str, f64>, String> {
    if let Some(k) = given.keys().find(|k| !t.params().iter().any(|p| p.key == k.as_str())) {
        let keys: Vec<&str> = t.params().iter().map(|p| p.key).collect();
        return Err(format!("{} に「{k}」という寸法はありません。使えるのは {} です", t.label(), keys.join(" ")));
    }
    let mut v = BTreeMap::new();
    for p in t.params() {
        let x = given.get(p.key).copied().unwrap_or(p.default);
        if !x.is_finite() {
            return Err(format!("{} の値が数ではありません", p.key));
        }
        v.insert(p.key, x);
    }
    Ok(v)
}

/// The commands as they are built up.
struct Script {
    cmds: Vec<Value>,
}

const BLACK: &str = "#262628";
const METAL: &str = "#c4c6cc";

impl Script {
    fn push(&mut self, command: &str, params: Value) {
        self.cmds.push(json!({"command": command, "params": params}));
    }

    /// A box from `lo` to `hi`: a new body `body`, or joined to / cut from it.
    fn block(&mut self, name: &str, body: &str, op: &str, lo: [f64; 3], hi: [f64; 3]) {
        let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let center = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        let mut p = json!({"length": size[0], "width": size[1], "height": size[2], "center": center, "operation": op, "name": name});
        self.target(&mut p, body, op);
        self.push("solid.box", p);
    }

    /// A cylinder of radius `r` from `base` along `axis` ("X", "Y" or "Z") for `len`.
    fn cylinder(&mut self, name: &str, body: &str, op: &str, base: [f64; 3], axis: &str, r: f64, len: f64) {
        let mut p = json!({"radius": r, "height": len, "base": base, "axis": axis, "operation": op, "name": name});
        self.target(&mut p, body, op);
        self.push("solid.cylinder", p);
    }

    fn target(&self, p: &mut Value, body: &str, op: &str) {
        if op == "new" {
            p["body_name"] = json!(body);
        } else {
            p["targets"] = json!([body]);
        }
    }

    fn color(&mut self, bodies: &[&str], hex: &str, metal: bool) {
        let pbr = if metal { json!({"metallic": 1.0, "roughness": 0.3, "dielectric_ior": 1.5}) } else { json!({"metallic": 0.0, "roughness": 0.6, "dielectric_ior": 1.5}) };
        self.push("appearance.assign", json!({"bodies": bodies, "color": hex, "pbr": pbr}));
    }

    /// A leg in every through-hole of the footprint, from inside the body (`top`) down through the board: a flat blade in a slot,
    /// a square pin in a round hole. One body `Legs` (pieces need not touch).
    fn legs(&mut self, pads: &[Pad], top: f64) {
        let mut first = true;
        for (i, pad) in pads.iter().enumerate().filter(|(_, p)| p.drill.is_some()) {
            let drill = pad.drill.unwrap_or(1.0);
            let (w, h) = if pad.slot > drill {
                let long = (pad.slot - 0.4).max(drill * 0.6);
                let thin = (drill * 0.6).min(0.8);
                if pad.slot_v { (thin, long) } else { (long, thin) }
            } else {
                let s = (drill * 0.6).clamp(0.4, 1.2);
                (s, s)
            };
            let (x, y) = (pad.pos.x, pad.pos.y);
            self.block(&format!("Leg{}", i + 1), "Legs", if first { "new" } else { "join" }, [x - w / 2.0, y - h / 2.0, -LEG_BELOW], [x + w / 2.0, y + h / 2.0, top]);
            first = false;
        }
        if !first {
            self.color(&["Legs"], METAL, true);
        }
    }
}

/// How far the legs reach below the board face (through a 1.6 mm board and out the other side).
const LEG_BELOW: f64 = 3.5;

/// The middle of the pads and their extent.
fn pad_box(pads: &[Pad]) -> ([f64; 2], [f64; 2]) {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in pads {
        let (w, h) = p.shape.size();
        lo = [lo[0].min(p.pos.x - w / 2.0), lo[1].min(p.pos.y - h / 2.0)];
        hi = [hi[0].max(p.pos.x + w / 2.0), hi[1].max(p.pos.y + h / 2.0)];
    }
    (lo, hi)
}

/// The SolidCraft commands that build the template's body on the footprint's pads (`pads` in the footprint's frame).
pub fn script(t: Template, given: &BTreeMap<String, f64>, pads: &[Pad]) -> Result<Value, String> {
    if pads.is_empty() {
        return Err("パッドの無い部品には形を作れません".into());
    }
    let v = values(t, given)?;
    let positive = |keys: &[&str]| -> Result<(), String> {
        match keys.iter().find(|k| v[*k] <= 0.0) {
            Some(k) => Err(format!("{k} は 0 より大きくしてください")),
            None => Ok(()),
        }
    };
    let mut s = Script { cmds: vec![] };
    match t {
        Template::DcJack => {
            positive(&["length", "width", "height", "bore", "pin", "depth"])?;
            let (len, w, h, bore, pin, depth) = (v["length"], v["width"], v["height"], v["bore"], v["pin"], v["depth"]);
            let r = w / 2.0;
            if h < r + 1.3 || bore >= w - 1.0 || pin >= bore || depth >= len - 1.0 {
                return Err("寸法が合いません: 高さは幅の半分 + 1 mm 以上、口は幅より 1 mm 以上小さく、ピンは口より細く、口の深さは長さより 1 mm 以上短くしてください".into());
            }
            // the axis runs along x through pin 1 (the centre pin's leg, at the back); the opening is towards −x
            let p1 = pads.iter().find(|p| p.pin == "1").unwrap_or(&pads[0]);
            // `height` is the top of the bezel, which stands 0.3 mm proud of the body
            let (back, y, zc, bezel) = (p1.pos.x + v["back"], p1.pos.y, h - r - 0.3, 0.8);
            let front = back - len;
            s.block("Base", "Housing", "new", [front + bezel, y - r, 0.0], [back, y + r, zc]);
            s.cylinder("Top", "Housing", "join", [front + bezel, y, zc], "X", r, len - bezel);
            s.block("BezelBase", "Housing", "join", [front, y - r - 0.3, 0.0], [front + bezel, y + r + 0.3, zc]);
            s.cylinder("BezelTop", "Housing", "join", [front, y, zc], "X", r + 0.3, bezel);
            s.cylinder("Mouth", "Housing", "cut", [front - 0.1, y, zc], "X", bore / 2.0, depth + 0.1);
            s.push("solid.fillet", json!({"edges": [[back, y + r, zc / 2.0], [back, y - r, zc / 2.0]], "radius": 1.0, "body": "Housing", "name": "BackCorners"}));
            s.push("solid.chamfer", json!({"edges": [[front, y + bore / 2.0, zc]], "distance": 0.4, "body": "Housing", "name": "MouthRim"}));
            s.color(&["Housing"], BLACK, false);
            s.cylinder("CentrePin", "Pin", "new", [front + depth * 0.25, y, zc], "X", pin / 2.0, depth * 0.75);
            s.color(&["Pin"], METAL, true);
            s.legs(pads, 0.5);
        }
        Template::Tact => {
            positive(&["size", "body", "height", "button"])?;
            let (size, body, h, button) = (v["size"], v["body"], v["height"], v["button"]);
            if h <= body || button >= size {
                return Err("寸法が合いません: ボタンの上は本体より高く、ボタンは本体より細くしてください".into());
            }
            let (lo, hi) = pad_box(pads);
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            let q = size / 2.0;
            let plate = 0.3;
            s.block("Body", "Body", "new", [c[0] - q, c[1] - q, 0.0], [c[0] + q, c[1] + q, body - plate]);
            s.push("solid.chamfer", json!({"edges": [[c[0] - q, c[1] - q, (body - plate) / 2.0], [c[0] + q, c[1] - q, (body - plate) / 2.0], [c[0] - q, c[1] + q, (body - plate) / 2.0], [c[0] + q, c[1] + q, (body - plate) / 2.0]], "distance": 0.3, "body": "Body", "name": "Corners"}));
            s.color(&["Body"], BLACK, false);
            s.block("Plate", "Plate", "new", [c[0] - q, c[1] - q, body - plate], [c[0] + q, c[1] + q, body]);
            s.color(&["Plate"], METAL, true);
            s.cylinder("Button", "Button", "new", [c[0], c[1], body], "Z", button / 2.0, h - body);
            s.push("solid.fillet", json!({"edges": [[c[0] + button / 2.0, c[1], h]], "radius": (button * 0.1).min(0.4), "body": "Button", "name": "ButtonEdge"}));
            s.color(&["Button"], BLACK, false);
            // the legs come out of the sides of the body: a tab from the body to each leg standing outside it
            s.legs(pads, 0.6);
            for (i, pad) in pads.iter().enumerate().filter(|(_, p)| p.drill.is_some()) {
                let (x, y) = (pad.pos.x, pad.pos.y);
                let dx = (x - c[0]).abs() - q;
                if dx > 0.05 {
                    let (a, b) = if x < c[0] { (x, c[0] - q + 0.2) } else { (c[0] + q - 0.2, x) };
                    s.block(&format!("Tab{}", i + 1), "Legs", "join", [a, y - 0.35, 0.3], [b, y + 0.35, 0.6]);
                }
            }
        }
        Template::Relay => {
            positive(&["length", "width", "height"])?;
            let (l, w, h) = (v["length"], v["width"], v["height"]);
            let (lo, hi) = pad_box(pads);
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            if l < hi[0] - lo[0] || w < hi[1] - lo[1] {
                return Err(format!("リレーの箱 ({l} × {w} mm) がパッドの範囲 ({:.1} × {:.1} mm) より小さいです", hi[0] - lo[0], hi[1] - lo[1]));
            }
            let (a, b) = ([c[0] - l / 2.0, c[1] - w / 2.0], [c[0] + l / 2.0, c[1] + w / 2.0]);
            s.block("Case", "Case", "new", [a[0], a[1], 0.0], [b[0], b[1], h]);
            let r = 0.8;
            let mid = |x: f64, y: f64| json!([x, y, h / 2.0]);
            s.push("solid.fillet", json!({"edges": [mid(a[0], a[1]), mid(b[0], a[1]), mid(a[0], b[1]), mid(b[0], b[1])], "radius": r, "body": "Case", "name": "Corners"}));
            s.color(&["Case"], if v["blue"] >= 0.5 { "#2850aa" } else { BLACK }, false);
            // the printed label on top
            let (iw, ih) = (l * 0.2, w * 0.25);
            s.block("Label", "Label", "new", [a[0] + iw, a[1] + ih, h], [b[0] - iw, b[1] - ih, h + 0.05]);
            s.color(&["Label"], "#e2e2e6", false);
            s.legs(pads, 0.5);
        }
        Template::Terminal => {
            positive(&["depth", "height"])?;
            let (d, h, open) = (v["depth"], v["height"], if v["open"] < 0.0 { -1.0 } else { 1.0 });
            // the poles stand in a row along y (as the single-row footprints): the block across them
            let holes: Vec<&Pad> = pads.iter().filter(|p| p.drill.is_some()).collect();
            if holes.is_empty() {
                return Err("端子台の形は、穴のあるパッドに作ります".into());
            }
            let x = holes.iter().map(|p| p.pos.x).sum::<f64>() / holes.len() as f64;
            let ys: Vec<f64> = holes.iter().map(|p| p.pos.y).collect();
            let pitch = ys.windows(2).map(|w| (w[0] - w[1]).abs()).filter(|g| *g > 0.1).fold(f64::MAX, f64::min);
            let pitch = if pitch == f64::MAX { 5.08 } else { pitch };
            let (y0, y1) = (ys.iter().cloned().fold(f64::MAX, f64::min) - pitch / 2.0, ys.iter().cloned().fold(f64::MIN, f64::max) + pitch / 2.0);
            s.block("Block", "Block", "new", [x - d / 2.0, y0, 0.0], [x + d / 2.0, y1, h]);
            let hole_r = (pitch * 0.32).min(1.8);
            let wire = (pitch * 0.6).min(3.2);
            for (i, y) in ys.iter().enumerate() {
                // the screw well from the top and the wire's opening in the side
                s.cylinder(&format!("Well{}", i + 1), "Block", "cut", [x, *y, h - 2.5], "Z", hole_r, 2.6);
                let face = x + open * d / 2.0;
                let (a, b) = if open > 0.0 { (face - d * 0.55, face + 0.1) } else { (face - 0.1, face + d * 0.55) };
                s.block(&format!("Wire{}", i + 1), "Block", "cut", [a, y - wire / 2.0, h * 0.25], [b, y + wire / 2.0, h * 0.25 + wire]);
            }
            s.color(&["Block"], "#1e6e3c", false);
            for (i, y) in ys.iter().enumerate() {
                let body = format!("Screw{}", i + 1);
                s.cylinder(&body, &body, "new", [x, *y, h - 2.5], "Z", hole_r - 0.1, 1.7);
                s.block(&format!("Slot{}", i + 1), &format!("Slot{}", i + 1), "new", [x - (hole_r - 0.3), y - 0.25, h - 0.8], [x + (hole_r - 0.3), y + 0.25, h - 0.75]);
            }
            let screws: Vec<String> = (1..=ys.len()).map(|i| format!("Screw{i}")).collect();
            s.color(&screws.iter().map(String::as_str).collect::<Vec<_>>(), METAL, true);
            let slots: Vec<String> = (1..=ys.len()).map(|i| format!("Slot{i}")).collect();
            s.color(&slots.iter().map(String::as_str).collect::<Vec<_>>(), "#141416", false);
            s.legs(pads, 0.5);
        }
    }
    Ok(Value::Array(s.cmds))
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

/// Runs a script with SolidCraft's command line (`cli`) and reads the body back. SolidCraft writes it both as a STEP (kept, for a
/// case CAD) and as triangles; the triangles come from our own STEP reader, which cuts a cylinder into a few dozen facets (a DC
/// jack is about 430 triangles, 20 kB in the part file, where SolidCraft's own fine cut is 8 500 and 280 kB: the 3D view's
/// smooth shading hides the difference). SolidCraft's triangles (thinned to `KEEP_TRIANGLES`) are the fallback when our reader
/// cannot read the STEP. The script, the STEP and the OBJ stay in `dir` (`shape.json` can be opened in SolidCraft and changed).
pub fn build(cli: &Path, script: &Value, dir: &Path) -> Result<Mesh3d, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (file, step, obj) = (dir.join("shape.json"), dir.join("shape.step"), dir.join("shape.obj"));
    std::fs::write(&file, serde_json::to_string_pretty(script).unwrap_or_default()).map_err(|e| format!("{}: {e}", file.display()))?;
    let _ = std::fs::remove_file(&step);
    let _ = std::fs::remove_file(&obj);
    let out = std::process::Command::new(cli).arg("run").arg(&file).arg("--out").arg(&step).arg("--out").arg(&obj).arg("--quality").arg("standard").arg("--quiet").output().map_err(|e| format!("{}: {e}", cli.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("SolidCraft が形を作れませんでした: {}", err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("")));
    }
    if let Ok(m) = std::fs::read(&step).map_err(|e| e.to_string()).and_then(|b| crate::step::read_step(&b))
        && !m.triangles.is_empty()
    {
        return Ok(crate::mesh::thin_out(m, crate::mesh::KEEP_TRIANGLES));
    }
    let colors = std::fs::read_to_string(obj.with_extension("mtl")).map(|t| crate::mesh_io::read_mtl(&t)).unwrap_or_default();
    let text = std::fs::read_to_string(&obj).map_err(|e| format!("{}: {e}", obj.display()))?;
    crate::mesh_io::read_obj(&text, &colors).map(|m| crate::mesh::thin_out(m, crate::mesh::KEEP_TRIANGLES))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PadShape, Point};

    fn hole(pin: &str, x: f64, y: f64, d: f64) -> Pad {
        Pad { pin: pin.into(), pos: Point::new(x, y), shape: PadShape::Circle { d: d + 0.8 }, drill: Some(d), slot: 0.0, slot_v: false }
    }

    fn commands(v: &Value) -> Vec<&str> {
        v.as_array().unwrap().iter().map(|c| c["command"].as_str().unwrap()).collect()
    }

    /// Every template writes a script with its body, its colours and a leg in each hole; the jack's opening is on −x of pin 1.
    #[test]
    fn templates_write_a_body_colours_and_legs() {
        let jack = [hole("1", 0.0, 0.0, 1.0), hole("2", -6.0, 0.0, 1.0), hole("3", -3.0, 4.7, 1.0)];
        let tact = [hole("1", 0.0, 0.0, 1.0), hole("2", 6.5, 0.0, 1.0), hole("3", 0.0, 4.5, 1.0), hole("4", 6.5, 4.5, 1.0)];
        let relay = [hole("1", 0.0, 0.0, 1.3), hole("2", 12.0, 0.0, 1.3), hole("3", 0.0, 12.0, 1.3), hole("4", 12.0, 12.0, 1.3), hole("5", 2.0, 6.0, 1.3)];
        let term: Vec<Pad> = (0..3).map(|i| hole(&(i + 1).to_string(), 0.0, -5.08 * i as f64, 1.4)).collect();
        for (t, pads) in [(Template::DcJack, &jack[..]), (Template::Tact, &tact[..]), (Template::Relay, &relay[..]), (Template::Terminal, &term[..])] {
            let s = script(t, &BTreeMap::new(), pads).unwrap();
            let c = commands(&s);
            assert!(c.contains(&"appearance.assign"), "{t:?} colours its bodies");
            let legs = s.as_array().unwrap().iter().filter(|x| x["params"]["name"].as_str().is_some_and(|n| n.starts_with("Leg"))).count();
            assert_eq!(legs, pads.len(), "{t:?}: a leg in every hole");
        }
        // the jack: the opening is cut along x from in front of pin 1, 11 mm tall by default
        let s = script(Template::DcJack, &BTreeMap::new(), &jack).unwrap();
        let mouth = s.as_array().unwrap().iter().find(|x| x["params"]["name"] == "Mouth").unwrap();
        assert_eq!(mouth["params"]["axis"], "X");
        assert!((mouth["params"]["base"][0].as_f64().unwrap() - (0.8 - 14.5 - 0.1)).abs() < 1e-9);
        // the terminal block: a well and a wire opening per pole
        let s = script(Template::Terminal, &BTreeMap::new(), &term).unwrap();
        assert_eq!(s.as_array().unwrap().iter().filter(|x| x["params"]["operation"] == "cut").count(), 6);
    }

    /// A misspelt dimension, sizes that do not fit and a part without pads are refused with the reason.
    #[test]
    fn wrong_dimensions_are_refused() {
        let jack = [hole("1", 0.0, 0.0, 1.0)];
        let typo: BTreeMap<String, f64> = [("lenght".to_string(), 12.0)].into();
        assert!(script(Template::DcJack, &typo, &jack).unwrap_err().contains("lenght"));
        let small: BTreeMap<String, f64> = [("bore".to_string(), 12.0)].into();
        assert!(script(Template::DcJack, &small, &jack).is_err());
        assert!(script(Template::Tact, &BTreeMap::new(), &[]).is_err());
        let tiny: BTreeMap<String, f64> = [("length".to_string(), 5.0)].into();
        let relay = [hole("1", 0.0, 0.0, 1.3), hole("2", 12.0, 0.0, 1.3)];
        assert!(script(Template::Relay, &tiny, &relay).unwrap_err().contains("パッド"));
        assert_eq!(Template::for_footprint("J1", "DCJack_DEMO_TH"), Some(Template::DcJack));
        assert_eq!(Template::for_footprint("J1", "USB_DEMO_TH"), None, "no template for a USB socket");
        assert_eq!(Template::for_footprint("V1", "TerminalBlock_4P_5.08"), Some(Template::Terminal));
    }

    /// Builds every template with SolidCraft (`--ignored`: needs SolidCraft installed): each comes back as a few hundred
    /// coloured triangles, as tall as its dimensions say, with the STEP kept beside the script.
    #[test]
    #[ignore]
    fn every_template_builds_in_solidcraft() {
        let Some((cli, _)) = find_solidcraft() else { panic!("SolidCraft is not installed") };
        let jack = [hole("1", 0.0, 0.0, 1.0), hole("2", -6.0, 0.0, 1.0), hole("3", -3.0, 4.7, 1.0)];
        let tact = [hole("1", 0.0, 0.0, 1.0), hole("2", 6.5, 0.0, 1.0), hole("3", 0.0, 4.5, 1.0), hole("4", 6.5, 4.5, 1.0)];
        let relay = [hole("1", 0.0, 0.0, 1.3), hole("2", 12.0, 0.0, 1.3), hole("3", 0.0, 12.0, 1.3), hole("4", 12.0, 12.0, 1.3), hole("5", 2.0, 6.0, 1.3)];
        let term: Vec<Pad> = (0..4).map(|i| hole(&(i + 1).to_string(), 0.0, -5.08 * i as f64, 1.4)).collect();
        for (t, pads, top) in [(Template::DcJack, &jack[..], 11.0), (Template::Tact, &tact[..], 4.3), (Template::Relay, &relay[..], 15.35), (Template::Terminal, &term[..], 10.0)] {
            let dir = std::env::temp_dir().join("circuitcad_shape_test").join(t.key());
            let m = build(&cli, &script(t, &BTreeMap::new(), pads).unwrap(), &dir).unwrap_or_else(|e| panic!("{t:?}: {e}"));
            let zmax = m.vertices.iter().map(|v| v[2]).fold(f32::MIN, f32::max);
            let zmin = m.vertices.iter().map(|v| v[2]).fold(f32::MAX, f32::min);
            println!("{t:?}: {} triangles, {} colours, z {zmin}..{zmax}", m.triangles.len(), m.palette.len());
            assert!(!m.triangles.is_empty() && m.triangles.len() <= 2_000, "{t:?}: {} triangles (our STEP reader, not SolidCraft's fine cut)", m.triangles.len());
            assert!(dir.join("shape.step").is_file(), "{t:?}: the STEP is kept");
            assert!(m.palette.len() >= 2, "{t:?}: body and legs in their colours");
            assert!((f64::from(zmax) - top).abs() < 0.05 && (f64::from(zmin) + LEG_BELOW).abs() < 0.05, "{t:?}: z {zmin}..{zmax}");
        }
    }
}

