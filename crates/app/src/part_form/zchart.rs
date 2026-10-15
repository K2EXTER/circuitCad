//! Chip part section, part 2: the impedance table drawn against the model curve (with sliders to tune the
//! two parasitics by eye), and the temperature characteristic. The same code serves the capacitor (ESR, ESL),
//! the inductor (DCR, parallel capacitance) and the resistor (series inductance, parallel capacitance).

use super::*;

impl PartForm {
    /// The text boxes of the pair of parasitics of the current kind: (first, second).
    pub(super) fn pas_pair(&mut self) -> (&mut String, &mut String) {
        match self.pas_kind {
            PassiveKind::Capacitor => (&mut self.pas_esr, &mut self.pas_esl),
            PassiveKind::Inductor => (&mut self.pas_dcr, &mut self.pas_cp),
            PassiveKind::Resistor => (&mut self.pas_esl, &mut self.pas_cp),
        }
    }

    /// Names, units and tips of the pair: ((name, unit), (name, unit)).
    fn pas_pair_names(&self) -> ((&'static str, &'static str), (&'static str, &'static str)) {
        match self.pas_kind {
            PassiveKind::Capacitor => (("ESR", "Ω"), ("ESL", "H")),
            PassiveKind::Inductor => (("DCR", "Ω"), ("並列容量", "F")),
            PassiveKind::Resistor => (("寄生インダクタンス", "H"), ("並列容量", "F")),
        }
    }

