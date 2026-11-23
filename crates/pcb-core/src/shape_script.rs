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
    Elcap,
    Header,
    UsbA,
    UsbC,
    BatteryAa,
    Fuse5x20,
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

// The usual dimensions below are those of common parts' data sheets: Panasonic FR 6.3 x 11 (electrolytic), a 2.54 mm header (2.5 mm
// plastic, pins 6 mm above it and 3 mm below the board), Molex 67643 (USB-A, 14 x 13.1 x 5.12) and GCT USB4105 (USB-C, 7.35 x 8.94 x
// 3.26), Keystone 2460 (one AA cell, 57.2 x 16.3 x 15.0) and a pair of Littelfuse 5 x 20 mm clips on a 22.5 mm pitch.
const ELCAP: &[Param] = &[
    p("diameter", "直径 (mm。6.3 × 11 なら 6.3)", 6.3),
    p("height", "高さ (基板の面から、mm)", 11.0),
];
const HEADER: &[Param] = &[
    p("body", "黒い樹脂の高さ (mm)", 2.5),
    p("above", "樹脂から上に出るピンの長さ (mm)", 6.0),
    p("below", "基板の裏から下に出るピンの長さ (mm)", 3.0),
    p("pin", "ピンの太さ (角、mm)", 0.64),
];
const USB_A: &[Param] = &[
    p("length", "奥行き (口から後ろの端まで、mm)", 14.0),
    p("width", "幅 (mm)", 13.1),
    p("height", "高さ (基板の面から、mm)", 5.12),
    p("open", "口の向き (+x なら 1、−x なら −1、+y なら 2、−y なら −2)", -1.0),
];
const USB_C: &[Param] = &[
    p("length", "奥行き (口から後ろの端まで、mm)", 7.35),
    p("width", "幅 (mm)", 8.94),
    p("height", "高さ (基板の面から、mm)", 3.26),
    p("open", "口の向き (+x なら 1、−x なら −1、+y なら 2、−y なら −2)", -1.0),
];
const BATTERY_AA: &[Param] = &[
    p("length", "ホルダの長さ (電池の向き、mm)", 57.2),
    p("width", "ホルダの幅 (mm)", 16.3),
    p("height", "ホルダの高さ (mm)", 15.0),
    p("cell", "電池の直径 (mm。単 3 は 14.5)", 14.5),
    p("cell_length", "電池の長さ (mm。単 3 は 50.5)", 50.5),
];
const FUSE_5X20: &[Param] = &[
    p("clip_height", "クリップの高さ (基板の面から、mm)", 9.0),
    p("clip_width", "クリップの幅 (mm)", 6.5),
    p("fuse", "ヒューズの長さ (mm)", 20.0),
];

impl Template {
    pub const ALL: [Template; 10] = [Template::DcJack, Template::Tact, Template::Relay, Template::Terminal, Template::Elcap, Template::Header, Template::UsbA, Template::UsbC, Template::BatteryAa, Template::Fuse5x20];

