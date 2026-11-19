//! "かんたん作成" for a Darlington pair (Q1 ①): the pair's hFE, its "ON" voltages and the built-in resistors of the
//! data sheet's schematic, fitted with the same model as the bundled TIP120 / TIP125 (`part_lib::fit::DarlingtonSpec`:
//! two Gummel-Poon transistors, the driver `area` times smaller), with the table of gaps.

use super::bjt::{lines, num, pairs, pairs_from_zero};
use super::*;
use part_lib::fit::{self, Fitted};

/// What the form holds for the Darlington box, as typed.
pub struct DarlingtonInput {
    /// The built-in resistors (base to the inner node, inner node to emitter) and how much larger the output
    /// transistor is than the driver.
    pub r1: String,
    pub r2: String,
    pub area: String,
    /// (IC, hFE) lines at `hfe_vce`.
    pub hfe: String,
    pub hfe_vce: String,
    /// (IC, VBE) lines at `on_vce`.
    pub vbe_on: String,
    pub on_vce: String,
    /// (IC, V) lines at IC/IB = `sat_ratio`.
    pub vbe_sat: String,
    pub vce_sat: String,
    pub sat_ratio: String,
    /// (reverse voltage, capacitance) lines; may be empty.
    pub cib: String,
    pub cob: String,
    /// fT (Hz); empty: no TF.
    pub ft: String,
    /// Early voltage (V); empty: 100 V assumed.
    pub vaf: String,
    /// What was taken from the PDF's table (one line each), or what was not found.
    pub found: Vec<(bool, String)>,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl Default for DarlingtonInput {
    fn default() -> Self {
        DarlingtonInput {
            r1: String::new(),
            r2: String::new(),
            area: "10".into(),
            hfe: String::new(),
            hfe_vce: "4".into(),
            vbe_on: String::new(),
            on_vce: "4".into(),
            vbe_sat: String::new(),
            vce_sat: String::new(),
            sat_ratio: "250".into(),
            cib: String::new(),
            cob: String::new(),
            ft: String::new(),
            vaf: String::new(),
            found: vec![],
            result: None,
            message: None,
        }
    }
}

/// The bundled TIP120's readings (the example button).
const EXAMPLE: &str = include_str!("../../../part-lib/fits/TIP120_TO-220.fit.json");

impl DarlingtonInput {
    /// The readings as the fit takes them.
    pub fn spec(&self, name: &str, source: &str, pnp: bool) -> Result<fit::Spec, String> {
        let hfe = pairs("hFE", &self.hfe)?;
        let (vbe_on, vbe_sat, vce_sat) = (pairs("VBE(on)", &self.vbe_on)?, pairs("VBE(sat)", &self.vbe_sat)?, pairs("VCE(sat)", &self.vce_sat)?);
        if hfe.len() < 3 || vbe_on.len() < 2 || vbe_sat.len() < 2 || vce_sat.len() < 2 {
            return Err("点が足りません。hFE は 3 点以上、VBE(on)・VBE(sat)・VCE(sat) は 2 点以上入れてください (hFE と IC の図、「ON」電圧の図から)".into());
        }
        if self.r1.trim().is_empty() || self.r2.trim().is_empty() {
            return Err("内蔵の抵抗 R1・R2 [Ω] を入れてください。データシートの回路の図に書いてあります (TIP120 は約 8k と 120)".into());
        }
        let area = num("大きさの比", &self.area)?;
        if area < 1.0 {
            return Err("大きさの比は 1 以上で入れてください (出力のトランジスタが駆動の何倍か。わからなければ 10)".into());
        }
        let ft = if self.ft.trim().is_empty() { None } else { Some((num("fT", &self.ft)?, "入力した値".to_string())) };
        let vaf = if self.vaf.trim().is_empty() {
            (100.0, "入力が無いので 100 V と仮定しました (データシートに出力の特性が無いことが多い)".to_string())
        } else {
            (num("VAF", &self.vaf)?, "入力した値".to_string())
        };
        let json = serde_json::json!({
            "kind": "darlington",
            "part": name,
            "name": part_lib::diode::model_name(name),
            "pnp": pnp,
            "source": source,
            "r1": num("R1", &self.r1)?,
            "r2": num("R2", &self.r2)?,
            "area": area,
            "figs": {"hfe": "hFE と IC の図", "on": "「ON」電圧の図", "cap": "容量の図"},
            "hfe": hfe,
            "hfe_vce": num("hFE の VCE", &self.hfe_vce)?,
            "vbe_on": vbe_on,
            "on_vce": num("VBE(on) の VCE", &self.on_vce)?,
            "vbe_sat": vbe_sat,
            "vce_sat": vce_sat,
            "sat_ratio": num("IC/IB", &self.sat_ratio)?,
            "cib": pairs_from_zero("Cib", &self.cib)?,
            "cob": pairs_from_zero("Cob", &self.cob)?,
            "ft": ft,
            "vaf": vaf,
            "diode": ["IS=1e-09 N=1.5 RS=0.05", "データシートに特性が無いので、一般的な値 (IS = 1 nA、N = 1.5、RS = 0.05 Ω) を仮に置きました"],
        });
        serde_json::from_value(json).map_err(|e| e.to_string())
    }

