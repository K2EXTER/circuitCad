//! `lineup FILE.pdf [series=FILE.elcap.json] [pick=PN,PN|bundle|all] [write] [dir=DIR]`: the lineup table of an
//! aluminium electrolytic capacitor's data sheet (`datasheet::lineup`), and the parts made from its rows with the series'
//! tables (`part_lib::elcap`); a power inductor's table when there is no capacitor's (`part_lib::coil`). Without `pick`
//! it lists the rows; `write` puts the picked parts into `DIR` (default: the session's library) and loads them.

use crate::Res;
use crate::session::{obj, opt, options};
use part_lib::{coil, elcap};
use serde_json::json;
use std::path::PathBuf;

/// A row read off the PDF as the part maker takes it.
pub fn elcap_row(r: &datasheet::lineup::ElcapRow) -> elcap::Row {
    elcap::Row {
        part: r.part.clone(),
        cap_uf: r.cap_uf,
        volts: r.volts.unwrap_or(0.0),
        d_mm: r.d_mm,
        l_mm: r.l_mm,
        impedance: r.impedance.clone(),
        ripple_a: r.ripple_a.unwrap_or(0.0),
        ripple_freq_hz: r.ripple_freq_hz,
        ripple_temp_c: r.ripple_temp_c,
        leak_ua: r.leak_ua,
        tan_d: r.tan_d,
        page: Some(r.page),
        mark: r.mark.clone(),
    }
}

/// An inductor's row read off the PDF as the part maker takes it.
pub fn coil_row(r: &datasheet::lineup::InductorRow) -> coil::Row {
    coil::Row { part: r.part.clone(), l_uh: r.l_uh, tol_pct: r.tol_pct, dcr_ohm: r.dcr_ohm, dcr_max: r.dcr_max, i_rms: r.i_rms, i_sat: r.i_sat, srf_hz: r.srf_hz }
}

impl crate::session::Session {
    /// `lineup` for a power inductor's table: the rows, or the picked parts made with the bundled series (`coils/`).
    fn lineup_coils(&mut self, rows: Vec<coil::Row>, words: &[String], kv: &[(String, String)]) -> Res {
        let series = match opt(kv, "series") {
            Some(n) => Some(coil::Series::named(n).ok_or_else(|| format!("コイルのシリーズ「{n}」は同梱にありません (crates/part-lib/coils/)"))?),
            None => rows.iter().find_map(|r| coil::Series::for_part(&r.part)),
        };
        let mut out = json!({"kind": "inductor", "rows": rows.len(), "series": series.as_ref().map(|s| format!("{} {}", s.maker, s.series))});
        let Some(pick) = opt(kv, "pick") else {
            let listed: Vec<_> = rows.iter().map(|r| json!({"part": r.part, "l_uh": r.l_uh, "tol_pct": r.tol_pct, "dcr_ohm": r.dcr_ohm, "dcr_max": r.dcr_max, "i_rms": r.i_rms, "i_sat": r.i_sat, "srf_hz": r.srf_hz})).collect();
            out["list"] = json!(listed);
            return Ok(obj(out));
        };
        let s = series.ok_or("コイルのシリーズの表がありません。同梱は crates/part-lib/coils/ を見本に作ってください")?;
        let names: Vec<String> = match pick {
            "bundle" => s.bundle.clone(),
            "all" => rows.iter().map(|r| r.part.clone()).collect(),
            list => list.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
        };
        let dir = opt(kv, "dir").map(PathBuf::from).unwrap_or_else(|| self.lib_dir.clone());
        let write = words.iter().any(|w| w.eq_ignore_ascii_case("write"));
        let (mut made, mut errors) = (vec![], vec![]);
        for n in &names {
            let Some(r) = rows.iter().find(|r| r.part.eq_ignore_ascii_case(n)) else {
                errors.push(format!("{n}: 表にありません"));
                continue;
            };
            match coil::make(&s, r) {
                Ok(p) => {
                    let mut m = json!({"part": p.name, "model": p.sim.as_ref().map(|m| m.text.clone())});
                    if write {
                        let path = elcap::write(&dir, &p)?;
                        m["written"] = json!(path.display().to_string());
                        if dir == self.lib_dir {
                            match self.lib.iter_mut().find(|x| x.name == p.name) {
                                Some(x) => *x = p,
                                None => self.lib.push(p),
                            }
                        }
                    }
                    made.push(m);
                }
                Err(e) => errors.push(e),
            }
        }
        out["made"] = json!(made);
        if !errors.is_empty() {
            out["errors"] = json!(errors);
        }
        Ok(obj(out))
    }

