//! "How do I pick this value?" calculators for beginners. Each returns the answer, the steps
//! behind it, cautions, and what happens when the part or the wiring goes wrong.

use crate::value::{format_value, parse_value};

#[derive(Clone, Debug, Default)]
pub struct CalcOut {
    /// What to put in the part's value field (the E24 value), when there is one.
    pub primary: Option<String>,
    /// Label / value lines (the answer and what follows from it).
    pub rows: Vec<(String, String)>,
    /// The calculation, step by step.
    pub steps: Vec<String>,
    /// Things to check about this design.
    pub cautions: Vec<String>,
    /// What typically happens when something fails (open, short, wrong value, wrong direction).
    pub faults: Vec<String>,
}

const E24: [f64; 24] = [
    1.0, 1.1, 1.2, 1.3, 1.5, 1.6, 1.8, 2.0, 2.2, 2.4, 2.7, 3.0, 3.3, 3.6, 3.9, 4.3, 4.7, 5.1, 5.6, 6.2, 6.8, 7.5, 8.2, 9.1,
];

/// The E24 value closest to `v` (`up`: the smallest one that is not below `v`).
pub fn e24(v: f64, up: bool) -> f64 {
    let dec = v.log10().floor();
    let mut best = f64::NAN;
    let mut err = f64::MAX;
    for d in [dec - 1.0, dec, dec + 1.0] {
        for m in E24.iter().chain(std::iter::once(&10.0)) {
            let c = m * 10f64.powf(d);
            if up && c < v * (1.0 - 1e-9) {
                continue;
            }
            let e = (c / v).ln().abs();
            if e < err {
                err = e;
                best = c;
            }
        }
    }
    // clean up float noise such as 4.700000000000001
    parse_value(&format_value(best)).unwrap_or(best)
}

fn pos(name: &str, v: f64) -> Result<f64, String> {
    if v.is_finite() && v > 0.0 { Ok(v) } else { Err(format!("{name}は 0 より大きい数にしてください")) }
}

fn r(v: f64) -> String {
    format!("{}Ω", format_value(v))
}

/// LED current-limiting resistor.
pub fn led_resistor(vs: f64, vf: f64, i_ma: f64) -> Result<CalcOut, String> {
    let (vs, vf, i) = (pos("電源電圧", vs)?, pos("LEDの順方向電圧", vf)?, pos("電流", i_ma)? / 1000.0);
    if vs <= vf {
        return Err(format!("電源電圧 ({vs} V) が LED の順方向電圧 ({vf} V) 以下です。これでは LED は光りません。電源を上げるか、LED の数を減らしましょう"));
    }
    let exact = (vs - vf) / i;
    let std = e24(exact, true);
    let real_i = (vs - vf) / std;
    let p = (vs - vf) * real_i;
    let mut o = CalcOut { primary: Some(format_value(std)), ..Default::default() };
    o.rows = vec![
        ("計算上の抵抗".into(), r(exact)),
        ("使う抵抗 (E24, 切り上げ)".into(), r(std)),
        ("実際に流れる電流".into(), format!("{:.2} mA", real_i * 1000.0)),
        ("抵抗の消費電力".into(), format!("{:.1} mW", p * 1000.0)),
    ];
    o.steps = vec![
        format!("抵抗にかかる電圧 = 電源 − LED電圧 = {vs} − {vf} = {:.2} V", vs - vf),
        format!("オームの法則 R = V ÷ I = {:.2} ÷ {} mA = {}", vs - vf, i_ma, r(exact)),
        format!("市販の値 (E24) の中で、電流が目標を超えないように大きいほうへ丸めて {}", r(std)),
    ];
    if real_i > 0.020 {
        o.cautions.push("20 mA を超えています。一般的な 5mm LED の定格は 20 mA 程度なので、電流を下げてください".into());
    }
    if p > 0.125 {
        o.cautions.push(format!("抵抗の発熱が {:.0} mW あります。1/4 W (250 mW) の抵抗でも余裕が少ないので、W数の大きい抵抗にしましょう", p * 1000.0));
    }
    if real_i < 0.001 {
        o.cautions.push("電流が 1 mA を下回ると、ほとんどの LED はとても暗くなります".into());
    }
    o.cautions.push("LED の順方向電圧は色で違います (赤 約2V・緑 約2.2V・青/白 約3V)。データシートの値を使いましょう。電流は目標より少し小さくても十分明るく見えます (5 mA でも見えます)".into());
    o.faults = vec![
        "抵抗を入れ忘れる/ショートする: LED に電流が制限なく流れ、一瞬で焼損します (電流の上限は LED ではなく抵抗が決めています)".into(),
        "抵抗が大きすぎる: 暗い、または光らない".into(),
        "LED の向きが逆: 電流が流れず光りません (壊れることはほぼありません。アノード/カソードを確認)".into(),
        "電源電圧が上がった (電池 → ACアダプタなど): 抵抗の電圧が増え、電流が増えて LED が傷みます".into(),
    ];
    Ok(o)
}

