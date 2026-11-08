//! Renaming parts (reference designators) in the table: every row that names one follows.

use super::*;
use std::collections::BTreeMap;

/// `s` with every word (a run of letters, digits and `_`) that is a key of `map` replaced, so "U1", "U1.3" and "C3-U1" all follow.
pub fn rename_words(s: &str, map: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            out.push_str(map.get(word.as_str()).map_or(word.as_str(), |n| n.as_str()));
            word.clear();
        }
    };
    for ch in s.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            word.push(ch);
        } else {
            flush(&mut word, &mut out);
            out.push(ch);
        }
    }
    flush(&mut word, &mut out);
    out
}

impl DesignSpec {
    /// Follow a renumbering (`map`: old reference designator -> new, applied all at once, so two parts can swap).
    /// Names of nets, groups and rows are not touched; only what names a part: the parts of a group, the two sides of a distance, a bypass
    /// row, the pads of a measurement, the parts of a width link, and the IC / capacitor of an accepted bypass finding.
    pub fn rename_parts(&mut self, map: &BTreeMap<String, String>) {
        let r = |s: &mut String| *s = rename_words(s, map);
        for g in &mut self.groups {
            g.parts.iter_mut().for_each(r);
        }
        for a in &mut self.apart {
            a.a.iter_mut().for_each(r);
            a.b.iter_mut().for_each(r);
        }
        for b in &mut self.bypass {
            r(&mut b.ic);
            b.caps.iter_mut().for_each(r);
        }
        for m in &mut self.measures {
            m.pads.iter_mut().for_each(r);
            m.except.iter_mut().for_each(r);
        }
        for n in &mut self.net_rules {
            for l in &mut n.links {
                r(&mut l.a);
                r(&mut l.b);
            }
        }
        for w in self.waivers.iter_mut().filter(|w| w.kind == "bypass") {
            r(&mut w.name);
            r(&mut w.item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn words_follow_and_other_text_stays() {
        let m = map(&[("R1", "R2"), ("R2", "R1"), ("U1", "U5")]);
        assert_eq!(rename_words("R1", &m), "R2");
        assert_eq!(rename_words("U1.3", &m), "U5.3");
        assert_eq!(rename_words("R1-R2", &m), "R2-R1", "a swap is done at once");
        assert_eq!(rename_words("R10", &m), "R10", "only whole words");
        assert_eq!(rename_words("R1_CH1", &m), "R1_CH1", "a copy of a sheet is another word");
    }

    #[test]
    fn the_table_follows_a_renumbering() {
        let mut s = DesignSpec::default();
        s.groups.push(PlaceGroup { name: "R1 group".into(), parts: vec!["R1".into(), "C1".into()], ..Default::default() });
        s.bypass.push(Bypass { ic: "U1".into(), caps: vec!["C1".into()], ..Default::default() });
        s.waivers.push(Waiver { kind: "bypass".into(), name: "U1".into(), item: "C1".into(), note: "ok".into() });
        s.rename_parts(&map(&[("R1", "R3"), ("C1", "C2"), ("U1", "U2")]));
        assert_eq!(s.groups[0].parts, vec!["R3", "C2"]);
        assert_eq!(s.groups[0].name, "R1 group", "a name the designer wrote stays");
        assert_eq!((s.bypass[0].ic.as_str(), s.bypass[0].caps[0].as_str()), ("U2", "C2"));
        assert_eq!((s.waivers[0].name.as_str(), s.waivers[0].item.as_str()), ("U2", "C2"));
    }
}
