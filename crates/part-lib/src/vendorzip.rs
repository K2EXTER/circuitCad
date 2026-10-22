//! SPICE models a maker publishes as a zip (Murata's `grm-*.zip`, ...) as a searchable index.
//!
//! The zip stays where the user put it (the makers' terms of use forbid redistribution); only its path is remembered and
//! a model's text is read out of it when the user picks it. Any zip works: every `.subckt` in a SPICE text file becomes
//! an entry searchable by name. A maker-specific reader (`EXTRACTORS`) can add what the name does not say: size,
//! dielectric, capacitance and rating, so that the list can be narrowed by them.

use crate::passive::{Generated, eng};
use crate::PinDef;
use sch_core::PinSide;
use std::io::Read;
use std::path::{Path, PathBuf};

/// File types searched for models.
const EXTENSIONS: &[&str] = &["mod", "lib", "cir", "sp", "spi", "spice", "sub", "inc", "ckt", "txt"];
/// A model file longer than this is skipped (a data sheet or a listing, not a model).
const MAX_FILE: u64 = 4 << 20;

/// What a maker's reader found out about a capacitor model.
#[derive(Clone, Debug, PartialEq)]
pub struct CapMeta {
    /// Metric code of the size, "1608" (empty when the maker's data does not give one).
    pub size_metric: String,
    /// Inch code of the size, "0603" (empty when not given).
    pub size_inch: String,
    /// "X5R", "X7R", "C0G", "SL", ... (empty when not given).
    pub dielectric: String,
    /// Nominal capacitance in farads.
    pub capacitance: f64,
    /// Rated voltage in volts.
    pub voltage: f64,
    /// One line for the part's description ("村田 GRM... 1608(0603) X5R 10uF 10V").
    pub summary: String,
    /// What the model leaves out or only approximates, one sentence each (becomes the part's `limits`).
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VendorEntry {
    /// The `.subckt` name, which is the maker's part number.
    pub part_number: String,
    /// Where the file is inside the zip.
    pub entry: String,
    /// Maker name when a reader recognised the file ("村田"), else empty.
    pub maker: String,
    /// Number of nodes on the `.subckt` line.
    pub nodes: usize,
    pub meta: Option<CapMeta>,
}

#[derive(Clone, Debug, Default)]
pub struct Filter {
    /// Words that must all appear in the part number.
    pub text: String,
    pub size_inch: String,
    pub dielectric: String,
    pub c_min: Option<f64>,
    pub c_max: Option<f64>,
    pub v_min: Option<f64>,
}

impl Filter {
    /// Whether the filter asks for something only a maker's reader can answer.
    pub fn needs_meta(&self) -> bool {
        !(self.size_inch.is_empty() && self.dielectric.is_empty() && self.c_min.is_none() && self.c_max.is_none() && self.v_min.is_none())
    }
}

#[derive(Clone, Debug)]
pub struct VendorIndex {
    pub path: PathBuf,
    pub entries: Vec<VendorEntry>,
}

/// A maker's reader: `(path inside the zip, file text)` -> (maker, capacitor figures). Add one per maker whose files
/// say more than the name (look at a real zip first: no two makers write the same header).
type Extractor = fn(&str, &str) -> Option<(&'static str, CapMeta)>;
const EXTRACTORS: &[Extractor] = &[murata, chemicon];

/// "1608M(0603)/X5R(-55to85[deg])/1.00e+01[uF]/10[V]" -> size, dielectric, capacitance, voltage.
fn parse_murata_description(d: &str) -> Option<(String, String, String, f64, f64)> {
    let mut it = d.trim().split('/');
    let size = it.next()?;
    let (metric, rest) = size.split_once('(')?;
    let metric = metric.trim_end_matches(|c: char| !c.is_ascii_digit()).to_string();
    let inch = rest.split(')').next()?.to_string();
    let dielectric = it.next()?.split('(').next()?.trim().to_string();
    let cap = it.next()?;
    let (num, unit) = cap.split_once('[')?;
    let scale = match unit.trim_end_matches(']') {
        "uF" => 1e-6,
        "nF" => 1e-9,
        "pF" => 1e-12,
        "F" => 1.0,
        _ => return None,
    };
    let volt = it.next()?.trim().trim_end_matches("[V]").parse::<f64>().ok()?;
    // the file gives 3 significant digits; rounding keeps 10 uF from coming out as 9.999999999999999e-6
    let farads = format!("{:.2e}", num.trim().parse::<f64>().ok()? * scale).parse::<f64>().ok()?;
    Some((metric, inch, dielectric, farads, volt))
}

fn murata(path: &str, text: &str) -> Option<(&'static str, CapMeta)> {
    if !text.contains("Murata") {
        return None;
    }
    let desc = text.lines().find_map(|l| l.split_once("Description :").map(|(_, d)| d.to_string()))?;
    let (size_metric, size_inch, dielectric, capacitance, voltage) = parse_murata_description(&desc)?;
    let high_k = !path.starts_with("Temperature_compensating");
    let mut notes = vec!["このモデルは 25℃・直流電圧 0V・小信号の値です。温度による変化は入っていません。".to_string()];
    if high_k {
        notes.push(format!(
            "直流の電圧をかけると容量が下がる性質 (DCバイアス特性) は入っていません。{size_inch} の {dielectric} は、定格 {}V に近い電圧では容量が半分以下になることもあります。",
            eng(voltage)
        ));
    }
    notes.push("周波数は 100Hz〜6GHz (温度補償型は 1MHz〜6GHz) の範囲で合わせたモデルです。範囲の外は信用できません。".into());
    let part = text.lines().find_map(|l| l.split_once("Murata P/N :").map(|(_, p)| p.trim().to_string())).unwrap_or_default();
    let summary = format!("村田 {part} {size_metric}({size_inch}) {dielectric} {}F {}V。メーカーが公開しているSPICEモデル (ESR・ESLを含む等価回路) です", eng(capacitance), eng(voltage));
    Some(("村田", CapMeta { size_metric, size_inch, dielectric, capacitance, voltage, summary, notes }))
}

/// Nippon Chemi-Con (日本ケミコン) aluminium / polymer electrolytics. The file has no description line: the figures come
/// from the part number (series, voltage `250` = 25.0 V or `6R3` = 6.3 V, three letters, capacitance code `821` = 82 x 10^1 uF,
/// tolerance), and are used only when the capacitors of the ladder add up to about that capacitance.
fn chemicon(_path: &str, text: &str) -> Option<(&'static str, CapMeta)> {
    if !text.contains("Chemi-Con") {
        return None;
    }
    let part = text.lines().find_map(|l| l.split_once("Part No.").and_then(|(_, p)| p.trim_start().strip_prefix(':')).map(|p| p.trim().to_string()))?;
    let temp = text.lines().find_map(|l| l.split_once("Temperature").and_then(|(_, p)| p.trim_start().strip_prefix(':')).map(|p| p.trim().to_string())).unwrap_or_default();
    let b = part.as_bytes();
    let letters = b.iter().take_while(|c| c.is_ascii_uppercase()).count();
    let rest = part.get(letters..)?;
    let v = rest.get(0..3)?;
    let voltage = if let Some((a, c)) = v.split_once('R') { format!("{a}.{c}").parse::<f64>().ok()? } else { v.parse::<f64>().ok()? / 10.0 };
    let code = rest.get(6..9)?;
    let digits: u32 = code.parse().ok()?;
    let capacitance = format!("{:.2e}", f64::from(digits / 10) * 10f64.powi((digits % 10) as i32) * 1e-6).parse::<f64>().ok()?;
    if !rest.get(3..6)?.bytes().all(|c| c.is_ascii_uppercase()) || voltage <= 0.0 || capacitance <= 0.0 {
        return None;
    }
    // the model's own capacitors (C1, C2, ...) against the nominal capacitance
    let sum: f64 = text
        .lines()
        .filter(|l| l.trim_start().starts_with(['C', 'c']) && !l.trim_start().starts_with('*'))
        .filter_map(|l| l.split_whitespace().nth(3).and_then(|v| v.parse::<f64>().ok()))
        .sum();
    if !(0.5..=1.5).contains(&(sum / capacitance)) {
        return None;
    }
    let mut notes = vec![format!("メーカーのRCはしご形モデル (7素子) です。{}の値で、温度による変化・直流電圧・寿命は入っていません。", if temp.is_empty() { "ある1つの温度" } else { temp.as_str() })];
    notes.push("電解コンデンサは極性があります。モデルは小信号の値で、逆電圧をかけたときや、リップル電流による発熱・寿命は扱いません。".into());
    let summary = format!("日本ケミコン {part} {}F {}V。メーカーが公開しているSPICEモデル (ESR・ESLを含む等価回路) です", eng(capacitance), eng(voltage));
    Some(("日本ケミコン", CapMeta { size_metric: String::new(), size_inch: String::new(), dielectric: String::new(), capacitance, voltage, summary, notes }))
}

/// The `.subckt` lines of a text: (name, number of nodes). Continuation lines (`+`) are not followed.
fn subckts(text: &str) -> Vec<(String, usize)> {
    let mut out = vec![];
    for line in text.lines() {
        let l = line.trim_start();
        if l.len() < 8 || !l[..7].eq_ignore_ascii_case(".subckt") || !l[7..].starts_with(char::is_whitespace) {
            continue;
        }
        let mut words = l[7..].split_whitespace();
        let Some(name) = words.next() else { continue };
        let nodes = words.take_while(|w| !w.contains('=') && !w.eq_ignore_ascii_case("params:")).count();
        out.push((name.to_string(), nodes));
    }
    out
}

/// The text of one `.subckt ... .ends` (inner subcircuits included) and the `.param`/`.model`/`.func` lines before it.
fn extract_block(text: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let head = |l: &str, key: &str| {
        let t = l.trim_start();
        t.len() > key.len() && t[..key.len()].eq_ignore_ascii_case(key) && t[key.len()..].starts_with(char::is_whitespace)
    };
    let start = lines.iter().position(|l| head(l, ".subckt") && l.trim_start()[7..].split_whitespace().next().is_some_and(|n| n.eq_ignore_ascii_case(name)))?;
    let mut depth = 0;
    let mut end = None;
    for (i, l) in lines.iter().enumerate().skip(start) {
        if head(l, ".subckt") {
            depth += 1;
        } else if l.trim_start().len() >= 5 && l.trim_start()[..5].eq_ignore_ascii_case(".ends") {
            depth -= 1;
            if depth == 0 {
                end = Some(i);
                break;
            }
        }
    }
    let end = end?;
    let mut out: Vec<&str> = lines[..start]
        .iter()
        .copied()
        .filter(|l| [".param", ".model", ".func", ".global"].iter().any(|k| l.trim_start().len() >= k.len() && l.trim_start()[..k.len()].eq_ignore_ascii_case(k)))
        .collect();
    out.extend(&lines[start..=end]);
    Some(out.join("\n"))
}

impl VendorIndex {
    /// Reads the SPICE text files of the zip. A few seconds at most for a maker's whole capacitor set (12 thousand files).
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("モデルのzipを開けませんでした ({}): {e}。設定でzipの場所を選び直してください", path.display()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("zipとして読めませんでした: {e}。メーカーのサイトからダウンロードしたzipを選んでください"))?;
        let mut entries = vec![];
        for i in 0..zip.len() {
            let Ok(f) = zip.by_index(i) else { continue };
            let name = f.name().to_string();
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            if f.is_dir() || !EXTENSIONS.contains(&ext.as_str()) || f.size() > MAX_FILE {
                continue;
            }
            let mut bytes = vec![];
            if f.take(MAX_FILE).read_to_end(&mut bytes).is_err() {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            let found = subckts(&text);
            // a maker's own header describes a file with one model
            let reader = if found.len() == 1 { EXTRACTORS.iter().find_map(|x| x(&name, &text)) } else { None };
            for (part_number, nodes) in found {
                let (maker, meta) = match &reader {
                    Some((m, meta)) => (m.to_string(), Some(meta.clone())),
                    None => (String::new(), None),
                };
                entries.push(VendorEntry { part_number, entry: name.clone(), maker, nodes, meta });
            }
        }
        if entries.is_empty() {
            return Err("このzipには、SPICEのモデル (.subckt を含む .mod・.lib・.cir など) が見つかりませんでした。メーカーの「SPICE」「netlist」のページからダウンロードしたzipを選んでください".into());
        }
        entries.sort_by(|a, b| {
            let c = |e: &VendorEntry| e.meta.as_ref().map_or(f64::MAX, |m| m.capacitance);
            (c(a), &a.part_number).partial_cmp(&(c(b), &b.part_number)).unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(Self { path: path.to_path_buf(), entries })
    }

    /// Sizes present, as inch codes, smallest first.
    pub fn sizes(&self) -> Vec<String> {
        let mut v: Vec<(String, String)> = self.entries.iter().filter_map(|e| e.meta.as_ref()).filter(|m| !m.size_inch.is_empty()).map(|m| (m.size_metric.clone(), m.size_inch.clone())).collect();
        v.sort();
        v.dedup();
        v.into_iter().map(|(_, i)| i).collect()
    }

    pub fn dielectrics(&self) -> Vec<String> {
        let mut v: Vec<String> = self.entries.iter().filter_map(|e| e.meta.as_ref()).map(|m| m.dielectric.clone()).filter(|d| !d.is_empty()).collect();
        v.sort();
        v.dedup();
        v
    }

    /// Entries a maker's reader described (the size, capacitance and rating filters work on these only).
    pub fn described(&self) -> usize {
        self.entries.iter().filter(|e| e.meta.is_some()).count()
    }

    pub fn search(&self, f: &Filter) -> Vec<&VendorEntry> {
        let words: Vec<String> = f.text.split_whitespace().map(|w| w.to_ascii_uppercase()).collect();
        // "0603" is 1608 in inch codes and 0201 in metric ones: an inch code wins, the metric code is the fallback.
        let size_is_inch = self.entries.iter().filter_map(|e| e.meta.as_ref()).any(|m| m.size_inch == f.size_inch);
        self.entries
            .iter()
            .filter(|e| {
                let pn = e.part_number.to_ascii_uppercase();
                words.iter().all(|w| pn.contains(w.as_str()))
            })
            .filter(|e| {
                if !f.needs_meta() {
                    return true;
                }
                let Some(m) = &e.meta else { return false };
                (f.size_inch.is_empty() || if size_is_inch { m.size_inch == f.size_inch } else { m.size_metric == f.size_inch })
                    && (f.dielectric.is_empty() || m.dielectric.eq_ignore_ascii_case(&f.dielectric))
                    && f.c_min.is_none_or(|v| m.capacitance >= v * 0.999)
                    && f.c_max.is_none_or(|v| m.capacitance <= v * 1.001)
                    && f.v_min.is_none_or(|v| m.voltage >= v)
            })
            .collect()
    }

    /// The file text of a model.
    pub fn read_file(&self, e: &VendorEntry) -> Result<String, String> {
        let file = std::fs::File::open(&self.path).map_err(|err| format!("モデルのzipを開けませんでした ({}): {err}", self.path.display()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|err| format!("zipとして読めませんでした: {err}"))?;
        let mut f = zip.by_name(&e.entry).map_err(|err| format!("{} がzipの中にありません: {err}", e.part_number))?;
        let mut bytes = vec![];
        f.read_to_end(&mut bytes).map_err(|err| format!("{} を読めませんでした: {err}", e.part_number))?;
        Ok(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
    }

    /// What a part form needs to make a two-pin part from a model: the pins, the `.subckt` and the Japanese notes on what
    /// the model leaves out.
    pub fn generate(&self, e: &VendorEntry) -> Result<Generated, String> {
        if e.nodes != 2 {
            return Err(format!("{} は端子が {} 個のモデルです。ここで作れるのは、端子が2個 (コンデンサ・抵抗・コイルなど) のモデルだけです", e.part_number, e.nodes));
        }
        let text = self.read_file(e)?;
        let block = extract_block(&text, &e.part_number).ok_or_else(|| format!("{} の .subckt が .ends で閉じていません", e.part_number))?;
        let pin = |n: &str, side| PinDef { number: n.into(), name: n.into(), side: Some(side), sim_only: false };
        let (value_text, description, limits) = match &e.meta {
            Some(m) => (eng(m.capacitance), m.summary.clone(), m.notes.clone()),
            None => (
                String::new(),
                format!("{}。メーカーが公開しているSPICEモデルです", e.part_number),
                vec!["メーカーのモデルです。どんな条件の値か (温度・直流電圧・周波数の範囲) は、zip の説明か元のファイルで確かめてください。".to_string()],
            ),
        };
        Ok(Generated {
            pins: vec![pin("1", PinSide::Left), pin("2", PinSide::Right)],
            sim_name: e.part_number.clone(),
            sim_text: block,
            sim_pins: vec!["1".into(), "2".into()],
            value_text,
            description,
            limits,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn murata_model(pn: &str, desc: &str) -> String {
        format!("* Murata Manufacturing\r\n* Description :{desc}\r\n* Murata P/N :{pn}\r\n.SUBCKT {pn} Port1 Port2\r\nC01 Port1 N01 1e-6\r\nR01 N01 Port2 1e-2\r\n.ENDS {pn}\r\n")
    }

    const OTHER_LIB: &str = "* some other maker\n.param k=2\n.SUBCKT CAP_A 1 2\nC1 1 2 {k}u\n.ends CAP_A\n.subckt NEST a b c\nX1 a b INNER\n.subckt INNER x y\nR1 x y 1\n.ends\n.ends NEST\n.SUBCKT WITHPARAM 1 2 params: r=10\nR1 1 2 {r}\n.ENDS\n";

    fn make_zip(dir: &Path) -> PathBuf {
        let p = dir.join("grm.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (path, pn, d) in [
            ("High_dielectric_constant/1608_0603/GRM188R61A106MAAL.mod", "GRM188R61A106MAAL", "1608M(0603)/X5R(-55to85[deg])/1.00e+01[uF]/10[V]"),
            ("High_dielectric_constant/1005_0402/GRM155R71C104KA88.mod", "GRM155R71C104KA88", "1005M(0402)/X7R(-55to125[deg])/1.00e-01[uF]/16[V]"),
            ("Temperature_compensating/1005_0402/GRM1555C1H101JA01.mod", "GRM1555C1H101JA01", "1005M(0402)/C0G(-55to125[deg])/1.00e+02[pF]/50[V]"),
        ] {
            z.start_file(path, o).unwrap();
            z.write_all(murata_model(pn, d).as_bytes()).unwrap();
        }
        z.start_file("other/lib.lib", o).unwrap();
        z.write_all(OTHER_LIB.as_bytes()).unwrap();
        z.start_file("readme.pdf", o).unwrap();
        z.write_all(b"not a model").unwrap();
        z.finish().unwrap();
        p
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc_vendorzip_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_description_gives_size_dielectric_capacitance_and_voltage() {
        let (m, i, d, c, v) = parse_murata_description("1608M(0603)/X5R(-55to85[deg])/1.00e+01[uF]/10[V]").unwrap();
        assert_eq!((m.as_str(), i.as_str(), d.as_str(), v), ("1608", "0603", "X5R", 10.0));
        assert_eq!(c, 10e-6);
        let (_, _, d, c, _) = parse_murata_description("1005M(0402)/C0G(-55to125[deg])/1.00e+02[pF]/50[V]").unwrap();
        assert_eq!(d, "C0G");
        assert!((c - 100e-12).abs() < 1e-18);
        assert!(parse_murata_description("nonsense").is_none());
    }

    #[test]
    fn subckt_lines_give_names_and_node_counts() {
        let s = subckts(OTHER_LIB);
        assert_eq!(s, vec![("CAP_A".to_string(), 2), ("NEST".to_string(), 3), ("INNER".to_string(), 2), ("WITHPARAM".to_string(), 2)]);
    }

    #[test]
    fn a_block_keeps_inner_subcircuits_and_the_params_before_it() {
        let b = extract_block(OTHER_LIB, "NEST").unwrap();
        assert!(b.starts_with(".param k=2
.subckt NEST") && b.contains(".subckt INNER") && b.trim_end().ends_with(".ends NEST"), "{b}");
        let a = extract_block(OTHER_LIB, "CAP_A").unwrap();
        assert!(a.starts_with(".param k=2") && a.contains("C1 1 2 {k}u") && !a.contains("NEST"), "{a}");
        assert!(extract_block(OTHER_LIB, "NOPE").is_none());
    }

    #[test]
    fn every_subckt_of_any_model_file_is_listed_and_a_makers_reader_adds_figures() {
        let d = tmp("index");
        let idx = VendorIndex::open(&make_zip(&d)).unwrap();
        assert_eq!(idx.entries.len(), 3 + 4, "3 Murata files + 4 subckts of the other library; the pdf is skipped");
        assert_eq!(idx.described(), 3);
        let g = idx.entries.iter().find(|e| e.part_number == "GRM1555C1H101JA01").unwrap();
        assert_eq!((g.maker.as_str(), g.meta.as_ref().map(|m| m.notes.iter().any(|n| n.contains("DCバイアス")))), ("村田", Some(false)));
        assert!(idx.entries.iter().find(|e| e.part_number == "CAP_A").unwrap().meta.is_none());
        assert_eq!(idx.sizes(), vec!["0402".to_string(), "0603".to_string()]);
        assert_eq!(idx.dielectrics(), vec!["C0G".to_string(), "X5R".to_string(), "X7R".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn filters_combine() {
        let d = tmp("filter");
        let idx = VendorIndex::open(&make_zip(&d)).unwrap();
        let by = |f: Filter| idx.search(&f).iter().map(|e| e.part_number.clone()).collect::<Vec<_>>();
        assert_eq!(by(Filter::default()).len(), 7, "no filter: everything, also what no reader described");
        assert_eq!(by(Filter { text: "cap_a".into(), ..Default::default() }), vec!["CAP_A"]);
        assert_eq!(by(Filter { text: "grm155 zzz".into(), ..Default::default() }), Vec::<String>::new(), "every word must appear in the name");
        assert_eq!(by(Filter { text: "r71c grm155".into(), ..Default::default() }), vec!["GRM155R71C104KA88"]);
        assert_eq!(by(Filter { size_inch: "0402".into(), ..Default::default() }).len(), 2, "a size filter leaves out entries without figures");
        assert_eq!(by(Filter { size_inch: "1005".into(), ..Default::default() }).len(), 2, "metric code works when it is not an inch code");
        assert_eq!(by(Filter { size_inch: "0603".into(), ..Default::default() }), vec!["GRM188R61A106MAAL"], "0603 is the inch code, not metric 0603");
        assert_eq!(by(Filter { c_min: Some(1e-6), ..Default::default() }), vec!["GRM188R61A106MAAL"]);
        assert_eq!(by(Filter { v_min: Some(20.0), ..Default::default() }), vec!["GRM1555C1H101JA01"]);
        assert_eq!(by(Filter { dielectric: "x5r".into(), ..Default::default() }), vec!["GRM188R61A106MAAL"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_two_node_model_comes_out_as_two_pins_a_subckt_and_notes() {
        let d = tmp("gen");
        let idx = VendorIndex::open(&make_zip(&d)).unwrap();
        let by = |pn: &str| idx.entries.iter().find(|e| e.part_number == pn).unwrap();
        let g = idx.generate(by("GRM188R61A106MAAL")).unwrap();
        assert!(g.sim_text.starts_with(".SUBCKT GRM188R61A106MAAL Port1 Port2"));
        assert!(!g.sim_text.contains('\r'));
        assert_eq!((g.sim_pins, g.value_text.as_str()), (vec!["1".to_string(), "2".to_string()], "10u"));
        assert!(g.limits.iter().any(|l| l.contains("DCバイアス")), "high-k parts warn about DC bias");
        assert!(!idx.generate(by("GRM1555C1H101JA01")).unwrap().limits.iter().any(|l| l.contains("DCバイアス")), "C0G does not");
        // a model no reader described still works, with the figures left for the user to type
        let o = idx.generate(by("CAP_A")).unwrap();
        assert!(o.sim_text.contains("C1 1 2 {k}u") && o.value_text.is_empty());
        assert!(o.limits.iter().any(|l| l.contains("確かめてください")));
        // a subcircuit with a parameter list still has 2 nodes; one with 3 is refused
        assert_eq!(by("WITHPARAM").nodes, 2);
        assert!(idx.generate(by("NEST")).unwrap_err().contains("3 個"));
        let _ = std::fs::remove_dir_all(&d);
    }


    const CHEMICON: &str = "*$
*//   Part No.    : APSW250ELL821MJB5S
*//   Temperature : 20deg.C
*//   Copyright (C) 2025 Nippon Chemi-Con Corporation.
.SUBCKT APSW250ELL821MJB5S_20deg-C 1 2
C1 4 2 5.33e-04
C2 5 2 1.61e-04
C3 6 2 5.93e-05
R1 3 4 7.54e-03
R2 4 5 4.25e-02
R3 5 6 5.48e-02
L1 1 3 4.20e-09
.ENDS
*$
";

    #[test]
    fn nippon_chemi_con_figures_come_from_the_part_number_when_the_model_agrees() {
        let (maker, m) = chemicon("x/APSW250ELL821MJB5S.lib", CHEMICON).unwrap();
        assert_eq!(maker, "日本ケミコン");
        assert_eq!((m.voltage, m.size_inch.as_str()), (25.0, ""));
        assert!((m.capacitance - 820e-6).abs() < 1e-12, "821 = 82 x 10^1 uF: {}", m.capacitance);
        assert!(m.notes[0].contains("20deg.C") && m.summary.contains("日本ケミコン APSW250ELL821MJB5S"));
        // 6.3 V is written 6R3, and the capacitance code is 3 digits
        let small = CHEMICON.replace("APSW250ELL821MJB5S", "APSW6R3ELL101MJB5S").replace("5.33e-04", "6.0e-05").replace("1.61e-04", "2.0e-05").replace("5.93e-05", "1.0e-05");
        assert_eq!(chemicon("x.lib", &small).map(|(_, m)| (m.voltage, m.capacitance)), Some((6.3, 100e-6)));
        // a model whose capacitors do not add up to the nominal value is not described (the name is not trusted)
        assert!(chemicon("x.lib", &CHEMICON.replace("5.33e-04", "5.33e-07")).is_none());
        assert!(chemicon("x.lib", "* some other maker
.SUBCKT A 1 2
.ends
").is_none());
    }

    #[test]
    fn an_electrolytic_model_in_a_zip_is_found_by_capacitance_and_rating() {
        let d = tmp("chem");
        let p = d.join("psw.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        z.start_file("SPICE_POLYMER-LEAD-PSW/APSW250ELL821MJB5S.lib", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(CHEMICON.as_bytes()).unwrap();
        z.finish().unwrap();
        let idx = VendorIndex::open(&p).unwrap();
        assert_eq!(idx.entries[0].part_number, "APSW250ELL821MJB5S_20deg-C");
        assert_eq!(idx.entries.len(), 1);
        let hit = |f: Filter| idx.search(&f).len();
        assert_eq!(hit(Filter { c_min: Some(500e-6), v_min: Some(16.0), ..Default::default() }), 1);
        assert_eq!(hit(Filter { v_min: Some(35.0), ..Default::default() }), 0);
        let g = idx.generate(&idx.entries[0]).unwrap();
        assert_eq!(g.value_text, "820u");
        assert!(g.sim_text.starts_with(".SUBCKT APSW250ELL821MJB5S_20deg-C 1 2") && g.sim_text.contains("L1 1 3 4.20e-09"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bad_zip_says_what_to_do() {
        let d = tmp("bad");
        let p = d.join("x.zip");
        std::fs::write(&p, b"nope").unwrap();
        assert!(VendorIndex::open(&p).unwrap_err().contains("zip"));
        assert!(VendorIndex::open(&d.join("missing.zip")).unwrap_err().contains("選び直して"));
        let empty = d.join("e.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&empty).unwrap());
        z.start_file("a.txt", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"hello").unwrap();
        z.finish().unwrap();
        assert!(VendorIndex::open(&empty).unwrap_err().contains("見つかりませんでした"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `VENDOR_ZIP=<zip> cargo test -p part-lib vendor_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn vendor_real_zip_is_indexed() {
        let Ok(p) = std::env::var("VENDOR_ZIP") else { return };
        let t = std::time::Instant::now();
        let idx = VendorIndex::open(Path::new(&p)).unwrap();
        println!("{} models ({} described) in {:?}; sizes {:?}; dielectrics {:?}", idx.entries.len(), idx.described(), t.elapsed(), idx.sizes(), idx.dielectrics());
        let two = idx.entries.iter().filter(|e| e.nodes == 2).count();
        println!("{two} with 2 nodes");
        let e = idx.entries.iter().find(|e| e.nodes == 2).unwrap();
        assert!(idx.generate(e).unwrap().sim_text.to_ascii_lowercase().contains(".ends"));
    }
}
