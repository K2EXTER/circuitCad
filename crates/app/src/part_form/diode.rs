//! "かんたん作成" for a rectifier / switching / Schottky diode: the learner reads a few points off the data
//! sheet's forward characteristics, the form fits IS, N and RS, shows the curve against the points, and
//! writes the `.model` card. The explanation is part of the section, so it doubles as a short tutorial.

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

    /// Write the model into the simulation tab (and the maximum voltage into the ratings).
    pub(super) fn dio_apply(&mut self) -> Result<String, String> {
        if self.name.trim().is_empty() {
            return Err("先に「名前など」の部品名を入れてください (モデルの名前に使います。例: 1N4007)".into());
        }
        let pins = self.parse_pins()?;
        let find = |n: &str| pins.iter().find(|p| p.name.eq_ignore_ascii_case(n)).map(|p| p.number.clone());
        let (Some(a), Some(k)) = (find("A"), find("K")) else {
            return Err("ピン表に、アノード A とカソード K の名前のピンが要ります (「② ピン」で、名前を A と K にしてください)".into());
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
        if self.sim_limits.trim().is_empty() {
            self.sim_limits = "IS・N・RS は、データシートのグラフから読み取った点に合わせた近似です。個体差と温度で、実物は変わります。\n接合容量・逆回復時間は、入れた数値だけの近似です (入れていなければ扱いません)。\n耐圧や定格電流を超えても壊れません。データシートの定格は、自分で確かめてください。".into();
        }
        if let (Some(v), true) = (vrrm, self.rt_v.trim().is_empty()) {
            self.rt_v = passive::eng(v);
            notes.push("逆耐圧を「⑤ 定格」の最大電圧にも入れました");
        }
        let tail = if notes.is_empty() { String::new() } else { format!("。{}", notes.join("。")) };
        Ok(format!("モデルを「④ シミュレーション」に入れました。そちらの「試し読み」で確かめられます{tail}"))
    }

    pub(super) fn diode_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("かんたん作成: ダイオードをデータシートの数値からつくる").strong());
        ui.small("メーカーのデータシートには SPICE モデルが載っていないことが多いので、グラフの数字から IS・N・RS を決めて、モデルを作ります");
        egui::CollapsingHeader::new("考え方: IS・N・RS とは? (はじめての人はここから)").id_salt("dio_howto").show(ui, |ui| {
            for t in [
                "ダイオードの電圧と電流の関係は、V = N × 26mV × ln(I / IS) + I × RS という式で近似できます。この 3 つの数 (IS・N・RS) を決めれば、実物に近い曲線になります。",
                "N (電流の 10 倍で電圧がどれだけ上がるか): 10 倍の電流で、電圧は約 N × 60mV 上がります。グラフで、10mA と 100mA の電圧の差を読むと分かります。1 〜 2 ほどです。",
                "IS (曲線の左右の位置): 小さいほど、曲線は右 (高い電圧) へ寄ります。流れ始める電圧を決めます。ショットキーは大きく (ナノアンペア〜)、シリコンは小さい (ピコアンペア) です。",
                "RS (大電流での曲がり): 大きい電流で、曲線が急に立ち上がる部分を決めます。定格電流に近い点を読むと決まります。",
                "データシートの表の Vf (たとえば 1.0V @ 1A) は「最大値」で、グラフは「典型値」です。モデルは典型値で作り、回路は最大値でも動くように設計します。",
                "ほかに、逆耐圧 (VRRM) と漏れ電流 (IR) も、表から写します。",
            ] {
                ui.label(egui::RichText::new(format!("・{t}")).small());
            }
        });
        ui.label("① データシートの「順方向特性 (Forward Characteristics)」のグラフから、電流と電圧の点を読みます (1行に「電流 電圧」、電流が離れた 3 点がおすすめ)");
        ui.add(egui::TextEdit::multiline(&mut self.dio_points).desired_rows(3).desired_width(220.0).font(egui::TextStyle::Monospace).hint_text("10m 0.70\n100m 0.79\n1 0.92"))
            .on_hover_text("グラフの縦軸 (電流) と横軸 (電圧) を読みます。電流は小・中・大の 3 点 (10mA・100mA・定格電流など)。m は 1000 分の 1。例: 100m 0.79 は 100mA のとき 0.79V");
        ui.horizontal(|ui| {
            if ui.button("1N4007 の例を入れる").on_hover_text("練習用の読み取り例を入れます。自分のデータシートのグラフで、読み直してみてください").clicked() {
                self.dio_points = EXAMPLE_POINTS.into();
                self.dio_vrrm = "1000".into();
                self.dio_ir = "5u".into();
                self.dio_message = Some(self.dio_fit());
            }
            if ui.button("② 点から IS・N・RS を求める").on_hover_text("点を通る曲線になるように、IS・N・RS を計算します。点が 2 つなら IS と N、3 つ以上なら RS も決まります").clicked() {
                self.dio_message = Some(self.dio_fit());
            }
        });
        ui.label("曲線を見ながら、スライダーで自分で合わせることもできます");
        egui::Grid::new("dio_sliders").num_columns(2).show(ui, |ui| {
            ui.label("IS (飽和電流)").on_hover_text("曲線の左右の位置。小さいほど右 (流れ始める電圧が高い) へ動きます");
            ui.add(egui::Slider::new(&mut self.dio_is, 1e-15..=1e-5).logarithmic(true).custom_formatter(|v, _| format!("{}A", passive::eng(v))).custom_parser(|s| sim_core::parse_value(s.trim_end_matches('A'))));
            ui.end_row();
            ui.label("N (理想係数)").on_hover_text("曲線の傾き。10 倍の電流で、電圧は約 N × 60mV 上がります");
            ui.add(egui::Slider::new(&mut self.dio_n, 0.8..=3.0).fixed_decimals(2));
            ui.end_row();
            ui.label("RS (直列抵抗)").on_hover_text("大電流での曲がり。大きいほど、電流が多い所で電圧が高くなります");
            ui.add(egui::Slider::new(&mut self.dio_rs, 0.001..=2.0).logarithmic(true).custom_formatter(|v, _| format!("{}Ω", passive::eng(v))).custom_parser(|s| sim_core::parse_value(s.trim_end_matches('Ω'))));
            ui.end_row();
        });
        self.dio_chart(ui);
        ui.label("③ 表から、逆耐圧と漏れ電流を写します (空でも作れます)");
        egui::Grid::new("dio_rev").num_columns(2).show(ui, |ui| {
            ui.label("逆耐圧 VRRM [V]").on_hover_text("「絶対最大定格」の Peak Repetitive Reverse Voltage。型番ごとに違うので、表で自分の型番の列を読みます (1N4007 は 1000)");
            ui.add(egui::TextEdit::singleline(&mut self.dio_vrrm).desired_width(80.0).hint_text("1000"));
            ui.end_row();
            ui.label("漏れ電流 IR [A]").on_hover_text("逆耐圧をかけたときの逆向きの電流 (Peak Reverse Current、25℃ の値)。1N4007 は 5μA = 5u");
            ui.add(egui::TextEdit::singleline(&mut self.dio_ir).desired_width(80.0).hint_text("5u"));
            ui.end_row();
        });
        ui.label("④ 高い周波数で効く値 (空でも作れます。スイッチング電源や整流の高速な切り替えで効きます)");
        egui::Grid::new("dio_charge").num_columns(2).show(ui, |ui| {
            ui.label("接合容量 Cj [F]").on_hover_text("データシートの Typical Junction Capacitance。測った逆電圧とセットで読みます (1N4007 は 8pF 〜 15pF、1MHz、逆電圧 4V)");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.dio_cj).desired_width(70.0).hint_text("15p"));
                ui.label("逆電圧 [V]");
                ui.add(egui::TextEdit::singleline(&mut self.dio_cj_vr).desired_width(50.0).hint_text("4"));
            });
            ui.end_row();
            ui.label("逆回復時間 trr [s]").on_hover_text("データシートの Reverse Recovery Time。電流を順方向から逆向きに切り替えたとき、逆向きの電流が止まるまでの時間。ショットキーには無い (ほぼ 0) ので、空で構いません");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.dio_trr).desired_width(70.0).hint_text("2u"));
                ui.label("IF [A]");
                ui.add(egui::TextEdit::singleline(&mut self.dio_trr_if).desired_width(50.0).hint_text("0.5"));
                ui.label("IR [A]");
                ui.add(egui::TextEdit::singleline(&mut self.dio_trr_ir).desired_width(50.0).hint_text("1"));
            });
            ui.end_row();
        });
        if ui.button("⑤ このモデルを「④ シミュレーション」に入れる").on_hover_text("モデルの文 (.model) を作って、④ のシミュレーションの欄に入れます。逆耐圧は ⑤ の最大電圧にも入ります").clicked() {
            self.dio_message = Some(self.dio_apply());
        }
        match &self.dio_message {
            Some(Ok(m)) => {
                ui.colored_label(Color32::from_rgb(60, 170, 90), m);
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
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
            painter.circle_filled(to(*v, *i), 4.0, Color32::from_rgb(230, 90, 60));
        }
        ui.label(egui::RichText::new("● 赤い点: データシートの点    ─ 青い線: モデルの曲線").size(13.5));
        if !pts.is_empty() {
            let rows: Vec<String> = pts.iter().map(|(i, v)| {
                let m = diode::vf(is, n, rs, *i);
                format!("{}A: データ {v:.2}V / モデル {m:.2}V (モデルが {:.0}mV {})", passive::eng(*i), ((m - v) * 1000.0).abs(), if m >= *v { "高い" } else { "低い" })
            }).collect();
            ui.label(egui::RichText::new(rows.join("\n")).size(13.5));
        }
    }
}
