//! The feature list (`docs/features.md`) keeps up with the code: every analysis of the schematic's analysis tab and every `ccad`
//! command is in it. A new one that is not fails here, so the list is written when the feature is.

/// The names the `ccad` session answers to: the arms of its command `match` (`cli/src/session.rs`).
fn ccad_commands() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../cli/src/session.rs");
    let text = std::fs::read_to_string(path).unwrap();
    let mut out = vec![];
    for line in text.lines() {
        let t = line.trim_start();
        // `"name" => self.name(args)` or `"a" | "b" => self.x(args)`
        // only the arms that call the session (the dispatch), not other matches on strings in the file
        let Some((head, body)) = t.split_once("=>").filter(|(h, b)| h.starts_with('"') && b.trim_start().starts_with("self.")) else { continue };
        let _ = body;
        // a guarded arm (`"part" if args… => …`): the name is before the guard
        let head = head.split(" if ").next().unwrap_or(head);
        for part in head.split('|') {
            let name = part.trim().trim_matches('"');
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') && !out.iter().any(|x| x == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}

#[test]
fn every_analysis_and_command_is_in_the_feature_list() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/features.md");
    let list = std::fs::read_to_string(path).unwrap();
    let mut missing = vec![];
    for (key, title, _) in crate::schematic::panel::analysis_sections() {
        if !list.contains(title) {
            missing.push(format!("解析「{title}」({key})"));
        }
    }
    let commands = ccad_commands();
    assert!(commands.len() > 40, "{commands:?}");
    for c in &commands {
        if !list.contains(&format!("`{c}`")) {
            missing.push(format!("ccad の `{c}`"));
        }
    }
    assert!(missing.is_empty(), "docs/features.md に書かれていません (1 行足してください):\n{}", missing.join("\n"));
}
