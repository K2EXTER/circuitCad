//! Bench recordings drawn over the simulated plots: an oscilloscope's CSV over the transient waveforms, and a few measured
//! (frequency, gain) points over the Bode plot. Kept with the sheet (key `"bench"`), outside `Schematic` so that adding a
//! recording does not change the circuit's signature (the results stay valid).

use super::*;

/// One recording, compared with one simulated signal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BenchCurve {
    /// What the learner calls it ("基板A F1").
    pub name: String,
    /// The simulated signal it is drawn with (`V(F1)`); its colour follows that signal.
    pub signal: String,
    /// Time (s) or frequency (Hz).
    pub x: Vec<f64>,
    /// Volts (transient) or dB (AC).
    pub y: Vec<f64>,
    /// Phase in degrees (AC, when it was measured; empty otherwise).
    pub phase: Vec<f64>,
    /// Added to `x` when drawing a transient recording (the scope's trigger is not the simulation's t = 0).
    pub shift: f64,
}

impl BenchCurve {
    /// The time axis as drawn (`x + shift`).
    pub fn shifted_x(&self) -> Vec<f64> {
        self.x.iter().map(|t| t + self.shift).collect()
    }

    fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({ "name": self.name, "signal": self.signal, "x": self.x, "y": self.y });
        if !self.phase.is_empty() {
            v["phase"] = serde_json::json!(self.phase);
        }
        if self.shift != 0.0 {
            v["shift"] = self.shift.into();
        }
        v
    }

    fn from_json(v: &serde_json::Value) -> Option<BenchCurve> {
        let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
        let nums = |k: &str| -> Option<Vec<f64>> { v.get(k).map_or(Some(vec![]), |a| a.as_array()?.iter().map(|x| x.as_f64()).collect()) };
        let c = BenchCurve { name: text("name"), signal: text("signal"), x: nums("x")?, y: nums("y")?, phase: nums("phase")?, shift: v.get("shift").and_then(|x| x.as_f64()).unwrap_or(0.0) };
        (c.x.len() == c.y.len() && (c.phase.is_empty() || c.phase.len() == c.x.len())).then_some(c)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BenchData {
    pub tran: Vec<BenchCurve>,
    pub ac: Vec<BenchCurve>,
}

impl BenchData {
    pub fn is_empty(&self) -> bool {
        self.tran.is_empty() && self.ac.is_empty()
    }
}

/// Stores the recordings in a sheet's JSON under `"bench"` (no key when there are none).
pub fn with_bench(sheet: &mut serde_json::Value, bench: &BenchData) {
    if let (Some(obj), false) = (sheet.as_object_mut(), bench.is_empty()) {
        let list = |cs: &[BenchCurve]| serde_json::Value::Array(cs.iter().map(BenchCurve::to_json).collect());
        let mut v = serde_json::json!({});
        if !bench.tran.is_empty() {
            v["tran"] = list(&bench.tran);
        }
        if !bench.ac.is_empty() {
            v["ac"] = list(&bench.ac);
        }
        obj.insert("bench".into(), v);
    }
}

/// Takes the recordings out of a sheet's JSON (empty for files without them; a recording this version cannot read is left out).
pub fn take_bench(sheet: &mut serde_json::Value) -> BenchData {
    let Some(v) = sheet.as_object_mut().and_then(|o| o.remove("bench")) else { return BenchData::default() };
    let list = |k: &str| v.get(k).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(BenchCurve::from_json).collect()).unwrap_or_default();
    BenchData { tran: list("tran"), ac: list("ac") }
}

