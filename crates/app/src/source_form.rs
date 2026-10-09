//! A form for the value of a voltage source: pick DC / sine / pulse and fill in numbers instead of
//! typing `SIN(0 1 1k) AC 1` by hand. The text stays the single source of truth: the form is
//! parsed from it and writes it back, so the value field and the form can be mixed freely.

use eframe::egui;
use sim_core::{parse_source_full, parse_value, Waveform};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaveKind {
    Dc,
    Sin,
    Pulse,
    /// PWL, noise, sums ... edited as text.
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceForm {
    pub kind: WaveKind,
    pub dc: String,
    /// offset, amplitude, frequency, delay, damping, phase
    pub sin: [String; 6],
    /// low, high, delay, rise, fall, width, period
    pub pulse: [String; 7],
    pub other: String,
    pub ac_on: bool,
    pub ac_mag: String,
    pub ac_phase: String,
}

impl Default for SourceForm {
    fn default() -> Self {
        let s = |x: &str| x.to_string();
        Self {
            kind: WaveKind::Dc,
            dc: s("5"),
            sin: [s("0"), s("1"), s("1k"), s("0"), s("0"), s("0")],
            pulse: [s("0"), s("5"), s("0"), s("1u"), s("1u"), s("0.5m"), s("1m")],
            other: String::new(),
            ac_on: false,
            ac_mag: s("1"),
            ac_phase: s("0"),
        }
    }
}

/// A number the way a person writes it: `1k`, `4.7u`, `0.5`. Falls back to plain digits when the
/// short form would not read back as exactly the same value.
pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    for (scale, suffix) in [(1e9, "G"), (1e6, "Meg"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "u"), (1e-9, "n"), (1e-12, "p")] {
        if a >= scale * 0.9999999 {
            let mut t = format!("{:.6}", v / scale);
            if t.contains('.') {
                t = t.trim_end_matches('0').trim_end_matches('.').to_string();
            }
            let short = format!("{t}{suffix}");
            if parse_value(&short).is_some_and(|x| (x - v).abs() <= 1e-9 * a) {
                return short;
            }
            break;
        }
    }
    format!("{v}")
}

/// The text before an `AC ...` clause.
fn without_ac(text: &str) -> &str {
    let lower = text.to_ascii_lowercase();
    for (i, _) in lower.match_indices("ac") {
        let before = i == 0 || lower.as_bytes()[i - 1].is_ascii_whitespace();
        let after = lower[i + 2..].chars().next().is_none_or(|c| c.is_whitespace());
        if before && after {
            return text[..i].trim();
        }
    }
    text.trim()
}

impl SourceForm {
    pub fn parse(text: &str) -> Self {
        let mut f = SourceForm::default();
        match parse_source_full(text) {
            Ok(src) => {
                if let Some((m, p)) = src.ac {
                    f.ac_on = true;
                    f.ac_mag = fmt_num(m);
                    f.ac_phase = fmt_num(p);
                }
                match src.wave {
                    Waveform::Dc(v) => {
                        f.kind = WaveKind::Dc;
                        f.dc = fmt_num(v);
                    }
                    Waveform::Sin { vo, va, freq, td, theta, phase } => {
                        f.kind = WaveKind::Sin;
                        f.sin = [vo, va, freq, td, theta, phase].map(fmt_num);
                    }
                    Waveform::Pulse { v1, v2, td, tr, tf, pw, per } => {
                        f.kind = WaveKind::Pulse;
                        f.pulse = [v1, v2, td, tr, tf, pw, per].map(fmt_num);
                    }
                    _ => {
                        f.kind = WaveKind::Other;
                        f.other = without_ac(text).to_string();
                    }
                }
            }
            Err(_) => {
                f.kind = WaveKind::Other;
                f.other = text.trim().to_string();
            }
        }
        f
    }

