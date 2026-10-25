use super::*;

/// A built footprint pad written back as an explicit pad.
pub(super) fn pad_spec(_fp: &Footprint, p: &pcb_core::footprints::FpPad) -> PadSpec {
    let (shape, w, h) = match p.shape {
        PadShape::Circle { d } => ("circle", d, 0.0),
        PadShape::Rect { w, h } => ("rect", w, h),
        PadShape::Oval { w, h } => ("oval", w, h),
    };
    PadSpec { pin: p.pin.clone(), x: p.pos.x, y: p.pos.y, shape: shape.into(), w, h, drill: p.drill, slot: p.slot, slot_v: p.slot_v }
}

pub(super) const EXTRAS_HELP: &str = "1行に1つ。座標は部品の中心からの mm (右が +X、上が +Y)。
  脚 ピン名 X Y パッド幅 パッド高さ 穴径 [長穴の長さ] [v]   … めっきのある穴 (USBのシェル脚など)。長穴は長さを足す (縦向きは最後に v)
  穴 X Y 穴径 [長穴の長さ] [v]                            … めっきなしの穴 (位置決めの突起用の穴、ネジ穴)
  パッド ピン名 X Y 幅 高さ                                … 面実装の追加パッド (シールドのはんだ付け部分など)
ピン名はピン表の「番号」と同じにします。シェル用のピン (例: SH) をピン表に足して、回路図でGNDなどにつなぎます。";

/// Example lines the learner edits to the numbers of the data sheet.
pub(super) const EXTRAS_EXAMPLES: &[(&str, &str, &str)] = &[
    ("シェル脚(めっき穴)", "脚 SH -6.5 0 2.0 3.0 1.2", "コネクタの金属の外側 (シェル) をはんだ付けする脚。ピン名 SH をピン表にも足します"),
    ("位置決めの穴(めっきなし)", "穴 0 -3 1.0", "部品の突起が入る穴。めっきなしなので、回路にはつながりません"),
    ("長穴(めっき)", "脚 SH 6.5 0 1.6 3.6 0.8 2.4 v", "穴径 0.8mm、長さ 2.4mm の縦の長穴 (USB Type-C の固定脚など)"),
    ("長穴(めっきなし)", "穴 0 3 1.0 2.0", "幅 1.0mm、長さ 2.0mm の横の長穴"),
    ("面実装のシールド", "パッド SH -4 -2 1.5 2.0", "面実装 (SMD) タイプの固定用パッド"),
];

/// A frequency: `30M` is 30 MHz here (not milli), `100k`, `1G`, an optional `Hz`.
pub(super) fn parse_freq(t: &str) -> Option<f64> {
    let l = t.trim().to_lowercase();
    let l = l.trim_end_matches("hz").trim();
    let l = if l.ends_with('m') && !l.ends_with("meg") { format!("{}meg", &l[..l.len() - 1]) } else { l.to_string() };
    sim_core::parse_value(&l)
}

/// An impedance or resistance: `20m` is 20 milliohm, an optional `Ω` / `ohm`.
pub(super) fn parse_ohm(t: &str) -> Option<f64> {
    let l = t.trim().to_lowercase();
    sim_core::parse_value(l.trim_end_matches("ohm").trim_end_matches('ω').trim())
}

pub(super) fn parse_side(t: &str) -> Option<PinSide> {
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
    // vendor files list the inner blocks too: take the first .subckt that no X line calls
    let called: Vec<String> = lines
        .iter()
        .filter(|l| l.starts_with(['X', 'x']))
        .filter_map(|l| l.split_whitespace().skip(1).take_while(|w| !w.contains('=') && !w.eq_ignore_ascii_case("params:")).last().map(str::to_ascii_lowercase))
        .collect();
    let subckts = lines.iter().filter(|l| l.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case(".subckt")));
    let top = subckts.clone().find(|l| l.split_whitespace().nth(1).is_some_and(|n| !called.contains(&n.to_ascii_lowercase()))).or(subckts.clone().next());
    if let Some(l) = top {
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
        if super::trial::is_encrypted(&self.sim_text) {
            return Err(super::trial::ENCRYPTED_MSG.into());
        }
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

    pub(super) fn parse_pins(&self) -> Result<Vec<PinDef>, String> {
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
    pub(super) fn parse_ratings(&self) -> Result<Option<PartRatings>, String> {
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
    pub(super) fn ratings_progress(&self) -> (usize, usize, usize) {
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
    pub(super) fn parse_extras(&self) -> Result<(Vec<PadSpec>, Vec<HoleSpec>), String> {
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
            shops: Default::default(),
            land_unverified: self.land_unverified,
            model3d: self.model3d.clone(),
        })
    }
}
