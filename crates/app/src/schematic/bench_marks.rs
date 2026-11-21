//! The DC readings taken on the real board (the bench tab's 「測る」, `bench_data::DcReading`) drawn on the sheet beside their nets
//! (candidate L2): "実測 4.98V", coloured by how it agrees with the DC operating point's prediction when there is one (the same
//! judgement as the bench tab's table: within the allowance, out by up to three times, further out).

use super::bench_data::{judge, Agree};
use super::*;

impl SchEditor {
    /// Where each reading goes: the net's label on this sheet, else the first pin of a part on the net.
    pub(super) fn reading_spots(&self) -> Vec<(Pt, String, f64, f64)> {
        let readings = &self.bench.dc;
        if readings.is_empty() {
            return vec![];
        }
        let nets = sch_core::extract(&self.sch);
        let mut out = vec![];
        for r in readings {
            let at = self.sch.labels.iter().find(|l| l.text == r.net).map(|l| l.pos).or_else(|| {
                let net = nets.iter().find(|n| n.name == r.net)?;
                let p = net.pins.first()?;
                let c = self.sch.components.iter().find(|c| c.refdes == p.refdes)?;
                c.pins().into_iter().find(|(pin, _)| *pin == p.pin).map(|x| x.1)
            });
            if let Some(at) = at {
                out.push((at, r.net.clone(), r.volts, r.uncert));
            }
        }
        out
    }

    /// The readings on the sheet (with the voltages shown).
    pub(super) fn draw_readings(&self, painter: &egui::Painter, rect: Rect) {
        if !self.show_v {
            return;
        }
        let spots = self.reading_spots();
        if spots.is_empty() {
            return;
        }
        let res = self.dc.as_ref().and_then(|d| d.outcome.as_ref().ok());
        let f = self.main_font(painter);
        for (pt, net, volts, uncert) in spots {
            let colour = match res.and_then(|r| r.voltage(&net)).map(|p| judge(p, None, volts, uncert).1) {
                Some(Agree::Good) => crate::theme::OK,
                Some(Agree::Off) => crate::theme::WARN,
                Some(Agree::Far) => crate::theme::ERROR,
                None => crate::canvas_theme::col(crate::canvas_theme::Role::Value),
            };
            let text = i18n::trf!("実測 {}", eng(volts, "V")).to_string();
            let galley = painter.layout_no_wrap(text, f.clone(), colour);
            // below the point, clear of the predicted voltage drawn above it
            let top_left = self.to_screen(rect, (pt.0 as f32, pt.1 as f32)) + Vec2::new(5.0, 4.0);
            painter.rect_filled(Rect::from_min_size(top_left, galley.size()).expand(1.0), 2.0, crate::canvas_theme::col(crate::canvas_theme::Role::TextChip));
            painter.rect_stroke(Rect::from_min_size(top_left, galley.size()).expand(1.0), 2.0, Stroke::new(1.0, colour), egui::StrokeKind::Outside);
            painter.galley(top_left, galley, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reading goes to its net's label, or to a pin on the net when the net has no label on the sheet.
    #[test]
    fn a_reading_sits_at_its_nets_label_or_a_pin() {
        let mut e = SchEditor::default();
        e.sch = serde_json::from_str(r#"{"components":[{"refdes":"R1","kind":"Resistor","value":"1k","pos":[10,10],"rot":0},{"refdes":"R2","kind":"Resistor","value":"1k","pos":[10,20],"rot":0}],"wires":[],"labels":[]}"#).unwrap();
        let (p1, p2) = (e.sch.components[0].pins(), e.sch.components[1].pins());
        e.sch.labels.push(sch_core::Label { text: "OUT".into(), pos: p1[1].1 });
        e.set_dc_reading("OUT", Some((2.5, 0.01)));
        let spots = e.reading_spots();
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].0, p1[1].1, "at the label");
        // a net with no label: at a pin on it
        let unlabelled = sch_core::extract(&e.sch).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == "R2")).unwrap().name;
        e.set_dc_reading(&unlabelled, Some((1.0, 0.0)));
        assert!(e.reading_spots().iter().any(|s| s.1 == unlabelled && p2.iter().any(|p| p.1 == s.0)));
        e.set_dc_reading("NOPE", Some((1.0, 0.0)));
        assert_eq!(e.reading_spots().len(), 2, "a net not on the sheet is not drawn");
    }
}
