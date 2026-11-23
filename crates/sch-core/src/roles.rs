//! What a part does with the parts around it (B11), for the bubble beside the part picked on the schematic: a voltage divider, an
//! RC filter, an LED's current-limiting resistor, a pull-up or pull-down, a bypass capacitor, an op amp's feedback, a coupling
//! capacitor. Each comes with a figure worked out from the values (the divided voltage, the cut-off frequency, the LED's current,
//! the gain) and a sentence on why the part is there. The parts are told apart by their kind or library glyph, never by name.

use crate::review::si_value;
use crate::{extract, Component, Kind, Net, PinKind, Schematic};
use std::collections::HashMap;

/// One role the picked part plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Role {
    /// "分圧", "RC ローパスフィルタ", ...
    pub name: String,
    /// The parts that play it together (the picked one first).
    pub parts: Vec<String>,
    /// The figure: "2.50 V", "fc = 1.59 kHz", "約 9.1 mA", "利得 11 倍".
    pub value: Option<String>,
    /// Why it is there, in a sentence or two.
    pub explain: String,
}

/// The forward voltage taken for an LED whose model says nothing else (a red one).
const LED_VF: f64 = 2.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Two {
    R,
    C,
    Led,
}

fn glyph(c: &Component) -> &str {
    c.custom.as_ref().map_or("", |s| if s.glyph.is_empty() { s.inferred_glyph() } else { s.glyph.as_str() })
}

fn two(c: &Component) -> Option<Two> {
    match c.kind {
        Kind::Resistor => Some(Two::R),
        Kind::Capacitor => Some(Two::C),
        Kind::Led => Some(Two::Led),
        Kind::Custom => match glyph(c) {
            "resistor" => Some(Two::R),
            "capacitor" | "electrolytic" => Some(Two::C),
            "led" => Some(Two::Led),
            _ => None,
        },
        _ => None,
    }
}

/// A number for people: "4.7 k", "10 µ".
fn eng(x: f64, unit: &str) -> String {
    let a = x.abs();
    let (v, p) = if a >= 1e6 {
        (x / 1e6, "M")
    } else if a >= 1e3 {
        (x / 1e3, "k")
    } else if a >= 1.0 || a == 0.0 {
        (x, "")
    } else if a >= 1e-3 {
        (x * 1e3, "m")
    } else if a >= 1e-6 {
        (x * 1e6, "µ")
    } else if a >= 1e-9 {
        (x * 1e9, "n")
    } else {
        (x * 1e12, "p")
    };
    let digits = if v.abs() >= 100.0 { 0 } else if v.abs() >= 10.0 { 1 } else { 2 };
    format!("{v:.digits$} {p}{unit}")
}

