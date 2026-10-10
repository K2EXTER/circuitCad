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

/// Heat of a linear regulator (or any part that burns (Vin - Vout) x I): junction temperature and the heat sink it needs.
pub fn heat_design(vin: f64, vout: f64, i_ma: f64, theta_ja: f64, theta_jc: f64, t_amb: f64, t_jmax: f64) -> Result<CalcOut, String> {
    let (vin, vout, i) = (pos("入力電圧", vin)?, pos("出力電圧", vout)?, pos("電流", i_ma)? / 1000.0);
    let (theta_ja, theta_jc) = (pos("熱抵抗 θja", theta_ja)?, pos("熱抵抗 θjc", theta_jc)?);
    if vin <= vout {
        return Err("入力電圧は出力電圧より高くしてください (差が熱になります)".into());
    }
    if t_jmax <= t_amb {
        return Err(format!("最大接合温度 ({t_jmax} ℃) は周囲温度 ({t_amb} ℃) より高くしてください"));
    }
    let p = (vin - vout) * i;
    let rise = p * theta_ja;
    let tj = t_amb + rise;
    // keep 20 % below the maximum junction temperature
    let tj_goal = t_jmax - 0.2 * (t_jmax - t_amb);
    let p_free = (tj_goal - t_amb) / theta_ja;
    let theta_cs = 1.0;
    let theta_total = (tj_goal - t_amb) / p;
    let theta_sa = theta_total - theta_jc - theta_cs;
    let mut o = CalcOut::default();
    o.rows = vec![
        ("発熱 P".into(), format!("{p:.2} W")),
        ("温度上昇 (放熱板なし)".into(), format!("{rise:.0} ℃")),
        ("接合部の温度 Tj".into(), format!("{tj:.0} ℃ (上限 {t_jmax:.0} ℃)")),
        ("放熱板なしで使える電力".into(), format!("{p_free:.2} W")),
    ];
    o.steps = vec![
        format!("発熱 P = (Vin − Vout) × I = ({vin} − {vout}) × {i_ma} mA = {p:.2} W"),
        format!("温度上昇 = P × θja = {p:.2} × {theta_ja} = {rise:.0} ℃。Tj = 周囲 {t_amb} ℃ + {rise:.0} = {tj:.0} ℃"),
        format!("余裕を見て、Tj を {tj_goal:.0} ℃ (上限の手前 2 割) 以下にしたい。放熱板なしで使えるのは P ≦ ({tj_goal:.0} − {t_amb}) ÷ {theta_ja} = {p_free:.2} W"),
    ];
    if tj <= tj_goal {
        o.rows.push(("判定".into(), "放熱板なしで使えます".into()));
    } else if theta_sa > 0.0 {
        o.rows.push(("必要な放熱板の熱抵抗".into(), format!("{theta_sa:.1} ℃/W 以下")));
        o.steps.push(format!(
            "放熱板をつけると、熱は 接合部 → ケース (θjc {theta_jc}) → 放熱板 (取り付け {theta_cs}) → 空気 の順に逃げる。全体 {theta_total:.1} ℃/W 以下にするので、放熱板は {theta_total:.1} − {theta_jc} − {theta_cs} = {theta_sa:.1} ℃/W 以下"
        ));
        o.cautions.push("放熱板をつけても足りないほどの熱です。入力電圧を下げる、スイッチングレギュレータにする、電流を減らす、のほうが現実的です".into());
    } else {
        o.rows.push(("判定".into(), "放熱板をつけても足りません".into()));
        o.cautions.push("熱が大きすぎて、放熱板でも間に合いません。入力電圧を下げる (差を小さくする)、スイッチングレギュレータにする、のどちらかです".into());
    }
    if tj > t_jmax {
        o.cautions.push(format!("放熱板なしだと Tj が上限 {t_jmax:.0} ℃ を超えます。多くのレギュレータは、熱保護で出力が止まります"));
    }
    o.cautions.push("θja は、基板の銅箔の広さや風で大きく変わります。データシートの値は目安として使い、実際は温度を測って確かめます".into());
    o.faults = vec![
        "放熱が足りない: 熱保護で出力が止まる、止まっては復帰を繰り返す (出力が震える)。保護がない部品は壊れる".into(),
        "放熱板の取り付けがゆるい/グリスなし: θcs が大きくなって温度が下がらない".into(),
        "周囲が高温 (箱の中): 周囲温度の分だけ Tj が上がる。25 ℃ でなく 40〜50 ℃ で見積もる".into(),
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

/// Smoothing capacitor after a full-wave bridge rectifier.
pub fn smoothing_cap(vrms: f64, i_ma: f64, freq: f64, ripple: f64) -> Result<CalcOut, String> {
    let (vrms, i, f, dv) = (pos("トランスの出力 (実効値)", vrms)?, pos("負荷電流", i_ma)? / 1000.0, pos("電源周波数", freq)?, pos("許容リップル", ripple)?);
    let vpk = vrms * std::f64::consts::SQRT_2;
    let vmax = vpk - 1.4; // two diodes of the bridge conduct at a time
    if dv >= vmax / 2.0 {
        return Err(format!("許容リップル ({dv} V) が大きすぎます。直流の電圧 (約 {vmax:.1} V) の半分より小さくしてください"));
    }
    let cx = i / (2.0 * f * dv);
    let c = e24(cx, true);
    let vmean = vmax - dv / 2.0;
    let rating = [6.3, 10.0, 16.0, 25.0, 35.0, 50.0, 63.0, 100.0, 160.0, 250.0].into_iter().find(|v| *v >= vpk * 1.25).unwrap_or(400.0);
    let mut o = CalcOut { primary: Some(format_value(c)), ..Default::default() };
    o.rows = vec![
        ("計算上の容量".into(), format!("{}F", format_value(cx))),
        ("使う容量 (E24, 切り上げ)".into(), format!("{}F", format_value(c))),
        ("直流電圧の目安 (平均)".into(), format!("{vmean:.1} V")),
        ("コンデンサの耐圧".into(), format!("{rating:.0} V 以上 (ピーク {vpk:.1} V の 1.25 倍以上)")),
        ("ダイオードの逆耐圧".into(), format!("{:.0} V 以上 (余裕を見て ピークの 2 倍以上)", vpk * 2.0)),
    ];
    o.steps = vec![
        format!("交流のピーク電圧 = 実効値 × √2 = {vrms} × 1.414 = {vpk:.1} V。ブリッジはダイオード2個ぶん (約 1.4 V) 下がるので、直流の最大は約 {vmax:.1} V"),
        "ブリッジ整流 (全波) のリップルの周波数は電源の 2 倍。山と山の間の時間は 1 ÷ (2 × f)".into(),
        format!("その間、コンデンサだけが電流 I を出します: ΔV = I × 時間 ÷ C から C = I ÷ (2 × f × ΔV) = {i} ÷ (2 × {f} × {dv}) = {}F", format_value(cx)),
        "式は少し大きめに出ます (ダイオードが電流を流す時間があるため)。そのぶん安全側です".into(),
    ];
    if c * vpk * 2.0 * f > 5.0 {
        o.cautions.push("容量が大きいので、電源を入れた瞬間に大きな突入電流が流れます。ヒューズが切れたりダイオードが傷んだりしないよう、ダイオードの定格 (サージ電流) とヒューズ (スローブロー) を確認しましょう".into());
    }
    o.cautions.push(format!("リップル電流で電解コンデンサが発熱します。負荷電流 {i_ma} mA の 1.5〜2 倍のリップル電流を流せる品 (データシートの許容リップル電流) を選びます"));
    o.cautions.push("電解コンデンサには極性があります。+ を電圧の高いほうにつなぎます。逆につなぐと発熱・破裂のおそれがあります".into());
    o.cautions.push("負荷がかかると、トランスの巻線抵抗の分だけ電圧は下がります (レッスン「交流を直流にする」で確認できます)。安定した電圧が必要なら、このあとにレギュレータを入れます".into());
    o.faults = vec![
        "ダイオード1個が断線: 半波整流になり、リップルが約 2 倍になります (周波数は電源と同じ)。".into(),
        "ダイオード1個がショート: 交流の半周期ごとにトランスの出力が短絡され、大電流・発熱・ヒューズ断になります。".into(),
        "コンデンサの容量抜け/ESR の増加 (電解コンデンサの劣化): リップルが増え、後ろの回路が誤動作します。古い機器で最初に壊れやすい部品です。".into(),
        "コンデンサが断線: 平滑されず、脈流 (全波整流の山の連なり) のままになります。".into(),
        "コンデンサのショート: 出力が 0 V になり、ダイオードとトランスに過電流が流れます。".into(),
    ];
    Ok(o)
}

/// How long a battery lasts and how far its internal resistance pulls the voltage down.
pub fn battery_life(cap_mah: f64, i_ma: f64, rint: f64, volts: f64, usable_pct: f64) -> Result<CalcOut, String> {
    let (cap, i, rint, v, pct) = (pos("電池の容量", cap_mah)?, pos("負荷の電流", i_ma)?, pos("内部抵抗", rint)?, pos("電池の電圧", volts)?, pos("使える割合", usable_pct)?);
    if pct > 100.0 {
        return Err("使える割合は 100 % 以下にしてください".into());
    }
    let hours = cap * pct / 100.0 / i;
    let drop = i / 1000.0 * rint;
    let mut o = CalcOut::default();
    let days = hours / 24.0;
    o.rows = vec![
        ("持ち時間の目安".into(), if hours >= 48.0 { format!("{hours:.0} 時間 (約 {days:.1} 日)") } else { format!("{hours:.1} 時間") }),
        ("内部抵抗での電圧降下".into(), format!("{:.2} V", drop)),
        ("電流を流したときの端子電圧".into(), format!("{:.2} V", v - drop)),
        ("電池が出す電力".into(), format!("{:.1} mW", i * (v - drop))),
    ];
    o.steps = vec![
        format!("持ち時間 = 容量 × 使える割合 ÷ 電流 = {cap} mAh × {pct} % ÷ {i} mA = {hours:.1} 時間"),
        format!("内部抵抗の電圧降下 = 電流 × 内部抵抗 = {} A × {rint} Ω = {drop:.2} V (そのぶん、負荷にかかる電圧が下がります)", format_value(i / 1000.0)),
    ];
    if drop > v * 0.1 {
        o.cautions.push(format!("電圧降下が電池の電圧の {:.0} % あります。内部抵抗の大きい電池 (コイン電池など) に、これだけの電流を流すのは向きません。電流を減らすか、内部抵抗の小さい電池 (アルカリ・リチウムイオン) にしましょう", drop / v * 100.0));
    }
    if pct > 85.0 {
        o.cautions.push("使える割合が高めです。電流が大きいほど、電池の容量は表示より減ります (データシートの容量は小さい電流のときの値)。余裕を見て 50〜80 % で見積もりましょう".into());
    }
    o.cautions.push("電池の電圧は、使うにつれて下がります。回路が動く最低電圧 (たとえば 3.3V のマイコンなら 2.7V 前後) になったら「空」と考えます。アルカリ電池は 1.5V → 0.9V、コイン電池は 3V → 2V、リチウムイオンは 4.2V → 3.0V が目安です".into());
    o.cautions.push("低温では、容量が減って内部抵抗が増えます。屋外で使う機器は、余裕を多めに見ます".into());
    o.faults = vec![
        "電池を逆向きに入れる: 多くの回路で動きません。ダイオードを入れて守ると、そのぶん電圧が下がります".into(),
        "電池をショートする: 内部抵抗だけで電流が決まり、大きな電流で発熱・膨張・破裂のおそれがあります (特にリチウムイオン)。電池の近くにヒューズや保護回路を入れます".into(),
        "古い電池と新しい電池を混ぜる: 古いほうが先に空になり、逆に充電されたり、液漏れしたりします".into(),
        "電池が弱ってきた: 内部抵抗が増え、負荷をつなぐと電圧が大きく下がって、無負荷では元気そうに見えます (電圧だけでは残量が分かりません)".into(),
    ];
    Ok(o)
}

/// Zener shunt regulator: resistor, and the power in the resistor and the zener.
pub fn zener_regulator(vin: f64, vz: f64, i_load_ma: f64, iz_min_ma: f64) -> Result<CalcOut, String> {
    let (vin, vz, il, izm) = (pos("入力電圧", vin)?, pos("ツェナー電圧", vz)?, pos("負荷電流 (最大)", i_load_ma)? / 1000.0, pos("ツェナーに残す電流", iz_min_ma)? / 1000.0);
    if vin <= vz {
        return Err("入力電圧はツェナー電圧より高くしてください (差が数 V 以上あると安定します)".into());
    }
    let rx = (vin - vz) / (il + izm);
    let rs = e24(rx, false);
    // with the load removed, all the current flows through the zener
    let i_all = (vin - vz) / rs;
    let i_min = i_all - il;
    let (p_r, p_z) = ((vin - vz) * i_all, vz * i_all);
    let mut o = CalcOut { primary: Some(format_value(rs)), ..Default::default() };
    o.rows = vec![
        ("計算上の抵抗".into(), r(rx)),
        ("使う抵抗 (E24)".into(), r(rs)),
        ("負荷最大のときツェナーに流れる電流".into(), format!("{:.1} mA", i_min * 1000.0)),
        ("負荷なしのときツェナーに流れる電流".into(), format!("{:.1} mA", i_all * 1000.0)),
        ("抵抗の発熱".into(), format!("{:.0} mW", p_r * 1000.0)),
        ("ツェナーの発熱 (負荷なし)".into(), format!("{:.0} mW", p_z * 1000.0)),
    ];
    o.steps = vec![
        "ツェナーダイオードは、逆向きに電圧をかけると一定の電圧 (ツェナー電圧) で電流を通します。抵抗で電流を制限して、その電圧を取り出します".into(),
        format!("抵抗の電流 = 負荷電流 + ツェナーに残す電流。R = (Vin − Vz) ÷ (I負荷 + Iz) = ({vin} − {vz}) ÷ {} = {}", format_value(il + izm), r(rx)),
        format!("負荷を外すと、電流がすべてツェナーに流れます: {:.1} mA。そのときの発熱 = Vz × I = {:.0} mW", i_all * 1000.0, p_z * 1000.0),
    ];
    if i_min < izm * 0.9 {
        o.cautions.push("負荷が最大のとき、ツェナーに十分な電流が残りません。電圧がずれます。抵抗を小さくするか、入力電圧を上げましょう".into());
    }
    if p_z > 0.4 {
        o.cautions.push("ツェナーの発熱が 0.4 W を超えます。一般的な 0.5 W 品では余裕が少ないので、1 W 品にするか、入力電圧を下げましょう".into());
    }
    if p_r > 0.2 {
        o.cautions.push("抵抗の発熱が 0.2 W を超えます。1/2 W 以上の抵抗にしましょう".into());
    }
    o.cautions.push("ツェナーの電圧は電流や温度で少しずれます (数 %)。精度が必要なら、リニアレギュレータを使います".into());
    o.cautions.push("大きな電流には向きません。電流が大きく変わる負荷には、ツェナーの後ろにトランジスタを付けるか、レギュレータ IC を使います。電力のほとんどを抵抗とツェナーが熱にします".into());
    o.faults = vec![
        "ツェナーを逆向きにつなぐ: 普通のダイオードとして 0.7 V で導通し、出力が 0.7 V になります".into(),
        "ツェナーが断線: 電圧が安定せず、入力電圧 (または負荷で決まる電圧) が、そのまま負荷にかかります".into(),
        "ツェナーがショート: 出力が 0 V になり、抵抗に大きな電流が流れて発熱します".into(),
        "抵抗が大きすぎる: 負荷が重いときにツェナーが働かず、電圧が下がります".into(),
    ];
    Ok(o)
}

/// Step-down (buck) converter: duty cycle, inductor and output capacitor from the ripple you accept.
pub fn buck_design(vin: f64, vout: f64, i_out_ma: f64, fsw_khz: f64, ripple_pct: f64, vripple_mv: f64) -> Result<CalcOut, String> {
    let (vin, vout, iout, f, rp, dv) = (pos("入力電圧", vin)?, pos("出力電圧", vout)?, pos("出力電流", i_out_ma)? / 1000.0, pos("スイッチング周波数", fsw_khz)? * 1000.0, pos("コイル電流のリップル", ripple_pct)? / 100.0, pos("出力電圧のリップル", vripple_mv)? / 1000.0);
    if vout >= vin {
        return Err("出力電圧は入力電圧より低くしてください。降圧コンバータは、電圧を下げるだけです".into());
    }
    let d = vout / vin;
    let di = rp * iout;
    let lx = (vin - vout) * d / (f * di);
    let l = e24(lx, true);
    let di_real = (vin - vout) * d / (f * l);
    let cx = di_real / (8.0 * f * dv);
    let c = e24(cx, true);
    let peak = iout + di_real / 2.0;
    let mut o = CalcOut { primary: Some(format_value(l)), ..Default::default() };
    o.rows = vec![
        ("デューティ比 (ONの割合)".into(), format!("{:.0} %", d * 100.0)),
        ("コイル (E24, 切り上げ)".into(), format!("{}H  (計算上 {}H)", format_value(l), format_value(lx))),
        ("そのときのコイル電流の揺れ".into(), format!("{:.0} mA (p-p)", di_real * 1000.0)),
        ("コイルに流れる最大の電流".into(), format!("{:.2} A  (コイルの定格はこれ以上に)", peak)),
        ("出力コンデンサ (E24, 切り上げ)".into(), format!("{}F 以上 (ESR の小さいもの)", format_value(c))),
        ("ダイオードの電流 (平均)".into(), format!("{:.0} mA", iout * (1.0 - d) * 1000.0)),
    ];
    o.steps = vec![
        format!("デューティ比 D = Vout ÷ Vin = {vout} ÷ {vin} = {:.2}", d),
        format!("コイル: L = (Vin − Vout) × D ÷ (f × ΔI)。ΔI は出力電流の {:.0} % = {:.0} mA とすると、L = {:.1} V × {:.2} ÷ ({} Hz × {:.3} A) = {}H", rp * 100.0, di * 1000.0, vin - vout, d, format_value(f), di, format_value(lx)),
        format!("出力コンデンサ: C = ΔI ÷ (8 × f × ΔV) = {:.3} A ÷ (8 × {} Hz × {} mV) = {}F", di_real, format_value(f), dv * 1000.0, format_value(cx)),
    ];
    if rp < 0.15 {
        o.cautions.push("コイル電流の揺れを小さくしすぎると、大きなコイルが必要で、応答も遅くなります。出力電流の 20〜40 % が定番です".into());
    }
    if rp > 0.6 {
        o.cautions.push("コイル電流の揺れが大きいと、軽負荷で電流が 0 まで下がる (不連続) ことがあり、リップルも増えます。20〜40 % が定番です".into());
    }
    o.cautions.push("コイルは、最大の電流で飽和しない (インダクタンスが落ちない) 定格電流のものにします。飽和すると電流が急に増え、スイッチやダイオードを壊します".into());
    o.cautions.push("ダイオードはショットキー (順方向電圧が小さい) を使い、逆耐圧は入力電圧より十分高く、電流はコイルの最大電流以上にします。ダイオードの損失が、効率を左右します".into());
    o.cautions.push("出力コンデンサは、容量だけでなく ESR (内部抵抗) でもリップルが決まります (ESR × コイル電流の揺れ)。ESR の小さいセラミックか低ESRの電解を選びます".into());
    o.cautions.push("基板では、スイッチ → ダイオード → コイルの電流が速く流れる輪を小さくし、配線を短く太くします (ノイズと損失を減らします)".into());
    o.faults = vec![
        "ダイオードの付け忘れ/断線: コイルの電流の逃げ場がなくなり、スイッチがOFFになった瞬間に SW の電圧が跳ね上がって、IC が壊れます".into(),
        "ダイオードを逆向きにつなぐ: 常に短絡のようになり、スイッチに大電流が流れます (発熱・破損)".into(),
        "コイルが飽和する・小さすぎる: 電流が大きく揺れ、ピーク電流で保護が働いたり、素子が壊れたりします".into(),
        "出力コンデンサの容量抜け (劣化): リップルが増え、出力が不安定になります。電解コンデンサは熱で劣化します".into(),
        "フィードバック分圧の抵抗が断線: FB の電圧が分からず、出力が入力電圧近くまで上がることがあります (出力に付ける回路を壊します)".into(),
    ];
    Ok(o)
}

/// Step-up (boost) converter: duty cycle, inductor and output capacitor, assuming 85 % efficiency.
pub fn boost_design(vin: f64, vout: f64, i_out_ma: f64, fsw_khz: f64, ripple_pct: f64, vripple_mv: f64) -> Result<CalcOut, String> {
    let (vin, vout, iout, f, rp, dv) = (pos("入力電圧", vin)?, pos("出力電圧", vout)?, pos("出力電流", i_out_ma)? / 1000.0, pos("スイッチング周波数", fsw_khz)? * 1000.0, pos("コイル電流のリップル", ripple_pct)? / 100.0, pos("出力電圧のリップル", vripple_mv)? / 1000.0);
    if vout <= vin {
        return Err("出力電圧は入力電圧より高くしてください。昇圧コンバータは、電圧を上げるだけです (下げるなら降圧コンバータ)".into());
    }
    const EFF: f64 = 0.85;
    let d = 1.0 - vin * EFF / vout;
    let iin = iout * vout / (vin * EFF);
    let di = rp * iin;
    let lx = vin * d / (f * di);
    let l = e24(lx, true);
    let di_real = vin * d / (f * l);
    let peak = iin + di_real / 2.0;
    let cx = iout * d / (f * dv);
    let c = e24(cx, true);
    let mut o = CalcOut { primary: Some(format_value(l)), ..Default::default() };
    o.rows = vec![
        ("デューティ比 (ONの割合)".into(), format!("{:.0} %", d * 100.0)),
        ("入力側に流れる平均電流".into(), format!("{:.2} A  (出力電流の {:.1} 倍)", iin, iin / iout)),
        ("コイル (E24, 切り上げ)".into(), format!("{}H  (計算上 {}H)", format_value(l), format_value(lx))),
        ("コイルに流れる最大の電流".into(), format!("{:.2} A  (コイル・スイッチの定格はこれ以上に)", peak)),
        ("出力コンデンサ (E24, 切り上げ)".into(), format!("{}F 以上 (ESR の小さいもの)", format_value(c))),
    ];
    o.steps = vec![
        format!("電力はほぼ保存されるので、入力電流は出力電流より大きくなります: Iin = Iout × Vout ÷ (Vin × 効率 {EFF}) = {:.2} A", iin),
        format!("デューティ比 D = 1 − Vin × 効率 ÷ Vout = 1 − {vin} × {EFF} ÷ {vout} = {:.2}", d),
        format!("コイル: L = Vin × D ÷ (f × ΔI)。ΔI は入力電流の {:.0} % = {:.0} mA とすると、L = {}H", rp * 100.0, di * 1000.0, format_value(lx)),
        format!("出力コンデンサ: C = Iout × D ÷ (f × ΔV) = {:.3} A × {:.2} ÷ ({} Hz × {} mV) = {}F", iout, d, format_value(f), dv * 1000.0, format_value(cx)),
    ];
    if d > 0.8 {
        o.cautions.push("デューティ比が 80 % を超えます。昇圧の比が大きすぎて、効率が落ち、IC の限界を超えることがあります。昇圧の比は 4〜5 倍くらいまでが目安です".into());
    }
    if iin > 3.0 {
        o.cautions.push("入力側の電流が 3 A を超えます。コイル・スイッチ・配線をそれに合わせて太くし、入力電源が出せるかも確認しましょう".into());
    }
    o.cautions.push("昇圧は、入力電圧が出力に素通しになる経路 (コイル → ダイオード) があります。出力の負荷が短絡すると、電流を止められません。ヒューズや過電流保護を検討します".into());
    o.cautions.push("電池で使うときは、電池の電圧が下がると入力電流が増えます。電池が減った状態 (電圧が低いとき) でも、計算が成り立つか確認します".into());
    o.cautions.push("ダイオードはショットキーにし、逆耐圧は出力電圧より十分高くします。コイルは最大の電流で飽和しない定格のものを選びます".into());
    o.faults = vec![
        "コイルが飽和する・小さすぎる: 電流が急に増え、スイッチやダイオードを壊したり、IC の保護が働いて出力が不安定になったりします".into(),
        "ダイオードの断線/逆向き: 出力に電気が出ません。コイルのエネルギーの逃げ場がなく、スイッチの電圧が跳ね上がって壊れます".into(),
        "出力コンデンサの容量抜け: リップルが増えます。昇圧は出力電流が途切れ途切れなので、降圧よりリップルが大きく出ます".into(),
        "負荷を短絡する: 入力電源からコイルとダイオード経由で直接電流が流れ続けます (降圧と違い、スイッチを切っても止まりません)".into(),
    ];
    Ok(o)
}

/// N-channel MOSFET low-side switch: conduction loss, gate resistor and pull-down.
pub fn mosfet_switch(vdrive: f64, vth: f64, i_ma: f64, rds_mohm: f64, vload: f64) -> Result<CalcOut, String> {
    let (vg, vth, i, rds, vl) = (pos("ゲートの駆動電圧", vdrive)?, pos("しきい値電圧", vth)?, pos("負荷の電流", i_ma)? / 1000.0, pos("オン抵抗", rds_mohm)? / 1000.0, pos("負荷の電源電圧", vload)?);
    if vg <= vth {
        return Err(format!("ゲートの駆動電圧 ({vg} V) がしきい値 ({vth} V) 以下です。これではオンになりません。ゲートを上げるか、しきい値の低い (ロジックレベル) MOSFET を選びましょう"));
    }
    let vds = i * rds;
    let p = i * i * rds;
    let mut o = CalcOut { primary: Some("100".into()), ..Default::default() };
    o.rows = vec![
        ("オンのときのドレイン電圧の降下".into(), format!("{:.3} V", vds)),
        ("MOSFET の発熱 (導通損失)".into(), format!("{:.0} mW", p * 1000.0)),
        ("ゲート抵抗".into(), "100Ω 前後 (47〜330Ω)".into()),
        ("ゲートのプルダウン抵抗".into(), "10kΩ (ゲートと GND の間)".into()),
        ("負荷にかかる電圧".into(), format!("{:.2} V  (電源 {vl} V − 降下)", vl - vds)),
    ];
    o.steps = vec![
        format!("オンのときの降下 = I × Rds(on) = {} A × {} Ω = {:.3} V", i, rds, vds),
        format!("発熱 = I² × Rds(on) = {} A² × {} Ω = {:.0} mW", i * i, rds, p * 1000.0),
        "ゲート抵抗: マイコンの出力と MOSFET のゲート容量の間に入れて、立ち上がりの電流と振動 (リンギング) を抑えます。小さな MOSFET なら 100Ω 前後で十分です".into(),
        "プルダウン: マイコンが起動する前や入力が浮いているときに、ゲートを 0V に引いて、勝手にオンにならないようにします".into(),
    ];
    if vg < 1.5 * vth {
        o.cautions.push(format!("ゲート電圧 {vg} V がしきい値 {vth} V の 1.5 倍に届きません。しきい値のぎりぎりでは、オン抵抗がデータシートの値 (多くはゲート 4.5〜10 V のときの値) より大きくなり、発熱します。データシートの「Rds(on) at Vgs=…」の条件を確認します"));
    }
    if p > 0.25 {
        o.cautions.push("発熱が 0.25 W を超えます。小さな面実装・TO-92 では熱くなるので、オン抵抗の小さい MOSFET を選ぶか、放熱を考えます".into());
    }
    o.cautions.push("誘導性の負荷 (リレー・モーター・ソレノイド) には、負荷と並列に、ダイオードを逆向きに入れます。スイッチが切れる瞬間に出る高い電圧で、MOSFET が壊れます".into());
    o.cautions.push("ドレインの最大電圧 Vds・連続電流 Id は、電源電圧と負荷電流に余裕を持たせます。MOSFET によっては、ボディダイオードで逆向きにも電流が流れます".into());
    o.faults = vec![
        "プルダウン抵抗がない: 起動時にゲートが浮き、負荷が勝手にオン/オフしたり、MOSFET が中途半端にオンになって発熱したりします".into(),
        "ゲート電圧が足りない (しきい値ぎりぎり): 中途半端にオンになり、オン抵抗が大きいまま、大きく発熱します".into(),
        "誘導性負荷の保護ダイオードがない: オフの瞬間に出る高い電圧 (逆起電力) で、MOSFET が壊れます".into(),
        "ゲートの静電気/過電圧: ゲートは薄い絶縁膜で、20V (多くの品の上限) を超えたり、静電気が加わったりすると壊れます".into(),
    ];
    Ok(o)
}

/// Op-amp amplifier resistors for a target gain (non-inverting or inverting).
pub fn opamp_gain(gain: f64, rf_ohm: f64, inverting: f64, gbw_khz: f64, vsupply: f64, vin_peak_mv: f64) -> Result<CalcOut, String> {
    let (g, rf, gbw, vs, vin) = (pos("ゲイン", gain)?, pos("帰還抵抗", rf_ohm)?, pos("利得帯域幅積", gbw_khz)? * 1000.0, pos("電源電圧", vsupply)?, pos("入力の振幅", vin_peak_mv)? / 1000.0);
    let inv = inverting >= 0.5;
    if !inv && g < 1.0 {
        return Err("非反転増幅のゲインは 1 以上です (1 倍はボルテージフォロワで、R2 は不要です)。1 より小さくしたいときは、反転増幅か分圧を使います".into());
    }
    let follower = !inv && (g - 1.0).abs() < 1e-9;
    let rfs = e24(rf, false);
    let r2 = if follower { f64::INFINITY } else if inv { e24(rf / g, false) } else { e24(rf / (g - 1.0), false) };
    let g_real = if follower { 1.0 } else if inv { rfs / r2 } else { 1.0 + rfs / r2 };
    let bw = gbw / g_real;
    let vout = vin * g_real;
    let mut o = CalcOut { primary: if follower { None } else { Some(format_value(r2)) }, ..Default::default() };
    o.rows = vec![
        ("方式".into(), if inv { format!("反転増幅: ゲイン −{g}") } else if follower { "ボルテージフォロワ".to_string() } else { format!("非反転増幅: ゲイン {g}") }),
        ("帰還抵抗 Rf (E24)".into(), r(rfs)),
        ("もう一方の抵抗 R2 (E24)".into(), if follower { "不要 (R2 は外す)".into() } else { r(r2) }),
        ("実際のゲイン".into(), format!("{}{:.2} 倍 ({:.1} dB)", if inv { "−" } else { "" }, g_real, 20.0 * g_real.log10())),
        ("使える帯域 (−3dB)".into(), format!("約 {}Hz", format_value(bw))),
        ("出力の振幅".into(), format!("{:.2} V (電源 ±{vs} V)", vout)),
    ];
    o.steps = vec![
        if inv {
            "反転増幅: ゲイン = −Rf ÷ Rin。入力は Rin (= R2) を通して反転入力に入れ、Rf で出力から戻します。入力インピーダンスは Rin で決まるので、大きくしすぎないよう注意".into()
        } else {
            "非反転増幅: ゲイン = 1 + Rf ÷ R2 (R2 は反転入力と GND の間)。入力は非反転入力に入れるので、入力インピーダンスが非常に大きくなります".into()
        },
        format!("R2 = {}。E24 に丸めると {}、実際のゲインは {:.2} 倍", if inv { "Rf ÷ ゲイン" } else { "Rf ÷ (ゲイン − 1)" }, if follower { "不要".to_string() } else { r(r2) }, g_real),
        format!("帯域: 利得帯域幅積 ÷ ゲイン = {}Hz ÷ {:.1} = 約 {}Hz", format_value(gbw), g_real, format_value(bw)),
    ];
    if vout > vs - 1.5 {
        o.cautions.push(format!("出力の振幅 {vout:.2} V が、電源 ±{vs} V から 1.5 V ほど引いた範囲 (±{:.1} V くらい) を超えます。LM358 などの出力は電源まで届かず、波形の山が削れます (クリップ)。ゲインを下げるか、電源電圧を上げます", vs - 1.5));
    }
    if bw < 20_000.0 {
        o.cautions.push("使える帯域が 20 kHz より狭いです。音声を扱うなら、ゲインを分けて 2 段にするか、利得帯域幅積の大きいオペアンプにします".into());
    }
    if !(1_000.0..=1_000_000.0).contains(&rf) {
        o.cautions.push("帰還抵抗は 1kΩ〜1MΩ くらいにします。小さすぎるとオペアンプの出力に負担がかかり、大きすぎると入力のバイアス電流・ノイズ・基板の容量の影響が出ます".into());
    }
    o.cautions.push("抵抗の許容差でゲインがずれます。精度が必要なら、許容差 1% の金属皮膜を使います (「温度とばらつき」で試せます)".into());
    o.faults = vec![
        "帰還抵抗が断線: 帰還がなくなり、ゲインが非常に大きくなって、出力が電源に張り付きます".into(),
        "R2 が断線 (非反転): ゲインが 1 倍 (ボルテージフォロワ) になります".into(),
        "電源の片方がない (片電源で両電源を前提に設計): 負の入力で出力が 0 V に張り付き、波形の半分が出ません".into(),
        "容量性の負荷 (長いケーブルなど) をつなぐ: 位相余裕が減り、発振することがあります。出力に数十Ωの抵抗を直列に入れます".into(),
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

    #[test]
    fn smoothing_cap_matches_the_ripple_formula() {
        let o = smoothing_cap(12.0, 150.0, 50.0, 1.5).unwrap();
        let c = parse_value(o.primary.as_deref().unwrap()).unwrap();
        // I / (2 f dV) = 1 mF
        assert!((c / 1e-3 - 1.0).abs() < 0.1, "{c}");
        assert!(o.rows.iter().any(|(k, v)| k.contains("耐圧") && v.contains("25 V")), "{:?}", o.rows);
        assert!(smoothing_cap(12.0, 150.0, 50.0, 20.0).is_err());
        assert!(!o.faults.is_empty() && !o.cautions.is_empty());
    }

    #[test]
    fn battery_life_and_zener_regulator() {
        let o = battery_life(230.0, 9.0, 15.0, 3.0, 70.0).unwrap();
        assert!(o.rows[0].1.contains("17.9"), "{:?}", o.rows);
        assert!(o.rows[1].1.contains("0.13"), "{:?}", o.rows);
        assert!(!o.cautions.is_empty() && !o.faults.is_empty());
        assert!(battery_life(230.0, 100.0, 15.0, 3.0, 70.0).unwrap().cautions.iter().any(|c| c.contains("内部抵抗の大きい電池")));
        assert!(battery_life(230.0, 9.0, 15.0, 3.0, 120.0).is_err());
        // 12 V -> 5.1 V zener with 20 mA load and 5 mA left: R = 6.9 / 25 mA = 276 -> 270 ohm
        let z = zener_regulator(12.0, 5.1, 20.0, 5.0).unwrap();
        assert_eq!(z.primary.as_deref(), Some("270"));
        assert!(z.cautions.iter().any(|c| c.contains("ツェナーの電圧は")));
        assert!(zener_regulator(5.0, 5.1, 20.0, 5.0).is_err());
    }

    #[test]
    fn buck_design_matches_the_textbook_numbers() {
        // 12 V -> 5 V, 500 mA, 150 kHz, 40 % current ripple, 30 mV ripple
        let o = buck_design(12.0, 5.0, 500.0, 150.0, 40.0, 30.0).unwrap();
        // D = 0.4167; dI = 0.2 A; L = 7 * 0.4167 / (150k * 0.2) = 97 uH -> 100 uH
        assert_eq!(o.primary.as_deref(), Some("100u"), "{:?}", o.rows);
        assert!(o.rows[0].1.contains("42 %"), "{:?}", o.rows);
        assert!(o.rows[1].1.contains("計算上 97"), "{:?}", o.rows);
        assert!(!o.cautions.is_empty() && !o.faults.is_empty() && o.steps.len() == 3);
        assert!(buck_design(5.0, 12.0, 500.0, 150.0, 40.0, 30.0).is_err());
    }

    #[test]
    fn boost_mosfet_and_opamp_calculators() {
        // 5 V -> 12 V, 200 mA, 200 kHz: D = 1 - 5*0.85/12 = 0.646
        let b = boost_design(5.0, 12.0, 200.0, 200.0, 40.0, 50.0).unwrap();
        assert!(b.rows[0].1.contains("65 %"), "{:?}", b.rows);
        assert!(parse_value(b.primary.as_deref().unwrap()).unwrap() > 10e-6);
        assert!(boost_design(12.0, 5.0, 200.0, 200.0, 40.0, 50.0).is_err());
        assert!(!b.cautions.is_empty() && !b.faults.is_empty() && b.steps.len() == 4);
        // 22 mohm at 5 A is 0.55 W
        let m = mosfet_switch(5.0, 1.0, 5000.0, 22.0, 12.0).unwrap();
        assert!(m.rows[1].1.contains("550 mW"), "{:?}", m.rows);
        assert!(m.cautions.iter().any(|c| c.contains("0.25 W")));
        assert!(mosfet_switch(2.0, 2.5, 100.0, 100.0, 5.0).is_err());
        assert!(mosfet_switch(2.5, 2.0, 100.0, 100.0, 5.0).unwrap().cautions.iter().any(|c| c.contains("1.5 倍")));
        // gain 11 non-inverting with Rf = 10k needs R2 = 1k; 1 MHz / 11 = 91 kHz
        let a = opamp_gain(11.0, 10e3, 0.0, 1000.0, 5.0, 100.0).unwrap();
        assert_eq!(a.primary.as_deref(), Some("1k"));
        assert!(a.rows[4].1.contains("91"), "{:?}", a.rows);
        // gain 101 with 100 mV peak wants 10 V: clipped on a +-5 V supply
        let c = opamp_gain(101.0, 100e3, 0.0, 1000.0, 5.0, 100.0).unwrap();
        assert!(c.cautions.iter().any(|x| x.contains("クリップ")), "{:?}", c.cautions);
        // inverting gain 10 with Rf = 100k needs 10k
        assert_eq!(opamp_gain(10.0, 100e3, 1.0, 1000.0, 5.0, 10.0).unwrap().primary.as_deref(), Some("10k"));
        assert!(opamp_gain(0.5, 10e3, 0.0, 1000.0, 5.0, 100.0).is_err());
    }

    #[test]
    fn heat_design_gives_junction_temperature_and_heat_sink() {
        // 12 V -> 5 V at 500 mA: 3.5 W, +175 C without a heat sink
        let o = heat_design(12.0, 5.0, 500.0, 50.0, 5.0, 25.0, 125.0).unwrap();
        assert!(o.rows.iter().any(|(k, v)| k == "発熱 P" && v.starts_with("3.50")));
        assert!(o.rows.iter().any(|(k, v)| k == "接合部の温度 Tj" && v.starts_with("200")));
        assert!(o.rows.iter().any(|(k, v)| k == "必要な放熱板の熱抵抗" && v.starts_with("16.")), "{:?}", o.rows);
        // 8 V input: 1.5 W, 100 C: fine without a sink
        let cool = heat_design(8.0, 5.0, 500.0, 50.0, 5.0, 25.0, 125.0).unwrap();
        assert!(cool.rows.iter().any(|(_, v)| v.contains("放熱板なしで使えます")));
        assert!(heat_design(5.0, 5.0, 100.0, 50.0, 5.0, 25.0, 125.0).is_err());
        assert!(heat_design(12.0, 5.0, 100.0, 50.0, 5.0, 130.0, 125.0).is_err());
    }
}
