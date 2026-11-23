//! "かんたん作成" for a synchronous buck converter with its switches inside (TPS62933): the data sheet's typ values go into the
//! template the bundled TPS62933 is made with (`part_lib::fit::buck`), the frequency law through the RT table's points, and the
//! switching model goes to the simulation tab with its notes. The example button fills the boxes from the bundled readings.

use super::*;
use part_lib::fit::{self, Fitted};

fn value(label: &str, t: &str) -> Result<f64, String> {
    sim_core::parse_value(t.trim()).filter(|v| v.is_finite()).ok_or_else(|| {
        if t.trim().is_empty() { format!("{label}が空です。データシートの typ を入れてください") } else { format!("{label}「{}」が数として読めません (例: 0.8、76m、5.5u)", t.trim()) }
    })
}

fn numbers(label: &str, t: &str) -> Result<Vec<f64>, String> {
    t.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(|s| value(label, s)).collect()
}

/// Exactly `N` numbers, or (`optional`) none.
fn fixed<const N: usize>(label: &str, t: &str, how: &str, optional: bool) -> Result<Option<[f64; N]>, String> {
    let v = numbers(label, t)?;
    if v.is_empty() && optional {
        return Ok(None);
    }
    v.try_into().map(Some).map_err(|_| format!("{label}は {how} で入れてください"))
}

fn show(v: f64) -> String {
    super::linear::show(v)
}

fn join(v: &[f64]) -> String {
    v.iter().map(|x| show(*x)).collect::<Vec<_>>().join(" ")
}

