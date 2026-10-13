//! "Create part" window: pin table + package dimensions + simulation model -> `PartDef`.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use part_lib::passive::{self, PassiveKind, PassiveTemplate};
use part_lib::templates::{ChargerTemplate, CurrentSetting, Topology};
use part_lib::{Issue, PartDef, PinDef};
use pcb_core::footprints::Footprint;
use pcb_core::body::{BodyMark, BodySpec};
use pcb_core::package::{HoleSpec, PackageSpec, PadSpec};
use pcb_core::PadShape;
use sch_core::{LifeRating, PartRatings, PinSide, RippleRating, SimKind, SimModel, SpecRow};

#[derive(Clone, Copy, PartialEq)]
pub enum PkgKind {
    Chip2,
    Dual,
    Quad,
    Row,
}

/// The tabs of the form.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FormTab {
    Basic,
    Pins,
    Package,
    Sim,
    Ratings,
}

const FORM_TABS: &[(FormTab, &str, &str)] = &[
    (FormTab::Basic, "① 基本", "部品の名前・番号の頭文字・値・説明。チップ部品や充電ICは、ここの「かんたん作成」で数字を入れるだけで作れます"),
    (FormTab::Pins, "② ピン", "ピンの番号と名前、記号のどの辺に出すか。データシートの「ピン配置」の表を見ながら入れます"),
    (FormTab::Package, "③ 基板の形", "データシートの推奨ランドパターン (パッド) と外形寸法 (本体の大きさ・高さ) を入れます。右の絵で確かめられます"),
    (FormTab::Sim, "④ シミュレーション", "SPICE モデルを貼ると、種類・端子を自動で入力します。シミュレーションしない部品 (基板づくりだけ) は「なし」のままです"),
    (FormTab::Ratings, "⑤ 定格", "データシートの絶対最大定格 (電圧・電流・電力) と電気的特性。入れておくと、回路で超えそうなときに警告します"),
];

/// What kind of part: the first question of the form.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PartType {
    Ic,
    Transistor,
    Diode,
    Capacitor,
    Resistor,
    Inductor,
    Connector,
}

