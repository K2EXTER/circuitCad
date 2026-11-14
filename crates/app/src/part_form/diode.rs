//! "かんたん作成" for a rectifier / switching / Schottky diode: the learner reads a few points off the data
//! sheet's forward characteristics, the form fits IS, N and RS, shows the curve against the points, and
//! writes the `.model` card. The explanation is part of the section, so it doubles as a short tutorial.
//! "ずれの表を見る" fits the same way as the bundled diodes (`part_lib::fit`, with the reverse ratings, the capacitance
//! and trr) and lists how far the model lands from each number; "PDF の表から読む" takes IR (and its VR), CT and trr.

use super::*;
use part_lib::diode;

/// The 1N4007 example: readings like those of a typical forward-characteristics graph.
const EXAMPLE_POINTS: &str = "10m 0.70\n100m 0.79\n1 0.92";

impl PartForm {
    /// Points typed as "current voltage" lines (`10m 0.7`, an `A` / `V` after a number is allowed).
    pub(super) fn dio_points(&self) -> Result<Vec<(f64, f64)>, String> {
        let num = |s: &str| {
            let l = s.trim().to_ascii_lowercase();
            sim_core::parse_value(l.trim_end_matches(['a', 'v']).trim())
        };
        let mut out = vec![];
        for (n, line) in self.dio_points.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',' || c == '|').filter(|s| !s.is_empty()).collect();
            match (t.first().and_then(|s| num(s)), t.get(1).and_then(|s| num(s))) {
                (Some(i), Some(v)) => out.push((i, v)),
                _ => return Err(format!("{}行目: 「電流 電圧」の形で入力してください (例: 10m 0.7)", n + 1)),
            }
        }
        Ok(out)
    }

    /// Fit IS, N and RS to the typed points and put them into the sliders.
    pub(super) fn dio_fit(&mut self) -> Result<String, String> {
        let pts = self.dio_points()?;
        let f = diode::fit(&pts)?;
        (self.dio_is, self.dio_n, self.dio_rs) = (f.is, f.n, f.rs);
        let mut m = format!("IS = {}A、N = {:.2}、RS = {}Ω と求めました。下の絵で、点とモデルの曲線が重なっているか確かめてください", passive::eng(f.is), f.n, passive::eng(f.rs));
        for n in &f.notes {
            m.push_str(&format!("\n注意: {n}"));
        }
        Ok(m)
    }

    /// The readings as the bundled diodes' fit takes them (`part_lib::fit::DiodeSpec`).
    pub(super) fn dio_spec(&self) -> Result<part_lib::fit::Spec, String> {
        let pts = self.dio_points()?;
        let num = |label: &str, t: &str| -> Result<Option<f64>, String> {
            let t = t.trim();
            if t.is_empty() {
                return Ok(None);
            }
            sim_core::parse_value(&t.to_ascii_lowercase()).filter(|v| *v > 0.0).map(Some).ok_or_else(|| format!("{label}「{t}」を、0 より大きい数で入力してください"))
        };
        let (Some(vrrm), Some(ir)) = (num("逆耐圧", &self.dio_vrrm)?, num("漏れ電流", &self.dio_ir)?) else {
            return Err("ずれの表には、手順3 の逆耐圧と漏れ電流が要ります (表から写します)".into());
        };
        let mut caps = vec![];
        for (n, line) in self.dio_caps.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
            let v = |s: &str| sim_core::parse_value(s.to_ascii_lowercase().trim_end_matches(['v', 'f']));
            match (t.first().and_then(|s| v(s)), t.get(1).and_then(|s| v(s))) {
                (Some(vr), Some(c)) if vr >= 0.0 && c > 0.0 => caps.push([vr, c]),
                _ => return Err(format!("容量の点の {}行目: 「逆電圧 容量」の形で入力してください (例: 4 15p)", n + 1)),
            }
        }
        let cap_at = if !caps.is_empty() {
            "容量と逆電圧のグラフから読んだ点".to_string()
        } else if let Some(cj) = num("接合容量 Cj", &self.dio_cj)? {
            let vr = sim_core::parse_value(&self.dio_cj_vr.trim().to_ascii_lowercase()).unwrap_or(0.0);
            caps.push([vr, cj]);
            "表の接合容量 1 点 (電圧による変化は VJ・M を仮定)".to_string()
        } else {
            "データシートの容量を入れていないので、入れていません".to_string()
        };
        let recovery = match num("逆回復時間 trr", &self.dio_trr)? {
            Some(trr) => {
                let i_f = num("trr の測定条件 IF", &self.dio_trr_if)?.ok_or("trr の測定条件の電流 IF を入れてください")?;
                let i_r = num("trr の測定条件 IR", &self.dio_trr_ir)?.ok_or("trr の測定条件の電流 IR を入れてください")?;
                serde_json::json!({"trr": trr, "if": i_f, "ir": i_r, "note": format!("表の trr {}s (IF = {}A、IR = {}A)", passive::eng(trr), passive::eng(i_f), passive::eng(i_r))})
            }
            None => serde_json::Value::Null,
        };
        let name = if self.name.trim().is_empty() { "D".to_string() } else { self.name.trim().to_string() };
        let source = if self.datasheet.trim().is_empty() { "データシート".to_string() } else { self.datasheet.trim().to_string() };
        let json = serde_json::json!({
            "kind": "diode",
            "part": name,
            "name": diode::model_name(&name),
            "source": source,
            "forward": {"at": "順方向特性のグラフから読んだ点", "points": pts.iter().map(|(i, v)| [*i, *v]).collect::<Vec<_>>()},
            "breakdown": {"v": vrrm, "i": ir, "note": format!("表の逆耐圧 {} V と、その漏れ電流 {}A", part_lib::fit::num::g(vrrm, 4), passive::eng(ir))},
            "capacitance": {"at": cap_at, "points": caps},
            "recovery": recovery,
        });
        serde_json::from_value(json).map_err(|e| e.to_string())
    }

    /// "ずれの表を見る": fits like the bundled diodes and puts IS / N / RS into the sliders too.
    pub(super) fn dio_fit_table(&mut self) {
        self.dio_result = Some(self.dio_spec().and_then(|s| part_lib::fit::fit(&s)));
        if let Some(Ok(f)) = &self.dio_result {
            let get = |k: &str| f.text.split_whitespace().find_map(|w| w.strip_prefix(&format!("{k}="))).and_then(|v| v.trim_end_matches(')').parse::<f64>().ok());
            if let (Some(is), Some(n), Some(rs)) = (get("IS"), get("N"), get("RS")) {
                (self.dio_is, self.dio_n, self.dio_rs) = (is, n, rs);
            }
        }
    }

    /// Fills VRRM / IR / Cj / trr from the open PDF's table (what is not found is said and left as it was).
    pub(super) fn dio_fill_from_pdf(&mut self) {
        use super::graph_reader::short;
        use datasheet::spec_table::SpecValue;
        let sheet = &self.sheet;
        let first = |symbols: &[&str]| symbols.iter().map(|s| sheet.spec_values(s)).find(|v| !v.is_empty()).unwrap_or_default();
        let at = |v: &SpecValue| format!("p.{}「{} {}」", v.page + 1, v.symbol, v.conditions);
        let mut found = vec![];
        // the leakage: its value at the rated voltage gives both numbers (VRRM is in the ratings table, which has no typ)
        let ir = first(&["IR", "IRM"]);
        let hot = |v: &SpecValue| v.conditions.contains("125") || v.conditions.contains("100");
        match ir.iter().filter(|v| !hot(v)).find_map(|v| Some((v.typ_si().or(v.max_si())?.abs(), v.condition("VR"), v))) {
            Some((i, vr, v)) => {
                self.dio_ir = short(i);
                if let Some(vr) = vr {
                    self.dio_vrrm = short(vr.abs());
                }
                found.push((true, format!("IR {}A{}: {}", short(i), vr.map_or(String::new(), |vr| format!(" (VR = {} V を逆耐圧に)", short(vr.abs()))), at(v))));
            }
            None => found.push((false, "IR (逆電流) が表に見つかりません。逆耐圧と一緒に、自分で入れてください".to_string())),
        }
        let cj = first(&["CJ", "CT", "CD"]);
        match cj.iter().find_map(|v| Some((v.typ_si().or(v.max_si())?.abs(), v.condition("VR").unwrap_or(0.0).abs(), v))) {
            Some((c, vr, v)) => {
                (self.dio_cj, self.dio_cj_vr) = (short(c), short(vr));
                found.push((true, format!("容量 {}F ({} V): {}", short(c), short(vr), at(v))));
            }
            None => found.push((false, "接合容量が表に見つかりません".to_string())),
        }
        let trr = first(&["trr", "tRR"]);
        match trr.iter().find_map(|v| Some((v.typ_si().or(v.max_si())?.abs(), v))) {
            Some((t, v)) => {
                self.dio_trr = short(t);
                if let Some(i) = v.condition("IF") {
                    self.dio_trr_if = short(i.abs());
                }
                if let Some(i) = v.condition("IR") {
                    self.dio_trr_ir = short(i.abs());
                }
                found.push((true, format!("trr {}s: {}。IF・IR の条件を確かめてください", short(t), at(v))));
            }
            None => found.push((false, "trr が表に見つかりません (ショットキーなら無くて構いません)".to_string())),
        }
        self.dio_found = found;
        self.dio_result = None;
    }

    /// Write the model into the simulation tab (and the maximum voltage into the ratings).
    pub(super) fn dio_apply(&mut self) -> Result<String, String> {
        if let Some(Ok(f)) = &self.dio_result {
            // the model of the gaps table (the same fit as the bundled diodes, with its notes)
            let pins = self.parse_pins()?;
            let find = |n: &str| pins.iter().find(|p| p.name.eq_ignore_ascii_case(n)).map(|p| p.number.clone());
            let (Some(a), Some(k)) = (find("A"), find("K")) else {
                return Err("ピン表に、アノード A とカソード K の名前のピンが要ります (「③ ピン」で、名前を A と K にしてください)".into());
            };
            let (text, limits, source) = (f.text.clone(), f.limits.join("\n"), f.source.clone());
            self.sim = SimChoice::Model;
            self.sim_element = "D".into();
            self.sim_name = text.split_whitespace().nth(1).unwrap_or("D").to_string();
            self.sim_text = text;
            self.sim_pins = format!("{a} {k}");
            self.sim_limits = limits;
            self.sim_origin = Some((sch_core::ModelOrigin::new(sch_core::OriginKind::Equivalent, source), self.sim_text.clone()));
            if self.rt_v.trim().is_empty() && !self.dio_vrrm.trim().is_empty() {
                self.rt_v = self.dio_vrrm.trim().to_string();
            }
            return Ok("ずれの表のモデル (同梱のダイオードと同じ作り方) を「② シミュレーション」に入れました。そちらの「試し読み」で確かめられます".into());
        }
        if self.name.trim().is_empty() {
            return Err("先に「名前など」の部品名を入れてください (モデルの名前に使います。例: 1N4007)".into());
        }
        let pins = self.parse_pins()?;
        let find = |n: &str| pins.iter().find(|p| p.name.eq_ignore_ascii_case(n)).map(|p| p.number.clone());
        let (Some(a), Some(k)) = (find("A"), find("K")) else {
            return Err("ピン表に、アノード A とカソード K の名前のピンが要ります (「③ ピン」で、名前を A と K にしてください)".into());
        };
        let num = |label: &str, t: &str| -> Result<Option<f64>, String> {
            let t = t.trim();
            if t.is_empty() {
                return Ok(None);
            }
            sim_core::parse_value(&t.to_ascii_lowercase()).filter(|v| *v > 0.0).map(Some).ok_or_else(|| format!("{label}「{t}」を、0 より大きい数で入力してください"))
        };
        let vrrm = num("逆耐圧", &self.dio_vrrm)?;
        let ir = num("漏れ電流", &self.dio_ir)?;
        let mut notes = vec![];
        let ir = if vrrm.is_none() && ir.is_some() {
            notes.push("逆耐圧が空なので、漏れ電流は使いませんでした");
            None
        } else {
            ir
        };
        // charge storage: Cj at a reverse voltage -> CJO; trr at IF / IR -> TT
        let mut charge = diode::Charge::default();
        if let Some(cj) = num("接合容量 Cj", &self.dio_cj)? {
            let vr = if self.dio_cj_vr.trim().is_empty() { 0.0 } else { sim_core::parse_value(&self.dio_cj_vr.trim().to_ascii_lowercase()).filter(|v| *v >= 0.0).ok_or("Cj を測った逆電圧を、0 以上の数 (V) で入力してください (例: 4)")? };
            charge = diode::Charge { cj0: diode::cj0_from(cj, vr, diode::DEFAULT_VJ, diode::DEFAULT_M)?, vj: diode::DEFAULT_VJ, m: diode::DEFAULT_M, tt: 0.0 };
            notes.push("接合容量 CJO を求めて入れました (VJ=0.7・M=0.5 と仮定)");
        }
        if let Some(trr) = num("逆回復時間 trr", &self.dio_trr)? {
            let i_f = num("trr の測定条件 IF", &self.dio_trr_if)?.ok_or("trr の測定条件の電流 IF を入れてください (データシートの trr の注に書いてあります)")?;
            let i_r = num("trr の測定条件 IR", &self.dio_trr_ir)?.ok_or("trr の測定条件の電流 IR を入れてください (データシートの trr の注に書いてあります)")?;
            charge.tt = diode::tt_from_trr(trr, i_f, i_r)?;
            notes.push("逆回復時間から TT (拡散の遷移時間) を求めて入れました (目安です)");
        }
        self.sim = SimChoice::Model;
        self.sim_element = "D".into();
        self.sim_name = diode::model_name(&self.name);
        self.sim_text = diode::model_text(&self.name, self.dio_is, self.dio_n, self.dio_rs, vrrm, ir, charge);
        self.sim_pins = format!("{a} {k}");
        self.sim_origin = Some((sch_core::ModelOrigin::new(sch_core::OriginKind::Equivalent, self.datasheet.trim()), self.sim_text.clone()));
        if self.sim_limits.trim().is_empty() {
            self.sim_limits = "IS・N・RS は、データシートのグラフから読み取った点に合わせた近似です。個体差と温度で、実物は変わります。\n接合容量・逆回復時間は、入れた数値だけの近似です (入れていなければ扱いません)。\n耐圧や定格電流を超えても壊れません。データシートの定格は、自分で確かめてください。".into();
        }
        if let (Some(v), true) = (vrrm, self.rt_v.trim().is_empty()) {
            self.rt_v = passive::eng(v);
            notes.push("逆耐圧を「① 基本」の定格の最大電圧にも入れました");
        }
        let tail = if notes.is_empty() { String::new() } else { format!("。{}", notes.join("。")) };
        Ok(format!("モデルを「② シミュレーション」に入れました。そちらの「試し読み」で確かめられます{tail}"))
    }

    pub(super) fn diode_ui(&mut self, ui: &mut egui::Ui) {
        crate::widgets::section_heading(ui, "かんたん作成: ダイオードをデータシートの数値からつくる");
        crate::icons::help_small(ui, "グラフの数字からモデルを作ります", "メーカーのデータシートには SPICE モデルが載っていないことが多いので、グラフの数字から IS・N・RS を決めて、モデルを作ります");
        egui::CollapsingHeader::new(i18n::tr("考え方: IS・N・RS とは? (はじめての人はここから)")).id_salt("dio_howto").show(ui, |ui| {
            for t in [
                "ダイオードの電圧と電流の関係は、V = N × 26mV × ln(I / IS) + I × RS という式で近似できます。この 3 つの数 (IS・N・RS) を決めれば、実物に近い曲線になります。",
                "N (電流の 10 倍で電圧がどれだけ上がるか): 10 倍の電流で、電圧は約 N × 60mV 上がります。グラフで、10mA と 100mA の電圧の差を読むと分かります。1 〜 2 ほどです。",
                "IS (曲線の左右の位置): 小さいほど、曲線は右 (高い電圧) へ寄ります。流れ始める電圧を決めます。ショットキーは大きく (ナノアンペア〜)、シリコンは小さい (ピコアンペア) です。",
                "RS (大電流での曲がり): 大きい電流で、曲線が急に立ち上がる部分を決めます。定格電流に近い点を読むと決まります。",
                "データシートの表の Vf (たとえば 1.0V @ 1A) は「最大値」で、グラフは「典型値」です。モデルは典型値で作り、回路は最大値でも動くように設計します。",
                "ほかに、逆耐圧 (VRRM) と漏れ電流 (IR) も、表から写します。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        crate::icons::help_label(ui, "手順1: 順方向特性の点", "手順1: データシートの「順方向特性 (Forward Characteristics)」のグラフから、電流と電圧の点を読みます (1行に「電流 電圧」、電流が離れた 3 点がおすすめ)");
        ui.add(egui::TextEdit::multiline(&mut self.dio_points).desired_rows(3).desired_width(220.0).font(egui::TextStyle::Monospace).hint_text("10m 0.70\n100m 0.79\n1 0.92"))
            .on_hover_text(i18n::tr("グラフの縦軸 (電流) と横軸 (電圧) を読みます。電流は小・中・大の 3 点 (10mA・100mA・定格電流など)。m は 1000 分の 1。例: 100m 0.79 は 100mA のとき 0.79V"));
        ui.horizontal(|ui| {
            if self.sheet.has_doc()
                && ui.button(i18n::tr("PDF のグラフから読む")).on_hover_text(i18n::tr("データシートの窓で、順方向特性のグラフの軸の目盛りと曲線の上をクリックして、点を読みます。読んだ点は、点の窓のボタンでこの欄に入ります")).clicked()
            {
                self.sheet.start_graph(Some(super::graph_reader::Dest::DiodeForward));
            }
            if ui.button(i18n::tr("1N4007 の例を入れる")).on_hover_text(i18n::tr("練習用の読み取り例を入れます。自分のデータシートのグラフで、読み直してみてください")).clicked() {
                self.dio_points = EXAMPLE_POINTS.into();
                self.dio_vrrm = "1000".into();
                self.dio_ir = "5u".into();
                self.dio_message = Some(self.dio_fit());
            }
            if ui.button(i18n::tr("手順2: 点から IS・N・RS を求める")).on_hover_text(i18n::tr("点を通る曲線になるように、IS・N・RS を計算します。点が 2 つなら IS と N、3 つ以上なら RS も決まります")).clicked() {
                self.dio_message = Some(self.dio_fit());
            }
        });
        ui.label(i18n::tr("曲線を見ながら、スライダーで自分で合わせることもできます"));
        egui::Grid::new("dio_sliders").num_columns(2).show(ui, |ui| {
            ui.label(i18n::tr("IS (飽和電流)")).on_hover_text(i18n::tr("曲線の左右の位置。小さいほど右 (流れ始める電圧が高い) へ動きます"));
            ui.add(egui::Slider::new(&mut self.dio_is, 1e-15..=1e-5).logarithmic(true).custom_formatter(|v, _| format!("{}A", passive::eng(v))).custom_parser(|s| sim_core::parse_value(s.trim_end_matches('A'))));
            ui.end_row();
            ui.label(i18n::tr("N (理想係数)")).on_hover_text(i18n::tr("曲線の傾き。10 倍の電流で、電圧は約 N × 60mV 上がります"));
            ui.add(egui::Slider::new(&mut self.dio_n, 0.8..=3.0).fixed_decimals(2));
            ui.end_row();
            ui.label(i18n::tr("RS (直列抵抗)")).on_hover_text(i18n::tr("大電流での曲がり。大きいほど、電流が多い所で電圧が高くなります"));
            ui.add(egui::Slider::new(&mut self.dio_rs, 0.001..=2.0).logarithmic(true).custom_formatter(|v, _| format!("{}Ω", passive::eng(v))).custom_parser(|s| sim_core::parse_value(s.trim_end_matches('Ω'))));
            ui.end_row();
        });
        self.dio_chart(ui);
        ui.label(i18n::tr("手順3: 表から、逆耐圧と漏れ電流を写します (空でも作れます)"));
        egui::Grid::new("dio_rev").num_columns(2).show(ui, |ui| {
            ui.label(i18n::tr("逆耐圧 VRRM [V]")).on_hover_text(i18n::tr("「絶対最大定格」の Peak Repetitive Reverse Voltage。型番ごとに違うので、表で自分の型番の列を読みます (1N4007 は 1000)"));
            ui.add(egui::TextEdit::singleline(&mut self.dio_vrrm).desired_width(80.0).min_size(egui::vec2(80.0, 0.0)).hint_text("1000"));
            ui.end_row();
            ui.label(i18n::tr("漏れ電流 IR [A]")).on_hover_text(i18n::tr("逆耐圧をかけたときの逆向きの電流 (Peak Reverse Current、25℃ の値)。1N4007 は 5μA = 5u"));
            ui.add(egui::TextEdit::singleline(&mut self.dio_ir).desired_width(80.0).min_size(egui::vec2(80.0, 0.0)).hint_text("5u"));
            ui.end_row();
        });
        crate::icons::help_label(ui, "手順4: 高い周波数で効く値", "手順4: 高い周波数で効く値 (空でも作れます。スイッチング電源や整流の高速な切り替えで効きます)");
        egui::Grid::new("dio_charge").num_columns(2).show(ui, |ui| {
            ui.label(i18n::tr("接合容量 Cj [F]")).on_hover_text(i18n::tr("データシートの Typical Junction Capacitance。測った逆電圧とセットで読みます (1N4007 は 8pF 〜 15pF、1MHz、逆電圧 4V)"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.dio_cj).desired_width(70.0).min_size(egui::vec2(70.0, 0.0)).hint_text("15p"));
                ui.label(i18n::tr("逆電圧 [V]"));
                ui.add(egui::TextEdit::singleline(&mut self.dio_cj_vr).desired_width(50.0).min_size(egui::vec2(50.0, 0.0)).hint_text("4"));
            });
            ui.end_row();
            ui.label(i18n::tr("逆回復時間 trr [s]")).on_hover_text(i18n::tr("データシートの Reverse Recovery Time。電流を順方向から逆向きに切り替えたとき、逆向きの電流が止まるまでの時間。ショットキーには無い (ほぼ 0) ので、空で構いません"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.dio_trr).desired_width(70.0).min_size(egui::vec2(70.0, 0.0)).hint_text("2u"));
                ui.label("IF [A]");
                ui.add(egui::TextEdit::singleline(&mut self.dio_trr_if).desired_width(50.0).min_size(egui::vec2(50.0, 0.0)).hint_text("0.5"));
                ui.label("IR [A]");
                ui.add(egui::TextEdit::singleline(&mut self.dio_trr_ir).desired_width(50.0).min_size(egui::vec2(50.0, 0.0)).hint_text("1"));
            });
            ui.end_row();
        });
        ui.horizontal(|ui| {
            ui.label(i18n::tr("容量の点 (空でも可): 逆電圧 容量")).on_hover_text(i18n::tr("容量と逆電圧のグラフから読んだ点。1 行に「逆電圧 [V] 容量 [F]」。入れると、上の Cj 1 点の代わりに使い、電圧による変化も合わせます"));
            if self.sheet.has_doc() && ui.small_button(i18n::tr("グラフから読む")).on_hover_text(i18n::tr("データシートの窓で、容量のグラフの目盛りと曲線の上をクリックして点を読みます")).clicked() {
                self.sheet.start_graph(Some(super::graph_reader::Dest::DiodeCap));
            }
        });
        ui.add(egui::TextEdit::multiline(&mut self.dio_caps).desired_rows(2).desired_width(220.0).font(egui::TextStyle::Monospace).hint_text("1 15p\n10 8p"));
        ui.horizontal_wrapped(|ui| {
            if self.sheet.has_doc() && ui.button(i18n::tr("PDF の表から読む")).on_hover_text(i18n::tr("開いているデータシートの表から、逆電流 IR (とその VR を逆耐圧に)・接合容量・trr を探して欄に入れます。入れた値は表と見比べて確かめてください")).clicked() {
                self.dio_fill_from_pdf();
            }
            if ui.button(i18n::tr("ずれの表を見る (同梱と同じ作り方)")).on_hover_text(i18n::tr("同梱のダイオードと同じ当てはめで、モデルと、データシートの各値とのずれを並べます。モデルの形の限界の注意も出ます")).clicked() {
                self.dio_fit_table();
            }
        });
        for (ok, line) in &self.dio_found {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::WARN }, egui::RichText::new(line).small());
        }
        match &self.dio_result {
            Some(Ok(f)) => {
                ui.monospace(&f.text);
                super::mosfet::fit_table(ui, "dio_rows", f);
            }
            Some(Err(e)) => {
                ui.colored_label(crate::theme::ERROR, i18n::tr_dyn(e));
            }
            None => {}
        }
        if ui.button(i18n::tr("手順5: このモデルを「② シミュレーション」に入れる")).on_hover_text(i18n::tr("モデルの文 (.model) を作って、② のシミュレーションの欄に入れます。ずれの表を作ったときは、そのモデル (注記つき) を入れます。逆耐圧は ① の定格の最大電圧にも入ります")).clicked() {
            self.dio_message = Some(self.dio_apply());
        }
        match &self.dio_message {
            Some(Ok(m)) => {
                ui.colored_label(crate::theme::OK, i18n::tr_dyn(m));
            }
            Some(Err(e)) => {
                ui.colored_label(crate::theme::ERROR, i18n::tr_dyn(e));
            }
            None => {}
        }
    }

    /// Voltage against current (log scale) of the typed points and the model.
    fn dio_chart(&self, ui: &mut egui::Ui) {
        let pts = self.dio_points().unwrap_or_default();
        let (is, n, rs) = (self.dio_is, self.dio_n, self.dio_rs);
        let (imin, imax) = pts.iter().fold((f64::MAX, 0.0f64), |(a, b), (i, _)| (a.min(*i), b.max(*i)));
        let (imin, imax) = if pts.is_empty() { (1e-3, 1.0) } else { (imin, imax) };
        let (d_lo, d_hi) = ((imin.log10().floor() - 1.0).max(-9.0), (imax.log10().ceil() + 1.0).min(3.0));
        let vlo = ((diode::vf(is, n, rs, 10f64.powf(d_lo)).min(pts.iter().map(|p| p.1).fold(f64::MAX, f64::min)) - 0.05) * 10.0).floor() / 10.0;
        let vhi = ((diode::vf(is, n, rs, 10f64.powf(d_hi)).max(pts.iter().map(|p| p.1).fold(0.0, f64::max)) + 0.05) * 10.0).ceil() / 10.0;
        let (vlo, vhi) = (vlo.max(0.0), vhi.max(vlo + 0.4));
        let size = Vec2::new(ui.available_width().min(520.0), 200.0);
        let (resp, painter) = ui.allocate_painter(size, Sense::hover());
        let plot = Rect::from_min_max(resp.rect.min + Vec2::new(52.0, 8.0), resp.rect.max - Vec2::new(8.0, 24.0));
        let vis = ui.visuals();
        painter.rect_filled(resp.rect, 2.0, vis.extreme_bg_color);
        let (grid, text) = (vis.widgets.noninteractive.bg_stroke.color, vis.text_color());
        let to = |v: f64, i: f64| Pos2::new(plot.left() + (((v - vlo) / (vhi - vlo)) as f32) * plot.width(), plot.bottom() - (((i.log10() - d_lo) / (d_hi - d_lo)) as f32) * plot.height());
        let font = FontId::proportional(13.5);
        let mut d = d_lo;
        while d <= d_hi + 1e-9 {
            let y = to(vlo, 10f64.powf(d)).y;
            painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, grid));
            painter.text(Pos2::new(plot.left() - 4.0, y), Align2::RIGHT_CENTER, format!("{}A", passive::eng(10f64.powf(d))), font.clone(), text);
            d += 1.0;
        }
        let mut v = (vlo * 5.0).ceil() / 5.0;
        while v <= vhi + 1e-9 {
            let x = to(v, 1.0).x;
            painter.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0, grid));
            if x < plot.right() - 16.0 {
                painter.text(Pos2::new(x, plot.bottom() + 3.0), Align2::CENTER_TOP, format!("{v:.1}V"), font.clone(), text);
            }
            v += 0.2;
        }
        let curve: Vec<Pos2> = (0..=80).map(|k| {
            let i = 10f64.powf(d_lo + (d_hi - d_lo) * k as f64 / 80.0);
            to(diode::vf(is, n, rs, i), i)
        }).filter(|p| plot.expand(2.0).contains(*p)).collect();
        painter.add(egui::Shape::line(curve, Stroke::new(2.0, Color32::from_rgb(70, 140, 230))));
        for (i, v) in &pts {
            painter.circle_filled(to(*v, *i), 4.0, crate::theme::ERROR);
        }
        ui.label(egui::RichText::new(i18n::tr("● 赤い点: データシートの点    ─ 青い線: モデルの曲線")).size(13.5));
        if !pts.is_empty() {
            let rows: Vec<String> = pts.iter().map(|(i, v)| {
                let m = diode::vf(is, n, rs, *i);
                let side = if m >= *v { i18n::tr("高い") } else { i18n::tr("低い") };
                i18n::trf!("{}A: データ {v:.2}V / モデル {m:.2}V (モデルが {:.0}mV {})", passive::eng(*i), ((m - v) * 1000.0).abs(), side)
            }).collect();
            ui.label(egui::RichText::new(rows.join("\n")).size(13.5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gaps_table_fits_like_the_bundled_diodes_and_asks_for_the_reverse_ratings() {
        let mut f = PartForm { name: "1N4007".into(), dio_points: EXAMPLE_POINTS.into(), ..Default::default() };
        f.dio_fit_table();
        assert!(matches!(&f.dio_result, Some(Err(e)) if e.contains("逆耐圧")), "{:?}", f.dio_result.as_ref().map(|r| r.as_ref().err()));
        (f.dio_vrrm, f.dio_ir, f.dio_caps) = ("1000".into(), "5u".into(), "1 15p
10 8p
100 3.5p".into());
        f.dio_fit_table();
        let Some(Ok(r)) = &f.dio_result else { panic!("{:?}", f.dio_result.as_ref().map(|r| r.as_ref().err())) };
        assert!(r.text.starts_with(".model 1N4007 D(") && r.text.contains("BV=1000") && r.text.contains("CJO="), "{}", r.text);
        assert_eq!(r.rows.iter().filter(|x| x.item == "VF").count(), 3);
        assert_eq!(r.rows.iter().filter(|x| x.item == "CJ").count(), 3);
        // the sliders follow the fit
        assert!((f.dio_n - 1.0).abs() < 2.0 && f.dio_is > 0.0);
    }
}
