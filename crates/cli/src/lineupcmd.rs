//! `lineup FILE.pdf [series=FILE.elcap.json] [pick=PN,PN|bundle|all] [write] [dir=DIR]`: the lineup table of an
//! aluminium electrolytic capacitor's data sheet (`datasheet::lineup`), and the parts made from its rows with the series'
//! tables (`part_lib::elcap`). Without `pick` it lists the rows; `write` puts the picked parts into `DIR` (default: the
//! session's library) and loads them.

use crate::Res;
use crate::session::{obj, opt, options};
use part_lib::elcap;
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
        page: Some(r.page),
    }
}

impl crate::session::Session {
    pub fn lineup(&mut self, args: &[String]) -> Res {
        let (words, kv) = options(args);
        let usage = "lineup FILE.pdf [series=FILE.elcap.json] [pick=PN,PN|bundle|all] [write] [dir=DIR]";
        let file = words.iter().find(|w| !w.eq_ignore_ascii_case("write")).ok_or(usage)?;
        let ds = datasheet::Datasheet::open_file(&PathBuf::from(file.as_str()))?;
        let rows: Vec<elcap::Row> = ds.electrolytic_rows().iter().map(elcap_row).collect();
        if rows.is_empty() {
            return Err(format!("{file}: 型番ごとの行の表 (容量・大きさ・型番の列) が見つかりませんでした。文字の無い PDF や、列が崩れる PDF は読めません"));
        }
        let series = match opt(&kv, "series") {
            Some(p) => Some(elcap::Series::load(&PathBuf::from(p))?),
            None => elcap::Series::for_part(&rows[0].part),
        };
        let listed: Vec<_> = rows
            .iter()
            .map(|r| {
                json!({"part": r.part, "page": r.page.map(|p| p + 1), "cap_uf": r.cap_uf, "volts": r.volts, "size": format!("{}x{}", r.d_mm, r.l_mm),
                       "impedance": r.impedance, "ripple_a": r.ripple_a, "leak_ua": r.leak_ua})
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
