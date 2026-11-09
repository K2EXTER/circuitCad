//! DC operating point and the voltage overlay data.

use super::*;

/// The RMS current of every net and the burst/peak figures (`tran_net_currents`).
pub type NetCurrents = (Vec<(String, f64)>, Vec<sim_core::NetStat>);

impl SchEditor {
    /// Problems that would stop the simulation (a value that is not a number, an unsupported part),
    /// as ERC findings placed on the offending part.
    pub(super) fn conversion_problems(&self) -> Vec<Problem> {
        let Err(errs) = from_schematic(&self.sch) else { return vec![] };
        errs.into_iter()
            .map(|e| Problem {
                sheet: self.sch.active_sheet,
                at: self.sch.components.iter().find(|c| c.refdes == e.refdes).map(|c| c.pos),
                message: format!("シミュレーションできません: {e}"),
                hint: "部品の値は 10k、4.7u、100n のように、数字のあとに k (キロ)・m (ミリ)・u (マイクロ)・n (ナノ) などを付けて書きます。部品を選んで、左の「値」を直しましょう".into(),
                severity: Severity::Error,
                pin: None,
            })
            .collect()
    }

    /// The circuit to simulate: the schematic converted, at the temperature set in the panel.
    pub(super) fn sim_circuit(&self) -> Result<Circuit, Vec<String>> {
        let temp = match self.temp_c.trim() {
            "" => T_NOM_C,
            t => match t.parse::<f64>() {
                Ok(v) if v > -273.0 && v < 1000.0 => v,
                _ => return Err(vec![format!("温度「{t}」を数値(℃)で入力してください (例: 27)")]),
            },
        };
        let c = sim_core::from_schematic_with_layout(&self.sch, &self.wiring, &self.couplings).map_err(|errs| errs.iter().map(|e| e.to_string()).collect::<Vec<_>>())?;
        Ok(c.at_temperature(temp))
    }

    /// The current each net carries at the DC operating point (for sizing the board's copper).
    pub fn dc_net_currents(&self) -> Result<Vec<(String, f64)>, String> {
        let sig = self.signature();
        if let Some((k, r)) = self.dc_currents_cache.borrow().as_ref()
            && *k == sig {
                return r.clone();
            }
        let result = (|| {
            let c = self.sim_circuit().map_err(|e| e.join(" / "))?;
            let r = dc_op(&c).map_err(|e| format!("DC解析ができませんでした: {e}"))?;
            Ok(sim_core::net_currents(&c, &r))
        })();
        *self.dc_currents_cache.borrow_mut() = Some((sig, result.clone()));
        self.dc_currents_rev.set(Some(self.rev));
        result
    }

    /// Are the DC currents of the nets at hand (worked out for this circuit already)? Cheap: it goes by the edit counter.
    pub fn dc_currents_ready(&self) -> bool {
        self.dc_currents_rev.get() == Some(self.rev)
    }

    /// The RMS current of every net over the settled part of the last transient run (the second half), with the
    /// burst and peak figures for the warning. For circuits whose current only exists in time.
    pub fn tran_net_currents(&self) -> Result<NetCurrents, String> {
        let Some(Ok(tr)) = self.tran.as_ref().map(|t| &t.outcome) else { return Err("先に回路図で「過渡解析を実行」してください (電流は、その結果の後半から求めます)".into()) };
        let c = self.sim_circuit().map_err(|e| e.join(" / "))?;
        let stats = sim_core::net_currents_tran(&c, tr, 0.5);
        Ok((stats.iter().map(|s| (s.net.clone(), s.rms)).collect(), stats))
    }

    pub(super) fn signature_cached(&self) -> &str {
        &self.sig_cache
    }

    pub(super) fn signature(&self) -> String {
        let mut s = serde_json::to_string(&self.sch).unwrap_or_default();
        for w in &self.wiring {
            s += &format!("|{}.{}={}/{}", w.refdes, w.pin, w.ohm, w.henry);
        }
        for k in &self.couplings {
            s += &format!("|{}.{}~{}.{}={}", k.a.0, k.a.1, k.b.0, k.b.1, k.k);
        }
        s
    }

