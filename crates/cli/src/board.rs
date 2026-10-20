//! Board commands: new, import, autoplace, route, drc, status, postlayout, currents, save, open, fab.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use pcb_core::autoroute::{autoroute, RouteOptions};
use pcb_core::model::Board;
use pcb_core::netlist::{self, NetlistNet, NetlistPart};
use serde_json::{json, Value};

const DEFAULT_SIZE: (f64, f64) = (60.0, 40.0);

impl Session {
    pub fn board_cmd(&mut self, args: &[String]) -> Res {
        let Some(sub) = args.first() else { return Err("board new|import|autoplace|route|drc|status|postlayout|currents|save|open|fab".into()) };
        let rest = &args[1..];
        match sub.to_ascii_lowercase().as_str() {
            "new" => {
                let size = match rest {
                    [] => DEFAULT_SIZE,
                    [w, h] => (num(w)?, num(h)?),
                    _ => return Err("board new [WIDTH HEIGHT]  (mm)".into()),
                };
                self.board = Some(Board::new("board", size.0, size.1));
                Ok(obj(json!({"width": size.0, "height": size.1})))
            }
            "import" => {
                let gate = self.erc_gate()?;
                let circuit_ok = gate.is_none();
                let (parts, nets) = self.netlist_of();
                if parts.is_empty() {
                    return Err("the schematic has no parts to import".into());
                }
                let board = self.board.get_or_insert_with(|| Board::new("board", DEFAULT_SIZE.0, DEFAULT_SIZE.1));
                // library parts bring their own footprint
                for c in self.sch.components.iter().filter(|c| c.custom.is_some()) {
                    if let Some(def) = self.lib.iter().find(|d| d.footprint_name() == c.footprint)
                        && let Ok(fp) = def.footprint()
                    {
                        board.add_footprint(fp);
                    }
                }
                let rep = netlist::import(board, &parts, &nets);
                Ok(obj(json!({"circuit_ok": circuit_ok, "added": rep.added, "removed": rep.removed, "updated": rep.updated, "warnings": rep.warnings.iter().cloned().chain(gate).collect::<Vec<_>>()})))
            }
            "autoplace" => {
                let board = self.board_mut()?;
                let r = pcb_core::autoplace::autoplace(board)?;
                Ok(obj(json!({"placed": r.placed, "width": r.width, "height": r.height})))
            }
            "route" => {
                let (_, kv) = options(rest);
                let only = opt(&kv, "net").map(str::to_string);
                let board = self.board_mut()?;
                let rep = autoroute(board, &RouteOptions::default(), only.as_deref());
                let failed: Vec<Value> = rep.failed.iter().map(|a| json!({"net": a.net})).collect();
                Ok(obj(json!({"routed": rep.routed, "failed": failed, "unrouted_left": pcb_core::ratsnest::ratsnest(board).len()})))
            }
            "drc" => {
                let board = self.board_mut()?;
                let v = pcb_core::drc::check(board);
                let list: Vec<Value> = v.iter().take(60).map(|x| json!({"message": x.message, "at": [x.at.x, x.at.y]})).collect();
                Ok(obj(json!({"violations": v.len(), "shown": list.len(), "list": list})))
            }
            "status" => {
                let board = self.board_mut()?;
                let unrouted = pcb_core::ratsnest::ratsnest(board).len();
                let drc = pcb_core::drc::check(board).len();
                Ok(obj(json!({"parts": board.parts.len(), "pads": board.pads.len(), "tracks": board.tracks.len(), "vias": board.vias.len(), "unrouted": unrouted, "drc_violations": drc})))
            }
            "currents" => {
                let (words, kv) = options(rest);
                let widen = words.iter().any(|w| w.eq_ignore_ascii_case("widen"));
                let use_tran = words.iter().any(|w| w.eq_ignore_ascii_case("tran"));
                let c = self.circuit()?;
                let mut stats: Vec<Value> = vec![];
                let currents = if use_tran {
                    let tr = self.last_tran.as_ref().ok_or("board currents tran: run 'tran' first (the current is read from its result)")?;
                    let from = opt(&kv, "from").map(|f| f.parse::<f64>().ok().filter(|v| (0.0..0.95).contains(v)).ok_or("from=F is the part of the run to skip, 0 to 0.9 (default 0.5: the second half, without the start-up)")).transpose()?.unwrap_or(0.5);
                    let st = sim_core::net_currents_tran(&c, tr, from);
                    let r3 = |x: f64| (x * 1e3).round() / 1e3;
                    stats = st
                        .iter()
                        .map(|s| json!({"net": s.net, "rms": r3(s.rms), "burst_rms_10ms": r3(s.burst_rms), "peak": r3(s.peak), "spiky": s.spiky()}))
                        .collect();
                    // the RMS of the settled part is what the copper heats with; the peak is only shown
                    st.iter().map(|s| (s.net.clone(), s.rms)).collect::<Vec<_>>()
                } else {
                    let dc = sim_core::dc_op(&c).map_err(|e| e.to_string())?;
                    sim_core::net_currents(&c, &dc)
                };
                let board = self.board_mut()?;
                let n = pcb_core::current::set_net_currents(board, &currents);
                let spec = pcb_core::current::CopperSpec::default();
                let conn = pcb_core::drc::connectivity(board);
                let mut widened = 0;
                if widen {
                    let thin: Vec<(String, f64)> = pcb_core::current::net_widths(board, &conn, &spec).into_iter().filter(|w| w.too_thin()).map(|w| (w.net, pcb_core::current::round_up_width(w.needed))).collect();
                    for (net, width) in thin {
                        widened += pcb_core::current::widen_net(board, &conn, &net, width);
                    }
                }
                let conn = pcb_core::drc::connectivity(board);
                let mut widths = pcb_core::current::net_widths(board, &conn, &spec);
                widths.sort_by(|a, b| b.amps.partial_cmp(&a.amps).unwrap());
                let list: Vec<Value> = widths
                    .iter()
                    .map(|w| json!({"net": w.net, "amps": (w.amps * 1e4).round() / 1e4, "needed_mm": pcb_core::current::round_up_width(w.needed), "thinnest_mm": w.thinnest, "too_thin": w.too_thin()}))
                    .collect();
                let source = if use_tran { "RMS of the settled part of the last 'tran' (time-weighted); 'stats' also has the largest 10 ms burst RMS and the peak, which are not used. spiky:true = peak above 10x RMS, a start-up surge or a spike that a real source would limit" } else { "the DC operating point" };
                Ok(obj(json!({"nets": n, "widened_tracks": widened, "list": list, "stats": stats,
                    "note": format!("Currents are from {source} (half the sum of the terminal currents on each net), widths from IPC-2221 at +10 C on 35 um outer copper. 'board currents [tran] widen' thickens the thin tracks; run 'board drc' after, then 'board postlayout on' to see the effect.")})))
            }
            "postlayout" => {
                let on = match rest.first().map(|w| w.to_ascii_lowercase()).as_deref() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("board postlayout on|off  (on: dc/tran/ac/noise add the copper resistance of the routed board in series with each pad)".into()),
                };
                let board = self.board_mut()?;
                let spec = pcb_core::irdrop::IrSpec::default();
                let mut w = pcb_core::irdrop::wiring_resistances(board, &spec);
                let couplings = pcb_core::irdrop::wiring_couplings(&w, &spec);
                let strongest: Vec<Value> = couplings.iter().take(5).map(|c| json!({"pads": [format!("{}.{}", c.a.0, c.a.1), format!("{}.{}", c.b.0, c.b.1)], "k": (c.k * 1e3).round() / 1e3})).collect();
                w.sort_by(|a, b| b.ohm.partial_cmp(&a.ohm).unwrap());
                let list: Vec<Value> = w.iter().take(10).map(|x| json!({"net": x.net, "pad": format!("{}.{}", x.refdes, x.pin), "milliohm": (x.ohm * 1e6).round() / 1e3, "nanohenry": (x.henry * 1e10).round() / 10.0, "picofarad": (x.farad * 1e13).round() / 10.0})).collect();
                self.post_layout = on;
                self.last_dc = None;
                self.last_tran = None;
                self.last_ac = None;
                Ok(obj(json!({"postlayout": on, "pads_with_resistance": w.len(), "largest": list, "couplings": couplings.len(), "strongest_couplings": strongest,
                    "note": "Resistance and inductance are measured from each net's supply pad (a connector if there is one) through tracks, vias and pours; unrouted nets add nothing, so run 'board route' first. The inductance is per pad, and so is the capacitance (the copper over a pour, taken as ground: 'picofarad', a flat estimate within about 10 %, neighbouring traces not counted; with the inductance it makes a long trace ring); 'couplings' counts the magnetic coupling (k) between wires of different nets, which lowers the loop inductance of a supply trace and its close return (V(REF.PIN.wire) probes the pin). Both show in ac and fast tran, not in dc."})))
            }
            "save" => {
                let f = rest.first().ok_or("board save FILE.pcbjson")?;
                let board = self.board_mut()?;
                let text = serde_json::to_string_pretty(&*board).map_err(|e| e.to_string())?;
                std::fs::write(f, text).map_err(|e| format!("{f}: {e}"))?;
                Ok(obj(json!({"file": f})))
            }
            "open" => {
                let f = rest.first().ok_or("board open FILE.pcbjson")?;
                let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
                self.board = Some(serde_json::from_str::<Board>(&text).map_err(|e| format!("{f}: {e}"))?);
                Ok(obj(json!({"file": f})))
            }
            "check" => {
                // the same pre-order check as the app's "発注の準備"
                let (_, kv) = options(rest);
                let fab = match opt(&kv, "fab") {
                    Some(id) => Some(pcb_core::fab::preset(id).ok_or_else(|| format!("unknown fab '{id}' (use one of: {})", pcb_core::fab::PRESETS.iter().map(|p| p.id).collect::<Vec<_>>().join(", ")))?),
                    None => None,
                };
                let (parts, nets) = self.netlist_of();
                let board = self.board_mut()?;
                let mut checks = pcb_core::fab::preflight(board, fab);
                // a board that no longer matches the schematic (nothing to compare before the first import or without a schematic)
                if !board.parts.is_empty() && !parts.is_empty() {
                    let d = pcb_core::netlist::diff(board, &parts, &nets);
                    if !d.is_empty() {
                        checks.insert(0, pcb_core::fab::schematic_sync_check(&d.summary(), d.blocking()));
                    }
                }
                use pcb_core::fab::Level;
                let count = |l: Level| checks.iter().filter(|c| c.level == l).count();
                let name = |l: Level| match l {
                    Level::Ok => "ok",
                    Level::Info => "info",
                    Level::Warn => "warn",
                    Level::Error => "error",
                };
                // what is fine is only counted; the rest comes with its reason and where it is
                let list: Vec<Value> = checks
                    .iter()
                    .filter(|c| c.level != Level::Ok)
                    .map(|c| {
                        let mut v = json!({"level": name(c.level), "title": c.title, "detail": c.detail});
                        if let Some(at) = c.at {
                            v["at"] = json!([(at.x * 100.0).round() / 100.0, (at.y * 100.0).round() / 100.0]);
                            v["places"] = json!(1 + c.also.len());
                        }
                        v
                    })
                    .collect();
                let (errors, warnings) = (count(Level::Error), count(Level::Warn));
                Ok(obj(json!({"ready": pcb_core::fab::ready(&checks), "fab": fab.map(|f| f.id), "ok": count(Level::Ok), "info": count(Level::Info), "warnings": warnings, "errors": errors, "list": list,
                    "note": "ready:false means at least one error; fix those first (warnings can be ordered but are probably not what was meant). The numbers of the fab presets are a guide: look at the fab's own page before ordering. Add fab=jlcpcb|pcbway|oshpark|standard to also check that fab's minimums and price tier."})))
            }
            "fab" => {
                let dir = std::path::Path::new(rest.first().ok_or("board fab DIR")?);
                let board = self.board_mut()?;
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let mut files = vec![];
                for (name, data) in pcb_core::export::fab_files(board).into_iter().chain(pcb_core::assembly::assembly_files(board)) {
                    std::fs::write(dir.join(&name), data).map_err(|e| e.to_string())?;
                    files.push(name);
                }
                let zip = pcb_core::export::zip_bytes(board).map_err(|e| e.to_string())?;
                let zip_name = format!("{}_gerber.zip", board.name);
                std::fs::write(dir.join(&zip_name), zip).map_err(|e| e.to_string())?;
                files.push(zip_name);
                Ok(obj(json!({"dir": dir.display().to_string(), "files": files})))
            }
            other => Err(format!("unknown board command '{other}'")),
        }
    }

    fn board_mut(&mut self) -> Result<&mut Board, String> {
        self.board.as_mut().ok_or_else(|| "there is no board yet (use 'board import' or 'board new' first)".to_string())
    }

    /// The parts and nets of the schematic as the board import sees them (same rules as the app).
    fn netlist_of(&self) -> (Vec<NetlistPart>, Vec<NetlistNet>) {
        let sch = &self.sch;
        let parts = sch
            .components
            .iter()
            .filter(|c| !c.kind.is_power_symbol())
            .map(|c| NetlistPart { refdes: c.refdes.clone(), value: c.value.clone(), footprint: c.footprint_name() })
            .collect();
        let nets = sch_core::extract(sch)
            .into_iter()
            .map(|n| NetlistNet {
                name: n.name,
                // simulation-only pins (sensor stimulus inputs) have no pad
                pins: n
                    .pins
                    .into_iter()
                    .filter(|p| {
                        !sch.components
                            .iter()
                            .find(|c| c.refdes == p.refdes)
                            .and_then(|c| c.custom.as_ref())
                            .is_some_and(|s| s.pins.iter().any(|sp| sp.sim_only && sp.number == p.pin))
                    })
                    .map(|p| (p.refdes, p.pin))
                    .collect(),
            })
            .collect();
        (parts, nets)
    }
}

fn num(s: &str) -> Result<f64, String> {
    s.parse::<f64>().ok().filter(|v| *v > 0.0 && v.is_finite()).ok_or_else(|| format!("'{s}' is not a positive size in mm"))
}
