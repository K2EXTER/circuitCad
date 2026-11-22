//! The moving dots of the DC result (X1): the current along each piece of wire (`sch_core::flow`) from the pins' currents of the
//! operating point, drawn by `crate::flow_dots`.

use super::*;
use sch_core::flow::{wire_flows, WireFlow};
use std::collections::HashMap;

impl SchEditor {
    /// The current along every piece of wire at the operating point `res` (empty when the circuit cannot be made).
    pub(super) fn flows_of(&self, res: &DcResult) -> Vec<WireFlow> {
        let Ok(c) = self.sim_circuit() else { return vec![] };
        let parts = sim_core::part_currents(&c, res);
        // the node of each pin's net, and how many pins of the part share it (a part's current into a node is shared out among
        // its pins there). With the board's copper in, the copper in front of a pin is named after the part (`R1.1.RW`), so what
        // the part puts into the net has already gone through it
        let mut node_of: HashMap<(String, String), usize> = HashMap::new();
        let mut share: HashMap<(String, usize), usize> = HashMap::new();
        for net in extract(&self.sch) {
            for p in &net.pins {
                let Some(n) = c.find_node(&net.name) else { continue };
                node_of.insert((p.refdes.clone(), p.pin.clone()), n);
                *share.entry((p.refdes.clone(), n)).or_default() += 1;
            }
        }
        wire_flows(&self.sch, |refdes, pin| {
            let n = *node_of.get(&(refdes.to_string(), pin.to_string()))?;
            let into = parts.get(refdes)?;
            Some(into.get(n)? / share[&(refdes.to_string(), n)] as f64)
        })
    }

    /// The dots, while a DC result of this circuit is showing and the setting is on.
    pub(super) fn draw_flow_dots(&self, painter: &egui::Painter, rect: Rect) {
        if !crate::flow_dots::on() || self.dc_stale() {
            return;
        }
        let Some(d) = self.dc.as_ref().filter(|d| d.outcome.is_ok()) else { return };
        if d.flows.iter().all(|f| f.amps.abs() < crate::flow_dots::MIN_AMPS) {
            return;
        }
        let t = painter.ctx().input(|i| i.time);
        // a dot every grid step, at least 10 px apart when zoomed out
        let spacing = (self.zoom * 1.0).max(10.0);
        let radius = (self.zoom * 0.13).clamp(1.8, 4.0);
        let col = crate::canvas_theme::col(crate::canvas_theme::Role::Current);
        let painter = painter.with_clip_rect(rect);
        for f in &d.flows {
            let (a, b) = (self.to_screen(rect, (f.a.0 as f32, f.a.1 as f32)), self.to_screen(rect, (f.b.0 as f32, f.b.1 as f32)));
            crate::flow_dots::paint(&painter, &crate::flow_dots::dots_on(a, b, f.amps, t, spacing), radius, col);
        }
        crate::flow_dots::keep_moving(painter.ctx());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The LED sample at its operating point: every wire carries the LED's current, out of the source's + round to its −.
    #[test]
    fn the_wires_of_the_led_sample_carry_the_led_current_round_the_loop() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/led.schjson");
        let mut e = SchEditor::default();
        e.sch = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        e.run_dc();
        let d = e.dc.as_ref().unwrap();
        let i = d.outcome.as_ref().unwrap().element("D1").unwrap().current;
        assert!(i > 5e-3, "{i}");
        let along = |a: Pt, b: Pt| d.flows.iter().find_map(|f| if (f.a, f.b) == (a, b) { Some(f.amps) } else if (f.a, f.b) == (b, a) { Some(-f.amps) } else { None }).unwrap();
        // out of the + pin, along the top to the resistor, from the resistor to the LED, down from the LED
        for (a, b) in [((0, -2), (0, -6)), ((0, -6), (6, -6)), ((10, -6), (14, -6)), ((18, -6), (18, 6))] {
            assert!((along(a, b) - i).abs() < 1e-9 * i.max(1.0) + 1e-9, "{a:?} -> {b:?}: {} vs {i}", along(a, b));
        }
        // back along the bottom to the source's − pin
        for (a, b) in [((18, 6), (0, 6)), ((0, 6), (0, 2))] {
            assert!((along(a, b) - i).abs() < 1e-9 + 1e-9 * i, "{a:?} -> {b:?}: {}", along(a, b));
        }
        // the dots go when the setting is on and the result is this circuit's; not on a stale result
        e.sig_cache = e.signature();
        assert!(!e.dc_stale());
        e.sch.components[1].value = "1k".into();
        e.sig_cache = e.signature();
        assert!(e.dc_stale(), "the result is from before the change: no dots");
    }
}