/// Resistor divider: pick R1 for a given R2.
pub fn divider(vin: f64, vout: f64, r2: f64) -> Result<CalcOut, String> {
    let (vin, vout, r2) = (pos("入力電圧", vin)?, pos("出力電圧", vout)?, pos("R2", r2)?);
    if vout >= vin {
        return Err("出力電圧は入力電圧より小さくしてください。分圧で電圧を上げることはできません".into());
    }
    let r1x = r2 * (vin - vout) / vout;
    let r1 = e24(r1x, false);
    let real = vin * r2 / (r1 + r2);
    let i = vin / (r1 + r2);
    let mut o = CalcOut { primary: Some(format_value(r1)), ..Default::default() };
    o.rows = vec![
        ("計算上のR1".into(), r(r1x)),
        ("使うR1 (E24)".into(), r(r1)),
        ("実際の出力電圧".into(), format!("{real:.3} V")),
        ("分圧に流れ続ける電流".into(), format!("{:.3} mA", i * 1000.0)),
    ];
    o.steps = vec![
        "分圧の式: Vout = Vin × R2 ÷ (R1 + R2)".into(),
        format!("R1 について解くと R1 = R2 × (Vin − Vout) ÷ Vout = {} × {:.3} ÷ {} = {}", r(r2), vin - vout, vout, r(r1x)),
        format!("E24 の最も近い値 {} を使うと、出力は {real:.3} V (目標との差 {:+.1} %)", r(r1), (real / vout - 1.0) * 100.0),
    ];
    o.cautions.push(format!("出力につなぐ負荷の抵抗が R2 ({}) より十分大きい (10 倍以上) ことを確認してください。小さいと電圧が大きく下がります", r(r2)));
    if i > 0.001 {
        o.cautions.push("分圧に 1 mA 以上流れ続けます。電池駆動なら R1・R2 を 10 倍にして電流を減らすのも手です (その代わり負荷の影響を受けやすくなります)".into());
    }
    o.cautions.push("分圧は電源には使えません。大きな電流を取り出すとすぐ電圧が落ちます。電力が必要ならレギュレータを使いましょう".into());
    o.faults = vec![
        "負荷をつないだ: 負荷が R2 と並列になり、出力電圧が設計より下がります (「負荷効果」。DC解析で確かめられます)".into(),
        "R1 が断線 (オープン): 出力が 0 V になります。R2 が断線なら出力が入力電圧まで上がり、後ろの回路を壊すことがあります".into(),
        "R1 か R2 がショート: 出力が入力電圧または 0 V に張り付きます".into(),
        "抵抗の誤差 (±5 % など) で出力もずれます。「温度とばらつき」のばらつき解析で何 V ずれるか見られます".into(),
    ];
    Ok(o)
}

/// RC low-pass (or high-pass) filter: resistor for a given capacitor and cutoff.
pub fn rc_filter(fc: f64, c: f64) -> Result<CalcOut, String> {
    let (fc, c) = (pos("カットオフ周波数", fc)?, pos("コンデンサ", c)?);
    let rx = 1.0 / (2.0 * std::f64::consts::PI * fc * c);
    let rs = e24(rx, false);
    let real = 1.0 / (2.0 * std::f64::consts::PI * rs * c);
    let mut o = CalcOut { primary: Some(format_value(rs)), ..Default::default() };
    o.rows = vec![
        ("計算上のR".into(), r(rx)),
        ("使うR (E24)".into(), r(rs)),
        ("実際のカットオフ".into(), format!("{}Hz", format_value(real))),
        ("時定数 τ = RC".into(), format!("{}s", format_value(rs * c))),
    ];
    o.steps = vec![
        "カットオフ周波数 fc = 1 ÷ (2π × R × C)  (ここでゲインが約 0.7 倍 = −3 dB)".into(),
        format!("R = 1 ÷ (2π × {}Hz × {}F) = {}", format_value(fc), format_value(c), r(rx)),
    ];
    if !(1e3..=1e6).contains(&rs) {
        o.cautions.push("抵抗は 1 kΩ〜1 MΩ に収めると扱いやすくなります。外れるときはコンデンサの値を変えて計算し直しましょう (R が小さいと前段の負担が大きく、大きいとノイズを拾いやすい)".into());
    }
    o.cautions.push("後ろにつなぐ回路の入力抵抗が R の 10 倍以上ないと、カットオフがずれます".into());
    o.cautions.push("ローパスは「R を直列・C を GND 側」、ハイパスは「C を直列・R を GND 側」です。同じ式でカットオフが決まります".into());
    o.faults = vec![
        "コンデンサの容量抜けや誤差 (セラミックは ±20 % や、電圧で容量が減るものもあります): カットオフが動きます".into(),
        "C が断線: ローパスは何も減衰しません。C がショート: 出力が GND に落ちます".into(),
        "電解コンデンサを逆向きにつなぐ: 発熱・破裂の恐れがあります。極性に注意".into(),
    ];
    Ok(o)
}

