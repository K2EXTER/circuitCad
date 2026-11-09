//! Settings that belong to a sheet and are saved with it: the part tolerances of "温度とばらつき" (they decide the variation and
//! worst-case runs and the tolerance range of the spec table) and the analysis settings of the transient and AC runs (so a sample
//! opens ready to run: a motor that needs 100 ms, a filter that needs its frequency range).

use super::*;

/// The boxes as typed.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetSettings {
    /// Tolerances in percent: resistors, capacitors, coils, DC supplies (the worst case only).
    pub r: String,
    pub c: String,
    pub l: String,
    pub v: String,
    /// The transient run: end time, largest step (empty = automatic), start from zero.
    pub tran_stop: String,
    pub tran_step: String,
    pub tran_uic: bool,
    /// The AC run: start and stop frequency, points per decade.
    pub ac_start: String,
    pub ac_stop: String,
    pub ac_ppd: String,
}

impl Default for SheetSettings {
    fn default() -> Self {
        Self {
            r: "5".into(),
            c: "10".into(),
            l: "10".into(),
            v: "0".into(),
            tran_stop: "10m".into(),
            tran_step: String::new(),
            tran_uic: false,
            ac_start: "10".into(),
            ac_stop: "1Meg".into(),
            ac_ppd: "20".into(),
        }
    }
}

impl SheetSettings {
    fn tolerances_default(&self) -> bool {
        let d = Self::default();
        (&self.r, &self.c, &self.l, &self.v) == (&d.r, &d.c, &d.l, &d.v)
    }

    fn analysis_default(&self) -> bool {
        let d = Self::default();
        (&self.tran_stop, &self.tran_step, self.tran_uic, &self.ac_start, &self.ac_stop, &self.ac_ppd) == (&d.tran_stop, &d.tran_step, d.tran_uic, &d.ac_start, &d.ac_stop, &d.ac_ppd)
    }
}

/// Puts the settings into a sheet's JSON: the tolerances under "tolerances", the analysis settings under "analysis". A part that is
/// at its defaults is not written (older files stay as they were).
pub fn with_settings(sheet: &mut serde_json::Value, t: &SheetSettings) {
    let Some(obj) = sheet.as_object_mut() else { return };
    if !t.tolerances_default() {
        obj.insert("tolerances".into(), serde_json::json!({"r": t.r, "c": t.c, "l": t.l, "v": t.v}));
    }
    if !t.analysis_default() {
        obj.insert(
            "analysis".into(),
            serde_json::json!({"tran_stop": t.tran_stop, "tran_step": t.tran_step, "tran_uic": t.tran_uic, "ac_start": t.ac_start, "ac_stop": t.ac_stop, "ac_ppd": t.ac_ppd}),
        );
    }
}

/// Takes the settings out of a sheet's JSON (the defaults for what a file does not have; a box that is missing keeps its default).
pub fn take_settings(sheet: &mut serde_json::Value) -> SheetSettings {
    let mut t = SheetSettings::default();
    let Some(obj) = sheet.as_object_mut() else { return t };
    if let Some(v) = obj.remove("tolerances") {
        for (key, slot) in [("r", &mut t.r), ("c", &mut t.c), ("l", &mut t.l), ("v", &mut t.v)] {
            if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                *slot = s.to_string();
            }
        }
    }
    if let Some(v) = obj.remove("analysis") {
        for (key, slot) in [("tran_stop", &mut t.tran_stop), ("tran_step", &mut t.tran_step), ("ac_start", &mut t.ac_start), ("ac_stop", &mut t.ac_stop), ("ac_ppd", &mut t.ac_ppd)] {
            if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                *slot = s.to_string();
            }
        }
        if let Some(b) = v.get("tran_uic").and_then(|x| x.as_bool()) {
            t.tran_uic = b;
        }
    }
    t
}

