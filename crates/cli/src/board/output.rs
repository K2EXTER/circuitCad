//! Board commands that save, open or write files: save, open, shopping, svg, assembly, case, dxf_import, dxf, fab.

use super::*;

impl Session {
    pub(super) fn board_save(&mut self, rest: &[String]) -> Res {
        let f = rest.first().ok_or("board save FILE.pcbjson")?;
        let board = self.board_mut()?;
        let text = serde_json::to_string_pretty(&*board).map_err(|e| e.to_string())?;
        std::fs::write(f, text).map_err(|e| format!("{f}: {e}"))?;
        Ok(obj(json!({"file": f})))
    }

    pub(super) fn board_open(&mut self, rest: &[String]) -> Res {
        let f = rest.first().ok_or("board open FILE.pcbjson")?;
        let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
        let mut board = serde_json::from_str::<Board>(&text).map_err(|e| format!("{f}: {e}"))?;
        board.migrate_finish_exclude();
        self.board = Some(board);
        Ok(obj(json!({"file": f})))
    }

    pub(super) fn board_shopping(&mut self, rest: &[String]) -> Res {
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

    pub(super) fn board_svg(&mut self, rest: &[String]) -> Res {
        let (pos, kv) = options(rest);
        let f = pos.first().ok_or("board svg FILE.svg [side=top|bottom|inner1...] [silk=0]   (a vector picture of one side, from the Gerber data; the bottom is seen from below)")?;
        let side = match opt(&kv, "side").unwrap_or("top").to_ascii_lowercase().as_str() {
            "top" => pcb_core::Side::Top,
            "bottom" => pcb_core::Side::Bottom,
            s => match s.strip_prefix("inner").and_then(|k| k.parse::<u8>().ok()) {
                Some(k) => pcb_core::Side::Inner(k),
                None => return Err(format!("side={s}: write top, bottom or inner1, inner2 ...")),
            },
        };
        let silk = opt(&kv, "silk") != Some("0");
        let board = self.board_mut()?;
        let svg = pcb_core::svg::board_svg(board, &pcb_core::svg::SvgOpts { side, silk })?;
        std::fs::write(f, &svg).map_err(|e| format!("{f}: {e}"))?;
        Ok(obj(json!({"file": f, "side": format!("{side:?}"), "bytes": svg.len()})))
    }

    /// The order of hand soldering (lowest first) with the orientation marks, and with a file the printable sheet (B3).
    pub(super) fn board_assembly(&mut self, rest: &[String]) -> Res {
        let (pos, _) = options(rest);
        let board = self.board_mut()?;
        let order = pcb_core::assembly_sheet::fitting_order(board);
        let parts: Vec<Value> = order
            .iter()
            .map(|f| json!({"refdes": f.refdes, "value": f.value, "kind": f.kind, "height_mm": (f.height * 10.0).round() / 10.0, "mark": f.mark.map(|(m, _)| m.label()), "bottom": f.bottom}))
            .collect();
        let mut answer = json!({"parts": parts, "note": "lowest body first (the board lies flat while each part is soldered); mark = which pin carries the orientation mark"});
        if let Some(f) = pos.first() {
            let svg = pcb_core::assembly_sheet::assembly_svg(board, &[])?;
            std::fs::write(f, &svg).map_err(|e| format!("{f}: {e}"))?;
            answer["file"] = json!(f);
        }
        Ok(obj(answer))
    }

    pub(super) fn board_case(&mut self, rest: &[String]) -> Res {
        let (_, kv) = options(rest);
        let num = |k: &str| opt(&kv, k).map(|s| sim_core::parse_value(s).filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("{k}={s} is not a size in mm (zero or more)"))).transpose();
        let spec = pcb_core::case::CaseSpec { inner_w: num("w")?.unwrap_or(0.0), inner_d: num("d")?.unwrap_or(0.0), top: num("top")?.unwrap_or(0.0), bottom: num("bottom")?.unwrap_or(0.0), clearance: num("clear")?.unwrap_or(0.5) };
        if spec.inner_w == 0.0 && spec.inner_d == 0.0 && spec.top == 0.0 && spec.bottom == 0.0 {
            return Err("board case [w=MM d=MM] [top=MM] [bottom=MM] [clear=0.5] [dxf=FILE]   (the inside of the case: width and depth for the floor, the room above and below the board for the parts' heights. With w and d it also answers the screw posts under the mounting holes (bottom = their height, else 5 mm) and the openings in the walls for the connectors at the edge; dxf= writes the case's drawing: the floor with the posts and the walls folded out with the openings)".into());
        }
        let board = self.board_mut()?;
        let r = pcb_core::case::check(board, &spec);
        let tall: Vec<Value> = r.too_tall.iter().map(|(p, s, h, o)| json!({"part": p, "side": format!("{s:?}"), "height_mm": h, "over_mm": o})).collect();
        let mut out = json!({"ok": r.ok(), "board_mm": [r.board_w, r.board_d], "floor_checked": r.floor_checked, "fits": r.fits, "turned_a_quarter": r.turned, "spare_mm": [r.spare_w, r.spare_d], "too_tall": tall,
            "note": "the heights of the parts are the 3D view's (a footprint's own height, else a guess by kind); the board's size is the box round its outline and parts"});
        if r.floor_checked {
            // inside the case: x along the width, y along the depth from the inner left front corner, z up from the floor
            let ins = pcb_core::case::inside(board, &spec);
            let r1 = |v: f64| (v * 100.0).round() / 100.0;
            out["posts"] = json!(ins.posts.iter().map(|p| json!({"hole": [r1(p.hole.x), r1(p.hole.y)], "at": [r1(p.at.x), r1(p.at.y)], "height_mm": p.height, "screw": p.screw, "outer_d": p.outer_d, "pilot_d": p.pilot_d, "problem": p.problem})).collect::<Vec<_>>());
            out["openings"] = json!(ins.openings.iter().map(|o| json!({"part": o.refdes, "wall": o.wall.label(), "along_mm": [r1(o.along.0), r1(o.along.1)], "along_from": o.wall.along_from(), "up_mm": [r1(o.up.0), r1(o.up.1)], "to_wall_mm": r1(o.to_wall)})).collect::<Vec<_>>());
            out["case_notes"] = json!(ins.notes);
            if let Some(f) = opt(&kv, "dxf") {
                std::fs::write(f, pcb_core::case::drawing(board, &spec)?).map_err(|e| format!("{f}: {e}"))?;
                out["dxf"] = json!(f);
            }
        }
        Ok(obj(out))
    }