    /// Fills the boxes with the bundled TIP120's readings.
    pub fn example(&mut self) {
        let v: serde_json::Value = serde_json::from_str(EXAMPLE).unwrap_or_default();
        let get = |k: &str| -> Vec<[f64; 2]> { serde_json::from_value(v[k].clone()).unwrap_or_default() };
        let n = |k: &str| v[k].as_f64().map(super::graph_reader::short).unwrap_or_default();
        *self = DarlingtonInput {
            r1: n("r1"),
            r2: n("r2"),
            area: n("area"),
            hfe: lines(&get("hfe")),
            hfe_vce: n("hfe_vce"),
            vbe_on: lines(&get("vbe_on")),
            on_vce: n("on_vce"),
            vbe_sat: lines(&get("vbe_sat")),
            vce_sat: lines(&get("vce_sat")),
            sat_ratio: n("sat_ratio"),
            cib: lines(&get("cib")),
            cob: lines(&get("cob")),
            ft: v["ft"][0].as_f64().map(|f| format!("{f}")).unwrap_or_default(),
            vaf: String::new(),
            found: vec![],
            result: None,
            message: None,
        };
    }
}

impl PartForm {
    pub(super) fn darl_fit(&mut self) {
        let (name, source) = (self.linear_name("DARL"), self.linear_source());
        let pnp = self.glyph == "pnp";
        self.darl.result = Some(self.darl.spec(&name, &source, pnp).and_then(|s| fit::fit(&s)));
        self.darl.message = None;
    }

    /// The TIP120 example fitted (`--form-darlington-example`, for screenshots).
    pub fn darlington_example_and_fit(&mut self) {
        self.darl.example();
        self.darl_fit();
    }

    /// Takes points read off a graph for one of the Darlington's fields.
    pub(super) fn darl_take_graph(&mut self, dest: super::graph_reader::Dest, points: &[(f64, f64)], page: usize) {
        use super::graph_reader::Dest;
        let text = lines(&points.iter().map(|(a, b)| [a.abs(), b.abs()]).collect::<Vec<_>>());
        let field = match dest {
            Dest::DarlHfe => &mut self.darl.hfe,
            Dest::DarlVbeOn => &mut self.darl.vbe_on,
            Dest::DarlVbeSat => &mut self.darl.vbe_sat,
            Dest::DarlVceSat => &mut self.darl.vce_sat,
            _ => return,
        };
        *field = text;
        self.darl.result = None;
        self.darl.message = Some((true, format!("p.{} のグラフの {} 点を入れました", page + 1, points.len())));
    }