/// The boxes of the buck converter section, as typed.
#[derive(Clone, Default)]
pub struct BuckInput {
    pub vref: String,
    pub vref_range: String,
    /// "non-switching, shutdown" (A).
    pub iq: String,
    /// "rising falling" (V).
    pub uvlo: String,
    /// "rising falling Ip Ih", empty: no EN pin.
    pub en: String,
    /// "high side, low side" (Ω).
    pub rds: String,
    /// "peak (high side), valley (low side)" (A).
    pub ilim: String,
    /// The minimum peak current of PFM (A); empty: forced continuous.
    pub ipeak_min: String,
    /// The SS pin's current (A); empty: the internal soft start `tss`.
    pub iss: String,
    pub tss: String,
    /// "kΩ kHz" lines; empty: no RT pin (fixed at `open`).
    pub rt: String,
    /// "kHz above-kΩ": RT left open (or the fixed frequency).
    pub open: String,
    /// "kHz below-kΩ": RT tied to GND, empty: as open.
    pub short: String,
    /// "on off" (s).
    pub tmin: String,
    pub ovp: String,
    /// "Vout Cout bandwidth [phase margin]" of the data sheet's loop figure.
    pub loop_point: String,
    /// "gm gcs offset slope qg": not in a data sheet.
    pub internal: String,
    pub from_figures: String,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl BuckInput {
    pub fn from_spec(s: &fit::BuckSpec) -> BuckInput {
        let opt_pair = |v: Option<f64>, w: Option<f64>| match (v, w) {
            (Some(a), Some(b)) => join(&[a, b]),
            (Some(a), None) => show(a),
            _ => String::new(),
        };
        let l = &s.loop_point;
        let mut lp = vec![l.vout, l.cout, l.bw];
        lp.extend(l.pm);
        BuckInput {
            vref: show(s.vref),
            vref_range: s.vref_range.map(|r| join(&r)).unwrap_or_default(),
            iq: join(&[s.iq, s.i_shdn]),
            uvlo: join(&s.uvlo),
            en: s.en.as_ref().map(|e| join(&[e.rise, e.fall, e.ip, e.ih])).unwrap_or_default(),
            rds: join(&[s.rds_hs, s.rds_ls]),
            ilim: join(&[s.ihs_limit, s.ils_limit]),
            ipeak_min: s.ipeak_min.map(show).unwrap_or_default(),
            iss: s.iss.map(show).unwrap_or_default(),
            tss: if s.tss > 0.0 { show(s.tss) } else { String::new() },
            rt: s.fsw.rt.iter().map(|q| format!("{} {}", fit::num::g(q[0], 6), fit::num::g(q[1], 6))).collect::<Vec<_>>().join("\n"),
            open: opt_pair(Some(s.fsw.open), s.fsw.open_above),
            short: opt_pair(s.fsw.short, s.fsw.short_below),
            tmin: join(&[s.ton_min, s.toff_min]),
            ovp: s.ovp.map(|o| fit::num::g(o, 6)).unwrap_or_default(),
            loop_point: join(&lp),
            internal: join(&[s.gm, s.gcs, s.comp_offset, s.slope, s.qg]),
            from_figures: s.from_figures.clone(),
            ..Default::default()
        }
    }

    pub fn spec(&self, part: &str, source: &str) -> Result<fit::BuckSpec, String> {
        let [iq, i_shdn] = fixed::<2>("電源電流", &self.iq, "「12u 2u」(スイッチしていないとき・止めたとき [A])", false)?.unwrap_or_default();
        let uvlo = fixed::<2>("UVLO", &self.uvlo, "「3.6 3.3」(立ち上がり・立ち下がり [V])", false)?.unwrap_or_default();
        let [rds_hs, rds_ls] = fixed::<2>("オン抵抗", &self.rds, "「76m 32m」(上側・下側 [Ω])", false)?.unwrap_or_default();
        let [ihs_limit, ils_limit] = fixed::<2>("電流制限", &self.ilim, "「5 3.8」(上側のピーク・下側の谷 [A])", false)?.unwrap_or_default();
        let [ton_min, toff_min] = fixed::<2>("最小オン・オフ時間", &self.tmin, "「70n 140n」[s]", false)?.unwrap_or_default();
        let en = fixed::<4>("EN", &self.en, "「1.21 1.17 0.7u 1.4u」(立ち上がり・立ち下がり [V]・引き上げの電流・オンの間に足す電流 [A]) か空", true)?
            .map(|[rise, fall, ip, ih]| serde_json::json!({"rise": rise, "fall": fall, "ip": ip, "ih": ih}));
        let mut rt = vec![];
        for l in self.rt.lines().map(str::trim).filter(|l| !l.is_empty()) {
            match numbers("RT と周波数", l)?.as_slice() {
                [r, f] => rt.push([*r, *f]),
                _ => return Err("RT と周波数は 1 行に「RT [kΩ] 周波数 [kHz]」で入れてください (例: 71.5 310)".into()),
            }
        }
        let (open, open_above) = match numbers("RT 開放の周波数", &self.open)?.as_slice() {
            [f] => (*f, None),
            [f, r] => (*f, Some(*r)),
            _ => return Err("RT 開放の周波数は「500 280」(kHz と、これより大きい RT [kΩ] を開放とみる) か、周波数 1 つで入れてください".into()),
        };
        let (short, short_below) = match numbers("RT を GND の周波数", &self.short)?.as_slice() {
            [] => (None, None),
            [f] => (Some(*f), None),
            [f, r] => (Some(*f), Some(*r)),
            _ => return Err("RT を GND の周波数は「1200 1」(kHz と、これより小さい RT [kΩ] を GND とみる) か空で入れてください".into()),
        };
        let lp = numbers("ループの図", &self.loop_point)?;
        let (lvout, lcout, lbw, lpm) = match lp.as_slice() {
            [v, c, b] => (*v, *c, *b, None),
            [v, c, b, m] => (*v, *c, *b, Some(*m)),
            _ => return Err("ループの図は「5 44u 49.4k 57」(出力 [V]・実効の出力容量 [F]・帯域 [Hz]・位相余裕 [度]、位相余裕は無くても可) で入れてください".into()),
        };
        let internal = numbers("内部の値", &self.internal)?;
        let [gm, gcs, comp_offset, slope, qg] = match internal.as_slice() {
            [a, b, c, d] => [*a, *b, *c, *d, 5e-9],
            [a, b, c, d, e] => [*a, *b, *c, *d, *e],
            _ => return Err("内部の値は「300u 4 0.5 1.5 5n」(誤差アンプの gm・電流の指令 A/V・COMP のオフセット V・スロープ補償 A・ゲートの電荷 C、最後は無くても可) で入れてください".into()),
        };
        let opt = |label: &str, t: &str| -> Result<Option<f64>, String> { if t.trim().is_empty() { Ok(None) } else { value(label, t).map(Some) } };
        let iss = opt("SS ピンの電流", &self.iss)?;
        let tss = opt("内部のソフトスタートの時間", &self.tss)?.unwrap_or(0.0);
        let json = serde_json::json!({
            "part": part, "name": part_lib::diode::model_name(part), "source": source, "cite": source,
            "from_figures": self.from_figures,
            "vref": value("基準電圧", &self.vref)?,
            "vref_range": fixed::<2>("基準電圧の min・max", &self.vref_range, "「0.792 0.808」の 2 つの数", true)?,
            "iq": iq, "i_shdn": i_shdn, "uvlo": uvlo, "en": en,
            "rds_hs": rds_hs, "rds_ls": rds_ls, "ihs_limit": ihs_limit, "ils_limit": ils_limit,
            "ipeak_min": opt("PFM の最小ピーク電流", &self.ipeak_min)?,
            "iss": iss, "tss": tss,
            "fsw": {"rt": rt, "open": open, "open_above": open_above, "short": short, "short_below": short_below},
            "ton_min": ton_min, "toff_min": toff_min,
            "ovp": opt("OVP", &self.ovp)?,
            "gm": gm, "gcs": gcs, "comp_offset": comp_offset, "slope": slope, "qg": qg,
            "loop_point": {"vout": lvout, "cout": lcout, "bw": lbw, "pm": lpm},
        });
        let s: fit::BuckSpec = serde_json::from_value(json).map_err(|e| e.to_string())?;
        fit::buck::check_buck(&s)?;
        Ok(s)
    }
}

/// The bundled TPS62933's readings: the example.
fn tps62933() -> fit::BuckSpec {
    match serde_json::from_str::<fit::Spec>(include_str!("../../../part-lib/fits/TPS62933_SOT583.fit.json")) {
        Ok(fit::Spec::Buck(s)) => s,
        _ => unreachable!("the bundled TPS62933 readings are a buck"),
    }
}

/// The pins by role, by name; the optional pins only when their boxes are filled.
fn buck_roles(b: &BuckInput) -> Vec<(&'static str, &'static [&'static str])> {
    let mut r: Vec<(&str, &[&str])> = vec![
        ("入力", &["VIN", "IN", "PVIN"]),
        ("GND", &["GND", "PGND"]),
        ("SW", &["SW", "LX", "PH"]),
        ("BST", &["BST", "BOOT", "CB", "CBOOT"]),
        ("FB", &["FB", "VFB"]),
    ];
    if !b.en.trim().is_empty() {
        r.push(("EN", &["EN", "EN/UVLO"]));
    }
    if b.rt.lines().filter(|l| !l.trim().is_empty()).count() >= 2 {
        r.push(("RT", &["RT", "FREQ", "RT/CLK"]));
    }
    if !b.iss.trim().is_empty() {
        r.push(("SS", &["SS", "SS/TR", "TR"]));
    }
    r
}

impl PartForm {
    fn buck_make(&mut self) {
        let (name, source) = (self.linear_name("BUCK"), self.linear_source());
        self.buck.result = Some(self.buck.spec(&name, &source).and_then(|s| fit::fit(&fit::Spec::Buck(s))));
        self.buck.message = None;
    }

    /// The TPS62933 example in the boxes and made (`--form-buck-example`, which also sets its pins and puts the model into ②).
    pub fn buck_example_and_make(&mut self) {
        self.buck = BuckInput::from_spec(&tps62933());
        self.buck_make();
    }

    /// The TPS62933's SOT583 pins, named for the roles.
    pub fn buck_pins(&mut self) {
        self.pins_text = "3 VIN left\n2 EN left\n1 RT left\n7 SS left\n4 GND bottom\n6 BST right\n5 SW right\n8 FB right".into();
        self.prefix = "U".into();
    }

    pub fn buck_apply(&mut self) -> Result<String, String> {
        let Some(Ok(f)) = self.buck.result.clone() else { return Err("モデルができていません".into()) };
        let roles = buck_roles(&self.buck);
        self.linear_apply(&f, &roles)
    }

    pub(super) fn buck_ui(&mut self, ui: &mut egui::Ui) {
        crate::icons::help_small(
            ui,
            "表の typ の値からスイッチングのモデルを作ります",
            "スイッチを内蔵した同期整流の降圧 DC-DC コンバータのモデルを、同梱の TPS62933 と同じひな形で作ります。実際にスイッチを入り切りするので、過渡解析でデューティ・コイルのリップル電流・出力のリップル・ソフトスタートが見られます (計算は重め)。制御はピーク電流モードで、内部の補償はデータシートのループの図の帯域に合わせて決めます",
        );
        egui::CollapsingHeader::new(i18n::tr("考え方: どの値を使う? (はじめての人はここから)")).id_salt("buck_howto").show(ui, |ui| {
            for t in [
                "基準電圧 (VFB・VREF): FB ピンの電圧。出力 = 基準電圧 × (1 + 上の抵抗 ÷ 下の抵抗)。",
                "オン抵抗と電流制限: 表の上側 (High-side)・下側 (Low-side) の MOSFET の値。電流制限は上側のピークと下側の谷。",
                "周波数: RT の抵抗と周波数の表の点を 1 行ずつ (2 点以上でべき乗の式を通します)。RT を開放・GND にしたときの周波数も。RT ピンが無ければ点を空にして、開放の欄に固定の周波数を。",
                "軽い負荷: 「PFM の最小ピーク電流」(IPEAK_MIN) を入れると、軽い負荷で周期を飛ばす PFM に。空なら周波数を保つ連続モード (FCCM)。",
                "ループの図: データシートの「Loop Frequency Response」の図の条件 (出力電圧・実効の出力容量) と帯域 (BW)。内部の補償の値はデータシートに無いので、この帯域になるように決めます。",
                "内部の値 (gm・電流の指令・オフセット・スロープ・ゲートの電荷) はデータシートに無い仮の値です。ふつうは例のままで構いません。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(i18n::tr("TPS62933 の例を入れる")).on_hover_text(i18n::tr("同梱の TPS62933 を作ったときの値 (TI のデータシートの表と図) を入れます")).clicked() {
                self.buck_example_and_make();
            }
            if ui.button(i18n::tr("ピンを TPS62933 の並びにする")).on_hover_text(i18n::tr("ピン表を SOT583 の TPS62933 の並び (1 RT・2 EN・3 VIN・4 GND・5 SW・6 BST・7 SS・8 FB) にします")).clicked() {
                self.buck_pins();
            }
        });
        let b = &mut self.buck;
        let mut changed = false;
        egui::Grid::new("buck_inputs").num_columns(2).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, label: &str, tip: &str, f: &mut String, hint: &str, w: f32| -> bool {
                ui.label(i18n::tr(label)).on_hover_text(i18n::tr(tip));
                let c = ui.add(egui::TextEdit::singleline(f).desired_width(w).hint_text(hint)).changed();
                ui.end_row();
                c
            };
            changed |= row(ui, "基準電圧 typ [V]", "FB ピンの電圧 (VFB)", &mut b.vref, "0.8", 60.0);
            changed |= row(ui, "基準電圧 min max [V]", "25 ℃の部品ごとの違い (空でも可)", &mut b.vref_range, "0.792 0.808", 110.0);
            changed |= row(ui, "電源電流 [A]: 動作 止めたとき", "スイッチしていないときの電流 (IQ) と、EN で止めたときの電流 (ISHDN)", &mut b.iq, "12u 2u", 110.0);
            changed |= row(ui, "UVLO [V]: 立ち上がり 立ち下がり", "VIN がこれより上で動き始め、下で止まる", &mut b.uvlo, "3.6 3.3", 110.0);
            changed |= row(ui, "EN (空でも可)", "「立ち上がり 立ち下がり [V] 引き上げの電流 オンの間に足す電流 [A]」。空なら EN ピンなし", &mut b.en, "1.21 1.17 0.7u 1.4u", 160.0);
            changed |= row(ui, "オン抵抗 [Ω]: 上側 下側", "RDSON_HS と RDSON_LS", &mut b.rds, "76m 32m", 110.0);
            changed |= row(ui, "電流制限 [A]: ピーク 谷", "上側の電流制限 (IHS_LIMIT) と下側の電流制限 (ILS_LIMIT)", &mut b.ilim, "5 3.8", 110.0);
            changed |= row(ui, "PFM の最小ピーク電流 [A]", "IPEAK_MIN。空なら軽い負荷でも周波数を保つ連続モード (FCCM)", &mut b.ipeak_min, "0.75", 60.0);
            changed |= row(ui, "SS ピンの電流 [A]", "ソフトスタートのコンデンサを充電する電流 (ISS)。時間 = CSS × 基準電圧 ÷ この電流。空なら下の内部の時間", &mut b.iss, "5.5u", 60.0);
            changed |= row(ui, "内部のソフトスタート [s]", "SS ピンの無い部品の、決まったソフトスタートの時間 (TSS)", &mut b.tss, "2m", 60.0);
            ui.label(i18n::tr("RT と周波数: kΩ kHz")).on_hover_text(i18n::tr("1 行に「RT [kΩ] 周波数 [kHz]」を 2 行以上 (表か図の点)。空なら RT ピンなしで、下の「開放」の周波数に固定"));
            changed |= ui.add(egui::TextEdit::multiline(&mut b.rt).desired_rows(2).desired_width(160.0).font(egui::TextStyle::Monospace).hint_text("71.5 310\n9.09 2100")).changed();
            ui.end_row();
            changed |= row(ui, "RT 開放 [kHz] (と境の kΩ)", "RT を開放にしたときの周波数と、これより大きい抵抗を開放とみる境 (無ければ 280 kΩ)", &mut b.open, "500 280", 110.0);
            changed |= row(ui, "RT を GND [kHz] (と境の kΩ)", "RT を GND につないだときの周波数と、これより小さい抵抗を GND とみる境 (空なら開放と同じ)", &mut b.short, "1200 1", 110.0);
            changed |= row(ui, "最小オン・オフ時間 [s]", "tON_MIN と tOFF_MIN", &mut b.tmin, "70n 140n", 110.0);
            changed |= row(ui, "OVP (基準の何倍)", "出力の過電圧で上側を止める FB の割合 (115 % なら 1.15)。空なら無し", &mut b.ovp, "1.15", 60.0);
            changed |= row(ui, "ループの図: Vout Cout 帯域 位相余裕", "データシートのループの図の条件と、そこに書かれた帯域 [Hz] と位相余裕 [度]", &mut b.loop_point, "5 44u 49.4k 57", 160.0);
            changed |= row(ui, "内部の値 (仮): gm gcs オフセット スロープ Qg", "データシートに無い値。誤差アンプの gm [A/V]・COMP 1 V あたりの電流の指令 [A/V]・COMP のオフセット [V]・1 周期のスロープ補償 [A]・上側のゲートの電荷 [C]", &mut b.internal, "300u 4 0.5 1.5 5n", 160.0);
        });
        if changed {
            b.from_figures.clear();
        }
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作る")).on_hover_text(i18n::tr("入れた値をひな形に入れてモデルを作り、RT の点と式の周波数を並べます")).clicked() {
                self.buck_make();
            }
            if matches!(self.buck.result, Some(Ok(_)))
                && ui.button(i18n::tr("「② シミュレーション」に入れる")).on_hover_text(i18n::tr("作ったモデルと注記を、シミュレーションのタブに入れます。ピンは名前で探します (EN・RT・SS は欄が空なら要りません)")).clicked()
            {
                self.buck.message = Some(match self.buck_apply() {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
            }
        });
        if let Some(Ok(f)) = &self.buck.result
            && !f.rows.is_empty()
        {
            super::mosfet::fit_table(ui, "buck_rows", f);
        }
        Self::result_box(ui, &self.buck.result, &self.buck.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_gives_the_bundled_model_and_the_optional_pins_can_go() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../part-lib/examples/TPS62933_SOT583.part.json");
        let d: part_lib::PartDef = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let s = BuckInput::from_spec(&tps62933()).spec("TPS62933", "s").unwrap();
        assert_eq!(fit::fit(&fit::Spec::Buck(s)).unwrap().text, d.sim.unwrap().text);
        // no EN, no RT (a fixed frequency), no SS (an internal soft start): only the five pins are wanted
        let mut b = BuckInput::from_spec(&tps62933());
        (b.en, b.rt, b.short, b.iss, b.tss, b.open) = (String::new(), String::new(), String::new(), String::new(), "2m".into(), "500".into());
        let s = b.spec("X", "s").unwrap();
        assert!(s.en.is_none() && s.fsw.rt.is_empty() && s.iss.is_none());
        assert_eq!(buck_roles(&b).len(), 5);
        let text = fit::fit(&fit::Spec::Buck(s)).unwrap().text;
        assert!(text.starts_with(".subckt X vin gnd sw bst fb\n"), "{}", text.lines().next().unwrap());
        // a box that is wrong says which and how
        b.rds = "76m".into();
        assert!(b.spec("X", "s").unwrap_err().contains("オン抵抗"));
    }
}