    /// The source text, or what is wrong with the numbers.
    pub fn to_text(&self) -> Result<String, String> {
        let num = |label: &str, s: &str| parse_value(s).ok_or_else(|| format!("{label}「{s}」を数値にしてください"));
        let mut t = match self.kind {
            WaveKind::Dc => fmt_num(num("電圧", &self.dc)?),
            WaveKind::Sin => {
                let labels = ["オフセット", "振幅", "周波数", "遅延", "減衰", "位相"];
                let v: Vec<f64> = (0..6).map(|i| num(labels[i], &self.sin[i])).collect::<Result<_, _>>()?;
                if v[2] <= 0.0 {
                    return Err("周波数は正の値にしてください".into());
                }
                // trailing parameters that are zero need not be written
                let n = if v[5] != 0.0 { 6 } else if v[4] != 0.0 { 5 } else if v[3] != 0.0 { 4 } else { 3 };
                format!("SIN({})", v[..n].iter().map(|x| fmt_num(*x)).collect::<Vec<_>>().join(" "))
            }
            WaveKind::Pulse => {
                let labels = ["低い値", "高い値", "遅延", "立上り", "立下り", "幅", "周期"];
                let defaults = [0.0, 0.0, 0.0, 1e-9, 1e-9, f64::NAN, f64::NAN];
                let mut v = vec![];
                for i in 0..7 {
                    if self.pulse[i].trim().is_empty() {
                        if i < 2 {
                            return Err(format!("{}を入力してください", labels[i]));
                        }
                        v.push(defaults[i]);
                    } else {
                        v.push(num(labels[i], &self.pulse[i])?);
                    }
                }
                // width and period may both be left out (a single pulse), but not just one of them
                let n = if !v[6].is_nan() {
                    if v[5].is_nan() {
                        return Err("周期を入れるときは、幅も入力してください".into());
                    }
                    7
                } else if !v[5].is_nan() {
                    6
                } else {
                    5
                };
                format!("PULSE({})", v[..n].iter().map(|x| fmt_num(*x)).collect::<Vec<_>>().join(" "))
            }
            WaveKind::Other => {
                if self.other.trim().is_empty() {
                    return Err("値を入力してください".into());
                }
                self.other.trim().to_string()
            }
        };
        if self.ac_on {
            let m = num("AC の大きさ", &self.ac_mag)?;
            let p = if self.ac_phase.trim().is_empty() { 0.0 } else { num("AC の位相", &self.ac_phase)? };
            t.push_str(&format!(" AC {}", fmt_num(m)));
            if p != 0.0 {
                t.push_str(&format!(" {}", fmt_num(p)));
            }
        }
        // the result must be something the simulator accepts
        parse_source_full(&t).map(|_| t)
    }
}

fn field(ui: &mut egui::Ui, label: &str, text: &mut String, tip: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label).on_hover_text(tip);
        let bad = !text.trim().is_empty() && parse_value(text).is_none();
        let mut edit = egui::TextEdit::singleline(text).desired_width(70.0);
        if bad {
            edit = edit.text_color(egui::Color32::from_rgb(230, 60, 60));
        }
        changed = ui.add(edit).on_hover_text(tip).changed();
    });
    changed
}