    pub(super) fn darlington_ui(&mut self, ui: &mut egui::Ui) {
        use super::graph_reader::Dest;
        crate::widgets::section_heading(ui, "かんたん作成: ダーリントンをデータシートのグラフからつくる");
        crate::icons::help_small(ui, "2 個つないだトランジスタを 1 つの回路として当てはめます", "TIP120 のような、トランジスタ 2 個と抵抗 2 本が 1 つに入った部品です。データシートの「2 個合わせた」hFE と電圧の図の点から、同梱の TIP120 と同じ作り方でモデルを作り、読んだ点とのずれを並べます");
        egui::CollapsingHeader::new(i18n::tr("考え方: どの図を読む? (はじめての人はここから)")).id_salt("darl_howto").show(ui, |ui| {
            for t in [
                "内蔵の抵抗 R1・R2: データシートの最初のほうにある回路の図 (ベースから中の点・中の点からエミッタ) の値です。",
                "hFE と IC の図 (DC Current Gain): 2 個合わせた値なので 1000 を超えます。25 °C の線を 5〜10 点。",
                "「ON」電圧の図 (VBE(sat)・VBE(on)・VCE(sat) と IC): 3 本の線が 1 つの図にあることが多いです。飽和の条件 IC/IB (TIP120 は 250) も図に書いてあります。",
                "大きさの比: 出力のトランジスタが駆動の何倍か。データシートには無いので、わからなければ 10 のままにします。",
                "容量 (Cib・Cob) と fT は空でも作れます。PNP (TIP125 など) は、マイナスの値も大きさで入れます。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if self.sheet.has_doc()
                && ui.button(i18n::tr("PDF の表から読む")).on_hover_text(i18n::tr("開いているデータシートの「電気的特性」の表から、hFE・VBE・飽和電圧の typ (IC つき)、fT (無ければ |hfe| × 周波数)、Cob・Cib を探して欄に入れます。多くの表は保証値 (min・max) だけなので、曲線は図から読みます")).clicked()
            {
                let sheet = &self.sheet;
                self.darl.fill_from_pdf(|s| sheet.spec_values(s));
                self.darl.put_resistors(sheet.darlington_resistors());
            }
            if ui.button(i18n::tr("TIP120 の例を入れる")).on_hover_text(i18n::tr("同梱の TIP120 を作ったときにグラフから読んだ点を入れます。自分のデータシートの値に書き換えて使います")).clicked() {
                self.darlington_example_and_fit();
            }
        });
        for (ok, line) in &self.darl.found {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::WARN }, egui::RichText::new(line).small());
        }
        self.read_all_button(ui, &[Dest::DarlHfe, Dest::DarlVbeOn, Dest::DarlVbeSat, Dest::DarlVceSat]);
        let has_doc = self.sheet.has_doc();
        let mut start = None;
        let d = &mut self.darl;
        ui.horizontal_wrapped(|ui| {
            let small = |ui: &mut egui::Ui, label: &str, tip: &str, f: &mut String, hint: &str| {
                ui.label(i18n::tr_dyn(label)).on_hover_text(i18n::tr_dyn(tip));
                ui.add(egui::TextEdit::singleline(f).desired_width(50.0).hint_text(hint));
            };
            small(ui, "R1 [Ω]", i18n::tr("ベースと中の点の間の抵抗 (データシートの回路の図)"), &mut d.r1, "8k");
            small(ui, "R2 [Ω]", i18n::tr("中の点とエミッタの間の抵抗"), &mut d.r2, "120");
            small(ui, i18n::tr("大きさの比"), i18n::tr("出力のトランジスタが駆動の何倍の大きさか (データシートに無い仮定。10 が目安)"), &mut d.area, "10");
            small(ui, i18n::tr("hFE の VCE [V]"), i18n::tr("hFE の図の測定条件の VCE"), &mut d.hfe_vce, "4");
            small(ui, i18n::tr("VBE(on) の VCE [V]"), i18n::tr("VBE(on) の線の測定条件の VCE"), &mut d.on_vce, "4");
            small(ui, i18n::tr("飽和の IC/IB"), i18n::tr("VBE(sat)・VCE(sat) の線の条件 (図に書いてあります)"), &mut d.sat_ratio, "250");
        });
        egui::Grid::new("darl_inputs").num_columns(6).show(ui, |ui| {
            curve_row(ui, has_doc, "hFE: IC hFE", "1 行に「コレクタ電流 [A] 2 個合わせた hFE」", &mut d.hfe, "0.5 2000\n2 4500\n7 1100", Dest::DarlHfe, &mut start);
            curve_row(ui, has_doc, "VBE(on): IC V", "1 行に「IC [A] ベースの電圧 [V]」(上の VCE のとき)", &mut d.vbe_on, "0.5 1.35\n3 1.72", Dest::DarlVbeOn, &mut start);
            ui.end_row();
            curve_row(ui, has_doc, "VBE(sat): IC V", "飽和させたときのベースの電圧 (上の IC/IB のとき)", &mut d.vbe_sat, "0.5 1.41\n3 1.84", Dest::DarlVbeSat, &mut start);
            curve_row(ui, has_doc, "VCE(sat): IC V", "飽和させたときのコレクタ・エミッタ間の電圧", &mut d.vce_sat, "0.5 0.75\n3 1.15", Dest::DarlVceSat, &mut start);
            ui.end_row();
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("Cib: V C")).on_hover_text(i18n::tr("入力容量 (エミッタ・ベース間)。1 行に「逆電圧 [V] 容量 [F]」。Cob と 2 点ずつあるときだけ使います"));
            ui.add(egui::TextEdit::multiline(&mut d.cib).desired_rows(2).desired_width(110.0).font(egui::TextStyle::Monospace).hint_text("0.1 104p\n2 59p"));
            ui.label(i18n::tr("Cob: V C")).on_hover_text(i18n::tr("出力容量 (コレクタ・ベース間)。空でも作れます"));
            ui.add(egui::TextEdit::multiline(&mut d.cob).desired_rows(2).desired_width(110.0).font(egui::TextStyle::Monospace).hint_text("0.1 157p\n0.5 141p"));
            ui.label(i18n::tr("fT [Hz]")).on_hover_text(i18n::tr("2 個合わせた遷移周波数。表に |hfe| (1 MHz) の min しか無ければ、その値 × 1 MHz。空なら速さ (TF) は入れません"));
            ui.add(egui::TextEdit::singleline(&mut d.ft).desired_width(60.0).hint_text("4meg"));
            ui.label(i18n::tr("VAF [V]")).on_hover_text(i18n::tr("Early 電圧。空なら 100 V と仮定します"));
            ui.add(egui::TextEdit::singleline(&mut d.vaf).desired_width(50.0).hint_text("100"));
        });
        if let Some(dest) = start {
            self.sheet.start_graph(Some(dest));
        }
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作ってずれを見る")).on_hover_text(i18n::tr("入れた点に合わせてモデルを作り、点とモデルの値を並べます (数秒かかることがあります)")).clicked() {
                self.darl_fit();
            }
            if let Some(Ok(f)) = self.darl.result.clone()
                && ui.button(i18n::tr("「② シミュレーション」に入れる")).on_hover_text(i18n::tr("作ったモデルと注記を、シミュレーションのタブに入れます。ピンは名前で探します (C・B・E)")).clicked()
            {
                let roles: &[(&str, &[&str])] = &[("コレクタ", &["C", "COLLECTOR"]), ("ベース", &["B", "BASE"]), ("エミッタ", &["E", "EMITTER"])];
                self.darl.message = Some(match self.linear_apply(&f, roles) {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
            }
        });
        if let Some(Ok(f)) = &self.darl.result {
            super::mosfet::fit_table(ui, "darl_rows", f);
        }
        Self::result_box(ui, &self.darl.result, &self.darl.message);
    }
}

