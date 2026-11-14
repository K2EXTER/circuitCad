//! Filling the fields of the form from numbers clicked in the data sheet window.

use super::datasheet_view::{Picked, Target};
use super::*;

/// Where a value came from: the field, the page (from 0) and the text that was clicked.
pub type Source = (Target, usize, String);

impl PartForm {
    /// Takes what was clicked in the data sheet window, if anything, and puts it in its field.
    pub(super) fn take_pdf_pick(&mut self) {
        if let Some(p) = self.sheet.take_picked() {
            self.apply_pick(p);
        }
    }

    /// Takes the pins the person accepted in the pin table window, if any, and puts them in the pin box.
    pub(super) fn take_pdf_pins(&mut self) {
        use super::pin_import::PinMode;
        let Some(imp) = self.sheet.take_pins() else { return };
        // the table stays the one the pins are compared with from now on
        self.pin_ref = Some(super::pdf_check::PinRef { page: imp.page, area: imp.area, num_col: imp.num_col, name_col: imp.name_col });
        self.pin_check = None;
        let read = imp.lines.lines().count();
        self.pick_msg = Some(match imp.mode {
            PinMode::CheckOnly => (true, format!("ピン表は変えずに、p.{} の表 ({read} 本) と比べます。結果は「③ ピン」に出ます", imp.page + 1)),
            mode => {
                self.pins_text = super::pin_import::merged(&self.pins_text, &imp.lines, mode == PinMode::Append);
                let n = self.pins_text.lines().filter(|l| !l.trim().is_empty()).count();
                let how = if mode == PinMode::Append { "あとに足しました" } else { "入れました" };
                (true, format!("ピン表に p.{} の {read} 本のピンを{how} (合計 {n} 本)。向き (left/right) は「③ ピン」で書き足せます", imp.page + 1))
            }
        });
    }

    /// Takes the points read off a graph that the person sent to the form, and puts them in their field.
    pub(super) fn take_pdf_graph(&mut self) {
        use super::graph_reader::{Dest, short};
        let Some(out) = self.sheet.take_graph() else { return };
        match out.dest {
            Dest::DiodeForward => {
                self.dio_points = out.points.iter().map(|(i, v)| format!("{} {}", short(*i), short(*v))).collect::<Vec<_>>().join("\n");
                self.dio_message = Some(self.dio_fit().map(|m| format!("p.{} のグラフの {} 点を入れました。{m}", out.page + 1, out.points.len())));
            }
        }
    }

    /// Puts a clicked word in the field it was asked for. A number goes through the unit conversion; what was
    /// assumed or converted is said in `pick_msg`, and a number that cannot be read leaves the field alone.
    pub(super) fn apply_pick(&mut self, p: Picked) {
        let Some(q) = p.target.quantity() else {
            self.name = p.text.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
            self.pick_msg = None;
            return;
        };
        let Some(mut v) = datasheet::parse_value(&p.text) else {
            self.pick_msg = Some((false, format!("「{}」は数字として読めませんでした。数字の部分をクリックしてください", p.text)));
            return;
        };
        if v.unit.is_none() {
            v.unit = p.unit_after;
        }
        let c = match datasheet::convert(&v, q) {
            Ok(c) => c,
            Err(e) => {
                self.pick_msg = Some((false, format!("「{}」: {e}", p.text)));
                return;
            }
        };
        let x = c.value;
        match p.target {
            Target::Pitch => self.pitch = x,
            Target::Row => self.row = x,
            Target::PadLen => self.len = x,
            Target::PadWidth => self.width = x,
            Target::Span => self.span = x,
            Target::PadDia => self.pad_d = x,
            Target::Drill => self.drill = x,
            Target::BodyW => self.body_w = x,
            Target::BodyH => self.body_h = x,
            Target::Height => self.height = x,
            Target::MaxV => self.rt_v = number_text(x),
            Target::MaxI => self.rt_i = number_text(x),
            Target::MaxP => self.rt_p = number_text(x),
            Target::ThermalW => self.thermal_w = x,
            Target::ThermalH => self.thermal_h = x,
            Target::Name => {}
        }
        self.sources.retain(|s| s.0 != p.target);
        self.sources.push((p.target, p.page, p.text.clone()));
        let unit = match q {
            datasheet::Quantity::Length => "mm",
            datasheet::Quantity::Voltage => "V",
            datasheet::Quantity::Current => "A",
            datasheet::Quantity::Power => "W",
        };
        let note = c.note.map(|n| format!(" ({n})")).unwrap_or_default();
        self.pick_msg = Some((true, format!("{} に {} {unit} を入れました (p.{} の「{}」){note}", p.target.label(), number_text(x), p.page + 1, p.text)));
    }

