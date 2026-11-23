//! A synchronous buck converter with its switches inside (TPS62933: 3.8–30 V in, 3 A, 76 mΩ and 32 mΩ MOSFETs, fixed-frequency
//! peak current mode with internal compensation): a switching model that turns its switches on and off at the frequency the RT
//! pin sets, so the duty cycle, the inductor's ripple current, the output ripple and the conduction losses come out in the
//! transient analysis. Built from the data sheet's typ values (not fitted); the frequency against RT is a power law through the
//! table's points, and the loop is set to the bandwidth the data sheet's loop figure shows for its design example.
//!
//! The clock is a ramp `frac(time × fsw)`. The high-side switch is turned on by an SR latch at the start of each cycle and off
//! when its current plus the slope compensation reaches the error amplifier's command (or at the maximum duty), the latch
//! being a 1 pF capacitor driven to 0 or 1 in about a nanosecond. The low side is the complement; with PFM it turns off when its
//! current reverses (a second latch, armed by the high side), and the high side skips cycles while the command is below zero
//! demand, its peak at least the minimum peak current. The high side needs the bootstrap capacitor (BST to SW, charged from an
//! internal 5 V through a diode while SW is low), as the real part does.

use super::num::g;
use super::{Out, Row};
use serde::Deserialize;

/// The switching frequency: a power law in the RT resistance through the table's points (kΩ, kHz), and the RT pin left open or
/// tied to GND (kHz); without RT points, `open` is the fixed frequency.
#[derive(Clone, Debug, Deserialize)]
pub struct Frequency {
    #[serde(default)]
    pub rt: Vec<[f64; 2]>,
    /// RT left open (above `open_above` kΩ): kHz.
    pub open: f64,
    #[serde(default)]
    pub open_above: Option<f64>,
    /// RT tied to GND (below `short_below` kΩ): kHz.
    #[serde(default)]
    pub short: Option<f64>,
    #[serde(default)]
    pub short_below: Option<f64>,
}

/// The EN pin: rising and falling thresholds (V), the pull-up current and the extra current above the threshold (A) that makes the
/// hysteresis of a resistor divider from VIN.
#[derive(Clone, Debug, Deserialize)]
pub struct EnPin {
    pub rise: f64,
    pub fall: f64,
    pub ip: f64,
    pub ih: f64,
}