/// The numbers of a pasted table or a CSV file, column by column. Lines whose cells are not all numbers (headers, the
/// scope's settings) are skipped; only rows with the most common count of numbers are kept, so a footer or a settings line
/// with two numbers does not break the table.
pub fn read_table(text: &str) -> Result<Vec<Vec<f64>>, String> {
    let rows: Vec<Vec<f64>> = text
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split([',', '\t', ';', ' ']).map(|c| c.trim().trim_matches('"')).filter(|c| !c.is_empty()).collect();
            let nums: Option<Vec<f64>> = cells.iter().map(|c| c.parse::<f64>().ok().or_else(|| parse_value(c))).collect();
            nums.filter(|n| n.len() >= 2 && n.iter().all(|v| v.is_finite()))
        })
        .collect();
    let mut counts: Vec<(usize, usize)> = vec![];
    for r in &rows {
        match counts.iter_mut().find(|(n, _)| *n == r.len()) {
            Some((_, c)) => *c += 1,
            None => counts.push((r.len(), 1)),
        }
    }
    let Some(&(width, _)) = counts.iter().max_by_key(|(n, c)| (*c, *n)) else {
        return Err("数字の行が見つかりません。1行に「時間, 電圧」や「周波数 dB」のように、2つ以上の数字を並べてください".into());
    };
    let rows: Vec<&Vec<f64>> = rows.iter().filter(|r| r.len() == width).collect();
    Ok((0..width).map(|k| rows.iter().map(|r| r[k]).collect()).collect())
}

/// How the columns of a measured frequency response are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcColumns {
    /// frequency, gain in dB [, phase in degrees]
    Db,
    /// frequency, input amplitude, output amplitude [, phase] (the same unit, Vpp or Vrms)
    Amplitudes,
}

/// A measured frequency response from a table: frequency, dB and the phase (empty when not measured), sorted by frequency.
pub fn ac_points(cols: &[Vec<f64>], layout: AcColumns) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), String> {
    let need = if layout == AcColumns::Db { 2 } else { 3 };
    if cols.len() < need {
        return Err(if layout == AcColumns::Db { "周波数と dB の 2 列が要ります" } else { "周波数・入力の振幅・出力の振幅の 3 列が要ります" }.into());
    }
    let mut pts: Vec<(f64, f64, Option<f64>)> = vec![];
    for i in 0..cols[0].len() {
        let f = cols[0][i];
        let db = match layout {
            AcColumns::Db => cols[1][i],
            AcColumns::Amplitudes => {
                let (vin, vout) = (cols[1][i], cols[2][i]);
                if vin <= 0.0 || vout <= 0.0 {
                    return Err(format!("{} Hz の行: 振幅は正の数で書いてください", sim_core::format_value(f)));
                }
                20.0 * (vout / vin).log10()
            }
        };
        if f <= 0.0 {
            return Err("周波数は正の数で書いてください".into());
        }
        pts.push((f, db, cols.get(need).map(|c| c[i])));
    }
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let phase: Vec<f64> = if pts.iter().all(|p| p.2.is_some()) { pts.iter().map(|p| p.2.unwrap_or(0.0)).collect() } else { vec![] };
    Ok((pts.iter().map(|p| p.0).collect(), pts.iter().map(|p| p.1).collect(), phase))
}

/// A transient recording from a table: the first column is the time, `column` (1 = the second) the value. Sorted by time.
pub fn tran_points(cols: &[Vec<f64>], column: usize) -> Result<(Vec<f64>, Vec<f64>), String> {
    let y = cols.get(column).ok_or_else(|| format!("{} 列目がありません (この表は {} 列です)", column + 1, cols.len()))?;
    let mut pts: Vec<(f64, f64)> = cols[0].iter().copied().zip(y.iter().copied()).collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    if pts.len() < 2 {
        return Err("点が 2 つ以上要ります".into());
    }
    Ok(pts.into_iter().unzip())
}

/// Linear interpolation of `v(t)` at `x` (None outside the data).
pub fn value_at(t: &[f64], v: &[f64], x: f64) -> Option<f64> {
    if t.len() < 2 || x < t[0] || x > t[t.len() - 1] {
        return None;
    }
    let i = t.partition_point(|&a| a < x).clamp(1, t.len() - 1);
    let (t0, t1) = (t[i - 1], t[i]);
    Some(if t1 > t0 { v[i - 1] + (v[i] - v[i - 1]) * (x - t0) / (t1 - t0) } else { v[i] })
}