/// A gain for people: 11, 2.5, 0.33.
fn times(g: f64) -> String {
    let s = format!("{g:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

struct Ctx<'a> {
    s: &'a Schematic,
    nets: Vec<Net>,
    net_of: HashMap<(String, String), usize>,
    volts: &'a dyn Fn(&str) -> Option<f64>,
}

impl<'a> Ctx<'a> {
    fn comp(&self, r: &str) -> Option<&'a Component> {
        self.s.components.iter().find(|c| c.refdes == r)
    }
    fn net(&self, r: &str, pin: &str) -> Option<usize> {
        self.net_of.get(&(r.to_string(), pin.to_string())).copied()
    }
    fn name(&self, n: usize) -> &str {
        &self.nets[n].name
    }
    /// The nets of a two-terminal part's ends (pins in their order: A then K for an LED).
    fn ends(&self, c: &Component) -> Option<(usize, usize)> {
        let pins = c.pins();
        if pins.len() != 2 {
            return None;
        }
        Some((self.net(&c.refdes, &pins[0].0)?, self.net(&c.refdes, &pins[1].0)?))
    }
    fn value(&self, c: &Component) -> Option<f64> {
        si_value(c.value.split_whitespace().next().unwrap_or(""))
    }
    fn v(&self, n: usize) -> Option<f64> {
        if self.name(n) == "GND" { Some(0.0) } else { (self.volts)(self.name(n)) }
    }
    fn is_gnd(&self, n: usize) -> bool {
        self.name(n) == "GND"
    }
    /// A net held at a fixed voltage by a supply (not ground): a source's +, a power symbol, a regulator's output.
    fn is_supply(&self, n: usize) -> bool {
        let name = self.name(n);
        name != "GND"
            && (self.s.components.iter().any(|c| c.kind == Kind::Power && c.value == name)
                || self.nets[n].pins.iter().any(|p| {
                    self.comp(&p.refdes).is_some_and(|c| c.kind == Kind::VSource && p.pin == "+")
                        || self.lib_pin(&p.refdes, &p.pin).is_some_and(|(nm, k)| k == Some(PinKind::PowerOut) || matches!(nm.as_str(), "5V" | "3V3") || (matches!(nm.as_str(), "OUT" | "VOUT" | "VO") && self.is_regulator(&p.refdes)))
                }))
    }
    /// A regulator: a library part with an input pin as well as its output (an op amp's OUT is a signal, not a supply).
    fn is_regulator(&self, r: &str) -> bool {
        self.comp(r).and_then(|c| c.custom.as_ref()).is_some_and(|s| glyph(self.comp(r).unwrap()) != "opamp" && s.pins.iter().any(|q| matches!(q.name.to_ascii_uppercase().as_str(), "IN" | "VIN" | "VI")))
    }

    fn lib_pin(&self, r: &str, pin: &str) -> Option<(String, Option<PinKind>)> {
        let sym = self.comp(r)?.custom.as_ref()?;
        sym.pins.iter().find(|q| q.number == pin).map(|q| (q.name.to_ascii_uppercase(), q.kind))
    }
    /// The two-terminal parts of a kind between two nets, or from one net to anywhere.
    fn on(&self, n: usize, kind: Two) -> Vec<(&'a Component, usize)> {
        self.s
            .components
            .iter()
            .filter(|c| two(c) == Some(kind))
            .filter_map(|c| {
                let (a, b) = self.ends(c)?;
                if a == n { Some((c, b)) } else if b == n { Some((c, a)) } else { None }
            })
            .collect()
    }
    /// The pins on a net that are not the two-terminal parts listed (what else the net goes to).
    fn others(&self, n: usize, but: &[&str]) -> Vec<(String, String)> {
        self.nets[n].pins.iter().filter(|p| !but.contains(&p.refdes.as_str())).map(|p| (p.refdes.clone(), p.pin.clone())).collect()
    }
    /// An input that listens without loading (a logic or op-amp input, a switch): what a pull-up serves.
    fn has_input(&self, n: usize) -> bool {
        self.nets[n].pins.iter().any(|p| {
            self.lib_pin(&p.refdes, &p.pin).is_some_and(|(name, k)| matches!(k, Some(PinKind::Input | PinKind::Bidir | PinKind::OpenDrain)) || name == "RESET" || name.ends_with("RST"))
                || self.comp(&p.refdes).is_some_and(|c| matches!(glyph(c), "switch_push" | "switch_spdt"))
        })
    }
    /// An IC's supply pin on the net.
    fn feeds_ic(&self, n: usize) -> bool {
        self.nets[n].pins.iter().any(|p| self.lib_pin(&p.refdes, &p.pin).is_some_and(|(name, k)| k == Some(PinKind::PowerIn) && !name.contains("GND") || ["VCC", "VDD", "V+"].contains(&name.as_str())))
    }
}

