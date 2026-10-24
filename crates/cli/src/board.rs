//! Board commands: new, import, pour, autoplace, move, route, improve, labels, class, impedance, drc, status, postlayout, currents, save, open, fab.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use pcb_core::autoroute::RouteOptions;
use pcb_core::model::Board;
use pcb_core::netlist::{self, NetlistNet, NetlistPart};
use serde_json::{json, Value};

const DEFAULT_SIZE: (f64, f64) = (60.0, 40.0);

impl Session {
    pub fn board_cmd(&mut self, args: &[String]) -> Res {
        let Some(sub) = args.first() else { return Err("board new|import|pour|autoplace|move|route|improve|labels|class|impedance|drc|status|postlayout|currents|save|open|fab".into()) };
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
                for c in self.sch.flat().components.iter().filter(|c| c.custom.is_some()) {
                    if let Some(def) = self.lib.iter().find(|d| d.footprint_name() == c.footprint)
                        && let Ok(fp) = def.footprint()
                    {
                        board.add_footprint(fp);
                        if let Some(m) = &def.model3d {
                            board.models3d.insert(def.footprint_name(), m.clone());
                        }
                    }
                }
                let rep = netlist::import(board, &parts, &nets);
                netlist::import_shops(board, &shops_of(&self.sch));
                netlist::import_hints(board, &netlist::NetHints { currents: self.sch.net_currents.clone().into_iter().collect(), pairs: self.sch.diff_pairs.clone() });
                Ok(obj(json!({"circuit_ok": circuit_ok, "added": rep.added, "removed": rep.removed, "updated": rep.updated, "warnings": rep.warnings.iter().cloned().chain(gate).collect::<Vec<_>>()})))
            }
            "autoplace" => {
                let board = self.board_mut()?;
                let r = pcb_core::autoplace::autoplace(board)?;
                Ok(obj(json!({"placed": r.placed, "width": r.width, "height": r.height, "poured_nets_ignored": r.poured, "repoured_vias": r.repoured, "airwire_crossings": r.crossings})))
            }
            "pour" => {
                // the GND pour of the editor: the bottom copper as a pour, with a via for every surface pad of the net
                let (_, kv) = options(rest);
                let net = opt(&kv, "net").map(str::to_string);
                let board = self.board_mut()?;
                let o = pcb_core::groundpour::PourOptions { net, top: opt(&kv, "top") != Some("0"), ..Default::default() };
                let r = pcb_core::groundpour::ground_pour(board, &o)?;
                Ok(obj(json!({"zone_added": r.zone_added, "top_added": r.top_added, "vias": r.vias, "already": r.already, "left": r.left})))
            }
            "move" => {
                // board move REF X Y [rot=0..3]: put a part (its origin) at X, Y mm; tracks are not moved
                let (words, kv) = options(rest);
                let [refdes, x, y] = words.as_slice() else { return Err("board move REF X Y [rot=0..3]  (mm; the part's origin; wiring is not moved)".into()) };
                let (x, y) = (num(x)?, num(y)?);
                let board = self.board_mut()?;
                let Some(part) = board.parts.iter().find(|p| p.refdes.eq_ignore_ascii_case(refdes)).cloned() else {
                    let known: Vec<&str> = board.parts.iter().map(|p| p.refdes.as_str()).collect();
                    return Err(format!("no part {refdes} on the board (parts: {})", known.join(" ")));
                };
                let rot = match opt(&kv, "rot") {
                    Some(r) => r.parse::<u8>().map_err(|_| format!("rot={r}: use 0, 1, 2 or 3 (quarter turns)"))? % 4,
                    None => part.rot,
                };
                board.place_part(&part.refdes, pcb_core::model::Point::new(x, y), rot);
                Ok(obj(json!({"part": part.refdes, "x": x, "y": y, "rot": rot})))
            }
            "impedance" => {
                // the impedance of a track and the width for a wanted one (a rule of thumb, +-10 %):
                //   board impedance [er=4.5] [height=1.53] [gap=0.36] [copper=35] [layer=outer|inner]
                //                   width=W [space=S]        -> the impedance of a track of width W (a pair S apart: also differential)
                //                   target=Z                 -> the width for Z ohm
                //                   diff=Z space=S          -> the width of each track of a pair S apart for Z ohm differential
                //                   diff=Z width=W          -> the space of a pair of tracks of width W for Z ohm differential
                // `er`, `height`, `gap`, `copper` set the layer stack of the board first (and keep it)
                let (_, kv) = options(rest);
                let board = self.board_mut()?;
                let get = |k: &str| -> Result<Option<f64>, String> { opt(&kv, k).map(|v| v.parse::<f64>().ok().filter(|x| *x > 0.0 && x.is_finite()).ok_or_else(|| format!("{k}={v}: a positive number is expected"))).transpose() };
                if let Some(v) = get("er")? {
                    board.stackup.er = v;
                }
                if let Some(v) = get("height")? {
                    board.stackup.height = v;
                }
                if let Some(v) = get("gap")? {
                    board.stackup.plane_gap = v;
                }
                if let Some(v) = get("copper")? {
                    board.stackup.copper_um = v;
                }
                let kind = match opt(&kv, "layer") {
                    Some("inner") => pcb_core::impedance::Kind::Stripline,
                    Some("outer") | None => pcb_core::impedance::Kind::Microstrip,
                    Some(o) => return Err(format!("layer={o}: use outer or inner")),
                };
                let s = board.stackup;
                let round = |x: f64| (x * 1000.0).round() / 1000.0;
                let (width, space, target, diff) = (get("width")?, get("space")?, get("target")?, get("diff")?);
                let mut out = json!({"layer": kind.label(), "stackup": {"er": s.er, "height": s.height, "gap": s.plane_gap, "copper_um": s.copper_um}});
                let m = out.as_object_mut().unwrap();
                match (target, diff) {
                    (Some(z), None) => {
                        let w = pcb_core::impedance::width_for(&s, kind, z).ok_or_else(|| format!("target={z}: no track width gives that impedance on this layer stack"))?;
                        m.insert("target".into(), json!(z));
                        m.insert("width".into(), json!(round(w)));
                    }
                    (None, Some(z)) => match (width, space) {
                        (None, Some(sp)) => {
                            let w = pcb_core::impedance::width_for_differential(&s, kind, sp, z).ok_or_else(|| format!("diff={z} space={sp}: no track width gives that differential impedance"))?;
                            m.insert("diff".into(), json!(z));
                            m.insert("space".into(), json!(sp));
                            m.insert("width".into(), json!(round(w)));
                        }
                        (Some(w), None) => {
                            let sp = pcb_core::impedance::space_for_differential(&s, kind, w, z).ok_or_else(|| format!("diff={z} width={w}: no space gives that differential impedance"))?;
                            m.insert("diff".into(), json!(z));
                            m.insert("width".into(), json!(w));
                            m.insert("space".into(), json!(round(sp)));
                        }
                        _ => return Err("diff=Z needs width=W (to find the space) or space=S (to find the width), one of them".into()),
                    },
                    (Some(_), Some(_)) => return Err("target=Z and diff=Z are two questions: ask one at a time".into()),
                    (None, None) => {
                        if let Some(w) = width {
                            m.insert("width".into(), json!(w));
                            m.insert("z0".into(), json!(round(pcb_core::impedance::single_ended(&s, kind, w))));
                            if let Some(sp) = space {
                                m.insert("space".into(), json!(sp));
                                m.insert("zdiff".into(), json!(round(pcb_core::impedance::differential(&s, kind, w, sp))));
                            }
                        }
                    }
                }
                Ok(obj(out))
            }
            "class" => {
                // net classes: `board class` lists them; `board class NAME [width=mm] [clearance=mm] [via=dia/drill] [nets=A,B]` makes or
                // changes one (a net listed in it leaves its other class); `board class NAME remove=1` deletes it
                let (pos, kv) = options(rest);
                let board = self.board_mut()?;
                let Some(name) = pos.first() else {
                    let list: Vec<Value> = board.net_classes.iter().map(|c| json!({"name": c.name, "nets": c.nets, "width": c.track_width, "clearance": c.clearance, "via": [c.via_diameter, c.via_drill]})).collect();
                    return Ok(obj(json!({"classes": list})));
                };
                if opt(&kv, "remove") == Some("1") {
                    let before = board.net_classes.len();
                    board.net_classes.retain(|c| &c.name != name);
                    return Ok(obj(json!({"removed": before - board.net_classes.len()})));
                }
                let k = match board.net_classes.iter().position(|c| &c.name == name) {
                    Some(k) => k,
                    None => {
                        board.net_classes.push(pcb_core::NetClass { name: name.clone(), ..Default::default() });
                        board.net_classes.len() - 1
                    }
                };
                let floor = board.rules.clone();
                if let Some(w) = opt(&kv, "width") {
                    let w = num(w)?;
                    if w + 1e-9 < floor.min_track_width {
                        return Err(format!("width={w}: thinner than the board's minimum track width {} mm", floor.min_track_width));
                    }
                    board.net_classes[k].track_width = w;
                }
                if let Some(c) = opt(&kv, "clearance") {
                    let c = num(c)?;
                    if c + 1e-9 < floor.min_clearance {
                        return Err(format!("clearance={c}: less than the board's minimum clearance {} mm", floor.min_clearance));
                    }
                    board.net_classes[k].clearance = c;
                }
                if let Some(v) = opt(&kv, "via") {
                    let (d, h) = v.split_once('/').ok_or("via=DIAMETER/DRILL, e.g. via=0.8/0.4")?;
                    let (d, h) = (num(d)?, num(h)?);
                    if h >= d {
                        return Err(format!("via={v}: the drill must be smaller than the diameter"));
                    }
                    (board.net_classes[k].via_diameter, board.net_classes[k].via_drill) = (d, h);
                }
                if let Some(n) = opt(&kv, "nets") {
                    let nets: Vec<String> = n.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    for (j, other) in board.net_classes.iter_mut().enumerate() {
                        if j != k {
                            other.nets.retain(|x| !nets.contains(x));
                        }
                    }
                    board.net_classes[k].nets = nets;
                }
                let c = &board.net_classes[k];
                Ok(obj(json!({"name": c.name, "nets": c.nets, "width": c.track_width, "clearance": c.clearance, "via": [c.via_diameter, c.via_drill]})))
            }
            "labels" => {
                // the part designators off the copper (the app's 「文字を配線からよける」): the ones that touch a track, a pad, a via, a hole or
                // another text go to the clearest spot around their part; what was placed by hand stays
                let board = self.board_mut()?;
                let r = pcb_core::labels::avoid_overlaps(board);
                Ok(obj(json!({"moved": r.moved, "still_touching": r.still_touching})))
            }
            "improve" => {
                // make the routes of the whole board shorter and its vias fewer: nets that detour (or carry a via) are ripped up
                // and routed again, alone or with a neighbour in the way; a change is kept only when it is better and clean
                let (_, kv) = options(rest);
                let only = opt(&kv, "net").map(str::to_string);
                let board = self.board_mut()?;
                let ro = RouteOptions { pad_lead: Some(0.5), ..RouteOptions::default() };
                let (tracks, vias, len) = (board.tracks.len(), board.vias.len(), board.tracks.iter().map(|t| t.a.dist(t.b)).sum::<f64>());
                let r = pcb_core::reroute::improve_routes(board, &ro, None, only.as_deref());
                let len_after: f64 = board.tracks.iter().map(|t| t.a.dist(t.b)).sum();
                Ok(obj(json!({"rerouted_nets": r.improved, "saved_mm": (r.saved * 10.0).round() / 10.0, "tracks": [tracks, board.tracks.len()], "vias": [vias, board.vias.len()], "length_mm": [(len * 10.0).round() / 10.0, (len_after * 10.0).round() / 10.0], "unrouted_left": pcb_core::ratsnest::ratsnest(board).len()})))
            }
            "route" => {
                let (_, kv) = options(rest);
                let only = opt(&kv, "net").map(str::to_string);
                // like the editor: when no currents are declared, the circuit's DC currents are taken first, so that the nets that
                // carry a lot of current get a track width that fits (`currents=0` turns it off; a circuit that does not solve is skipped)
                let none_declared = self.board_mut()?.net_currents.is_empty();
                let auto = if none_declared && opt(&kv, "currents") != Some("0") {
                    self.circuit().ok().and_then(|c| sim_core::dc_op(&c).ok().map(|dc| sim_core::net_currents(&c, &dc)))
                } else {
                    None
                };
                let board = self.board_mut()?;
                let auto_nets = auto.map_or(0, |list| pcb_core::current::set_net_currents(board, &list));
                // like the editor: leave and enter pads at a right angle (`lead=0` turns it off)
                let lead = match opt(&kv, "lead") {
                    Some(v) => num(v)?,
                    None => 0.5,
                };
                // neighbouring pads of one net (the two LX pins of a regulator) become one piece of copper first (`bridge=0` leaves it out)
                let bridged = if opt(&kv, "bridge") == Some("0") { 0 } else { pcb_core::bridge::bridge_pads(board) };
                let before = board.tracks.len();
                let ro = RouteOptions { pad_lead: (lead > 0.0).then_some(lead), ..RouteOptions::default() };
                let rep = pcb_core::autoroute::autoroute_sized(board, &ro, only.as_deref());
                let aligned = if lead > 0.0 && rep.routed > 0 {
                    let new: Vec<usize> = (before..board.tracks.len()).collect();
                    pcb_core::padentry::align_entries(board, &pcb_core::padentry::EntryOptions { lead, spread: true }, Some(&new)).aligned
                } else {
                    0
                };
                let joined = pcb_core::merge::merge_collinear(board, before);
                let aligned = aligned + if lead > 0.0 && rep.routed > 0 { pcb_core::padentry::align_entries(board, &pcb_core::padentry::EntryOptions { lead, spread: true }, Some(&(before..board.tracks.len()).collect::<Vec<_>>())).aligned } else { 0 };
                let joined = joined + pcb_core::merge::merge_collinear(board, before);
                let slits_filled = if rep.routed > 0 { pcb_core::drc::fill_slits(board) } else { 0 };
                // `improve=1`: then shorten the routes that detour and drop vias that are not needed. Off by default: right after
                // the router the gain is small (43 samples: 2 vias fewer in all); `board improve` is for hand-drawn routes
                let improved = if opt(&kv, "improve") == Some("1") && rep.failed.is_empty() { pcb_core::reroute::improve_routes(board, &ro, Some(before), None) } else { Default::default() };
                // the bottom has the GND pour: fill the top with ground too (`top=0` leaves it out)
                let top_pour = opt(&kv, "top") != Some("0") && rep.routed > 0 && board.zones.iter().any(|z| z.net == "GND" && z.side == pcb_core::model::Side::Bottom) && pcb_core::groundpour::top_pour(board, None) == Ok(true);
                let failed: Vec<Value> = rep.failed.iter().map(|a| json!({"net": a.net})).collect();
                Ok(obj(json!({"routed": rep.routed, "failed": failed, "aligned_pads": aligned, "joined_tracks": joined, "slits_filled": slits_filled, "pads_bridged": bridged, "top_pour": top_pour, "currents_declared": auto_nets, "wide_nets": rep.wide.iter().map(|(n, w)| json!({"net": n, "width_mm": w})).collect::<Vec<_>>(), "rerouted_nets": improved.improved, "saved_mm": (improved.saved * 10.0).round() / 10.0, "vias_change": improved.vias, "unrouted_left": pcb_core::ratsnest::ratsnest(board).len()})))
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
                let mut board = serde_json::from_str::<Board>(&text).map_err(|e| format!("{f}: {e}"))?;
                board.migrate_finish_exclude();
                self.board = Some(board);
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
            "shopping" => {
                // board shopping [boards=N] [spare=0] [file=PATH]: what to buy, with each shop's part number or a search link
                let (_, kv) = options(rest);
                let boards: usize = opt(&kv, "boards").map(|v| v.parse().map_err(|_| format!("boards={v}: a whole number is needed"))).transpose()?.unwrap_or(1);
                let spare_on = opt(&kv, "spare") != Some("0");
                let board = self.board_mut()?;
                let lines: Vec<Value> = pcb_core::shops::lines(board)
                    .iter()
                    .map(|l| {
                        let mut shops = serde_json::Map::new();
                        for (shop, s) in pcb_core::shops::SHOPS.iter().zip(&l.sources) {
                            shops.insert(shop.id.to_string(), match s {
                                pcb_core::shops::Source::Code { number, url } => json!({"number": number, "url": url}),
                                pcb_core::shops::Source::Search { url } => json!({"search": url}),
                            });
                        }
                        {
                        let (need, spare) = (l.qty * boards.max(1), if spare_on { l.spare(boards) } else { 0 });
                        json!({"value": l.value, "footprint": l.footprint, "refdes": l.refdes, "qty": need, "spare": spare, "buy": need + spare, "shops": shops})
                    }
                    })
                    .collect();
                let mut answer = json!({"boards": boards.max(1), "spare_rule": "chip R/C/L/LED: 2 or a tenth of the total, whichever is more (spare=0 turns it off); other parts none", "lines": lines, "note": "qty = pieces the boards need, buy = qty + spare. number = the shop's own part number (checked against the shop before use); search = a link to the shop's search page, because the number is not known"});
                if let Some(f) = opt(&kv, "file") {
                    std::fs::write(f, pcb_core::shops::purchase_csv(board, boards, spare_on)).map_err(|e| format!("{f}: {e}"))?;
                    answer["file"] = json!(f);
                }
                Ok(obj(answer))
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
        let sch = &*self.sch.flat();
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

/// The shop part numbers of the schematic's library parts, by reference designator.
fn shops_of(sch: &sch_core::Schematic) -> Vec<(String, std::collections::BTreeMap<String, String>)> {
    sch.flat().components.iter().filter_map(|c| c.custom.as_ref().map(|s| (c.refdes.clone(), s.shops.clone()))).collect()
}