/// Gain in dB at `f`, interpolated on the log-frequency axis (None outside the data).
pub fn db_at(freq: &[f64], db: &[f64], f: f64) -> Option<f64> {
    let lf: Vec<f64> = freq.iter().map(|x| x.max(1e-300).ln()).collect();
    value_at(&lf, db, f.max(1e-300).ln())
}

/// The -3 dB frequency of measured points: where the gain first falls 3 dB below the first (low-frequency) point,
/// interpolated on the log-frequency axis. None when the points do not cross it.
pub fn measured_cutoff(freq: &[f64], db: &[f64]) -> Option<f64> {
    let level = db.first()? - 3.0;
    (1..freq.len()).find(|&i| db[i - 1] >= level && db[i] < level).map(|i| {
        let (a, b) = (freq[i - 1].ln(), freq[i].ln());
        (a + (b - a) * (db[i - 1] - level) / (db[i - 1] - db[i])).exp()
    })
}

/// The low and high level of a waveform: its 1st and 99th percentile, so that the noise of a scope recording (a few
/// spikes) does not widen the range.
fn levels(v: &[f64]) -> (f64, f64) {
    let mut s: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    s.sort_by(f64::total_cmp);
    if s.is_empty() {
        return (0.0, 0.0);
    }
    let at = |q: f64| s[((s.len() - 1) as f64 * q).round() as usize];
    (at(0.01), at(0.99))
}

/// The time where a waveform first crosses the middle of its range going in the direction of its first big change
/// (a rising or a falling edge). Used to line a scope recording up with the simulation.
pub fn first_edge(t: &[f64], v: &[f64]) -> Option<f64> {
    let (lo, hi) = levels(v);
    if t.len() < 2 || hi - lo <= 0.0 {
        return None;
    }
    let mid = (lo + hi) / 2.0;
    (1..t.len()).find(|&i| (v[i - 1] - mid) * (v[i] - mid) <= 0.0 && v[i] != v[i - 1]).map(|i| t[i - 1] + (t[i] - t[i - 1]) * (mid - v[i - 1]) / (v[i] - v[i - 1]))
}

/// The 10 % -> 90 % time of the first edge (rising or falling), over the waveform's own range.
pub fn edge_time(t: &[f64], v: &[f64]) -> Option<f64> {
    let (lo, hi) = levels(v);
    let span = hi - lo;
    if span <= 0.0 {
        return None;
    }
    let edge = first_edge(t, v)?;
    let i = t.partition_point(|&x| x < edge).min(t.len() - 1);
    let rising = v[i] >= v[i.saturating_sub(1)];
    let (a, b) = if rising { (lo + 0.1 * span, lo + 0.9 * span) } else { (hi - 0.1 * span, hi - 0.9 * span) };
    // search outwards from the edge for the two levels
    let cross_before = |level: f64| (1..=i).rev().find(|&k| (v[k - 1] - level) * (v[k] - level) <= 0.0 && v[k] != v[k - 1]).map(|k| t[k - 1] + (t[k] - t[k - 1]) * (level - v[k - 1]) / (v[k] - v[k - 1]));
    let cross_after = |level: f64| (i.max(1)..t.len()).find(|&k| (v[k - 1] - level) * (v[k] - level) <= 0.0 && v[k] != v[k - 1]).map(|k| t[k - 1] + (t[k] - t[k - 1]) * (level - v[k - 1]) / (v[k] - v[k - 1]));
    let t10 = cross_before(a).or_else(|| cross_after(a))?;
    let t90 = cross_after(b)?;
    (t90 > t10).then_some(t90 - t10)
}

/// The state of the "実測を重ねる" box (not saved).
#[derive(Default)]
pub struct BenchUi {
    /// The pasted table.
    pub text: String,
    pub name: String,
    /// The simulated signal the next recording is compared with.
    pub signal: String,
    /// AC: the table has input and output amplitudes instead of dB.
    pub amplitudes: bool,
    /// Transient: which column holds the value, counted from the second (0 = the second column).
    pub column: usize,
    /// The last message: (it worked, text).
    pub msg: Option<(bool, String)>,
}