    /// `board case_step [FILE] [dx= dy= dz=] [rot=0..3] | off`: the case read from its STEP, placed round the board (moved by dx/dy/dz and
    /// turned by rot quarter turns from where it first stood), and what it runs into.
    pub(super) fn board_case_step(&mut self, rest: &[String]) -> Res {
        let (pos, kv) = options(rest);
        let board = self.board_mut()?;
        if pos.first().is_some_and(|p| p == "off") {
            board.case3d = None;
            return Ok(obj(json!({"case": null})));
        }
        if let Some(f) = pos.first() {
            let bytes = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
            let name = std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            board.case3d = Some(pcb_core::case_model::CaseModel::from_step(&bytes, &name, board)?);
        }
        let Some(case) = board.case3d.as_mut() else {
            return Err("board case_step FILE.step [dx=MM dy=MM dz=MM] [rot=0..3]   (the case read from its STEP, set round the board: the middle of its box over the board's, its bottom 5 mm below the board; dx/dy/dz move it, rot turns it by quarter turns. 'board case_step' alone checks it again, 'board case_step off' takes it away)".into());
        };
        for (k, axis) in [("dx", 0), ("dy", 1), ("dz", 2)] {
            if let Some(v) = opt(&kv, k) {
                case.offset[axis] += sim_core::parse_value(v).ok_or_else(|| format!("{k}={v}: a length in mm"))?;
            }
        }
        if let Some(r) = opt(&kv, "rot") {
            case.rot = r.parse::<u8>().map_err(|_| format!("rot={r}: 0, 1, 2 or 3 quarter turns"))? % 4;
        }
        let case = case.clone();
        let board = self.board_mut()?;
        let hits = pcb_core::case_model::collisions(board, &case);
        let names: Vec<&str> = hits.parts.iter().map(|i| board.parts[*i].refdes.as_str()).collect();
        let r1 = |v: f64| (v * 100.0).round() / 100.0;
        Ok(obj(json!({"file": case.file, "triangles": case.mesh.triangles.len(), "size_mm": case.size().map(r1), "offset_mm": case.offset.map(r1), "rot": case.rot,
            "collides": names, "board_collides": hits.board, "ok": names.is_empty() && !hits.board,
            "note": "parts are checked by the boxes the 3D view draws, so a round part next to a wall can be reported though it just clears it"})))
    }

    pub(super) fn board_dxf_import(&mut self, rest: &[String]) -> Res {
        let (pos, kv) = options(rest);
        let f = pos.first().ok_or("board dxf_import FILE.dxf [layer=NAME] [origin=0]   (the outline = the biggest closed loop of lines, arcs and polylines; circles inside it become holes; origin=0 keeps the drawing's coordinates instead of moving the lower left corner to 0,0)")?;
        let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
        let shape = pcb_core::dxf_in::read(&text, opt(&kv, "layer"))?;
        let board = self.board_mut()?;
        pcb_core::dxf_in::apply(board, &shape, opt(&kv, "origin") != Some("0"));
        let (lo, hi) = board.bounds().unwrap_or_default();
        Ok(obj(json!({"outline_points": board.outline.len(), "holes": shape.holes.len(), "width_mm": hi.x - lo.x, "height_mm": hi.y - lo.y, "notes": shape.notes})))
    }

    pub(super) fn board_dxf(&mut self, rest: &[String]) -> Res {
        let f = rest.first().ok_or("board dxf FILE.dxf   (the outline, holes, pad drills and part boxes in mm, seen from the top, for the case's CAD)")?;
        let board = self.board_mut()?;
        std::fs::write(f, pcb_core::dxf::mechanical(board)).map_err(|e| format!("{f}: {e}"))?;
        Ok(obj(json!({"file": f, "outline_points": board.outline.len(), "holes": board.holes.len(), "parts": board.parts.len()})))
    }

    pub(super) fn board_step(&mut self, rest: &[String]) -> Res {
        let f = rest.first().ok_or("board step FILE.step   (the board raised to 1.6 mm with its mounting and through-hole drills, and every part's solid as the 3D view draws it, named by designator and coloured, in mm, for the case's CAD; an assembly with the board and every part a product of its own; cylinders, round holes and slots written round, a maker's mesh as its triangles)")?;
        let board = self.board_mut()?;
        if board.outline.len() < 3 {
            return Err("the board has no outline yet (board new W H, or board dxf_import)".into());
        }
        let solids = pcb_core::step_out::solids(board);
        let names: Vec<&str> = solids.iter().map(|s| s.name.as_str()).collect();
        std::fs::write(f, pcb_core::step_out::write(board)).map_err(|e| format!("{f}: {e}"))?;
        Ok(obj(json!({"file": f, "solids": solids.len(), "faces": solids.iter().map(|s| s.faces.len()).sum::<usize>(), "names": names})))
    }

    pub(super) fn board_fab(&mut self, rest: &[String]) -> Res {
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
}
