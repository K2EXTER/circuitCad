// Gathers every `en/*.tsv` into one list of `include_str!`s, so a new table file needs no code change.
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("en");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "tsv")).collect())
        .unwrap_or_default();
    files.sort();
    let mut out = String::from("&[\n");
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy();
        out.push_str(&format!("    ({name:?}, include_str!({:?})),\n", f.display().to_string()));
    }
    out.push(']');
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("tables.rs");
    std::fs::write(dest, out).unwrap();
}
