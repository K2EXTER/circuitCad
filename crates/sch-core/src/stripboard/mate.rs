//! Two boards of a project plugged into each other (L4): the holes of the connectors that join them, on the two layouts (stripboard
//! or perfboard), must make the same pattern, the second board turned as it goes on. `mate_check` says whether they do (and how the
//! second board goes on: turned, or only upside down), `mate_parts` places the second board's connectors so that they match the first's
//! (to lay the rest out round them, `perfboard::perf_layout_around`).

use super::{At, StripLayout, StripPart};
use crate::Schematic;

/// One connector pair of the link between the two boards: the part on the first board, the part on the second, and the pins joined
/// (first board's pin, second board's pin).
#[derive(Clone, Debug, PartialEq)]
pub struct MateLink {
    pub a: String,
    pub b: String,
    pub pairs: Vec<(String, String)>,
}

/// How the two layouts' connectors fit.
#[derive(Clone, Debug, PartialEq)]
pub enum Mate {
    /// They match with the second board turned `turn` quarter turns (clockwise, seen from above), parts up as the first's.
    Fits { turn: u8 },
    /// They match only with the second board upside down (its parts facing the first board).
    Flipped { turn: u8 },
    /// They do not match: each side's pattern in words.
    Differs { a: String, b: String },
    /// A connector or a pin is not on a layout.
    Missing(String),
}

impl Mate {
    pub fn ok(&self) -> bool {
        matches!(self, Mate::Fits { .. })
    }

    /// One line for the person; the boards' names as they go in the sentence (「main」, この基板).
    pub fn message(&self, a_board: &str, b_board: &str) -> String {
        match self {
            Mate::Fits { turn: 0 } => format!("{b_board}のコネクタの穴の並びは{a_board}と合います"),
            Mate::Fits { turn } => format!("{b_board}のコネクタの穴の並びは、{}° 回すと{a_board}と合います", *turn as u32 * 90),
            Mate::Flipped { .. } => format!("{b_board}を裏返す (部品を{a_board}の側に向ける) ときだけ、コネクタの穴の並びが合います。部品を上にして差すなら、並びが鏡に映した形です"),
            Mate::Differs { a, b } => format!("コネクタの穴の並びが合いません: {a_board}は {a}、{b_board}は {b}"),
            Mate::Missing(m) => m.clone(),
        }
    }
}

/// The hole of `refdes`'s pin `pin` on a layout.
fn hole(l: &StripLayout, refdes: &str, pin: &str) -> Option<At> {
    l.parts.iter().find(|p| p.refdes == refdes)?.leads.iter().find(|(n, _)| n == pin).map(|x| x.1)
}

/// The joined holes on the two layouts, pin pair by pin pair: (first board's hole, second board's hole).
fn joined(a: &StripLayout, b: &StripLayout, links: &[MateLink]) -> Result<Vec<(At, At)>, String> {
    let mut out = vec![];
    for k in links {
        for (pa, pb) in &k.pairs {
            let ha = hole(a, &k.a, pa).ok_or_else(|| format!("{} のピン {pa} が、この基板の並べ方にありません", k.a))?;
            let hb = hole(b, &k.b, pb).ok_or_else(|| format!("{} のピン {pb} が、相手の基板の並べ方にありません", k.b))?;
            out.push((ha, hb));
        }
    }
    if out.is_empty() {
        return Err("つないだピンがありません".into());
    }
    Ok(out)
}

/// A hole turned `turn` quarter turns clockwise (and mirrored left to right first when `flip`), as signed numbers.
fn turned(h: At, turn: u8, flip: bool) -> (i32, i32) {
    let (r, c) = (h.0 as i32, if flip { -(h.1 as i32) } else { h.1 as i32 });
    match turn % 4 {
        0 => (r, c),
        1 => (c, -r),
        2 => (-r, -c),
        _ => (-c, r),
    }
}

/// A set of holes in words: how many, in how many rows and columns.
fn pattern(holes: &[At]) -> String {
    let rows: std::collections::BTreeSet<u16> = holes.iter().map(|h| h.0).collect();
    let cols: std::collections::BTreeSet<u16> = holes.iter().map(|h| h.1).collect();
    format!("{} 穴が {} 行 × {} 列", holes.len(), rows.len(), cols.len())
}

/// Whether the second board's connectors (`b`) match the first's (`a`) for the pins `links` joins.
pub fn mate_check(a: &StripLayout, b: &StripLayout, links: &[MateLink]) -> Mate {
    let pairs = match joined(a, b, links) {
        Ok(p) => p,
        Err(e) => return Mate::Missing(e),
    };
    let fits = |turn: u8, flip: bool| {
        let (a0, b0) = (pairs[0].0, turned(pairs[0].1, turn, flip));
        let d = (a0.0 as i32 - b0.0, a0.1 as i32 - b0.1);
        pairs.iter().all(|(ha, hb)| {
            let q = turned(*hb, turn, flip);
            (q.0 + d.0, q.1 + d.1) == (ha.0 as i32, ha.1 as i32)
        })
    };
    if let Some(turn) = (0..4).find(|t| fits(*t, false)) {
        return Mate::Fits { turn };
    }
    if let Some(turn) = (0..4).find(|t| fits(*t, true)) {
        return Mate::Flipped { turn };
    }
    let (ha, hb): (Vec<At>, Vec<At>) = pairs.into_iter().unzip();
    Mate::Differs { a: pattern(&ha), b: pattern(&hb) }
}

/// The second board's connectors placed to match the first's (parts up, not turned): each joined pin on the hole of the first board's
/// pin, the whole moved to the top-left with `margin` free holes on the left (2: room for the wires from off the board) and one above.
/// Each part keeps its look and mount from the second board's schematic. Locked, the rest of the second board is laid out round them.
pub fn mate_parts(a: &StripLayout, b_sch: &Schematic, links: &[MateLink], margin: u16) -> Result<Vec<StripPart>, String> {
    let c = crate::breadboard::read(b_sch);
    let mut holes: Vec<(String, String, At)> = vec![];
    for k in links {
        for (pa, pb) in &k.pairs {
            let ha = hole(a, &k.a, pa).ok_or_else(|| format!("{} のピン {pa} が、相手の基板の並べ方にありません", k.a))?;
            holes.push((k.b.clone(), pb.clone(), ha));
        }
    }
    let (r0, c0) = (holes.iter().map(|h| h.2 .0).min().unwrap_or(0), holes.iter().map(|h| h.2 .1).min().unwrap_or(0));
    let mut parts: Vec<StripPart> = vec![];
    for k in links {
        if parts.iter().any(|p| p.refdes == k.b) {
            continue;
        }
        let (_, look, pins) = c.on.iter().find(|x| x.0 == k.b).ok_or_else(|| format!("{} がこの基板の回路図にありません", k.b))?;
        let mut leads = vec![];
        for (pin, _) in pins {
            let at = holes.iter().find(|h| h.0 == k.b && h.1 == *pin).map(|h| h.2).ok_or_else(|| format!("{} のピン {pin} は相手の基板につながっていないので、置く穴が決まりません", k.b))?;
            leads.push((pin.clone(), (at.0 - r0 + 1, at.1 - c0 + margin)));
        }
        parts.push(StripPart { refdes: k.b.clone(), look: *look, leads, mount: c.mounts.get(&k.b).cloned().unwrap_or_default() });
    }
    Ok(parts)
}
