//! Board commands for pours, zones and drawn copper or silk: pour, zones, keepout, copper, slits, thermal, picture, pinlabels, labels.

use super::*;

impl Session {
    pub(super) fn board_pour(&mut self, rest: &[String]) -> Res {
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

    pub(super) fn board_pinlabels(&mut self, rest: &[String]) -> Res {
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

    pub(super) fn board_picture(&mut self, rest: &[String]) -> Res {
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

    pub(super) fn board_zones(&mut self, rest: &[String]) -> Res {
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

    pub(super) fn board_keepout(&mut self, rest: &[String]) -> Res {
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

    pub(super) fn board_copper(&mut self, rest: &[String]) -> Res {
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

    pub(super) fn board_slits(&mut self, _rest: &[String]) -> Res {
        // board slits: fill the gaps narrower than the clearance between copper of one net with a short piece of copper (the editor's 「つないで埋める」); the answer is `filled`
        let board = self.board_mut()?;
        let filled = pcb_core::drc::fill_slits(board);
        Ok(obj(json!({"filled": filled})))
    }

    pub(super) fn board_labels(&mut self, _rest: &[String]) -> Res {
        // the part designators off the copper (the app's 「文字を配線からよける」): the ones that touch a track, a pad, a via, a hole or
        // another text go to the clearest spot around their part; what was placed by hand stays
        let board = self.board_mut()?;
        let r = pcb_core::labels::avoid_overlaps(board);
        Ok(obj(json!({"moved": r.moved, "still_touching": r.still_touching})))
    }

    pub(super) fn board_thermal(&mut self, _rest: &[String]) -> Res {
        // the app's 「サーマルビアを置く」: a grid of 0.3 mm vias inside every heat pad (QFN / ESOP exposed pads) that has fewer than the
        // check asks for; nothing is added when that would make the DRC worse. Joining them to the back copper is up to the pour.
        let board = self.board_mut()?;
        let lacking = pcb_core::drc::thermal_via_checks(board).len();
        let added = pcb_core::drc::add_thermal_vias(board);
        let still = pcb_core::drc::thermal_via_checks(board).len();
        Ok(obj(json!({"pads_lacking": lacking, "vias_added": added, "pads_still_lacking": still, "drc_violations": pcb_core::drc::check_final(board).len()})))
    }
}