/// The voltage of the nets that a source fixes without solving anything: ground, a DC voltage source's + (with − on ground), a power
/// symbol named like a voltage (5V, +5V, 3V3, 3.3V, 12V).
pub fn fixed_volts(s: &Schematic) -> HashMap<String, f64> {
    let nets = extract(s);
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone());
    let mut out = HashMap::new();
    out.insert("GND".to_string(), 0.0);
    for c in &s.components {
        if c.kind == Kind::VSource
            && net_of(&c.refdes, "-").as_deref() == Some("GND")
            && let (Some(p), Some(v)) = (net_of(&c.refdes, "+"), c.value.split_whitespace().next().and_then(si_value))
        {
            out.entry(p).or_insert(v);
        }
        if c.kind == Kind::Power {
            let t = c.value.trim_start_matches('+').to_ascii_uppercase();
            // 3V3 is 3.3 V; 5V, 12V, 3.3V read as numbers
            let v = if let Some((a, b)) = t.split_once('V').filter(|(a, b)| !a.is_empty() && !b.is_empty() && b.chars().all(|ch| ch.is_ascii_digit())) {
                format!("{a}.{b}").parse::<f64>().ok()
            } else {
                t.strip_suffix('V').and_then(|n| n.parse::<f64>().ok())
            };
            if let Some(v) = v {
                out.entry(c.value.clone()).or_insert(v);
            }
        }
    }
    out
}

/// The roles of part `refdes` with the parts around it. `volts` gives a net's voltage when it is known (the DC result, or
/// `fixed_volts`).
pub fn roles_of(s: &Schematic, refdes: &str, volts: &dyn Fn(&str) -> Option<f64>) -> Vec<Role> {
    let nets = extract(s);
    let mut net_of = HashMap::new();
    for (i, n) in nets.iter().enumerate() {
        for p in &n.pins {
            net_of.entry((p.refdes.clone(), p.pin.clone())).or_insert(i);
        }
    }
    let x = Ctx { s, nets, net_of, volts };
    let Some(c) = x.comp(refdes) else { return vec![] };
    let mut out = vec![];
    match two(c) {
        Some(Two::R) => resistor(&x, c, &mut out),
        Some(Two::C) => capacitor(&x, c, &mut out),
        Some(Two::Led) => led(&x, c, &mut out),
        None if glyph(c) == "opamp" => opamp(&x, c, &mut out),
        None => {}
    }
    out
}

/// An LED in series with a resistor between `from` and `to`: its current, (V(from) − V(to) − VF) / R.
fn led_current(x: &Ctx, r: &Component, led: &Component) -> Option<(f64, String)> {
    let (a, k) = x.ends(led)?;
    let (ra, rb) = x.ends(r)?;
    let mid = if ra == a || ra == k { ra } else if rb == a || rb == k { rb } else { return None };
    let far = if mid == ra { rb } else { ra };
    let other_led_end = if mid == a { k } else { a };
    // the resistor on the anode side: V(far) − VF − V(cathode); on the cathode side: V(anode) − VF − V(far)
    let (hi, lo) = if mid == a { (x.v(far)?, x.v(other_led_end)?) } else { (x.v(other_led_end)?, x.v(far)?) };
    let ohms = x.value(r)?;
    let i = (hi - lo - LED_VF) / ohms;
    (ohms > 0.0).then(|| (i, format!("({} − {} − VF {LED_VF} V) ÷ {}", eng(hi, "V"), eng(lo, "V"), eng(ohms, "Ω"))))
}

