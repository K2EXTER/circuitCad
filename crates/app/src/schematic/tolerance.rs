//! The part tolerances of "温度とばらつき" (resistors, capacitors, coils, DC supplies), saved with the sheet. They decide the
//! variation and worst-case runs and the tolerance range in the spec table, so a circuit built from 1 % resistors keeps its 1 %.

use super::*;

/// The tolerance boxes, in percent as typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Tolerances {
    pub r: String,
    pub c: String,
    pub l: String,
    /// DC supplies (the worst case only).
    pub v: String,
}

impl Default for Tolerances {
    fn default() -> Self {
        Self { r: "5".into(), c: "10".into(), l: "10".into(), v: "0".into() }
    }
}

/// Puts the tolerances into a sheet's JSON, under "tolerances" (nothing when they are the defaults, so older files stay as they were).
pub fn with_tolerances(sheet: &mut serde_json::Value, t: &Tolerances) {
    if let (Some(obj), false) = (sheet.as_object_mut(), *t == Tolerances::default()) {
        obj.insert("tolerances".into(), serde_json::json!({"r": t.r, "c": t.c, "l": t.l, "v": t.v}));
    }
}

/// Takes the tolerances out of a sheet's JSON (the defaults for files without them; a box that is missing keeps its default).
pub fn take_tolerances(sheet: &mut serde_json::Value) -> Tolerances {
    let mut t = Tolerances::default();
    if let Some(v) = sheet.as_object_mut().and_then(|o| o.remove("tolerances")) {
        for (key, slot) in [("r", &mut t.r), ("c", &mut t.c), ("l", &mut t.l), ("v", &mut t.v)] {
            if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                *slot = s.to_string();
            }
        }
    }
    t
}

impl SchEditor {
    pub fn tolerances(&self) -> Tolerances {
        Tolerances { r: self.mc_tol_r.clone(), c: self.mc_tol_c.clone(), l: self.mc_tol_l.clone(), v: self.mc_tol_v.clone() }
    }

    /// Sets the boxes from a file: they count as saved.
    pub fn set_tolerances(&mut self, t: Tolerances) {
        self.mc_tol_r = t.r.clone();
        self.mc_tol_c = t.c.clone();
        self.mc_tol_l = t.l.clone();
        self.mc_tol_v = t.v.clone();
        self.saved_tol = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerances_round_trip_and_defaults_are_not_written() {
        let mut v = serde_json::json!({});
        with_tolerances(&mut v, &Tolerances::default());
        assert!(v.get("tolerances").is_none(), "{v}");
        let t = Tolerances { r: "1".into(), ..Default::default() };
        with_tolerances(&mut v, &t);
        assert_eq!(take_tolerances(&mut v), t);
        assert!(v.get("tolerances").is_none(), "taken out of the sheet");
        // a file without them, or with only some boxes
        assert_eq!(take_tolerances(&mut serde_json::json!({})), Tolerances::default());
        let mut part = serde_json::json!({"tolerances": {"c": "20"}});
        assert_eq!(take_tolerances(&mut part), Tolerances { c: "20".into(), ..Default::default() });
    }

    #[test]
    fn a_changed_tolerance_is_an_unsaved_change_and_is_written_to_the_file() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/divider_tolerance.schjson");
        let mut e = SchEditor::default();
        e.load_path(path);
        assert!(!e.dirty());
        e.mc_tol_r = "1".into();
        assert!(e.dirty(), "a new tolerance must be saved");
        let text = e.sheet_json().unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(take_tolerances(&mut v).r, "1");
        e.mark_saved();
        assert!(!e.dirty());
        // opening another file brings its own tolerances (here the defaults)
        e.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/led.schjson"));
        assert_eq!(e.tolerances(), Tolerances::default());
        assert!(!e.dirty());
    }
}