/// One multi-line field of "value value" lines with its "グラフから読む" button (shared by the Darlington and the
/// light parts' boxes).
#[allow(clippy::too_many_arguments)]
pub(super) fn curve_row(ui: &mut egui::Ui, has_doc: bool, label: &str, tip: &str, field: &mut String, hint: &str, dest: super::graph_reader::Dest, start: &mut Option<super::graph_reader::Dest>) {
    ui.label(i18n::tr_dyn(label)).on_hover_text(i18n::tr_dyn(tip));
    egui::ScrollArea::vertical().id_salt(label).max_height(64.0).show(ui, |ui| {
        ui.add(egui::TextEdit::multiline(field).desired_rows(3).desired_width(170.0).min_size(egui::vec2(170.0, 0.0)).font(egui::TextStyle::Monospace).hint_text(hint));
    });
    if has_doc && ui.small_button(i18n::tr("グラフから読む")).on_hover_text(i18n::tr("データシートの窓が、図の見出しからこの図のページへ移り、軸の目盛りも図の数字から入れます。曲線を 1 回クリックすると点が取れます (目盛りが読めない図は、目盛りからクリック)")).clicked() {
        *start = Some(dest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_points_and_resistors_say_what_to_put() {
        let mut d = DarlingtonInput::default();
        assert!(d.spec("X", "s", false).unwrap_err().contains("点が足りません"));
        d.example();
        d.r1.clear();
        assert!(d.spec("X", "s", false).unwrap_err().contains("R1"));
        d.r1 = "8k".into();
        d.area = "0.5".into();
        assert!(d.spec("X", "s", false).unwrap_err().contains("1 以上"));
    }

    #[test]
    fn the_example_reads_as_the_bundled_tip120() {
        let mut d = DarlingtonInput::default();
        d.example();
        let Ok(fit::Spec::Darlington(s)) = d.spec("TIP120", "s", false) else { panic!("not a Darlington") };
        let Ok(fit::Spec::Darlington(want)) = serde_json::from_str::<fit::Spec>(EXAMPLE) else { panic!() };
        assert_eq!((s.r1, s.r2, s.area, s.sat_ratio), (want.r1, want.r2, want.area, want.sat_ratio));
        assert_eq!((s.hfe.len(), s.vbe_on.len(), s.vce_sat.len(), s.cib.len()), (want.hfe.len(), want.vbe_on.len(), want.vce_sat.len(), want.cib.len()));
        assert_eq!(s.ft.map(|f| f.0), want.ft.map(|f| f.0));
    }
}