fn resistor(x: &Ctx, c: &Component, out: &mut Vec<Role>) {
    let Some((a, b)) = x.ends(c) else { return };
    // an LED's current-limiting resistor
    for end in [a, b] {
        for (led, _) in x.on(end, Two::Led) {
            if x.others(end, &[&c.refdes, &led.refdes]).is_empty() {
                let value = led_current(x, c, led);
                out.push(Role {
                    name: "LED の電流制限".into(),
                    parts: vec![c.refdes.clone(), led.refdes.clone()],
                    value: value.as_ref().map(|(i, _)| format!("約 {}", eng(*i, "A"))),
                    explain: format!(
                        "LED は電圧がわずかに上がるだけで電流が急に増えるので、抵抗で電流を決めます{}。ふつうの LED は 5〜20 mA。抵抗を小さくすると明るく、大きくすると暗くなります",
                        value.map_or(String::new(), |(_, how)| format!(" ({how})"))
                    ),
                });
            }
        }
    }
    // a voltage divider: the picked resistor from a fixed voltage (a supply or ground) to a point X that is not, and a second one
    // from X to another fixed voltage (what else hangs on X is the load)
    let fixed = |n: usize| x.is_gnd(n) || x.is_supply(n);
    for (mid, far) in [(b, a), (a, b)] {
        if !fixed(far) || fixed(mid) {
            continue;
        }
        let partners: Vec<(&Component, usize)> = x.on(mid, Two::R).into_iter().filter(|(r, other)| r.refdes != c.refdes && *other != far && fixed(*other)).collect();
        if partners.len() == 1 && x.on(mid, Two::Led).is_empty() {
            let (r2, far2) = partners[0];
            let (Some(v1), Some(v2), Some(rv1), Some(rv2)) = (x.v(far), x.v(far2), x.value(c), x.value(r2)) else {
                out.push(Role { name: "分圧".into(), parts: vec![c.refdes.clone(), r2.refdes.clone()], value: None, explain: "2 本の抵抗で電圧を分けて、間の点に、両端の間の電圧を作ります".into() });
                continue;
            };
            let vout = v2 + (v1 - v2) * rv2 / (rv1 + rv2);
            out.push(Role {
                name: "分圧".into(),
                parts: vec![c.refdes.clone(), r2.refdes.clone()],
                value: Some(format!("{} = {}", x.name(mid), eng(vout, "V"))),
                explain: format!(
                    "2 本の抵抗で電圧を分けて、間の点 {} に {} を作ります (V = {} × {} ÷ ({} + {}))。間の点に電流をたくさん取ると下がるので、つなぐ先は入力 (電流をほとんど取らないもの) にします",
                    x.name(mid),
                    eng(vout, "V"),
                    eng(v1 - v2, "V"),
                    eng(rv2, "Ω"),
                    eng(rv1, "Ω"),
                    eng(rv2, "Ω")
                ),
            });
        }
    }
    // an RC filter: this resistor and a capacitor from one of its ends to ground (or a fixed voltage)
    rc(x, c, a, b, out);
    // a pull-up / pull-down: from a supply or ground to a net with an input or a switch
    for (rail, sig) in [(a, b), (b, a)] {
        if (x.is_supply(rail) || x.is_gnd(rail)) && !x.is_supply(sig) && !x.is_gnd(sig) && x.has_input(sig) && x.on(sig, Two::R).len() == 1 {
            let up = !x.is_gnd(rail);
            let i = x.v(rail).zip(x.value(c)).filter(|(_, r)| *r > 0.0).map(|(v, r)| v / r);
            out.push(Role {
                name: if up { "プルアップ".into() } else { "プルダウン".into() },
                parts: vec![c.refdes.clone()],
                value: i.filter(|_| up).map(|i| format!("押したとき {}", eng(i, "A"))),
                explain: if up {
                    format!("何もつながっていないとき {} を High ({}) に保ちます。スイッチやオープンドレインの出力が GND に落とすと Low になります。抵抗が無いと入力が浮いて、雑音で 0 と 1 を行き来します", x.name(sig), x.name(rail))
                } else {
                    format!("何もつながっていないとき {} を Low (GND) に保ちます。スイッチが電源につなぐと High になります", x.name(sig))
                },
            });
        }
    }
    // an op amp's feedback or input resistor
    for (end, other) in [(a, b), (b, a)] {
        for p in x.nets[end].pins.clone() {
            let Some(op) = x.comp(&p.refdes).filter(|o| glyph(o) == "opamp") else { continue };
            let Some((pin_name, _)) = x.lib_pin(&op.refdes, &p.pin) else { continue };
            if pin_name.ends_with("IN-") && x.nets[other].pins.iter().any(|q| q.refdes == op.refdes && x.lib_pin(&q.refdes, &q.pin).is_some_and(|(n, _)| n.ends_with("OUT"))) {
                let mut more = vec![];
                opamp(x, op, &mut more);
                out.push(Role {
                    name: "帰還の抵抗 (オペアンプ)".into(),
                    parts: vec![c.refdes.clone(), op.refdes.clone()],
                    value: more.first().and_then(|r| r.value.clone()),
                    explain: "出力の一部を − 入力に戻して (負帰還)、増幅の倍率 (利得) を抵抗の比で決めます。オペアンプそのものの利得は大きくばらつくので、こうして抵抗で決めると安定します".into(),
                });
            }
        }
    }
}