    /// The name on the command line (`ccad shape dc_jack`).
    pub fn key(self) -> &'static str {
        match self {
            Template::DcJack => "dc_jack",
            Template::Tact => "tact",
            Template::Relay => "relay",
            Template::Terminal => "terminal",
            Template::Elcap => "elcap",
            Template::Header => "header",
            Template::UsbA => "usb_a",
            Template::UsbC => "usb_c",
            Template::BatteryAa => "battery_aa",
            Template::Fuse5x20 => "fuse_5x20",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Template::DcJack => "DC ジャック (横向き、口は −x)",
            Template::Tact => "タクトスイッチ (スルーホール)",
            Template::Relay => "リレー (箱形)",
            Template::Terminal => "ねじ端子台",
            Template::Elcap => "電解コンデンサ (縦置き)",
            Template::Header => "ピンヘッダ (2.54 mm)",
            Template::UsbA => "USB-A のメス (横向き)",
            Template::UsbC => "USB-C のメス (横向き)",
            Template::BatteryAa => "単 3 電池のホルダ (1 本)",
            Template::Fuse5x20 => "5 × 20 mm ヒューズのクリップ",
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
            Template::Elcap => ELCAP,
            Template::Header => HEADER,
            Template::UsbA => USB_A,
            Template::UsbC => USB_C,
            Template::BatteryAa => BATTERY_AA,
            Template::Fuse5x20 => FUSE_5X20,
        }
    }

    /// The template that fits a footprint, by the same names the 3D view's guess uses.
    pub fn for_footprint(refdes: &str, footprint: &str) -> Option<Template> {
        use crate::model3d::{Kind, part_kind};
        let fp = footprint.to_ascii_lowercase();
        match part_kind(refdes, footprint) {
            Kind::Shell if fp.contains("jack") || fp.contains("barrel") => Some(Template::DcJack),
            Kind::Shell if fp.contains("usb_c") || fp.contains("type_c") || fp.contains("typec") => Some(Template::UsbC),
            Kind::Shell if fp.contains("usb") && !fp.contains("micro") && !fp.contains("mini") => Some(Template::UsbA),
            Kind::Tact => Some(Template::Tact),
            Kind::Relay => Some(Template::Relay),
            Kind::Terminal => Some(Template::Terminal),
            Kind::Header => Some(Template::Header),
            Kind::Can if fp.contains("cp_") || fp.contains("elec") || fp.contains("radial") => Some(Template::Elcap),
            Kind::BatteryTray if !fp.contains("aaa") && !fp.contains("18650") => Some(Template::BatteryAa),
            Kind::FuseHolder => Some(Template::Fuse5x20),
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
pub(crate) struct Script {
    pub(crate) cmds: Vec<Value>,
}

const BLACK: &str = "#262628";
const METAL: &str = "#c4c6cc";

impl Script {
    pub(crate) fn push(&mut self, command: &str, params: Value) {
        self.cmds.push(json!({"command": command, "params": params}));
    }

    /// A box from `lo` to `hi`: a new body `body`, or joined to / cut from it.
    pub(crate) fn block(&mut self, name: &str, body: &str, op: &str, lo: [f64; 3], hi: [f64; 3]) {
        let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let center = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        let mut p = json!({"length": size[0], "width": size[1], "height": size[2], "center": center, "operation": op, "name": name});
        self.target(&mut p, body, op);
        self.push("solid.box", p);
    }

    /// A cylinder of radius `r` from `base` along `axis` ("X", "Y" or "Z") for `len`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cylinder(&mut self, name: &str, body: &str, op: &str, base: [f64; 3], axis: &str, r: f64, len: f64) {
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

    pub(crate) fn color(&mut self, bodies: &[&str], hex: &str, metal: bool) {
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
        Template::Elcap => {
            positive(&["diameter", "height"])?;
            let (d, h) = (v["diameter"], v["height"]);
            let (lo, hi) = pad_box(pads);
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            if d < (hi[0] - lo[0]).max(hi[1] - lo[1]) || h < 2.0 {
                return Err("寸法が合いません: 直径は足の間隔より大きく、高さは 2 mm 以上にしてください".into());
            }
            let r = d / 2.0;
            // the can standing a little off the board (the rubber seal), its top edge rounded, the vent cross pressed in the top
            s.cylinder("Can", "Can", "new", [c[0], c[1], 0.3], "Z", r, h - 0.3);
            s.push("solid.fillet", json!({"edges": [[c[0] + r, c[1], h]], "radius": (d * 0.06).min(0.5), "body": "Can", "name": "TopEdge"}));
            s.block("VentX", "Can", "cut", [c[0] - r * 0.6, c[1] - 0.15, h - 0.25], [c[0] + r * 0.6, c[1] + 0.15, h + 0.1]);
            s.block("VentY", "Can", "cut", [c[0] - 0.15, c[1] - r * 0.6, h - 0.25], [c[0] + 0.15, c[1] + r * 0.6, h + 0.1]);
            s.color(&["Can"], "#1f2f6e", false);
            // the minus stripe on the side of pin 2 (the minus pin of a radial electrolytic)
            let minus = pads.iter().find(|p| p.pin == "2").unwrap_or(&pads[pads.len() - 1]);
            let (dx, dy) = (minus.pos.x - c[0], minus.pos.y - c[1]);
            let (a, b) = if dx.abs() >= dy.abs() {
                let x = c[0] + dx.signum() * r;
                ([x - 0.05, c[1] - r * 0.35, 1.0], [x + 0.05, c[1] + r * 0.35, h - 0.8])
            } else {
                let y = c[1] + dy.signum() * r;
                ([c[0] - r * 0.35, y - 0.05, 1.0], [c[0] + r * 0.35, y + 0.05, h - 0.8])
            };
            s.block("Stripe", "Stripe", "new", a, b);
            s.color(&["Stripe"], "#c8ccd6", false);
            s.legs(pads, 0.5);
        }
        Template::Header => {
            positive(&["body", "above", "below", "pin"])?;
            let (body, above, below, pin) = (v["body"], v["above"], v["below"], v["pin"]);
            let holes: Vec<&Pad> = pads.iter().filter(|p| p.drill.is_some()).collect();
            if holes.is_empty() {
                return Err("ピンヘッダの形は、穴のあるパッドに作ります".into());
            }
            // the pitch: the nearest two holes (2.54 when there is one)
            let mut pitch = f64::MAX;
            for (i, a) in holes.iter().enumerate() {
                for b in &holes[i + 1..] {
                    pitch = pitch.min(a.pos.dist(b.pos));
                }
            }
            let pitch = if pitch == f64::MAX { 2.54 } else { pitch };
            let q = pitch / 2.0;
            for (i, h) in holes.iter().enumerate() {
                let (x, y) = (h.pos.x, h.pos.y);
                s.block(&format!("Plastic{}", i + 1), "Plastic", if i == 0 { "new" } else { "join" }, [x - q, y - q, 0.0], [x + q, y + q, body]);
                s.block(&format!("Pin{}", i + 1), "Pins", if i == 0 { "new" } else { "join" }, [x - pin / 2.0, y - pin / 2.0, -below], [x + pin / 2.0, y + pin / 2.0, body + above]);
            }
            s.color(&["Plastic"], BLACK, false);
            s.color(&["Pins"], "#d4af37", true);
        }
        Template::UsbA | Template::UsbC => {
            positive(&["length", "width", "height"])?;
            let (len, w, h) = (v["length"], v["width"], v["height"]);
            let (o, across) = mouth_axes(v["open"])?;
            let (lo, hi) = pad_box(pads);
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            // the back of the shell over the far end of the pads, the mouth `len` towards `o`
            let back_u = pads.iter().map(|p| (p.pos.x - c[0]) * o[0] + (p.pos.y - c[1]) * o[1]).fold(f64::MAX, f64::min) - 1.0;
            let at = |u: f64, v: f64| [c[0] + u * o[0] + v * across[0], c[1] + u * o[1] + v * across[1]];
            let span = |u0: f64, u1: f64, v0: f64, v1: f64, z0: f64, z1: f64| {
                let (a, b) = (at(u0, v0), at(u1, v1));
                ([a[0].min(b[0]), a[1].min(b[1]), z0], [a[0].max(b[0]), a[1].max(b[1]), z1])
            };
            let front = back_u + len;
            let wall = if t == Template::UsbA { 0.3 } else { 0.25 };
            let (a, b) = span(back_u, front, -w / 2.0, w / 2.0, 0.0, h);
            s.block("Shell", "Shell", "new", a, b);
            if t == Template::UsbC {
                // the C plug's rounded sides: the shell's four edges along the mouth
                let e = |v: f64, z: f64| { let p = at((back_u + front) / 2.0, v); json!([p[0], p[1], z]) };
                s.push("solid.fillet", json!({"edges": [e(-w / 2.0, 0.0), e(w / 2.0, 0.0), e(-w / 2.0, h), e(w / 2.0, h)], "radius": h / 2.0 - 0.05, "body": "Shell", "name": "Round"}));
            }
            let (a, b) = span(back_u + wall + 0.5, front + 0.1, -w / 2.0 + wall, w / 2.0 - wall, wall, h - wall);
            s.block("Mouth", "Shell", "cut", a, b);
            s.color(&["Shell"], METAL, true);
            // the tongue inside, with the contacts on it
            let (tw, tt) = if t == Template::UsbA { (w * 0.8, 1.8) } else { (w * 0.62, 0.7) };
            let zt = if t == Template::UsbA { h * 0.62 } else { h / 2.0 };
            let (a, b) = span(back_u + wall + 0.4, front - 0.6, -tw / 2.0, tw / 2.0, zt - tt / 2.0, zt + tt / 2.0);
            s.block("Tongue", "Tongue", "new", a, b);
            s.color(&["Tongue"], if t == Template::UsbA { "#f2f2f2" } else { BLACK }, false);
            s.legs(pads, 0.5);
        }
        Template::BatteryAa => {
            positive(&["length", "width", "height", "cell", "cell_length"])?;
            let (len, w, h, d, cl) = (v["length"], v["width"], v["height"], v["cell"], v["cell_length"]);
            if d >= w || cl >= len || h < d * 0.6 {
                return Err("寸法が合いません: 電池はホルダより細く短く、ホルダの高さは電池の直径の 6 割以上にしてください".into());
            }
            // the cell lies along the line through the two contacts (the farthest two pads)
            let (mut a, mut b, mut far) = (&pads[0], &pads[0], 0.0);
            for (i, p) in pads.iter().enumerate() {
                for q in &pads[i + 1..] {
                    if p.pos.dist(q.pos) > far {
                        (a, b, far) = (p, q, p.pos.dist(q.pos));
                    }
                }
            }
            let along_x = (b.pos.x - a.pos.x).abs() >= (b.pos.y - a.pos.y).abs();
            let c = [(a.pos.x + b.pos.x) / 2.0, (a.pos.y + b.pos.y) / 2.0];
            let bx = |u0: f64, u1: f64, v0: f64, v1: f64, z0: f64, z1: f64| if along_x { ([c[0] + u0, c[1] + v0, z0], [c[0] + u1, c[1] + v1, z1]) } else { ([c[0] + v0, c[1] + u0, z0], [c[0] + v1, c[1] + u1, z1]) };
            let axis = if along_x { "X" } else { "Y" };
            let base = |u: f64, z: f64| if along_x { [c[0] + u, c[1], z] } else { [c[0], c[1] + u, z] };
            // the cell rests 1.5 mm above the board on the tray's floor
            let zc = 1.5 + d / 2.0;
            let (p0, p1) = bx(-len / 2.0, len / 2.0, -w / 2.0, w / 2.0, 0.0, h);
            s.block("Tray", "Tray", "new", p0, p1);
            // the cell's bed and the open top over it
            s.cylinder("Bed", "Tray", "cut", base(-cl / 2.0 - 0.5, zc), axis, d / 2.0 + 0.3, cl + 1.0);
            let (p0, p1) = bx(-cl / 2.0 - 0.5, cl / 2.0 + 0.5, -d * 0.42, d * 0.42, zc, h + 0.1);
            s.block("Open", "Tray", "cut", p0, p1);
            s.color(&["Tray"], BLACK, false);
            // the cell: its can, and the plus nub at the end towards the first contact
            s.cylinder("Cell", "Cell", "new", base(-cl / 2.0 + 1.0, zc), axis, d / 2.0, cl - 1.0);
            s.color(&["Cell"], "#2d7d46", false);
            s.cylinder("Nub", "Nub", "new", base(-cl / 2.0, zc), axis, d * 0.19, 1.0);
            s.color(&["Nub"], METAL, true);
            s.legs(pads, 1.0);
        }
        Template::Fuse5x20 => {
            positive(&["clip_height", "clip_width", "fuse"])?;
            let (ch, cw, fl) = (v["clip_height"], v["clip_width"], v["fuse"]);
            if ch < 6.5 {
                return Err("クリップの高さは 6.5 mm 以上にしてください (5 mm のヒューズが浮く高さ)".into());
            }
            let (lo, hi) = pad_box(pads);
            let along_x = hi[0] - lo[0] >= hi[1] - lo[1];
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            let span = if along_x { hi[0] - lo[0] } else { hi[1] - lo[1] };
            let bx = |u0: f64, u1: f64, v0: f64, v1: f64, z0: f64, z1: f64| if along_x { ([c[0] + u0, c[1] + v0, z0], [c[0] + u1, c[1] + v1, z1]) } else { ([c[0] + v0, c[1] + u0, z0], [c[0] + v1, c[1] + u1, z1]) };
            let base = |u: f64, z: f64| if along_x { [c[0] + u, c[1], z] } else { [c[0], c[1] + u, z] };
            let axis = if along_x { "X" } else { "Y" };
            // a clip over each end of the pads: a base and two springy sides
            let zf = ch - 2.8;
            for (k, sign) in [(1, -1.0), (2, 1.0)] {
                let u = sign * span / 2.0;
                let body = format!("Clip{k}");
                let (p0, p1) = bx(u - 2.0, u + 2.0, -cw / 2.0, cw / 2.0, 0.0, 0.5);
                s.block(&format!("ClipBase{k}"), &body, "new", p0, p1);
                for (side, v0) in [("A", -cw / 2.0), ("B", cw / 2.0 - 0.4)] {
                    let (p0, p1) = bx(u - 2.0, u + 2.0, v0, v0 + 0.4, 0.0, ch);
                    s.block(&format!("ClipSide{k}{side}"), &body, "join", p0, p1);
                }
            }
            s.color(&["Clip1", "Clip2"], METAL, true);
            // the glass fuse with its metal caps, lying in the clips
            s.cylinder("Glass", "Glass", "new", base(-fl / 2.0 + 5.0, zf), axis, 2.4, fl - 10.0);
            s.color(&["Glass"], "#c8dce8", false);
            s.cylinder("CapA", "Caps", "new", base(-fl / 2.0, zf), axis, 2.5, 5.0);
            s.cylinder("CapB", "Caps", "join", base(fl / 2.0 - 5.0, zf), axis, 2.5, 5.0);
            s.color(&["Caps"], METAL, true);
            s.legs(pads, 0.5);
        }
    }
    Ok(Value::Array(s.cmds))
}

/// A drawing to model a part over (E7): seen from above (on the board's plane) or from the front (standing behind the part, its
/// picture's up along +z), the PNG and its width in millimetres.
pub struct Sketch {
    pub side: bool,
    pub path: String,
    pub width: f64,
}

/// The SolidCraft script of a hand-made shape: the drawings as canvases, a starting body `[w, h, height]` standing on the board's
/// face round the origin, and a leg in each through-hole of the footprint (E7 ②: through the board and 0.5 mm up into the body).
pub fn sketch_script(drawings: &[Sketch], starter: Option<[f64; 3]>, pads: &[Pad]) -> Value {
    let mut cmds = vec![];
    // how far behind the part the side view stands: clear of the part and of the top view
    let behind = drawings.iter().filter(|d| !d.side).map(|d| d.width / 2.0).fold(starter.map_or(10.0, |s| s[1] / 2.0 + 5.0), f64::max);
    if let Some([w, h, height]) = starter {
        cmds.push(json!({"command": "solid.box", "params": {"length": w, "width": h, "height": height, "corner": [-w / 2.0, -h / 2.0, 0.0], "body_name": "body"}}));
        cmds.push(json!({"command": "appearance.assign", "params": {"bodies": ["body"], "color": "#3a3a3a", "opacity": 0.8}}));
    }
    for d in drawings {
        cmds.push(if d.side {
            json!({"command": "canvas.insert", "params": {
                "path": d.path,
                "plane": {"origin": [0.0, behind, 0.0], "x_dir": [1.0, 0.0, 0.0], "y_dir": [0.0, 0.0, 1.0]},
                "center": [0.0, starter.map_or(d.width / 4.0, |s| s[2] / 2.0)],
                "width": d.width, "opacity": 0.6, "name": "横から見た図"}})
        } else {
            json!({"command": "canvas.insert", "params": {"path": d.path, "plane": "XY", "center": [0.0, 0.0], "width": d.width, "opacity": 0.6, "name": "上から見た図"}})
        });
    }
    cmds.extend(legs_commands(pads, 0.5));
    Value::Array(cmds)
}

/// The mouth's direction from the `open` dimension (±1 along x, ±2 along y) and the direction across it.
fn mouth_axes(open: f64) -> Result<([f64; 2], [f64; 2]), String> {
    match open.round() as i64 {
        1 => Ok(([1.0, 0.0], [0.0, 1.0])),
        -1 => Ok(([-1.0, 0.0], [0.0, 1.0])),
        2 => Ok(([0.0, 1.0], [1.0, 0.0])),
        -2 => Ok(([0.0, -1.0], [1.0, 0.0])),
        _ => Err("open は 1 (+x)・−1 (−x)・2 (+y)・−2 (−y) のどれかにしてください".into()),
    }
}

/// The legs at the footprint's through-holes as SolidCraft commands (a body `Legs` from `top` above the board down through it), for a
/// shape drawn by hand from the data sheet's drawing (E7 ②: the legs need not be drawn).
pub fn legs_commands(pads: &[Pad], top: f64) -> Vec<Value> {
    let mut s = Script { cmds: vec![] };
    s.legs(pads, top);
    s.cmds
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
/// The board's STEP (`step_out::write`: the board, the parts with a maker's own faces when there, the copper and the silkscreen) written
/// into `dir` as `<name>.step` and opened in the SolidCraft app (E8): to measure, cut a section, go on with the case's design or make a
/// picture there. The app is started and left running.
pub fn open_board_in_app(board: &crate::model::Board, dir: &Path) -> Result<PathBuf, String> {
    let (_, app) = find_solidcraft().ok_or("SolidCraft が見つかりません (SOLIDCRAFT_DIR か PATH に solidcraft.exe のあるフォルダを)")?;
    let step = write_board_step(board, dir)?;
    std::process::Command::new(&app).arg(&step).spawn().map_err(|e| format!("{}: {e}", app.display()))?;
    Ok(step)
}

/// The board's STEP written into `dir`, named after the board.
fn write_board_step(board: &crate::model::Board, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name: String = board.name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let step = dir.join(format!("{}.step", if name.is_empty() { "board".into() } else { name }));
    std::fs::write(&step, crate::step_out::write(board)).map_err(|e| format!("{}: {e}", step.display()))?;
    Ok(step)
}

/// A picture of the board made by SolidCraft (E8): `snapshot` (the app's own shading, quick) or `render` (path traced, a few seconds
/// to a minute), from the view `view` (iso, top, front, ...; the snapshot only), `w` x `h` pixels. The board's STEP goes into `dir`.
pub fn board_picture(board: &crate::model::Board, dir: &Path, out: &Path, view: &str, size: (u32, u32), render: bool) -> Result<(), String> {
    let (cli, _) = find_solidcraft().ok_or("SolidCraft が見つかりません (SOLIDCRAFT_DIR か PATH に solidcraft-cli.exe のあるフォルダを)")?;
    let step = write_board_step(board, dir)?;
    let mut c = std::process::Command::new(&cli);
    if render {
        c.arg("render").arg(&step).arg("--out").arg(out).arg("--overwrite").arg("--timeout").arg("300");
    } else {
        c.arg("snapshot").arg("--in").arg(&step).arg("--out").arg(out).arg("--view").arg(view).arg("--no-grid").arg("--no-axes").arg("--quality").arg("fine");
    }
    let r = c.arg("--width").arg(size.0.to_string()).arg("--height").arg(size.1.to_string()).output().map_err(|e| format!("{}: {e}", cli.display()))?;
    if !r.status.success() || !out.is_file() {
        let err = String::from_utf8_lossy(&r.stderr);
        return Err(format!("SolidCraft が絵を作れませんでした: {}", err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("")));
    }
    Ok(())
}

/// Runs a script and writes its STEP (`name.step`) and the script (`name.json`) into `dir`: the STEP's path.
pub fn run_to_step(cli: &Path, script: &Value, dir: &Path, name: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (file, step) = (dir.join(format!("{name}.json")), dir.join(format!("{name}.step")));
    std::fs::write(&file, serde_json::to_string_pretty(script).unwrap_or_default()).map_err(|e| format!("{}: {e}", file.display()))?;
    let _ = std::fs::remove_file(&step);
    let out = std::process::Command::new(cli).arg("run").arg(&file).arg("--out").arg(&step).arg("--quiet").output().map_err(|e| format!("{}: {e}", cli.display()))?;
    if !out.status.success() || !step.is_file() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("SolidCraft が形を作れませんでした: {}", err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("")));
    }
    Ok(step)
}

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

    /// SolidCraft makes a picture of a board from its STEP (`--ignored`: needs SolidCraft): a PNG of the size asked.
    #[test]
    #[ignore]
    fn solidcraft_makes_a_picture_of_the_board() {
        let b: crate::model::Board = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/examples/rlc_board.pcbjson")).unwrap()).unwrap();
        let dir = std::env::temp_dir().join("circuitcad_board_picture");
        let out = dir.join("board.png");
        let _ = std::fs::remove_file(&out);
        board_picture(&b, &dir, &out, "iso", (640, 400), false).unwrap();
        let png = std::fs::read(&out).unwrap();
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']), "a PNG");
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

    /// The sketch script: the top view on the board's plane, the side view standing behind the part and the starter, a leg in each
    /// through-hole (E7 ②④).
    #[test]
    fn the_sketch_script_has_its_canvases_starter_and_legs() {
        let pads = [hole("1", 0.0, 0.0, 0.8), hole("2", 2.54, 0.0, 0.8)];
        let s = sketch_script(&[Sketch { side: false, path: "top.png".into(), width: 12.0 }, Sketch { side: true, path: "side.png".into(), width: 12.0 }], Some([6.0, 4.0, 3.0]), &pads);
        let cmds = commands(&s);
        assert_eq!(cmds.iter().filter(|c| **c == "canvas.insert").count(), 2);
        assert!(cmds.contains(&"solid.box"));
        let a = s.as_array().unwrap();
        assert_eq!(a.iter().filter(|c| c["params"]["name"].as_str().is_some_and(|n| n.starts_with("Leg"))).count(), 2);
        let side = a.iter().find(|c| c["params"]["name"] == "横から見た図").unwrap();
        assert!(side["params"]["plane"]["origin"][1].as_f64().unwrap() > 6.0, "behind the part and the top view");
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
        assert_eq!(Template::for_footprint("J1", "USB_DEMO_TH"), Some(Template::UsbA));
        assert_eq!(Template::for_footprint("J1", "USB_C_Receptacle_GCT_USB4105"), Some(Template::UsbC));
        assert_eq!(Template::for_footprint("J1", "PinHeader_1x4"), Some(Template::Header));
        assert_eq!(Template::for_footprint("C1", "CP_Radial_D6.3mm_P2.50mm"), Some(Template::Elcap));
        assert_eq!(Template::for_footprint("BT1", "BatteryHolder_AA"), Some(Template::BatteryAa));
        assert_eq!(Template::for_footprint("F1", "Fuse_5x20_P22.5"), Some(Template::Fuse5x20));
        assert!(script(Template::UsbA, &[("open".to_string(), 3.0)].into(), &relay).unwrap_err().contains("open"));
        assert!(script(Template::Header, &BTreeMap::new(), &[Pad { drill: None, ..hole("1", 0.0, 0.0, 1.0) }]).is_err(), "a header needs holes");
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
        let cap = [hole("1", 0.0, 0.0, 0.8), hole("2", 2.5, 0.0, 0.8)];
        let header: Vec<Pad> = (0..4).map(|i| hole(&(i + 1).to_string(), 0.0, 2.54 * i as f64, 1.0)).collect();
        let usb = [hole("1", 0.0, 0.0, 0.9), hole("2", 0.0, 2.5, 0.9), hole("3", 0.0, 4.5, 0.9), hole("4", 0.0, 7.0, 0.9)];
        let aa = [hole("1", 0.0, 0.0, 1.2), hole("2", 50.0, 0.0, 1.2)];
        let fuse = [hole("1", 0.0, 0.0, 1.3), hole("2", 22.5, 0.0, 1.3)];
        for (t, pads, top) in [
            (Template::DcJack, &jack[..], 11.0),
            (Template::Tact, &tact[..], 4.3),
            (Template::Relay, &relay[..], 15.35),
            (Template::Terminal, &term[..], 10.0),
            (Template::Elcap, &cap[..], 11.0),
            (Template::Header, &header[..], 8.5),
            (Template::UsbA, &usb[..], 5.12),
            (Template::UsbC, &usb[..], 3.26),
            (Template::BatteryAa, &aa[..], 16.0),
            (Template::Fuse5x20, &fuse[..], 9.0),
        ] {
            let dir = std::env::temp_dir().join("circuitcad_shape_test").join(t.key());
            let m = build(&cli, &script(t, &BTreeMap::new(), pads).unwrap(), &dir).unwrap_or_else(|e| panic!("{t:?}: {e}"));
            let zmax = m.vertices.iter().map(|v| v[2]).fold(f32::MIN, f32::max);
            let zmin = m.vertices.iter().map(|v| v[2]).fold(f32::MAX, f32::min);
            println!("{t:?}: {} triangles, {} colours, z {zmin}..{zmax}", m.triangles.len(), m.palette.len());
            assert!(!m.triangles.is_empty() && m.triangles.len() <= 2_000, "{t:?}: {} triangles (our STEP reader, not SolidCraft's fine cut)", m.triangles.len());
            assert!(dir.join("shape.step").is_file(), "{t:?}: the STEP is kept");
            assert!(m.palette.len() >= 2, "{t:?}: body and legs in their colours");
            let below = if t == Template::Header { 3.0 } else { LEG_BELOW };
            assert!((f64::from(zmax) - top).abs() < 0.05 && (f64::from(zmin) + below).abs() < 0.05, "{t:?}: z {zmin}..{zmax}");
        }
    }
}