/// 78xx-style linear regulator: capacitors and heat.
pub fn regulator(vin: f64, vout: f64, i_ma: f64) -> Result<CalcOut, String> {
    let (vin, vout, i) = (pos("入力電圧", vin)?, pos("出力電圧", vout)?, pos("出力電流", i_ma)? / 1000.0);
    if vin <= vout {
        return Err("入力電圧は出力電圧より高くする必要があります (78xx 系はあと 2〜3 V 以上の余裕が必要です)".into());
    }
    let p = (vin - vout) * i;
    let mut o = CalcOut { primary: Some("100n".into()), ..Default::default() };
    o.rows = vec![
        ("入力側コンデンサ".into(), "0.33 µF 以上 (セラミック 330n〜10µ)".into()),
        ("出力側コンデンサ".into(), "0.1 µF 以上 (100n。電解 10µ〜100µ を追加すると負荷変動に強い)".into()),
        ("レギュレータの発熱".into(), format!("{p:.2} W")),
        ("効率 (出力 ÷ 入力)".into(), format!("{:.0} %", vout / vin * 100.0)),
    ];
    o.steps = vec![
        format!("リニアレギュレータは余った電圧を熱にします: 発熱 = (Vin − Vout) × I = ({vin} − {vout}) × {i_ma} mA = {p:.2} W"),
        "コンデンサは、入力側は配線が長いときの発振防止、出力側は負荷が急に変わったときの電圧の揺れ抑制が目的です (データシート推奨値を優先)".into(),
    ];
    if vin - vout < 2.0 {
        o.cautions.push("入力と出力の差が 2 V 未満です。78xx 系は差が小さいと出力が下がります。低ドロップアウト (LDO) 品を検討してください".into());
    }
    if p > 1.0 {
        o.cautions.push("発熱が 1 W を超えます。TO-220 でも放熱板がほぼ必須です。入力電圧を下げる、スイッチングレギュレータにする、などを検討しましょう".into());
    } else if p > 0.5 {
        o.cautions.push("発熱が 0.5 W を超えます。小さなパッケージでは熱くなるので、触って確かめましょう".into());
    }
    if vin > 35.0 {
        o.cautions.push("78xx の入力は 35 V が上限の品が多いです。データシートで最大入力電圧を確認してください".into());
    }
    o.cautions.push("コンデンサはレギュレータの足のすぐ近くに置きます。基板では配線が長いと効果が減ります".into());
    o.faults = vec![
        "出力コンデンサが無い/遠い: 発振して出力がノイズだらけになることがあります".into(),
        "放熱が足りない: 熱保護で出力が止まる/不安定になる。長く使うと寿命が縮みます".into(),
        "入力の電源より出力が高くなる (出力側に電源が残る): 逆流でレギュレータが壊れることがあります。大きな出力コンデンサがある場合は保護ダイオードを検討".into(),
        "入出力の向きを逆にする: 壊れます。ピン配置 (IN・GND・OUT) を確認".into(),
    ];
    Ok(o)
}