const PART_TYPES: &[(PartType, &str, &str)] = &[
    (PartType::Ic, "IC", "オペアンプ・レギュレータ・マイコンなど。足が2列に並ぶ (SOIC・DIP) 形を例に、ピン・パッケージ・本体の外形を入れます。QFN や放熱パッドは「③ 基板の形」で変えます"),
    (PartType::Transistor, "トランジスタ", "NPN・PNP・MOSFET。足が1列の形 (TO-92 など) を例に入れます。記号は規格の形になります"),
    (PartType::Diode, "ダイオード", "整流・ショットキー・ツェナー・LED。足が2本の形 (DO-35 など) を例に入れます。カソードの棒も付きます"),
    (PartType::Capacitor, "コンデンサ", "積層セラミック・電解・タンタル・フィルム。種類を選ぶと、基板の形 (リード付きかチップか)・極性・高さの例が入ります。「① 基本」の「かんたん作成」に、容量と ESR などの数字を入れてモデルを作ります"),
    (PartType::Resistor, "抵抗", "チップ抵抗 (0603 など) かリード付き。「① 基本」の「かんたん作成」に、抵抗値を入れて作ります"),
    (PartType::Inductor, "コイル", "チップインダクタかリード付き。「① 基本」の「かんたん作成」に、インダクタンスと DCR を入れて作ります"),
    (PartType::Connector, "コネクタ", "ピンヘッダ・端子台など、足が1列に並ぶ部品"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SimChoice {
    None,
    Model,
    Subckt,
}

pub struct PartForm {
    /// The tab shown and the kind of part chosen first (None = not chosen: the form is as before).
    pub tab: FormTab,
    pub ptype: Option<PartType>,
    /// The sub-kind chosen under the kind ("ceramic", "electrolytic", "tantalum", "film", "chip", "leaded").
    pub ptype_sub: &'static str,
    /// A polarised capacitor (electrolytic, tantalum): the symbol shows the + and the pins are named + and -.
    pub pas_polar: bool,
    pub name: String,
    pub category: String,
    pub prefix: String,
    pub value: String,
    pub description: String,
    pub datasheet: String,
    pub pins_text: String,
    pub pkg: PkgKind,
    pub pitch: f64,
    pub row: f64,
    pub span: f64,
    pub len: f64,
    pub width: f64,
    pub pad_d: f64,
    pub drill: f64,
    pub tht: bool,
    /// The exposed pad under an IC (QFN, ESOP, SOT-223 tab): its pin, width and length. The pin must be in the pin table.
    pub thermal_on: bool,
    pub thermal_pin: String,
    pub thermal_w: f64,
    pub thermal_h: f64,
    /// The body drawn on the silkscreen: a rectangle (`body_w` x `body_h`) or a circle (`body_w` across), centred `body_x`, `body_y` from the origin.
    pub body_on: bool,
    pub body_circle: bool,
    pub body_w: f64,
    pub body_h: f64,
    pub body_x: f64,
    pub body_y: f64,
    pub body_mark: BodyMark,
    /// The pin the orientation mark belongs to (empty = the first pad).
    pub body_pin: String,
    /// Body height above the board in mm (0 = not given: the 3D view guesses from the footprint).
    pub height: f64,
    /// Extra pads and holes beyond the package: shield legs, locating pegs, slots. One per line, see `EXTRAS_HELP`.
    pub extras: String,
    /// A built-in footprint used as the package (empty = the dimensions typed below).
    pub fp_preset: String,
    /// Symbol drawn for a chip part (see `PartDef.glyph`); set by the chip part section.
    pub glyph: String,
    /// The chip part section: kind, value and what the data sheet says, as typed.
    pub pas_kind: PassiveKind,
    pub pas_value: String,
    pub pas_esr: String,
    /// The impedance at 100 kHz from the data sheet's table; for an electrolytic it is about the ESR.
    pub pas_z100k: String,
    pub pas_esl: String,
    pub pas_dcr: String,
    pub pas_cp: String,
    pub pas_srf: String,
    pub pas_bias: String,
    pub pas_table: String,
    pub pas_message: Option<Result<String, String>>,
    pub sim: SimChoice,
    pub sim_element: String,
    pub sim_name: String,
    pub sim_text: String,
    pub sim_pins: String,
    /// What the model leaves out, one note per line.
    pub sim_limits: String,
    /// Data sheet ratings: absolute maximum voltage [V] / current [A] / power [W] (blank = not given) and
    /// further rows, one per line as `name | value | condition`.
    pub rt_v: String,
    pub rt_i: String,
    pub rt_p: String,
    pub rt_rows: String,
    /// An electrolytic capacitor's rated ripple current [A rms] and the frequency it is given at, the data sheet's
    /// multiplier table (one `frequency multiplier` per line), and its endurance (hours at a temperature).
    pub rt_ripple: String,
    pub rt_ripple_f: String,
    pub rt_ripple_table: String,
    pub rt_life_h: String,
    pub rt_life_t: String,
    /// Which checklist of what to look up the form offers (index into `CHECKLISTS`).
    pub rt_kind: usize,
    /// The charger template section and what it said last.
    pub tpl: ChargerTemplate,
    pub tpl_message: String,
    /// What the last "fill from SPICE" did or why it could not.
    pub spice_message: Option<Result<String, String>>,
}

impl Default for PartForm {
    fn default() -> Self {
        Self {
            tab: FormTab::Basic,
            ptype: None,
            ptype_sub: "",
            pas_polar: false,
            name: String::new(),
            category: "ユーザー".into(),
            prefix: "U".into(),
            value: String::new(),
            description: String::new(),
            datasheet: String::new(),
            pins_text: "1 IN\n2 GND\n3 OUT\n4 VCC".into(),
            pkg: PkgKind::Dual,
            pitch: 1.27,
            row: 5.4,
            span: 8.4,
            len: 1.55,
            width: 0.6,
            pad_d: 1.7,
            drill: 1.0,
            tht: false,
            thermal_on: false,
            thermal_pin: String::new(),
            thermal_w: 3.0,
            thermal_h: 3.0,
            body_on: false,
            body_circle: false,
            body_w: 3.9,
            body_h: 4.9,
            body_x: 0.0,
            body_y: 0.0,
            body_mark: BodyMark::Dot,
            body_pin: String::new(),
            height: 0.0,
            extras: String::new(),
            fp_preset: String::new(),
            glyph: String::new(),
            pas_kind: PassiveKind::Capacitor,
            pas_value: String::new(),
            pas_esr: String::new(),
            pas_z100k: String::new(),
            pas_esl: String::new(),
            pas_dcr: String::new(),
            pas_cp: String::new(),
            pas_srf: String::new(),
            pas_bias: String::new(),
            pas_table: String::new(),
            pas_message: None,
            sim: SimChoice::None,
            sim_element: "D".into(),
            sim_name: String::new(),
            sim_text: String::new(),
            sim_pins: String::new(),
            sim_limits: String::new(),
            rt_v: String::new(),
            rt_i: String::new(),
            rt_p: String::new(),
            rt_rows: String::new(),
            rt_ripple: String::new(),
            rt_ripple_f: "100k".into(),
            rt_ripple_table: String::new(),
            rt_life_h: String::new(),
            rt_life_t: "105".into(),
            rt_kind: 0,
            tpl: ChargerTemplate::default(),
            tpl_message: String::new(),
            spice_message: None,
        }
    }
}

/// A built footprint pad written back as an explicit pad.
fn pad_spec(_fp: &Footprint, p: &pcb_core::footprints::FpPad) -> PadSpec {
    let (shape, w, h) = match p.shape {
        PadShape::Circle { d } => ("circle", d, 0.0),
        PadShape::Rect { w, h } => ("rect", w, h),
        PadShape::Oval { w, h } => ("oval", w, h),
    };
    PadSpec { pin: p.pin.clone(), x: p.pos.x, y: p.pos.y, shape: shape.into(), w, h, drill: p.drill, slot: p.slot, slot_v: p.slot_v }
}

const EXTRAS_HELP: &str = "1行に1つ。座標は部品の中心からの mm (右が +X、上が +Y)。
  脚 ピン名 X Y パッド幅 パッド高さ 穴径 [長穴の長さ] [v]   … めっきのある穴 (USBのシェル脚など)。長穴は長さを足す (縦向きは最後に v)
  穴 X Y 穴径 [長穴の長さ] [v]                            … めっきなしの穴 (位置決めの突起用の穴、ネジ穴)
  パッド ピン名 X Y 幅 高さ                                … 面実装の追加パッド (シールドのはんだ付け部分など)
ピン名はピン表の「番号」と同じにします。シェル用のピン (例: SH) をピン表に足して、回路図でGNDなどにつなぎます。";

/// Example lines the learner edits to the numbers of the data sheet.
const EXTRAS_EXAMPLES: &[(&str, &str, &str)] = &[
    ("シェル脚(めっき穴)", "脚 SH -6.5 0 2.0 3.0 1.2", "コネクタの金属の外側 (シェル) をはんだ付けする脚。ピン名 SH をピン表にも足します"),
    ("位置決めの穴(めっきなし)", "穴 0 -3 1.0", "部品の突起が入る穴。めっきなしなので、回路にはつながりません"),
    ("長穴(めっき)", "脚 SH 6.5 0 1.6 3.6 0.8 2.4 v", "穴径 0.8mm、長さ 2.4mm の縦の長穴 (USB Type-C の固定脚など)"),
    ("長穴(めっきなし)", "穴 0 3 1.0 2.0", "幅 1.0mm、長さ 2.0mm の横の長穴"),
    ("面実装のシールド", "パッド SH -4 -2 1.5 2.0", "面実装 (SMD) タイプの固定用パッド"),
];

/// A frequency: `30M` is 30 MHz here (not milli), `100k`, `1G`, an optional `Hz`.
fn parse_freq(t: &str) -> Option<f64> {
    let l = t.trim().to_lowercase();
    let l = l.trim_end_matches("hz").trim();
    let l = if l.ends_with('m') && !l.ends_with("meg") { format!("{}meg", &l[..l.len() - 1]) } else { l.to_string() };
    sim_core::parse_value(&l)
}

/// An impedance or resistance: `20m` is 20 milliohm, an optional `Ω` / `ohm`.
fn parse_ohm(t: &str) -> Option<f64> {
    let l = t.trim().to_lowercase();
    sim_core::parse_value(l.trim_end_matches("ohm").trim_end_matches('ω').trim())
}

fn parse_side(t: &str) -> Option<PinSide> {
    match t.to_ascii_lowercase().as_str() {
        "left" | "l" | "左" => Some(PinSide::Left),
        "right" | "r" | "右" => Some(PinSide::Right),
        "top" | "t" | "上" => Some(PinSide::Top),
        "bottom" | "b" | "下" => Some(PinSide::Bottom),
        _ => None,
    }
}

/// What pasted SPICE text says about the part: model or subcircuit, its name and the terminal roles in node order.
#[derive(Debug, PartialEq)]
pub struct SpiceGuess {
    pub subckt: bool,
    pub name: String,
    /// Element letter of a `.model` (D, Q, M, S); empty for a `.subckt`.
    pub element: &'static str,
    /// Terminal names in node order (a `.subckt`'s ports, or A K / C B E / D G S for a `.model`).
    pub roles: Vec<String>,
    /// Side of each terminal on the symbol, same order as `roles`.
    pub sides: Vec<&'static str>,
}

/// Find the first `.subckt` (else the first `.model`) in pasted SPICE text.
pub fn guess_spice(text: &str) -> Result<SpiceGuess, String> {
    let mut lines: Vec<String> = vec![];
    for raw in text.lines() {
        let l = raw.split(';').next().unwrap_or("").trim();
        if l.is_empty() || l.starts_with('*') {
            continue;
        }
        match (l.strip_prefix('+'), lines.last_mut()) {
            (Some(rest), Some(last)) => {
                last.push(' ');
                last.push_str(rest.trim());
            }
            _ => lines.push(l.to_string()),
        }
    }
    let card = |dir: &str| lines.iter().find(|l| l.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case(dir)));
    if let Some(l) = card(".subckt") {
        let mut it = l.split_whitespace().skip(1);
        let name = it.next().ok_or(".subckt に名前がありません。`.subckt 名前 端子1 端子2 …` の形で書きます")?.to_string();
        let roles: Vec<String> = it.take_while(|w| !w.contains('=') && !w.eq_ignore_ascii_case("params:")).map(String::from).collect();
        if roles.is_empty() {
            return Err(format!(".subckt {name} に端子がありません。名前のあとに端子を並べます (例: .subckt {name} IN OUT GND)"));
        }
        let half = roles.len().div_ceil(2);
        let sides = (0..roles.len()).map(|i| if i < half { "left" } else { "right" }).collect();
        return Ok(SpiceGuess { subckt: true, name, element: "", roles, sides });
    }
    let l = card(".model").ok_or("`.model` も `.subckt` も見つかりません。SPICEモデルの文をそのまま貼ってください")?;
    let rest: Vec<&str> = l.split_whitespace().skip(1).collect();
    let name = rest.first().ok_or(".model に名前がありません。`.model 名前 種類(パラメータ)` の形で書きます")?.to_string();
    let ty = rest.get(1..).unwrap_or(&[]).join(" ");
    let ty = ty.split(['(', ' ']).next().unwrap_or("").to_ascii_uppercase();
    let (element, roles, sides): (&str, &[&str], &[&str]) = match ty.as_str() {
        "D" => ("D", &["A", "K"], &["left", "right"]),
        "NPN" | "PNP" => ("Q", &["C", "B", "E"], &["top", "left", "bottom"]),
        "NMOS" | "PMOS" => ("M", &["D", "G", "S"], &["top", "left", "bottom"]),
        "SW" => ("S", &[], &[]),
        _ => return Err(format!(".model {name} の種類「{ty}」は未対応です (D / NPN / PNP / NMOS / PMOS / SW)")),
    };
    Ok(SpiceGuess { subckt: false, name, element, roles: roles.iter().map(|s| s.to_string()).collect(), sides: sides.to_vec() })
}

impl PartForm {
    /// Fill the model kind, name, element, pin list and node order from the pasted SPICE text.
    /// Returns what was done, for the learner. Pins the learner already wrote are kept when they can be matched.
    pub fn autofill_from_spice(&mut self) -> Result<String, String> {
        let g = guess_spice(&self.sim_text)?;
        self.sim = if g.subckt { SimChoice::Subckt } else { SimChoice::Model };
        self.sim_name = g.name.clone();
        if !g.subckt {
            self.sim_element = g.element.to_string();
            if self.prefix.trim().is_empty() || self.prefix == "U" {
                self.prefix = if g.element == "D" { "D" } else { "Q" }.into();
            }
        }
        if self.name.trim().is_empty() {
            self.name = g.name.clone();
        }
        if g.roles.is_empty() {
            return Ok(format!("スイッチ {} のモデルとして入力しました。ピンの対応 (制御側 2 本、スイッチ側 2 本) は自分で確かめてください", g.name));
        }
        // match the pins already written by name, in the model's node order
        let existing = self.parse_pins().unwrap_or_default();
        let find = |role: &str| existing.iter().find(|p| p.name.eq_ignore_ascii_case(role) || p.number.eq_ignore_ascii_case(role)).map(|p| p.number.clone());
        let matched: Vec<Option<String>> = g.roles.iter().map(|r| find(r)).collect();
        if !existing.is_empty() && matched.iter().all(Option::is_some) {
            self.sim_pins = matched.into_iter().flatten().collect::<Vec<_>>().join(" ");
            return Ok(format!("{} を入力しました。いまのピン表の名前 ({}) と対応づけました", g.name, g.roles.join(" ")));
        }
        let is_default = self.pins_text.trim() == PartForm::default().pins_text.trim() || self.pins_text.trim().is_empty();
        if is_default {
            // model terminals get numbers in node order, the symbol sides put them where a schematic expects them
            let numbers: Vec<String> = (1..=g.roles.len()).map(|n| n.to_string()).collect();
            self.pins_text = g.roles.iter().zip(&numbers).zip(&g.sides).map(|((r, n), s)| format!("{n} {r} {s}")).collect::<Vec<_>>().join("\n");
            self.sim_pins = numbers.join(" ");
            if !g.subckt {
                self.pkg = if g.element == "D" { PkgKind::Chip2 } else { PkgKind::Row };
            } else {
                // a dual-row package needs an even number of pins; the learner sets the real package from the data sheet
                self.pkg = if g.roles.len() % 2 == 0 { PkgKind::Dual } else { PkgKind::Row };
            }
            return Ok(format!("{} を入力しました。ピン {} 本 ({}) を作りました。ピン番号はデータシートの番号に直してください (sim の対応も同じ番号に)", g.name, g.roles.len(), g.roles.join(" ")));
        }
        if existing.len() == g.roles.len() {
            self.sim_pins = existing.iter().map(|p| p.number.clone()).collect::<Vec<_>>().join(" ");
            return Ok(format!("{} を入力しました。ピンの名前が合わなかったので、ピン表の上から順に端子 ({}) へ対応づけました。順番を確かめてください", g.name, g.roles.join(" ")));
        }
        Err(format!("モデルの端子は {} 本 ({}) ですが、ピン表は {} 本です。ピン表を端子の数に合わせるか、ピン表を消してからもう一度押してください", g.roles.len(), g.roles.join(" "), existing.len()))
    }

    fn parse_pins(&self) -> Result<Vec<PinDef>, String> {
        let mut out = vec![];
        for (n, line) in self.pins_text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
            if t.len() < 2 {
                return Err(format!("{}行目: 「番号 名前」の形式で入力してください", n + 1));
            }
            let side = match t.get(2) {
                None => None,
                Some(s) => Some(parse_side(s).ok_or_else(|| format!("{}行目: 向き「{s}」は left/right/top/bottom のいずれかです", n + 1))?),
            };
            out.push(PinDef { number: t[0].into(), name: t[1].into(), side, sim_only: false });
        }
        Ok(out)
    }

    /// The data sheet information typed into the form (None when nothing was typed).
    fn parse_ratings(&self) -> Result<Option<PartRatings>, String> {
        let num = |label: &str, t: &str| -> Result<Option<f64>, String> {
            let t = t.trim();
            if t.is_empty() {
                return Ok(None);
            }
            sim_core::parse_value(t)
                .filter(|v| *v > 0.0)
                .map(Some)
                .ok_or_else(|| format!("{label}「{t}」を、0 より大きい数で入力してください (例: 40 / 1.5 / 500m)"))
        };
        let mut rows = vec![];
        for (n, line) in self.rt_rows.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            if parts.len() < 2 || parts[0].is_empty() {
                return Err(format!("データシートの情報 {}行目: 「項目 | 値 | 条件」の形式で入力してください (条件は省略できます。例: 順方向電圧 | 0.9V | IF = 1A)", n + 1));
            }
            if parts[1].is_empty() {
                continue; // an item of the checklist that has not been looked up yet
            }
            // from a ※ on, a condition is the checklist's note to the learner, not a data sheet figure
            let condition = parts.get(2).map(|c| c.split('※').next().unwrap_or("").trim().to_string()).unwrap_or_default();
            rows.push(SpecRow { name: parts[0].into(), value: parts[1].into(), condition });
        }
        let ripple = match num("定格リップル電流", &self.rt_ripple)? {
            None => None,
            Some(i_rms) => {
                let freq_hz = parse_freq(self.rt_ripple_f.trim()).filter(|f| *f > 0.0).ok_or_else(|| format!("リップル電流の周波数「{}」を、0 より大きい数で入力してください (例: 100k / 120)", self.rt_ripple_f.trim()))?;
                let mut coeffs = vec![];
                for (n, line) in self.rt_ripple_table.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
                    let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',' || c == '|').filter(|s| !s.is_empty()).collect();
                    match (t.first().and_then(|s| parse_freq(s)), t.get(1).and_then(|s| s.parse::<f64>().ok())) {
                        (Some(f), Some(m)) if f > 0.0 && m > 0.0 => coeffs.push((f, m)),
                        _ => return Err(format!("リップル電流の補正係数 {}行目: 「周波数 係数」の形で入力してください (例: 120 0.55 / 1k 0.77)", n + 1)),
                    }
                }
                coeffs.sort_by(|a, b| a.0.total_cmp(&b.0));
                Some(RippleRating { i_rms, freq_hz, coeffs })
            }
        };
        let life = match num("寿命", &self.rt_life_h)? {
            None => None,
            Some(hours) => {
                let temp_c = self.rt_life_t.trim().parse::<f64>().ok().filter(|t| *t > -100.0 && *t < 400.0).ok_or_else(|| format!("寿命の温度「{}」を数値 (℃) で入力してください (例: 105)", self.rt_life_t.trim()))?;
                Some(LifeRating { hours, temp_c })
            }
        };
        let r = PartRatings { v_max: num("最大電圧", &self.rt_v)?, i_max: num("最大電流", &self.rt_i)?, p_max: num("最大電力", &self.rt_p)?, rows, ripple, life };
        Ok(Some(r).filter(|r| !r.is_empty()))
    }

    /// How much of the data sheet section is filled: (maximum ratings given 0..=3, rows with a value, rows still open).
    fn ratings_progress(&self) -> (usize, usize, usize) {
        let given = |t: &str| sim_core::parse_value(t.trim()).is_some_and(|v| v > 0.0) as usize;
        let max_given = given(&self.rt_v) + given(&self.rt_i) + given(&self.rt_p);
        // the capacitor's ripple current and life count as figures looked up
        let (mut filled, mut open) = (given(&self.rt_ripple) + given(&self.rt_life_h), 0);
        for line in self.rt_rows.lines().filter(|l| !l.trim().is_empty()) {
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            if parts.len() >= 2 && !parts[0].is_empty() {
                if parts[1].is_empty() { open += 1 } else { filled += 1 }
            }
        }
        (max_given, filled, open)
    }

    /// The extra pads and holes typed into the form.
    fn parse_extras(&self) -> Result<(Vec<PadSpec>, Vec<HoleSpec>), String> {
        let (mut pads, mut holes) = (vec![], vec![]);
        for (n, line) in self.extras.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
            let at = |m: String| format!("追加のパッドと穴 {}行目: {m}", n + 1);
            // a trailing v / h says which way a slot runs
            let (t, slot_v) = match t.last().map(|s| s.to_ascii_lowercase()) {
                Some(o) if matches!(o.as_str(), "v" | "縦") => (&t[..t.len() - 1], true),
                Some(o) if matches!(o.as_str(), "h" | "横") => (&t[..t.len() - 1], false),
                _ => (&t[..], false),
            };
            let num = |i: usize, what: &str| -> Result<f64, String> {
                t.get(i).and_then(|s| s.parse::<f64>().ok()).ok_or_else(|| at(format!("{what}を数字 (mm) で入力してください")))
            };
            let slot = |i: usize| -> Result<f64, String> { if t.len() > i { num(i, "長穴の長さ") } else { Ok(0.0) } };
            match t[0].to_ascii_lowercase().as_str() {
                "脚" | "leg" | "pth" | "th" => {
                    let (x, y, w, h, drill, sl) = (num(2, "X")?, num(3, "Y")?, num(4, "パッドの幅")?, num(5, "パッドの高さ")?, num(6, "穴径")?, slot(7)?);
                    let shape = if sl > 0.0 || (w - h).abs() > 1e-9 { "oval" } else { "circle" };
                    pads.push(PadSpec { pin: t.get(1).ok_or_else(|| at("ピン名がありません".into()))?.to_string(), x, y, shape: shape.into(), w, h, drill: Some(drill), slot: sl, slot_v });
                }
                "パッド" | "pad" | "smd" => {
                    let (x, y, w, h) = (num(2, "X")?, num(3, "Y")?, num(4, "パッドの幅")?, num(5, "パッドの高さ")?);
                    pads.push(PadSpec { pin: t.get(1).ok_or_else(|| at("ピン名がありません".into()))?.to_string(), x, y, shape: "rect".into(), w, h, drill: None, slot: 0.0, slot_v: false });
                }
                "穴" | "hole" | "npth" => holes.push(HoleSpec { x: num(1, "X")?, y: num(2, "Y")?, drill: num(3, "穴径")?, slot: slot(4)?, slot_v }),
                other => return Err(at(format!("先頭の言葉「{other}」が分かりません。脚 / 穴 / パッド のどれかで始めてください (書き方は下の説明)"))),
            }
        }
        Ok((pads, holes))
    }

    pub fn to_def(&self) -> Result<PartDef, String> {
        let pins = self.parse_pins()?;
        let (mut extra_pads, extra_holes) = self.parse_extras()?;
        if self.thermal_on && matches!(self.pkg, PkgKind::Dual | PkgKind::Quad) {
            let pin = self.thermal_pin.trim();
            if pin.is_empty() {
                return Err("放熱パッドのピン番号を入れてください (ピン表に、そのピンの行も書きます。例: 9 EP)".into());
            }
            if !pins.iter().any(|p| p.number == pin) {
                return Err(format!("放熱パッドのピン「{pin}」が、ピン表にありません。ピン表に「{pin} GND」のように1行足してください"));
            }
            if self.thermal_w <= 0.0 || self.thermal_h <= 0.0 {
                return Err("放熱パッドの幅と長さは 0 より大きい値にしてください".into());
            }
            extra_pads.push(PadSpec { pin: pin.to_string(), x: 0.0, y: 0.0, shape: "rect".into(), w: self.thermal_w, h: self.thermal_h, drill: None, slot: 0.0, slot_v: false });
        }
        // pins that only exist as extra pads (the shell, say) are not part of the package's row of pads
        let n = pins.iter().filter(|p| !extra_pads.iter().any(|e| e.pin == p.number)).count();
        let package = match self.pkg {
            PkgKind::Chip2 => PackageSpec::Chip2 { pitch: self.pitch, pad_len: self.len, pad_width: self.width },
            PkgKind::Dual => PackageSpec::Dual {
                pins: n,
                pitch: self.pitch,
                row_spacing: self.row,
                pad_len: self.len,
                pad_width: self.width,
                drill: self.tht.then_some(self.drill),
            },
            PkgKind::Quad => PackageSpec::Quad { pins: n, pitch: self.pitch, span: self.span, pad_len: self.len, pad_width: self.width },
            PkgKind::Row => PackageSpec::SingleRow { pins: n, pitch: self.pitch, pad_d: self.pad_d, drill: self.drill },
        };
        // a built-in footprint replaces the typed dimensions
        let (package, footprint_name) = match self.fp_preset.trim() {
            "" => (package, None),
            name => {
                let fp = pcb_core::footprints::get(name).ok_or_else(|| format!("フットプリント {name} が見つかりません"))?;
                let pads = fp.pads.iter().map(|p| pad_spec(&fp, p)).collect();
                let holes = fp.holes.iter().map(|h| HoleSpec { x: h.pos.x, y: h.pos.y, drill: h.drill, slot: h.slot, slot_v: h.slot_v }).collect();
                (PackageSpec::Explicit { pads, holes }, Some(name.to_string()))
            }
        };
        let package = if extra_pads.is_empty() && extra_holes.is_empty() {
            package
        } else {
            // the package's own pads and the extras become one explicit list
            let fp = package.build(self.name.trim())?;
            let mut pads: Vec<PadSpec> = fp.pads.iter().map(|p| pad_spec(&fp, p)).collect();
            pads.extend(extra_pads);
            PackageSpec::Explicit { pads, holes: extra_holes }
        };
        let sim = match self.sim {
            SimChoice::None => None,
            k => Some(SimModel {
                kind: if k == SimChoice::Model { SimKind::Model } else { SimKind::Subckt },
                element: self.sim_element.trim().to_string(),
                name: self.sim_name.trim().to_string(),
                text: self.sim_text.clone(),
                pins: self.sim_pins.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(String::from).collect(),
                params: Default::default(),
                limits: self.sim_limits.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
            }),
        };
        Ok(PartDef {
            name: self.name.trim().to_string(),
            category: self.category.trim().to_string(),
            description: self.description.clone(),
            datasheet: self.datasheet.clone(),
            prefix: self.prefix.trim().to_string(),
            value: Some(self.value.trim().to_string()).filter(|s| !s.is_empty()),
            pins,
            package,
            footprint_name,
            height: Some(self.height).filter(|h| *h > 0.0),
            sim,
            ratings: self.parse_ratings()?,
            glyph: self.glyph.clone(),
            body: self.body_on.then(|| BodySpec {
                shape: if self.body_circle { "circle" } else { "rect" }.into(),
                w: self.body_w,
                h: self.body_h,
                x: self.body_x,
                y: self.body_y,
                mark: self.body_mark,
                mark_pin: self.body_pin.trim().to_string(),
            }),
        })
    }

    fn pas_number(&self, label: &str, t: &str, freq: bool) -> Result<Option<f64>, String> {
        let t = t.trim();
        if t.is_empty() {
            return Ok(None);
        }
        let parsed = if freq { parse_freq(t) } else { parse_ohm(t) };
        parsed.filter(|v| *v > 0.0).map(Some).ok_or_else(|| format!("{label}「{t}」を、0 より大きい数で入力してください (例: 20m / 1n / 30M)"))
    }

    /// Fill the form from the chip part section: pins, symbol, value and a `.subckt` with the parasitics.
    fn apply_passive(&mut self) -> Result<String, String> {
        let value = self.pas_number(self.pas_kind.label(), &self.pas_value, false)?.ok_or("値を入力してください (例: 100n / 4.7k / 10u)")?;
        let mut t = PassiveTemplate::new(self.pas_kind, value);
        t.esr = self.pas_number("ESR", &self.pas_esr, false)?;
        t.esl = self.pas_number("ESL", &self.pas_esl, false)?;
        t.dcr = self.pas_number("DCR", &self.pas_dcr, false)?;
        t.cp = self.pas_number("並列容量", &self.pas_cp, false)?;
        let mut notes = vec![];
        if let Some(f0) = self.pas_number("自己共振周波数", &self.pas_srf, true)? {
            match t.kind {
                PassiveKind::Capacitor if t.esl.is_none() => {
                    let esl = passive::esl_from_srf(value, f0)?;
                    t.esl = Some(esl);
                    notes.push(format!("ESL は自己共振周波数から {} と求めました", passive::eng(esl)));
                }
                PassiveKind::Inductor | PassiveKind::Resistor if t.cp.is_none() => {
                    let cp = passive::cp_from_srf(value, f0)?;
                    t.cp = Some(cp);
                    notes.push(format!("並列容量は自己共振周波数から {} と求めました", passive::eng(cp)));
                }
                _ => {}
            }
        }
        let bias = self.pas_bias.trim().trim_end_matches('%').trim();
        if !bias.is_empty() {
            t.bias_loss = bias.parse::<f64>().map_err(|_| format!("DCバイアスで下がる量「{bias}」を % の数で入力してください (例: 30)"))? / 100.0;
        }
        let size = self.fp_preset.rsplit('_').next().filter(|_| !self.fp_preset.is_empty()).map(|s| format!("_{s}")).unwrap_or_default();
        let name = if self.name.trim().is_empty() { format!("{}_{}{size}", t.kind.prefix(), passive::eng(value)) } else { self.name.trim().to_string() };
        let g = t.generate(&name)?;
        self.name = name;
        self.prefix = t.kind.prefix().into();
        self.value = g.value_text;
        self.description = g.description;
        if self.category.trim().is_empty() || self.category == "ユーザー" {
            self.category = t.kind.label().into();
        }
        self.pins_text = if self.pas_polar && t.kind == PassiveKind::Capacitor { "1 + left\n2 - right" } else { "1 1 left\n2 2 right" }.into();
        self.sim = SimChoice::Subckt;
        self.sim_name = g.sim_name;
        self.sim_text = g.sim_text;
        self.sim_pins = g.sim_pins.join(" ");
        self.sim_limits = g.limits.join("\n");
        self.glyph = if self.pas_polar && t.kind == PassiveKind::Capacitor { "electrolytic" } else { t.kind.glyph() }.into();
        if self.fp_preset.is_empty() {
            self.pkg = PkgKind::Chip2;
        }
        let tail = if notes.is_empty() { String::new() } else { format!("。{}", notes.join("。")) };
        Ok(format!("ピンとモデルを作りました{tail}。このあと、パッケージを選び (0603 など)、定格 (耐圧など) をデータシートから入れて保存します"))
    }

    /// Fit ESR and ESL to the impedance table typed in the form.
    fn fit_from_table(&mut self) -> Result<String, String> {
        let c = self.pas_number("容量", &self.pas_value, false)?.ok_or("先に上の「値」に容量を入れてください (表の容量。DCバイアス時の表ならそのときの容量)")?;
        let mut pts = vec![];
        for (n, line) in self.pas_table.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',' || c == '|').filter(|s| !s.is_empty()).collect();
            let (f, z) = match (t.first().and_then(|s| parse_freq(s)), t.get(1).and_then(|s| parse_ohm(s))) {
                (Some(f), Some(z)) => (f, z),
                _ => return Err(format!("インピーダンスの表 {}行目: 「周波数 インピーダンス」の形で入力してください (例: 1M 0.05)", n + 1)),
            };
            pts.push((f, z));
        }
        let fit = passive::fit_cap(c, &pts)?;
        self.pas_esr = passive::eng(fit.esr);
        self.pas_esl = passive::eng(fit.esl);
        let mut m = format!("ESR = {}Ω、ESL = {}H と求めました (共振 {}Hz、表との差は最大 {:.0}%)。上の欄に入れたので、「部品を作る」を押すとモデルに入ります", self.pas_esr, self.pas_esl, passive::eng(fit.srf), fit.max_error_pct);
        for w in &fit.warnings {
            m.push_str(&format!("\n注意: {w}"));
        }
        Ok(m)
    }

    fn passive_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("チップ部品 (コンデンサ・抵抗・コイル) を、データシートの数値からつくる").id_salt("pas_section").default_open(!self.glyph.is_empty()).show(ui, |ui| {
            ui.small("容量などの値に、データシートにある ESR・ESL・DCR などを足すと、実物に近いモデルになります。空の欄は入れません。値だけでも作れます");
            ui.horizontal(|ui| {
                for k in [PassiveKind::Capacitor, PassiveKind::Resistor, PassiveKind::Inductor] {
                    ui.selectable_value(&mut self.pas_kind, k, k.label());
                }
            });
            let kind = self.pas_kind;
            egui::Grid::new("pas_grid").num_columns(2).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, label: &str, text: &mut String, tip: &str, hint: &str| {
                    ui.label(label).on_hover_text(tip);
                    ui.add(egui::TextEdit::singleline(text).desired_width(120.0).hint_text(hint)).on_hover_text(tip);
                    ui.end_row();
                };
                row(ui, &format!("値 ({})", kind.unit()), &mut self.pas_value, "コンデンサは容量 (例: 100n、4.7u)、抵抗は抵抗値 (例: 10k)、コイルはインダクタンス (例: 10u)", "100n");
                match kind {
                    PassiveKind::Capacitor => {
                        row(ui, "ESR (等価直列抵抗)", &mut self.pas_esr, "データシートの ESR か、インピーダンスの最小値。大きいほどリップルが大きく、発熱します。下の表から求めることもできます", "20m");
                        // for an electrolytic / tantalum only: there the impedance at 100 kHz is about the ESR
                        if self.pas_polar {
                            ui.label("インピーダンス Z (Ω)").on_hover_text("データシートの「インピーダンス」(100kHz の値) を入れて「ESR に入れる」を押すと、上の ESR に写します。電解コンデンサでは、100kHz のインピーダンスはほぼ ESR です (容量の分は、そのころには小さくなっています)。ただし ESR は周波数と温度で大きく変わります (120Hz の tanδ から求めると、もっと大きくなります)");
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.pas_z100k).desired_width(80.0).hint_text("0.3"));
                                if ui.small_button("→ ESR に入れる").on_hover_text("この値を ESR の欄に入れます (100kHz のときの値として)").clicked() {
                                    match self.pas_number("インピーダンス", &self.pas_z100k, false) {
                                        Ok(Some(z)) => self.pas_esr = passive::eng(z),
                                        Ok(None) => {}
                                        Err(e) => self.pas_message = Some(Err(e)),
                                    }
                                }
                            });
                            ui.end_row();
                        }
                        row(ui, "ESL (等価直列インダクタンス)", &mut self.pas_esl, "高い周波数で効いてくる直列のインダクタンス。小さいチップほど小さい (0603 で 0.5〜1nH ほど)。自己共振周波数から求めることもできます", "800p");
                        row(ui, "自己共振周波数 (Hz)", &mut self.pas_srf, "容量と ESL が共振する周波数。ESL が空のとき、ここから ESL を求めます。M は MHz (例: 30M)", "30M");
                        row(ui, "DCバイアスで下がる量 (%)", &mut self.pas_bias, "セラミックコンデンサは、直流の電圧をかけると容量が減ります。使う電圧での値をデータシートの「DCバイアス特性」の曲線から読み、減った割合を入れると、その分小さい容量で計算します", "30");
                    }
                    PassiveKind::Resistor => {
                        row(ui, "寄生インダクタンス (H)", &mut self.pas_esl, "高い周波数で効く直列のインダクタンス。チップ抵抗で 1〜2nH ほど", "2n");
                        row(ui, "並列容量 (F)", &mut self.pas_cp, "両端の間の容量。チップ抵抗で 0.05〜0.2pF ほど", "100f");
                    }
                    PassiveKind::Inductor => {
                        row(ui, "DCR (巻線抵抗)", &mut self.pas_dcr, "巻線の直流抵抗。損失と発熱の元になります", "100m");
                        row(ui, "並列容量 (F)", &mut self.pas_cp, "巻線の間の容量。自己共振周波数から求めることもできます", "1p");
                        row(ui, "自己共振周波数 (Hz)", &mut self.pas_srf, "コイルがこれより高い周波数でコンデンサのようになる限界。並列容量が空のとき、ここから求めます。M は MHz", "30M");
                    }
                }
            });
            if kind == PassiveKind::Capacitor {
                ui.label("インピーダンスの表 (データシートの周波数特性。1行に「周波数 インピーダンス」。周波数の M は MHz)");
                ui.add(egui::TextEdit::multiline(&mut self.pas_table).desired_rows(4).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text("100k 0.2\n1M 0.03\n10M 0.05\n100M 0.4"))
                    .on_hover_text("データシートのグラフから、いくつかの点を読み取って書きます。谷 (一番低い所) の前後も入れると確かです。例: 周波数 1M、インピーダンス 0.05 は「1M 0.05」");
                if ui.button("表から ESR と ESL を求める").on_hover_text("表に合うように、ESR と ESL を計算して上の欄に入れます。「値」の容量を使います").clicked() {
                    self.pas_message = Some(self.fit_from_table());
                }
            }
            if ui.button("部品を作る (ピンとモデルを入れる)").on_hover_text("ピン、記号、値、モデルを入れます。パッケージは下の「パッケージ」で選びます").clicked() {
                self.pas_message = Some(self.apply_passive());
            }
            match &self.pas_message {
                Some(Ok(m)) => {
                    ui.colored_label(Color32::from_rgb(60, 170, 90), m);
                }
                Some(Err(e)) => {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                }
                None => {}
            }
        });
    }


    /// Fill the pin list, the model and the description from the charger template.
    fn apply_template(&mut self) {
        match self.tpl.generate(&self.name) {
            Ok(g) => {
                let side = |s: Option<PinSide>| match s {
                    Some(PinSide::Left) => "left",
                    Some(PinSide::Right) => "right",
                    Some(PinSide::Top) => "top",
                    _ => "bottom",
                };
                self.pins_text = g.pins.iter().map(|p| format!("{} {} {}", p.number, p.name, side(p.side))).collect::<Vec<_>>().join("\n");
                self.sim = SimChoice::Subckt;
                self.sim_name = g.sim_name;
                self.sim_text = g.sim_text;
                self.sim_pins = g.sim_pins.join(" ");
                self.description = g.description;
                self.sim_limits = g.limits.join("\n");
                if self.category.trim().is_empty() || self.category == "ユーザー" {
                    self.category = "電源管理".into();
                }
                self.tpl_message = format!("ピン {} 本とモデルを作りました。このあと、下の「パッケージ」をデータシートの寸法に合わせ、ピン番号がデータシートの番号になるよう直して保存します", g.pins.len());
            }
            Err(e) => self.tpl_message = e,
        }
    }

    fn template_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("テンプレートからピンとモデルを作る (リチウムイオン充電IC)").id_salt("tpl_charger").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("例から始める:");
                if ui.small_button("TP4056 風").on_hover_text("リニア式、電流 = 1200 ÷ R_PROG。まず形を見るのに向いています").clicked() {
                    self.tpl = ChargerTemplate::default();
                }
                if ui.small_button("IP2332 風").on_hover_text("降圧スイッチング式、電流 = 0.35 + 2.75 ÷ R_ISET[kΩ]、入力 4.3〜6V、温度入力あり").clicked() {
                    self.tpl = ChargerTemplate {
                        topology: Topology::Buck,
                        setting: CurrentSetting::Resistor,
                        current: 2.0,
                        k: 2750.0,
                        offset: 0.35,
                        precharge_fraction: 0.2,
                        vin_min: 4.3,
                        vin_max: 6.0,
                        dropout: 0.1,
                        ntc: Some((0.3, 2.73)),
                        ..Default::default()
                    };
                }
            });
            let t = &mut self.tpl;
            ui.horizontal(|ui| {
                ui.label("方式");
                ui.selectable_value(&mut t.topology, Topology::Linear, "リニア")
                    .on_hover_text("入力から電池へ、電流をそのまま通します。回路は簡単ですが、入力と電池の電圧差が熱になります (TP4056 など)");
                ui.selectable_value(&mut t.topology, Topology::Buck, "降圧スイッチング")
                    .on_hover_text("コイルを使って電圧を下げながら充電します。効率が良く、大きな電流向き。LX 端子に外付けのコイルをつなぎます (TP5100・IP2332 など)");
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("セル数");
                ui.add(egui::DragValue::new(&mut t.cells).range(1..=8)).on_hover_text("電池を直列につなぐ数。満充電電圧 = セル数 × 1セルの電圧");
                ui.label("1セルの満充電電圧");
                ui.add(egui::DragValue::new(&mut t.cell_voltage).speed(0.01).range(2.0..=5.0).suffix(" V"))
                    .on_hover_text("リチウムイオンは 4.2V (高電圧タイプ 4.35V)、リン酸鉄リチウムは 3.6V");
                if ui.small_button("4.2V").on_hover_text("リチウムイオン電池の 1 セルの満充電電圧").clicked() {
                    t.cell_voltage = 4.2;
                }
                if ui.small_button("3.6V (リン酸鉄)").on_hover_text("リン酸鉄リチウム電池の 1 セルの満充電電圧").clicked() {
                    t.cell_voltage = 3.6;
                }
            });
            ui.horizontal(|ui| {
                ui.label("充電電流の決め方");
                ui.selectable_value(&mut t.setting, CurrentSetting::Fixed, "決まった値").on_hover_text("データシートに電流が1つだけ書かれている場合");
                ui.selectable_value(&mut t.setting, CurrentSetting::Resistor, "抵抗で設定 (PROG 端子)")
                    .on_hover_text("端子につなぐ抵抗で電流を決める場合。データシートの式 I = 定数 + k ÷ R に合わせます");
            });
            match t.setting {
                CurrentSetting::Fixed => {
                    ui.horizontal(|ui| {
                        ui.label("充電電流");
                        ui.add(egui::DragValue::new(&mut t.current).speed(0.01).range(0.0..=20.0).suffix(" A")).on_hover_text("充電電流 (A)。データシートに書かれた値を入れます");
                    });
                }
                CurrentSetting::Resistor => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("電流 I[A] =");
                        ui.add(egui::DragValue::new(&mut t.offset).speed(0.01).range(0.0..=20.0)).on_hover_text("式の定数の部分 (無ければ 0)。例: IP2332 は 0.35");
                        ui.label("+ k ÷ R[Ω]    k =");
                        ui.add(egui::DragValue::new(&mut t.k).speed(1.0).range(0.0..=1e6))
                            .on_hover_text("TP4056 は 1200 (1.2kΩ で 1A)。式が「2.75 ÷ R[kΩ]」なら、Ω に直して k = 2750");
                        ui.label("PROG 開放時の電流");
                        ui.add(egui::DragValue::new(&mut t.current).speed(0.01).range(0.0..=20.0).suffix(" A"))
                            .on_hover_text("PROG 端子に何もつながないときの電流。0 なら充電しません。IP2332 は 2A");
                    });
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("プリチャージ: 1セルがこの電圧未満");
                ui.add(egui::DragValue::new(&mut t.precharge_per_cell).speed(0.01).range(0.0..=5.0).suffix(" V"))
                    .on_hover_text("空に近い電池は、小さな電流でそっと充電します。0 ならプリチャージなし");
                ui.label("電流の割合");
                ui.add(egui::DragValue::new(&mut t.precharge_fraction).speed(0.01).range(0.0..=1.0)).on_hover_text("0.1 = 設定電流の 10%");
                ui.label("終了電流の割合");
                ui.add(egui::DragValue::new(&mut t.termination_fraction).speed(0.01).range(0.0..=0.9))
                    .on_hover_text("定電圧のあと、電流がこの割合まで減ったら満充電とみなします (0.1 = 10%)。状態表示の切替に使います");
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("入力電圧");
                ui.add(egui::DragValue::new(&mut t.vin_min).speed(0.05).range(0.5..=60.0).prefix("下限 ").suffix(" V"))
                    .on_hover_text("これを下回ると動きません (低電圧ロックアウト)");
                ui.add(egui::DragValue::new(&mut t.vin_max).speed(0.05).range(0.0..=60.0).prefix("上限 ").suffix(" V"))
                    .on_hover_text("これを超えると止まります (過電圧保護)。0 なら上限なし");
                ui.add(egui::DragValue::new(&mut t.dropout).speed(0.01).range(0.0..=5.0).prefix("電池より ").suffix(" V 以上高い"))
                    .on_hover_text("入力が電池の電圧よりこれだけ高くないと充電できません");
            });
            ui.horizontal_wrapped(|ui| {
                let mut ntc = t.ntc.is_some();
                if ui
                    .checkbox(&mut ntc, "温度入力 (NTC 端子)")
                    .on_hover_text("電池の温度を見る端子。サーミスタ (NTC) をつなぎ、温度が低すぎる・高すぎるときは充電を止めます")
                    .changed()
                {
                    t.ntc = ntc.then_some((0.3, 2.73));
                }
                if let Some((lo, hi)) = &mut t.ntc {
                    ui.add(egui::DragValue::new(lo).speed(0.01).range(0.0..=5.0).prefix("正常な電圧 ").suffix(" V から")).on_hover_text("NTC 端子の電圧がこの値より低いと、温度が高すぎる (または低すぎる) として充電を止めます");
                    ui.add(egui::DragValue::new(hi).speed(0.01).range(0.0..=5.0).suffix(" V まで"))
                        .on_hover_text("NTC に 100µA を流したときの電圧がこの範囲なら充電します (電圧 = 100µA × NTC の抵抗)");
                }
                ui.checkbox(&mut t.status, "状態出力 (CHRG / STDBY)").on_hover_text("充電中は CHRG、満充電は STDBY が Low になる出力 (LED をつなぐ端子)");
            });
            ui.horizontal(|ui| {
                if ui
                    .button("ピンとモデルを作る")
                    .on_hover_text("上の数字から、ピンの一覧とシミュレーションモデルを作って、下の欄に書き込みます (すでにある内容は置き換わります)")
                    .clicked()
                {
                    self.apply_template();
                }
                if !self.tpl_message.is_empty() {
                    ui.small(&self.tpl_message);
                }
            });
        }).header_response.on_hover_text("データシートの数字を入れるだけで、充電ICの「外から見た動き」のモデルを作ります。市販のSPICEモデルが無いICに使えます。数字は、データシートの「電気的特性」の表から拾います");
    }

    /// The ① basic information tab (name, category, prefix, value, description, data sheet URL) and the easy templates.
    fn tab_basic(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("part_basic").num_columns(2).show(ui, |ui| {
            ui.label("部品名");
            ui.text_edit_singleline(&mut self.name).on_hover_text("部品の名前 (例: LM358、2N3904)。部品の一覧や回路図の値に出ます");
            ui.end_row();
            ui.label("カテゴリ");
            ui.text_edit_singleline(&mut self.category).on_hover_text("一覧のグループ名 (例: オペアンプ、トランジスタ)");
            ui.end_row();
            ui.label("部品番号の頭文字");
            ui.text_edit_singleline(&mut self.prefix).on_hover_text("回路図で付く部品番号の頭文字。IC は U、トランジスタは Q、抵抗は R、ダイオードは D");
            ui.end_row();
            ui.label("値 (省略で部品名)");
            ui.text_edit_singleline(&mut self.value).on_hover_text("回路図に表示する値。空なら部品名が入ります");
            ui.end_row();
            ui.label("説明");
            ui.text_edit_singleline(&mut self.description).on_hover_text("一覧で部品にポインタを置いたときに出る説明");
            ui.end_row();
            ui.label("データシートURL");
            ui.text_edit_singleline(&mut self.datasheet).on_hover_text("データシート (メーカーの仕様書) の URL。あとで見返すためのメモです");
            ui.end_row();
        });

        ui.add_space(6.0);
        ui.label(egui::RichText::new("かんたん作成 (テンプレート)").strong());
            ui.small("チップ部品や充電ICは、データシートの数字を入れるだけで、下の欄を自動で埋められます");
            self.passive_ui(ui);
            self.template_ui(ui);
    }

    /// The tab of the data sheet's ratings.
    fn tab_ratings(&mut self, ui: &mut egui::Ui) {
            ui.label("部品を追加するときは、メーカーのデータシートを開いて調べます。見る表は 2 つです: 「絶対最大定格 (Absolute Maximum Ratings)」 = 超えると壊れる値、「電気的特性 (Electrical Characteristics)」 = 動作中の代表的な値。調べて入れておくと、回路のシミュレーション結果と比べて、超えそうなときに警告できます");
            // the kind chosen at the top decides which items to look up; only an IC (op-amp, regulator ...) or no choice
            // at all leaves the question open
            let decided = self.checklist_for_type();
            if let Some(k) = decided {
                self.rt_kind = k;
            }
            ui.horizontal(|ui| {
                if decided.is_some() {
                    ui.label(format!("調べる項目: {}", CHECKLISTS[self.rt_kind.min(CHECKLISTS.len() - 1)].0)).on_hover_text("上で選んだ部品の種類に合わせた項目です。変えるときは、いちばん上の「部品の種類」を選び直します");
                } else {
                    ui.label(if self.ptype == Some(PartType::Ic) { "どの IC?" } else { "部品の種類" });
                    egui::ComboBox::from_id_salt("rt_kind").selected_text(CHECKLISTS[self.rt_kind.min(CHECKLISTS.len() - 1)].0).show_ui(ui, |ui| {
                        for (k, (name, _, _)) in CHECKLISTS.iter().enumerate() {
                            ui.selectable_value(&mut self.rt_kind, k, *name);
                        }
                    }).response.on_hover_text("選ぶと、その種類で調べるとよい項目が出ます");
                }
                if ui.button("調べる項目のチェックリストを入れる").on_hover_text("下の「そのほかの情報」に、調べる項目を並べます。データシートを見て、値を書き込んでください (すでに書いた行は、そのまま残ります)").clicked() {
                    self.rt_rows = merge_checklist(&self.rt_rows, self.rt_kind);
                }
            });
            ui.label(egui::RichText::new(CHECKLISTS[self.rt_kind.min(CHECKLISTS.len() - 1)].1).weak());
            egui::Grid::new("part_ratings_grid").num_columns(2).show(ui, |ui| {
                ui.label("最大電圧 [V]").on_hover_text("部品にかけてよい最大の電圧。ダイオードなら逆耐圧 (VRRM)、トランジスタなら Vce の最大 (VCEO)、MOSFET なら Vds の最大");
                ui.add(egui::TextEdit::singleline(&mut self.rt_v).desired_width(80.0)).on_hover_text("例: 40 / 1000");
                ui.end_row();
                ui.label("最大電流 [A]").on_hover_text("流してよい最大の連続電流 (ダイオードなら平均順電流 IF、トランジスタなら Ic の最大)");
                ui.add(egui::TextEdit::singleline(&mut self.rt_i).desired_width(80.0)).on_hover_text("例: 1 / 200m");
                ui.end_row();
                ui.label("最大電力 [W]").on_hover_text("部品が熱にしてよい最大の電力 (許容損失 Ptot)。周囲温度が高いと下がります。データシートの条件を確認します");
                ui.add(egui::TextEdit::singleline(&mut self.rt_p).desired_width(80.0)).on_hover_text("例: 0.5 / 625m / 1");
                ui.end_row();
            });
            // only where it belongs: an electrolytic / tantalum capacitor, a part that already has such figures, or
            // a part whose kind was not chosen
            let filled = !self.rt_ripple.trim().is_empty() || !self.rt_life_h.trim().is_empty();
            let polar = self.pas_polar || filled;
            if polar || self.ptype.is_none() {
            egui::CollapsingHeader::new("電解コンデンサの項目 (リップル電流と寿命)").id_salt("rt_electrolytic").default_open(polar).show(ui, |ui| {
                ui.small("入れると、過渡解析の結果に、このコンデンサに流れたリップル電流が定格を超えていないか、推定寿命がどれくらいかが出ます。データシートの「定格リップル電流」「寿命 (耐久性)」「リップル電流の周波数補正係数」の表から写します");
                egui::Grid::new("rt_electrolytic_grid").num_columns(2).show(ui, |ui| {
                    ui.label("定格リップル電流 [A rms]").on_hover_text("コンデンサに流してよい、交流分の実効値 (データシートの Rated Ripple Current)。mA rms で書いてあるときは 540m のように入れます。最高温度 (105℃ など) での値です");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.rt_ripple).desired_width(80.0).hint_text("540m")).on_hover_text("例: 540m / 1.2。データシートの表の、容量と電圧に合う行の値");
                        ui.label("周波数");
                        ui.add(egui::TextEdit::singleline(&mut self.rt_ripple_f).desired_width(60.0)).on_hover_text("その値が決められている周波数。データシートの表の見出しにあります (たいてい 100kHz。120Hz のこともあります)");
                    });
                    ui.end_row();
                    ui.label("周波数の補正係数").on_hover_text("リップル電流の定格に掛ける係数の表 (Multiplier for Ripple Current)。低い周波数ほど、流してよい電流が小さくなります。1行に「周波数 係数」");
                    ui.add(egui::TextEdit::multiline(&mut self.rt_ripple_table).desired_rows(4).desired_width(160.0).hint_text("120 0.55\n1k 0.77\n10k 0.94\n100k 1.0")).on_hover_text("例: 120 0.55 / 1k 0.77 / 10k 0.94 / 100k 1.0。容量の範囲ごとに表が分かれているときは、この部品の容量の行を入れます。空のときは、周波数によらず定格のままです");
                    ui.end_row();
                    ui.label("寿命 [時間]").on_hover_text("定格電圧・定格リップル電流・最高温度で使ったときの寿命 (Endurance)。ケースの大きさで違うことがあります (例: 6000 / 10000)");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.rt_life_h).desired_width(80.0).hint_text("6000")).on_hover_text("例: 6000 / 10000");
                        ui.label("温度 [℃]");
                        ui.add(egui::TextEdit::singleline(&mut self.rt_life_t).desired_width(50.0)).on_hover_text("その寿命が決められている温度。最高使用温度 (105 など)");
                    });
                    ui.end_row();
                });
            });
            }
            ui.label("そのほかの情報 (1行に1つ。「項目 | 値 | 条件」。値が空の行は、まだ調べていない項目として残ります)");
            ui.add(egui::TextEdit::multiline(&mut self.rt_rows).desired_rows(5).desired_width(f32::INFINITY).hint_text("順方向電圧 | 0.9V | IF = 1A"))
                .on_hover_text("定格・代表値を、データシートのまま書き写します。条件 (電流や温度) も一緒に書くと、あとで比べやすくなります。部品の一覧でポインタを置いたときに出ます");
            let (max_given, rows_given, rows_open) = self.ratings_progress();
            let summary = format!("入力済み: 最大定格 {max_given} / 3 項目、そのほかの情報 {rows_given} 行{}", if rows_open > 0 { format!(" (まだ調べていない項目 {rows_open} 行)") } else { String::new() });
            if max_given == 0 && rows_given == 0 {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "データシートの情報が空です。空のまま保存もできますが、定格のチェックと「部品選びのヒント」が使えません。データシートを開いて、調べてみましょう");
            } else {
                ui.label(summary);
            }
            if self.datasheet.trim().is_empty() && (max_given > 0 || rows_given > 0) {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "データシートの URL が空です。調べた元を残しておくと、あとで見返せます (上の「データシートURL」)");
            }
    }

    /// The tab of the simulation model.
    fn tab_sim(&mut self, ui: &mut egui::Ui) {
            ui.label("シミュレーションモデル (部品の特性)");
            ui.small("メーカーのサイトなどで見つけた SPICE モデル (`.model` / `.subckt`) をそのまま貼ると、種類・名前・ピンを自動で入力します");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.sim, SimChoice::None, "なし").on_hover_text("シミュレーションしない部品にします (基板づくりだけに使う部品)");
                ui.selectable_value(&mut self.sim, SimChoice::Model, ".model (ダイオード/トランジスタ等)").on_hover_text("ダイオードやトランジスタなどの 1 素子を `.model` 文で書いたモデル");
                ui.selectable_value(&mut self.sim, SimChoice::Subckt, ".subckt (IC マクロモデル)").on_hover_text("IC の中身を、複数の素子の組み合わせ (`.subckt`) で書いたモデル");
            });
            let mut autofill = false;
            let pasted = ui.add(
                egui::TextEdit::multiline(&mut self.sim_text)
                    .desired_rows(if self.sim == SimChoice::None { 3 } else { 5 })
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("ここに SPICE モデルを貼る: .model D1 D(IS=1n)  /  .subckt NAME 1 2 3 ... .ends"),
            ).on_hover_text("貼り付けると、モデルの種類・名前・端子からフォームを自動で埋めます。ピン表を自分で書いてあるときは、その名前 (A K / C B E など) に合わせて対応づけます");
            // a paste into an empty model box fills the form right away
            if pasted.changed() && self.sim == SimChoice::None && !self.sim_text.trim().is_empty() {
                autofill = true;
            }
            ui.horizontal(|ui| {
                if ui.button("貼ったモデルから自動入力").on_hover_text("上の SPICE 文を読んで、種類・モデル名・素子記号・ピン表・ノード順を入力します。何度押しても大丈夫です").clicked() {
                    autofill = true;
                }
                if ui.button("SPICEファイルを読む…").on_hover_text("メーカーからダウンロードした .lib / .mod / .sub / .cir / .txt のファイルを読み込み、上の欄に入れて自動入力します").clicked()
                    && let Some(p) = rfd::FileDialog::new().add_filter("SPICE モデル", &["lib", "mod", "sub", "cir", "spice", "txt", "inc"]).pick_file()
                {
                    match std::fs::read_to_string(&p) {
                        Ok(t) => {
                            self.sim_text = t;
                            autofill = true;
                        }
                        Err(e) => self.spice_message = Some(Err(format!("ファイルを読めませんでした: {e}。文字コードが UTF-8 でないときは、テキストエディタで開いて貼り付けてください"))),
                    }
                }
            });
            if autofill {
                self.spice_message = Some(self.autofill_from_spice());
            }
            match &self.spice_message {
                Some(Ok(m)) => {
                    ui.colored_label(Color32::from_rgb(60, 170, 90), m);
                }
                Some(Err(e)) => {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                }
                None => {}
            }
            if self.sim != SimChoice::None {
                ui.horizontal(|ui| {
                    if self.sim == SimChoice::Model {
                        ui.label("素子記号");
                        ui.add(egui::TextEdit::singleline(&mut self.sim_element).desired_width(30.0)).on_hover_text("素子の種類の記号。ダイオード D、バイポーラトランジスタ Q、MOSFET M、スイッチ S");
                    }
                    ui.label("モデル名");
                    ui.add(egui::TextEdit::singleline(&mut self.sim_name).desired_width(120.0)).on_hover_text("`.model` や `.subckt` に付けた名前。上の本文と同じ名前にします");
                    ui.label("ノード順のピン番号");
                    ui.add(egui::TextEdit::singleline(&mut self.sim_pins).desired_width(160.0).hint_text("例: A K")).on_hover_text("モデルの端子の順番に、部品のピン番号を並べます。例: ダイオードなら A K (アノード カソード)、トランジスタなら C B E の順");
                });
                ui.label("このモデルの限界 (1行に1つ。実物との違いを書くと、部品を選んだときに表示されます)");
                ui.add(
                    egui::TextEdit::multiline(&mut self.sim_limits)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("例: 温度による変化は扱いません。
    接合容量は扱いません。"),
                );
            }

    }

    /// The tab of the pins.
    fn tab_pins(&mut self, ui: &mut egui::Ui) {
            ui.label("ピン (1行に「番号 名前 [left|right|top|bottom]」。向き省略で前半が左・後半が右。NC は未接続ピン扱い)");
            ui.add(egui::TextEdit::multiline(&mut self.pins_text).desired_rows(6).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace)).on_hover_text("ピンの定義を1行に1つ書きます。例: `1 IN+ left` (番号 名前 向き)。向きは left / right / top / bottom で、省略すると前半が左、後半が右になります。NC は未接続のピンです");
    }

    /// The tab of the board shape: package, thermal pad, body, extra pads and holes.
    fn tab_package(&mut self, ui: &mut egui::Ui) {
            ui.label("パッケージ (データシートの推奨ランドパターン寸法 mm。パッド数はピン数に合わせます)");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.pkg, PkgKind::Chip2, "2端子チップ").on_hover_text("抵抗・コンデンサ・ダイオードなど、足が2つのチップ部品");
                ui.selectable_value(&mut self.pkg, PkgKind::Dual, "デュアル(SOIC/DIP)").on_hover_text("足が左右の2列に並ぶ IC (SOIC、DIP など)");
                ui.selectable_value(&mut self.pkg, PkgKind::Quad, "クアッド(QFP)").on_hover_text("足が四方に並ぶ IC (QFP、QFN など)");
                ui.selectable_value(&mut self.pkg, PkgKind::Row, "1列(ヘッダ)").on_hover_text("足が1列に並ぶ部品 (ピンヘッダ、コネクタなど)");
            });
            ui.horizontal(|ui| {
                ui.label("標準のフットプリント");
                let shown = if self.fp_preset.is_empty() { "(下の寸法を使う)".to_string() } else { self.fp_preset.clone() };
                egui::ComboBox::from_id_salt("fp_preset").selected_text(shown).width(200.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.fp_preset, String::new(), "(下の寸法を使う)");
                    for n in pcb_core::footprints::names() {
                        ui.selectable_value(&mut self.fp_preset, n.clone(), n);
                    }
                }).response.on_hover_text("0603 や 0805 のチップ部品、DIP・ピンヘッダなど、あらかじめ用意された形です。選ぶと、下の寸法の代わりにこの形を使います。ピン番号は、形ごとのピン名 (チップは 1 と 2、ダイオードは K と A、トランジスタは E B C など) に合わせます");
            });
            let dv = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
                ui.add(egui::DragValue::new(v).speed(0.01).range(0.0..=100.0).prefix(format!("{label} ")).suffix(" mm")).on_hover_text(pkg_tip(label));
            };
            ui.horizontal_wrapped(|ui| {
                dv(ui, "ピッチ", &mut self.pitch);
                match self.pkg {
                    PkgKind::Chip2 => {
                        dv(ui, "パッド長", &mut self.len);
                        dv(ui, "パッド幅", &mut self.width);
                    }
                    PkgKind::Dual => {
                        dv(ui, "列間隔", &mut self.row);
                        dv(ui, "パッド長", &mut self.len);
                        dv(ui, "パッド幅", &mut self.width);
                        ui.checkbox(&mut self.tht, "スルーホール(DIP)").on_hover_text("足を基板の穴に通す形 (DIP) にします。オフなら面実装 (表面にはんだ付け)");
                        if self.tht {
                            dv(ui, "穴径", &mut self.drill);
                        }
                    }
                    PkgKind::Quad => {
                        dv(ui, "対向パッド間隔", &mut self.span);
                        dv(ui, "パッド長", &mut self.len);
                        dv(ui, "パッド幅", &mut self.width);
                    }
                    PkgKind::Row => {
                        dv(ui, "パッド径", &mut self.pad_d);
                        dv(ui, "穴径", &mut self.drill);
                    }
                }
            });
            if matches!(self.pkg, PkgKind::Dual | PkgKind::Quad) {
                ui.checkbox(&mut self.thermal_on, "放熱パッド (ICの裏の大きなパッド) を付ける").on_hover_text("QFN・ESOP・SOT-223 などの、裏や中央にある大きなパッド。ここで大きさとピン番号を入れると、パッケージの中心に置きます。ピン表にもそのピンの行を足してください (例: 9 GND)。発注前チェックで、サーマルビアが足りないと注意します");
                if self.thermal_on {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("ピン番号");
                        ui.add(egui::TextEdit::singleline(&mut self.thermal_pin).desired_width(40.0).hint_text("9")).on_hover_text("放熱パッドのピン番号。ピン表にある番号にします (GND につなぐことが多い)");
                        ui.add(egui::DragValue::new(&mut self.thermal_w).speed(0.01).range(0.1..=50.0).prefix("幅 ").suffix(" mm")).on_hover_text("データシートの推奨ランドパターンの、放熱パッドの幅");
                        ui.add(egui::DragValue::new(&mut self.thermal_h).speed(0.01).range(0.1..=50.0).prefix("長さ ").suffix(" mm")).on_hover_text("データシートの推奨ランドパターンの、放熱パッドの長さ");
                    });
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(&mut self.height).speed(0.01).range(0.0..=100.0).prefix("高さ ").suffix(" mm")).on_hover_text("データシートの側面図の、基板からの本体の高さ。0 のままなら、3D表示は名前から推測します。ケースに入るかの確認と、3D表示の本体の高さに使います");
                ui.small("側面図の高さ (0 = 未入力)");
            });
            ui.checkbox(&mut self.body_on, "本体の外形 (上面図の幅×長さ) を描く").on_hover_text("データシートの外形寸法 (3面図の上面図) の、部品の本体の大きさを入れます。基板のシルク (白い線) に本体の輪郭が出て、向きの印 (1番ピンの点・カソードの棒・＋) も付きます。パッドに重なる所は自動で避けます。3D表示でも、この幅×長さ×上の高さの本体になります");
            if self.body_on {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.body_circle, false, "四角").on_hover_text("四角い本体 (IC、チップ部品)");
                    ui.selectable_value(&mut self.body_circle, true, "円").on_hover_text("丸い本体 (ラジアルの電解コンデンサ、LED)");
                    let dv = |ui: &mut egui::Ui, label: &str, v: &mut f64, lo: f64| {
                        ui.add(egui::DragValue::new(v).speed(0.01).range(lo..=100.0).prefix(format!("{label} ")).suffix(" mm")).on_hover_text(body_tip(label));
                    };
                    dv(ui, if self.body_circle { "直径" } else { "幅" }, &mut self.body_w, 0.1);
                    if !self.body_circle {
                        dv(ui, "長さ", &mut self.body_h, 0.1);
                    }
                    dv(ui, "中心X", &mut self.body_x, -100.0);
                    dv(ui, "中心Y", &mut self.body_y, -100.0);
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("向きの印");
                    for (m, label, tip) in [
                        (BodyMark::None, "なし", "印を付けません"),
                        (BodyMark::Dot, "点", "本体の角の内側に点を打ちます。ICの1番ピンの印です"),
                        (BodyMark::Bar, "棒", "本体の端の近くに線を引きます。ダイオードのカソードや、LEDの平らな側の印です"),
                        (BodyMark::Plus, "＋", "極性のあるコンデンサの ＋ 側の印です"),
                    ] {
                        ui.selectable_value(&mut self.body_mark, m, label).on_hover_text(tip);
                    }
                    if self.body_mark != BodyMark::None {
                        ui.label("のピン");
                        ui.add(egui::TextEdit::singleline(&mut self.body_pin).desired_width(40.0).hint_text("1")).on_hover_text("印を付けるピンの番号。空なら最初のパッド (1番ピン)。ダイオードのカソードなら K、電解コンデンサの＋なら＋側のピン番号");
                    }
                });
            }
            egui::CollapsingHeader::new("追加のパッドと穴 (USBの固定脚・位置決めの穴・長穴)").id_salt("part_extras").default_open(!self.extras.trim().is_empty()).show(ui, |ui| {
                ui.small("USB・DCジャック・ピンソケットなどは、信号のピンのほかに、基板に固定するための脚や穴があります。ここに足すと、部品を置いたとき一緒に付き、部品を動かしたり回したりするとついてきます。製造ファイルのドリルにも出ます");
                ui.small(EXTRAS_HELP);
                ui.horizontal_wrapped(|ui| {
                    ui.label("例を足す:");
                    for (label, line, tip) in EXTRAS_EXAMPLES {
                        if ui.small_button(*label).on_hover_text(format!("{tip}
    数字は例です。データシートの推奨ランドパターンの寸法に直してください")).clicked() {
                            if !self.extras.is_empty() && !self.extras.ends_with('\n') {
                                self.extras.push('\n');
                            }
                            self.extras.push_str(line);
                        }
                    }
                });
                ui.add(egui::TextEdit::multiline(&mut self.extras).desired_rows(4).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text("脚 SH -6.5 0 2.0 3.0 1.2"))
                    .on_hover_text("追加のパッドと穴を1行に1つ書きます。# で始まる行は無視します。上の説明のとおりの形で書きます");
            });
    }

    /// Draws the form. Returns a finished definition when the user presses save.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<PartDef> {
        let built = self.to_def();
        self.kind_bar(ui);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (tab, label, tip) in FORM_TABS {
                let warn = match tab {
                    FormTab::Basic => self.name.trim().is_empty(),
                    FormTab::Pins => self.parse_pins().is_err(),
                    FormTab::Package => built.is_err() || built.as_ref().is_ok_and(|d| d.footprint().is_err()),
                    _ => false,
                };
                let text = if warn { format!("⚠ {label}") } else { (*label).to_string() };
                ui.selectable_value(&mut self.tab, *tab, egui::RichText::new(text).size(15.0)).on_hover_text(*tip);
            }
        });
        ui.separator();
        let h = 470.0;
        let mut saved = None;
        ui.horizontal_top(|ui| {
            let right_w = 370.0;
            // the window may be dragged wider; the form keeps to a width where nothing stretches across the screen
            let total = ui.available_width().clamp(760.0, 1040.0);
            let left_w = total - right_w - 20.0;
            ui.allocate_ui_with_layout(Vec2::new(left_w, h), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(left_w);
                egui::ScrollArea::vertical().id_salt("part_form_tab").max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                    match self.tab {
                        FormTab::Basic => self.tab_basic(ui),
                        FormTab::Pins => self.tab_pins(ui),
                        FormTab::Package => self.tab_package(ui),
                        FormTab::Sim => self.tab_sim(ui),
                        FormTab::Ratings => self.tab_ratings(ui),
                    }
                });
            });
            ui.separator();
            ui.allocate_ui_with_layout(Vec2::new(right_w, h), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(right_w);
                saved = self.preview_pane(ui, &built);
            });
        });
        saved
    }

    /// The first question: what kind of part is this. Fills pins, package, body and symbol with a starting point.
    fn kind_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("部品の種類").strong());
            for (t, label, tip) in PART_TYPES {
                if ui.selectable_label(self.ptype == Some(*t), egui::RichText::new(*label).size(15.0)).on_hover_text(*tip).clicked() {
                    self.apply_type(*t);
                }
            }
        });
        match self.ptype {
            Some(PartType::Transistor) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("どの種類?");
                    for (k, label, tip) in [
                        ("npn", "NPN", "ベース電流でコレクタ→エミッタの電流を制御する。負荷を GND 側で切り替える"),
                        ("pnp", "PNP", "NPN と電流の向きが逆。電源側で切り替える"),
                        ("nmos", "Nch MOSFET", "ゲート電圧で制御。大電流のスイッチに向く"),
                        ("pmos", "Pch MOSFET", "N チャネルと極性が逆。電源側で切り替える"),
                    ] {
                        if ui.selectable_label(self.glyph == k, label).on_hover_text(tip).clicked() {
                            self.set_transistor(k);
                        }
                    }
                });
            }
            Some(PartType::Diode) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("どの種類?");
                    for (k, label, tip) in [
                        ("diode", "整流・スイッチング", "電流を一方向に流す。記号は三角と線"),
                        ("schottky", "ショットキー", "順方向の電圧降下が小さい。棒の両端が S 形の記号"),
                        ("zener", "ツェナー", "逆向きで電圧を一定に保つ。棒の両端が Z 形の記号"),
                    ] {
                        if ui.selectable_label(self.glyph == k, label).on_hover_text(tip).clicked() {
                            self.glyph = k.into();
                        }
                    }
                });
            }
            Some(PartType::Capacitor) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("どの種類?");
                    for (k, label, tip) in [
                        ("ceramic", "積層セラミック", "チップ部品。小さくて高い周波数に強い。極性なし。電圧がかかると容量が減ります"),
                        ("electrolytic", "電解", "リード付きの円筒形。容量が大きく、電源の平滑に使う。極性があり、ESR は大きめ"),
                        ("tantalum", "タンタル", "小さくて容量が大きい。極性があり、耐圧に余裕を見ないと壊れます"),
                        ("film", "フィルム", "リード付きの箱形。極性なし。ESR・ESL が小さく、容量が安定します"),
                    ] {
                        if ui.selectable_label(self.ptype_sub == k, label).on_hover_text(tip).clicked() {
                            self.set_capacitor(k);
                        }
                    }
                });
                ui.small("名前は、種類の頭文字から始めると、部品一覧の「コンデンサ」の木で種類ごとに分かれます (電解 CE_ / タンタル CT_ / フィルム CF_)。値は「① 基本」のかんたん作成で入れます");
            }
            Some(t @ (PartType::Resistor | PartType::Inductor)) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("どの形?");
                    for (k, label, tip) in [("chip", "チップ", "面実装の小さい部品 (0603・0805 など)"), ("leaded", "リード付き", "足を基板の穴に通す部品")] {
                        if ui.selectable_label(self.ptype_sub == k, label).on_hover_text(tip).clicked() {
                            self.set_resistor_or_inductor(t, k);
                        }
                    }
                });
            }
            _ => {}
        }
    }

    /// Shows one tab ("basic", "pins", "package", "sim", "ratings"), for scripts and screenshots.
    pub fn show_tab(&mut self, name: &str) {
        if let Some((t, _, _)) = FORM_TABS.iter().zip(["basic", "pins", "package", "sim", "ratings"]).find(|(_, n)| *n == name).map(|(t, _)| t) {
            self.tab = *t;
        }
    }

    /// The checklist of what to look up that the kind of part chosen at the top settles (None = still open: an IC
    /// may be an op-amp or a regulator, or no kind was chosen).
    fn checklist_for_type(&self) -> Option<usize> {
        let by_name = |head: &str| CHECKLISTS.iter().position(|c| c.0.starts_with(head));
        match self.ptype? {
            PartType::Ic => None,
            PartType::Transistor => by_name(if matches!(self.glyph.as_str(), "nmos" | "pmos") { "MOSFET" } else { "トランジスタ" }),
            PartType::Diode => by_name(if self.glyph == "zener" { "ツェナー" } else { "ダイオード" }),
            PartType::Capacitor if self.pas_polar => by_name("電解コンデンサ"),
            PartType::Capacitor | PartType::Resistor | PartType::Inductor | PartType::Connector => by_name("そのほか"),
        }
    }

    /// A form that starts from this kind of part ("ic", "transistor", "diode", "chip", "connector"), for scripts and screenshots.
    pub fn for_type(name: &str) -> Self {
        let mut f = PartForm::default();
        // a kind of capacitor by its own name
        if let Some(k) = ["ceramic", "electrolytic", "tantalum", "film"].into_iter().find(|k| *k == name) {
            f.set_capacitor(k);
            return f;
        }
        if let Some((t, _, _)) = PART_TYPES.iter().find(|(t, _, _)| format!("{t:?}").eq_ignore_ascii_case(name)) {
            f.apply_type(*t);
        }
        f
    }

    fn apply_type(&mut self, t: PartType) {
        self.ptype = Some(t);
        self.fp_preset.clear();
        self.thermal_on = false;
        self.extras.clear();
        self.body_pin.clear();
        match t {
            PartType::Ic => {
                self.prefix = "U".into();
                self.glyph.clear();
                self.pins_text = "1 IN\n2 GND\n3 OUT\n4 VCC".into();
                self.pkg = PkgKind::Dual;
                self.tht = false;
                (self.pitch, self.row, self.len, self.width) = (1.27, 5.4, 1.55, 0.6);
                self.body_on = true;
                self.body_circle = false;
                self.body_mark = BodyMark::Dot;
                self.fit_body();
                self.tab = FormTab::Pins;
            }
            PartType::Transistor => {
                self.prefix = "Q".into();
                self.pkg = PkgKind::Row;
                (self.pitch, self.pad_d, self.drill) = (2.54, 1.5, 0.8);
                self.body_on = false;
                self.set_transistor("npn");
                self.tab = FormTab::Pins;
            }
            PartType::Diode => {
                self.prefix = "D".into();
                self.glyph = "diode".into();
                self.pins_text = "1 K right\n2 A left".into();
                self.pkg = PkgKind::Row;
                (self.pitch, self.pad_d, self.drill) = (7.62, 1.8, 0.9);
                self.body_on = true;
                self.body_circle = false;
                (self.body_w, self.body_h, self.body_x, self.body_y) = (3.8, 1.8, 0.0, 0.0);
                self.body_mark = BodyMark::Bar;
                self.body_pin = "1".into();
                self.tab = FormTab::Pins;
            }
            PartType::Capacitor => {
                self.set_capacitor("ceramic");
                self.tab = FormTab::Basic;
            }
            PartType::Resistor | PartType::Inductor => {
                self.set_resistor_or_inductor(t, "chip");
                self.tab = FormTab::Basic;
            }
            PartType::Connector => {
                self.prefix = "J".into();
                self.glyph.clear();
                self.pins_text = "1 1\n2 2\n3 3\n4 4".into();
                self.pkg = PkgKind::Row;
                (self.pitch, self.pad_d, self.drill) = (2.54, 1.7, 1.0);
                self.body_on = true;
                self.body_circle = false;
                (self.body_w, self.body_h, self.body_x, self.body_y) = (10.16, 2.5, 0.0, 0.0);
                self.body_mark = BodyMark::Dot;
                self.tab = FormTab::Pins;
            }
        }
    }

    /// Starting points for the kinds of capacitor: the footprint, the polarity, the height and the name prefix.
    fn set_capacitor(&mut self, kind: &'static str) {
        self.ptype = Some(PartType::Capacitor);
        self.ptype_sub = kind;
        self.pas_kind = PassiveKind::Capacitor;
        self.prefix = "C".into();
        self.pas_polar = matches!(kind, "electrolytic" | "tantalum");
        self.glyph = if self.pas_polar { "electrolytic" } else { "capacitor" }.into();
        self.pins_text = if self.pas_polar { "1 + left\n2 - right" } else { "1 1 left\n2 2 right" }.into();
        self.body_on = false;
        self.thermal_on = false;
        self.extras.clear();
        match kind {
            "ceramic" => {
                self.fp_preset.clear();
                self.pkg = PkgKind::Chip2;
                (self.pitch, self.len, self.width) = (1.65, 0.8, 0.95);
                self.height = 0.0;
            }
            "electrolytic" => {
                self.fp_preset = "CP_Radial_D5_P2.0".into();
                self.height = 11.0;
            }
            "tantalum" => {
                self.fp_preset = "CP_Radial_D5_P2.0".into();
                self.height = 7.0;
            }
            _ => {
                self.fp_preset = "C_Disc_5.08".into();
                self.height = 9.0;
            }
        }
        let head = match kind {
            "electrolytic" => "CE_",
            "tantalum" => "CT_",
            "film" => "CF_",
            _ => "C_",
        };
        if self.name.trim().is_empty() {
            self.name = head.into();
        } else if let Some(rest) = ["CE_", "CT_", "CF_", "C_"].iter().find_map(|h| self.name.strip_prefix(h)) {
            self.name = format!("{head}{rest}");
        }
    }

    fn set_resistor_or_inductor(&mut self, t: PartType, shape: &'static str) {
        let resistor = t == PartType::Resistor;
        self.ptype = Some(t);
        self.ptype_sub = shape;
        self.pas_kind = if resistor { PassiveKind::Resistor } else { PassiveKind::Inductor };
        self.prefix = if resistor { "R" } else { "L" }.into();
        self.glyph = if resistor { "resistor" } else { "inductor" }.into();
        self.pins_text = "1 1 left\n2 2 right".into();
        self.pas_polar = false;
        self.body_on = false;
        self.thermal_on = false;
        self.extras.clear();
        if shape == "chip" {
            self.fp_preset.clear();
            self.pkg = PkgKind::Chip2;
            (self.pitch, self.len, self.width) = if resistor { (1.65, 0.8, 0.95) } else { (1.825, 1.025, 1.4) };
            self.height = 0.0;
        } else {
            self.fp_preset = "R_Axial_10.16".into();
            self.height = if resistor { 2.4 } else { 5.0 };
        }
    }

    fn set_transistor(&mut self, kind: &str) {
        self.glyph = kind.into();
        // pin numbers 1 2 3 along the row; the sides decide where each lead leaves the symbol
        self.pins_text = match kind {
            "npn" => "1 E bottom\n2 B left\n3 C top",
            "pnp" => "1 E top\n2 B left\n3 C bottom",
            "nmos" => "1 S bottom\n2 G left\n3 D top",
            _ => "1 S top\n2 G left\n3 D bottom",
        }
        .into();
    }

    /// A body that fits the pads of a dual-row IC (the data sheet's outline is the value to type; this is a start).
    fn fit_body(&mut self) {
        if self.pkg == PkgKind::Dual {
            let n = self.pins_text.lines().filter(|l| !l.trim().is_empty()).count().max(2);
            self.body_w = (self.row - 1.5).max(1.0);
            self.body_h = self.pitch * (n / 2).saturating_sub(1) as f64 + 1.1;
            self.body_x = 0.0;
            self.body_y = 0.0;
        }
    }

    /// The right-hand side: the symbol and the footprint drawn to scale, what is wrong, and the save button.
    fn preview_pane(&mut self, ui: &mut egui::Ui, built: &Result<PartDef, String>) -> Option<PartDef> {
        let def = match built {
            Ok(d) => d,
            Err(e) => {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                return None;
            }
        };
        ui.label(egui::RichText::new("回路図の記号").strong());
        symbol_preview(ui, def);
        ui.label(egui::RichText::new("基板の形 (実寸比)").strong());
        if let Ok(fp) = def.footprint() {
            footprint_preview(ui, &fp);
            if let Some(h) = fp.height {
                ui.small(format!("本体の高さ {h:.2} mm"));
            }
        }
        let issues: Vec<Issue> = def.validate();
        egui::ScrollArea::vertical().id_salt("part_form_issues").max_height(110.0).auto_shrink([false, true]).show(ui, |ui| {
            if issues.is_empty() {
                ui.colored_label(Color32::from_rgb(60, 170, 90), "✔ 問題は見つかりません");
            }
            for i in &issues {
                let (c, mark) = if i.error { (Color32::from_rgb(230, 60, 60), "エラー") } else { (Color32::from_rgb(220, 160, 40), "注意") };
                ui.colored_label(c, format!("{mark}: {}", i.message));
            }
        });
        let ok = !def.has_errors();
        if ui.add_enabled(ok, egui::Button::new(egui::RichText::new("保存してライブラリに追加").size(15.0))).on_hover_text("この部品をライブラリに保存して、すぐに回路図で使えるようにします。エラーが残っていると押せません").clicked() {
            return Some(def.clone());
        }
        None
    }
}