impl SchEditor {
    pub fn sheet_settings(&self) -> SheetSettings {
        SheetSettings {
            r: self.mc_tol_r.clone(),
            c: self.mc_tol_c.clone(),
            l: self.mc_tol_l.clone(),
            v: self.mc_tol_v.clone(),
            tran_stop: self.tran_tstop.clone(),
            tran_step: self.tran_tstep.clone(),
            tran_uic: self.tran_uic,
            ac_start: self.ac_fstart.clone(),
            ac_stop: self.ac_fstop.clone(),
            ac_ppd: self.ac_ppd.clone(),
        }
    }

    /// Sets the boxes from a file: they count as saved.
    pub fn set_sheet_settings(&mut self, t: SheetSettings) {
        self.mc_tol_r = t.r.clone();
        self.mc_tol_c = t.c.clone();
        self.mc_tol_l = t.l.clone();
        self.mc_tol_v = t.v.clone();
        self.tran_tstop = t.tran_stop.clone();
        self.tran_tstep = t.tran_step.clone();
        self.tran_uic = t.tran_uic;
        self.ac_fstart = t.ac_start.clone();
        self.ac_fstop = t.ac_stop.clone();
        self.ac_ppd = t.ac_ppd.clone();
        self.saved_tol = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_defaults_are_not_written() {
        let mut v = serde_json::json!({});
        with_settings(&mut v, &SheetSettings::default());
        assert!(v.get("tolerances").is_none() && v.get("analysis").is_none(), "{v}");
        let t = SheetSettings { r: "1".into(), ..Default::default() };
        with_settings(&mut v, &t);
        assert!(v.get("analysis").is_none(), "only the part that differs is written: {v}");
        assert_eq!(take_settings(&mut v), t);
        assert!(v.get("tolerances").is_none(), "taken out of the sheet");
        let a = SheetSettings { tran_stop: "100m".into(), tran_uic: true, ac_stop: "10k".into(), ..Default::default() };
        let mut w = serde_json::json!({});
        with_settings(&mut w, &a);
        assert!(w.get("tolerances").is_none() && w.get("analysis").is_some());
        assert_eq!(take_settings(&mut w), a);
        // a file without them, or with only some boxes
        assert_eq!(take_settings(&mut serde_json::json!({})), SheetSettings::default());
        let mut part = serde_json::json!({"tolerances": {"c": "20"}, "analysis": {"tran_stop": "5m"}});
        assert_eq!(take_settings(&mut part), SheetSettings { c: "20".into(), tran_stop: "5m".into(), ..Default::default() });
    }

    #[test]
    fn a_changed_setting_is_an_unsaved_change_and_is_written_to_the_file() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/divider_tolerance.schjson");
        let mut e = SchEditor::default();
        e.load_path(path);
        assert!(!e.dirty());
        e.mc_tol_r = "1".into();
        e.tran_tstop = "50m".into();
        assert!(e.dirty(), "new settings must be saved");
        let text = e.sheet_json().unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let back = take_settings(&mut v);
        assert_eq!((back.r.as_str(), back.tran_stop.as_str()), ("1", "50m"));
        e.mark_saved();
        assert!(!e.dirty());
        // opening another file brings its own settings (here the defaults)
        e.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/led.schjson"));
        assert_eq!(e.sheet_settings(), SheetSettings::default());
        assert!(!e.dirty());
    }

    #[test]
    fn the_samples_open_with_the_analysis_they_need() {
        let open = |f: &str| {
            let mut e = SchEditor::default();
            e.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(f));
            e
        };
        let m = open("motor_driver.schjson");
        assert_eq!((m.tran_tstop.as_str(), m.tran_tstep.as_str()), ("100m", "50u"));
        assert!(!m.dirty());
        assert_eq!(open("load_step.schjson").tran_tstop, "3m");
        assert_eq!(open("psrr_filter.schjson").ac_fstart, "1");
        // the sample list opens them the same way
        let s = crate::samples::SAMPLES.iter().find(|s| s.json.contains("DC_MOTOR")).unwrap();
        let mut e = SchEditor::default();
        assert!(e.open_sample(s.title, s.json));
        assert_eq!(e.tran_tstop, "100m");
    }
}