    /// A small button that asks for a number to be clicked in the PDF for `target`. It shows the page when the field
    /// was filled that way. Nothing is drawn while no PDF is open.
    pub(super) fn pick_button(&mut self, ui: &mut egui::Ui, target: Target, text: &str) {
        if !self.sheet.has_doc() {
            return;
        }
        let from = self.sources.iter().find(|s| s.0 == target).map(|s| (s.1, s.2.clone()));
        let label = match &from {
            Some((page, _)) => format!("{text} ✓p.{}", page + 1),
            None => text.to_string(),
        };
        let tip = match &from {
            Some((page, word)) => format!("p.{} の「{word}」から入れました。押すと、選び直せます", page + 1),
            None => format!("データシートの窓で、{}の数字をクリックして入れます。インチ・mil は mm に換算します (単位は数字の右にあるものを使います)", target.label()),
        };
        if ui.small_button(i18n::tr_dyn(&label)).on_hover_text(i18n::tr_dyn(&tip)).clicked() {
            self.pick_msg = None;
            self.sheet.start_pick(target);
        }
    }

    /// A row of buttons, one per field, labelled with the field names. Nothing while no PDF is open (the PDF is
    /// opened from the button on the tab row).
    pub(super) fn pick_row(&mut self, ui: &mut egui::Ui, targets: &[Target]) {
        if !self.sheet.has_doc() || targets.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.small(i18n::tr("PDFから入れる"));
            for t in targets {
                self.pick_button(ui, *t, t.label());
            }
        });
    }

    /// What the last click in the PDF did, or why it could not.
    pub(super) fn pick_message(&self, ui: &mut egui::Ui) {
        if let Some((ok, m)) = &self.pick_msg {
            let color = if *ok { Color32::from_rgb(90, 190, 120) } else { Color32::from_rgb(230, 80, 70) };
            ui.colored_label(color, m);
        }
    }
}

/// 1.27 -> "1.27", 0.2 -> "0.2", 30.0 -> "30": the number as a person writes it in a box.
pub(super) fn number_text(x: f64) -> String {
    let s = format!("{x:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(form: &mut PartForm, target: Target, text: &str, unit_after: Option<&str>) {
        form.apply_pick(Picked { target, text: text.into(), unit_after: unit_after.map(String::from), page: 4 });
    }

    #[test]
    fn a_clicked_number_goes_to_its_field_in_millimetres() {
        let mut f = PartForm::default();
        pick(&mut f, Target::Pitch, "0.65", Some("mm"));
        assert_eq!(f.pitch, 0.65);
        assert_eq!(f.sources, [(Target::Pitch, 4, "0.65".to_string())]);
        assert!(f.pick_msg.as_ref().is_some_and(|(ok, m)| *ok && m.contains("p.5")));
        // inches written after the number
        pick(&mut f, Target::Row, ".300", Some("in"));
        assert_eq!(f.row, 7.62);
        assert!(f.pick_msg.as_ref().unwrap().1.contains("換算"));
        // no unit anywhere: taken as mm and said so
        pick(&mut f, Target::PadLen, "1.5", None);
        assert_eq!(f.len, 1.5);
        assert!(f.pick_msg.as_ref().unwrap().1.contains("単位が書かれていない"));
        // the same field again replaces its source
        pick(&mut f, Target::Pitch, "1.27", None);
        assert_eq!(f.sources.iter().filter(|s| s.0 == Target::Pitch).count(), 1);
        assert_eq!(f.pitch, 1.27);
    }

    #[test]
    fn a_wrong_click_leaves_the_field_alone_and_says_why() {
        let mut f = PartForm::default();
        let before = f.pitch;
        pick(&mut f, Target::Pitch, "SOIC", None);
        assert_eq!(f.pitch, before);
        assert!(f.pick_msg.as_ref().is_some_and(|(ok, m)| !*ok && m.contains("読めません")));
        pick(&mut f, Target::Pitch, "30V", None);
        assert_eq!(f.pitch, before);
        assert!(f.pick_msg.as_ref().unwrap().1.contains("長さ"));
        assert!(f.sources.is_empty());
    }

    #[test]
    fn ratings_take_plain_numbers() {
        let mut f = PartForm::default();
        pick(&mut f, Target::MaxV, "30V", None);
        pick(&mut f, Target::MaxI, "200mA", None);
        pick(&mut f, Target::MaxP, "625", Some("mW"));
        assert_eq!((f.rt_v.as_str(), f.rt_i.as_str(), f.rt_p.as_str()), ("30", "0.2", "0.625"));
        pick(&mut f, Target::MaxV, "5.7A", None);
        assert_eq!(f.rt_v, "30");
    }

    #[test]
    fn a_name_is_trimmed_of_marks() {
        let mut f = PartForm::default();
        pick(&mut f, Target::Name, "LM393B,", None);
        assert_eq!(f.name, "LM393B");
    }

    #[test]
    fn numbers_are_written_as_a_person_would() {
        assert_eq!((number_text(1.27), number_text(30.0), number_text(0.2), number_text(0.625)), ("1.27".into(), "30".into(), "0.2".into(), "0.625".into()));
    }
}