impl SchEditor {
    pub fn set_bench(&mut self, bench: BenchData) {
        self.bench = bench;
        self.saved_bench = self.bench.clone();
    }

    pub fn bench_data(&self) -> &BenchData {
        &self.bench
    }
}

/// The colour of a recording: the colour of the simulated signal it is compared with, or grey when that one is not shown.
pub fn bench_color(selected_order: &[&str], signal: &str) -> Color32 {
    selected_order.iter().position(|n| *n == signal).map_or(Color32::from_gray(225), |k| PALETTE[k % PALETTE.len()])
}

/// Builds a recording from a table's text. `ac`: frequency response, otherwise a transient recording.
fn make_curve(text: &str, st: &BenchUi, ac: bool, name: String) -> Result<BenchCurve, String> {
    let cols = read_table(text)?;
    let mut c = BenchCurve { name, signal: st.signal.clone(), ..Default::default() };
    if ac {
        let (f, db, ph) = ac_points(&cols, if st.amplitudes { AcColumns::Amplitudes } else { AcColumns::Db })?;
        (c.x, c.y, c.phase) = (f, db, ph);
    } else {
        let (t, v) = tran_points(&cols, st.column + 1)?;
        (c.x, c.y) = (t, v);
    }
    Ok(c)
}

/// The "実測を重ねる" box in the left column of the transient / AC viewer: paste a table or read a CSV, then the list of
/// recordings with how they compare. `signals` are the simulated signals that can be picked; `sim(name)` gives a simulated
/// signal as (x, y) in the plot's units (time and volts, or frequency and dB).
pub fn bench_controls(ui: &mut egui::Ui, curves: &mut Vec<BenchCurve>, st: &mut BenchUi, ac: bool, signals: &[&str], shown: &[&str], sim: &dyn Fn(&str) -> Option<(Vec<f64>, Vec<f64>)>) {
    let title = if curves.is_empty() { i18n::tr("実測を重ねる").to_string() } else { i18n::trf!("実測を重ねる ({})", curves.len()) };
    egui::CollapsingHeader::new(title).id_salt(("bench", ac)).show(ui, |ui| {
        ui.small(if ac {
            i18n::tr("測った点を、ボード線図の上に点で重ねます。1行に「周波数 dB」(位相を足してもよい) か、「周波数 入力の振幅 出力の振幅」を書きます")
        } else {
            i18n::tr("オシロスコープの CSV を、波形の上に重ねます。1列目が時間、値の列は下で選びます。見出しの行は読み飛ばします")
        });
        // the first signal on the plot is the likely one to compare with
        if st.signal.is_empty() || !signals.contains(&st.signal.as_str()) {
            st.signal = shown.iter().find(|n| signals.contains(n)).or(signals.first()).map(|s| s.to_string()).unwrap_or_default();
        }
        ui.horizontal(|ui| {
            ui.label(i18n::tr("比べる信号"));
            egui::ComboBox::from_id_salt(("bench_sig", ac)).selected_text(st.signal.clone()).width(90.0).show_ui(ui, |ui| {
                for s in signals {
                    ui.selectable_value(&mut st.signal, s.to_string(), *s);
                }
            });
        })
        .response
        .on_hover_text(i18n::tr("実測を、どのシミュレーションの信号と比べるか。色がその信号にそろい、差が下に出ます"));
        ui.horizontal(|ui| {
            ui.label(i18n::tr("名前"));
            ui.add(egui::TextEdit::singleline(&mut st.name).desired_width(110.0).hint_text(i18n::tr("例 基板A")));
        });
        if ac {
            ui.horizontal(|ui| {
                ui.radio_value(&mut st.amplitudes, false, i18n::tr("周波数・dB")).on_hover_text(i18n::tr("例: 1k -4.5 (位相を足すなら 1k -4.5 -48)"));
                ui.radio_value(&mut st.amplitudes, true, i18n::tr("周波数・入力・出力")).on_hover_text(i18n::tr("例: 1k 1.02 0.61 (入力と出力の振幅。Vpp どうし、実効値どうしのように単位をそろえます)。dB は 20×log10(出力/入力) で計算します"));
            });
        } else {
            ui.horizontal(|ui| {
                ui.label(i18n::tr("値の列"));
                let mut n = st.column + 2;
                if ui.add(egui::DragValue::new(&mut n).range(2..=9)).on_hover_text(i18n::tr("値を読む列。2 なら CH1、3 なら CH2 のように、CSV の並びに合わせます")).changed() {
                    st.column = n - 2;
                }
            });
        }
        ui.add(egui::TextEdit::multiline(&mut st.text).desired_rows(3).desired_width(f32::INFINITY).hint_text(if ac { "100 -0.1\n1k -4.5\n10k -16.6" } else { "0, 0.01\n1e-4, 0.30" }));
        ui.horizontal_wrapped(|ui| {
            let add = |text: &str, fallback: String, st: &mut BenchUi, curves: &mut Vec<BenchCurve>| {
                let name = if st.name.trim().is_empty() { fallback } else { st.name.trim().to_string() };
                match make_curve(text, st, ac, name.clone()) {
                    Ok(c) => {
                        st.msg = Some((true, i18n::trf!("「{}」を重ねました ({} 点)", name, c.x.len())));
                        curves.push(c);
                    }
                    Err(e) => st.msg = Some((false, i18n::tr_dyn(&e).to_string())),
                }
            };
            if ui.button(i18n::tr("貼った表を重ねる")).on_hover_text(i18n::tr("上の欄に貼った数字の表を、実測として重ねます")).clicked() {
                let text = st.text.clone();
                add(&text, i18n::trf!("実測{}", curves.len() + 1), st, curves);
            }
            if ui.button(i18n::tr("CSV を読む…")).on_hover_text(i18n::tr("オシロスコープなどが書き出した CSV ファイルを読んで重ねます")).clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt", "tsv"]).pick_file()
            {
                match std::fs::read_to_string(&p) {
                    Ok(text) => {
                        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        add(&text, stem, st, curves);
                    }
                    Err(e) => st.msg = Some((false, i18n::trf!("ファイルを読めませんでした: {e}"))),
                }
            }
        });
        if let Some((ok, m)) = &st.msg {
            ui.small(egui::RichText::new(m).color(if *ok { crate::theme::OK } else { crate::theme::ERROR }));
        }
        let mut remove = None;
        for (i, c) in curves.iter_mut().enumerate() {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("● {}", c.name)).color(bench_color(shown, &c.signal)).strong());
                ui.small(format!("↔ {}", c.signal));
                if ui.small_button("🗑").on_hover_text(i18n::tr("この実測を消します")).clicked() {
                    remove = Some(i);
                }
            });
            let Some((sx, sy)) = sim(&c.signal) else {
                ui.small(i18n::trf!("  {} がいまの結果にありません", c.signal));
                continue;
            };
            if ac {
                ac_comparison(ui, c, &sx, &sy, i);
            } else {
                tran_comparison(ui, c, &sx, &sy);
            }
        }
        if let Some(i) = remove {
            curves.remove(i);
        }
    });
}

