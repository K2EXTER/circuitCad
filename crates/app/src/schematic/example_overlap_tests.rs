use super::*;

#[test]
fn shipped_example_circuits_have_no_overlapping_parts() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut found = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "schjson") {
            let sch: Schematic = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            let boxes: Vec<_> = sch.components.iter().map(|c| (c, sheet_box(c.kind, comp_extents(c), c.pos, c.rot))).collect();
            for (i, (a, ab)) in boxes.iter().enumerate() {
                for (b, bb) in &boxes[i + 1..] {
                    if let (Some(x), Some(y)) = (ab, bb)
                        && boxes_overlap(*x, *y)
                    {
                        found.push(format!("{}: {} / {}", e.file_name().to_string_lossy(), a.refdes, b.refdes));
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "overlapping parts in the examples: {found:?}");
}

#[test]
fn shipped_examples_pass_the_erc_without_errors() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut bad = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "schjson") {
            let sch: Schematic = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            for p in erc(&sch).into_iter().filter(|p| p.severity == Severity::Error) {
                bad.push(format!("{}: {}", e.file_name().to_string_lossy(), p.message));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