/// A resistor `r` (between `a` and `b`) and a capacitor from one of its ends to ground or a fixed voltage: a low-pass filter (the
/// capacitor on the far end) — or, from the capacitor's side, a high-pass one.
fn rc(x: &Ctx, r: &Component, a: usize, b: usize, out: &mut Vec<Role>) {
    for (inp, mid) in [(a, b), (b, a)] {
        for (cap, to) in x.on(mid, Two::C) {
            if !(x.is_gnd(to) || x.is_supply(to)) || x.is_supply(mid) || x.is_gnd(mid) || x.is_supply(inp) && x.is_gnd(to) && x.feeds_ic(mid) {
                continue;
            }
            let fc = x.value(r).zip(x.value(cap)).filter(|(rv, cv)| *rv > 0.0 && *cv > 0.0).map(|(rv, cv)| 1.0 / (2.0 * std::f64::consts::PI * rv * cv));
            out.push(Role {
                name: "RC ローパスフィルタ".into(),
                parts: vec![r.refdes.clone(), cap.refdes.clone()],
                value: fc.map(|f| format!("fc = {}", eng(f, "Hz"))),
                explain: format!(
                    "{} から来た信号の、速い変化 (高い周波数) をコンデンサが逃がし、ゆっくりした変化だけを {} に通します。カットオフ周波数 fc = 1 ÷ (2π R C) より上は弱まります。スイッチのチャタリング取り・雑音取り・PWM を平らな電圧にするのに使います",
                    x.name(inp),
                    x.name(mid)
                ),
            });
        }
    }
}

fn capacitor(x: &Ctx, c: &Component, out: &mut Vec<Role>) {
    let Some((a, b)) = x.ends(c) else { return };
    let farads = x.value(c);
    // across a supply: bypass (small, beside an IC) or bulk smoothing
    for (rail, gnd) in [(a, b), (b, a)] {
        if x.is_supply(rail) && x.is_gnd(gnd) {
            let small = farads.is_none_or(|f| f <= 1.1e-6);
            if small && x.feeds_ic(rail) {
                out.push(Role {
                    name: "バイパスコンデンサ (パスコン)".into(),
                    parts: vec![c.refdes.clone()],
                    value: farads.map(|f| eng(f, "F")),
                    explain: format!("IC が切り替わる瞬間に要る電流を、すぐそばから出して、電源の線 {} の電圧が揺れないようにします。基板では IC の電源ピンのすぐ近くに置きます (遠いと効きません)", x.name(rail)),
                });
            } else if !small {
                out.push(Role {
                    name: "電源の平滑 (大きなコンデンサ)".into(),
                    parts: vec![c.refdes.clone()],
                    value: farads.map(|f| eng(f, "F")),
                    explain: format!("電源 {} の電圧のでこぼこ (リップル) をならし、負荷が急に電流を取ったときに電荷を貸します。レギュレータの入出力にも、データシートが推奨する値を付けます", x.name(rail)),
                });
            }
        }
    }
    // the capacitor of an RC filter (the resistor feeding its live end)
    for (mid, to) in [(a, b), (b, a)] {
        if (x.is_gnd(to) || x.is_supply(to)) && !x.is_supply(mid) && !x.is_gnd(mid) {
            for (r, inp) in x.on(mid, Two::R) {
                if !x.is_supply(inp) || !x.feeds_ic(mid) {
                    let mut v = vec![];
                    rc(x, r, inp, mid, &mut v);
                    out.extend(v.into_iter().filter(|role| role.parts.contains(&c.refdes)).map(|mut role| {
                        role.parts = vec![c.refdes.clone(), r.refdes.clone()];
                        role
                    }));
                }
            }
        }
    }
    // a coupling capacitor: between two signal nets, passing the change and blocking the DC
    if [a, b].iter().all(|n| !x.is_gnd(*n) && !x.is_supply(*n)) && out.is_empty() {
        let hp = [a, b].into_iter().find_map(|n| x.on(n, Two::R).into_iter().find(|(_, o)| x.is_gnd(*o) || x.is_supply(*o)).map(|(r, _)| r));
        let fc = hp.and_then(|r| x.value(r)).zip(farads).map(|(rv, cv)| 1.0 / (2.0 * std::f64::consts::PI * rv * cv));
        out.push(Role {
            name: "結合コンデンサ (カップリング)".into(),
            parts: vec![c.refdes.clone()],
            value: fc.map(|f| format!("fc = {}", eng(f, "Hz"))),
            explain: "直流は通さず、変化 (交流の信号) だけを次の段へ渡します。前後の段の直流の電圧 (バイアス) がずれていても、信号だけつなげます。下の周波数は、後ろの抵抗との RC で決まります (ハイパスフィルタ)".into(),
        });
    }
}

