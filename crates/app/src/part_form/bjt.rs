//! "かんたん作成" for a bipolar transistor (M10): the points of the data sheet's typical curves (hFE, VBE(on),
//! VBE(sat) / VCE(sat), the capacitances; read with the graph window or typed) and fT, fitted with the same Gummel-Poon
//! model as the bundled transistors (`part_lib::fit`), with the table of how far the model lands from each point.

use super::*;
use part_lib::fit::{self, Fitted};

/// What the form holds for the transistor section, as typed ("value value" lines).
pub struct BjtInput {
    /// (IC, hFE) lines: the hFE graph's values themselves.
    pub hfe: String,
    /// How the hFE values were made from a normalized graph (the table's hFE they were scaled to), for the model's notes.
    pub hfe_note: Option<String>,
    /// VCE of the hFE curve.
    pub hfe_vce: String,
    /// (IC, VBE) lines at `on_vce`.
    pub vbe_on: String,
    pub on_vce: String,
    /// (IC, V) lines at IC/IB = 10.
    pub vbe_sat: String,
    pub vce_sat: String,
    /// (reverse voltage, capacitance) lines; may be empty.
    pub cobo: String,
    pub cibo: String,
    /// fT (Hz) and whether it is the typ (else the table's min).
    pub ft: String,
    pub ft_typ: bool,
    /// Early voltage (V); empty: 100 V assumed.
    pub vaf: String,
    /// Fit NF too (a VBE(on) curve that rises more than 60 mV a decade).
    pub fit_nf: bool,
    /// What was taken from the PDF's table (one line each), or what was not found.
    pub found: Vec<(bool, String)>,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl Default for BjtInput {
    fn default() -> Self {
        BjtInput {
            hfe: String::new(),
            hfe_note: None,
            hfe_vce: "1".into(),
            vbe_on: String::new(),
            on_vce: "1".into(),
            vbe_sat: String::new(),
            vce_sat: String::new(),
            cobo: String::new(),
            cibo: String::new(),
            ft: String::new(),
            ft_typ: true,
            vaf: String::new(),
            fit_nf: false,
            found: vec![],
            result: None,
            message: None,
        }
    }
}

/// The bundled 2N3904's readings (the example button).
const EXAMPLE: &str = include_str!("../../../part-lib/fits/2N3904_TO-92.fit.json");

pub(super) fn num(label: &str, t: &str) -> Result<f64, String> {
    let l = t.trim().to_ascii_lowercase();
    // "300MHz": in SPICE's way of writing numbers "m" is milli, so the unit's prefix is turned into "meg" / "g" first
    let l = if let Some(n) = l.strip_suffix("mhz") { format!("{n}meg") } else if let Some(n) = l.strip_suffix("ghz") { format!("{n}g") } else { l.trim_end_matches("hz").to_string() };
    let l = l.trim_end_matches(['v', 'a', 'f']).trim();
    sim_core::parse_value(l).map(f64::abs).filter(|v| *v > 0.0).ok_or_else(|| format!("{label}「{}」を、0 より大きい数で入れてください (例: 10m、0.7、300meg)", t.trim()))
}

/// "value value" lines as pairs (magnitudes: a PNP's negative readings are taken as they are drawn).
pub(super) fn pairs(label: &str, text: &str) -> Result<Vec<[f64; 2]>, String> {
    let mut out = vec![];
    for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
        let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
        let [a, b] = t.as_slice() else {
            return Err(format!("{label}の {}行目: 2 つの数 (例: 10m 0.7) で書いてください", n + 1));
        };
        out.push([num(label, a)?, num(label, b)?]);
    }
    out.sort_by(|a, b| a[0].total_cmp(&b[0]));
    Ok(out)
}

