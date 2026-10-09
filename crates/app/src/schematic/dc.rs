//! DC operating point and the voltage overlay data.

use super::*;

impl SchEditor {
    pub(super) fn signature_cached(&self) -> &str {
        &self.sig_cache
    }

    pub(super) fn signature(&self) -> String {
        serde_json::to_string(&self.sch).unwrap_or_default()
    }

    pub(super) fn run_dc(&mut self) {
        let outcome = match from_schematic(&self.sch) {
            Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
            Ok(c) => dc_op(&c).map_err(|e| vec![e.to_string()]),
        };
        let mut pin_v = vec![];
        if let Ok(res) = &outcome {
            for net in extract(&self.sch) {
                let Some(v) = res.voltage(&net.name) else { continue };
                for p in &net.pins {
                    if let Some(c) = self.sch.components.iter().find(|c| c.refdes == p.refdes)
                        && let Some((_, pt)) = c.pins().into_iter().find(|(n, _)| *n == p.pin) {
                            pin_v.push((pt, v));
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
        let mut wire_v = vec![None; self.sch.wires.len()];
        let mut v_range = (0.0f64, 0.0f64);
        if let Ok(res) = &outcome {
            for (k, w) in self.sch.wires.iter().enumerate() {
                wire_v[k] = net_at(&self.sch, w.a).and_then(|n| res.voltage(&n));
            }
            let vs = res.voltages.iter().map(|(_, v)| *v);
            v_range = (vs.clone().fold(0.0, f64::min), vs.fold(0.0, f64::max));
        }
        self.dc = Some(DcView { outcome, signature: self.signature(), pin_v, wire_v, v_range });
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