fn led(x: &Ctx, c: &Component, out: &mut Vec<Role>) {
    let Some((a, k)) = x.ends(c) else { return };
    for end in [a, k] {
        for (r, _) in x.on(end, Two::R) {
            if x.others(end, &[&c.refdes, &r.refdes]).is_empty() {
                let i = led_current(x, r, c);
                out.push(Role {
                    name: "LED (電流制限の抵抗つき)".into(),
                    parts: vec![c.refdes.clone(), r.refdes.clone()],
                    value: i.map(|(i, _)| format!("約 {}", eng(i, "A"))),
                    explain: format!("{} が流れる電流を決めています。LED の明るさは電流でほぼ決まります。電流 = (電源 − LED の電圧 VF) ÷ 抵抗。VF は赤で約 2 V、青・白で約 3 V", r.refdes),
                });
            }
        }
    }
}

/// An op amp's gain from the resistors on its − input: non-inverting 1 + Rf/Rg, inverting −Rf/Rin, a follower when OUT is IN−.
fn opamp(x: &Ctx, c: &Component, out: &mut Vec<Role>) {
    let sym = c.custom.as_ref().unwrap();
    // each channel: the pins whose names end in IN-, IN+, OUT with the same prefix ("1IN-" ...)
    for inm in sym.pins.iter().filter(|p| p.name.to_ascii_uppercase().ends_with("IN-")) {
        let prefix = &inm.name[..inm.name.len() - 3];
        let find = |suffix: &str| sym.pins.iter().find(|p| p.name.eq_ignore_ascii_case(&format!("{prefix}{suffix}")));
        let (Some(outp), Some(inp)) = (find("OUT"), find("IN+")) else { continue };
        let (Some(nm), Some(no), Some(np)) = (x.net(&c.refdes, &inm.number), x.net(&c.refdes, &outp.number), x.net(&c.refdes, &inp.number)) else { continue };
        if nm == no {
            out.push(Role { name: "ボルテージフォロワ".into(), parts: vec![c.refdes.clone()], value: Some("利得 1 倍".into()), explain: "出力を − 入力にそのまま戻すと、出力は + 入力と同じ電圧になります。電流をほとんど取らずに受けて、大きな電流で出せる「緩衝 (バッファ)」です".into() });
            continue;
        }
        let rf = x.on(no, Two::R).into_iter().find(|(_, o)| *o == nm).map(|(r, _)| r);
        let Some(rf) = rf else { continue };
        let rg = x.on(nm, Two::R).into_iter().find(|(r, _)| r.refdes != rf.refdes);
        let Some((rg, far)) = rg else { continue };
        let (Some(f), Some(g)) = (x.value(rf), x.value(rg)) else { continue };
        if x.is_gnd(far) || (x.is_supply(far) && !x.is_gnd(np)) {
            out.push(Role {
                name: "非反転増幅".into(),
                parts: vec![c.refdes.clone(), rf.refdes.clone(), rg.refdes.clone()],
                value: Some(format!("利得 {} 倍", times(1.0 + f / g))),
                explain: format!("+ 入力 ({}) の信号を、同じ向きに 1 + {} ÷ {} 倍にします", x.name(np), rf.refdes, rg.refdes),
            });
        } else {
            out.push(Role {
                name: "反転増幅".into(),
                parts: vec![c.refdes.clone(), rf.refdes.clone(), rg.refdes.clone()],
                value: Some(format!("利得 −{} 倍", times(f / g))),
                explain: format!("{} から {} を通って入る信号を、逆向きに {} ÷ {} 倍にします。+ 入力 ({}) が基準の電圧です", x.name(far), rg.refdes, rf.refdes, rg.refdes, x.name(np)),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn link(s: &mut Schematic, a: (usize, usize), b: (usize, usize)) {
        let pa = s.components[a.0].pins()[a.1].1;
        let pb = s.components[b.0].pins()[b.1].1;
        s.wires.push(Wire { a: pa, b: (pb.0, pa.1) });
        s.wires.push(Wire { a: (pb.0, pa.1), b: pb });
    }

    fn roles(s: &Schematic, r: &str) -> Vec<Role> {
        let v = fixed_volts(s);
        roles_of(s, r, &|n| v.get(n).copied())
    }

    /// 5 V → R1 10k → MID → R2 10k → GND, and on MID an RC to OUT: R3 1k and C1 100n to GND; an LED with R4 330 from 5 V.
    fn sheet() -> Schematic {
        let mut s = Schematic::default();
        let v = s.add(Kind::VSource, (0, 0), 0);
        s.components[v].value = "5".into();
        let g = s.add(Kind::Ground, (0, 40), 0);
        let r1 = s.add(Kind::Resistor, (20, 0), 1);
        let r2 = s.add(Kind::Resistor, (20, 20), 1);
        let r3 = s.add(Kind::Resistor, (40, 10), 0);
        let c1 = s.add(Kind::Capacitor, (60, 20), 1);
        let r4 = s.add(Kind::Resistor, (80, 0), 1);
        let d1 = s.add(Kind::Led, (80, 20), 1);
        for (k, val) in [(r1, "10k"), (r2, "10k"), (r3, "1k"), (c1, "100n"), (r4, "330")] {
            s.components[k].value = val.into();
        }
        link(&mut s, (v, 1), (g, 0));
        link(&mut s, (v, 0), (r1, 0));
        link(&mut s, (r1, 1), (r2, 0));
        link(&mut s, (r2, 1), (g, 0));
        link(&mut s, (r1, 1), (r3, 0));
        link(&mut s, (r3, 1), (c1, 0));
        link(&mut s, (c1, 1), (g, 0));
        link(&mut s, (v, 0), (r4, 0));
        link(&mut s, (r4, 1), (d1, 0));
        link(&mut s, (d1, 1), (g, 0));
        s
    }

    #[test]
    fn a_divider_an_rc_filter_and_an_led_resistor_are_named_with_their_figures() {
        let s = sheet();
        let r1 = roles(&s, "R1");
        let div = r1.iter().find(|r| r.name == "分圧").unwrap_or_else(|| panic!("{r1:?}"));
        assert!(div.value.as_deref().unwrap().ends_with("2.50 V"), "{div:?}");
        assert_eq!(div.parts, ["R1", "R2"]);
        let r3 = roles(&s, "R3");
        let rc = r3.iter().find(|r| r.name == "RC ローパスフィルタ").unwrap_or_else(|| panic!("{r3:?}"));
        assert_eq!(rc.value.as_deref(), Some("fc = 1.59 kHz"));
        let c1 = roles(&s, "C1");
        assert!(c1.iter().any(|r| r.name == "RC ローパスフィルタ" && r.parts == ["C1", "R3"]), "{c1:?}");
        let r4 = roles(&s, "R4");
        let led = r4.iter().find(|r| r.name == "LED の電流制限").unwrap_or_else(|| panic!("{r4:?}"));
        // (5 − 2) / 330 = 9.09 mA
        assert_eq!(led.value.as_deref(), Some("約 9.09 mA"));
        assert!(roles(&s, "D1").iter().any(|r| r.value.as_deref() == Some("約 9.09 mA")));
    }

    #[test]
    fn fixed_volts_reads_sources_and_power_symbols() {
        let mut s = sheet();
        let p = s.add(Kind::Power, (100, 0), 0);
        s.components[p].value = "3V3".into();
        let v = fixed_volts(&s);
        assert_eq!(v.get("3V3"), Some(&3.3));
        assert_eq!(v.get("GND"), Some(&0.0));
        assert!(v.values().any(|x| *x == 5.0));
    }

    fn part(name: &str, glyph: &str, pins: &[(&str, &str, Option<PinKind>)]) -> CustomSymbol {
        CustomSymbol {
            unit: 0,
            name: name.into(),
            prefix: "U".into(),
            pins: pins.iter().map(|(n, nm, k)| SymPin { unit: 0, number: n.to_string(), name: nm.to_string(), side: PinSide::Left, sim_only: false, gap: 0, kind: *k }).collect(),
            sim: None,
            ratings: None,
            glyph: glyph.into(),
            shops: Default::default(),
            programming: vec![],
            companions: vec![],
        }
    }

    /// Names the net at a pin with a label (no wire to cross another pin).
    fn name(s: &mut Schematic, i: usize, pin: &str, net: &str) {
        let at = s.components[i].pins().into_iter().find(|(n, _)| n == pin).unwrap().1;
        s.labels.push(Label { pos: at, text: net.into() });
    }

    #[test]
    fn an_op_amps_gain_and_a_pull_up() {
        let mut s = Schematic::default();
        let v = s.add(Kind::VSource, (0, 0), 0);
        s.components[v].value = "5".into();
        name(&mut s, v, "+", "VCC");
        name(&mut s, v, "-", "GND");
        let sig = s.add(Kind::VSource, (0, 30), 0);
        s.components[sig].value = "1".into();
        name(&mut s, sig, "+", "IN");
        name(&mut s, sig, "-", "GND");
        let op = s.add_custom(part("OPAMP", "opamp", &[("1", "OUT", None), ("2", "IN-", None), ("3", "IN+", None)]), "OPAMP", "SOT", (40, 30), 0);
        name(&mut s, op, "1", "OUT");
        name(&mut s, op, "2", "FB");
        name(&mut s, op, "3", "IN");
        let rf = s.add(Kind::Resistor, (70, 10), 0);
        let rg = s.add(Kind::Resistor, (70, 40), 0);
        s.components[rf].value = "10k".into();
        s.components[rg].value = "1k".into();
        name(&mut s, rf, "1", "OUT");
        name(&mut s, rf, "2", "FB");
        name(&mut s, rg, "1", "FB");
        name(&mut s, rg, "2", "GND");
        let ops = roles(&s, "U1");
        assert!(ops.iter().any(|r| r.name == "非反転増幅" && r.value.as_deref() == Some("利得 11 倍")), "{ops:?}");
        assert!(roles(&s, "R1").iter().any(|r| r.name.starts_with("帰還の抵抗") && r.value.as_deref() == Some("利得 11 倍")), "{:?}", roles(&s, "R1"));
        // a pull-up: 10k from the 5 V net to a logic input
        let mcu = s.add_custom(part("MCU", "", &[("1", "PB0", Some(PinKind::Input))]), "MCU", "DIP", (100, 0), 0);
        name(&mut s, mcu, "1", "BTN");
        let rp = s.add(Kind::Resistor, (100, 30), 0);
        s.components[rp].value = "10k".into();
        name(&mut s, rp, "1", "VCC");
        name(&mut s, rp, "2", "BTN");
        let up = roles(&s, "R3");
        assert!(up.iter().any(|r| r.name == "プルアップ" && r.value.as_deref() == Some("押したとき 500 µA")), "{up:?}");
    }

    #[test]
    fn small_helpers() {
        assert_eq!(eng(1591.5, "Hz"), "1.59 kHz");
        assert_eq!(eng(0.00909, "A"), "9.09 mA");
        assert_eq!(eng(2.5, "V"), "2.50 V");
        assert_eq!(eng(100e-9, "F"), "100 nF");
    }
}