/// Draw the form; returns true when anything changed.
pub fn show(ui: &mut egui::Ui, f: &mut SourceForm) -> bool {
    let mut changed = false;
    ui.label("電源の種類");
    ui.horizontal_wrapped(|ui| {
        let before = f.kind;
        ui.selectable_value(&mut f.kind, WaveKind::Dc, "直流").on_hover_text("一定の電圧。電池や電源ライン (5V など) に使います");
        ui.selectable_value(&mut f.kind, WaveKind::Sin, "交流 (正弦波)").on_hover_text("なめらかに振動する交流。信号源や、フィルタの動きを見るのに使います");
        ui.selectable_value(&mut f.kind, WaveKind::Pulse, "パルス").on_hover_text("0 と 5V などを繰り返す矩形波。ロジックやスイッチの動作確認に使います");
        ui.selectable_value(&mut f.kind, WaveKind::Other, "その他").on_hover_text("PWL (折れ線) やノイズなど。テキストで直接書きます");
        if f.kind != before {
            changed = true;
            if before == WaveKind::Dc && f.kind == WaveKind::Sin {
                // start the sine at the DC level that was set
                f.sin[0] = f.dc.clone();
            }
        }
    });
    match f.kind {
        WaveKind::Dc => {
            changed |= field(ui, "電圧 [V]", &mut f.dc, "一定の電圧。例: 5, 3.3, 12");
        }
        WaveKind::Sin => {
            changed |= field(ui, "オフセット [V]", &mut f.sin[0], "波の中心の電圧 (直流分)。0 なら 0V を中心に振れます");
            changed |= field(ui, "振幅 [V]", &mut f.sin[1], "中心から山までの高さ。1 なら −1V〜+1V で振れます (山から谷までは 2V)");
            changed |= field(ui, "周波数 [Hz]", &mut f.sin[2], "1 秒あたりの波の数。例: 50, 1k, 100k");
            ui.collapsing("詳細 (遅延・減衰・位相)", |ui| {
                changed |= field(ui, "遅延 [s]", &mut f.sin[3], "この時間まで波を始めません");
                changed |= field(ui, "減衰 [1/s]", &mut f.sin[4], "波がだんだん小さくなる速さ。0 なら減衰しません");
                changed |= field(ui, "位相 [度]", &mut f.sin[5], "波の開始位置のずれ。90 でコサイン波");
            });
        }
        WaveKind::Pulse => {
            changed |= field(ui, "低い値 [V]", &mut f.pulse[0], "パルスが出ていない間の電圧");
            changed |= field(ui, "高い値 [V]", &mut f.pulse[1], "パルスの間の電圧");
            changed |= field(ui, "幅 [s]", &mut f.pulse[5], "高い値でいる時間。空欄だとずっと高いままです");
            changed |= field(ui, "周期 [s]", &mut f.pulse[6], "繰り返しの間隔。空欄だと 1 回だけです");
            ui.collapsing("詳細 (遅延・立上り・立下り)", |ui| {
                changed |= field(ui, "遅延 [s]", &mut f.pulse[2], "最初のパルスが始まるまでの時間");
                changed |= field(ui, "立上り [s]", &mut f.pulse[3], "低い値から高い値に変わるのにかける時間");
                changed |= field(ui, "立下り [s]", &mut f.pulse[4], "高い値から低い値に戻るのにかける時間");
            });
        }
        WaveKind::Other => {
            changed |= ui.add(egui::TextEdit::singleline(&mut f.other).desired_width(220.0)).changed();
            ui.small("例: PWL(0 0 1m 5 2m 0) / NOISE(0.05 1Meg) / 5 + NOISE(0.05 1Meg) + SIN(0 0.02 100k)");
        }
    }
    ui.add_space(2.0);
    let ac = ui.checkbox(&mut f.ac_on, "AC解析の入力にする").on_hover_text(
        "周波数特性 (AC解析) を測るとき、この電源を入力として使います。\nAC解析のときだけ働く設定で、過渡解析やDC解析の電圧には影響しません",
    );
    changed |= ac.changed();
    if f.ac_on {
        changed |= field(ui, "AC の大きさ", &mut f.ac_mag, "通常は 1。出力がそのまま利得(倍率)になります");
        changed |= field(ui, "AC の位相 [度]", &mut f.ac_phase, "通常は 0");
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_the_way_people_write_them() {
        assert_eq!(fmt_num(1000.0), "1k");
        assert_eq!(fmt_num(4.7e-6), "4.7u");
        assert_eq!(fmt_num(0.5e-3), "500u");
        assert_eq!(fmt_num(2.5), "2.5");
        assert_eq!(fmt_num(1.5e6), "1.5Meg");
        assert_eq!(fmt_num(0.0), "0");
        assert_eq!(fmt_num(-3.3), "-3.3");
        assert_eq!(fmt_num(f64::INFINITY), "");
        for v in [1e-9, 0.1234567891, 123456.789, 5e-7, 33e-12, 9.99999e8] {
            assert!((parse_value(&fmt_num(v)).unwrap() - v).abs() <= 1e-6 * v.abs(), "{v}: {}", fmt_num(v));
        }
    }

    #[test]
    fn every_kind_survives_a_round_trip() {
        for text in [
            "5",
            "3.3",
            "SIN(0 1 1k)",
            "SIN(2.5 2.5 50 1m 10 90)",
            "PULSE(0 5 100u 1u 1u 400u 1m)",
            "PULSE(0 5)",
            "PWL(0 0 1m 5 2m 0)",
            "5 + NOISE(0.05 1Meg) + SIN(0 0.02 100k)",
            "DC 1.7 AC 1".trim(),
            "AC 1",
            "SIN(0 1 1k) AC 1 45",
        ] {
            let f = SourceForm::parse(text);
            let back = f.to_text().unwrap_or_else(|e| panic!("{text}: {e}"));
            let (a, b) = (parse_source_full(text).unwrap(), parse_source_full(&back).unwrap());
            assert_eq!((a.wave.clone(), a.ac), (b.wave.clone(), b.ac), "{text} -> {back}");
        }
    }

    #[test]
    fn the_form_reads_the_kind_and_the_ac_flag() {
        let f = SourceForm::parse("DC 1.7 AC 1");
        assert_eq!(f.kind, WaveKind::Dc);
        assert_eq!((f.dc.as_str(), f.ac_on, f.ac_mag.as_str()), ("1.7", true, "1"));
        let f = SourceForm::parse("AC 1");
        assert_eq!((f.kind, f.dc.as_str(), f.ac_on), (WaveKind::Dc, "0", true));
        let f = SourceForm::parse("SIN(0 1 1k)");
        assert_eq!((f.kind, f.sin[2].as_str(), f.ac_on), (WaveKind::Sin, "1k", false));
        assert_eq!(SourceForm::parse("PWL(0 0 1m 5)").kind, WaveKind::Other);
        assert_eq!(SourceForm::parse("PWL(0 0 1m 5)").other, "PWL(0 0 1m 5)");
        // unreadable text stays editable as text
        let f = SourceForm::parse("what?");
        assert_eq!((f.kind, f.other.as_str()), (WaveKind::Other, "what?"));
        // the AC clause is not part of the "other" text
        assert_eq!(SourceForm::parse("PWL(0 0 1m 5) AC 1").other, "PWL(0 0 1m 5)");
    }

    #[test]
    fn toggling_ac_only_touches_the_ac_clause() {
        let mut f = SourceForm::parse("5");
        assert_eq!(f.to_text().unwrap(), "5");
        f.ac_on = true;
        assert_eq!(f.to_text().unwrap(), "5 AC 1");
        f.ac_phase = "90".into();
        assert_eq!(f.to_text().unwrap(), "5 AC 1 90");
        f.ac_on = false;
        assert_eq!(f.to_text().unwrap(), "5");
    }

    #[test]
    fn switching_the_kind_builds_sensible_text() {
        let mut f = SourceForm::parse("2.5");
        f.kind = WaveKind::Sin;
        f.sin[0] = f.dc.clone();
        assert_eq!(f.to_text().unwrap(), "SIN(2.5 1 1k)");
        f.kind = WaveKind::Pulse;
        assert_eq!(f.to_text().unwrap(), "PULSE(0 5 0 1u 1u 500u 1m)");
        f.pulse[5] = String::new();
        f.pulse[6] = String::new();
        assert_eq!(f.to_text().unwrap(), "PULSE(0 5 0 1u 1u)");
        f.pulse[6] = "1m".into();
        assert!(f.to_text().is_err(), "a period without a width is refused");
    }

    #[test]
    fn bad_numbers_are_reported_not_written() {
        let mut f = SourceForm::parse("SIN(0 1 1k)");
        f.sin[2] = "fast".into();
        assert!(f.to_text().unwrap_err().contains("周波数"));
        f.sin[2] = "0".into();
        assert!(f.to_text().is_err());
        let mut g = SourceForm::parse("5");
        g.dc = "abc".into();
        assert!(g.to_text().is_err());
        g.dc = "5".into();
        g.ac_on = true;
        g.ac_mag = "x".into();
        assert!(g.to_text().unwrap_err().contains("AC"));
    }
}
