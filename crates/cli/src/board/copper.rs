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

    pub(super) fn board_stitch(&mut self, rest: &[String]) -> Res {
        // board stitch [net=GND] [pitch=5] [mode=grid|fence] [via=0.6] [drill=0.3]: vias of the net where it is poured on two or
        // more layers (a grid over the overlap, or a fence along the board edge)
        let (_, kv) = options(rest);
        let mut o = pcb_core::stitch::StitchOptions { net: opt(&kv, "net").map(str::to_string), ..Default::default() };
        o.mode = match opt(&kv, "mode") {
            None | Some("grid") => pcb_core::stitch::StitchMode::Grid,
            Some("fence") => pcb_core::stitch::StitchMode::Fence,
            Some(m) => return Err(format!("mode={m}: use grid or fence")),
        };
        if let Some(v) = opt(&kv, "pitch") {
            o.pitch = num(v)?;
        }
        if let Some(v) = opt(&kv, "via") {
            o.via_diameter = num(v)?;
        }
        if let Some(v) = opt(&kv, "drill") {
            o.via_drill = num(v)?;
        }
        let board = self.board_mut()?;
        let r = pcb_core::stitch::stitch_vias(board, &o)?;
        let layers: Vec<String> = r.layers.iter().map(|s| layer_name(*s)).collect();
        Ok(obj(json!({"added": r.added, "dropped": r.dropped, "layers": layers})))
    }

    pub(super) fn board_fiducials(&mut self, rest: &[String]) -> Res {
        // board fiducials [d=1.0] [open=3.0] [layer=top|bottom] [remove=1]: three marks for the pick-and-place camera near three
        // corners (bare copper dot, a ring with no mask round it), or take them off
        let (_, kv) = options(rest);
        let mut o = pcb_core::fiducial::FiducialOptions::default();
        if let Some(l) = opt(&kv, "layer") {
            o.side = parse_layer(l)?;
        }
        if let Some(v) = opt(&kv, "d") {
            o.diameter = num(v)?;
        }
        if let Some(v) = opt(&kv, "open") {
            o.opening = num(v)?;
        }
        let board = self.board_mut()?;
        if opt(&kv, "remove") == Some("1") {
            return Ok(obj(json!({"removed": pcb_core::fiducial::remove_fiducials(board, o.side)})));
        }
        let r = pcb_core::fiducial::place_fiducials(board, &o)?;
        let at: Vec<Value> = board.pads.iter().filter(|p| pcb_core::fiducial::is_fiducial(p) && p.side == o.side).map(|p| json!([(p.pos.x * 100.0).round() / 100.0, (p.pos.y * 100.0).round() / 100.0])).collect();
        Ok(obj(json!({"placed": r.put, "missed_corners": r.missed, "at": at})))
    }

    pub(super) fn board_dim(&mut self, rest: &[String]) -> Res {
        // board dim | dim outline | dim add X1,Y1 X2,Y2 [dir=aligned|h|v] [offset=2] [height=1.2] | dim remove N | dim clear:
        // dimensions kept on the board (a drawing layer: DXF and SVG, never Gerber)
        use pcb_core::dimension::{DimDir, Dimension};
        let (pos, kv) = options(rest);
        let point = |s: &str| -> Result<pcb_core::model::Point, String> {
            let v: Vec<f64> = s.split(',').map(|x| x.trim().parse::<f64>().map_err(|_| format!("{s}: write a point as X,Y (mm)"))).collect::<Result<_, _>>()?;
            match v.as_slice() {
                [x, y] => Ok(pcb_core::model::Point::new(*x, *y)),
                _ => Err(format!("{s}: write a point as X,Y (mm)")),
            }
        };
        let board = self.board_mut()?;
        match pos.first().map(String::as_str) {
            None | Some("list") => {}
            Some("outline") => {
                pcb_core::dimension::add_outline_dimensions(board)?;
            }
            Some("add") => {
                let (Some(a), Some(b)) = (pos.get(1), pos.get(2)) else { return Err("board dim add X1,Y1 X2,Y2 [dir=aligned|h|v] [offset=2]".into()) };
                let dir = match opt(&kv, "dir") {
                    None | Some("aligned") => DimDir::Aligned,
                    Some("h") | Some("horizontal") => DimDir::Horizontal,
                    Some("v") | Some("vertical") => DimDir::Vertical,
                    Some(d) => return Err(format!("dir={d}: use aligned, h or v")),
                };
                let offset = match opt(&kv, "offset") {
                    Some(v) => v.parse::<f64>().ok().filter(|o| o.is_finite()).ok_or_else(|| format!("offset={v}: a number of mm (negative puts the line on the right)"))?,
                    None => 2.0,
                };
                let mut d = Dimension::new(point(a)?, point(b)?, offset, dir);
                if let Some(h) = opt(&kv, "height") {
                    d.text_height = num(h)?;
                }
                if d.length() < 0.01 {
                    return Err("the two points are the same along that direction: nothing to measure".into());
                }
                board.dimensions.push(d);
            }
            Some("remove") => {
                let i: usize = pos.get(1).and_then(|v| v.parse().ok()).ok_or("board dim remove N (the index in the list)")?;
                if i >= board.dimensions.len() {
                    return Err(format!("there is no dimension {i} (there are {})", board.dimensions.len()));
                }
                board.dimensions.remove(i);
            }
            Some("clear") => board.dimensions.clear(),
            Some(o) => return Err(format!("board dim {o}: use outline, add, remove, clear (or nothing for the list)")),
        }
        let list: Vec<Value> = board
            .dimensions
            .iter()
            .enumerate()
            .map(|(i, d)| json!({"index": i, "a": [d.a.x, d.a.y], "b": [d.b.x, d.b.y], "dir": format!("{:?}", d.dir).to_lowercase(), "length_mm": (d.length() * 100.0).round() / 100.0, "offset": d.offset}))
            .collect();
        Ok(obj(json!({"dimensions": list})))
    }

    pub(super) fn board_forbid(&mut self, rest: &[String]) -> Res {
        // board forbid | forbid rect=x0,y0,x1,y1 | poly=x,y;x,y;x,y [layers=top,bottom] [tracks=0] [vias=0] [pours=0] [parts=0] [name=]
        // | forbid remove N | forbid clear: keep-out areas (the DRC reports what is inside, the router goes round, pours leave them)
        let (pos, kv) = options(rest);
        let board = self.board_mut()?;
        match pos.first().map(String::as_str) {
            Some("remove") => {
                let i: usize = pos.get(1).and_then(|v| v.parse().ok()).ok_or("board forbid remove N (the index in the list)")?;
                if i >= board.keepouts.len() {
                    return Err(format!("there is no area {i} (there are {})", board.keepouts.len()));
                }
                board.keepouts.remove(i);
            }
            Some("clear") => board.keepouts.clear(),
            Some(o) => return Err(format!("board forbid {o}: use rect= / poly= to add, remove N, clear (or nothing for the list)")),
            None => {
                let nums = |v: &str| -> Result<Vec<f64>, String> { v.split([',', ';']).map(|x| x.trim().parse::<f64>().map_err(|_| format!("{x} is not a number"))).collect() };
                let polygon = if let Some(v) = opt(&kv, "rect") {
                    let r = nums(v)?;
                    let [x0, y0, x1, y1] = r.as_slice() else { return Err("rect=x0,y0,x1,y1 (four numbers, mm)".into()) };
                    let (x0, x1, y0, y1) = (x0.min(*x1), x0.max(*x1), y0.min(*y1), y0.max(*y1));
                    vec![pcb_core::model::Point::new(x0, y0), pcb_core::model::Point::new(x1, y0), pcb_core::model::Point::new(x1, y1), pcb_core::model::Point::new(x0, y1)]
                } else if let Some(v) = opt(&kv, "poly") {
                    let r = nums(v)?;
                    if r.len() < 6 || r.len() % 2 != 0 {
                        return Err("poly=x,y;x,y;x,y (three points or more, mm)".into());
                    }
                    r.chunks(2).map(|c| pcb_core::model::Point::new(c[0], c[1])).collect()
                } else {
                    vec![]
                };
                if !polygon.is_empty() {
                    let mut k = pcb_core::keepout::Keepout::new(polygon);
                    k.name = opt(&kv, "name").unwrap_or("").to_string();
                    if let Some(l) = opt(&kv, "layers") {
                        k.layers = l.split(',').map(parse_layer).collect::<Result<_, _>>()?;
                    }
                    let off = |n: &str| opt(&kv, n) == Some("0");
                    (k.tracks, k.vias, k.pours, k.parts) = (!off("tracks"), !off("vias"), !off("pours"), !off("parts"));
                    if !(k.tracks || k.vias || k.pours || k.parts) {
                        return Err("an area that forbids nothing: leave at least one of tracks, vias, pours, parts on".into());
                    }
                    board.keepouts.push(k);
                }
            }
        }
        let inside = pcb_core::keepout::keepout_checks(board);
        let list: Vec<Value> = board
            .keepouts
            .iter()
            .enumerate()
            .map(|(i, k)| json!({"index": i, "name": k.name, "points": k.polygon.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(), "layers": if k.layers.is_empty() { vec!["all".to_string()] } else { k.layers.iter().map(|s| layer_name(*s)).collect() }, "forbids": k.what()}))
            .collect();
        Ok(obj(json!({"areas": list, "inside": inside.iter().map(|v| json!({"at": [v.at.x, v.at.y], "message": v.message})).collect::<Vec<_>>()})))
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