    /// Points typed in the impedance table: (Hz, ohm).
    pub(super) fn pas_table_points(&self) -> Result<Vec<(f64, f64)>, String> {
        let mut pts = vec![];
        for (n, line) in self.pas_table.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',' || c == '|').filter(|s| !s.is_empty()).collect();
            match (t.first().and_then(|s| parse_freq(s)), t.get(1).and_then(|s| parse_ohm(s))) {
                (Some(f), Some(z)) => pts.push((f, z)),
                _ => return Err(format!("インピーダンスの表 {}行目: 「周波数 インピーダンス」の形で入力してください (例: 1M 0.05)", n + 1)),
            }
        }
        Ok(pts)
    }

    /// A temperature coefficient typed in ppm (`100`, `-30`, `100ppm`) as a fraction; None when blank.
    pub(super) fn pas_ppm(&self, label: &str, t: &str) -> Result<Option<f64>, String> {
        let l = t.trim().to_ascii_lowercase();
        let l = l.trim_end_matches("ppm/℃").trim_end_matches("ppm").trim();
        if l.is_empty() {
            return Ok(None);
        }
        sim_core::parse_value(l).map(|v| Some(v * 1e-6)).ok_or_else(|| format!("{label}「{t}」を ppm の数で入力してください (例: 100 / -30)"))
    }

    /// Points of the temperature table: (°C, percent).
    pub(super) fn pas_tc_points(&self) -> Result<Vec<(f64, f64)>, String> {
        let mut pts = vec![];
        for (n, line) in self.pas_tctable.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',' || c == '|').filter(|s| !s.is_empty()).collect();
            let num = |s: &str| sim_core::parse_value(s.trim_end_matches(['%', '℃', 'C', 'c']));
            match (t.first().and_then(|s| num(s)), t.get(1).and_then(|s| num(s))) {
                (Some(a), Some(b)) => pts.push((a, b)),
                _ => return Err(format!("温度の表 {}行目: 「温度 変化率(%)」の形で入力してください (例: 85 -7)", n + 1)),
            }
        }
        Ok(pts)
    }

    /// Fit the pair of parasitics to the impedance table typed in the form.
    pub(super) fn fit_from_table(&mut self) -> Result<String, String> {
        let kind = self.pas_kind;
        let what = match kind {
            PassiveKind::Capacitor => "容量",
            PassiveKind::Inductor => "インダクタンス",
            PassiveKind::Resistor => "抵抗値",
        };
        let v = self.pas_number(what, &self.pas_value, false)?.ok_or_else(|| format!("先に上の「値」に{what}を入れてください (表の値。DCバイアス時の表ならそのときの値)"))?;
        let pts = self.pas_table_points()?;
        let (a, b, srf, err, warnings) = if kind == PassiveKind::Capacitor {
            let f = passive::fit_cap(v, &pts)?;
            (f.esr, f.esl, f.srf, f.max_error_pct, f.warnings)
        } else {
            let f = passive::fit_passive(kind, v, &pts)?;
            (f.a, f.b, f.srf, f.max_error_pct, f.warnings)
        };
        let ((na, ua), (nb, ub)) = self.pas_pair_names();
        let (sa, sb) = (passive::eng(a), passive::eng(b));
        let (pa, pb) = self.pas_pair();
        (*pa, *pb) = (sa.clone(), sb.clone());
        let mut m = format!("{na} = {sa}{ua}、{nb} = {sb}{ub} と求めました (共振 {}Hz、表との差は最大 {err:.0}%)。下の絵で、点と曲線が重なっているか確かめてください。「部品を作る」を押すとモデルに入ります", passive::eng(srf));
        for w in &warnings {
            m.push_str(&format!("\n注意: {w}"));
        }
        Ok(m)
    }

    /// Fit TC1 / TC2 to the temperature table typed in the form.
    pub(super) fn fit_tc_from_table(&mut self) -> Result<String, String> {
        let pts = self.pas_tc_points()?;
        let (tc1, tc2, err) = passive::fit_tempco(&pts)?;
        self.pas_tc1 = format!("{:.1}", tc1 * 1e6);
        self.pas_tc2 = if tc2 == 0.0 { String::new() } else { format!("{:.3}", tc2 * 1e6) };
        let mut m = format!("温度係数 TC1 = {}ppm/℃、TC2 = {}ppm/℃² と求めました (表との差は最大 {err:.1}ポイント)", self.pas_tc1, if self.pas_tc2.is_empty() { "0".into() } else { self.pas_tc2.clone() });
        for (t, p) in &pts {
            let model = (passive::tempco_ratio(tc1, tc2, *t) - 1.0) * 100.0;
            m.push_str(&format!("\n{t:.0}℃: 表 {p:.1}% / モデル {model:.1}%"));
        }
        if err > 3.0 {
            m.push_str("\n注意: 2 次式では、表の曲線の形をうまく写せません。実物の曲線は、温度によっては大きく曲がります。使う温度の範囲の点だけで求め直すと、近くなります");
        }
        Ok(m)
    }

    /// A slider bound to a text box that holds an engineering-notation number.
    fn text_slider(ui: &mut egui::Ui, label: &str, tip: &str, text: &mut String, log_range: (f64, f64), unit: &str, fallback: f64) {
        let mut v = sim_core::parse_value(text.trim()).filter(|v| *v > 0.0).unwrap_or(fallback);
        ui.label(label).on_hover_text(tip);
        let unit = unit.to_string();
        let r = ui.add(
            egui::Slider::new(&mut v, 10f64.powf(log_range.0)..=10f64.powf(log_range.1))
                .logarithmic(true)
                .custom_formatter(move |x, _| format!("{}{unit}", passive::eng(x)))
                .custom_parser(|s| sim_core::parse_value(s.trim_end_matches(['Ω', 'H', 'F']))),
        );
        if r.changed() {
            *text = passive::eng(v);
        }
        ui.end_row();
    }

    /// The impedance table (log-log), the model curve and the sliders that move it.
    pub(super) fn pas_chart(&mut self, ui: &mut egui::Ui) {
        let kind = self.pas_kind;
        let ((la, ha), (lb, hb)) = passive::fit_ranges(kind);
        let fallback = (10f64.powf((la + ha) / 2.0 - 1.0), 10f64.powf((lb + hb) / 2.0 - 1.0));
        let Some(value) = self.pas_number("値", &self.pas_value, false).ok().flatten() else {
            ui.small("上の「値」を入れると、表の点とモデルの曲線を重ねた絵が出ます");
            self.pas_sliders(ui);
            return;
        };
        let pts = self.pas_table_points().unwrap_or_default();
        let (a, b) = {
            let (pa, pb) = self.pas_pair();
            (sim_core::parse_value(pa.trim()).filter(|v| *v > 0.0).unwrap_or(fallback.0), sim_core::parse_value(pb.trim()).filter(|v| *v > 0.0).unwrap_or(fallback.1))
        };
        let (fmin, fmax) = pts.iter().fold((f64::MAX, 0.0f64), |(x, y), p| (x.min(p.0), y.max(p.0)));
        let (fmin, fmax) = if pts.is_empty() { (1e4, 1e9) } else { (fmin, fmax) };
        let (d_lo, d_hi) = ((fmin.log10().floor() - 0.0).max(0.0), (fmax.log10().ceil() + 0.0).max(fmin.log10().floor() + 2.0).min(11.0));
        let zs: Vec<f64> = (0..=240).map(|k| passive::impedance(kind, value, a, b, 10f64.powf(d_lo + (d_hi - d_lo) * k as f64 / 240.0))).collect();
        let (zmin, zmax) = zs.iter().chain(pts.iter().map(|p| &p.1)).fold((f64::MAX, 0.0f64), |(x, y), z| (x.min(*z), y.max(*z)));
        let (z_lo, z_hi) = (zmin.log10().floor(), (zmax.log10().ceil()).max(zmin.log10().floor() + 1.0));
        let size = Vec2::new(ui.available_width().min(520.0), 190.0);
        let (resp, painter) = ui.allocate_painter(size, Sense::hover());
        let plot = Rect::from_min_max(resp.rect.min + Vec2::new(62.0, 8.0), resp.rect.max - Vec2::new(14.0, 24.0));
        let vis = ui.visuals();
        painter.rect_filled(resp.rect, 2.0, vis.extreme_bg_color);
        let (grid, text) = (vis.widgets.noninteractive.bg_stroke.color, vis.text_color());
        let to = |f: f64, z: f64| Pos2::new(plot.left() + (((f.log10() - d_lo) / (d_hi - d_lo)) as f32) * plot.width(), plot.bottom() - (((z.log10() - z_lo) / (z_hi - z_lo)) as f32) * plot.height());
        let font = FontId::proportional(13.5);
        let mut d = z_lo;
        while d <= z_hi + 1e-9 {
            let y = to(1.0, 10f64.powf(d)).y;
            painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, grid));
            painter.text(Pos2::new(plot.left() - 4.0, y), Align2::RIGHT_CENTER, format!("{}Ω", passive::eng(10f64.powf(d))), font.clone(), text);
            d += 1.0;
        }
        let mut d = d_lo;
        while d <= d_hi + 1e-9 {
            let x = to(10f64.powf(d), 1.0).x;
            painter.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0, grid));
            let label = format!("{}Hz", passive::eng(10f64.powf(d)).replace("Meg", "M"));
            let (pos, align) = if d <= d_lo + 1e-9 {
                (Pos2::new(x + 2.0, plot.bottom() + 3.0), Align2::LEFT_TOP)
            } else if d >= d_hi - 1e-9 {
                (Pos2::new(x, plot.bottom() + 3.0), Align2::RIGHT_TOP)
            } else {
                (Pos2::new(x, plot.bottom() + 3.0), Align2::CENTER_TOP)
            };
            painter.text(pos, align, label, font.clone(), text);
            d += 1.0;
        }
        let curve: Vec<Pos2> = zs.iter().enumerate().map(|(k, z)| to(10f64.powf(d_lo + (d_hi - d_lo) * k as f64 / 240.0), *z)).collect();
        painter.add(egui::Shape::line(curve, Stroke::new(2.0, Color32::from_rgb(70, 140, 230))));
        for (f, z) in &pts {
            painter.circle_filled(to(*f, *z), 4.0, Color32::from_rgb(230, 90, 60));
        }
        ui.label(egui::RichText::new("● 赤い点: データシートの点    ─ 青い線: モデルの曲線").size(13.5));
        self.pas_sliders(ui);
    }

    /// Sliders for the pair of parasitics (they edit the text boxes above).
    fn pas_sliders(&mut self, ui: &mut egui::Ui) {
        let kind = self.pas_kind;
        let ((na, ua), (nb, ub)) = self.pas_pair_names();
        let ((la, ha), (lb, hb)) = passive::fit_ranges(kind);
        let fallback = (10f64.powf((la + ha) / 2.0 - 1.0), 10f64.powf((lb + hb) / 2.0 - 1.0));
        let (tip_a, tip_b) = match kind {
            PassiveKind::Capacitor => ("谷 (一番低い所) の深さ。小さいほど谷が深くなります", "谷より高い周波数での立ち上がり。大きいほど、共振が低い周波数になります"),
            PassiveKind::Inductor => ("低い周波数でのインピーダンスの下限。大きいほど、低い所が持ち上がります", "山 (共振) の位置と高さ。大きいほど、共振が低い周波数になります"),
            PassiveKind::Resistor => ("高い周波数でインピーダンスが上がる量。大きいほど、早く上がり始めます", "高い周波数でインピーダンスが下がる量。大きいほど、早く下がり始めます"),
        };
        ui.label("曲線を見ながら、スライダーで自分で合わせることもできます");
        egui::Grid::new("pas_sliders").num_columns(2).show(ui, |ui| {
            let (pa, pb) = self.pas_pair();
            Self::text_slider(ui, na, tip_a, pa, (la, ha), ua, fallback.0);
            Self::text_slider(ui, nb, tip_b, pb, (lb, hb), ub, fallback.1);
        });
    }

    /// The temperature part of the section.
    pub(super) fn pas_temperature(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(egui::RichText::new("温度による値の変化 (温度スイープで確かめられます)").strong());
        let kind = self.pas_kind;
        ui.small(match kind {
            PassiveKind::Resistor => "データシートの TCR (温度係数、ppm/℃) を入れます。金属皮膜抵抗は ±100 ほど、厚膜チップ抵抗は ±100〜200 ほどです。空なら温度で変わりません",
            PassiveKind::Capacitor => "温度特性のグラフ (縦軸が容量の変化率 %) から、温度と変化率を読んで入れます。C0G は ±30ppm/℃ ほどでほぼ一定、X7R・X5R は温度で数 % 〜 十数 % 動きます。空なら温度で変わりません",
            PassiveKind::Inductor => "インダクタンスの温度係数 (ppm/℃)。DCR は銅の係数 (1℃ で約 0.4%) が自動で入ります。空ならインダクタンスは温度で変わりません",
        });
        ui.horizontal(|ui| {
            ui.label("TC1 (ppm/℃)").on_hover_text("1℃ あたりの変化 (ppm = 100万分の 1)。+100 なら、温度が 100℃ 上がると 1% 増えます。マイナスもあります");
            ui.add(egui::TextEdit::singleline(&mut self.pas_tc1).desired_width(70.0).hint_text("100"));
            ui.label("TC2 (ppm/℃²)").on_hover_text("温度の 2 乗にかかる係数。曲がりを表すとき以外は空で構いません");
            ui.add(egui::TextEdit::singleline(&mut self.pas_tc2).desired_width(70.0));
        });
        if kind != PassiveKind::Resistor || !self.pas_tctable.trim().is_empty() {
            ui.label("温度特性の表 (グラフから読んだ「温度 変化率(%)」を 1 行ずつ。27℃ を 0% とします)");
            ui.add(egui::TextEdit::multiline(&mut self.pas_tctable).desired_rows(3).desired_width(220.0).font(egui::TextStyle::Monospace).hint_text("-55 -3\n85 -4\n125 -9"))
                .on_hover_text("例: 125 -9 は、125℃ で容量が 9% 減る、という意味です。27℃ に近い点は要りません。点が 1 つなら直線、2 つ以上なら曲がりも合わせます");
            if ui.button("表から温度係数を求める").on_hover_text("表に合うように、TC1 と TC2 を計算して上の欄に入れます").clicked() {
                self.pas_message = Some(self.fit_tc_from_table());
            }
        }
    }

    /// Points of the L-versus-current table: (A, percent).
    pub(super) fn pas_sat_points(&self) -> Result<Vec<(f64, f64)>, String> {
        let mut pts = vec![];
        for (n, line) in self.pas_sattable.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',' || c == '|').filter(|s| !s.is_empty()).collect();
            let num = |s: &str| sim_core::parse_value(s.trim_end_matches(['%', 'A', 'a']));
            match (t.first().and_then(|s| num(s)), t.get(1).and_then(|s| num(s))) {
                (Some(i), Some(p)) => pts.push((i, p)),
                _ => return Err(format!("飽和の表 {}行目: 「電流 変化率(%)」の形で入力してください (例: 3 -30)", n + 1)),
            }
        }
        Ok(pts)
    }

    /// Fit Isat (30 % drop) to the L-versus-current table typed in the form.
    pub(super) fn fit_sat_from_table(&mut self) -> Result<String, String> {
        let pts = self.pas_sat_points()?;
        let (isat, i0, err) = passive::fit_saturation(&pts)?;
        self.pas_isat = passive::eng(isat);
        self.pas_ldrop = "30".into();
        let mut m = format!("飽和電流 Isat = {}A (L が 30% 下がる電流) と求めました (表との差は最大 {err:.1}ポイント)", self.pas_isat);
        for (i, p) in &pts {
            let model = (passive::inductance_ratio(i0, *i) - 1.0) * 100.0;
            m.push_str(&format!("\n{}A: 表 {p:.0}% / モデル {model:.0}%", passive::eng(*i)));
        }
        if err > 8.0 {
            m.push_str("\n注意: このモデルの曲線の形と、表の形が合っていません。使う電流の範囲 (Isat より少し大きいところまで) の点だけで、求め直してみてください");
        }
        Ok(m)
    }

    /// The saturation part of the inductor section: Isat, the table, and the inductance against current.
    pub(super) fn pas_saturation(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(egui::RichText::new("磁気飽和 (電流が大きいとインダクタンスが下がる)").strong());
        ui.small("電源用のコイルでは大事な特性です。電流が Isat を超えると、インダクタンスが急に下がり、電流が急に増えます。データシートの Isat (インダクタンスが 30% 下がる電流) か、「インダクタンス - 直流重畳電流」のグラフから読みます。空なら飽和しません");
        ui.horizontal(|ui| {
            ui.label("Isat [A]").on_hover_text("インダクタンスが規定の割合だけ下がる電流 (Saturation Current)。データシートでは 30% 低下が多いです");
            ui.add(egui::TextEdit::singleline(&mut self.pas_isat).desired_width(70.0).hint_text("3"));
            ui.label("そのときの低下 [%]").on_hover_text("Isat で、インダクタンスが何 % 下がるか。30 が多いですが、20 や 10 のデータシートもあります。空なら 30");
            ui.add(egui::TextEdit::singleline(&mut self.pas_ldrop).desired_width(50.0).hint_text("30"));
        });
        ui.label("インダクタンスと電流の表 (グラフから読んだ「電流 変化率(%)」を 1 行ずつ。マイナスで下がる)");
        ui.add(egui::TextEdit::multiline(&mut self.pas_sattable).desired_rows(3).desired_width(220.0).font(egui::TextStyle::Monospace).hint_text("1 -5\n3 -30\n6 -60"))
            .on_hover_text("例: 3 -30 は、3A で L が 30% 下がる、という意味です。Isat より少し大きい電流までの点があると確かです");
        if ui.button("表から Isat を求める").on_hover_text("表に合うように、Isat (30% 低下) を計算して上の欄に入れます").clicked() {
            self.pas_message = Some(self.fit_sat_from_table());
        }
        self.sat_chart(ui);
    }

    /// Inductance (percent of the small-current value) against current: the model and the typed points.
    fn sat_chart(&self, ui: &mut egui::Ui) {
        let pts = self.pas_sat_points().unwrap_or_default();
        let isat = sim_core::parse_value(self.pas_isat.trim()).filter(|v| *v > 0.0);
        let drop = self.pas_number("", self.pas_ldrop.trim().trim_end_matches('%'), false).ok().flatten().map_or(0.3, |d| d / 100.0);
        if pts.is_empty() && isat.is_none() {
            return;
        }
        let i0 = isat.filter(|_| (0.01..=0.9).contains(&drop)).map(|i| passive::saturation_i0(i, drop));
        let imax = pts.iter().map(|p| p.0).chain(isat.map(|i| i * 1.6)).fold(0.0, f64::max).max(1e-3);
        let size = Vec2::new(ui.available_width().min(520.0), 170.0);
        let (resp, painter) = ui.allocate_painter(size, Sense::hover());
        let plot = Rect::from_min_max(resp.rect.min + Vec2::new(46.0, 8.0), resp.rect.max - Vec2::new(14.0, 24.0));
        let vis = ui.visuals();
        painter.rect_filled(resp.rect, 2.0, vis.extreme_bg_color);
        let (grid, text) = (vis.widgets.noninteractive.bg_stroke.color, vis.text_color());
        let to = |i: f64, pct: f64| Pos2::new(plot.left() + ((i / imax) as f32) * plot.width(), plot.bottom() - ((pct / 100.0) as f32).clamp(0.0, 1.0) * plot.height());
        let font = FontId::proportional(13.5);
        for pct in [0.0, 25.0, 50.0, 75.0, 100.0] {
            let y = to(0.0, pct).y;
            painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, grid));
            painter.text(Pos2::new(plot.left() - 4.0, y), Align2::RIGHT_CENTER, format!("{pct:.0}%"), font.clone(), text);
        }
        for k in 0..=4 {
            let i = imax * k as f64 / 4.0;
            let x = to(i, 0.0).x;
            painter.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0, grid));
            let (pos, align) = match k {
                0 => (Pos2::new(x + 2.0, plot.bottom() + 3.0), Align2::LEFT_TOP),
                4 => (Pos2::new(x, plot.bottom() + 3.0), Align2::RIGHT_TOP),
                _ => (Pos2::new(x, plot.bottom() + 3.0), Align2::CENTER_TOP),
            };
            painter.text(pos, align, format!("{}A", passive::eng(i)), font.clone(), text);
        }
        if let Some(i0) = i0 {
            let curve: Vec<Pos2> = (0..=120).map(|k| {
                let i = imax * k as f64 / 120.0;
                to(i, passive::inductance_ratio(i0, i) * 100.0)
            }).collect();
            painter.add(egui::Shape::line(curve, Stroke::new(2.0, Color32::from_rgb(70, 140, 230))));
        }
        for (i, p) in &pts {
            painter.circle_filled(to(*i, 100.0 + p), 4.0, Color32::from_rgb(230, 90, 60));
        }
        ui.label(egui::RichText::new("● 赤い点: データシートの点    ─ 青い線: モデルの曲線").size(13.5));
    }
}