/// A design point of the data sheet's loop figure: the output, the effective output capacitance and the bandwidth it shows (Hz).
/// The internal compensation (`rc`, `cc`) is set from it.
#[derive(Clone, Debug, Deserialize)]
pub struct LoopPoint {
    pub vout: f64,
    pub cout: f64,
    pub bw: f64,
    /// The phase margin the figure shows (degrees), for the notes.
    #[serde(default)]
    pub pm: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BuckSpec {
    #[serde(flatten)]
    pub common: super::Common,
    pub cite: String,
    #[serde(default)]
    pub from_figures: String,
    /// The FB reference (V) and its min and max at 25 °C (part to part).
    pub vref: f64,
    #[serde(default)]
    pub vref_range: Option<[f64; 2]>,
    /// Non-switching quiescent current and shutdown current (A).
    pub iq: f64,
    pub i_shdn: f64,
    /// VIN's undervoltage lockout, rising and falling (V).
    pub uvlo: [f64; 2],
    #[serde(default)]
    pub en: Option<EnPin>,
    /// On-resistances (Ω).
    pub rds_hs: f64,
    pub rds_ls: f64,
    /// The high side's peak current limit and the low side's valley current limit (A).
    pub ihs_limit: f64,
    pub ils_limit: f64,
    /// The minimum peak current at light load (A): with it the part runs PFM at light load (the low side turns off when its
    /// current reverses); without it, forced continuous conduction.
    #[serde(default)]
    pub ipeak_min: Option<f64>,
    /// The SS pin's charge current (A); none: an internal soft start of `tss` seconds and no SS pin.
    #[serde(default)]
    pub iss: Option<f64>,
    #[serde(default)]
    pub tss: f64,
    pub fsw: Frequency,
    /// Minimum on and off times (s).
    pub ton_min: f64,
    pub toff_min: f64,
    /// Output overvoltage: the high side stops above this fraction of the reference (1.15); none: no OVP.
    #[serde(default)]
    pub ovp: Option<f64>,
    /// Error amplifier's transconductance (A/V), the current command per volt of COMP (A/V), its offset (V), and the slope
    /// compensation (A over a cycle): not in the data sheet; `gm` and `gcs` set the scale, the loop point the gain.
    pub gm: f64,
    pub gcs: f64,
    pub comp_offset: f64,
    pub slope: f64,
    pub loop_point: LoopPoint,
    /// The high side's gate charge (C), drawn from the bootstrap capacitor at each turn-on (and so from VIN through the internal
    /// 5 V): not in the data sheet.
    #[serde(default = "default_qg")]
    pub qg: f64,
}

fn default_qg() -> f64 {
    5e-9
}

/// The power law f = k × R^−n (kHz, kΩ) through the RT points (least squares in log–log; two points exactly).
pub fn rt_law(points: &[[f64; 2]]) -> Option<(f64, f64)> {
    if points.len() < 2 {
        return None;
    }
    let xs: Vec<f64> = points.iter().map(|p| p[0].ln()).collect();
    let ys: Vec<f64> = points.iter().map(|p| p[1].ln()).collect();
    let n = xs.len() as f64;
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let slope = sxy / sxx;
    Some(((my - slope * mx).exp(), -slope))
}

/// The internal compensation from the loop point: the series Rc for the bandwidth (the crossover of a current-mode loop into
/// the output capacitor, fc = (Vref / Vout) gm Rc Gcs / (2π Cout)) and Cc for a zero at a quarter of it.
pub fn compensation(p: &BuckSpec) -> (f64, f64) {
    let l = &p.loop_point;
    let rc = 2.0 * std::f64::consts::PI * l.cout * l.bw * (l.vout / p.vref) / (p.gm * p.gcs);
    let cc = 1.0 / (2.0 * std::f64::consts::PI * rc * l.bw / 4.0);
    (rc, cc)
}

fn khz(f: f64) -> String {
    if f >= 1e3 { format!("{} MHz", g(f / 1e3, 3)) } else { format!("{} kHz", g(f, 3)) }
}

/// The `.subckt` (pins: vin gnd sw bst fb [en] [rt] [ss]).
pub fn buck_model(p: &BuckSpec) -> String {
    let n = &p.common.name;
    let mut pins = "vin gnd sw bst fb".to_string();
    if p.en.is_some() {
        pins += " en";
    }
    let has_rt = p.fsw.rt.len() >= 2;
    if has_rt {
        pins += " rt";
    }
    if p.iss.is_some() {
        pins += " ss";
    }
    let (rc, cc) = compensation(p);
    let vhi = p.comp_offset + p.ihs_limit / p.gcs + 0.3;
    let mut l = vec![format!(".subckt {n} {pins}")];
    // supply, UVLO and EN: hysteresis by a delayed copy of the state
    l.push(format!("* VIN under-voltage lockout ({} V rising, {} V falling)", g(p.uvlo[0], 4), g(p.uvlo[1], 4)));
    l.push(format!(
        "Buv uv gnd V=step(V(vin,gnd)-{r},0.02)+(1-step(V(vin,gnd)-{r},0.02))*step(V(vin,gnd)-{f},0.02)*V(uvd,gnd)",
        r = g(p.uvlo[0], 6),
        f = g(p.uvlo[1], 6)
    ));
    l.extend(["Ruv uv uvd 1k".into(), "Cuv uvd gnd 10p".into()]);
    match &p.en {
        Some(e) => {
            l.push(format!(
                "* EN: {} V rising, {} V falling; pulled up by {} uA (and {} uA more while on), so a floating EN turns the part on",
                g(e.rise, 4),
                g(e.fall, 4),
                g(e.ip * 1e6, 3),
                g(e.ih * 1e6, 3)
            ));
            l.push(format!(
                "Beo eo gnd V=step(V(en,gnd)-{r},0.005)+(1-step(V(en,gnd)-{r},0.005))*step(V(en,gnd)-{f},0.005)*V(eod,gnd)",
                r = g(e.rise, 6),
                f = g(e.fall, 6)
            ));
            l.extend(["Reo eo eod 1k".into(), "Ceo eod gnd 10p".into()]);
            l.push(format!("Ben gnd en I=({}+{}*V(eo,gnd))*limit((5-V(en,gnd))*2,0,1)*step(V(vin,gnd)-2,0.1)", g(e.ip, 6), g(e.ih, 6)));
            l.push("Ren en gnd 1G".into());
            l.push("Bon on gnd V=V(uv,gnd)*V(eo,gnd)".into());
        }
        None => l.push("Bon on gnd V=V(uv,gnd)".into()),
    }
    l.push(format!("* supply current: {} uA switching aside, {} uA shut down (the gate drive's current is not counted)", g(p.iq * 1e6, 3), g(p.i_shdn * 1e6, 3)));
    l.push(format!("Bq vin gnd I=({}+{}*V(on,gnd))*tanh(V(vin,gnd))", g(p.i_shdn, 6), g(p.iq - p.i_shdn, 6)));
    // internal 5 V and the bootstrap
    l.push("* internal 5 V (from VIN) charging the bootstrap capacitor through a diode while SW is low; the high side needs BST - SW > 2.5 V".into());
    l.extend([
        "Bvcc vc0 gnd V=limit(V(vin,gnd)-0.2,0,5)".into(),
        "Vvcc vc0 vcc DC 0".into(),
        "Bvm vin gnd I=I(Vvcc)".into(),
        "Dbst vcc bst DBST".into(),
        "Rbst bst sw 1Meg".into(),
        "Bbok bok gnd V=step(V(bst,sw)-2.5,0.2)".into(),
    ]);
    l.push(format!(
        "* the high side's gate ({} nC at 4.4 V): the driver joins it to BST (on) or to SW (off) through 2 Ohm; without a bootstrap capacitor BST does not ride up with SW and the gate cannot hold the switch on",
        g(p.qg * 1e9, 3)
    ));
    l.extend([
        "Bqn qn gnd V=1-V(q,gnd)".into(),
        "Sgu bst gate q gnd SWDRV".into(),
        "Sgd gate sw qn gnd SWDRV".into(),
        format!("Cgate gate sw {}", g(p.qg / 4.4, 6)),
        "Rgate gate sw 1Meg".into(),
    ]);
    // frequency
    let fexpr = if let Some((k, e)) = rt_law(&p.fsw.rt) {
        let open_above = p.fsw.open_above.unwrap_or(280.0);
        let short = p.fsw.short.unwrap_or(p.fsw.open);
        let short_below = p.fsw.short_below.unwrap_or(1.0);
        l.push(format!(
            "* RT: 10 uA out of the pin, its voltage read as the resistance (kOhm); fsw = {} x RT^-{} kHz through the table's points, {} open (> {} kOhm), {} tied to GND (< {} kOhm)",
            g(k, 6),
            g(e, 6),
            khz(p.fsw.open),
            g(open_above, 4),
            khz(short),
            g(short_below, 4)
        ));
        l.push("Brt gnd rt I=10u*limit((3.2-V(rt,gnd))*5,0,1)".into());
        l.push("Rrtl rt gnd 1G".into());
        format!(
            "if(V(rt,gnd)*100<{sb},{s},if(V(rt,gnd)*100>{oa},{o},{k}*pwr(max(V(rt,gnd)*100,{sb}),-{e})))",
            sb = g(short_below, 6),
            s = g(short, 6),
            oa = g(open_above, 6),
            o = g(p.fsw.open, 6),
            k = g(k, 6),
            e = g(e, 6)
        )
    } else {
        l.push(format!("* fixed frequency {}", khz(p.fsw.open)));
        g(p.fsw.open, 6)
    };
    l.push(format!("Bfs fs gnd V={fexpr}"));
    l.push("* the clock: a ramp from 0 to 1 over each cycle".into());
    l.push("Br r gnd V=time*V(fs,gnd)*1e3-floor(time*V(fs,gnd)*1e3)".into());
    // soft start and the reference
    let ss_node = match p.iss {
        Some(iss) => {
            l.push(format!("* SS: {} uA into the soft-start capacitor; the reference is the lower of {} V and SS", g(iss * 1e6, 3), g(p.vref, 4)));
            l.push(format!("Bss gnd ss I={}*V(on,gnd)*limit((3-V(ss,gnd))*5,0,1)", g(iss, 6)));
            l.push("Bssd ss gnd I=(1-V(on,gnd))*V(ss,gnd)/1k".into());
            l.push("Rssl ss gnd 1G".into());
            "ss"
        }
        None => {
            l.push(format!("* internal soft start: the reference rises to {} V in {} ms", g(p.vref, 4), g(p.tss * 1e3, 3)));
            l.push(format!("Bss gnd ssi I=1n*{}*V(on,gnd)*limit((3-V(ssi,gnd))*5,0,1)", g(p.vref / p.tss, 6)));
            l.push("Cssi ssi gnd 1n".into());
            l.push("Bssd ssi gnd I=(1-V(on,gnd))*V(ssi,gnd)/1k".into());
            l.push("Rssl ssi gnd 1G".into());
            "ssi"
        }
    };
    l.push("* the reference's difference from part to part (0 V; the variation analyses move it over the data sheet's range)".into());
    l.push("Vtol vt gnd DC 0".into());
    if let Some([lo, hi]) = p.vref_range {
        l.push(format!("* spread Vtol {} {} vout (data sheet: FB {} to {} V)", g(lo - p.vref, 6), g(hi - p.vref, 6), g(lo, 6), g(hi, 6)));
    }
    l.push(format!("Bref ref gnd V=min({}+V(vt,gnd),V({ss_node},gnd))", g(p.vref, 6)));
    // error amplifier and compensation
    l.push(format!(
        "* error amplifier: {} uA/V into COMP, the internal compensation {} + {} (set for the data sheet's loop: {} at Vout {} V, Cout {} uF)",
        g(p.gm * 1e6, 4),
        super::reference::ohm_text(rc),
        cap_text(cc),
        khz(p.loop_point.bw / 1e3),
        g(p.loop_point.vout, 4),
        g(p.loop_point.cout * 1e6, 4)
    ));
    l.push(format!("Bea gnd comp I=limit({}*(V(ref,gnd)-V(fb,gnd)),-100u,100u)", g(p.gm, 6)));
    l.extend([format!("Rc comp cz {}", g(rc, 6)), format!("Cc cz gnd {}", g(cc, 6)), "Rcl comp gnd 100Meg".into()]);
    l.push(format!("Bcl comp gnd I=10*(max(V(comp,gnd)-{},0)+min(V(comp,gnd),0))+(1-V(on,gnd))*V(comp,gnd)*1m", g(vhi, 6)));
    l.push("Rfb fb gnd 100Meg".into());
    // the current command and the latch
    let ipk_min = p.ipeak_min.unwrap_or(0.0);
    l.push(format!(
        "* the peak current command: {} A per volt of COMP above {} V, from {} A up to the high side's limit {} A",
        g(p.gcs, 4),
        g(p.comp_offset, 4),
        g(ipk_min, 4),
        g(p.ihs_limit, 4)
    ));
    l.push(format!("Bic ic gnd V=limit((V(comp,gnd)-{})*{},{},{})", g(p.comp_offset, 6), g(p.gcs, 6), g(ipk_min, 6), g(p.ihs_limit, 6)));
    let tb = format!("max({}*V(fs,gnd)*1e3,0.03)", g(p.ton_min, 6));
    let dmax = format!("(1-{}*V(fs,gnd)*1e3)", g(p.toff_min, 6));
    let mut gate = "V(on,gnd)*V(bok,gnd)".to_string();
    if p.ipeak_min.is_some() {
        gate += &format!("*step(V(comp,gnd)-{},0.01)", g(p.comp_offset, 6));
    }
    if let Some(o) = p.ovp {
        gate += &format!("*step({}-V(fb,gnd),0.005)", g(o * p.vref, 6));
    }
    gate += &format!("*step({}-I(Vls),0.05)", g(p.ils_limit, 6));
    l.push(format!(
        "* the high side: set at the start of each cycle (not while the low side's current is above its limit {} A{}{}), reset when its current plus the slope compensation ({} A a cycle) reaches the command, after the minimum on time {} ns, or at the maximum duty (minimum off time {} ns)",
        g(p.ils_limit, 4),
        if p.ipeak_min.is_some() { ", nor while COMP asks for nothing: PFM" } else { "" },
        if p.ovp.is_some() { ", nor above the OVP" } else { "" },
        g(p.slope, 4),
        g(p.ton_min * 1e9, 4),
        g(p.toff_min * 1e9, 4)
    ));
    l.push(format!("Bset set gnd V=(1-step(V(r,gnd)-{tb},0.002))*{gate}"));
    l.push(format!(
        "Brst rst gnd V=max(step(I(Vhs)+{}*V(r,gnd)-V(ic,gnd),0.01)*step(V(r,gnd)-{tb},0.01),max(step(V(r,gnd)-{dmax},0.01),1-V(on,gnd)*V(bok,gnd)))",
        g(p.slope, 6)
    ));
    l.push("Bq1 gnd q I=1m*(V(set,gnd)*(1-V(rst,gnd))*(1-V(q,gnd))-V(rst,gnd)*V(q,gnd))".into());
    l.extend(["Cq q gnd 20p".into(), "Rq q gnd 1G".into()]);
    // the low side
    let lg = if p.ipeak_min.is_some() {
        l.push("* the low side: on while the high side is off, until its current reverses (armed again by the high side)".into());
        l.push("Bzc gnd zc I=1m*(V(q,gnd)*(1-V(zc,gnd))-(1-V(q,gnd))*step(-I(Vls)-0.02,0.01)*V(zc,gnd))".into());
        l.extend(["Czc zc gnd 20p".into(), "Rzc zc gnd 1G".into()]);
        "V(on,gnd)*(1-V(q,gnd))*V(zc,gnd)"
    } else {
        l.push("* the low side: on while the high side is off (forced continuous conduction)".into());
        "V(on,gnd)*(1-V(q,gnd))"
    };
    l.push(format!("Blg lg gnd V={lg}"));
    l.push(format!("* the switches ({} and {}) with their body diodes; Vhs and Vls carry the inductor's current", super::reference::ohm_text(p.rds_hs), super::reference::ohm_text(p.rds_ls)));
    l.extend([
        "Vhs vin hsi DC 0".into(),
        "Shs hsi sw gate sw SWHS".into(),
        "Vls gnd lsi DC 0".into(),
        "Sls lsi sw lg gnd SWLS".into(),
        "Dhs sw vin DBODY".into(),
        "Dls gnd sw DBODY".into(),
        format!(".model SWHS SW(RON={} ROFF=1e7 VT=2.5 VW=0.5)", g(p.rds_hs, 6)),
        ".model SWDRV SW(RON=10 ROFF=1e9 VT=0.5 VW=0.1)".into(),
        format!(".model SWLS SW(RON={} ROFF=1e7 VT=0.5 VW=0.1)", g(p.rds_ls, 6)),
        ".model DBODY D(IS=1e-12 N=1.2 RS=0.05)".into(),
        ".model DBST D(IS=1e-12 N=1 RS=2)".into(),
        format!(".ends {n}"),
    ]);
    l.join("\n")
}

fn cap_text(c: f64) -> String {
    if c >= 1e-9 { format!("{} nF", g(c * 1e9, 3)) } else { format!("{} pF", g(c * 1e12, 3)) }
}

/// The notes: what the model is made from and what it leaves out.
pub fn buck_limits(p: &BuckSpec) -> Vec<String> {
    let (rc, cc) = compensation(p);
    let mut v = vec![format!(
        "{} の表の typ から作った、実際にスイッチを入り切りする動作モデルです (基準電圧 {} V、オン抵抗 {} と {}、電流制限 ピーク {} A・谷 {} A、最小オン時間 {} ns・最小オフ時間 {} ns)。デューティ比、コイルのリップル電流、出力のリップル、スイッチの導通損失が波形に出ます。",
        p.cite,
        g(p.vref, 4),
        super::reference::ohm_text(p.rds_hs),
        super::reference::ohm_text(p.rds_ls),
        g(p.ihs_limit, 3),
        g(p.ils_limit, 3),
        g(p.ton_min * 1e9, 3),
        g(p.toff_min * 1e9, 3)
    )];
    if !p.from_figures.is_empty() {
        v.push(format!("表に無い値は図から: {}。", p.from_figures));
    }
    v.push(format!(
        "制御はピーク電流モードです。内部の補償 (誤差アンプ {} µA/V・{} と {}・電流の指令 {} A/V・スロープ補償 1 周期 {} A) はデータシートに無いので、データシートのループの図 (出力 {} V・実効の出力容量 {} µF で帯域 {}{}) に合わせて決めました。ほかの出力電圧やコンデンサでは、帯域は出力電圧と容量に反比例して変わります。",
        g(p.gm * 1e6, 3),
        super::reference::ohm_text(rc),
        cap_text(cc),
        g(p.gcs, 3),
        g(p.slope, 3),
        g(p.loop_point.vout, 3),
        g(p.loop_point.cout * 1e6, 3),
        khz(p.loop_point.bw / 1e3),
        p.loop_point.pm.map(|m| format!("・位相余裕 {} 度", g(m, 3))).unwrap_or_default()
    ));
    let mode = if p.ipeak_min.is_some() {
        format!("軽い負荷では PFM (コイルの電流が逆になると下側のスイッチを切り、ピークは少なくとも {} A、要らない周期は飛ばす) で、周波数が下がります", g(p.ipeak_min.unwrap_or(0.0), 3))
    } else {
        "軽い負荷でも周波数を保つ連続モード (FCCM) です".into()
    };
    v.push(format!("{mode}。"));
    v.push(format!("上側のスイッチのゲートの電荷 ({} nC、データシートに無いので仮の値) を入るたびに BST から取り、内部の 5 V を通して VIN から補います。", g(p.qg * 1e9, 3)));
    v.push("扱わないもの: スイッチの切り替わりの時間と切り替え損失、下側のゲートを動かす電流、デッドタイム、周波数の拡散 (spread spectrum)、最小オン時間・オフ時間での周波数の折り返し (その代わり、そこでデューティが止まります)、過熱保護、出力の短絡でのヒカップ、温度による変化。".into());
    v.push("BST と SW の間に 0.1 µF のコンデンサを付けないと、上側のスイッチが入らず出力が出ません (実物と同じ)。".into());
    v.push("時間刻みは 1 周期の 1/50 以下 (500 kHz なら 40 ns) にしてください。粗いと、スイッチの切り替わりの位置がずれて出力の平均がずれます。計算に時間がかかるので、見る時間はソフトスタートが終わるまで (数 ms) から始めましょう。".into());
    v
}

/// Values the template cannot use, with what to fix.
pub fn check_buck(p: &BuckSpec) -> Result<(), String> {
    for (what, v) in [
        ("基準電圧", p.vref),
        ("上側のオン抵抗", p.rds_hs),
        ("下側のオン抵抗", p.rds_ls),
        ("上側の電流制限", p.ihs_limit),
        ("下側の電流制限", p.ils_limit),
        ("周波数 (RT 開放)", p.fsw.open),
        ("最小オン時間", p.ton_min),
        ("最小オフ時間", p.toff_min),
        ("誤差アンプの gm", p.gm),
        ("電流の指令の傾き", p.gcs),
        ("ループの図の出力電圧", p.loop_point.vout),
        ("ループの図の出力容量", p.loop_point.cout),
        ("ループの図の帯域", p.loop_point.bw),
    ] {
        if !(v.is_finite() && v > 0.0) {
            return Err(format!("{what}は 0 より大きい数にしてください"));
        }
    }
    if p.uvlo[1] >= p.uvlo[0] {
        return Err("UVLO は「立ち上がり 立ち下がり」の順で、立ち下がりを低くしてください".into());
    }
    if let Some(e) = &p.en
        && (e.fall >= e.rise || e.ip < 0.0 || e.ih < 0.0)
    {
        return Err("EN のしきい値は立ち下がりを立ち上がりより低く、電流は 0 以上にしてください".into());
    }
    if p.iss.is_none() && !(p.tss > 0.0) {
        return Err("SS ピンの電流か、内部のソフトスタートの時間のどちらかを入れてください".into());
    }
    if p.fsw.rt.len() == 1 || p.fsw.rt.iter().any(|q| !(q[0] > 0.0 && q[1] > 0.0)) {
        return Err("RT の点は (kΩ, kHz) を 2 つ以上、どちらも正の数で (RT ピンが無ければ空)".into());
    }
    if let Some([lo, hi]) = p.vref_range
        && !(lo <= p.vref && p.vref <= hi)
    {
        return Err("基準電圧の min・max の間に typ が入っていません".into());
    }
    if p.ton_min * p.fsw.open * 1e3 >= 0.5 || p.toff_min * p.fsw.open * 1e3 >= 0.5 {
        return Err("最小オン時間・オフ時間が 1 周期の半分より長くなっています。単位 (秒) を確かめてください".into());
    }
    Ok(())
}

pub(super) fn fit_buck(p: &BuckSpec) -> Result<Out, String> {
    check_buck(p)?;
    // the frequency law against the table's RT points
    let rows: Vec<Row> = match rt_law(&p.fsw.rt) {
        Some((k, e)) => p.fsw.rt.iter().map(|q| super::row("fSW", format!("RT = {} kΩ", g(q[0], 4)), q[1] * 1e3, k * q[0].powf(-e) * 1e3, "Hz")).collect(),
        None => vec![],
    };
    Ok((buck_model(p), buck_limits(p), rows, vec![]))
}
