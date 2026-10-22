//! Murata's SPICE netlists for chip ceramic capacitors (`grm-*.zip` from their site) as a searchable index.
//! The zip stays where the user put it (the terms of use forbid redistribution); only its path is remembered
//! and a part's `.SUBCKT` is read out of it when the user picks the part.

use crate::passive::{Generated, eng};
use crate::PinDef;
use sch_core::PinSide;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Dielectric group the zip is divided into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// X5R / X7R and the like: large capacitance, drops with DC bias.
    HighK,
    /// C0G / SL and the like: small capacitance, stable.
    TempComp,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MurataEntry {
    pub part_number: String,
    /// Metric code of the size, "1608".
    pub size_metric: String,
    /// Inch code of the size, "0603".
    pub size_inch: String,
    /// "X5R", "X7R", "C0G", "SL", ...
    pub dielectric: String,
    /// Nominal capacitance in farads.
    pub capacitance: f64,
    /// Rated voltage in volts.
    pub voltage: f64,
    pub family: Family,
    /// Where the `.mod` file is inside the zip.
    pub entry: String,
}

#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub text: String,
    pub size_inch: String,
    pub dielectric: String,
    pub c_min: Option<f64>,
    pub c_max: Option<f64>,
    pub v_min: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct MurataIndex {
    pub path: PathBuf,
    pub entries: Vec<MurataEntry>,
}

