use super::*;

pub(super) fn set_value(s: &mut Schematic, refdes: &str, value: &str) {
    if let Some(c) = s.components.iter_mut().find(|c| c.refdes == refdes) {
        c.value = value.into();
    }
}

/// The sample turned into a passing answer: parts dropped, values set, output labels added.
pub(super) fn solution_of(ch: &Challenge) -> Schematic {
    let mut s: Schematic = serde_json::from_str(ch.solution).unwrap_or_default();
    for r in ch.drop {
        if let Some(c) = s.components.iter().find(|c| c.refdes == *r) {
            let pins: Vec<Pt> = c.pins().into_iter().map(|(_, p)| p).collect();
            s.labels.retain(|l| !pins.contains(&l.pos));
            s.wires.retain(|w| !pins.contains(&w.a) && !pins.contains(&w.b));
        }
        s.components.retain(|c| c.refdes != *r);
    }
    for (r, v) in ch.values {
        set_value(&mut s, r, v);
    }
    for l in &mut s.labels {
        if let Some((_, to)) = ch.rename.iter().find(|(from, _)| *from == l.text) {
            l.text = (*to).into();
        }
    }
    for (r, pin, net) in ch.labels {
        if let Some((_, p)) = s.components.iter().find(|c| c.refdes == *r).and_then(|c| c.pins().into_iter().find(|(n, _)| n == pin)) {
            s.labels.push(Label { pos: p, text: (*net).into() });
        }
    }
    s
}

/// The starting sheet of a level: the parts of the level with no wire, and ground symbols.
pub(super) fn starting_sheet(ch: &Challenge, level: u8) -> Schematic {
    let mut full = solution_of(ch);
    full.wires.clear();
    full.labels.clear();
    let keep = if level == 0 { ch.parts0 } else { ch.parts1 };
    full.components.retain(|c| keep.iter().any(|(r, _)| *r == c.refdes));
    for c in &mut full.components {
        if let Some((_, pos)) = keep.iter().find(|(r, _)| *r == c.refdes) {
            c.pos = *pos;
        }
    }
    for (r, v) in ch.start_values {
        set_value(&mut full, r, v);
    }
    for (r, pin, dx, dy) in if level == 0 { ch.grounds0 } else { ch.grounds1 } {
        let at = full.components.iter().find(|c| c.refdes == *r).and_then(|c| c.pins().into_iter().find(|(n, _)| n == pin)).map(|(_, p)| (p.0 + dx, p.1 + dy));
        if let Some(p) = at {
            full.add(Kind::Ground, p, 0);
        }
    }
    full
}

/// A copy with `set` applied and, if `load` is given, a load resistor (the checker's own, named RTEST) from VOUT to GND.
pub(super) fn prepared(s: &Schematic, load: Option<&str>, set: &[(&str, &str)]) -> Schematic {
    let mut t = s.clone();
    for (r, v) in set {
        // the first voltage source is meant when the part number is not on the sheet (a learner may renumber)
        if t.components.iter().any(|c| c.refdes == *r) {
            set_value(&mut t, r, v);
        }
    }
    if let Some(ohms) = load {
        let i = t.add(Kind::Resistor, (400, 400), 0);
        t.components[i].refdes = "RTEST".into();
        t.components[i].value = ohms.into();
        let pins = t.components[i].pins();
        for (name, net) in [("1", "VOUT"), ("2", "GND")] {
            if let Some((_, p)) = pins.iter().find(|(n, _)| n == name) {
                t.labels.push(Label { pos: *p, text: net.into() });
            }
        }
    }
    t
}

/// The DC solution of `s`, or why there is none (with the simulator's advice).
pub(super) fn solve(s: &Schematic) -> Result<sim_core::DcResult, String> {
    let c = sim_core::from_schematic(s).map_err(|e| e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" / "))?;
    sim_core::dc_op(&c).map_err(|e| {
        let mut m = e.to_string();
        for h in sim_core::diagnose(&c, &e) {
            m.push_str(&format!(" ヒント: {h}"));
        }
        m
    })
}

