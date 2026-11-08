//! Board commands that save, open or write files: save, open, shopping, svg, case, dxf_import, dxf, fab.

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

    pub(super) fn board_case(&mut self, rest: &[String]) -> Res {
        let (_, kv) = options(rest);
        let num = |k: &str| opt(&kv, k).map(|s| sim_core::parse_value(s).filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("{k}={s} is not a size in mm (zero or more)"))).transpose();
        let spec = pcb_core::case::CaseSpec { inner_w: num("w")?.unwrap_or(0.0), inner_d: num("d")?.unwrap_or(0.0), top: num("top")?.unwrap_or(0.0), bottom: num("bottom")?.unwrap_or(0.0), clearance: num("clear")?.unwrap_or(0.5) };
        if spec.inner_w == 0.0 && spec.inner_d == 0.0 && spec.top == 0.0 && spec.bottom == 0.0 {
            return Err("board case [w=MM d=MM] [top=MM] [bottom=MM] [clear=0.5]   (the inside of the case: width and depth for the floor, the room above and below the board for the parts' heights)".into());
        }
        let board = self.board_mut()?;
        let r = pcb_core::case::check(board, &spec);
        let tall: Vec<Value> = r.too_tall.iter().map(|(p, s, h, o)| json!({"part": p, "side": format!("{s:?}"), "height_mm": h, "over_mm": o})).collect();
        Ok(obj(json!({"ok": r.ok(), "board_mm": [r.board_w, r.board_d], "floor_checked": r.floor_checked, "fits": r.fits, "turned_a_quarter": r.turned, "spare_mm": [r.spare_w, r.spare_d], "too_tall": tall,
            "note": "the heights of the parts are the 3D view's (a footprint's own height, else a guess by kind); the board's size is the box round its outline and parts"})))
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
