//! Assembly files: bill of materials and component placement list (CSV, JLCPCB-style columns).

use crate::model::*;
use std::collections::BTreeMap;

/// Natural order for designators: R2 before R10.
fn natural_key(refdes: &str) -> (String, u32, String) {
    let split = refdes.find(|c: char| c.is_ascii_digit()).unwrap_or(refdes.len());
    let (prefix, rest) = refdes.split_at(split);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    (prefix.to_string(), digits.parse().unwrap_or(0), rest[digits.len()..].to_string())
}

fn csv(field: &str) -> String {
    if field.contains([',', '"', '\n']) { format!("\"{}\"", field.replace('"', "\"\"")) } else { field.to_string() }
}

/// One row per distinct (value, footprint), designators listed together.
pub fn bom_csv(board: &Board) -> String {
    let mut groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for p in &board.parts {
        groups.entry((p.value.clone(), p.footprint.clone())).or_default().push(p.refdes.clone());
    }
    let mut rows: Vec<(String, String, Vec<String>)> = groups.into_iter().map(|((v, f), mut r)| {
        r.sort_by_key(|x| natural_key(x));
        (v, f, r)
    }).collect();
    rows.sort_by_key(|(_, _, r)| natural_key(&r[0]));
    let mut out = String::from("Comment,Designator,Footprint,LCSC Part #\n");
    for (value, footprint, refs) in rows {
        out.push_str(&format!("{},{},{},\n", csv(&value), csv(&refs.join(",")), csv(&footprint)));
    }
    out
}

/// Surface-mount parts only (parts with a through-hole pad are hand-soldered). The rotation is
/// the part's rotation as seen from the top of the board, for both sides.
pub fn cpl_csv(board: &Board) -> String {
    let mut parts: Vec<&Part> = board
        .parts
        .iter()
        .filter(|p| board.pads.iter().filter(|pad| pad.refdes == p.refdes).all(|pad| pad.drill.is_none()))
        .collect();
    parts.sort_by_key(|p| natural_key(&p.refdes));
    let mut out = String::from("Designator,Mid X,Mid Y,Layer,Rotation\n");
    for p in parts {
        let layer = if p.side == Side::Bottom { "Bottom" } else { "Top" };
        out.push_str(&format!("{},{:.4}mm,{:.4}mm,{layer},{}\n", csv(&p.refdes), p.pos.x, p.pos.y, (p.rot as u32 % 4) * 90));
    }
    out
}

/// (file name, contents) for the assembly service.
pub fn assembly_files(board: &Board) -> Vec<(String, String)> {
    vec![(format!("{}-BOM.csv", board.name), bom_csv(board)), (format!("{}-CPL.csv", board.name), cpl_csv(board))]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        let mut b = Board::new("demo", 40.0, 30.0);
        for (r, v, f, x) in [("R10", "10k", "R_0805", 5.0), ("R2", "10k", "R_0805", 10.0), ("C1", "100n", "C_0805", 15.0), ("J1", "Conn", "PinHeader_1x2", 25.0)] {
            b.set_part(Part { refdes: r.into(), value: v.into(), footprint: f.into(), pos: Point::new(x, 10.0), rot: 1, side: Side::Top }).unwrap();
        }
        b
    }

    #[test]
    fn bom_groups_identical_parts_in_natural_order() {
        let csv = bom_csv(&board());
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "Comment,Designator,Footprint,LCSC Part #");
        assert!(lines.contains(&"10k,\"R2,R10\",R_0805,"), "{csv}");
        assert!(lines.contains(&"100n,C1,C_0805,"));
        assert!(lines.contains(&"Conn,J1,PinHeader_1x2,"));
        assert_eq!(lines.len(), 4);
        // rows ordered by first designator: C, J, R
        assert!(lines[1].starts_with("100n") && lines[3].starts_with("10k"));
    }

    #[test]
    fn cpl_lists_only_surface_mount_parts() {
        let csv = cpl_csv(&board());
        assert!(csv.contains("C1,15.0000mm,10.0000mm,Top,90"), "{csv}");
        assert!(csv.contains("R2,"));
        assert!(!csv.contains("J1"), "through-hole header must not be placed by machine");
        assert_eq!(csv.lines().count(), 4);
    }

    #[test]
    fn natural_sorting() {
        let mut v = vec!["R10", "R2", "C3", "R1"];
        v.sort_by_key(|x| natural_key(x));
        assert_eq!(v, ["C3", "R1", "R2", "R10"]);
    }
}