/// Like [`pairs`], but the first number of a line may be 0 (a capacitance at 0 V).
pub(super) fn pairs_from_zero(label: &str, text: &str) -> Result<Vec<[f64; 2]>, String> {
    let zero_first = |l: &str| l.split(|c: char| c.is_whitespace() || c == ',').find(|w| !w.is_empty()).and_then(|w| sim_core::parse_value(w.trim_end_matches('v'))) == Some(0.0);
    let (zeros, rest): (Vec<&str>, Vec<&str>) = text.lines().partition(|l| zero_first(l));
    let mut out = pairs(label, &rest.join("
"))?;
    for l in zeros {
        let second = l.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).nth(1).unwrap_or("");
        out.push([0.0, num(label, second)?]);
    }
    out.sort_by(|a, b| a[0].total_cmp(&b[0]));
    Ok(out)
}

pub(super) fn lines(points: &[[f64; 2]]) -> String {
    use super::graph_reader::short;
    points.iter().map(|[a, b]| format!("{} {}", short(*a), short(*b))).collect::<Vec<_>>().join("\n")
}

impl BjtInput {
    /// The readings as the fit takes them.
    pub fn spec(&self, name: &str, source: &str, pnp: bool) -> Result<fit::Spec, String> {
        let hfe = pairs("hFE", &self.hfe)?;
        if hfe.len() < 3 {
            return Err("hFE の点を 3 つ以上入れてください (hFE と IC の図から、小さい電流・山・大きい電流)".into());
        }
        let vbe_on = pairs("VBE(on)", &self.vbe_on)?;
        let (vbe_sat, vce_sat) = (pairs("VBE(sat)", &self.vbe_sat)?, pairs("VCE(sat)", &self.vce_sat)?);
        if vbe_on.is_empty() && vbe_sat.is_empty() {
            return Err("VBE(on) か VBE(sat) の点を入れてください (ベースの電圧が無いと、IS が決まりません)".into());
        }
        let (cobo, cibo) = (pairs("Cobo", &self.cobo)?, pairs("Cibo", &self.cibo)?);
        let ft = if self.ft.trim().is_empty() { None } else { Some(num("fT", &self.ft)?) };
        let vaf = if self.vaf.trim().is_empty() {
            (100.0, "入力が無いので 100 V と仮定 (データシートに出力の特性があれば、IC と VCE の線の傾きから決めます)".to_string())
        } else {
            (num("VAF", &self.vaf)?, "入力した値".to_string())
        };
        let json = serde_json::json!({
            "kind": "bjt",
            "part": name,
            "name": part_lib::diode::model_name(name),
            "pnp": pnp,
            "source": source,
            "figs": {"hfe": "hFE と IC の図", "on": "VBE・飽和電圧の図", "cap": "容量の図"},
            "hfe_peak": 1.0,
            "hfe_absolute": true,
            "hfe_note": self.hfe_note.clone().unwrap_or_else(|| "hFE と IC の図の値をそのまま使いました。実物の hFE は、ランクと個体で大きく違います (表の min・max)。".into()),
            "hfe_norm": hfe,
            "hfe_vce": num("hFE の VCE", &self.hfe_vce)?,
            "fit_nf": self.fit_nf,
            "vbe_on": vbe_on,
            "on_vce": num("VBE(on) の VCE", &self.on_vce)?,
            "vbe_sat": vbe_sat,
            "vce_sat": vce_sat,
            "cobo": cobo,
            "cibo": cibo,
            "ft_typ": if self.ft_typ { ft } else { None },
            "ft_min": if self.ft_typ { None } else { ft },
            "vaf": vaf,
        });
        serde_json::from_value(json).map_err(|e| e.to_string())
    }

    /// Fills the boxes with the bundled 2N3904's readings (its hFE curve scaled to hFE, its VAF from its hoe).
    pub fn example(&mut self) {
        let v: serde_json::Value = serde_json::from_str(EXAMPLE).unwrap_or_default();
        let get = |k: &str| -> Vec<[f64; 2]> { serde_json::from_value(v[k].clone()).unwrap_or_default() };
        let peak = v["hfe_peak"].as_f64().unwrap_or(1.0);
        let hfe: Vec<[f64; 2]> = get("hfe_norm").into_iter().map(|[i, h]| [i, h * peak]).collect();
        self.hfe = lines(&hfe);
        self.hfe_note = None;
        self.vbe_on = lines(&get("vbe_on"));
        self.vbe_sat = lines(&get("vbe_sat"));
        self.vce_sat = lines(&get("vce_sat"));
        self.cobo = lines(&get("cobo"));
        self.cibo = lines(&get("cibo"));
        (self.hfe_vce, self.on_vce) = ("1".into(), "1".into());
        self.ft = "300meg".into();
        self.ft_typ = false;
        // hoe min / max at IC = 1 mA, VCE = 10 V: VAF = IC / hoe (geometric mean) − VCE, to 10 V as the bundled fit has it
        let hoe: [f64; 2] = serde_json::from_value(v["hoe"].clone()).unwrap_or([1e-6, 4e-5]);
        self.vaf = part_lib::fit::num::g(((1e-3 / (hoe[0] * hoe[1]).sqrt() - 10.0) / 10.0).round_ties_even() * 10.0, 4);
        self.fit_nf = false;
        self.found.clear();
    }

    /// Fills the boxes from the open PDF's table (typ values with their currents), and says what was not found. `name`
    /// (the part's name) picks a rank's rows ("BC547B · (IC = ...)") when the table has rows per rank.
    pub fn fill_from_pdf(&mut self, name: &str, values: impl Fn(&str) -> Vec<datasheet::spec_table::SpecValue>) {
        use super::graph_reader::short;
        use datasheet::spec_table::SpecValue;
        self.found.clear();
        self.hfe_note = None;
        // hFE has no unit: its typ as printed
        let typ = |v: &SpecValue| if v.unit.as_deref().is_none_or(|u| u.trim().is_empty() || u == "-" || u == "—") { v.typ.map(f64::abs) } else { v.typ_si().map(f64::abs) };
        let at = |v: &SpecValue| format!("p.{}「{} {}」", v.page + 1, v.symbol, v.conditions);
        let first = |symbols: &[&str]| symbols.iter().map(|s| values(s)).find(|v| !v.is_empty()).unwrap_or_default();
        let name = name.trim();
        // (IC, value) lines of a symbol's typ rows, where the rows give IC. `forced`: the saturation rows, kept only at
        // IC/IB = 10 (the fit's condition) when the row gives IB
        let mut rows = |symbols: &[&str], field: &mut String, label: &str, forced: bool| {
            let mut vals = first(symbols);
            // rows per rank: the part's own rank when its name is among them
            if !name.is_empty() && vals.iter().any(|v| v.conditions.contains(name)) {
                vals.retain(|v| v.conditions.contains(name) || !v.conditions.contains('·'));
            }
            let all = vals.len();
            let pts: Vec<[f64; 2]> = vals
                .iter()
                .filter(|v| {
                    // a row that names IB without a number ("IB = See Note 1") is another condition
                    !forced || match v.condition("IB") {
                        Some(ib) => v.condition("IC").is_some_and(|ic| (8.0..=12.0).contains(&(ic / ib).abs())),
                        None => !v.conditions.contains("IB"),
                    }
                })
                .filter_map(|v| Some([v.condition("IC")?.abs(), typ(v)?]))
                .collect();
            let twice = pts.windows(2).any(|w| (w[0][0] / w[1][0] - 1.0).abs() < 0.01) || {
                let mut ics: Vec<f64> = pts.iter().map(|p| p[0]).collect();
                ics.sort_by(f64::total_cmp);
                ics.windows(2).any(|w| (w[0] / w[1] - 1.0).abs() < 0.01)
            };
            if pts.is_empty() {
                self.found.push((false, if all == 0 {
                    format!("{label} が表に見つかりません")
                } else if forced {
                    format!("{label} に、IC/IB = 10 で typ と IC のそろった行がありません。グラフから読んでください")
                } else {
                    format!("{label} に typ と IC のそろった行がありません。グラフから読んでください")
                }));
            } else if twice {
                self.found.push((false, format!("{label}: 同じ IC の行が何行もあります (ランクごとの値など)。部品名にランクまで入れる (例: BC547B) か、グラフから読んでください")));
            } else {
                *field = lines(&pts);
                let ics: Vec<String> = pts.iter().map(|p| format!("{}A", part_lib::passive::eng(p[0]))).collect();
                self.found.push((true, format!("{label} {} 点 (IC = {}): p.{}。IC の単位を表と見比べてください (µ が文字で抜けることがあります)", pts.len(), ics.join("、"), vals[0].page + 1)));
            }
        };
        rows(&["hFE", "HFE"], &mut self.hfe, "hFE", false);
        rows(&["VBE(on)", "VBE"], &mut self.vbe_on, "VBE(on)", false);
        rows(&["VBE(sat)"], &mut self.vbe_sat, "VBE(sat)", true);
        rows(&["VCE(sat)"], &mut self.vce_sat, "VCE(sat)", true);
        let ft = first(&["fT", "fT"]);
        match ft.iter().find_map(|v| typ(v).map(|t| (t, true, v)).or_else(|| v.min_si().map(|m| (m.abs(), false, v)))) {
            Some((f, is_typ, v)) => {
                (self.ft, self.ft_typ) = (short(f), is_typ);
                self.found.push((true, format!("fT {} MHz ({}): {}", part_lib::fit::num::g(f / 1e6, 4), if is_typ { "typ" } else { "min" }, at(v))));
            }
            None => self.found.push((false, "fT が表に見つかりません".into())),
        }
        for (sym, cond, field) in [("Cobo", "VCB", 0), ("Cibo", "VEB", 1)] {
            let vals = first(&[sym, if field == 0 { "Cob" } else { "Cib" }]);
            match vals.iter().find_map(|v| Some((typ(v).or(v.max_si().map(f64::abs))?, v.condition(cond).map(f64::abs).unwrap_or(0.0), v))) {
                Some((c, vr, v)) => {
                    let line = format!("{} {}", short(vr), short(c));
                    if field == 0 { self.cobo = line } else { self.cibo = line }
                    self.found.push((true, format!("{sym} {}F ({} V): {}。グラフがあれば、そちらから何点か読むと電圧による変化も入ります", short(c), short(vr), at(v))));
                }
                None => self.found.push((false, format!("{sym} が表に見つかりません"))),
            }
        }
    }
}

impl PartForm {
    fn bjt_pnp(&self) -> bool {
        self.glyph == "pnp"
    }

    pub(super) fn bjt_fit(&mut self) {
        let source = if self.datasheet.trim().is_empty() { "データシート".to_string() } else { self.datasheet.trim().to_string() };
        let name = if self.name.trim().is_empty() { "BJT".to_string() } else { self.name.trim().to_string() };
        self.bjt.result = Some(self.bjt.spec(&name, &source, self.bjt_pnp()).and_then(|s| fit::fit(&s)));
        self.bjt.message = None;
    }

    /// Puts the fitted model into the simulation tab.
    pub(super) fn bjt_apply(&mut self) -> Result<String, String> {
        let Some(Ok(f)) = &self.bjt.result else { return Err("先に「モデルを作ってずれを見る」を押してください".into()) };
        let pins = self.parse_pins()?;
        let find = |n: &str| pins.iter().find(|p| super::pin_import::role_letter(&p.name).eq_ignore_ascii_case(n)).map(|p| p.number.clone());
        let (Some(c), Some(b), Some(e)) = (find("C"), find("B"), find("E")) else {
            return Err("ピン表に、C・B・E の名前のピンが要ります (「③ ピン」で直してください)".into());
        };
        self.sim = SimChoice::Model;
        self.sim_element = "Q".into();
        self.sim_name = f.text.split_whitespace().nth(1).unwrap_or("Q").to_string();
        self.sim_text = f.text.clone();
        self.sim_pins = format!("{c} {b} {e}");
        self.sim_limits = f.limits.join("\n");
        self.sim_origin = Some((sch_core::ModelOrigin::new(sch_core::OriginKind::Equivalent, f.source.clone()), self.sim_text.clone()));
        Ok("モデルを「② シミュレーション」に入れました。そちらの「試し読み」で確かめられます".into())
    }

    /// Takes points read off a graph for one of the transistor's fields.
    pub(super) fn bjt_take_graph(&mut self, dest: super::graph_reader::Dest, points: &[(f64, f64)], page: usize) {
        use super::graph_reader::Dest;
        let text = lines(&points.iter().map(|(a, b)| [a.abs(), b.abs()]).collect::<Vec<_>>());
        let field = match dest {
            Dest::BjtHfe => &mut self.bjt.hfe,
            Dest::BjtVbeOn => &mut self.bjt.vbe_on,
            Dest::BjtVbeSat => &mut self.bjt.vbe_sat,
            Dest::BjtVceSat => &mut self.bjt.vce_sat,
            Dest::BjtCobo => &mut self.bjt.cobo,
            Dest::BjtCibo => &mut self.bjt.cibo,
            _ => return,
        };
        *field = text;
        if dest == Dest::BjtHfe {
            self.bjt.hfe_note = None;
        }
        self.bjt.result = None;
        self.bjt.message = Some((true, format!("p.{} のグラフの {} 点を入れました", page + 1, points.len())));
    }

    /// "PDF の表から読む" and "モデルを作ってずれを見る" pressed (`--form-bjt-pdf`, for screenshots).
    pub fn bjt_from_pdf_and_fit(&mut self) {
        let sheet = &self.sheet;
        let name = self.name.clone();
        self.bjt.fill_from_pdf(&name, |s| sheet.spec_values(s));
        self.bjt_fit();
    }

    /// The 2N3904 example fitted (`--form-bjt-example`, for screenshots).
    pub fn bjt_example_and_fit(&mut self) {
        self.bjt.example();
        self.bjt_fit();
    }

    pub(super) fn bjt_ui(&mut self, ui: &mut egui::Ui) {
        use super::graph_reader::Dest;
        crate::widgets::section_heading(ui, "かんたん作成: トランジスタをデータシートのグラフからつくる");
        crate::icons::help_small(ui, "グラフの点からモデルを作ります", "データシートの代表特性のグラフ (hFE・ベースの電圧・飽和電圧・容量) の点と fT から、同梱のトランジスタと同じ作り方 (Gummel-Poon) でモデルを作り、読んだ点とのずれを並べます");
        egui::CollapsingHeader::new(i18n::tr("考え方: どのグラフを読む? (はじめての人はここから)")).id_salt("bjt_howto").show(ui, |ui| {
            for t in [
                "hFE と IC の図 (DC Current Gain): 小さい電流・山・大きい電流で 5〜8 点。山の高さ (BF) と、大きい電流で下がる所 (IKF) が決まります。図が相対値 (山が 1) なら、表の typ を掛けて hFE にします。",
                "VBE(on) と IC の図 (ベースの電圧): IS (飽和電流) が決まります。1 桁で約 60 mV 上がります。それより大きく上がる図なら「NF も当てはめる」に入れます。",
                "VBE(sat)・VCE(sat) と IC の図 (IC/IB = 10 の飽和): ベースとコレクタの抵抗、逆方向の電流増幅率 BR が決まります。",
                "Cobo・Cibo と逆電圧の図 (容量): CJC・CJE が決まります。表の値 1 点でも入れられます。fT (遷移周波数) から、速さ TF が決まります。",
                "PNP は、マイナスの付いた値も大きさで入れます。グラフの窓の「縦軸の値を先に書く」は、IC が縦軸の図 (IC と VBE の図など) のときに入れます。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if self.sheet.has_doc()
                && ui.button(i18n::tr("PDF の表から読む")).on_hover_text(i18n::tr("開いているデータシートの「電気的特性」の表から、hFE・VBE・飽和電圧の typ (IC つき)、fT、Cobo・Cibo を探して欄に入れます。表は点が少ないので、グラフからも読むと良いモデルになります")).clicked()
            {
                let sheet = &self.sheet;
                self.bjt.fill_from_pdf(&self.name, |s| sheet.spec_values(s));
                self.bjt.result = None;
            }
            if ui.button(i18n::tr("2N3904 の例を入れる")).on_hover_text(i18n::tr("同梱の 2N3904 を作ったときにグラフから読んだ点を入れます。自分のデータシートの値に書き換えて使います")).clicked() {
                self.bjt.example();
                self.bjt_fit();
            }
        });
        for (ok, line) in &self.bjt.found {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::WARN }, egui::RichText::new(line).small());
        }
        self.read_all_button(ui, &[Dest::BjtHfe, Dest::BjtVbeOn, Dest::BjtVbeSat, Dest::BjtVceSat, Dest::BjtCobo, Dest::BjtCibo]);
        let has_doc = self.sheet.has_doc();
        let mut start = None;
        egui::Grid::new("bjt_inputs").num_columns(6).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, label: &str, tip: &str, field: &mut String, hint: &str, dest: Dest, start: &mut Option<Dest>| {
                ui.label(i18n::tr(label)).on_hover_text(i18n::tr(tip));
                // a fixed height with a scroll bar: the curves have up to ten points each, and the table of gaps comes below
                egui::ScrollArea::vertical().id_salt(label).max_height(64.0).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(field).desired_rows(3).desired_width(170.0).min_size(egui::vec2(170.0, 0.0)).font(egui::TextStyle::Monospace).hint_text(hint));
                });
                if has_doc && ui.small_button(i18n::tr("グラフから読む")).on_hover_text(i18n::tr("データシートの窓が、図の見出し (DC Current Gain など) からこの図のページへ移り、軸の目盛りも図の数字から入れます。曲線を 1 回クリックすると点が取れます (目盛りが読めない図は、目盛りからクリック)。同じページの図なら、目盛りはそのまま使えます")).clicked() {
                    *start = Some(dest);
                }
            };
            row(ui, "hFE: IC hFE", "1 行に「コレクタ電流 [A] hFE」。m は 1000 分の 1 (10m = 10 mA)", &mut self.bjt.hfe, "1m 135\n10m 171\n100m 47", Dest::BjtHfe, &mut start);
            row(ui, "VBE(on): IC VBE", "1 行に「コレクタ電流 [A] ベースの電圧 [V]」(下の VCE のとき)", &mut self.bjt.vbe_on, "10m 0.706\n100m 0.869", Dest::BjtVbeOn, &mut start);
            ui.end_row();
            row(ui, "VBE(sat): IC V", "IC/IB = 10 で飽和させたときのベースの電圧。1 行に「IC [A] 電圧 [V]」", &mut self.bjt.vbe_sat, "10m 0.745\n100m 0.927", Dest::BjtVbeSat, &mut start);
            row(ui, "VCE(sat): IC V", "IC/IB = 10 で飽和させたときのコレクタ・エミッタ間の電圧", &mut self.bjt.vce_sat, "20m 0.117\n100m 0.36", Dest::BjtVceSat, &mut start);
            ui.end_row();
            row(ui, "Cobo: VCB C", "出力容量 (エミッタを開いて、コレクタ・ベース間)。1 行に「逆電圧 [V] 容量 [F]」。空でも作れます", &mut self.bjt.cobo, "1 2.78p\n10 1.69p", Dest::BjtCobo, &mut start);
            row(ui, "Cibo: VEB C", "入力容量 (コレクタを開いて、エミッタ・ベース間)。空でも作れます", &mut self.bjt.cibo, "0.5 3.76p\n5 2.41p", Dest::BjtCibo, &mut start);
            ui.end_row();
        });
        if let Some(d) = start {
            self.sheet.start_graph(Some(d));
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("hFE の VCE [V]")).on_hover_text(i18n::tr("hFE の図の測定条件の VCE (図の中か、題に書いてあります)"));
            ui.add(egui::TextEdit::singleline(&mut self.bjt.hfe_vce).desired_width(40.0).min_size(egui::vec2(40.0, 0.0)));
            ui.label(i18n::tr("VBE(on) の VCE [V]")).on_hover_text(i18n::tr("ベースの電圧の図の測定条件の VCE"));
            ui.add(egui::TextEdit::singleline(&mut self.bjt.on_vce).desired_width(40.0).min_size(egui::vec2(40.0, 0.0)));
            ui.label(i18n::tr("fT [Hz]")).on_hover_text(i18n::tr("遷移周波数。300meg = 300 MHz。空なら速さ (TF) は入れません"));
            ui.add(egui::TextEdit::singleline(&mut self.bjt.ft).desired_width(70.0).hint_text("300meg"));
            ui.checkbox(&mut self.bjt.ft_typ, i18n::tr("typ")).on_hover_text(i18n::tr("表の typ なら入れます。min しか無いときは外します (min のぶん遅いモデルになります)"));
            ui.label(i18n::tr("VAF [V]")).on_hover_text(i18n::tr("Early 電圧 (出力の特性の線の傾き)。空なら 100 V と仮定します"));
            ui.add(egui::TextEdit::singleline(&mut self.bjt.vaf).desired_width(50.0).hint_text("100"));
            ui.checkbox(&mut self.bjt.fit_nf, i18n::tr("NF も当てはめる")).on_hover_text(i18n::tr("ベースの電圧の図が、電流 1 桁で 60 mV より大きく上がるときに入れます (入れないと hFE の曲線が曲がります)"));
        });
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作ってずれを見る")).on_hover_text(i18n::tr("入れた点に合わせてモデルを作り、点とモデルの値を並べます (数秒かかることがあります)")).clicked() {
                self.bjt_fit();
            }
            if matches!(self.bjt.result, Some(Ok(_))) && ui.button(i18n::tr("「② シミュレーション」に入れる")).on_hover_text(i18n::tr("作ったモデルと、その注記 (何から作り、何を扱わないか) を、シミュレーションのタブに入れます")).clicked() {
                self.bjt.message = Some(match self.bjt_apply() {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
            }
        });
        match &self.bjt.result {
            Some(Ok(f)) => {
                ui.monospace(&f.text);
                super::mosfet::fit_table(ui, "bjt_rows", f);
            }
            Some(Err(e)) => {
                ui.colored_label(crate::theme::ERROR, e);
            }
            None => {}
        }
        if let Some((ok, m)) = &self.bjt.message {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::ERROR }, m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `NAME=value` pairs of a `.model` card.
    fn params(text: &str) -> Vec<(String, f64)> {
        let inner = text.split_once('(').unwrap().1.trim_end_matches(')');
        inner.split_whitespace().map(|kv| kv.split_once('=').unwrap()).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect()
    }

    #[test]
    fn the_example_fits_like_the_bundled_2n3904() {
        let mut b = BjtInput::default();
        b.example();
        let f = fit::fit(&b.spec("2N3904", "s", false).unwrap()).unwrap();
        let bundled = part_lib::EXAMPLES.iter().find(|(n, _)| *n == "2N3904_TO-92").unwrap().1;
        let def: part_lib::PartDef = serde_json::from_str(bundled).unwrap();
        let want = params(&def.sim.unwrap().text);
        let got = params(&f.text);
        for (k, v) in &want {
            let g = got.iter().find(|(n, _)| n == k).unwrap_or_else(|| panic!("{k} missing in {}", f.text)).1;
            assert!((g / v - 1.0).abs() < 0.002, "{k}: {g} vs {v}");
        }
    }

    #[test]
    fn a_frequency_with_its_unit_reads_as_hertz() {
        assert_eq!(num("fT", "300MHz").unwrap(), 300e6);
        assert_eq!(num("fT", "300meg").unwrap(), 300e6);
        assert_eq!(num("fT", "1.5GHz").unwrap(), 1.5e9);
        assert_eq!(num("IC", "10m").unwrap(), 0.01);
    }

    #[test]
    fn missing_and_bad_points_say_which_and_how() {
        let mut b = BjtInput::default();
        b.hfe = "1m 100\n10m 150".into();
        assert!(b.spec("X", "s", false).unwrap_err().contains("3 つ以上"));
        b.hfe = "1m 100\n10m 150\n100m 80".into();
        assert!(b.spec("X", "s", false).unwrap_err().contains("VBE"));
        b.vbe_on = "10m".into();
        assert!(b.spec("X", "s", false).unwrap_err().contains("1行目"));
    }

    #[test]
    fn the_pdf_table_fills_currents_and_says_what_is_missing() {
        use datasheet::spec_table::SpecValue;
        let v = |symbol: &str, conditions: &str, min: Option<f64>, typ: Option<f64>, unit: &str| SpecValue { page: 1, symbol: symbol.into(), conditions: conditions.into(), min, typ, max: None, unit: Some(unit.into()), rect: datasheet::Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 } };
        let table = [
            v("hFE", "BC547A · (IC = 2.0 mA, VCE = 5.0 V)", Some(110.0), Some(180.0), ""),
            v("hFE", "BC547B · (IC = 2.0 mA, VCE = 5.0 V)", Some(200.0), Some(290.0), ""),
            v("VCE(sat)", "IC = 10 mA, IB = 0.5 mA", None, Some(0.09), "V"),
            v("VCE(sat)", "IC = 100 mA, IB = 10 mA", None, Some(0.2), "V"),
            v("fT", "IC = 10 mA, VCE = 5.0 V", Some(150.0), None, "MHz"),
        ];
        let read = |b: &mut BjtInput, name: &str| b.fill_from_pdf(name, |s| table.iter().filter(|x| x.symbol.eq_ignore_ascii_case(s)).cloned().collect());
        // two ranks at the same current: nothing is put in without the rank in the name
        let mut b = BjtInput::default();
        read(&mut b, "BC547");
        assert_eq!(b.hfe, "");
        assert!(b.found.iter().any(|(ok, l)| !ok && l.contains("ランク")), "{:?}", b.found);
        read(&mut b, "BC547B");
        assert_eq!(b.hfe, "0.002 290");
        // only the IC/IB = 10 row of the saturation voltage (the fit's condition)
        assert_eq!(b.vce_sat, "0.1 0.2");
        assert_eq!((b.ft.as_str(), b.ft_typ), ("1.5e+08", false));
        assert!(b.found.iter().any(|(ok, l)| !ok && l.contains("Cobo")), "{:?}", b.found);
    }
}
