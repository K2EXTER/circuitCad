//! The parts placed on a schematic kept up with the library (candidate Y1): a placed part carries a copy of its library part
//! (`CustomSymbol`: the pins, the model, the ratings, the shops' numbers ...), so a model made or corrected in the library later
//! does not reach the schematics drawn before. Here the copies are compared with the library as it is now, and brought up to date
//! on request: the part stays where it is (place, turn, reference, value); its pins are kept when the library's differ (the
//! wires end on the old pins: such a part is listed to be placed again by hand).

use crate::PartDef;
use sch_core::{Component, CustomSymbol, Schematic};

/// A placed part whose copy differs from its library part.
#[derive(Clone, Debug, PartialEq)]
pub struct Stale {
    pub refdes: String,
    /// The library part's name.
    pub part: String,
    /// What differs, in words ("モデル", "定格" ...); the pins and the footprint by name too.
    pub changes: Vec<&'static str>,
    /// The library's pins differ from the copy's: updating keeps the copy's pins (the wires end on them).
    pub pins_differ: bool,
}

/// The library part a placed part was made from: the same name, the same footprint first.
fn source<'a>(c: &Component, lib: &'a [PartDef]) -> Option<&'a PartDef> {
    let sym = c.custom.as_ref()?;
    let named: Vec<&PartDef> = lib.iter().filter(|p| p.name == sym.name).collect();
    named.iter().copied().find(|p| p.footprint_name() == c.footprint).or_else(|| named.first().copied())
}

/// The fields of the copy that differ from the library's symbol, in words.
fn changes(old: &CustomSymbol, new: &CustomSymbol, old_fp: &str, new_fp: &str) -> Vec<&'static str> {
    let mut out = vec![];
    if old.sim != new.sim {
        out.push(if old.sim.is_none() { "モデル (新しく付いた)" } else { "モデル" });
    }
    if old.ratings != new.ratings {
        out.push("定格");
    }
    if old.shops != new.shops {
        out.push("店の型番");
    }
    if old.glyph != new.glyph {
        out.push("記号の形");
    }
    if old.programming != new.programming {
        out.push("書き込みの方法");
    }
    if old.companions != new.companions {
        out.push("推奨の周辺部品");
    }
    if old.prefix != new.prefix {
        out.push("部品番号の頭");
    }
    if old.pins != new.pins {
        out.push("ピン");
    }
    if !old_fp.is_empty() && old_fp != new_fp {
        out.push("フットプリント");
    }
    out
}

fn all_components(s: &Schematic) -> impl Iterator<Item = &Component> {
    s.components.iter().chain(s.sheets.iter().flat_map(|x| x.components.iter()))
}

/// The placed parts whose copy differs from the library now (each reference once, in order).
pub fn stale(s: &Schematic, lib: &[PartDef]) -> Vec<Stale> {
    let mut out: Vec<Stale> = vec![];
    for c in all_components(s) {
        if out.iter().any(|x| x.refdes == c.refdes) {
            continue;
        }
        let (Some(old), Some(p)) = (c.custom.as_ref(), source(c, lib)) else { continue };
        let new = p.symbol();
        let ch = changes(old, &new, &c.footprint, &p.footprint_name());
        if !ch.is_empty() {
            out.push(Stale { refdes: c.refdes.clone(), part: p.name.clone(), pins_differ: old.pins != new.pins, changes: ch });
        }
    }
    out
}

/// The parts `refdes` (all the stale ones when empty) brought up to the library: the library's symbol and footprint, the copy's
/// pins kept where the library's differ. Returns the references updated.
pub fn refresh(s: &mut Schematic, lib: &[PartDef], refdes: &[String]) -> Vec<String> {
    let mut done: Vec<String> = vec![];
    let comps = s.components.iter_mut().chain(s.sheets.iter_mut().flat_map(|x| x.components.iter_mut()));
    for c in comps {
        if !refdes.is_empty() && !refdes.contains(&c.refdes) {
            continue;
        }
        let Some(p) = source(c, lib) else { continue };
        let Some(old) = c.custom.as_ref() else { continue };
        let mut new = p.symbol();
        if changes(old, &new, &c.footprint, &p.footprint_name()).is_empty() {
            continue;
        }
        if old.pins != new.pins {
            new.pins = old.pins.clone();
        }
        c.custom = Some(new);
        if !c.footprint.is_empty() {
            c.footprint = p.footprint_name();
        }
        if !done.contains(&c.refdes) {
            done.push(c.refdes.clone());
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib_part(name: &str) -> PartDef {
        crate::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<PartDef>(t).unwrap()).find(|p| p.name == name).unwrap()
    }

    /// A schematic whose relay was placed before the library part had a model: listed with its new model, brought up to date
    /// (the model in, the place and the reference kept); then nothing is listed. A part placed from the library as it is now is
    /// never listed.
    #[test]
    fn a_part_placed_before_its_model_gets_it() {
        let relay = lib_part("TQ2-L2-5V");
        let diode = lib_part("BAV199");
        let lib = vec![relay.clone(), diode.clone()];
        let mut s = Schematic::default();
        let k = s.add(sch_core::Kind::Custom, (10, 10), 1);
        let mut old = relay.symbol();
        old.sim = None;
        s.components[k].custom = Some(old);
        s.components[k].footprint = relay.footprint_name();
        let d = s.add(sch_core::Kind::Custom, (30, 10), 0);
        s.components[d].custom = Some(diode.symbol());
        s.components[d].footprint = diode.footprint_name();
        let (kref, pos) = (s.components[k].refdes.clone(), s.components[k].pos);
        let found = stale(&s, &lib);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].refdes, kref);
        assert_eq!(found[0].changes, ["モデル (新しく付いた)"]);
        assert!(!found[0].pins_differ);
        assert_eq!(refresh(&mut s, &lib, &[]), std::slice::from_ref(&kref));
        let c = &s.components[k];
        assert!(c.custom.as_ref().unwrap().sim.is_some() && c.pos == pos && c.refdes == kref && c.rot == 1);
        assert!(stale(&s, &lib).is_empty());
    }

    /// A copy whose pins differ from the library's keeps its pins (the wires end on them) and is said to differ there.
    #[test]
    fn the_pins_of_a_placed_part_are_kept() {
        let relay = lib_part("TQ2-L2-5V");
        let mut s = Schematic::default();
        let k = s.add(sch_core::Kind::Custom, (0, 0), 0);
        let mut old = relay.symbol();
        old.sim = None;
        old.pins.swap(0, 1);
        let pins = old.pins.clone();
        s.components[k].custom = Some(old);
        let found = stale(&s, std::slice::from_ref(&relay));
        assert!(found[0].pins_differ && found[0].changes.contains(&"ピン"), "{found:?}");
        refresh(&mut s, std::slice::from_ref(&relay), &[]);
        let sym = s.components[k].custom.as_ref().unwrap();
        assert!(sym.sim.is_some() && sym.pins == pins, "the model in, the pins as they were");
    }
}