    pub(super) fn run_dc(&mut self) {
        let outcome = match self.sim_circuit() {
            Err(errs) => Err(errs),
            Ok(c) => dc_op(&c).map_err(|e| {
                let mut msgs = vec![e.to_string()];
                msgs.extend(diagnose(&c, &e).into_iter().map(|h| format!("ヒント: {h}")));
                msgs
            }),
        };
        self.status = match &outcome {
            Ok(r) => format!("DC動作点を計算しました (反復 {} 回)", r.iterations),
            Err(e) => format!("DC解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.dc = Some(self.dc_view(outcome, None));
    }

    /// The overlay data (pin voltages, wire colours) for a DC result; `fault` names the broken part when the result is a what-if.
    pub(super) fn dc_view(&self, outcome: Result<DcResult, Vec<String>>, fault: Option<String>) -> DcView {
        let mut pin_v = vec![];
        // the points that are ground (no voltage label: a ground is 0 V by definition)
        let mut ground_pts = vec![];
        if let Ok(res) = &outcome {
            for net in extract(&self.sch) {
                let Some(v) = res.voltage(&net.name) else { continue };
                let ground = net.name == "GND" || net.name == "0";
                for p in &net.pins {
                    // with the board's copper resistance in front of the pin, show the voltage that reaches the pin
                    let v = res.voltage(&format!("{}.{}.wire", p.refdes, p.pin)).unwrap_or(v);
                    if let Some(c) = self.sch.components.iter().find(|c| c.refdes == p.refdes)
                        && let Some((_, pt)) = c.pins().into_iter().find(|(n, _)| *n == p.pin) {
                            pin_v.push((pt, v));
                            if ground {
                                ground_pts.push(pt);
                            }
                        }
                }
            }
            // ground symbols and named power symbols show their net voltage too
            for c in self.sch.components.iter().filter(|c| c.kind.is_power_symbol()) {
                let name = if c.kind == Kind::Ground { "GND".to_string() } else { c.value.clone() };
                if let Some(v) = res.voltage(&name) {
                    pin_v.push((c.pins()[0].1, v));
                    if c.kind == Kind::Ground || name == "GND" {
                        ground_pts.push(c.pins()[0].1);
                    }
                }
            }
        }
        let marks = self.voltage_marks(&pin_v, &ground_pts);
        let mut wire_v = vec![None; self.sch.wires.len()];
        let mut v_range = (0.0f64, 0.0f64);
        if let Ok(res) = &outcome {
            for (k, w) in self.sch.wires.iter().enumerate() {
                wire_v[k] = net_at(&self.sch, w.a).and_then(|n| res.voltage(&n));
                // behind the board's copper resistance the wire that ends at a pin takes that pin's voltage
                // (the mean of the two ends when both are pins), so the colours show the drop along the wiring
                if !self.wiring.is_empty() {
                    let ends: Vec<f64> = [w.a, w.b].iter().filter_map(|e| pin_v.iter().find(|(pt, _)| pt == e).map(|(_, v)| *v)).collect();
                    if !ends.is_empty() {
                        wire_v[k] = Some(ends.iter().sum::<f64>() / ends.len() as f64);
                    }
                }
            }
            let vs = res.voltages.iter().map(|(_, v)| *v);
            v_range = (vs.clone().fold(0.0, f64::min), vs.fold(0.0, f64::max));
        }
        let ratings = outcome.as_ref().map(|r| sim_core::check_ratings(&self.with_library_ratings(), r)).unwrap_or_default();
        DcView { outcome, signature: self.signature(), pin_v, marks, wire_v, v_range, fault, ratings }
    }

    /// Where the voltages are written on the sheet: one per piece of drawn wiring (the pins and wires that touch each other),
    /// not one per pin, so a wire does not carry the same "5.000 V" at both ends. A net split by net labels into pieces keeps a
    /// value on each piece; pins on one piece that read differently (copper resistance in front of them) all keep theirs.
    /// Ground at exactly 0 V is left out (a ground pin raised by copper resistance is shown). The point chosen is a power symbol, then a net label, then the topmost (then leftmost) pin.
    fn voltage_marks(&self, pin_v: &[(Pt, f64)], ground_pts: &[Pt]) -> Vec<(Pt, f64)> {
        // union-find over pins and wire ends; a point on the inside of a wire (a T junction) joins that wire
        let mut pts: Vec<Pt> = pin_v.iter().map(|(p, _)| *p).collect();
        for w in &self.sch.wires {
            pts.extend([w.a, w.b]);
        }
        pts.sort();
        pts.dedup();
        let idx = |p: Pt| pts.binary_search(&p).ok();
        let mut parent: Vec<usize> = (0..pts.len()).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let join = |parent: &mut Vec<usize>, a: usize, b: usize| {
            let (ra, rb) = (root(parent, a), root(parent, b));
            parent[ra] = rb;
        };
        let on_wire = |p: Pt, a: Pt, b: Pt| {
            let cross = (b.0 - a.0) as i64 * (p.1 - a.1) as i64 - (b.1 - a.1) as i64 * (p.0 - a.0) as i64;
            cross == 0 && p.0 >= a.0.min(b.0) && p.0 <= a.0.max(b.0) && p.1 >= a.1.min(b.1) && p.1 <= a.1.max(b.1)
        };
        for w in &self.sch.wires {
            let (Some(a), Some(b)) = (idx(w.a), idx(w.b)) else { continue };
            join(&mut parent, a, b);
            for (k, p) in pts.iter().enumerate() {
                if on_wire(*p, w.a, w.b) {
                    join(&mut parent, k, a);
                }
            }
        }
        let rank = |p: Pt| {
            if self.sch.components.iter().any(|c| c.kind == Kind::Power && c.pos == p) {
                0
            } else if self.sch.labels.iter().any(|l| l.pos == p) {
                1
            } else {
                2
            }
        };
        // a ground pin is left out only while it really is at 0 V: behind the board's copper resistance it rises (ground bounce),
        // and that is exactly what the user is looking for
        let mut order: Vec<&(Pt, f64)> = pin_v.iter().filter(|(p, v)| !(ground_pts.contains(p) && v.abs() < 1e-9)).collect();
        order.sort_by_key(|(p, _)| (rank(*p), p.1, p.0));
        let mut marks: Vec<(Pt, f64)> = vec![];
        let mut taken: Vec<(usize, String)> = vec![];
        for (p, v) in order {
            let Some(i) = idx(*p) else { continue };
            let piece = root(&mut parent, i);
            let text = eng(*v, "V");
            if taken.iter().any(|(r, t)| *r == piece && *t == text) || marks.iter().any(|(q, _)| q == p) {
                continue;
            }
            taken.push((piece, text));
            marks.push((*p, *v));
        }
        marks
    }

    /// The sheet with data sheet ratings filled in from the library for placed parts that were saved without
    /// them (circuits from before the ratings existed).
    pub(super) fn with_library_ratings(&self) -> Schematic {
        let mut s = self.sch.clone();
        for c in &mut s.components {
            if let Some(sym) = c.custom.as_mut()
                && let Some(d) = self.library.iter().find(|d| d.name == sym.name) {
                    match sym.ratings.as_mut() {
                        None => sym.ratings = d.ratings.clone().filter(|r| !r.is_empty()),
                        // a capacitor saved before ripple current and life were part of the ratings gets them from the library
                        Some(r) => {
                            if r.ripple.is_none() {
                                r.ripple = d.ratings.as_ref().and_then(|x| x.ripple.clone());
                            }
                            if r.life.is_none() {
                                r.life = d.ratings.as_ref().and_then(|x| x.life.clone());
                            }
                        }
                    }
                }
        }
        s
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