fn ac_comparison(ui: &mut egui::Ui, c: &BenchCurve, sf: &[f64], sdb: &[f64], i: usize) {
    let mag: Vec<f64> = sdb.iter().map(|d| 10f64.powf(d / 20.0)).collect();
    let sim_fc = sim_core::minus_3db(sf, &mag);
    let meas_fc = measured_cutoff(&c.x, &c.y);
    match (meas_fc, sim_fc) {
        (Some(m), Some(s)) => {
            ui.small(i18n::trf!("  -3dB点: 実測 {} / シミュレーション {} ({:+.1}%)", eng(m, "Hz"), eng(s, "Hz"), (m / s - 1.0) * 100.0))
                .on_hover_text(i18n::tr("実測の -3dB点は、いちばん低い周波数の点から 3dB 下がる所を、点のあいだでつないで求めます。点が粗いと少しずれます"));
        }
        (None, Some(s)) => {
            ui.small(i18n::trf!("  -3dB点: シミュレーション {} (実測の点は、まだそこを横切っていません)", eng(s, "Hz")));
        }
        _ => {}
    }
    egui::Grid::new(("bench_ac_grid", i)).striped(true).spacing([8.0, 1.0]).show(ui, |ui| {
        for h in ["周波数", "実測", "シミュ", "差"] {
            ui.small(i18n::tr(h));
        }
        ui.end_row();
        for (f, m) in c.x.iter().zip(&c.y).take(16) {
            ui.small(eng(*f, "Hz"));
            ui.small(format!("{m:.2} dB"));
            match db_at(sf, sdb, *f) {
                Some(s) => {
                    ui.small(format!("{s:.2} dB"));
                    let d = m - s;
                    let col = if d.abs() <= 1.0 { crate::theme::OK } else if d.abs() <= 3.0 { crate::theme::WARN } else { crate::theme::ERROR };
                    ui.small(egui::RichText::new(format!("{d:+.2}")).color(col)).on_hover_text(i18n::tr("実測 − シミュレーション (dB)。±1 dB 以内は緑、±3 dB 以内は黄"));
                }
                None => {
                    ui.small("—").on_hover_text(i18n::tr("シミュレーションの周波数範囲の外です"));
                    ui.small("");
                }
            }
            ui.end_row();
        }
    });
}

