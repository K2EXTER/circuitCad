//! "かんたん作成" for the light parts (Q1 ①): an optocoupler (the LED's forward points, the output curves' flat part,
//! VCE(sat) and the cut-off) and a photodiode (the light current, the open-circuit voltage, the dark current, the
//! capacitance), made the same way as the bundled PC817 and BPW34 (`part_lib::fit::{OptocouplerSpec, PhotodiodeSpec}`).

use super::bjt::{lines, num, pairs, pairs_from_zero};
use super::darlington::curve_row;
use super::graph_reader::short;
use super::*;
use part_lib::fit::{self, Fitted};

/// What the form holds for the optocoupler box, as typed.
pub struct OptoInput {
    /// (IF, VF) lines of the LED at 25 °C.
    pub led: String,
    /// The table's VF typ: "IF VF" (compared only); may be empty.
    pub led_typ: String,
    /// The LED's capacitance (F) and reverse voltage rating (V).
    pub led_cap: String,
    pub led_vr: String,
    /// (IF, VCE, IC) lines on the flat part of the output curves.
    pub output: String,
    /// (IF, CTR %) lines of the CTR curve (compared only); may be empty.
    pub ctr_curve: String,
    /// The table's CTR min and max (%).
    pub ctr_min: String,
    pub ctr_max: String,
    /// VCE(sat) typ and its condition IF, IC.
    pub vce_sat: String,
    pub sat_if: String,
    pub sat_ic: String,
    /// The cut-off frequency (Hz).
    pub fc: String,
    /// What was taken from the PDF's table (one line each), or what was not found.
    pub found: Vec<(bool, String)>,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl Default for OptoInput {
    fn default() -> Self {
        OptoInput {
            led: String::new(),
            led_typ: String::new(),
            led_cap: "30p".into(),
            led_vr: "6".into(),
            output: String::new(),
            ctr_curve: String::new(),
            ctr_min: String::new(),
            ctr_max: String::new(),
            vce_sat: String::new(),
            sat_if: "20m".into(),
            sat_ic: "1m".into(),
            fc: String::new(),
            found: vec![],
            result: None,
            message: None,
        }
    }
}

/// What the form holds for the photodiode box, as typed.
pub struct PhotodiodeInput {
    /// The reverse light current per mW/cm² (A).
    pub sensitivity: String,
    /// (irradiance mW/cm², current A) lines read off the figure (compared); may be empty.
    pub light: String,
    /// The short-circuit current at 1 mW/cm² (A, compared); may be empty.
    pub isc: String,
    /// The open-circuit voltage (V) at 1 mW/cm² and its temperature coefficient (mV/K, compared; may be empty).
    pub voc: String,
    pub voc_tc: String,
    /// The emission coefficient (empty: 1) and the series resistance (empty: 10 Ω).
    pub n: String,
    pub rs: String,
    /// The dark current and its reverse voltage.
    pub dark_v: String,
    pub dark_i: String,
    /// The breakdown voltage (V) and its current (A).
    pub bv: String,
    pub ibv: String,
    /// (reverse voltage, capacitance) lines.
    pub cap: String,
    /// What was taken from the PDF's table (one line each), or what was not found.
    pub found: Vec<(bool, String)>,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl Default for PhotodiodeInput {
    fn default() -> Self {
        PhotodiodeInput {
            sensitivity: String::new(),
            light: String::new(),
            isc: String::new(),
            voc: String::new(),
            voc_tc: String::new(),
            n: String::new(),
            rs: String::new(),
            dark_v: "10".into(),
            dark_i: String::new(),
            bv: String::new(),
            ibv: "100u".into(),
            cap: String::new(),
            found: vec![],
            result: None,
            message: None,
        }
    }
}

const PC817: &str = include_str!("../../../part-lib/fits/PC817_DIP-4.fit.json");
const BPW34: &str = include_str!("../../../part-lib/fits/BPW34.fit.json");

/// A number that may be left empty.
fn opt(label: &str, t: &str) -> Result<Option<f64>, String> {
    if t.trim().is_empty() { Ok(None) } else { num(label, t).map(Some) }
}

/// A number that must be there.
fn need(label: &str, t: &str, how: &str) -> Result<f64, String> {
    if t.trim().is_empty() {
        return Err(format!("{label}を入れてください。{how}"));
    }
    num(label, t)
}

/// "IF VCE IC" lines.
fn triples(label: &str, text: &str) -> Result<Vec<[f64; 3]>, String> {
    let mut out = vec![];
    for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
        let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
        let [a, b, c] = t.as_slice() else {
            return Err(format!("{label}の {}行目: 3 つの数 (例: 10m 5 12m) で書いてください", n + 1));
        };
        out.push([num(label, a)?, num(label, b)?, num(label, c)?]);
    }
    Ok(out)
}

/// "value value" lines with every digit (the examples: `lines` rounds to 4 digits, and the LED's IS moves with the
/// fourth digit of VF).
pub(super) fn exact(points: &[[f64; 2]]) -> String {
    points.iter().map(|[a, b]| format!("{} {}", digits(*a), digits(*b))).collect::<Vec<_>>().join("
")
}

/// A number with 6 significant digits, small ones with an SI prefix (`61.92p`).
pub(super) fn digits(v: f64) -> String {
    let g = |x: f64| part_lib::fit::num::g(x, 6);
    match v.abs() {
        0.0 => "0".into(),
        a if a < 1e-9 => format!("{}p", g(v * 1e12)),
        a if a < 1e-6 => format!("{}n", g(v * 1e9)),
        a if a < 1e-3 => format!("{}u", g(v * 1e6)),
        _ => g(v),
    }
}

impl OptoInput {
    /// The readings as the fit takes them.
    pub fn spec(&self, name: &str, source: &str) -> Result<fit::Spec, String> {
        let led = pairs("LED の順方向", &self.led)?;
        if led.len() < 3 {
            return Err("LED の順方向の点を 3 つ以上入れてください (順方向電流と電圧の図の 25 °C の線から)".into());
        }
        let output = triples("出力の点", &self.output)?;
        if output.len() < 2 {
            return Err("出力の点を 2 つ以上入れてください (コレクタ電流と VCE の図の、平らな所の「IF VCE IC」)".into());
        }
        let led_typ = pairs("表の VF typ", &self.led_typ)?.first().copied();
        let (ctr_min, ctr_max) = (need("CTR の min", &self.ctr_min, "表の電流伝達率 (CTR) の範囲 [%] です")?, need("CTR の max", &self.ctr_max, "表の電流伝達率 (CTR) の範囲 [%] です")?);
        let vce_sat = need("VCE(sat)", &self.vce_sat, "表の「コレクタ・エミッタ間の飽和電圧」の typ です")?;
        let fc = need("カットオフ周波数", &self.fc, "表の「カットオフ周波数 (fc)」です。応答時間しか無ければ 0.35 ÷ 立ち上がり時間")?;
        let json = serde_json::json!({
            "kind": "optocoupler",
            "part": name,
            "name": part_lib::diode::model_name(name),
            "source": source,
            "led": led,
            "led_at": "順方向電流と電圧の図の 25 °C",
            "led_typ": led_typ,
            "led_cap": num("LED の容量", &self.led_cap)?,
            "led_vr": num("LED の逆電圧", &self.led_vr)?,
            "output": output,
            "output_at": "コレクタ電流と VCE の図の平らな所",
            "ctr_curve": pairs("CTR の曲線", &self.ctr_curve)?,
            "ctr_curve_at": "CTR と IF の図",
            "ctr_range": [ctr_min, ctr_max],
            "ctr_note": format!("表の CTR は {}〜{}%", short(ctr_min), short(ctr_max)),
            "bf": [200.0, "仮の値。外から見えるのは CTR = K × BF だけ"],
            "vce_sat": [vce_sat, num("VCE(sat) の IF", &self.sat_if)?, num("VCE(sat) の IC", &self.sat_ic)?],
            "fc": [fc, "入力したカットオフ周波数"],
            "leaves_out": "暗電流、温度による CTR の変化、CTR の IF による変化 (CTR の図の山の形)、入力と出力の間の容量、絶縁耐圧",
        });
        serde_json::from_value(json).map_err(|e| e.to_string())
    }

    /// Fills the boxes with the bundled PC817's readings.
    pub fn example(&mut self) {
        let Ok(fit::Spec::Optocoupler(s)) = serde_json::from_str::<fit::Spec>(PC817) else { return };
        *self = OptoInput {
            led: exact(&s.led),
            led_typ: s.led_typ.map(|p| lines(&[p])).unwrap_or_default(),
            led_cap: short(s.led_cap),
            led_vr: short(s.led_vr),
            output: s.output.iter().map(|[a, b, c]| format!("{} {} {}", digits(*a), digits(*b), digits(*c))).collect::<Vec<_>>().join("\n"),
            ctr_curve: lines(&s.ctr_curve),
            ctr_min: short(s.ctr_range[0]),
            ctr_max: short(s.ctr_range[1]),
            vce_sat: short(s.vce_sat[0]),
            sat_if: short(s.vce_sat[1]),
            sat_ic: short(s.vce_sat[2]),
            fc: format!("{}", s.fc.0),
            found: vec![],
            result: None,
            message: None,
        };
    }
}

impl PhotodiodeInput {
    /// The readings as the fit takes them.
    pub fn spec(&self, name: &str, source: &str) -> Result<fit::Spec, String> {
        let k = need("感度", &self.sensitivity, "表の「逆方向の光電流 (Ira)」の typ を、1 mW/cm² あたりにした値です (例: 50u)")?;
        let voc = need("開放電圧", &self.voc, "表の「開放電圧 (Vo・VOC)」です (例: 0.35)")?;
        let cap = pairs_from_zero("容量", &self.cap)?;
        if cap.is_empty() {
            return Err("容量の点を 1 つ以上入れてください (表の CD と、あれば容量と逆電圧の図から「逆電圧 容量」)".into());
        }
        let (n, n_why) = match opt("N", &self.n)? {
            Some(n) => (n, "入力した値"),
            None => (1.0, "データシートに開放電圧が 1 つしか無いので N = 1 と置きました"),
        };
        let (rs, rs_why) = match opt("RS", &self.rs)? {
            Some(r) => (r, "入力した値"),
            None => (10.0, "データシートに順方向の特性が無いので 10 Ω と置きました (光電流の範囲では効きません)"),
        };
        let bv = need("降伏電圧", &self.bv, "表の「逆方向の降伏電圧 V(BR)」の min です")?;
        let json = serde_json::json!({
            "kind": "photodiode",
            "part": name,
            "name": part_lib::diode::model_name(name),
            "source": source,
            "sensitivity": [k, "入力した光電流 (1 mW/cm² あたり)"],
            "light_points": pairs("光電流の点", &self.light)?,
            "isc": opt("短絡電流", &self.isc)?,
            "voc": voc,
            "voc_tc": opt("開放電圧の温度係数", &self.voc_tc)?,
            "n": [n, n_why],
            "rs": [rs, rs_why],
            "dark": [num("暗電流の逆電圧", &self.dark_v)?, need("暗電流", &self.dark_i, "表の「暗電流 (Iro・ID)」の typ です (例: 2n)")?],
            "breakdown": {"v": bv, "i": num("降伏の電流", &self.ibv)?, "note": "入力した降伏電圧"},
            "capacitance": cap,
            "cap_at": "入力した容量の点",
            "response": "応答の速さ (光で生まれた電荷が動く時間。このモデルは接合容量だけで遅れます)、波長による感度",
        });
        serde_json::from_value(json).map_err(|e| e.to_string())
    }

    /// Fills the boxes with the bundled BPW34's readings.
    pub fn example(&mut self) {
        let Ok(fit::Spec::Photodiode(s)) = serde_json::from_str::<fit::Spec>(BPW34) else { return };
        *self = PhotodiodeInput {
            sensitivity: short(s.sensitivity.0),
            light: exact(&s.light_points),
            isc: s.isc.map(short).unwrap_or_default(),
            voc: short(s.voc),
            voc_tc: s.voc_tc.map(short).unwrap_or_default(),
            n: short(s.n.0),
            rs: short(s.rs.0),
            dark_v: short(s.dark[0]),
            dark_i: short(s.dark[1]),
            bv: short(s.breakdown.v),
            ibv: short(s.breakdown.i),
            cap: exact(&s.capacitance),
            found: vec![],
            result: None,
            message: None,
        };
    }
}

impl PartForm {
    pub(super) fn opto_make(&mut self) {
        let (name, source) = (self.linear_name("OPTO"), self.linear_source());
        self.opto.result = Some(self.opto.spec(&name, &source).and_then(|s| fit::fit(&s)));
        self.opto.message = None;
    }

    pub(super) fn pd_make(&mut self) {
        let (name, source) = (self.linear_name("PD"), self.linear_source());
        let mut spec = self.pd.spec(&name, &source);
        // the temperature coefficient of the open-circuit voltage is negative (num reads magnitudes)
        if let Ok(fit::Spec::Photodiode(s)) = &mut spec
            && let Some(tc) = s.voc_tc.as_mut()
            && !self.pd.voc_tc.trim().starts_with('+')
        {
            *tc = -tc.abs();
        }
        self.pd.result = Some(spec.and_then(|s| fit::fit(&s)));
        self.pd.message = None;
    }

    /// The PC817 example made (`--form-opto-example`, for screenshots).
    pub fn opto_example_and_make(&mut self) {
        self.opto.example();
        self.opto_make();
    }

    /// The BPW34 example made (`--form-photodiode-example`, for screenshots).
    pub fn photodiode_example_and_make(&mut self) {
        self.pd.example();
        self.pd_make();
    }

    /// Takes points read off a graph for one of the light parts' fields.
    pub(super) fn opto_take_graph(&mut self, dest: super::graph_reader::Dest, points: &[(f64, f64)], page: usize) {
        use super::graph_reader::Dest;
        let text = lines(&points.iter().map(|(a, b)| [a.abs(), b.abs()]).collect::<Vec<_>>());
        let msg = Some((true, format!("p.{} のグラフの {} 点を入れました", page + 1, points.len())));
        match dest {
            Dest::OptoLed => (self.opto.led, self.opto.result, self.opto.message) = (text, None, msg),
            Dest::OptoCtr => (self.opto.ctr_curve, self.opto.result, self.opto.message) = (text, None, msg),
            Dest::PdLight => (self.pd.light, self.pd.result, self.pd.message) = (text, None, msg),
            Dest::PdCap => (self.pd.cap, self.pd.result, self.pd.message) = (text, None, msg),
            _ => {}
        }
    }

    pub(super) fn opto_ui(&mut self, ui: &mut egui::Ui) {
        use super::graph_reader::Dest;
        crate::widgets::section_heading(ui, "かんたん作成: フォトカプラをデータシートのグラフからつくる");
        crate::icons::help_small(ui, "LED と出力のトランジスタの点からモデルを作ります", "PC817 のような、LED とフォトトランジスタが 1 つに入った部品です。LED の順方向の図・出力の図の点と、表の CTR・飽和電圧・カットオフ周波数から、同梱の PC817 と同じ作り方でモデルを作り、読んだ点とのずれを並べます。ピンは A・K (LED) と C・E (出力) の名前で探します");
        egui::CollapsingHeader::new(i18n::tr("考え方: どの図を読む? (はじめての人はここから)")).id_salt("opto_howto").show(ui, |ui| {
            for t in [
                "LED の順方向 (Forward current vs. forward voltage): 25 °C の線を 1〜50 mA で 4〜6 点。「電流 [A] 電圧 [V]」。",
                "出力の点 (Collector current vs. collector-emitter voltage): IF ごとの線の、平らな所 (VCE 3〜7 V) を「IF VCE IC」で。IF を 2〜3 通り読むと、CTR と出力の傾き (VAF) が決まります。",
                "CTR (電流伝達率) = IC ÷ IF。実物はランクと個体で何倍も違います (PC817 は 50〜600%)。表の範囲は注記に書くだけで、モデルは出力の点に合わせた 1 つの値です。",
                "CTR と IF の図は、比べるために並べるだけです (別の個体の曲線のことが多い)。空でも作れます。",
                "カットオフ周波数から速さが決まります。表に応答時間 tr しか無ければ 0.35 ÷ tr で入れます。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if self.sheet.has_doc()
                && ui.button(i18n::tr("PDF の表から読む")).on_hover_text(i18n::tr("開いているデータシートの「電気的特性」の表から、VF・LED の容量・CTR の範囲 (CTR の行、無ければ IF のときの IC から)・VCE(sat) とその条件・カットオフ周波数を探して欄に入れます。LED と出力の曲線は図から読みます")).clicked()
            {
                let sheet = &self.sheet;
                self.opto.fill_from_pdf(|s| sheet.spec_values(s));
            }
            if ui.button(i18n::tr("PC817 の例を入れる")).on_hover_text(i18n::tr("同梱の PC817 を作ったときにグラフと表から読んだ値を入れます。自分のデータシートの値に書き換えて使います")).clicked() {
                self.opto_example_and_make();
            }
        });
        for (ok, line) in &self.opto.found {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::WARN }, egui::RichText::new(line).small());
        }
        self.read_all_button(ui, &[Dest::OptoLed, Dest::OptoCtr]);
        let has_doc = self.sheet.has_doc();
        let mut start = None;
        let o = &mut self.opto;
        egui::Grid::new("opto_inputs").num_columns(6).show(ui, |ui| {
            curve_row(ui, has_doc, "LED の順方向: IF VF", "1 行に「順方向電流 [A] 電圧 [V]」(25 °C)", &mut o.led, "1m 1.07\n10m 1.18\n50m 1.45", Dest::OptoLed, &mut start);
            ui.label(i18n::tr("出力: IF VCE IC")).on_hover_text(i18n::tr("1 行に「LED の電流 [A] VCE [V] コレクタ電流 [A]」。出力の図の平らな所から"));
            egui::ScrollArea::vertical().id_salt("opto_output").max_height(64.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut o.output).desired_rows(3).desired_width(170.0).min_size(egui::vec2(170.0, 0.0)).font(egui::TextStyle::Monospace).hint_text("5m 5 5.3m\n10m 5 12.4m\n20m 5 25m"));
            });
            ui.label("");
            ui.end_row();
            curve_row(ui, has_doc, "CTR の曲線: IF CTR%", "比べるだけ。1 行に「IF [A] CTR [%]」。空でも作れます", &mut o.ctr_curve, "1m 520\n5m 560", Dest::OptoCtr, &mut start);
            ui.end_row();
        });
        ui.horizontal_wrapped(|ui| {
            let small = |ui: &mut egui::Ui, label: &str, tip: &str, f: &mut String, hint: &str| {
                ui.label(i18n::tr_dyn(label)).on_hover_text(i18n::tr_dyn(tip));
                ui.add(egui::TextEdit::singleline(f).desired_width(50.0).hint_text(hint));
            };
            ui.label(i18n::tr("表の VF typ: IF VF")).on_hover_text(i18n::tr("比べるだけ。「20m 1.2」のように。空でも可"));
            ui.add(egui::TextEdit::singleline(&mut o.led_typ).desired_width(80.0).hint_text("20m 1.2"));
            small(ui, "LED の容量 [F]", "表の端子間容量 Ct", &mut o.led_cap, "30p");
            small(ui, "LED の逆電圧 [V]", "絶対最大定格の VR", &mut o.led_vr, "6");
            small(ui, "CTR min [%]", "表の電流伝達率の min", &mut o.ctr_min, "50");
            small(ui, "CTR max [%]", "表の電流伝達率の max", &mut o.ctr_max, "600");
            small(ui, "VCE(sat) [V]", "表の飽和電圧の typ", &mut o.vce_sat, "0.1");
            small(ui, "その IF [A]", "VCE(sat) の条件の IF", &mut o.sat_if, "20m");
            small(ui, "その IC [A]", "VCE(sat) の条件の IC", &mut o.sat_ic, "1m");
            small(ui, "fc [Hz]", "表のカットオフ周波数。80k = 80 kHz", &mut o.fc, "80k");
        });
        if let Some(dest) = start {
            self.sheet.start_graph(Some(dest));
        }
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作ってずれを見る")).on_hover_text(i18n::tr("入れた点に合わせてモデルを作り、点とモデルの値を並べます")).clicked() {
                self.opto_make();
            }
            if let Some(Ok(f)) = self.opto.result.clone()
                && ui.button(i18n::tr("「② シミュレーション」に入れる")).on_hover_text(i18n::tr("作ったモデルと注記を、シミュレーションのタブに入れます。ピンは名前で探します (A・K・E・C)")).clicked()
            {
                let roles: &[(&str, &[&str])] = &[("LED のアノード", &["A", "ANODE"]), ("LED のカソード", &["K", "CATHODE"]), ("エミッタ", &["E", "EMITTER"]), ("コレクタ", &["C", "COLLECTOR"])];
                self.opto.message = Some(match self.linear_apply(&f, roles) {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
            }
        });
        if let Some(Ok(f)) = &self.opto.result {
            super::mosfet::fit_table(ui, "opto_rows", f);
        }
        Self::result_box(ui, &self.opto.result, &self.opto.message);
    }

    pub(super) fn photodiode_ui(&mut self, ui: &mut egui::Ui) {
        use super::graph_reader::Dest;
        crate::widgets::section_heading(ui, "かんたん作成: フォトダイオードをデータシートの表からつくる");
        crate::icons::help_small(ui, "光電流・開放電圧・暗電流・容量からモデルを作ります", "同梱の BPW34 と同じ作り方です。光の強さはシミュレーション専用のピン IRRAD (1 V = 1 mW/cm²) で与えます。ピンは A・K と IRRAD (か L) の名前で探すので、「③ ピン」に IRRAD のピンを足しておいてください (基板には出ません)");
        egui::CollapsingHeader::new(i18n::tr("考え方: どの値を使う? (はじめての人はここから)")).id_salt("pd_howto").show(ui, |ui| {
            for t in [
                "感度: 表の「逆方向の光電流 (Ira・IL)」の typ。測定の光の強さ (Ee) が 1 mW/cm² でなければ、その値で割ります。光の強さに比例するとして扱います。",
                "光電流の点・短絡電流: 比べるためだけに並べます (空でも可)。光電流と光の強さの図から「Ee [mW/cm²] 電流 [A]」。",
                "開放電圧 (Vo): 光を当てて何もつながないときの電圧。これと感度から、ダイオードの IS が決まります。",
                "暗電流: 光が無いときに逆電圧で流れる電流。並列の抵抗が決まります。",
                "容量: 表の CD (0 V など) と、あれば容量と逆電圧の図から。逆電圧を上げると小さくなり、速く応答します。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if self.sheet.has_doc()
                && ui.button(i18n::tr("PDF の表から読む")).on_hover_text(i18n::tr("開いているデータシートの「電気的特性」の表から、光電流 (mW/cm² あたりに直す)・短絡電流・開放電圧とその温度係数・暗電流・降伏電圧・容量を探して欄に入れます")).clicked()
            {
                let sheet = &self.sheet;
                self.pd.fill_from_pdf(|s| sheet.spec_values(s));
            }
            if ui.button(i18n::tr("BPW34 の例を入れる")).on_hover_text(i18n::tr("同梱の BPW34 を作ったときに表と図から読んだ値を入れます。自分のデータシートの値に書き換えて使います")).clicked() {
                self.photodiode_example_and_make();
            }
        });
        for (ok, line) in &self.pd.found {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::WARN }, egui::RichText::new(line).small());
        }
        self.read_all_button(ui, &[Dest::PdLight, Dest::PdCap]);
        let has_doc = self.sheet.has_doc();
        let mut start = None;
        let p = &mut self.pd;
        ui.horizontal_wrapped(|ui| {
            let small = |ui: &mut egui::Ui, label: &str, tip: &str, f: &mut String, hint: &str| {
                ui.label(i18n::tr_dyn(label)).on_hover_text(i18n::tr_dyn(tip));
                ui.add(egui::TextEdit::singleline(f).desired_width(50.0).hint_text(hint));
            };
            small(ui, "感度 [A/(mW/cm²)]", "1 mW/cm² あたりの逆方向の光電流。50u = 50 µA", &mut p.sensitivity, "50u");
            small(ui, "短絡電流 [A]", "比べるだけ。1 mW/cm² で短絡したときの電流。空でも可", &mut p.isc, "47u");
            small(ui, "開放電圧 [V]", "1 mW/cm² で開放したときの電圧", &mut p.voc, "0.35");
            small(ui, "その温度係数 [mV/K]", "比べるだけ。ふつうマイナス (-2.6)。空でも可", &mut p.voc_tc, "-2.6");
            small(ui, "暗電流 [A]", "光が無いときの逆電流の typ", &mut p.dark_i, "2n");
            small(ui, "その逆電圧 [V]", "暗電流の測定条件", &mut p.dark_v, "10");
            small(ui, "降伏電圧 [V]", "表の V(BR) の min", &mut p.bv, "60");
            small(ui, "その電流 [A]", "降伏電圧の測定条件の電流", &mut p.ibv, "100u");
            small(ui, "N", "空なら 1。開放電圧が 2 つ以上の条件で分かるときだけ", &mut p.n, "1");
            small(ui, "RS [Ω]", "空なら 10 Ω", &mut p.rs, "10");
        });
        egui::Grid::new("pd_inputs").num_columns(6).show(ui, |ui| {
            curve_row(ui, has_doc, "光電流: Ee I", "比べるだけ。1 行に「光の強さ [mW/cm²] 電流 [A]」。空でも可", &mut p.light, "0.1 5u\n1 50u", Dest::PdLight, &mut start);
            curve_row(ui, has_doc, "容量: VR C", "1 行に「逆電圧 [V] 容量 [F]」", &mut p.cap, "0 70p\n3 26p\n10 16p", Dest::PdCap, &mut start);
            ui.end_row();
        });
        if let Some(dest) = start {
            self.sheet.start_graph(Some(dest));
        }
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作ってずれを見る")).on_hover_text(i18n::tr("入れた値からモデルを作り、表・図の値とモデルの値を並べます")).clicked() {
                self.pd_make();
            }
            if let Some(Ok(f)) = self.pd.result.clone()
                && ui.button(i18n::tr("「② シミュレーション」に入れる")).on_hover_text(i18n::tr("作ったモデルと注記を、シミュレーションのタブに入れます。ピンは名前で探します (A・K・IRRAD)")).clicked()
            {
                let roles: &[(&str, &[&str])] = &[("アノード", &["A", "ANODE"]), ("カソード", &["K", "CATHODE"]), ("光の強さ (シミュレーション専用)", &["IRRAD", "L", "LIGHT"])];
                self.pd.message = Some(match self.linear_apply(&f, roles) {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
            }
        });
        if let Some(Ok(f)) = &self.pd.result {
            super::mosfet::fit_table(ui, "pd_rows", f);
        }
        Self::result_box(ui, &self.pd.result, &self.pd.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundled part's model text.
    fn bundled(part: &str) -> String {
        let text = part_lib::EXAMPLES.iter().find(|(n, _)| *n == part).unwrap().1;
        let def: part_lib::PartDef = serde_json::from_str(text).unwrap();
        def.sim.unwrap().text
    }

    /// The numbers of a model text, in order (names and words dropped).
    fn numbers(text: &str) -> Vec<f64> {
        text.split(|c: char| c.is_whitespace() || c == '=' || c == '(' || c == ')' || c == '*').filter_map(|w| w.parse::<f64>().ok()).collect()
    }

    fn close(got: &str, want: &str) {
        let (g, w) = (numbers(got), numbers(want));
        assert_eq!(g.len(), w.len(), "{got}\n---\n{want}");
        for (a, b) in g.iter().zip(&w) {
            assert!((a - b).abs() <= 1e-3 * b.abs().max(1e-30), "{a} vs {b}\n{got}\n---\n{want}");
        }
    }

    #[test]
    fn the_examples_give_the_bundled_models() {
        let mut o = OptoInput::default();
        o.example();
        close(&fit::fit(&o.spec("PC817", "s").unwrap()).unwrap().text, &bundled("PC817_DIP-4"));
        let mut p = PhotodiodeInput::default();
        p.example();
        let mut spec = p.spec("BPW34", "s").unwrap();
        if let fit::Spec::Photodiode(s) = &mut spec {
            assert_eq!(s.voc_tc, Some(2.6), "num reads magnitudes; pd_make puts the sign back");
        }
        close(&fit::fit(&spec).unwrap().text, &bundled("BPW34"));
    }

    #[test]
    fn missing_values_say_what_and_where() {
        let mut o = OptoInput::default();
        assert!(o.spec("X", "s").unwrap_err().contains("3 つ以上"));
        o.example();
        o.output = "5m 5".into();
        assert!(o.spec("X", "s").unwrap_err().contains("3 つの数"));
        o.example();
        o.fc.clear();
        assert!(o.spec("X", "s").unwrap_err().contains("カットオフ"));
        let mut p = PhotodiodeInput::default();
        assert!(p.spec("X", "s").unwrap_err().contains("感度"));
        p.example();
        p.cap.clear();
        assert!(p.spec("X", "s").unwrap_err().contains("容量"));
    }
}