/// The schematic symbol of the part, as it will be drawn on the sheet.
fn symbol_preview(ui: &mut egui::Ui, def: &PartDef) {
    let sym = def.symbol();
    let lines = crate::schematic::symbol_lines(&sym);
    let (resp, painter) = ui.allocate_painter(Vec2::new(ui.available_width().min(370.0), 120.0), Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 2.0, Color32::from_rgb(248, 245, 236));
    if lines.is_empty() {
        return;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for (a, b) in &lines {
        for p in [a, b] {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
    }
    let scale = ((r.width() - 40.0) / (x1 - x0).max(1.0)).min((r.height() - 24.0) / (y1 - y0).max(1.0)).min(26.0);
    let c = r.center();
    let to = |p: [f32; 2]| Pos2::new(c.x + (p[0] - (x0 + x1) / 2.0) * scale, c.y + (p[1] - (y0 + y1) / 2.0) * scale);
    for (a, b) in &lines {
        painter.line_segment([to(*a), to(*b)], Stroke::new(1.6, Color32::from_rgb(120, 30, 30)));
    }
    for (name, p) in crate::schematic::symbol_pin_labels(&sym) {
        painter.text(to(p), Align2::CENTER_BOTTOM, name, FontId::proportional(10.0), Color32::from_gray(90));
    }
}

fn footprint_preview(ui: &mut egui::Ui, fp: &pcb_core::footprints::Footprint) {
    let (resp, painter) = ui.allocate_painter(Vec2::new(ui.available_width().min(420.0), 170.0), Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 2.0, Color32::from_rgb(18, 22, 18));
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in &fp.pads {
        let (w, h) = p.shape.size();
        x0 = x0.min(p.pos.x - w / 2.0);
        x1 = x1.max(p.pos.x + w / 2.0);
        y0 = y0.min(p.pos.y - h / 2.0);
        y1 = y1.max(p.pos.y + h / 2.0);
    }
    for l in &fp.silk {
        for q in [l.a, l.b] {
            x0 = x0.min(q.x);
            x1 = x1.max(q.x);
            y0 = y0.min(q.y);
            y1 = y1.max(q.y);
        }
    }
    for h in &fp.holes {
        let (a, b) = pcb_core::slot_ends(h.pos, h.drill, h.slot, h.slot_v);
        x0 = x0.min(a.x.min(b.x) - h.drill / 2.0);
        x1 = x1.max(a.x.max(b.x) + h.drill / 2.0);
        y0 = y0.min(a.y.min(b.y) - h.drill / 2.0);
        y1 = y1.max(a.y.max(b.y) + h.drill / 2.0);
    }
    let scale = ((r.width() as f64 - 20.0) / (x1 - x0).max(0.1)).min((r.height() as f64 - 20.0) / (y1 - y0).max(0.1)) as f32;
    let c = r.center();
    let to = |x: f64, y: f64| Pos2::new(c.x + ((x - (x0 + x1) / 2.0) as f32) * scale, c.y - ((y - (y0 + y1) / 2.0) as f32) * scale);
    for p in &fp.pads {
        let (w, h) = p.shape.size();
        let ctr = to(p.pos.x, p.pos.y);
        let size = Vec2::new(w as f32 * scale, h as f32 * scale);
        let col = if p.pin == "1" { Color32::from_rgb(240, 120, 60) } else { Color32::from_rgb(220, 60, 60) };
        match p.shape {
            pcb_core::PadShape::Circle { .. } => {
                painter.circle_filled(ctr, size.x / 2.0, col);
            }
            _ => {
                painter.rect_filled(Rect::from_center_size(ctr, size), 0.0, col);
            }
        }
        if let Some(d) = p.drill {
            let (a, b) = pcb_core::slot_ends(p.pos, d, p.slot, p.slot_v);
            crate::board_editor::capsule(&painter, to(a.x, a.y), to(b.x, b.y), d as f32 * scale / 2.0, Color32::from_rgb(18, 22, 18), None);
        }
        painter.text(ctr, Align2::CENTER_CENTER, &p.pin, FontId::proportional(11.0), Color32::WHITE);
    }
    // holes of the part (no copper): an outline only
    for h in &fp.holes {
        let (a, b) = pcb_core::slot_ends(h.pos, h.drill, h.slot, h.slot_v);
        crate::board_editor::capsule(&painter, to(a.x, a.y), to(b.x, b.y), h.drill as f32 * scale / 2.0, Color32::TRANSPARENT, Some(Stroke::new(1.5, Color32::LIGHT_GRAY)));
    }
    // the body outline and orientation mark (silkscreen)
    for l in &fp.silk {
        painter.line_segment([to(l.a.x, l.a.y), to(l.b.x, l.b.y)], Stroke::new(1.3, Color32::from_gray(235)));
    }
    painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);
}

/// What to look up for each kind of part: (kind, which figures go in the three boxes, [(item, where to find it)]).
const CHECKLISTS: &[(&str, &str, &[(&str, &str)])] = &[
    (
        "ダイオード",
        "最大電圧 = 逆耐圧 (VRRM)、最大電流 = 平均順電流 (IF(AV))。どちらも「絶対最大定格」の表にあります",
        &[
            ("順方向電圧 Vf", "電気的特性。条件の電流 (IF) も一緒に書く"),
            ("逆回復時間 trr", "電気的特性。スイッチング電源や高い周波数で効く"),
            ("接合容量 Cj", "電気的特性。高い周波数で効く"),
            ("非繰り返しサージ電流 IFSM", "絶対最大定格。電源を入れた瞬間の電流に耐えるか"),
            ("逆方向漏れ電流 IR", "電気的特性。温度で増える"),
        ],
    ),
    (
        "ツェナーダイオード",
        "最大電力 = 許容損失 (Pd)。最大電流は、最大ツェナー電流 (IZM) を入れます",
        &[
            ("ツェナー電圧 Vz", "電気的特性。測定電流 (IZT) と許容差 (±5% など) も書く"),
            ("ツェナーインピーダンス Zzt", "電気的特性。小さいほど電圧が安定"),
            ("温度係数", "電気的特性。温度で電圧がどれだけ動くか"),
            ("最大ツェナー電流 IZM", "絶対最大定格 (または許容損失 ÷ Vz)"),
        ],
    ),
    (
        "トランジスタ (バイポーラ)",
        "最大電圧 = コレクタ・エミッタ間電圧 (VCEO)、最大電流 = コレクタ電流 (IC)、最大電力 = 許容損失 (Ptot)。すべて「絶対最大定格」にあります",
        &[
            ("電流増幅率 hFE", "電気的特性。範囲 (最小〜最大) と、条件の電流 (IC) を書く"),
            ("コレクタ・エミッタ飽和電圧 VCE(sat)", "電気的特性。スイッチに使うとき"),
            ("ベース・エミッタ間電圧 VBE", "電気的特性。約 0.7V"),
            ("トランジション周波数 fT", "電気的特性。増幅できる周波数の目安"),
        ],
    ),
    (
        "MOSFET",
        "最大電圧 = ドレイン・ソース間電圧 (VDS)、最大電流 = ドレイン電流 (ID)、最大電力 = 許容損失 (PD)。「絶対最大定格」にあります",
        &[
            ("しきい値電圧 VGS(th)", "電気的特性。ゲートをこれ以上にするとオンになり始める"),
            ("オン抵抗 RDS(on)", "電気的特性。ゲート電圧 (VGS) の条件に注意。スイッチの発熱を決める"),
            ("ゲート・ソース間の最大電圧 VGS", "絶対最大定格。超えるとゲートが壊れる"),
            ("ゲート電荷 Qg", "電気的特性。ゲートを駆動する電流とスイッチング速度に効く"),
        ],
    ),
    (
        "レギュレータ (三端子・LDO)",
        "最大電圧 = 入力電圧の最大、最大電流 = 出力電流の最大。「絶対最大定格」にあります",
        &[
            ("出力電圧 Vout", "電気的特性。許容差 (±4% など) も書く"),
            ("ドロップアウト電圧", "電気的特性。入力が出力よりこれ以上高くないと安定化しない"),
            ("ライン・レギュレーション", "電気的特性。入力が変わったときの出力の変化"),
            ("ロード・レギュレーション", "電気的特性。負荷が変わったときの出力の変化"),
            ("熱抵抗 θJA", "熱の表。電力 × 熱抵抗 = 温度上昇"),
            ("静止電流 (グラウンド電流)", "電気的特性。電池で使うときの消費"),
        ],
    ),
    (
        "オペアンプ",
        "最大電圧 = 電源電圧の最大 (単電源なら合計、両電源なら ± の 2 倍)。「絶対最大定格」にあります",
        &[
            ("利得帯域幅積 GBW", "電気的特性。ゲインを上げると帯域が狭くなる"),
            ("入力オフセット電圧 Vos", "電気的特性。直流のずれ"),
            ("入力バイアス電流 Ib", "電気的特性。大きな抵抗を使うと効く"),
            ("出力電圧の振れ幅", "電気的特性。電源までは届かない (レール・ツー・レールかどうか)"),
            ("スルーレート", "電気的特性。大きな信号で追いつける速さ"),
            ("動作する電源電圧の範囲", "推奨動作条件。単電源で使えるか"),
        ],
    ),
    (
        "電解コンデンサ (アルミ・タンタル)",
        "最大電圧 = 定格電圧 (Rated Voltage、Vdc)。最大電流・最大電力は空のままでよい。下の「電解コンデンサの項目」に、リップル電流と寿命を入れます",
        &[
            ("使用温度範囲", "仕様の表。最高温度 (105℃ など) で寿命とリップル電流の値が決まる"),
            ("容量許容差", "仕様の表。±20% (20℃、120Hz) など"),
            ("漏れ電流 LC", "仕様の表。0.01CV または 3µA の大きいほど、など。2 分後の値"),
            ("損失角の正接 tanδ", "仕様の表。電圧ごと、120Hz。ESR に換算できる (ESR = tanδ ÷ 2πfC)"),
            ("インピーダンス Z (100kHz)", "サイズの表。20℃ と −10℃。電解ではほぼ ESR (「かんたん作成」の「ESR に入れる」)"),
            ("低温特性 Z(−25℃)/Z(20℃)", "仕様の表。寒いと ESR が何倍になるか"),
            ("外形 φD × L とリード間隔 F", "寸法の表。基板の形 (リード間隔、穴径はリード径 d + 0.2〜0.3mm) と高さに使う"),
        ],
    ),
    (
        "そのほか",
        "部品に合わせて、「絶対最大定格」の電圧・電流・電力を入れます",
        &[
            ("動作温度範囲", "絶対最大定格 / 推奨動作条件"),
            ("電源電圧の範囲", "推奨動作条件"),
            ("消費電流", "電気的特性"),
        ],
    ),
];

/// Adds the checklist of `kind` to the rows already typed: an item whose name is already there is not added again.
fn merge_checklist(rows: &str, kind: usize) -> String {
    let (_, _, items) = CHECKLISTS[kind.min(CHECKLISTS.len() - 1)];
    let mut out: Vec<String> = rows.lines().map(String::from).collect();
    for (name, hint) in items {
        if !out.iter().any(|l| l.split('|').next().is_some_and(|n| n.trim() == *name)) {
            out.push(format!("{name} |  | ※{hint}"));
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{add_subckt_instance, dc_op, Circuit, Element};

    struct Rig {
        c: Circuit,
    }

    /// The generated model with `vin` on VIN, a stiff cell source on BAT and `rprog` ohms on PROG
    /// (None = open); other pins get light loads. Returns the circuit.
    fn rig(t: &ChargerTemplate, vin: f64, vbat: f64, rprog: Option<f64>) -> Rig {
        let g = t.generate("TPL").unwrap();
        let mut c = Circuit::new();
        let ports: Vec<String> = g.pins.iter().map(|p| p.name.to_ascii_lowercase()).collect();
        let nodes: Vec<usize> = ports.iter().map(|p| if p == "gnd" { 0 } else { c.node(p) }).collect();
        add_subckt_instance(&mut c, "U1", &g.sim_name, &g.sim_text, &nodes).unwrap_or_else(|e| panic!("{e}"));
        let node = |c: &mut Circuit, n: &str| c.node(n);
        let (vin_n, bat_n) = (node(&mut c, "vin"), node(&mut c, "bat"));
        c.add(Element::vdc("Vin", vin_n, 0, vin));
        c.add(Element::vdc("Vbat", bat_n, 0, vbat));
        if ports.iter().any(|p| p == "lx") {
            let lx = node(&mut c, "lx");
            c.add(Element::Inductor { name: "L1".into(), a: lx, b: bat_n, l: 2.2e-6 });
        }
        if let (Some(r), true) = (rprog, ports.iter().any(|p| p == "prog")) {
            let prog = node(&mut c, "prog");
            c.add(Element::Resistor { name: "Rp".into(), a: prog, b: 0, r });
        }
        for (n, pull) in [("chrg", true), ("stdby", true), ("ntc", false)] {
            if ports.iter().any(|p| p == n) {
                let id = node(&mut c, n);
                if pull {
                    c.add(Element::Resistor { name: format!("Rpull_{n}"), a: vin_n, b: id, r: 1e3 });
                } else {
                    c.add(Element::Resistor { name: "Rntc_ext".into(), a: id, b: 0, r: 10e3 });
                }
            }
        }
        Rig { c }
    }

    fn charge(t: &ChargerTemplate, vin: f64, vbat: f64, rprog: Option<f64>) -> f64 {
        dc_op(&rig(t, vin, vbat, rprog).c).unwrap_or_else(|e| panic!("{e:?}")).element("Vbat").unwrap().current
    }

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1e-3)
    }

    #[test]
    fn the_default_template_is_a_working_linear_charger() {
        let t = ChargerTemplate::default();
        // I = 1200 / R
        for (r, want) in [(1.2e3, 1.0), (2.4e3, 0.5), (4.8e3, 0.25)] {
            let i = charge(&t, 5.0, 3.7, Some(r));
            assert!(near(i, want, 0.04), "R {r}: {i}");
        }
        // pre-charge: a tenth below 2.9 V; taper near 4.2 V; nothing when full or without supply
        assert!(near(charge(&t, 5.0, 2.5, Some(2.4e3)), 0.05, 0.1));
        let mid = charge(&t, 5.0, 4.19, Some(2.4e3));
        assert!(mid < 0.45 && mid > 0.05, "{mid}");
        assert!(charge(&t, 5.0, 4.21, Some(2.4e3)).abs() < 0.02);
        assert!(charge(&t, 3.5, 3.0, Some(2.4e3)).abs() < 5e-3);
        // an open PROG pin gives the open-pin current (0 here)
        assert!(charge(&t, 5.0, 3.7, None).abs() < 5e-3);
    }

    #[test]
    fn the_ip2332_style_template_reproduces_the_bundled_part() {
        let t = ChargerTemplate {
            topology: Topology::Buck,
            current: 2.0,
            k: 2750.0,
            offset: 0.35,
            precharge_fraction: 0.2,
            vin_min: 4.3,
            vin_max: 6.0,
            dropout: 0.1,
            ntc: Some((0.3, 2.73)),
            ..Default::default()
        };
        // same table as the datasheet: 4.3k -> 1 A, 2.4k -> 1.5 A, open -> 2 A
        assert!(near(charge(&t, 5.0, 3.7, Some(4.3e3)), 0.99, 0.05));
        assert!(near(charge(&t, 5.0, 3.7, Some(2.4e3)), 1.5, 0.05));
        assert!(near(charge(&t, 5.0, 3.7, None), 2.0, 0.05));
        assert!(charge(&t, 6.5, 3.7, Some(4.3e3)).abs() < 5e-3, "over-voltage");
        assert!(charge(&t, 4.0, 3.7, Some(4.3e3)).abs() < 5e-3, "under-voltage");
    }

    #[test]
    fn two_cells_float_at_8v4_and_pre_charge_at_5v8() {
        let t = ChargerTemplate { cells: 2, vin_min: 9.0, ..Default::default() };
        assert!(near(charge(&t, 12.0, 7.4, Some(2.4e3)), 0.5, 0.05));
        assert!(charge(&t, 12.0, 8.45, Some(2.4e3)).abs() < 0.02);
        assert!(near(charge(&t, 12.0, 5.0, Some(2.4e3)), 0.05, 0.1), "below 5.8 V");
    }

    #[test]
    fn a_fixed_current_part_has_no_prog_pin_and_status_lines_follow() {
        let t = ChargerTemplate { setting: CurrentSetting::Fixed, current: 0.8, ..Default::default() };
        assert!(near(charge(&t, 5.0, 3.7, None), 0.8, 0.04));
        let r = rig(&t, 5.0, 3.7, None);
        let op = dc_op(&r.c).unwrap();
        assert!(op.voltage("chrg").unwrap() < 1.0 && op.voltage("stdby").unwrap() > 4.0, "charging");
        let r = rig(&t, 5.0, 4.2, None);
        let op = dc_op(&r.c).unwrap();
        assert!(op.voltage("stdby").unwrap() < 1.0 && op.voltage("chrg").unwrap() > 4.0, "full");
    }

    #[test]
    fn applying_the_template_fills_the_form_and_the_result_passes_the_checks() {
        let mut f = PartForm { name: "MYCHG".into(), tpl: ChargerTemplate { status: false, ..Default::default() }, ..Default::default() };
        f.apply_template();
        assert!(f.tpl_message.contains("ピン 4 本"), "{}", f.tpl_message);
        assert!(f.pins_text.contains("2 GND bottom") && f.pins_text.contains("4 PROG left"));
        assert!(f.sim == SimChoice::Subckt);
        assert_eq!(f.sim_pins, "1 2 3 4");
        assert_eq!(f.category, "電源管理");
        // with a 4-pin dual package the definition has no errors
        f.pkg = PkgKind::Dual;
        let def = f.to_def().unwrap();
        assert!(!def.has_errors(), "{:?}", def.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
        // the notes about what the model leaves out go into the saved part
        assert!(f.sim_limits.lines().count() >= 4, "{}", f.sim_limits);
        assert_eq!(def.sim.as_ref().unwrap().limits.len(), f.sim_limits.lines().count());
        assert!(def.sim.unwrap().limits.iter().any(|l| l.contains("リニア") || l.contains("LX")));
        // a missing name is explained
        let mut g = PartForm::default();
        g.apply_template();
        assert!(g.tpl_message.contains("部品名"), "{}", g.tpl_message);
    }
}

/// Hover text of a package dimension box of the part form (data sheet "recommended land pattern").
fn pkg_tip(label: &str) -> &'static str {
    match label {
        "ピッチ" => "隣り合う足の中心どうしの間隔 (mm)。例: 1.27、0.65、2.54",
        "パッド長" => "はんだ付けする銅の四角 (パッド) の長さ。データシートの推奨ランドパターンの値",
        "パッド幅" => "パッドの幅。足の幅より少し広くします",
        "列間隔" => "向かい合う足の列の中心どうしの間隔 (mm)。DIP の 0.3 インチ幅なら 7.62",
        "対向パッド間隔" => "向かい合うパッドの外側から外側まで (または中心から中心まで) の距離。データシートの推奨寸法を見ます",
        "パッド径" => "パッド (丸い銅) の直径",
        "穴径" => "スルーホールの穴の直径。足の太さ + 0.2〜0.3mm",
        _ => "データシートの推奨ランドパターンの寸法 (mm)",
    }
}

#[cfg(test)]
mod ratings_tests {
    use super::*;

    fn form() -> PartForm {
        let mut f = PartForm::default();
        f.name = "TEST".into();
        f.pins_text = "1 A\n2 K".into();
        f.pkg = PkgKind::Chip2;
        f
    }

    /// The checklist adds the items to look up; typing a value turns a line into a data sheet row, and the
    /// note to the learner (※...) is not saved as a condition.
    #[test]
    fn the_checklist_turns_into_rows_as_values_are_filled_in() {
        let mut f = form();
        f.rt_kind = 0; // diode
        f.rt_rows = merge_checklist(&f.rt_rows, 0);
        assert!(f.rt_rows.lines().count() >= 4 && f.rt_rows.contains("順方向電圧 Vf") && f.rt_rows.contains("※"));
        // nothing is looked up yet: the part has no ratings, and saving is still possible
        let d = f.to_def().unwrap();
        assert!(d.ratings.is_none());
        assert_eq!(f.ratings_progress(), (0, 0, f.rt_rows.lines().count()));
        // the learner fills in the forward voltage and the reverse voltage
        f.rt_rows = f.rt_rows.replace("順方向電圧 Vf |  | ※", "順方向電圧 Vf | 0.6V (最大) | IF = 1A ※");
        f.rt_rows = f.rt_rows.replace("順方向電圧 Vf |  | ", "順方向電圧 Vf | 0.6V (最大) | ");
        f.rt_v = "40".into();
        let r = f.to_def().unwrap().ratings.unwrap();
        assert_eq!(r.v_max, Some(40.0));
        let vf = r.rows.iter().find(|x| x.name == "順方向電圧 Vf").expect("the filled row");
        assert_eq!(vf.value, "0.6V (最大)");
        assert_eq!(vf.condition, "IF = 1A", "the note after ※ is dropped");
        let (max_given, filled, open) = f.ratings_progress();
        assert_eq!((max_given, filled), (1, 1));
        assert!(open >= 3);
        // asking for the checklist again does not repeat the items that are already there
        let again = merge_checklist(&f.rt_rows, 0);
        assert_eq!(again.lines().count(), f.rt_rows.lines().count());
        // every kind has a hint and items
        for (name, hint, items) in CHECKLISTS {
            assert!(!name.is_empty() && !hint.is_empty() && !items.is_empty(), "{name}");
        }
    }

    #[test]
    fn a_line_without_a_name_or_a_separator_is_explained() {
        let mut f = form();
        f.rt_rows = "順方向電圧 0.6V".into();
        assert!(f.to_def().unwrap_err().contains("項目 | 値 | 条件"));
        f.rt_rows = " | 0.6V".into();
        assert!(f.to_def().is_err());
        f.rt_rows = String::new();
        f.rt_v = "abc".into();
        assert!(f.to_def().unwrap_err().contains("最大電圧"));
    }
}

#[cfg(test)]
mod spice_paste_tests {
    use super::*;

    #[test]
    fn a_pasted_diode_model_fills_the_form() {
        let mut f = PartForm::default();
        f.sim_text = "* vendor\n.MODEL DX D (IS=1n\n+ RS=1)".into();
        f.autofill_from_spice().unwrap();
        assert_eq!((f.sim, f.sim_name.as_str(), f.sim_element.as_str(), f.sim_pins.as_str()), (SimChoice::Model, "DX", "D", "1 2"));
        assert_eq!(f.name, "DX");
        let d = f.to_def().unwrap();
        assert!(!d.has_errors(), "{:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
    }

    #[test]
    fn a_transistor_keeps_its_pin_names_in_node_order() {
        let mut f = PartForm::default();
        f.name = "BC".into();
        f.pins_text = "1 E\n2 B\n3 C".into();
        f.pkg = PkgKind::Row;
        f.sim_text = ".model BC NPN(BF=200)".into();
        f.autofill_from_spice().unwrap();
        assert_eq!(f.sim_pins, "3 2 1", "C B E");
        assert_eq!(f.sim_element, "Q");
        assert!(!f.to_def().unwrap().has_errors());
    }

    #[test]
    fn a_subcircuit_makes_one_pin_per_port() {
        let mut f = PartForm::default();
        f.sim_text = ".subckt AMP IN+ IN- OUT VCC GND\nR1 IN+ OUT 1k\n.ends".into();
        f.autofill_from_spice().unwrap();
        assert_eq!(f.sim, SimChoice::Subckt);
        assert_eq!(f.parse_pins().unwrap().len(), 5);
        assert_eq!(f.sim_pins, "1 2 3 4 5");
        let d = f.to_def().unwrap();
        assert!(!d.has_errors(), "{:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
    }

    #[test]
    fn bad_text_and_mismatched_pins_are_explained() {
        let mut f = PartForm::default();
        f.sim_text = "R1 1 2 1k".into();
        assert!(f.autofill_from_spice().unwrap_err().contains(".model"));
        f.sim_text = ".model Z TRIAC(X=1)".into();
        assert!(f.autofill_from_spice().unwrap_err().contains("未対応"));
        f.sim_text = ".model BC NPN()".into();
        f.pins_text = "1 X\n2 Y".into();
        assert!(f.autofill_from_spice().unwrap_err().contains("3 本"));
    }
}

#[cfg(test)]
mod extras_tests {
    use super::*;

    fn usb_form() -> PartForm {
        let mut f = PartForm::default();
        f.name = "USB_TEST".into();
        f.prefix = "J".into();
        f.pins_text = "1 VBUS\n2 D-\n3 D+\n4 GND\nSH SHIELD".into();
        f.pkg = PkgKind::Row;
        f.pitch = 2.5;
        f.extras = "脚 SH -6.5 0 2.0 3.0 1.2\n脚 SH 6.5 0 2.0 3.0 1.2\n# a comment\n穴 0 -3 1.0\n穴 0 3 1.0 2.0".into();
        f
    }

    #[test]
    fn shield_legs_and_pegs_join_the_package() {
        let d = usb_form().to_def().unwrap();
        assert!(!d.has_errors(), "{:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
        let fp = d.footprint().unwrap();
        // 4 signal pads from the row, 2 shield legs sharing the pin SH
        assert_eq!(fp.pads.len(), 6);
        assert_eq!(fp.pads.iter().filter(|p| p.pin == "SH").count(), 2);
        assert_eq!(fp.holes.len(), 2);
        assert!(fp.holes[1].slot > fp.holes[1].drill, "the second peg hole is a slot");
    }

    /// The data sheet's body and exposed pad go into the footprint: an 8-pin SOIC with a pin 1 dot, a QFN-like
    /// part with a thermal pad, and the mistakes the form refuses.
    #[test]
    fn the_body_and_the_thermal_pad_come_from_the_form() {
        let mut f = PartForm { name: "SOIC8_TEST".into(), pins_text: "1 A
2 B
3 C
4 D
5 E
6 F
7 G
8 H".into(), ..PartForm::default() };
        f.pkg = PkgKind::Dual;
        f.body_on = true;
        let plain = f.to_def().unwrap().footprint().unwrap();
        assert!(!plain.silk.is_empty(), "the body outline is drawn");
        // the thermal pad is one more pad, in the middle, sharing nothing with the row
        f.pins_text.push_str("
9 EP");
        f.thermal_on = true;
        f.thermal_pin = "9".into();
        f.thermal_w = 2.4;
        f.thermal_h = 3.4;
        let d = f.to_def().unwrap();
        assert!(!d.has_errors(), "{:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
        let fp = d.footprint().unwrap();
        assert_eq!(fp.pads.len(), 9);
        let ep = fp.pads.iter().find(|p| p.pin == "9").unwrap();
        assert_eq!((ep.pos.x, ep.pos.y), (0.0, 0.0));
        assert_eq!(ep.shape.size(), (2.4, 3.4));
        // the outline stays clear of every pad
        assert!(fp.silk.iter().all(|l| fp.pads.iter().all(|p| {
            let (w, h) = p.shape.size();
            [l.a, l.b].iter().all(|q| (q.x - p.pos.x).abs() > w / 2.0 || (q.y - p.pos.y).abs() > h / 2.0)
        })));
        // a pin that is not in the pin table, a missing pin and a bad body are refused with a message
        f.thermal_pin = "99".into();
        assert!(f.to_def().unwrap_err().contains("ピン表"));
        f.thermal_pin = "".into();
        assert!(f.to_def().unwrap_err().contains("ピン番号"));
        f.thermal_on = false;
        f.body_w = 0.0;
        assert!(f.to_def().unwrap().validate().iter().any(|i| i.error && i.message.contains("幅")));
        f.body_w = 3.9;
        f.body_pin = "Z".into();
        assert!(f.to_def().unwrap().validate().iter().any(|i| i.error && i.message.contains("向きの印")));
        // the definition survives saving as JSON
        f.body_pin = "1".into();
        let d = f.to_def().unwrap();
        let back: PartDef = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back.body, d.body);
    }

    /// Every starting point of the "kind of part" bar makes a part with no errors once it has a name, with its
    /// own symbol, and the transistor / diode choices change the symbol.
    #[test]
    fn each_kind_of_part_starts_from_a_valid_definition() {
        for name in ["ic", "transistor", "diode", "capacitor", "resistor", "inductor", "connector"] {
            let mut f = PartForm::for_type(name);
            assert!(f.ptype.is_some(), "{name}");
            f.name = format!("TEST_{name}");
            let d = f.to_def().unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!d.has_errors(), "{name}: {:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
            assert!(d.footprint().is_ok(), "{name}");
            assert!(!crate::schematic::symbol_lines(&d.symbol()).is_empty(), "{name}: a symbol to draw");
        }
        // a transistor of each kind gets its own pin sides and a standard symbol with the small layout
        for (kind, glyph) in [("npn", "npn"), ("pnp", "pnp"), ("nmos", "nmos"), ("pmos", "pmos")] {
            let mut f = PartForm::for_type("transistor");
            f.set_transistor(kind);
            f.name = "Q_TEST".into();
            let d = f.to_def().unwrap();
            assert_eq!(d.glyph, glyph);
            assert!(!d.has_errors(), "{kind}: {:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
            let sym = d.symbol();
            assert_eq!((sym.layout().hw, sym.layout().hh), (0, 0), "{kind}: the small transistor layout");
        }
        // each kind of capacitor: the polarity, the footprint and the name prefix, and the template keeps them
        for (kind, polar, head, fp) in [("ceramic", false, "C_", ""), ("electrolytic", true, "CE_", "CP_Radial_D5_P2.0"), ("tantalum", true, "CT_", "CP_Radial_D5_P2.0"), ("film", false, "CF_", "C_Disc_5.08")] {
            let mut f = PartForm::for_type("capacitor");
            f.set_capacitor(kind);
            assert!(f.name.starts_with(head), "{kind}: {}", f.name);
            assert_eq!(f.fp_preset, fp);
            f.name = format!("{head}100u_TEST");
            f.pas_value = "100u".into();
            f.apply_passive().unwrap();
            let d = f.to_def().unwrap();
            assert!(!d.has_errors(), "{kind}: {:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
            assert_eq!(d.glyph == "electrolytic", polar, "{kind}");
            assert_eq!(d.pins.iter().any(|p| p.name == "+"), polar, "{kind}");
            assert!(d.footprint().unwrap().pads.iter().all(|p| (p.drill.is_some()) == !fp.is_empty()), "{kind}: leaded parts are through-hole");
            assert_eq!(d.capacitor_family(), Some(match kind { "ceramic" => "積層セラミック", "electrolytic" => "電解", "tantalum" => "タンタル", _ => "フィルム" }), "{kind}");
        }
        // a leaded resistor and a chip coil
        let mut f = PartForm::for_type("resistor");
        f.set_resistor_or_inductor(PartType::Resistor, "leaded");
        f.name = "R_TEST".into();
        assert_eq!(f.to_def().unwrap().footprint().unwrap().pads.len(), 2);
        // a diode keeps its 2 leads small
        let mut f = PartForm::for_type("diode");
        f.name = "D_TEST".into();
        f.glyph = "zener".into();
        let sym = f.to_def().unwrap().symbol();
        assert_eq!((sym.layout().hw, sym.layout().hh), (0, 1));
    }

    /// The electrolytic capacitor's figures: rated ripple current with its frequency table, and the endurance.
    #[test]
    fn ripple_current_and_life_are_read_from_the_ratings_tab() {
        let mut f = PartForm::for_type("electrolytic");
        f.name = "CE_TEST".into();
        let name_of = |f: &PartForm| CHECKLISTS[f.checklist_for_type().unwrap()].0;
        assert!(name_of(&f).starts_with("電解コンデンサ"), "the checklist follows the kind");
        // the kind at the top settles the checklist; an IC leaves the question open
        let mut t = PartForm::for_type("transistor");
        assert!(name_of(&t).starts_with("トランジスタ"));
        t.set_transistor("nmos");
        assert_eq!(name_of(&t), "MOSFET");
        let mut d = PartForm::for_type("diode");
        assert_eq!(name_of(&d), "ダイオード");
        d.glyph = "zener".into();
        assert!(name_of(&d).starts_with("ツェナー"));
        assert!(PartForm::for_type("ic").checklist_for_type().is_none() && PartForm::default().checklist_for_type().is_none());
        assert_eq!(name_of(&PartForm::for_type("ceramic")), "そのほか");
        f.rt_v = "16".into();
        f.rt_ripple = "540m".into();
        f.rt_ripple_f = "100k".into();
        f.rt_ripple_table = "120 0.55
1k 0.77
10k 0.94
100k 1.0".into();
        f.rt_life_h = "6000".into();
        f.rt_life_t = "105".into();
        let r = f.to_def().unwrap().ratings.unwrap();
        let rip = r.ripple.clone().unwrap();
        assert!((rip.i_rms - 0.54).abs() < 1e-9 && rip.freq_hz == 100e3 && rip.coeffs.len() == 4);
        assert!((rip.allowed_at(120.0) - 0.54 * 0.55).abs() < 1e-9);
        assert_eq!(r.life, Some(sch_core::LifeRating { hours: 6000.0, temp_c: 105.0 }));
        assert!(r.summary().contains("定格リップル電流") && r.summary().contains("寿命"));
        // mistakes are told in words
        f.rt_ripple_table = "120 abc".into();
        assert!(f.to_def().unwrap_err().contains("1行目"));
        f.rt_ripple_table.clear();
        f.rt_ripple_f = "x".into();
        assert!(f.to_def().unwrap_err().contains("周波数"));
        f.rt_ripple_f = "100k".into();
        f.rt_life_t = "hot".into();
        assert!(f.to_def().unwrap_err().contains("温度"));
        // with nothing typed there are no such figures
        let mut g = PartForm::for_type("electrolytic");
        g.name = "CE_X".into();
        assert!(g.to_def().unwrap().ratings.is_none());
    }

    #[test]
    fn a_vertical_plated_slot_and_smd_pad_are_read() {
        let mut f = usb_form();
        f.extras = "脚 SH 6.5 0 1.6 3.6 0.8 2.4 v\nパッド SH -4 -2 1.5 2.0".into();
        let fp = f.to_def().unwrap().footprint().unwrap();
        let slot = fp.pads.iter().find(|p| p.slot > 0.0).unwrap();
        assert!(slot.slot_v && slot.drill == Some(0.8));
        assert!(fp.pads.iter().any(|p| p.drill.is_none() && p.pin == "SH"));
    }

    #[test]
    fn mistakes_name_the_line() {
        let mut f = usb_form();
        f.extras = "脚 SH -6.5 0 2.0 3.0".into();
        assert!(f.to_def().unwrap_err().contains("1行目"));
        f.extras = "ねじ 1 2 3".into();
        assert!(f.to_def().unwrap_err().contains("脚 / 穴 / パッド"));
        f.extras = "脚 SH 0 0 2.0 3.0 1.2 0.5".into();
        let d = f.to_def().unwrap();
        assert!(d.validate().iter().any(|i| i.error && i.message.contains("slot")), "a slot shorter than its width is refused by the package");
    }
}

#[cfg(test)]
mod passive_form_tests {
    use super::*;

    #[test]
    fn a_chip_capacitor_from_data_sheet_numbers() {
        let mut f = PartForm::default();
        f.fp_preset = "C_0603".into();
        f.pas_value = "100n".into();
        f.pas_esr = "20m".into();
        f.pas_srf = "30M".into();
        f.pas_bias = "30%".into();
        let msg = f.apply_passive().unwrap();
        assert!(msg.contains("ESL は自己共振周波数から"), "{msg}");
        assert_eq!((f.name.as_str(), f.prefix.as_str(), f.glyph.as_str(), f.value.as_str()), ("C_100n_0603", "C", "capacitor", "100n"));
        let d = f.to_def().unwrap();
        assert!(!d.has_errors(), "{:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
        assert_eq!(d.footprint_name(), "C_0603");
        assert_eq!(d.footprint().unwrap().pads.len(), 2);
        let sim = d.sim.clone().unwrap();
        assert!(sim.text.contains("Resr") && sim.text.contains("Lesl") && sim.text.contains("C1 n_esl 2 70n"), "{}", sim.text);
        assert!(d.symbol().glyph == "capacitor");
    }

    #[test]
    fn the_impedance_table_gives_esr_and_esl() {
        let mut f = PartForm::default();
        f.pas_value = "100n".into();
        f.pas_table = "100k abc".into();
        assert!(f.fit_from_table().unwrap_err().contains("1行目"));
        // |Z| of 100 nF with ESR 30 mΩ and ESL 0.9 nH
        let z = |hz: f64| passive::cap_impedance(100e-9, 0.03, 0.9e-9, hz);
        f.pas_table = [1e5, 1e6, 5e6, 2.4e7, 1e8, 3e8].iter().map(|&hz| format!("{} {:.5}", hz, z(hz))).collect::<Vec<_>>().join("\n");
        let m = f.fit_from_table().unwrap();
        assert!(m.contains("求めました"), "{m}");
        let esr = sim_core::parse_value(&f.pas_esr).unwrap();
        let esl = sim_core::parse_value(&f.pas_esl).unwrap();
        assert!((esr / 0.03 - 1.0).abs() < 0.05 && (esl / 0.9e-9 - 1.0).abs() < 0.05, "{esr} {esl}");
    }

    #[test]
    fn frequencies_read_m_as_mega_and_resistances_as_milli() {
        assert_eq!(parse_freq("30M"), Some(30e6));
        assert_eq!(parse_freq("100kHz"), Some(100e3));
        assert_eq!(parse_freq("1.5MHz"), Some(1.5e6));
        assert_eq!(parse_ohm("20m"), Some(0.02));
        assert_eq!(parse_ohm("0.05Ω"), Some(0.05));
    }

    #[test]
    fn mistakes_in_the_chip_section_are_explained() {
        let mut f = PartForm::default();
        assert!(f.apply_passive().unwrap_err().contains("値を入力"));
        f.pas_value = "100n".into();
        f.pas_esr = "abc".into();
        assert!(f.apply_passive().unwrap_err().contains("ESR"));
        f.pas_esr.clear();
        f.pas_bias = "200".into();
        assert!(f.apply_passive().unwrap_err().contains("0〜95"));
    }
}

/// Hover text of a body dimension box (data sheet "package outline").
fn body_tip(label: &str) -> &'static str {
    match label {
        "幅" => "本体の横の大きさ (データシートの外形寸法の E など)。パッドの列と直角の向きではなく、基板の X 方向です",
        "直径" => "丸い本体の直径 (データシートの D)",
        "長さ" => "本体の縦の大きさ (基板の Y 方向)",
        "中心X" => "本体の中心の、部品の原点 (パッケージの中心) からのずれ (X)。ふつうは 0",
        _ => "本体の中心の、部品の原点 (パッケージの中心) からのずれ (Y)。ふつうは 0",
    }
}