/// "1608M(0603)/X5R(-55to85[deg])/1.00e+01[uF]/10[V]" -> (1608, 0603, X5R, 10e-6, 10).
fn parse_description(d: &str) -> Option<(String, String, String, f64, f64)> {
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

impl MurataIndex {
    /// Reads the head of every `.mod` file in the zip. A few seconds at most for the whole GRM set (12 thousand files).
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("村田のモデルのzipを開けませんでした ({}): {e}。設定でzipの場所を選び直してください", path.display()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("zipとして読めませんでした: {e}。村田の「SPICE netlist」からダウンロードしたzipを選んでください"))?;
        let mut entries = vec![];
        for i in 0..zip.len() {
            let Ok(mut f) = zip.by_index(i) else { continue };
            let name = f.name().to_string();
            if !name.to_ascii_lowercase().ends_with(".mod") {
                continue;
            }
            let family = if name.starts_with("Temperature_compensating") { Family::TempComp } else { Family::HighK };
            let mut head = vec![0u8; 700];
            let n = f.read(&mut head).unwrap_or(0);
            let head = String::from_utf8_lossy(&head[..n]);
            let Some(desc) = head.lines().find_map(|l| l.split_once("Description :").map(|(_, d)| d.to_string())) else { continue };
            let Some((size_metric, size_inch, dielectric, capacitance, voltage)) = parse_description(&desc) else { continue };
            let part_number = name.rsplit('/').next().unwrap_or(&name).rsplit_once('.').map_or(String::new(), |(s, _)| s.to_string());
            entries.push(MurataEntry { part_number, size_metric, size_inch, dielectric, capacitance, voltage, family, entry: name });
        }
        if entries.is_empty() {
            return Err("このzipには村田のコンデンサのモデル (.mod) が見つかりませんでした。村田の「SPICE netlist」(GRM シリーズ) のzipを選んでください".into());
        }
        entries.sort_by(|a, b| (a.capacitance, &a.part_number).partial_cmp(&(b.capacitance, &b.part_number)).unwrap_or(std::cmp::Ordering::Equal));
        Ok(Self { path: path.to_path_buf(), entries })
    }

    /// Sizes present, as inch codes, smallest first.
    pub fn sizes(&self) -> Vec<String> {
        let mut v: Vec<(String, String)> = self.entries.iter().map(|e| (e.size_metric.clone(), e.size_inch.clone())).collect();
        v.sort();
        v.dedup();
        v.into_iter().map(|(_, i)| i).collect()
    }

    pub fn dielectrics(&self) -> Vec<String> {
        let mut v: Vec<String> = self.entries.iter().map(|e| e.dielectric.clone()).collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn search(&self, f: &Filter) -> Vec<&MurataEntry> {
        let text = f.text.trim().to_ascii_uppercase();
        // "0603" is 1608 in inch codes and 0201 in metric ones: an inch code wins, the metric code is the fallback.
        let size_is_inch = self.entries.iter().any(|e| e.size_inch == f.size_inch);
        self.entries
            .iter()
            .filter(|e| text.is_empty() || e.part_number.to_ascii_uppercase().contains(&text))
            .filter(|e| f.size_inch.is_empty() || if size_is_inch { e.size_inch == f.size_inch } else { e.size_metric == f.size_inch })
            .filter(|e| f.dielectric.is_empty() || e.dielectric.eq_ignore_ascii_case(&f.dielectric))
            .filter(|e| f.c_min.is_none_or(|v| e.capacitance >= v * 0.999))
            .filter(|e| f.c_max.is_none_or(|v| e.capacitance <= v * 1.001))
            .filter(|e| f.v_min.is_none_or(|v| e.voltage >= v))
            .collect()
    }

    /// The `.mod` file text of a part.
    pub fn read_model(&self, e: &MurataEntry) -> Result<String, String> {
        let file = std::fs::File::open(&self.path).map_err(|err| format!("村田のモデルのzipを開けませんでした ({}): {err}", self.path.display()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|err| format!("zipとして読めませんでした: {err}"))?;
        let mut f = zip.by_name(&e.entry).map_err(|err| format!("{} がzipの中にありません: {err}", e.part_number))?;
        let mut bytes = vec![];
        f.read_to_end(&mut bytes).map_err(|err| format!("{} を読めませんでした: {err}", e.part_number))?;
        Ok(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
    }

    /// What a part form needs to make the part: two pins, the `.SUBCKT` and the Japanese notes on what the model leaves out.
    pub fn generate(&self, e: &MurataEntry) -> Result<Generated, String> {
        let text = self.read_model(e)?;
        let start = text.to_ascii_uppercase().find(".SUBCKT").ok_or_else(|| format!("{} に .SUBCKT がありません", e.part_number))?;
        let block = text[start..].to_string();
        let value_text = eng(e.capacitance);
        let pin = |n: &str, side| PinDef { number: n.into(), name: n.into(), side: Some(side), sim_only: false };
        let mut limits = vec![
            "村田のモデルは 25℃・直流電圧 0V・小信号の値です。温度による変化は入っていません。".to_string(),
        ];
        if e.family == Family::HighK {
            limits.push(format!(
                "直流の電圧をかけると容量が下がる性質 (DCバイアス特性) は入っていません。{} の {} は、定格 {}V に近い電圧では容量が半分以下になることもあります。",
                e.size_inch,
                e.dielectric,
                eng(e.voltage)
            ));
        }
        limits.push("周波数は 100Hz〜6GHz (温度補償型は 1MHz〜6GHz) の範囲で合わせたモデルです。範囲の外は信用できません。".into());
        Ok(Generated {
            pins: vec![pin("1", PinSide::Left), pin("2", PinSide::Right)],
            sim_name: e.part_number.clone(),
            sim_text: block,
            sim_pins: vec!["1".into(), "2".into()],
            value_text: value_text.clone(),
            description: format!(
                "村田 {} {}({}) {} {}F {}V。村田が公開しているSPICEモデル (ESR・ESLを含む等価回路) です",
                e.part_number, e.size_metric, e.size_inch, e.dielectric, value_text, eng(e.voltage)
            ),
            limits,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn model(pn: &str, desc: &str) -> String {
        format!("* hdr\r\n* Description :{desc}\r\n* Murata P/N :{pn}\r\n.SUBCKT {pn} Port1 Port2\r\nC01 Port1 N01 1e-6\r\nR01 N01 Port2 1e-2\r\n.ENDS {pn}\r\n")
    }

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
            z.write_all(model(pn, d).as_bytes()).unwrap();
        }
        z.start_file("readme.txt", o).unwrap();
        z.write_all(b"not a model").unwrap();
        z.finish().unwrap();
        p
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc_murata_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_description_gives_size_dielectric_capacitance_and_voltage() {
        let (m, i, d, c, v) = parse_description("1608M(0603)/X5R(-55to85[deg])/1.00e+01[uF]/10[V]").unwrap();
        assert_eq!((m.as_str(), i.as_str(), d.as_str(), v), ("1608", "0603", "X5R", 10.0));
        assert_eq!(c, 10e-6);
        let (_, _, d, c, _) = parse_description("1005M(0402)/C0G(-55to125[deg])/1.00e+02[pF]/50[V]").unwrap();
        assert_eq!(d, "C0G");
        assert!((c - 100e-12).abs() < 1e-18);
        assert!(parse_description("nonsense").is_none());
    }

    #[test]
    fn the_index_lists_models_and_ignores_other_files() {
        let d = tmp("index");
        let idx = MurataIndex::open(&make_zip(&d)).unwrap();
        assert_eq!(idx.entries.len(), 3);
        assert_eq!(idx.entries[0].part_number, "GRM1555C1H101JA01", "smallest capacitance first");
        assert_eq!(idx.entries[0].family, Family::TempComp);
        assert_eq!(idx.sizes(), vec!["0402".to_string(), "0603".to_string()]);
        assert_eq!(idx.dielectrics(), vec!["C0G".to_string(), "X5R".to_string(), "X7R".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn filters_combine() {
        let d = tmp("filter");
        let idx = MurataIndex::open(&make_zip(&d)).unwrap();
        let by = |f: Filter| idx.search(&f).iter().map(|e| e.part_number.clone()).collect::<Vec<_>>();
        assert_eq!(by(Filter { size_inch: "0402".into(), ..Default::default() }).len(), 2);
        assert_eq!(by(Filter { size_inch: "1005".into(), ..Default::default() }).len(), 2, "metric code works when it is not an inch code");
        assert_eq!(by(Filter { size_inch: "0603".into(), ..Default::default() }), vec!["GRM188R61A106MAAL"], "0603 is the inch code, not metric 0603");
        assert_eq!(by(Filter { c_min: Some(1e-6), ..Default::default() }), vec!["GRM188R61A106MAAL"]);
        assert_eq!(by(Filter { v_min: Some(20.0), ..Default::default() }), vec!["GRM1555C1H101JA01"]);
        assert_eq!(by(Filter { text: "grm155r".into(), ..Default::default() }), vec!["GRM155R71C104KA88"]);
        assert_eq!(by(Filter { dielectric: "x5r".into(), ..Default::default() }), vec!["GRM188R61A106MAAL"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_part_comes_out_as_two_pins_and_a_subckt_with_notes() {
        let d = tmp("gen");
        let idx = MurataIndex::open(&make_zip(&d)).unwrap();
        let e = idx.entries.iter().find(|e| e.part_number == "GRM188R61A106MAAL").unwrap();
        let g = idx.generate(e).unwrap();
        assert!(g.sim_text.starts_with(".SUBCKT GRM188R61A106MAAL Port1 Port2"));
        assert!(!g.sim_text.contains('\r'));
        assert_eq!(g.sim_pins, vec!["1", "2"]);
        assert_eq!(g.value_text, "10u");
        assert!(g.limits.iter().any(|l| l.contains("DCバイアス")), "high-k parts warn about DC bias");
        let c0g = idx.entries.iter().find(|e| e.family == Family::TempComp).unwrap();
        assert!(!idx.generate(c0g).unwrap().limits.iter().any(|l| l.contains("DCバイアス")), "C0G does not");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bad_zip_says_what_to_do() {
        let d = tmp("bad");
        let p = d.join("x.zip");
        std::fs::write(&p, b"nope").unwrap();
        assert!(MurataIndex::open(&p).unwrap_err().contains("zip"));
        assert!(MurataIndex::open(&d.join("missing.zip")).unwrap_err().contains("選び直して"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `MURATA_ZIP=<grm-*.zip> cargo test -p part-lib murata_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn murata_real_zip_is_indexed() {
        let Ok(p) = std::env::var("MURATA_ZIP") else { return };
        let t = std::time::Instant::now();
        let idx = MurataIndex::open(Path::new(&p)).unwrap();
        println!("{} models in {:?}; sizes {:?}; dielectrics {:?}", idx.entries.len(), t.elapsed(), idx.sizes(), idx.dielectrics());
        let f = Filter { size_inch: "0603".into(), dielectric: "X5R".into(), c_min: Some(9.9e-6), c_max: Some(10.1e-6), ..Default::default() };
        for e in idx.search(&f).iter().take(5) {
            println!("{} {}V", e.part_number, e.voltage);
        }
        let e = idx.search(&f)[0];
        assert!(idx.generate(e).unwrap().sim_text.contains(".ENDS"));
    }
}
