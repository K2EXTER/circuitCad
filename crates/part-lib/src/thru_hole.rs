//! Hole, pad and pitch for a leaded (through-hole) part whose data sheet gives only the mechanical drawing
//! (lead diameter, body length and diameter) and no recommended land pattern.

/// Extra hole diameter over the largest lead diameter, in mm (a lead must go in without force, and
/// solder must flow around it).
pub const HOLE_CLEARANCE: f64 = 0.25;
/// Copper ring left around the hole on each side, in mm.
pub const ANNULAR_RING: f64 = 0.4;
/// Straight length kept between the body and each lead's bend, besides the lead's own diameter, in mm.
pub const BEND_ALLOWANCE: f64 = 1.0;

/// What to enter in the package tab.
#[derive(Clone, Debug, PartialEq)]
pub struct ThruHole {
    pub drill: f64,
    pub pad_d: f64,
    /// Axial part: distance between the two holes (a multiple of 2.54 mm); radial part: the lead spacing given.
    pub pitch: f64,
    /// Axial part only: the body drawn on the board (length along the leads, diameter across them).
    pub body_len: f64,
    pub body_dia: f64,
    /// Words for the learner: why each number is what it is.
    pub notes: Vec<String>,
}

fn up(v: f64, step: f64) -> f64 {
    // rounded to the micron so that 1.2 is 1.2 and not 1.2000000000000002
    (((v / step) - 1e-9).ceil() * step * 1000.0).round() / 1000.0
}

fn hole(lead_max: f64, notes: &mut Vec<String>) -> (f64, f64) {
    let drill = up(lead_max + HOLE_CLEARANCE, 0.1);
    let pad = up(drill + 2.0 * ANNULAR_RING, 0.1);
    notes.push(format!("穴径 {drill:.1}mm = リード径の最大 {lead_max:.3}mm + 余裕 {HOLE_CLEARANCE}mm を 0.1mm の単位に切り上げ (きつい穴は、リードが入らず、はんだも流れません)"));
    notes.push(format!("パッド径 {pad:.1}mm = 穴径 + 銅の輪 (片側 {ANNULAR_RING}mm) × 2 を切り上げ"));
    (drill, pad)
}

/// Axial-lead part (diode, resistor) mounted lying down: `lead_max` lead diameter, `body_len` and `body_dia`
/// the body's largest length and diameter, all in mm.
pub fn axial(lead_max: f64, body_len: f64, body_dia: f64) -> Result<ThruHole, String> {
    check(&[("リード径", lead_max), ("本体の長さ", body_len), ("本体の径", body_dia)])?;
    let mut notes = vec![];
    let (drill, pad_d) = hole(lead_max, &mut notes);
    let need = body_len + 2.0 * (lead_max + BEND_ALLOWANCE);
    let pitch = up(need, 2.54);
    notes.push(format!(
        "穴の間隔 {pitch:.2}mm = 本体の長さ {body_len:.2}mm + 両側のリードの曲げしろ ((リード径 + {BEND_ALLOWANCE}mm) × 2) = {need:.2}mm 以上の、2.54mm の倍数のうち一番小さいもの (基板の格子に合わせるため)"
    ));
    notes.push("リードを曲げる位置は、本体から離します (根元で曲げると、本体が割れます)。間隔を広げるぶんには問題ありません。基板の面積が許すなら、7.62mm や 10.16mm も使われます".into());
    Ok(ThruHole { drill, pad_d, pitch, body_len, body_dia, notes })
}

/// Radial-lead part (LED, electrolytic capacitor): the lead spacing `pitch` comes from the data sheet.
pub fn radial(lead_max: f64, pitch: f64) -> Result<ThruHole, String> {
    check(&[("リード径", lead_max), ("リードの間隔", pitch)])?;
    let mut notes = vec![];
    let (drill, pad_d) = hole(lead_max, &mut notes);
    notes.push(format!("穴の間隔は、データシートのリードの間隔 (F または Lead Spacing) {pitch:.2}mm をそのまま使います"));
    if pad_d + 0.3 > pitch {
        notes.push(format!("パッド径 {pad_d:.1}mm が間隔 {pitch:.2}mm に近すぎます。パッドの間が狭くて、はんだがつながる (ブリッジ) おそれがあります。パッドを小さくするか、細長い (楕円の) パッドにしてください"));
    }
    Ok(ThruHole { drill, pad_d, pitch, body_len: 0.0, body_dia: 0.0, notes })
}

fn check(v: &[(&str, f64)]) -> Result<(), String> {
    for (name, x) in v {
        if !(x.is_finite() && *x > 0.0) {
            return Err(format!("{name}を、0 より大きい数 (mm) で入れてください"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn do41_from_the_1n4007_data_sheet() {
        // lead 0.864, body 5.21 x 2.72 (the largest values of the drawing)
        let t = axial(0.864, 5.21, 2.72).unwrap();
        assert!((t.drill - 1.2).abs() < 1e-9, "{t:?}");
        assert!((t.pad_d - 2.0).abs() < 1e-9, "{t:?}");
        // 5.21 + 2 x (0.864 + 1.0) = 8.94 -> 10.16
        assert!((t.pitch - 10.16).abs() < 1e-9, "{t:?}");
        assert!(t.notes.iter().any(|n| n.contains("2.54mm の倍数")));
    }

    #[test]
    fn a_small_part_gets_a_small_pitch() {
        // DO-35: lead 0.55, body 3.05 x 1.6 -> 3.05 + 2 x 1.55 = 6.15 -> 7.62
        let t = axial(0.55, 3.05, 1.6).unwrap();
        assert!((t.drill - 0.8).abs() < 1e-9 && (t.pitch - 7.62).abs() < 1e-9, "{t:?}");
    }

    #[test]
    fn radial_keeps_the_spacing_and_warns_when_pads_nearly_touch() {
        let t = radial(0.5, 2.54).unwrap();
        assert_eq!(t.pitch, 2.54);
        assert!(t.notes.iter().all(|n| !n.contains("近すぎ")));
        let t = radial(1.2, 2.54).unwrap();
        assert!(t.notes.iter().any(|n| n.contains("近すぎ")), "{t:?}");
    }

    #[test]
    fn zero_or_negative_numbers_are_explained() {
        assert!(axial(0.0, 5.0, 2.0).unwrap_err().contains("リード径"));
        assert!(radial(0.5, -1.0).unwrap_err().contains("リードの間隔"));
    }
}