    pub fn lineup(&mut self, args: &[String]) -> Res {
        let (words, kv) = options(args);
        let usage = "lineup FILE.pdf [series=FILE.elcap.json|NAME] [pick=PN,PN|bundle|all] [write] [dir=DIR]";
        let file = words.iter().find(|w| !w.eq_ignore_ascii_case("write")).ok_or(usage)?;
        let ds = datasheet::Datasheet::open_file(&PathBuf::from(file.as_str()))?;
        let mut rows: Vec<elcap::Row> = ds.electrolytic_rows().iter().map(elcap_row).collect();
        if rows.is_empty() {
            let coils: Vec<coil::Row> = ds.inductor_rows().iter().map(coil_row).collect();
            if !coils.is_empty() {
                return self.lineup_coils(coils, &words, &kv);
            }
            return Err(format!("{file}: 型番ごとの行の表 (電解コンデンサは容量・大きさ・型番、コイルはインダクタンスと型番の列) が見つかりませんでした。文字の無い PDF や、列が崩れる PDF は読めません"));
        }
        let series = match opt(&kv, "series") {
            // a file, or the name of a bundled series ("ZLJ")
            Some(p) => match elcap::Series::named(p) {
                Some(s) => Some(s),
                None => Some(elcap::Series::load(&PathBuf::from(p))?),
            },
            None => elcap::Series::for_table(&rows, (0..ds.page_count().min(2)).flat_map(|p| ds.words(p).iter().map(|w| w.text.as_str()))),
        };
        // a table without part numbers (Rubycon's): named by the series' pattern
        elcap::name_rows(series.as_ref(), &mut rows);
        let listed: Vec<_> = rows
            .iter()
            .map(|r| {
                json!({"part": r.part, "page": r.page.map(|p| p + 1), "cap_uf": r.cap_uf, "volts": r.volts, "size": format!("{}x{}", r.d_mm, r.l_mm),
                       "impedance": r.impedance, "ripple_a": r.ripple_a, "ripple_hz": r.ripple_freq_hz, "ripple_temp_c": r.ripple_temp_c, "tan_d": r.tan_d, "leak_ua": r.leak_ua})
            })
            .collect();
        let mut out = json!({"rows": rows.len(), "series": series.as_ref().map(|s| format!("{} {}", s.maker, s.series))});
        if let Some(s) = &series {
            let bad: Vec<_> = elcap::leakage_mismatches(s, &rows).iter().map(|r| r.part.clone()).collect();
            if !bad.is_empty() {
                out["leakage_mismatch"] = json!(bad);
            }
        }
        let Some(pick) = opt(&kv, "pick") else {
            out["list"] = json!(listed);
            return Ok(obj(out));
        };
        let s = series.ok_or("シリーズの表 (series=FILE.elcap.json) がありません。同梱は crates/part-lib/elcaps/ を見本に作ってください")?;
        let names: Vec<String> = match pick {
            "bundle" => s.bundle.clone(),
            "all" => rows.iter().map(|r| r.part.clone()).collect(),
            list => list.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
        };
        let dir = opt(&kv, "dir").map(PathBuf::from).unwrap_or_else(|| self.lib_dir.clone());
        let write = words.iter().any(|w| w.eq_ignore_ascii_case("write"));
        let mut made = vec![];
        let mut errors = vec![];
        for n in &names {
            let Some(r) = rows.iter().find(|r| r.part.eq_ignore_ascii_case(n)) else {
                errors.push(format!("{n}: 表にありません"));
                continue;
            };
            match elcap::make(&s, r) {
                Ok(p) => {
                    let mut m = json!({"part": p.name, "model": p.sim.as_ref().map(|m| m.text.clone())});
                    if write {
                        let path = elcap::write(&dir, &p)?;
                        m["written"] = json!(path.display().to_string());
                        if dir == self.lib_dir {
                            match self.lib.iter_mut().find(|x| x.name == p.name) {
                                Some(x) => *x = p,
                                None => self.lib.push(p),
                            }
                        }
                    }
                    made.push(m);
                }
                Err(e) => errors.push(e),
            }
        }
        out["made"] = json!(made);
        if !errors.is_empty() {
            out["errors"] = json!(errors);
        }
        Ok(obj(out))
    }
}

