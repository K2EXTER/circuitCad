//! Board commands that lay or follow copper paths: path/fork, track, via, net, guard, route, improve.

use super::*;

impl Session {
    pub(super) fn board_path_fork(&mut self, sub: &str, rest: &[String]) -> Res {
        let sub = sub.to_ascii_lowercase();
        // board path A.1 B.1: the way along the copper from pad A to pad B (tracks and vias only, a pour is not walked)
        // board fork FROM.1 A.1 B.1: how far the ways from one pad to two others run together before they part
        let (pos, _) = options(rest);
        let pad = |k: usize| -> Result<pcb_core::drc::PadRef, String> {
            pos.get(k).and_then(|s| pcb_core::drc::PadRef::parse(s)).ok_or_else(|| "pads are written PART.PIN, e.g. RS1.1".to_string())
        };
        let board = self.board_mut()?;
        let g = pcb_core::drc::CopperGraph::build(board);
        let r1 = |x: f64| (x * 1000.0).round() / 1000.0;
        if sub.as_str() == "path" {
            let (a, b) = (pad(0)?, pad(1)?);
            match g.path(&a, &b) {
                Some(p) => Ok(obj(json!({"from": a.to_string(), "to": b.to_string(), "length_mm": r1(p.length), "vias": p.vias, "layers": p.layers.iter().map(|s| s.label()).collect::<Vec<_>>(), "points": p.points.len()}))),
                None => Ok(obj(json!({"from": a.to_string(), "to": b.to_string(), "path": null, "note": "no tracks and vias join these pads (a pour is not walked, and a pad that is not on the board has no path)"}))),
            }
        } else {
            let (from, a, b) = (pad(0)?, pad(1)?, pad(2)?);
            match g.fork(&from, &a, &b) {
                Some(f) => Ok(obj(json!({"from": from.to_string(), "a": a.to_string(), "b": b.to_string(), "run_together_mm": r1(f.length), "at": [r1(f.at.x), r1(f.at.y)], "a_length_mm": r1(f.a_length), "b_length_mm": r1(f.b_length)}))),
                None => Ok(obj(json!({"from": from.to_string(), "fork": null, "note": "one of the pads cannot be reached along tracks and vias"}))),
            }
        }
    }

    pub(super) fn board_guard(&mut self, rest: &[String]) -> Res {
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

    pub(super) fn board_track(&mut self, rest: &[String]) -> Res {
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
                Ok(obj(json!({"removed": n, "vias_removed": vias_gone, "unrouted": pcb_core::ratsnest::open_connections(board).len()})))
            }
            _ => Err("board track add ... | board track remove ...".into()),
        }
    }

    pub(super) fn board_via(&mut self, rest: &[String]) -> Res {
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
                Ok(obj(json!({"removed": before - board.vias.len(), "unrouted": pcb_core::ratsnest::open_connections(board).len()})))
            }
            _ => Err("board via add x,y ... | board via remove x,y".into()),
        }
    }

    pub(super) fn board_net(&mut self, rest: &[String]) -> Res {
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
        let open: Vec<Value> = pcb_core::ratsnest::open_connections(board).into_iter().filter(|a| a.net == name).map(|a| json!({"from": [a.a.x, a.a.y], "to": [a.b.x, a.b.y], "length_mm": (a.a.dist(a.b) * 100.0).round() / 100.0})).collect();
        let total: f64 = per_layer.values().map(|v| v.1).sum();
        Ok(obj(json!({"net": name, "pads": pads, "tracks": tracks, "track_length_mm": (total * 100.0).round() / 100.0, "vias": vias, "pours": pours, "open": open, "routed": open.is_empty() && !pads.is_empty()})))
    }

    pub(super) fn board_improve(&mut self, rest: &[String]) -> Res {
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

    pub(super) fn board_route(&mut self, rest: &[String]) -> Res {
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
        // one net asked for (`net=`): the rest of the board stays as it is, so the top pour only comes with `top=1`
        let top_asked = if opt(&kv, "net").is_some() { opt(&kv, "top") == Some("1") } else { opt(&kv, "top") != Some("0") };
        let top_pour = top_asked && rep.routed > 0 && board.zones.iter().any(|z| z.net == "GND" && z.side == pcb_core::model::Side::Bottom) && pcb_core::groundpour::top_pour(board, None) == Ok(true);
        let failed: Vec<Value> = rep.failed.iter().map(|a| json!({"net": a.net})).collect();
        Ok(obj(json!({"routed": rep.routed, "failed": failed, "aligned_pads": aligned, "joined_tracks": joined, "slits_filled": slits_filled, "pads_bridged": bridged, "top_pour": top_pour, "currents_declared": auto_nets, "wide_nets": rep.wide.iter().map(|(n, w)| json!({"net": n, "width_mm": w})).collect::<Vec<_>>(), "rerouted_nets": improved.improved, "saved_mm": (improved.saved * 10.0).round() / 10.0, "vias_change": improved.vias, "unrouted_left": pcb_core::ratsnest::ratsnest(board).len()})))
    }
}