/// The advice for a measured value: the first range of `explain` that contains it, with {v} / {mA} filled in.
pub(super) fn advice(explain: &[(f64, f64, &str)], v: f64) -> String {
    explain
        .iter()
        .find(|(lo, hi, _)| v >= *lo && v < *hi)
        .map(|(_, _, t)| t.replace("{v}", &format!("{v:.2}")).replace("{mA}", &format!("{:.1}", v * 1e3)))
        .unwrap_or_default()
}

/// The index (in `nets`) of the net that has this pin of this part.
fn net_of(nets: &[sch_core::Net], refdes: &str, pin: &str) -> Option<usize> {
    nets.iter().position(|n| n.pins.iter().any(|p| p.refdes == refdes && p.pin == pin))
}

fn pin_name(p: (&str, &str)) -> String {
    format!("{} の {} 番ピン", p.0, p.1)
}

/// Why the pins of a group are not all on one net, if they are not.
fn joined_problem(nets: &[sch_core::Net], group: &[(&str, &str)]) -> Option<String> {
    let first = group.first()?;
    let Some(n0) = net_of(nets, first.0, first.1) else { return Some(format!("{} が、どこにもつながっていません", pin_name(*first))) };
    for p in &group[1..] {
        match net_of(nets, p.0, p.1) {
            None => return Some(format!("{} が、どこにもつながっていません", pin_name(*p))),
            Some(n) if n != n0 => return Some(format!("{} が、{} とつながっていません", pin_name(*p), pin_name(*first))),
            _ => {}
        }
    }
    None
}

/// A part of this kind: the basic part, or a library chip part drawn as one (a `C_100n_0603` is a capacitor too).
fn is_kind(c: &Component, kind: Kind) -> bool {
    if c.kind == kind {
        return true;
    }
    let glyph = c.custom.as_ref().map(|s| s.glyph.as_str()).unwrap_or("");
    c.kind == Kind::Custom && match kind {
        Kind::Capacitor => matches!(glyph, "capacitor" | "electrolytic"),
        Kind::Resistor => glyph == "resistor",
        Kind::Inductor => glyph == "inductor",
        _ => false,
    }
}

/// Why a `Link` is not met: too few parts of `kind` between the two nets, or their values are off.
fn between_problem(sch: &Schematic, nets: &[sch_core::Net], kind: Kind, l: &Link) -> Option<String> {
    let (Some(na), Some(nb)) = (net_of(nets, l.a.0, l.a.1), net_of(nets, l.b.0, l.b.1)) else {
        return Some(format!("{}: {} か {} が、どこにもつながっていません", l.name, pin_name(l.a), pin_name(l.b)));
    };
    if na == nb {
        return Some(format!("{}: {} と {} が、同じネットになっています (間に部品を入れるには、つなぎません)", l.name, pin_name(l.a), pin_name(l.b)));
    }
    let mut across = vec![];
    for c in sch.components.iter().filter(|c| is_kind(c, kind)) {
        let pins = c.pins();
        let ends: Vec<Option<usize>> = pins.iter().map(|(n, _)| net_of(nets, &c.refdes, n)).collect();
        if ends.len() == 2 && ((ends[0] == Some(na) && ends[1] == Some(nb)) || (ends[0] == Some(nb) && ends[1] == Some(na))) {
            across.push(c);
        }
    }
    let fine: Vec<&&Component> = across.iter().filter(|c| parse_value(&c.value).is_some_and(|v| v >= l.lo && v <= l.hi)).collect();
    if fine.len() >= l.min {
        return None;
    }
    if across.len() >= l.min {
        let c = across.iter().find(|c| !parse_value(&c.value).is_some_and(|v| v >= l.lo && v <= l.hi))?;
        return Some(format!("{}: {} の値 {} が、目安の範囲 ({} 〜 {}) から外れています", l.name, c.refdes, c.value, format_value(l.lo), format_value(l.hi)));
    }
    Some(format!("{}: {} 個必要ですが、{} 個しか見つかりません", l.name, l.min, across.len()))
}