#[cfg(test)]
mod real_pdf {
    use super::*;

    /// Rubycon YXF (YXF.pdf, obsolete; skipped without the file in microohm-meter/): its table is drawn with glyphs
    /// that have no Unicode (read back with the OCR, Windows only), it has no part numbers, two tables stand side by
    /// side and the rated voltage is written once beside each group.
    #[test]
    fn rubycon_yxf_without_text_or_part_numbers() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter/YXF.pdf");
        let Ok(ds) = datasheet::Datasheet::open_file(&path) else { return };
        if datasheet::ocr::max_side().is_none() {
            return;
        }
        let mut rows: Vec<elcap::Row> = ds.electrolytic_rows().iter().map(elcap_row).collect();
        let s = elcap::Series { series: "YXF".into(), part_pattern: "{V}YXF{C}MEFC{D}X{L}".into(), ..Default::default() };
        elcap::name_rows(Some(&s), &mut rows);
        assert_eq!(rows.len(), 64);
        let count = |v: f64| rows.iter().filter(|r| r.volts == v).count();
        assert_eq!([6.3, 10.0, 16.0, 25.0, 35.0, 50.0].map(count), [11, 10, 10, 10, 9, 14]);
        let r = rows.iter().find(|r| r.part == "16YXF1000MEFC10X20").expect("16 V 1000 uF");
        assert_eq!((r.ripple_a, r.impedance.clone()), (1.05, vec![(20.0, 100e3, 0.078), (-10.0, 100e3, 0.3)]));
        let r = rows.iter().find(|r| r.part == "50YXF2.2MEFC5X11").expect("50 V 2.2 uF, the right table");
        assert_eq!((r.ripple_a, r.impedance.clone()), (0.043, vec![(20.0, 100e3, 2.5), (-10.0, 100e3, 6.0)]));
    }

    /// Rubycon ZLJ (ZLJ.pdf; skipped without the file in microohm-meter/): 278 rows on three pages of two tables
    /// each, the "●" rows (EFU) told apart, every part number once, the series found from the title.
    #[test]
    fn rubycon_zlj_with_marked_rows() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter/ZLJ.pdf");
        let Ok(ds) = datasheet::Datasheet::open_file(&path) else { return };
        let mut rows: Vec<elcap::Row> = ds.electrolytic_rows().iter().map(elcap_row).collect();
        let s = elcap::Series::for_table(&rows, ds.words(0).iter().map(|w| w.text.as_str())).expect("ZLJ from the title");
        elcap::name_rows(Some(&s), &mut rows);
        assert_eq!(rows.len(), 278);
        assert_eq!(rows.iter().filter(|r| r.part.contains("MEFU")).count(), 14);
        let mut names: Vec<&str> = rows.iter().map(|r| r.part.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 278, "every part number once");
        let r = rows.iter().find(|r| r.part == "100ZLJ270MEFU12.5X30").expect("a low mark");
        assert_eq!((r.ripple_a, r.impedance[0].2), (2.52, 0.033));
        for b in &s.bundle {
            assert!(rows.iter().any(|r| &r.part == b), "{b} in the table");
        }
    }

    /// Nichicon UVR (e-uvr.pdf; skipped without the file in microohm-meter/): no impedance column, two leakage columns
    /// of which the second is empty from 160 V; every row's leakage after 2 minutes is 0.01 CV up to 100 V.
    #[test]
    fn nichicon_uvr_general_purpose() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter/e-uvr.pdf");
        let Ok(ds) = datasheet::Datasheet::open_file(&path) else { return };
        let rows: Vec<elcap::Row> = ds.electrolytic_rows().iter().map(elcap_row).collect();
        assert_eq!(rows.len(), 111);
        let s = elcap::Series::for_table(&rows, std::iter::empty()).expect("UVR");
        assert!(elcap::leakage_mismatches(&s, &rows).is_empty());
        let r = rows.iter().find(|r| r.part == "UVR1H101MPD").unwrap();
        assert_eq!((r.tan_d, r.leak_ua, r.ripple_a, r.ripple_freq_hz, r.ripple_temp_c), (Some(0.12), Some(50.0), 0.26, 120.0, 85.0));
        let r = rows.iter().find(|r| r.part == "UVR2W470MHD").unwrap();
        assert_eq!((r.volts, r.leak_ua), (450.0, Some(946.0)));
        for b in &s.bundle {
            assert!(rows.iter().any(|r| &r.part == b), "{b} in the table");
        }
    }

    /// Nichicon UPW (e-upw.pdf; skipped without the file in microohm-meter/): 197 rows on four pages, every leakage
    /// value 0.03 CV.
    #[test]
    fn nichicon_upw() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter/e-upw.pdf");
        let Ok(ds) = datasheet::Datasheet::open_file(&path) else { return };
        let rows: Vec<elcap::Row> = ds.electrolytic_rows().iter().map(elcap_row).collect();
        assert_eq!(rows.len(), 197);
        let s = elcap::Series::for_table(&rows, std::iter::empty()).expect("UPW");
        assert!(elcap::leakage_mismatches(&s, &rows).is_empty());
        let r = rows.iter().find(|r| r.part == "UPW1H821MHD6").unwrap();
        assert_eq!((r.d_mm, r.l_mm, r.ripple_a, r.impedance.clone()), (18.0, 20.0, 2.42, vec![(20.0, 100e3, 0.042), (-10.0, 100e3, 0.084)]));
        for b in &s.bundle {
            assert!(rows.iter().any(|r| &r.part == b), "{b} in the table");
        }
    }

    /// Bourns SRN6045 (SRN6045.pdf; skipped without the file in microohm-meter/): a power inductor's table, with a
    /// figure of the drawing beside it on the 22 uH row's line.
    #[test]
    fn bourns_srn6045_inductors() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter/SRN6045.pdf");
        let Ok(ds) = datasheet::Datasheet::open_file(&path) else { return };
        assert!(ds.electrolytic_rows().is_empty());
        let rows: Vec<coil::Row> = ds.inductor_rows().iter().map(coil_row).collect();
        assert_eq!(rows.len(), 16);
        let r = rows.iter().find(|r| r.part == "SRN6045-220M").unwrap();
        assert_eq!((r.l_uh, r.tol_pct, r.dcr_ohm, r.dcr_max, r.i_rms, r.i_sat, r.srf_hz), (22.0, Some(20.0), Some(0.142), true, Some(1.5), Some(1.9), Some(11e6)));
        let s = coil::Series::for_part(&rows[0].part).expect("SRN6045");
        for b in &s.bundle {
            let r = rows.iter().find(|r| &r.part == b).unwrap_or_else(|| panic!("{b} in the table"));
            let bundled = part_lib::EXAMPLES.iter().find(|e| e.0 == b).unwrap().1;
            assert_eq!(coil::to_text(&coil::make(&s, r).unwrap()), bundled.replace("\r\n", "\n"), "{b}");
        }
    }
}