/// NPN transistor used as a switch: base resistor.
pub fn transistor_switch(vin: f64, ic_ma: f64, hfe: f64) -> Result<CalcOut, String> {
    let (vin, ic, hfe) = (pos("入力電圧", vin)?, pos("負荷電流", ic_ma)? / 1000.0, pos("hFE", hfe)?);
    let vbe = 0.7;
    if vin <= vbe {
        return Err("入力電圧が 0.7 V 以下ではトランジスタが ON になりません。入力電圧を上げてください".into());
    }
    // switch use: drive the base harder than the ideal so the transistor saturates
    let beta = (hfe / 5.0).clamp(1.0, 20.0);
    let ib = ic / beta;
    let rx = (vin - vbe) / ib;
    let rs = e24(rx, false);
    let real_ib = (vin - vbe) / rs;
    let mut o = CalcOut { primary: Some(format_value(rs)), ..Default::default() };
    o.rows = vec![
        ("ベース電流の目標".into(), format!("{:.2} mA", ib * 1000.0)),
        ("計算上のRb".into(), r(rx)),
        ("使うRb (E24)".into(), r(rs)),
        ("実際のベース電流".into(), format!("{:.2} mA", real_ib * 1000.0)),
    ];
    o.steps = vec![
        format!("スイッチとして使うので、確実に ON (飽和) させるため、増幅率を {beta:.0} 倍まで控えめに見て設計します (hFE が {hfe} でも、そこまで頼りません)"),
        format!("ベース電流 Ib = Ic ÷ {beta:.0} = {:.2} mA", ib * 1000.0),
        format!("Rb = (Vin − Vbe) ÷ Ib = ({vin} − {vbe}) ÷ {:.2} mA = {}", ib * 1000.0, r(rx)),
    ];
    if ic > 0.1 {
        o.cautions.push("負荷電流が 100 mA を超えます。2N3904 などの小信号品では足りません。定格 (コレクタ電流) を確認するか、MOSFET を使いましょう".into());
    }
    o.cautions.push("リレーやモーターなどコイル負荷には、逆起電力を逃がすダイオードを負荷に並列に入れます (向きは電源側がカソード)".into());
    o.cautions.push("マイコンのピンから駆動するなら、ピンが出せる電流 (多くは 20 mA 前後) 以下に収めます".into());
    o.faults = vec![
        "Rb を入れない: ベースに大電流が流れ、トランジスタや駆動側のピンが壊れます".into(),
        "Rb が大きすぎる: 十分に ON にならず (飽和しない)、トランジスタが熱くなり負荷も動きません".into(),
        "ベースがオープン (入力が浮く): ノイズで勝手に ON/OFF します。ベースと GND の間に 10k〜100k を入れると安定します".into(),
        "コイル負荷のダイオードが無い: OFF 時の高電圧でトランジスタが壊れます".into(),
        "エミッタとコレクタを逆につなぐ: 増幅率が極端に低く、正しく動きません".into(),
    ];
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn e24_rounding() {
        assert_eq!(e24(100.0, false), 100.0);
        assert_eq!(e24(312.0, true), 330.0);
        assert_eq!(e24(312.0, false), 300.0);
        assert_eq!(e24(4800.0, false), 4700.0);
        assert_eq!(e24(0.95e-3, true), 1.0e-3);
    }

    #[test]
    fn led_resistor_for_a_red_led_on_5v() {
        let o = led_resistor(5.0, 2.0, 10.0).unwrap();
        assert_eq!(o.primary.as_deref(), Some("300"));
        assert!(!o.faults.is_empty() && !o.steps.is_empty());
        // a 3 V supply cannot light a 3.2 V LED
        assert!(led_resistor(3.0, 3.2, 10.0).is_err());
        // too much current is called out
        assert!(led_resistor(5.0, 2.0, 30.0).unwrap().cautions.iter().any(|c| c.contains("20 mA")));
    }

    #[test]
    fn divider_gives_the_requested_voltage() {
        let o = divider(5.0, 3.3, 10e3).unwrap();
        let r1 = parse_value(o.primary.as_deref().unwrap()).unwrap();
        let v = 5.0 * 10e3 / (r1 + 10e3);
        assert!((v - 3.3).abs() < 0.1, "{v}");
        assert!(divider(3.3, 5.0, 1e3).is_err());
    }

    #[test]
    fn rc_filter_hits_the_cutoff() {
        let o = rc_filter(1000.0, 100e-9).unwrap();
        let r = parse_value(o.primary.as_deref().unwrap()).unwrap();
        let fc = 1.0 / (2.0 * std::f64::consts::PI * r * 100e-9);
        assert!((fc / 1000.0 - 1.0).abs() < 0.06, "{fc}");
    }

    #[test]
    fn regulator_warns_about_heat_and_dropout() {
        let hot = regulator(12.0, 5.0, 500.0).unwrap();
        assert!(hot.cautions.iter().any(|c| c.contains("1 W")), "{:?}", hot.cautions);
        let tight = regulator(6.0, 5.0, 10.0).unwrap();
        assert!(tight.cautions.iter().any(|c| c.contains("LDO")));
        assert!(regulator(5.0, 5.0, 10.0).is_err());
    }

    #[test]
    fn transistor_base_resistor_saturates() {
        let o = transistor_switch(5.0, 20.0, 100.0).unwrap();
        let rb = parse_value(o.primary.as_deref().unwrap()).unwrap();
        let ib = (5.0 - 0.7) / rb;
        assert!(ib * 100.0 > 20e-3 * 2.0, "base current {ib} too small to saturate");
        assert!(transistor_switch(0.5, 20.0, 100.0).is_err());
    }
}