impl SchEditor {
    /// Judges the circuit on the sheet by simulating it.
    pub(super) fn judge_challenge(&self) -> Vec<CheckResult> {
        let Some(ch) = self.challenge.as_ref().and_then(|a| challenge(a.id)) else { return vec![] };
        let mut out: Vec<CheckResult> = vec![];
        // set when a gating check fails: why the rest waits
        let mut waiting: Option<String> = None;
        for check in ch.checks {
            let label = match check {
                Check::Erc => "配線ミスの点検 (ERC) にエラーがない".to_string(),
                Check::Named { net } => format!("出力のネットに「{net}」のラベルがある"),
                Check::Voltage { label, .. } | Check::Stay { label, .. } | Check::Current { label, .. } | Check::Gain { label, .. } | Check::Has { label, .. } | Check::Joined { label, .. } | Check::Between { label, .. } | Check::Ratings { label, .. } => label.to_string(),
            };
            if let Some(why) = &waiting {
                out.push(CheckResult { label, ok: None, detail: why.clone() });
                continue;
            }
            let (ok, detail, gate_wait): (bool, String, Option<String>) = match check {
                Check::Erc => {
                    let errors: Vec<String> = erc(&self.sch).into_iter().filter(|p| p.severity == Severity::Error).map(|p| p.message).collect();
                    (errors.is_empty(), errors.first().map(|m| format!("{m} (「作図」タブの ERC の一覧で、場所を確かめられます)")).unwrap_or_default(), None)
                }
                Check::Named { net } => {
                    let named = extract(&self.sch).iter().any(|n| n.name == *net && !n.pins.is_empty());
                    let detail = if named { String::new() } else { format!("出力 (部品の間の点) に、ラベルツールで「{net}」という名前を付けてください。判定は、このネットを測ります") };
                    (named, detail, Some(format!("「{net}」のラベルを付けてから判定します")))
                }
                Check::Voltage { net, load, set, lo, hi, explain, gate, .. } => match solve(&prepared(&self.sch, *load, set)) {
                    Ok(dc) => match dc.voltage(net) {
                        Some(v) => {
                            let ok = v >= *lo && v <= *hi;
                            let detail = if ok { format!("出力 {v:.2} V") } else { advice(explain, v) };
                            (ok, detail, gate.then(|| "前の項目を直してから判定します".to_string()))
                        }
                        None => (false, format!("{net} というネットが見つかりません"), gate.then(|| "前の項目を直してから判定します".to_string())),
                    },
                    Err(e) => (false, format!("回路を解けませんでした: {e}"), gate.then(|| "前の項目を直してから判定します".to_string())),
                },
                Check::Stay { net, load, vary: (src, a, b), lo, hi, hint, .. } => {
                    let at = |v: &str| solve(&prepared(&self.sch, *load, &[(src, v)])).ok().and_then(|d| d.voltage(net));
                    match (at(a), at(b)) {
                        (Some(x), Some(y)) => {
                            let ok = x >= *lo && y <= *hi && y >= *lo && x <= *hi;
                            (ok, if ok { format!("{a}V のとき {x:.2} V、{b}V のとき {y:.2} V") } else { format!("{a}V のとき {x:.2} V、{b}V のとき {y:.2} V と、動きすぎです。{hint}") }, None)
                        }
                        _ => (false, "入力を変えて解けませんでした".to_string(), None),
                    }
                }
                Check::Current { who, set, lo, hi, explain, .. } => {
                    let s = prepared(&self.sch, None, set);
                    let refdes = match who {
                        Who::Refdes(r) => Some(r.to_string()),
                        Who::Kind(k) => s.components.iter().find(|c| c.kind == *k).map(|c| c.refdes.clone()),
                    };
                    match refdes.and_then(|r| solve(&s).ok().and_then(|d| d.element(&r).map(|e| e.current.abs()))) {
                        Some(i) => {
                            let ok = i >= *lo && i <= *hi;
                            (ok, if ok { format!("電流 {:.1} mA", i * 1e3) } else { advice(explain, i) }, None)
                        }
                        None => (false, "電流を測れませんでした (その部品が回路にあるか、回路が解けるかを確かめてください)".to_string(), None),
                    }
                }
                Check::Gain { freq, lo, hi, explain, .. } => {
                    let gain = sim_core::from_schematic(&prepared(&self.sch, None, &[])).ok().and_then(|c| {
                        let st = sim_core::AcSettings { fstart: *freq, fstop: freq * 10.0, sweep: sim_core::AcSweep::Decade(1) };
                        let r = sim_core::ac_analysis(&c, &st).ok()?;
                        r.signal("V(VOUT)").and_then(|s| s.db().first().copied())
                    });
                    match gain {
                        Some(g) => {
                            let ok = g >= *lo && g <= *hi;
                            (ok, if ok { format!("{freq:.0}Hz で {g:.1} dB") } else { advice(explain, g) }, None)
                        }
                        None => (false, "周波数特性を計算できませんでした (入力の電源が「AC 1」か、VOUT のラベルがあるかを確かめてください)".to_string(), None),
                    }
                }
                Check::Has { kind, min, hint, .. } => {
                    let n = self.sch.components.iter().filter(|c| c.kind == *kind).count();
                    (n >= *min, if n >= *min { format!("{n} 個あります") } else { format!("{n} 個しかありません。{hint}") }, None)
                }
                Check::Joined { groups, hint, .. } => {
                    let nets = extract(&self.sch);
                    let bad = groups.iter().find_map(|g| joined_problem(&nets, g));
                    match bad {
                        None => (true, String::new(), None),
                        Some(why) => (false, format!("{why}。{hint}"), None),
                    }
                }
                Check::Between { kind, links, hint, .. } => {
                    let nets = extract(&self.sch);
                    let bad = links.iter().find_map(|l| between_problem(&self.sch, &nets, *kind, l));
                    match bad {
                        None => (true, String::new(), None),
                        Some(why) => (false, format!("{why}。{hint}"), None),
                    }
                }
                Check::Ratings { load, set, .. } => {
                    let worst = self.with_ratings_of(&prepared(&self.sch, *load, set));
                    let over: Vec<String> = match solve(&worst) {
                        Ok(dc) => sim_core::check_ratings(&worst, &dc).into_iter().filter(|i| i.refdes != "RTEST" && i.level == sim_core::RatingLevel::Over).map(|i| i.message).collect(),
                        Err(_) => vec![],
                    };
                    let cond = load.map(|l| format!("負荷 {l}Ω のとき、")).unwrap_or_default();
                    (over.is_empty(), if over.is_empty() { format!("{cond}いちばん厳しい条件でも、定格の範囲です").replace("のとき、いちばん", "のいちばん") } else { format!("{cond}{}。抵抗を大きくする (または、定格の大きい部品にする) と直ります", over[0]) }, None)
                }
            };
            if !ok {
                waiting = gate_wait.clone();
            }
            out.push(CheckResult { label, ok: Some(ok), detail });
        }
        out
    }

    /// The sheet with the data sheet ratings of library parts filled in for parts that were placed without them.
    pub(super) fn with_ratings_of(&self, s: &Schematic) -> Schematic {
        let mut t = s.clone();
        for c in &mut t.components {
            if let Some(sym) = c.custom.as_mut()
                && sym.ratings.is_none()
                && let Some(d) = self.library.iter().find(|d| d.name == sym.name) {
                    sym.ratings = d.ratings.clone().filter(|r| !r.is_empty());
                }
        }
        t
    }
}
