//! A made part as one file to hand on (候補 Q11, `.ccpart`): the part's JSON (its pins, footprint, model, ratings, 3D mesh and
//! the data sheet's edition with the values read off it: `PartDef::sheet`) with the maker's STEP file it was placed from
//! (`PartDef::model3d_src`, a path on the maker's computer) inside it, so the board's STEP keeps its curved faces on another
//! computer too. The data sheet's PDF is not put in (it is the maker's to hand out). Reading one writes the STEP into the
//! library folder's `step` folder and points the part at it.

use crate::PartDef;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The file's suffix.
pub const SUFFIX: &str = "ccpart";

#[derive(Serialize, Deserialize)]
struct Bundle {
    /// "ccpart", to tell the file from a part's own JSON.
    format: String,
    version: u32,
    part: PartDef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    step: Option<StepFile>,
}

#[derive(Serialize, Deserialize)]
struct StepFile {
    /// The file's name as it was (no folder).
    name: String,
    /// Its bytes in base64.
    data: String,
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            out.push(if k <= c.len() { B64[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

fn decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = B64.iter().position(|b| *b == c).ok_or("STEP のデータが base64 ではありません (ファイルが壊れています)")? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// The part as one file's text: its JSON with the STEP file of `model3d_src` inside (when that file is on this computer).
/// The STEP's path in the part becomes its file name alone.
pub fn write(def: &PartDef) -> String {
    let mut part = def.clone();
    let step = part.model3d_src.as_mut().and_then(|src| {
        let path = Path::new(&src.path);
        let bytes = std::fs::read(path).ok()?;
        let name = path.file_name()?.to_string_lossy().into_owned();
        src.path = name.clone();
        Some(StepFile { name, data: encode(&bytes) })
    });
    serde_json::to_string_pretty(&Bundle { format: SUFFIX.into(), version: 1, part, step }).unwrap_or_default()
}

/// The part of a bundle's text (or of a part's own JSON), with its STEP written into `lib_dir/step` (a file of the same name
/// with other bytes is not overwritten: the new one gets a number) and `model3d_src` pointed at it.
pub fn read(text: &str, lib_dir: &Path) -> Result<PartDef, String> {
    let b: Bundle = match serde_json::from_str::<Bundle>(text) {
        Ok(b) if b.format == SUFFIX => b,
        Ok(b) => return Err(format!("部品のまとめのファイルではありません (format が「{}」)", b.format)),
        // a part's own JSON is read as it is
        Err(e) => return serde_json::from_str::<PartDef>(text).map_err(|_| format!("部品のファイルとして読めません: {e}")),
    };
    let mut part = b.part;
    if let (Some(step), Some(src)) = (b.step, part.model3d_src.as_mut()) {
        let bytes = decode(&step.data)?;
        let dir = lib_dir.join("step");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let safe: String = step.name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
        let (stem, ext) = safe.rsplit_once('.').map_or((safe.as_str(), ""), |(s, e)| (s, e));
        let mut path = dir.join(&safe);
        let mut n = 2;
        while path.exists() && std::fs::read(&path).ok().as_deref() != Some(&bytes[..]) {
            path = dir.join(format!("{stem}_{n}.{ext}"));
            n += 1;
        }
        std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        src.path = path.display().to_string();
    }
    Ok(part)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_goes_there_and_back() {
        for s in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0u8, 255, 128, 7, 9]] {
            assert_eq!(decode(&encode(s)).unwrap(), s, "{s:?}");
        }
        assert_eq!(encode(b"Man"), "TWFu");
        assert_eq!(encode(b"Ma"), "TWE=");
    }

    /// A part with its maker's STEP: the bundle carries the file, and reading it writes the file into the library's `step` folder
    /// (a second part with another file of that name gets a numbered one).
    #[test]
    fn a_part_and_its_step_travel_together() {
        let dir = std::env::temp_dir().join(format!("ccpart_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let step = dir.join("body.step");
        std::fs::write(&step, b"ISO-10303-21; body").unwrap();
        let mut def: PartDef = serde_json::from_str(crate::EXAMPLES.iter().find(|(n, _)| n.contains("NE555")).map_or(crate::EXAMPLES[0].1, |e| e.1)).unwrap();
        def.model3d_src = Some(pcb_core::mesh::ModelSource::new(&step.display().to_string()));
        let text = write(&def);
        assert!(text.contains("\"format\": \"ccpart\"") && !text.contains(&*dir.display().to_string().replace('\\', "\\\\")), "the maker's folder is not in it");
        let lib = dir.join("lib");
        let got = read(&text, &lib).unwrap();
        let src = got.model3d_src.clone().unwrap();
        assert_eq!(std::fs::read(&src.path).unwrap(), b"ISO-10303-21; body");
        assert_eq!(got.name, def.name);
        // another file of the same name
        std::fs::write(&step, b"ISO-10303-21; other").unwrap();
        let other = read(&write(&def), &lib).unwrap();
        assert!(other.model3d_src.unwrap().path.ends_with("body_2.step"));
        // a part's own JSON is read too
        assert_eq!(read(&serde_json::to_string(&def).unwrap(), &lib).unwrap().name, def.name);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
