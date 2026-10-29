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
                let mut swaps: Vec<(String, pcb_core::swap::SwapSpec)> = vec![];
                for c in self.sch.flat().components.iter().filter(|c| c.custom.is_some()) {
                    if let Some(spec) = c.custom.as_ref().and_then(|sym| part_lib::swap_for(&sym.name, &self.lib)) {
                        swaps.push((c.refdes.clone(), spec));
                    }
                    if let Some(def) = self.lib.iter().find(|d| d.footprint_name() == c.footprint)
                        && let Ok(fp) = def.footprint()
                    {
                        board.add_footprint(fp);
                        if let Some(m) = &def.model3d {
                            board.models3d.insert(def.footprint_name(), m.clone());
                        }
                        if def.land_unverified {
                            board.land_unverified.insert(def.footprint_name(), def.name.clone());
                        }
                    }
                }
                let rep = netlist::import(board, &parts, &nets);
                netlist::import_shops(board, &shops_of(&self.sch));
                netlist::import_swaps(board, &swaps);
                let sheets: Vec<(String, String)> = self.sch.flat().components.iter().filter(|c| !c.kind.is_power_symbol() && !c.sim_only).map(|c| (c.refdes.clone(), self.sch.sheet_name(sch_core::sheet_of(c.pos).0))).collect();
                netlist::import_sheets(board, &sheets);
                netlist::import_hints(board, &netlist::NetHints { currents: self.sch.net_currents.clone().into_iter().collect(), pairs: self.sch.diff_pairs.clone(), requirements: self.sch.requirements.clone() });
                Ok(obj(json!({"circuit_ok": circuit_ok, "added": rep.added, "removed": rep.removed, "updated": rep.updated, "warnings": rep.warnings.iter().cloned().chain(gate).collect::<Vec<_>>()})))
            }
            "swap" => {
                // board swap [dry=1]: trade the nets of alike gates and alike inputs of logic ICs so that the airwires get shorter
                // and cross less (only gates with no copper on their pads). The trade is kept in the board, so a new import keeps it.
                let (_, kv) = options(rest);
                let dry = opt(&kv, "dry") == Some("1");
                let board = self.board_mut()?;
                if board.swap_specs.is_empty() {
                    return Err("no part with alike gates here: `board import` first (library parts such as 74HC32 carry the rule)".into());
                }
                let r = pcb_core::swap::run(board, dry);
                let plans: Vec<_> = r
                    .plans
                    .iter()
                    .map(|p| json!({"part": p.refdes, "moves": p.moves, "length_mm": [p.length.0, p.length.1], "crossings": [p.crossings.0, p.crossings.1]}))
                    .collect();
                Ok(obj(json!({"dry": dry, "swapped": plans, "locked": r.locked})))
            }
            "breakout" => {
                // board breakout [width=0.2]: before routing, a short wire out of every pin of a fine-pitch IC that needs one: the pins in the middle of a
                // side go straight out, the ones at the two ends turn outward (the nearer the end, the sooner), so they do not cross the way out of the
                // middle ones. A part whose stubs make the DRC find more than before gets none. The answer is `stubs`, `parts` and `dropped`.
                let (_, kv) = options(rest);
                let width = match opt(&kv, "width") {
                    Some(v) => num(v)?,
                    None => 0.2,
                };
                let board = self.board_mut()?;
                let r = pcb_core::breakout::breakout(board, width);
                Ok(obj(json!({"stubs": r.stubs, "parts": r.parts, "dropped": r.dropped})))
            }
            "orient" => {
                // board orient [dry=1] [parts=U1,U2] [minpads=3]: turn each part (0/90/180/270 degrees) to the way that makes its airwires shortest and crosses
                // the fewest. Parts with copper on a pad, locked parts, and turns that would land on something are left. A pin keeps its net.
                let (_, kv) = options(rest);
                let dry = opt(&kv, "dry") == Some("1");
                let only: Option<Vec<String>> = opt(&kv, "parts").map(|l| l.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
                let board = self.board_mut()?;
                let min_pads = opt(&kv, "minpads").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                let r = pcb_core::orient::run(board, only.as_deref(), min_pads, dry);
                let turned: Vec<_> = r
                    .turned
                    .iter()
                    .map(|t| json!({"part": t.refdes, "rot": [t.from, t.to], "length_mm": [t.length.0, t.length.1], "crossings": [t.crossings.0, t.crossings.1]}))
                    .collect();
                Ok(obj(json!({"dry": dry, "turned": turned, "tried": r.tried})))
            }
            "autoplace" => {
                // board autoplace [sheets=1] [arrays=0] [channels=0]: sheets=1 lays each schematic sheet out as a block (off by default), arrays=0 does not repeat the layout of copies (on by default)
                let (_, kv) = options(rest);
                let board = self.board_mut()?;
                let base = pcb_core::autoplace::Tuning::for_board(board);
                let flag = |name: &str, default: bool| match opt(&kv, name) {
                    Some("1") => true,
                    Some("0") => false,
                    _ => default,
                };
                let tuning = pcb_core::autoplace::Tuning { sheets: flag("sheets", base.sheets), arrays: flag("arrays", base.arrays), channels: flag("channels", base.channels), groups: flag("groups", base.groups), apart: flag("apart", base.apart), ..base };
                let r = pcb_core::autoplace::autoplace_with(board, &tuning)?;
                Ok(obj(json!({"placed": r.placed, "width": r.width, "height": r.height, "poured_nets_ignored": r.poured, "repoured_vias": r.repoured, "airwire_crossings": r.crossings})))
            }
            "pour" => {
                // the GND pour of the editor: the bottom copper as a pour, with a via for every surface pad of the net
                let (_, kv) = options(rest);
                let net = opt(&kv, "net").map(str::to_string);
                // layer=bottom (default) | inner1 | inner2 ...: the copper layer of the pour (an inner layer needs `board layers 4` first)
                let side = match opt(&kv, "layer") {
                    None | Some("bottom") => pcb_core::model::Side::Bottom,
                    Some(l) => match l.strip_prefix("inner").and_then(|n| n.parse::<u8>().ok()).filter(|n| *n >= 1) {
                        Some(n) => pcb_core::model::Side::Inner(n),
                        None => return Err(format!("layer={l}: use bottom, inner1, inner2 ...")),
                    },
                };
                let board = self.board_mut()?;
                let o = pcb_core::groundpour::PourOptions { net, top: opt(&kv, "top") != Some("0"), side, ..Default::default() };
                let r = pcb_core::groundpour::ground_pour(board, &o)?;
                Ok(obj(json!({"zone_added": r.zone_added, "top_added": r.top_added, "vias": r.vias, "already": r.already, "left": r.left})))
            }
            "pinlabels" => {
                // board pinlabels [height=1.0]: the names of the pins of the pin headers (`pinnames` in the circuit) on the silk beside the pads
                let (_, kv) = options(rest);
                let height = match opt(&kv, "height") {
                    Some(v) => num(v)?,
                    None => 1.0,
                };
                let names: Vec<(String, Vec<String>)> = self.sch.flat().components.iter().filter(|c| !c.pin_names.is_empty() && !c.sim_only).map(|c| (c.refdes.clone(), c.pin_names.clone())).collect();
                let board = self.board_mut()?;
                let n = pcb_core::pinlabels::write_pin_labels(board, &names, height);
                Ok(obj(json!({"parts": names.len(), "texts": n})))
            }
            "guard" => {
                // board guard net=A guard=GRDA [margin=1.0] [clearance=0.25]: a pour of the guard net (the follower's output) along every
                // track and pad of the sensitive net, so no voltage lies across the surface between them and the rails
                let (_, kv) = options(rest);
                let net = opt(&kv, "net").ok_or("board guard net=NET guard=GUARDNET [margin=1.0] [clearance=0.25]")?.to_string();
                let guard = opt(&kv, "guard").ok_or("board guard net=NET guard=GUARDNET [margin=1.0] [clearance=0.25]")?.to_string();
                let margin = match opt(&kv, "margin") {
                    Some(v) => num(v)?,
                    None => 1.0,
                };
                let clearance = match opt(&kv, "clearance") {
                    Some(v) => num(v)?,
                    None => 0.25,
                };
                let board = self.board_mut()?;
                // mode=tracks (default): a guard track on each side of every top track of the net, joined at the corners, one via of
                // the guard net on each run (route the guard net afterwards to join them); mode=pour: a pour round the net
                if opt(&kv, "mode") == Some("pour") {
                    let n = pcb_core::copper::add_guard(board, &net, &guard, margin, clearance)?;
                    return Ok(obj(json!({"net": net, "guard": guard, "pours": n})));
                }
                let gap = match opt(&kv, "gap") {
                    Some(v) => num(v)?,
                    None => 0.35,
                };
                let width = match opt(&kv, "width") {
                    Some(v) => num(v)?,
                    None => 0.3,
                };
                let (tracks, vias) = pcb_core::copper::add_guard_tracks(board, &net, &guard, gap, width)?;
                Ok(obj(json!({"net": net, "guard": guard, "tracks": tracks, "vias": vias})))
            }
            "picture" => {
                // board picture [layer=top|bottom|inner1 ...] [rect=x0,y0,x1,y1] [px=1600] [net=A,B] [marks=1] [others=1] [grid=mm] [out=file.png]:
                // a PNG of one copper layer (pours, tracks, pads, vias, holes, silk) with a millimetre grid and its numbers; `net=` draws those
                // nets in colours of their own, `marks=1` numbers the DRC findings and draws the open connections, `others=1` shows the other
                // copper layers faintly. The answer has the file and a legend (what the colours and numbers mean); look at the file.
                let (_, kv) = options(rest);
                let side = match opt(&kv, "layer") {
                    Some(l) => parse_layer(l)?,
                    None => pcb_core::model::Side::Top,
                };
                let area = match opt(&kv, "rect") {
                    Some(v) => {
                        let r: Vec<f64> = v.split(',').map(|x| x.trim().parse::<f64>().map_err(|_| format!("rect: {x} is not a number"))).collect::<Result<_, _>>()?;
                        let [x0, y0, x1, y1] = r.as_slice() else { return Err("rect=x0,y0,x1,y1 (four numbers, mm)".into()) };
                        Some((pcb_core::model::Point::new(x0.min(*x1), y0.min(*y1)), pcb_core::model::Point::new(x0.max(*x1), y0.max(*y1))))
                    }
                    None => None,
                };
                let max_side = match opt(&kv, "px") {
                    Some(v) => v.parse::<usize>().map_err(|_| format!("px={v}: a whole number is needed"))?,
                    None => 1600,
                };
                let flag = |name: &str| matches!(opt(&kv, name), Some("1") | Some("on") | Some("true"));
                let grid = match opt(&kv, "grid") {
                    Some(g) => num(g)?,
                    None => 0.0,
                };
                let highlight: Vec<String> = opt(&kv, "net").map(|v| v.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()).unwrap_or_default();
                // without out=, the picture goes to the temporary folder (the working folder of a server is not one to write into)
                let out = opt(&kv, "out").map(str::to_string).unwrap_or_else(|| std::env::temp_dir().join(format!("ccad-picture-{}.png", layer_name(side))).display().to_string().replace('\\', "/"));
                let parts: Vec<String> = opt(&kv, "part").map(|v| v.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()).unwrap_or_default();
                // ghost=SA,SB: the way the router would wire those nets, drawn as shadows (on a copy: the board is not changed); the
                // wiring they have now is left out of the copy first. `ghostvias=1` lets the shadow use vias (it does not by default)
                let ghost_nets: Vec<String> = opt(&kv, "ghost").map(|v| v.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()).unwrap_or_default();
                let ghost_vias = flag("ghostvias");
                // ghost_guard=A:GRDA,B:GRDB: the guard tracks (`board guard`) round those nets in the copy, drawn as shadows of the guard nets
                let ghost_guard: Vec<(String, String)> = opt(&kv, "ghostguard").map(|v| v.split(',').filter_map(|p| p.split_once(':')).map(|(n, g)| (n.trim().to_string(), g.trim().to_string())).collect()).unwrap_or_default();
                let guard_gap = match opt(&kv, "guardgap") {
                    Some(v) => num(v)?,
                    None => 0.25,
                };
                let guard_width = match opt(&kv, "guardwidth") {
                    Some(v) => num(v)?,
                    None => 0.3,
                };
                let board = self.board_mut()?;
                if let pcb_core::model::Side::Inner(n) = side
                    && n > board.inner_layers
                {
                    return Err(format!("layer={}: この基板の内層は {} 枚です (`board layers 4` で 4 層にできます)", layer_name(side), board.inner_layers));
                }
                let mut ghost = vec![];
                let mut failed: Vec<String> = vec![];
                let mut notes: Vec<String> = vec![];
                if !ghost_nets.is_empty() || !ghost_guard.is_empty() {
                    let mut g = board.clone();
                    for n in &ghost_nets {
                        pcb_core::shove::rip_up_net(&mut g, n);
                    }
                    let ro = RouteOptions { outer_only: true, no_vias: !ghost_vias, ..RouteOptions::default() };
                    for n in &ghost_nets {
                        let rep = pcb_core::autoroute::autoroute_sized(&mut g, &ro, Some(n));
                        if !rep.failed.is_empty() {
                            failed.push(n.clone());
                        }
                    }
                    let conn = pcb_core::drc::connectivity(&g);
                    let mut shadows: Vec<(String, Vec<pcb_core::model::Track>, Vec<pcb_core::model::Via>)> = vec![];
                    for n in &ghost_nets {
                        let tracks: Vec<pcb_core::model::Track> = g.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some(n.as_str())).map(|(_, t)| t.clone()).collect();
                        let vias: Vec<pcb_core::model::Via> = g.vias.iter().enumerate().filter(|(k, _)| conn.via_net(*k) == Some(n.as_str())).map(|(_, v)| v.clone()).collect();
                        shadows.push((n.clone(), tracks, vias));
                    }
                    // the guard tracks come after the routes: they follow the routed tracks of the nets
                    let mut guards: Vec<(String, Vec<pcb_core::model::Track>, Vec<pcb_core::model::Via>)> = vec![];
                    for (net, guard) in &ghost_guard {
                        let (t0, v0) = (g.tracks.len(), g.vias.len());
                        match pcb_core::copper::add_guard_tracks(&mut g, net, guard, guard_gap, guard_width) {
                            Ok(_) => {
                                let (t, v) = (g.tracks[t0..].to_vec(), g.vias[v0..].to_vec());
                                match guards.iter_mut().find(|(n, _, _)| n == guard) {
                                    Some((_, tt, vv)) => {
                                        tt.extend(t);
                                        vv.extend(v);
                                    }
                                    None => guards.push((guard.clone(), t, v)),
                                }
                            }
                            Err(e) => notes.push(format!("ガード {guard} ({net} の両側): {e}")),
                        }
                    }
                    if !guards.is_empty() {
                        notes.push("影のうち、ガードのネット (GRD…) は、`board guard` で検出線の両側に置くガード配線の案です (検出線の影のあとに置くので、検出線の影が前提)".into());
                    }
                    ghost.extend(shadows);
                    ghost.extend(guards);
                }
                let mut pic = pcb_core::picture::render(board, &pcb_core::picture::PictureOpts { side, area, max_side, highlight, marks: flag("marks"), others: flag("others"), grid, net_names: flag("names"), parts, ghost, finish: flag("finish") });
                for n in notes {
                    pic.legend.push(n);
                }
                for n in &failed {
                    pic.legend.push(format!("影 {n}: 経路が見つからない接続があります (ビアを許す `ghostvias=1`、または周りの配置を空けてみてください)"));
                }
                image::save_buffer(&out, &pic.rgba, pic.w as u32, pic.h as u32, image::ColorType::Rgba8).map_err(|e| format!("{out} に書けません: {e}"))?;
                Ok(obj(json!({"file": out, "width": pic.w, "height": pic.h, "legend": pic.legend})))
            }
            "track" => {
                // board track add x1,y1 x2,y2 [x3,y3 ...] [layer=top] [width=0.25] [net=NAME] [float=1]: lay a track (a line of pieces through the points).
                // With net=, the track must end up on that net (it touches a pad, track or via of it); else it is taken back and the answer says what it touched.
                // board track remove net=NAME [layer=top] [vias=1] | rect=x0,y0,x1,y1 [layer=top] | index=N: take tracks (and with vias=1 the vias of the net) away.
                let (pos, kv) = options(rest);
                let board = self.board_mut()?;
                match pos.first().map(|a| a.to_ascii_lowercase()).as_deref() {
                    Some("add") => {
                        let pts: Vec<pcb_core::model::Point> = pos[1..].iter().map(|s| {
                            let v: Vec<f64> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                            if v.len() == 2 { Ok(pcb_core::model::Point::new(v[0], v[1])) } else { Err(format!("{s}: a point is x,y (mm)")) }
                        }).collect::<Result<_, _>>()?;
                        if pts.len() < 2 {
                            return Err("board track add x1,y1 x2,y2 [x3,y3 ...] [layer=top] [width=0.25] [net=NAME]".into());
                        }
                        let side = match opt(&kv, "layer") {
                            Some(l) => parse_layer(l)?,
                            None => pcb_core::model::Side::Top,
                        };
                        let width = match opt(&kv, "width") {
                            Some(w) => num(w)?,
                            None => board.rules.min_track_width.max(0.25),
                        };
                        let before = board.tracks.clone();
                        let first = board.tracks.len();
                        for w in pts.windows(2) {
                            board.tracks.push(pcb_core::model::Track { side, a: w[0], b: w[1], width });
                        }
                        let conn = pcb_core::drc::connectivity(board);
                        let nets: std::collections::BTreeSet<String> = (first..board.tracks.len()).filter_map(|i| conn.track_net(i).map(str::to_string)).collect();
                        let length: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
                        if let Some(want) = opt(&kv, "net") {
                            if !nets.contains(want) || nets.len() > 1 {
                                board.tracks = before;
                                return Err(format!("track: the new track would be on {} (not {want}): it touches {}. Start and end it on pads or tracks of {want}", if nets.is_empty() { "no net (it touches nothing)".to_string() } else { nets.iter().cloned().collect::<Vec<_>>().join(" and ") }, if nets.len() > 1 { "more than one net (a short)" } else { "nothing of that net" }));
                            }
                        } else if nets.len() > 1 {
                            board.tracks = before;
                            return Err(format!("track: it would join the nets {} (a short): taken back", nets.iter().cloned().collect::<Vec<_>>().join(", ")));
                        } else if nets.is_empty() && opt(&kv, "float") != Some("1") {
                            board.tracks = before;
                            return Err("track: it touches no pad, track or via (it would float): taken back. Start it on a pad or a track; float=1 allows it".into());
                        }
                        let drc = pcb_core::drc::check_final(board).len();
                        Ok(obj(json!({"laid": pts.len() - 1, "layer": layer_name(side), "width": width, "length_mm": (length * 100.0).round() / 100.0, "net": nets.iter().next(), "drc_violations": drc})))
                    }
                    Some("remove") => {
                        let conn = pcb_core::drc::connectivity(board);
                        let side = opt(&kv, "layer").map(parse_layer).transpose()?;
                        let mut gone = vec![false; board.tracks.len()];
                        if let Some(net) = opt(&kv, "net") {
                            for (i, t) in board.tracks.iter().enumerate() {
                                gone[i] = conn.track_net(i) == Some(net) && side.is_none_or(|s| s == t.side);
                            }
                        } else if let Some(v) = opt(&kv, "rect") {
                            let r: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                            let [x0, y0, x1, y1] = r.as_slice() else { return Err("rect=x0,y0,x1,y1 (four numbers, mm)".into()) };
                            let (lx, hx, ly, hy) = (x0.min(*x1), x0.max(*x1), y0.min(*y1), y0.max(*y1));
                            let inside = |p: pcb_core::model::Point| p.x >= lx && p.x <= hx && p.y >= ly && p.y <= hy;
                            for (i, t) in board.tracks.iter().enumerate() {
                                gone[i] = inside(t.a) && inside(t.b) && side.is_none_or(|s| s == t.side);
                            }
                        } else if let Some(v) = opt(&kv, "index") {
                            let i: usize = v.parse().map_err(|_| format!("index={v}: a whole number is needed"))?;
                            if i >= gone.len() {
                                return Err(format!("index={i}: the board has {} tracks", gone.len()));
                            }
                            gone[i] = true;
                        } else {
                            return Err("board track remove net=NAME [layer=top] [vias=1] | rect=x0,y0,x1,y1 [layer=top] | index=N".into());
                        }
                        let mut vias_gone = 0;
                        if opt(&kv, "vias") == Some("1")
                            && let Some(net) = opt(&kv, "net") {
                                let keep: Vec<bool> = (0..board.vias.len()).map(|k| conn.via_net(k) != Some(net)).collect();
                                vias_gone = keep.iter().filter(|k| !**k).count();
                                let mut k = 0;
                                board.vias.retain(|_| {
                                    k += 1;
                                    keep[k - 1]
                                });
                            }
                        let n = gone.iter().filter(|g| **g).count();
                        let mut k = 0;
                        board.tracks.retain(|_| {
                            k += 1;
                            !gone[k - 1]
                        });
                        Ok(obj(json!({"removed": n, "vias_removed": vias_gone, "unrouted": pcb_core::ratsnest::ratsnest(board).len()})))
                    }
                    _ => Err("board track add ... | board track remove ...".into()),
                }
            }
            "via" => {
                // board via add x,y [net=NAME] [dia=0.6] [drill=0.3]: a via (the net is what it touches, or net=); board via remove x,y [r=0.3]: the vias within r mm of it.
                let (pos, kv) = options(rest);
                let board = self.board_mut()?;
                let at = |s: &str| -> Result<pcb_core::model::Point, String> {
                    let v: Vec<f64> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    if v.len() == 2 { Ok(pcb_core::model::Point::new(v[0], v[1])) } else { Err(format!("{s}: a point is x,y (mm)")) }
                };
                match pos.first().map(|a| a.to_ascii_lowercase()).as_deref() {
                    Some("add") => {
                        let p = at(pos.get(1).ok_or("board via add x,y [net=NAME] [dia=0.6] [drill=0.3]")?)?;
                        let dia = match opt(&kv, "dia") {
                            Some(v) => num(v)?,
                            None => (board.rules.min_drill + 2.0 * board.rules.min_annular_ring).max(0.6),
                        };
                        let drill = match opt(&kv, "drill") {
                            Some(v) => num(v)?,
                            None => board.rules.min_drill.max(0.3),
                        };
                        board.vias.push(pcb_core::model::Via { pos: p, diameter: dia, drill, net: opt(&kv, "net").unwrap_or("").to_string() });
                        let conn = pcb_core::drc::connectivity(board);
                        let net = conn.via_net(board.vias.len() - 1).map(str::to_string);
                        Ok(obj(json!({"at": [p.x, p.y], "diameter": dia, "drill": drill, "net": net, "drc_violations": pcb_core::drc::check_final(board).len()})))
                    }
                    Some("remove") => {
                        let p = at(pos.get(1).ok_or("board via remove x,y [r=0.3]")?)?;
                        let r = match opt(&kv, "r") {
                            Some(v) => num(v)?,
                            None => 0.3,
                        };
                        let before = board.vias.len();
                        board.vias.retain(|v| v.pos.dist(p) > r);
                        Ok(obj(json!({"removed": before - board.vias.len(), "unrouted": pcb_core::ratsnest::ratsnest(board).len()})))
                    }
                    _ => Err("board via add x,y ... | board via remove x,y".into()),
                }
            }
            "net" => {
                // board net NAME: what the board has of a net (its pads with their places, the tracks per layer with their lengths and widths, the vias,
                // the pours, the connections still open): to check a route against the picture without counting by eye.
                let name = rest.first().ok_or("board net NAME")?.to_string();
                let board = self.board_mut()?;
                let conn = pcb_core::drc::connectivity(board);
                let pads: Vec<Value> = board.pads.iter().enumerate().filter(|(j, _)| conn.pad_net(*j) == Some(name.as_str())).map(|(_, p)| json!({"pad": format!("{}.{}", p.refdes, p.pin), "at": [p.pos.x, p.pos.y]})).collect();
                let mut per_layer: std::collections::BTreeMap<String, (usize, f64, f64, f64)> = Default::default();
                for (i, t) in board.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some(name.as_str())) {
                    let _ = i;
                    let e = per_layer.entry(layer_name(t.side)).or_insert((0, 0.0, f64::MAX, 0.0));
                    e.0 += 1;
                    e.1 += t.a.dist(t.b);
                    e.2 = e.2.min(t.width);
                    e.3 = e.3.max(t.width);
                }
                let tracks: Vec<Value> = per_layer.iter().map(|(l, (n, len, wmin, wmax))| json!({"layer": l, "pieces": n, "length_mm": (len * 100.0).round() / 100.0, "width_min": wmin, "width_max": wmax})).collect();
                let vias = (0..board.vias.len()).filter(|k| conn.via_net(*k) == Some(name.as_str())).count();
                let pours: Vec<Value> = board.zones.iter().filter(|z| z.net == name).map(|z| json!({"layer": layer_name(z.side)})).collect();
                let open: Vec<Value> = pcb_core::ratsnest::ratsnest(board).into_iter().filter(|a| a.net == name).map(|a| json!({"from": [a.a.x, a.a.y], "to": [a.b.x, a.b.y], "length_mm": (a.a.dist(a.b) * 100.0).round() / 100.0})).collect();
                let total: f64 = per_layer.values().map(|v| v.1).sum();
                Ok(obj(json!({"net": name, "pads": pads, "tracks": tracks, "track_length_mm": (total * 100.0).round() / 100.0, "vias": vias, "pours": pours, "open": open, "routed": open.is_empty() && !pads.is_empty()})))
            }
            "zones" => {
                // board zones [merge | bake]: the pours (layer, net, its shapes, area). Each pour is made of shapes that add and shapes that take away
                // (a hole, a keep-out), with corners and arcs kept. `merge` joins the pours of one net and layer that overlap or touch into one and
                // bakes it; `bake` turns the shapes of every pour into the final corners (straight lines): the area stays, the arcs and the
                // separate shapes are gone. Both also tidy the corners (duplicates and straight-line points out).
                let board = self.board_mut()?;
                let word = rest.first().map(|a| a.to_ascii_lowercase());
                let merging = word.as_deref() == Some("merge");
                let baking = word.as_deref() == Some("bake");
                let merged = if merging { Some(pcb_core::zonefill::merge_all_zones(board)) } else { None };
                let baked = if baking { Some((0..board.zones.len()).filter(|&i| pcb_core::zonefill::bake_zone(board, i)).count()) } else { None };
                let tidied = if merging || baking { Some(pcb_core::zonefill::tidy_zones(board)) } else { None };
                let list: Vec<_> = board.zones.iter().enumerate().map(|(i, z)| {
                    let area: f64 = z.region().iter().map(|s| pcb_core::geom::signed_area(&s.outer).abs() - s.holes.iter().map(|h| pcb_core::geom::signed_area(h).abs()).sum::<f64>()).sum();
                    let shapes: Vec<_> = z.shapes.iter().map(|zs| json!({"op": if zs.op == pcb_core::model::ShapeOp::Add { "add" } else { "sub" }, "corners": zs.shape.verts.len(), "arcs": zs.shape.verts.iter().filter(|v| v.arc_mid.is_some()).count()})).collect();
                    json!({"index": i, "net": z.net, "layer": layer_name(z.side), "shapes": shapes, "area_mm2": (area * 10.0).round() / 10.0})
                }).collect();
                Ok(obj(json!({"zones": list, "merged": merged, "baked": baked, "points_removed": tidied.map(|t| t.0), "shapes_dropped": tidied.map(|t| t.1)})))
            }
            "keepout" => {
                // board keepout layer=inner1 [net=VNEG] rect=x0,y0,x1,y1 | circle=x,y,r | pad=J3E,J3F [margin=1.0]: no copper of the pours on
                // the layer (of the net, when given) inside the area. `pad=` is a circle round each pad (its radius + margin).
                // `board keepout clear layer=inner1 [net=VNEG]` takes the holes away again.
                let (pos, kv) = options(rest);
                let side = parse_layer(opt(&kv, "layer").ok_or("layer=top|bottom|inner1 ...")?)?;
                let net = opt(&kv, "net").map(str::to_string);
                let board = self.board_mut()?;
                if pos.first().is_some_and(|a| a.eq_ignore_ascii_case("clear")) {
                    let n = pcb_core::zonefill::clear_zone_holes(board, side, net.as_deref());
                    return Ok(obj(json!({"removed": n})));
                }
                let numbers = |v: &str, n: usize, what: &str| -> Result<Vec<f64>, String> {
                    let l: Vec<f64> = v.split(',').map(|x| x.trim().parse::<f64>().map_err(|_| format!("{what}: {x} is not a number"))).collect::<Result<_, _>>()?;
                    if l.len() == n { Ok(l) } else { Err(format!("{what}: {n} numbers are needed")) }
                };
                let mut shapes: Vec<pcb_core::shape::Shape> = vec![];
                if let Some(v) = opt(&kv, "rect") {
                    let r = numbers(v, 4, "rect=x0,y0,x1,y1")?;
                    shapes.push(pcb_core::shape::Shape::rect(r[0], r[1], r[2], r[3], 0.0));
                }
                if let Some(v) = opt(&kv, "circle") {
                    let c = numbers(v, 3, "circle=x,y,r")?;
                    shapes.push(pcb_core::shape::Shape::circle(pcb_core::model::Point::new(c[0], c[1]), c[2]));
                }
                if let Some(v) = opt(&kv, "pad") {
                    let margin = match opt(&kv, "margin") {
                        Some(m) => num(m)?,
                        None => 1.0,
                    };
                    for r in v.split(',') {
                        let r = r.trim();
                        let pads: Vec<_> = board.pads.iter().filter(|p| p.refdes == r || format!("{}.{}", p.refdes, p.pin) == r).collect();
                        if pads.is_empty() {
                            return Err(format!("pad={r}: no such part or pad"));
                        }
                        for p in pads {
                            let (w, h) = p.shape.size();
                            shapes.push(pcb_core::shape::Shape::circle(p.pos, w.max(h) / 2.0 + margin));
                        }
                    }
                }
                if shapes.is_empty() {
                    return Err("board keepout layer=inner1 [net=NET] rect=x0,y0,x1,y1 | circle=x,y,r | pad=REF[,REF] [margin=1.0]".into());
                }
                let holes: usize = shapes.iter().map(|s| pcb_core::zonefill::add_zone_cut(board, side, net.as_deref(), s)).sum();
                Ok(obj(json!({"areas": shapes.len(), "holes": holes})))
            }
            "copper" => {
                // board copper net=DRN rect=x0,y0,x1,y1 [layers=top,bottom] [vias=6]: a patch of copper of a net (the drain of a power transistor)
                // on the layers, and vias of the net that join them
                let (_, kv) = options(rest);
                let net = opt(&kv, "net").ok_or("board copper net=NET rect=x0,y0,x1,y1 [layers=top,bottom] [vias=6]")?.to_string();
                let r: Vec<f64> = opt(&kv, "rect").ok_or("rect=x0,y0,x1,y1 (mm)")?.split(',').map(|v| v.trim().parse::<f64>().map_err(|_| format!("rect: {v} is not a number"))).collect::<Result<_, _>>()?;
                let [x0, y0, x1, y1] = r.as_slice() else { return Err("rect=x0,y0,x1,y1 (four numbers, mm)".into()) };
                let sides: Vec<pcb_core::model::Side> = opt(&kv, "layers").unwrap_or("top,bottom").split(',').map(|l| match l.trim() {
                    "top" => Ok(pcb_core::model::Side::Top),
                    "bottom" => Ok(pcb_core::model::Side::Bottom),
                    o => o.strip_prefix("inner").and_then(|n| n.parse::<u8>().ok()).filter(|n| *n >= 1).map(pcb_core::model::Side::Inner).ok_or_else(|| format!("layers={o}: use top, bottom, inner1 ...")),
                }).collect::<Result<_, _>>()?;
                let vias = match opt(&kv, "vias") {
                    Some(v) => v.parse::<usize>().map_err(|_| format!("vias={v}: a whole number is needed"))?,
                    None => 6,
                };
                let board = self.board_mut()?;
                let (pours, put) = pcb_core::copper::add_copper(board, &net, (*x0, *y0, *x1, *y1), &sides, vias)?;
                Ok(obj(json!({"net": net, "pours": pours, "vias": put})))
            }
            "nudge" => {
                // board nudge [budget=SECONDS] [layers=outer] [vias=0]: for each connection still open after a full route, try small parts near
                // it 1-3 mm away (their wiring taken away and routed again); a move stays only when fewer connections are open and the DRC
                // finds nothing more. The answer is `moved` (part, dx, dy), `open_before`, `open_after`, `tried`.
                let (_, kv) = options(rest);
                let budget = match opt(&kv, "budget") {
                    Some(v) => num(v)?.max(1.0),
                    None => 300.0,
                };
                let grid = match opt(&kv, "grid") {
                    Some(g) => num(g)?,
                    None => RouteOptions::default().grid,
                };
                let ro = RouteOptions { grid, pad_lead: Some(0.5), outer_only: opt(&kv, "layers") == Some("outer"), no_vias: opt(&kv, "vias") == Some("0"), ..RouteOptions::default() };
                let board = self.board_mut()?;
                let r = pcb_core::nudge::route_with_nudging(board, &ro, std::time::Duration::from_secs_f64(budget));
                let moved: Vec<Value> = r.moved.iter().map(|(p, dx, dy)| json!({"part": p, "dx": dx, "dy": dy})).collect();
                Ok(obj(json!({"moved": moved, "open_before": r.open_before, "open_after": r.open_after, "tried": r.tried, "out_of_time": r.out_of_time})))
            }
            "req" => self.board_req(),
            "slits" => {
                // board slits: fill the gaps narrower than the clearance between copper of one net with a short piece of copper (the editor's 「つないで埋める」); the answer is `filled`
                let board = self.board_mut()?;
                let filled = pcb_core::drc::fill_slits(board);
                Ok(obj(json!({"filled": filled})))
            }
            "layers" => {
                // board layers N: the number of copper layers (2, 4, 6 ...), like the editor's 「銅箔の層数」. Refuses when an inner layer that would go has copper.
                let (words, _) = options(rest);
                let total = words.first().ok_or("board layers N  (2, 4, 6 ...)")?.parse::<usize>().map_err(|_| "board layers N  (2, 4, 6 ...)")?;
                if total < 2 || total % 2 != 0 || total > 8 {
                    return Err("board layers N: N is 2, 4, 6 or 8".into());
                }
                let inner = (total - 2) as u8;
                let board = self.board_mut()?;
                let lost = board.tracks.iter().filter(|t| matches!(t.side, pcb_core::model::Side::Inner(n) if n > inner)).count() + board.zones.iter().filter(|z| matches!(z.side, pcb_core::model::Side::Inner(n) if n > inner)).count();
                if lost > 0 {
                    return Err(format!("{total} layers would drop {lost} track(s)/zone(s) on the inner layers that go away: remove them first"));
                }
                board.inner_layers = inner;
                Ok(obj(json!({"layers": total})))
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
                // net classes: `board class` lists them; `board class NAME [width=mm] [clearance=mm] [via=dia/drill] [nets=A,B] [role=power|switching|
                // sense|analog|logic|none]` makes or changes one (a net listed in it leaves its other class); `board class NAME remove=1` deletes it.
                // The role is the order the router takes the nets in (a big current first, then switching, sensing, analog, logic), and a sensing
                // line gets no vias.
                let (pos, kv) = options(rest);
                let board = self.board_mut()?;
                let Some(name) = pos.first() else {
                    let list: Vec<Value> = board.net_classes.iter().map(|c| json!({"name": c.name, "nets": c.nets, "width": c.track_width, "clearance": c.clearance, "via": [c.via_diameter, c.via_drill], "role": c.role.name()})).collect();
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
                if let Some(r) = opt(&kv, "role") {
                    board.net_classes[k].role = pcb_core::model::Role::parse(r).ok_or_else(|| format!("role={r}: use power, switching, sense, analog, logic or none"))?;
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
                Ok(obj(json!({"name": c.name, "nets": c.nets, "width": c.track_width, "clearance": c.clearance, "via": [c.via_diameter, c.via_drill], "role": c.role.name()})))
            }
            "labels" => {
                // the part designators off the copper (the app's 「文字を配線からよける」): the ones that touch a track, a pad, a via, a hole or
                // another text go to the clearest spot around their part; what was placed by hand stays
                let board = self.board_mut()?;
                let r = pcb_core::labels::avoid_overlaps(board);
                Ok(obj(json!({"moved": r.moved, "still_touching": r.still_touching})))
            }
            "thermal" => {
                // the app's 「サーマルビアを置く」: a grid of 0.3 mm vias inside every heat pad (QFN / ESOP exposed pads) that has fewer than the
                // check asks for; nothing is added when that would make the DRC worse. Joining them to the back copper is up to the pour.
                let board = self.board_mut()?;
                let lacking = pcb_core::drc::thermal_via_checks(board).len();
                let added = pcb_core::drc::add_thermal_vias(board);
                let still = pcb_core::drc::thermal_via_checks(board).len();
                Ok(obj(json!({"pads_lacking": lacking, "vias_added": added, "pads_still_lacking": still, "drc_violations": pcb_core::drc::check_final(board).len()})))
            }
            "improve" => {
                // make the routes of the whole board shorter and its vias fewer: nets that detour (or carry a via) are ripped up
                // and routed again, alone or with a neighbour in the way; a change is kept only when it is better and clean
                let (_, kv) = options(rest);
                let only = opt(&kv, "net").map(str::to_string);
                let board = self.board_mut()?;
                let ro = RouteOptions { pad_lead: Some(0.5), outer_only: opt(&kv, "layers") == Some("outer"), ..RouteOptions::default() };
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
                let grid = match opt(&kv, "grid") {
                    Some(g) => num(g)?,
                    None => RouteOptions::default().grid,
                };
                let ro = RouteOptions { grid, pad_lead: (lead > 0.0).then_some(lead), outer_only: opt(&kv, "layers") == Some("outer"), no_vias: opt(&kv, "vias") == Some("0"), grid_ladder: opt(&kv, "ladder") != Some("0"), ..RouteOptions::default() };
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
                let v = pcb_core::drc::check_final(board);
                let list: Vec<Value> = v.iter().take(60).map(|x| json!({"message": x.message, "at": [x.at.x, x.at.y]})).collect();
                Ok(obj(json!({"violations": v.len(), "shown": list.len(), "list": list})))
            }
            "quality" => {
                // board quality [top=10] [sort=bends|vias|detour|length|segments] [net=A,B] [poured=1]: how the wiring looks, as numbers (see `status` for the
                // summary), and the nets that look worst: the bends and the vias, how much longer a net is than its shortest tree (`detour`),
                // and the layers it runs on. A person's route keeps these low; the router does not mind.
                let (_, kv) = options(rest);
                let top = opt(&kv, "top").and_then(|v| v.parse::<usize>().ok()).unwrap_or(10);
                let sort = opt(&kv, "sort").unwrap_or("bends").to_string();
                let only: Option<Vec<String>> = opt(&kv, "net").map(|l| l.split(',').map(|s| s.trim().to_string()).collect());
                let board = self.board_mut()?;
                let (q, mut nets) = pcb_core::quality::measure(board);
                nets.retain(|n| only.as_ref().is_none_or(|o| o.contains(&n.net)));
                // the nets a pour carries are wired with a via at each pad, not as one tree: their bends and vias say little, and the "detour" none
                let poured: Vec<String> = board.zones.iter().map(|z| z.net.clone()).collect();
                let hide_poured = only.is_none() && opt(&kv, "poured") != Some("1");
                if hide_poured {
                    nets.retain(|n| !poured.contains(&n.net));
                }
                let key = |n: &pcb_core::quality::NetQuality| -> f64 {
                    match sort.as_str() {
                        "vias" => n.vias as f64,
                        "detour" => n.detour().unwrap_or(0.0),
                        "length" => n.length,
                        "segments" => n.segments as f64,
                        _ => n.bends as f64,
                    }
                };
                nets.sort_by(|a, b| key(b).total_cmp(&key(a)).then(a.net.cmp(&b.net)));
                let rows: Vec<Value> = nets
                    .iter()
                    .take(top)
                    .map(|n| json!({"net": n.net, "pads": n.pads, "length_mm": r1(n.length), "detour": n.detour().map(|d| (d * 100.0).round() / 100.0), "vias": n.vias, "bends": n.bends, "segments": n.segments, "by_layer_mm": n.by_layer.iter().map(|(l, v)| (l.clone(), r1(*v))).collect::<std::collections::BTreeMap<_, _>>()}))
                    .collect();
                Ok(obj(json!({"quality": quality_json(&q), "worst": rows, "sort": sort})))
            }
            "status" => {
                let board = self.board_mut()?;
                let wires = pcb_core::ratsnest::ratsnest(board);
                let unrouted = wires.len();
                // the open connections per net, so that one sees which nets are left without a `board net` for each
                let mut open: std::collections::BTreeMap<&str, usize> = Default::default();
                for w in &wires {
                    *open.entry(w.net.as_str()).or_default() += 1;
                }
                let drc = pcb_core::drc::check_final(board).len();
                let (q, _) = pcb_core::quality::measure(board);
                Ok(obj(json!({"parts": board.parts.len(), "pads": board.pads.len(), "tracks": board.tracks.len(), "vias": board.vias.len(), "unrouted": unrouted, "open_nets": open, "drc_violations": drc, "quality": quality_json(&q)})))
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
                // pads=RS1,RNA.1,Q1: the resistance, inductance and capacitance of those pads (a part name takes all its pads)
                let wanted: Vec<String> = opt(&options(rest).1, "pads").map(|v| v.split(',').map(|x| x.trim().to_ascii_uppercase()).collect()).unwrap_or_default();
                let picked: Vec<Value> = w
                    .iter()
                    .filter(|x| wanted.iter().any(|q| *q == x.refdes.to_ascii_uppercase() || *q == format!("{}.{}", x.refdes, x.pin).to_ascii_uppercase()))
                    .map(|x| json!({"net": x.net, "pad": format!("{}.{}", x.refdes, x.pin), "milliohm": (x.ohm * 1e6).round() / 1e3, "nanohenry": (x.henry * 1e10).round() / 10.0}))
                    .collect();
                self.post_layout = on;
                self.last_dc = None;
                self.last_tran = None;
                self.last_ac = None;
                Ok(obj(json!({"postlayout": on, "pads_with_resistance": w.len(), "pads": picked, "largest": list, "couplings": couplings.len(), "strongest_couplings": strongest,
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

    pub(crate) fn board_mut(&mut self) -> Result<&mut Board, String> {
        self.board.as_mut().ok_or_else(|| "there is no board yet (use 'board import' or 'board new' first)".to_string())
    }

    /// The parts and nets of the schematic as the board import sees them (same rules as the app).
    fn netlist_of(&self) -> (Vec<NetlistPart>, Vec<NetlistNet>) {
        let sch = &*self.sch.flat();
        let parts = sch
            .components
            .iter()
            .filter(|c| !c.kind.is_power_symbol() && !c.sim_only)
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

/// "top", "bottom", "inner1" ... as a copper layer.
fn parse_layer(l: &str) -> Result<pcb_core::model::Side, String> {
    match l.trim() {
        "top" => Ok(pcb_core::model::Side::Top),
        "bottom" => Ok(pcb_core::model::Side::Bottom),
        o => o.strip_prefix("inner").and_then(|n| n.parse::<u8>().ok()).filter(|n| *n >= 1).map(pcb_core::model::Side::Inner).ok_or_else(|| format!("layer={o}: use top, bottom, inner1 ...")),
    }
}

fn layer_name(s: pcb_core::model::Side) -> String {
    match s {
        pcb_core::model::Side::Top => "top".into(),
        pcb_core::model::Side::Bottom => "bottom".into(),
        pcb_core::model::Side::Inner(n) => format!("inner{n}"),
    }
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// The numbers of `pcb_core::quality::Quality` for an answer.
fn quality_json(q: &pcb_core::quality::Quality) -> Value {
    json!({
        "length_mm": r1(q.length),
        "by_layer_mm": q.by_layer.iter().map(|(l, v)| (l.clone(), r1(*v))).collect::<std::collections::BTreeMap<_, _>>(),
        "vias": q.vias,
        "segments": q.segments,
        "bends": q.bends,
        "short_tracks": q.short,
        "diagonal_mm": r1(q.diagonal),
        "straight_mm": r1(q.straight),
        "other_angle_mm": r1(q.other_angle),
        "side_by_side_mm": r1(q.side_by_side),
    })
}