fn tran_comparison(ui: &mut egui::Ui, c: &mut BenchCurve, st: &[f64], sv: &[f64]) {
    ui.horizontal(|ui| {
        ui.small(i18n::tr("  時間のずらし"));
        let mut text = sim_core::format_value(c.shift);
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(60.0)).on_hover_text(i18n::tr("実測の時間に足す量 (秒)。オシロのトリガーの位置は、シミュレーションの 0 秒と違うので、ここで合わせます"));
        if r.changed()
            && let Some(v) = parse_value(&text)
        {
            c.shift = v;
        }
        ui.small("s");
        if ui.small_button(i18n::tr("立ち上がりを合わせる")).on_hover_text(i18n::tr("実測とシミュレーションの最初の変化 (振れ幅の真ん中を横切る所) が同じ時刻になるように、時間をずらします")).clicked()
            && let (Some(a), Some(b)) = (first_edge(&c.x, &c.y), first_edge(st, sv))
        {
            c.shift = b - a;
        }
    });
    let (m, s) = (edge_time(&c.x, &c.y), edge_time(st, sv));
    match (m, s) {
        (Some(m), Some(s)) => {
            ui.small(i18n::trf!("  変化の 10→90%: 実測 {} / シミュレーション {} ({:+.1}%)", eng(m, "s"), eng(s, "s"), (m / s - 1.0) * 100.0))
                .on_hover_text(i18n::tr("それぞれの波形の最初の変化 (立ち上がりか立ち下がり) で、振れ幅の 10% から 90% までの時間です。振れ幅の下と上は、雑音のとがりを除くため、値の小さいほうから 1% と大きいほうから 1% の所に取ります"));
        }
        (Some(m), None) => {
            ui.small(i18n::trf!("  変化の 10→90%: 実測 {}", eng(m, "s")));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_csv_with_a_header_and_settings_lines_is_read() {
        let csv = "Model,HDS272S\nTime(s),CH1(V),CH2(V)\nTrigger,0.5\n-1e-4,0.01,0.00\n0,0.02,0.01\n1e-4,0.30,0.15\n2e-4,0.55,0.35\n";
        let cols = read_table(csv).unwrap();
        assert_eq!(cols.len(), 3, "the settings line with one number is skipped");
        assert_eq!(cols[0], vec![-1e-4, 0.0, 1e-4, 2e-4]);
        let (t, v) = tran_points(&cols, 2).unwrap();
        assert_eq!(t.len(), 4);
        assert_eq!(v[3], 0.35);
        assert!(tran_points(&cols, 5).unwrap_err().contains("6 列目"));
    }

    #[test]
    fn pasted_points_with_prefixes_and_spaces_are_read() {
        let cols = read_table("100  -0.1\n1k -4.5\n10k\t-16.6\n").unwrap();
        let (f, db, ph) = ac_points(&cols, AcColumns::Db).unwrap();
        assert_eq!(f, vec![100.0, 1000.0, 10000.0]);
        assert_eq!(db[2], -16.6);
        assert!(ph.is_empty());
        assert!(read_table("hello\nworld").is_err());
    }

    #[test]
    fn amplitudes_become_db_and_the_cutoff_is_found() {
        // first-order low pass at 1 kHz, input 1 V: out = 1/sqrt(1+(f/fc)^2)
        let rows: String = [100.0, 300.0, 700.0, 1500.0, 5000.0]
            .iter()
            .map(|f: &f64| format!("{f}, 1.0, {}\n", 1.0 / (1.0 + (f / 1000.0).powi(2)).sqrt()))
            .collect();
        let (f, db, _) = ac_points(&read_table(&rows).unwrap(), AcColumns::Amplitudes).unwrap();
        assert!((db[0] + 0.043).abs() < 0.01, "{db:?}");
        let fc = measured_cutoff(&f, &db).unwrap();
        assert!((fc - 1000.0).abs() < 80.0, "{fc}");
        assert!(ac_points(&read_table("100 1").unwrap(), AcColumns::Amplitudes).is_err());
        // between 700 Hz and 1.5 kHz the straight line on the log axis is a few tenths of a dB off the curve
        assert!((db_at(&f, &db, 1000.0).unwrap() + 3.0).abs() < 0.5);
        assert_eq!(db_at(&f, &db, 10.0), None);
    }

    #[test]
    fn the_edge_and_its_rise_time_are_measured() {
        // RC step at t = 1 ms, tau = 0.2 ms: 10-90 % = 2.197 tau = 0.4394 ms
        let t: Vec<f64> = (0..4000).map(|i| i as f64 * 1e-6).collect();
        let v: Vec<f64> = t.iter().map(|&x| if x < 1e-3 { 0.0 } else { 1.0 - (-(x - 1e-3) / 2e-4).exp() }).collect();
        let edge = first_edge(&t, &v).unwrap();
        assert!((edge - (1e-3 + 2e-4 * 2f64.ln())).abs() < 2e-6, "{edge}");
        let rise = edge_time(&t, &v).unwrap();
        assert!((rise - 4.394e-4).abs() < 3e-6, "{rise}");
        // a falling edge too
        let w: Vec<f64> = v.iter().map(|x| 1.0 - x).collect();
        assert!((edge_time(&t, &w).unwrap() - 4.394e-4).abs() < 3e-6);
        assert_eq!(edge_time(&t, &vec![1.0; t.len()]), None);
        // a few spikes of noise do not move the levels (the 1st and 99th percentile)
        let mut spiky = v.clone();
        for k in [500, 1500, 2500] {
            spiky[k] += 0.4;
        }
        assert!((edge_time(&t, &spiky).unwrap() - 4.394e-4).abs() < 6e-6);
    }

    #[test]
    fn recordings_ride_along_in_the_sheet_json() {
        let mut v = serde_json::json!({ "components": [] });
        with_bench(&mut v, &BenchData::default());
        assert!(v.get("bench").is_none(), "no key when there is nothing");
        let data = BenchData { ac: vec![BenchCurve { name: "A".into(), signal: "V(F1)".into(), x: vec![100.0], y: vec![-0.1], ..Default::default() }], ..Default::default() };
        with_bench(&mut v, &data);
        assert!(v["bench"]["ac"][0].get("phase").is_none() && v["bench"].get("tran").is_none());
        assert_eq!(take_bench(&mut v), data);
        assert!(v.get("bench").is_none());
        assert_eq!(take_bench(&mut serde_json::json!({ "bench": 5 })), BenchData::default());
    }
}
