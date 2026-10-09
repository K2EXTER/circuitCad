//! DC operating point and the voltage overlay data.

use super::*;

impl SchEditor {
    /// Problems that would stop the simulation (a value that is not a number, an unsupported part),
    /// as ERC findings placed on the offending part.
    pub(super) fn conversion_problems(&self) -> Vec<Problem> {
        let Err(errs) = from_schematic(&self.sch) else { return vec![] };
        errs.into_iter()
            .map(|e| Problem {
                at: self.sch.components.iter().find(|c| c.refdes == e.refdes).map(|c| c.pos),
                message: format!("シミュレーションできません: {e}"),
                hint: "部品の値は 10k、4.7u、100n のように、数字のあとに k (キロ)・m (ミリ)・u (マイクロ)・n (ナノ) などを付けて書きます。部品を選んで、左の「値」を直しましょう".into(),
                severity: Severity::Error,
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
        let c = from_schematic(&self.sch).map_err(|errs| errs.iter().map(|e| e.to_string()).collect::<Vec<_>>())?;
        Ok(c.at_temperature(temp))
    }

    pub(super) fn signature_cached(&self) -> &str {
        &self.sig_cache
    }

    pub(super) fn signature(&self) -> String {
        serde_json::to_string(&self.sch).unwrap_or_default()
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
            Ok(r) => format!("DC動作点: 収束 ({} 回, {})", r.iterations, r.strategy),
            Err(e) => format!("DC解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.dc = Some(self.dc_view(outcome, None));
    }

    /// The overlay data (pin voltages, wire colours) for a DC result; `fault` names the broken part when the result is a what-if.
    pub(super) fn dc_view(&self, outcome: Result<DcResult, Vec<String>>, fault: Option<String>) -> DcView {
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
        let mut wire_v = vec![None; self.sch.wires.len()];
        let mut v_range = (0.0f64, 0.0f64);
        if let Ok(res) = &outcome {
            for (k, w) in self.sch.wires.iter().enumerate() {
                wire_v[k] = net_at(&self.sch, w.a).and_then(|n| res.voltage(&n));
            }
            let vs = res.voltages.iter().map(|(_, v)| *v);
            v_range = (vs.clone().fold(0.0, f64::min), vs.fold(0.0, f64::max));
        }
        DcView { outcome, signature: self.signature(), pin_v, wire_v, v_range, fault }
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
