//! `firmware`: building and loading the board's firmware with the outside tools (`sch_core::firmware`, candidate W4): the settings
//! kept with the schematic, the commands they make, which tools are missing, and running them.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use sch_core::firmware::{build_steps, flash_steps, missing, run_now, FirmwareBuild, Step, Toolchain};
use serde_json::json;

const USAGE: &str = "firmware [set toolchain=arduino|pico|ice40|custom folder=DIR board=FQBN|up5k:sg48 port=COM3 top=top pcf=top.pcf build=CMD flash=CMD] | firmware build|flash|all [dir=BASE]";

fn lines(steps: &[Step]) -> Vec<String> {
    steps.iter().map(|s| s.line()).collect()
}

impl Session {
    /// `firmware` (the settings and the commands), `firmware set key=value ...`, `firmware build|flash|all [dir=BASE]` (BASE: the folder
    /// the firmware folder is relative to, the current one by default).
    pub fn firmware(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let base = std::path::PathBuf::from(opt(&kv, "dir").unwrap_or("."));
        let what = pos.first().map(|s| s.as_str()).unwrap_or("show");
        let fw: &mut FirmwareBuild = &mut self.sch.firmware;
        match what {
            "set" => {
                for (k, v) in &kv {
                    match k.as_str() {
                        "toolchain" => {
                            Toolchain::parse(v).ok_or(format!("toolchain={v}: arduino・pico・ice40・custom のどれかにしてください"))?;
                            fw.toolchain = v.clone();
                        }
                        "folder" => fw.folder = v.clone(),
                        "board" => fw.board = v.clone(),
                        "port" => fw.port = v.clone(),
                        "top" => fw.top = v.clone(),
                        "pcf" => fw.pcf = v.clone(),
                        "build" => fw.build_cmd = v.clone(),
                        "flash" => fw.flash_cmd = v.clone(),
                        "dir" => {}
                        other => return Err(format!("{other}=: {USAGE}")),
                    }
                }
                self.show_firmware(&base)
            }
            "show" => self.show_firmware(&base),
            "build" | "flash" | "all" => {
                let mut steps = vec![];
                if what != "flash" {
                    steps.extend(build_steps(fw, &base)?);
                }
                if what != "build" {
                    // the load of a Pico looks for the .uf2 the build makes: worked out after the build when both run
                    if what == "all" && Toolchain::parse(&fw.toolchain) == Some(Toolchain::Pico) {
                        let (mut log, out) = run_now(&steps);
                        if out.is_ok() {
                            let more = flash_steps(fw, &base)?;
                            let (l2, o2) = run_now(&more);
                            log.extend(l2);
                            return Ok(obj(json!({"ok": o2.is_ok(), "error": o2.err(), "log": log})));
                        }
                        return Ok(obj(json!({"ok": false, "error": out.err(), "log": log})));
                    }
                    steps.extend(flash_steps(fw, &base)?);
                }
                let (log, out) = run_now(&steps);
                Ok(obj(json!({"ok": out.is_ok(), "error": out.err(), "commands": lines(&steps), "log": log})))
            }
            _ => Err(USAGE.into()),
        }
    }

    fn show_firmware(&self, base: &std::path::Path) -> Res {
        let fw = &self.sch.firmware;
        let build = build_steps(fw, base);
        let flash = flash_steps(fw, base);
        let all: Vec<Step> = build.iter().chain(flash.iter()).flatten().cloned().collect();
        let missing: Vec<serde_json::Value> = missing(&all).iter().map(|(t, how)| json!({"tool": t, "how": how})).collect();
        Ok(obj(json!({
            "settings": fw,
            "build": build.as_ref().map(|s| lines(s)).map_err(|e| e.clone()),
            "flash": flash.as_ref().map(|s| lines(s)).map_err(|e| e.clone()),
            "missing": missing,
        })))
    }
}
