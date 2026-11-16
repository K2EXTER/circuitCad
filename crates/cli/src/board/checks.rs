//! Board commands that check or measure: impedance, class, drc, quality, status, currents, postlayout, check.

use super::*;

impl Session {
    pub(super) fn board_nets(&mut self, rest: &[String]) -> Res {
        // board nets [sort=name|class|pads|length|detour|vias|width|current|open] [asc=1|desc=1] [csv=file]: one row per net (the editor's
        // "ネットの一覧"): class, pads, track length in all and per layer, detour, vias, thinnest / widest track, current and the width it needs, pours, open connections
        let (_, kv) = options(rest);
        let board = self.board_mut()?;
        let mut rows = pcb_core::nettable::net_table(board, None);
        let by = match opt(&kv, "sort") {
            None => pcb_core::nettable::SortBy::Name,
            Some(s) => pcb_core::nettable::SortBy::parse(s).ok_or_else(|| format!("sort={s}: use name, class, pads, length, detour, vias, width, current or open"))?,
        };
        let numeric = !matches!(by, pcb_core::nettable::SortBy::Name | pcb_core::nettable::SortBy::Class);
        let desc = if numeric { opt(&kv, "asc") != Some("1") } else { opt(&kv, "desc") == Some("1") };
        pcb_core::nettable::sort_rows(&mut rows, by, desc);
        if let Some(file) = opt(&kv, "csv") {
            std::fs::write(file, pcb_core::nettable::table_csv(&rows)).map_err(|e| format!("{file}: {e}"))?;
        }
        let r2 = |v: f64| (v * 100.0).round() / 100.0;
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                json!({"net": r.net, "class": r.class, "pads": r.pads, "length_mm": r2(r.length),
                    "by_layer_mm": r.by_layer.iter().map(|(k, v)| (k.clone(), json!(r2(*v)))).collect::<serde_json::Map<String, Value>>(),
                    "detour": r.detour().map(r2), "vias": r.vias, "thinnest_mm": r2(r.thinnest), "widest_mm": r2(r.widest),
                    "current_a": (r.current > 0.0).then_some(r.current), "recommended_mm": (r.current > 0.0).then(|| r2(r.needed)), "pours": r.pours, "open": r.open})
            })
            .collect();
        Ok(obj(json!({"nets": list, "count": rows.len(), "open": rows.iter().map(|r| r.open).sum::<usize>()})))
    }

    pub(super) fn board_impedance(&mut self, rest: &[String]) -> Res {
        // the impedance of a track and the width for a wanted one, from the field solver (the finished copper, the mask):
        //   board impedance [er=4.5] [height=1.53] [gap=0.36] [copper=35] [etch=25] [mask=20] [mask_top=10] [mask_er=3.8]
        //                   [layer=outer|inner] [ground_gap=G] [method=field|formula]
        //                   width=W [space=S]        -> the impedance of a track of width W (a pair S apart: also differential)
        //                   target=Z                 -> the width for Z ohm
        //                   diff=Z space=S          -> the width of each track of a pair S apart for Z ohm differential
        //                   diff=Z width=W          -> the space of a pair of tracks of width W for Z ohm differential
        // `er` ... `mask_er` set the layer stack of the board first (and keep it). `ground_gap` puts ground copper beside the track(s)
        // that far from their outer edge (a grounded coplanar line). `method=formula` gives the textbook closed forms (+-10 %) instead.
        use pcb_core::fieldsolve as fs;
        let (_, kv) = options(rest);
        let board = self.board_mut()?;
        let num = |k: &str, zero_ok: bool| -> Result<Option<f64>, String> {
            opt(&kv, k)
                .map(|v| v.parse::<f64>().ok().filter(|x| (*x > 0.0 || (zero_ok && *x == 0.0)) && x.is_finite()).ok_or_else(|| format!("{k}={v}: a {} number is expected", if zero_ok { "non-negative" } else { "positive" })))
                .transpose()
        };
        let get = |k: &str| num(k, false);
        for (k, zero_ok, field) in [
            ("er", false, &mut board.stackup.er as &mut f64),
            ("height", false, &mut board.stackup.height),
            ("gap", false, &mut board.stackup.plane_gap),
            ("copper", false, &mut board.stackup.copper_um),
            ("etch", true, &mut board.stackup.etch_um),
            ("mask", true, &mut board.stackup.mask_um),
            ("mask_top", true, &mut board.stackup.mask_top_um),
            ("mask_er", false, &mut board.stackup.mask_er),
        ] {
            if let Some(v) = num(k, zero_ok)? {
                *field = v;
            }
        }
        let kind = match opt(&kv, "layer") {
            Some("inner") => pcb_core::impedance::Kind::Stripline,
            Some("outer") | None => pcb_core::impedance::Kind::Microstrip,
            Some(o) => return Err(format!("layer={o}: use outer or inner")),
        };
        let field = match opt(&kv, "method") {
            Some("field") | None => true,
            Some("formula") => false,
            Some(o) => return Err(format!("method={o}: use field (the field solver) or formula (the textbook closed forms, +-10 %)")),
        };
        let ground_gap = get("ground_gap")?;
        if ground_gap.is_some() && !field {
            return Err("ground_gap= needs method=field: the closed forms have no ground beside the track".into());
        }
        let s = board.stackup;
        let sec = fs::Section { kind, width: 0.0, space: None, ground_gap };
        let round = |x: f64| (x * 1000.0).round() / 1000.0;
        let (width, space, target, diff) = (get("width")?, get("space")?, get("target")?, get("diff")?);
        let mut out = json!({"layer": kind.label(), "method": if field { "field" } else { "formula" },
            "stackup": {"er": s.er, "height": s.height, "gap": s.plane_gap, "copper_um": s.copper_um, "etch_um": s.etch_um, "mask_um": s.mask_um, "mask_top_um": s.mask_top_um, "mask_er": s.mask_er}});
        let m = out.as_object_mut().unwrap();
        if let Some(g) = ground_gap {
            m.insert("ground_gap".into(), json!(g));
        }
        // the answer for the width (and space) found: the impedance and, from the solver, εeff and the delay
        let (w, sp) = match (target, diff) {
            (Some(z), None) => {
                let w = if field { fs::width_for(&s, &sec, z) } else { pcb_core::impedance::width_for(&s, kind, z) };
                let w = w.ok_or_else(|| format!("target={z}: no track width gives that impedance on this layer stack"))?;
                m.insert("target".into(), json!(z));
                (Some(round(w)), None)
            }
            (None, Some(z)) => match (width, space) {
                (None, Some(sp)) => {
                    let w = if field { fs::width_for_differential(&s, &sec, sp, z) } else { pcb_core::impedance::width_for_differential(&s, kind, sp, z) };
                    let w = w.ok_or_else(|| format!("diff={z} space={sp}: no track width gives that differential impedance"))?;
                    m.insert("diff".into(), json!(z));
                    (Some(round(w)), Some(sp))
                }
                (Some(w), None) => {
                    let sp = if field { fs::space_for_differential(&s, &sec, w, z) } else { pcb_core::impedance::space_for_differential(&s, kind, w, z) };
                    let sp = sp.ok_or_else(|| format!("diff={z} width={w}: no space gives that differential impedance"))?;
                    m.insert("diff".into(), json!(z));
                    (Some(w), Some(round(sp)))
                }
                _ => return Err("diff=Z needs width=W (to find the space) or space=S (to find the width), one of them".into()),
            },
            (Some(_), Some(_)) => return Err("target=Z and diff=Z are two questions: ask one at a time".into()),
            (None, None) => (width, space),
        };
        if let Some(w) = w {
            m.insert("width".into(), json!(w));
            if let Some(sp) = sp {
                m.insert("space".into(), json!(sp));
            }
            if field {
                let mode = |md: &fs::Mode| json!({"z": round(md.z), "er_eff": round(md.er_eff), "delay_ps_mm": round(md.delay_ps_mm), "c_pf_m": round(md.c_pf_m), "l_nh_m": round(md.l_nh_m)});
                let one = fs::solve(&s, &fs::Section { width: w, space: None, ..sec });
                m.insert("z0".into(), json!(round(one.main_z())));
                if let fs::Solved::Single(md) = one {
                    m.insert("er_eff".into(), json!(round(md.er_eff)));
                    m.insert("delay_ps_mm".into(), json!(round(md.delay_ps_mm)));
                    m.insert("single".into(), mode(&md));
                }
                if let Some(sp) = sp {
                    let pair = fs::solve(&s, &fs::Section { width: w, space: Some(sp), ..sec });
                    m.insert("zdiff".into(), json!(round(pair.main_z())));
                    m.insert("zcommon".into(), json!(pair.common_z().map(round)));
                    if let fs::Solved::Pair { odd, even } = pair {
                        m.insert("odd".into(), mode(&odd));
                        m.insert("even".into(), mode(&even));
                    }
                }
                // the closed forms next to it, to see how far off a rule of thumb is
                let mut f = json!({"z0": round(pcb_core::impedance::single_ended(&s, kind, w))});
                if let Some(sp) = sp {
                    f["zdiff"] = json!(round(pcb_core::impedance::differential(&s, kind, w, sp)));
                }
                if ground_gap.is_none() {
                    m.insert("formula".into(), f);
                }
            } else {
                m.insert("z0".into(), json!(round(pcb_core::impedance::single_ended(&s, kind, w))));
                if let Some(sp) = sp {
                    m.insert("zdiff".into(), json!(round(pcb_core::impedance::differential(&s, kind, w, sp))));
                }
            }
        }
        Ok(obj(out))
    }

    pub(super) fn board_class(&mut self, rest: &[String]) -> Res {
        // net classes: `board class` lists them; `board class NAME [width=mm] [clearance=mm] [via=dia/drill] [nets=A,B] [role=power|switching|
        // sense|analog|logic|none]` makes or changes one (a net listed in it leaves its other class); `board class NAME remove=1` deletes it.
        // The role is the order the router takes the nets in (a big current first, then switching, sensing, analog, logic), and a sensing
        // line gets no vias.
        let (pos, kv) = options(rest);
        let board = self.board_mut()?;
        let Some(name) = pos.first() else {
            let list: Vec<Value> = board.net_classes.iter().map(|c| class_json(c)).collect();
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
        if let Some(v) = opt(&kv, "order") {
            board.net_classes[k].order = match v {
                "role" | "auto" => None,
                _ => Some(v.parse::<u8>().ok().filter(|n| *n <= 9).ok_or_else(|| format!("order={v}: a number from 0 to 9, or `role`"))?),
            };
        }
        if let Some(v) = opt(&kv, "layers") {
            let mut list = vec![];
            if v != "any" && !v.is_empty() {
                for key in v.split(',') {
                    let s = pcb_core::model::Side::from_key(key).filter(|s| board.copper_layers().contains(s)).ok_or_else(|| format!("layers={key}: a copper layer of this board (top, bottom, inner1 ...), or `any`"))?;
                    list.push(s);
                }
            }
            board.net_classes[k].layers = list;
        }
        if let Some(v) = opt(&kv, "max_length") {
            board.net_classes[k].max_length = v.parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("max_length={v}: a number of 0 or more (mm)"))?;
        }
        if let Some(v) = opt(&kv, "max_vias") {
            board.net_classes[k].max_vias = v.parse::<u32>().map_err(|_| format!("max_vias={v}: a whole number of 0 or more (0 = no limit)"))?;
        }
        if let Some(v) = opt(&kv, "max_neck") {
            board.net_classes[k].max_neck = v.parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("max_neck={v}: a number of 0 or more (mm)"))?;
        }
        if let Some(v) = opt(&kv, "vias") {
            board.net_classes[k].no_vias = match v {
                "none" => Some(true),
                "allow" => Some(false),
                "role" => None,
                _ => return Err(format!("vias={v}: use none, allow or role")),
            };
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
        // width exceptions: `except=NET from=X,Y to=X,Y [except_width=mm]` keeps the tracks along that stretch out of the width checks and the
        // slider (without except_width, with the width of a track that lies along it); `except_clear=NET` takes all of that net's away
        if let Some(net) = opt(&kv, "except_clear") {
            board.net_classes[k].width_exceptions.retain(|e| e.net != net);
        }
        if let Some(net) = opt(&kv, "except") {
            if !board.net_classes[k].nets.iter().any(|n| n == net) {
                return Err(format!("except={net}: the net is not in the class {name} (nets=...)"));
            }
            let point = |key: &str| -> Result<pcb_core::Point, String> {
                let v = opt(&kv, key).ok_or_else(|| format!("except={net} needs from=X,Y to=X,Y"))?;
                let (x, y) = v.split_once(',').ok_or_else(|| format!("{key}={v}: use X,Y in mm"))?;
                Ok(pcb_core::Point::new(num(x)?, num(y)?))
            };
            let (a, b) = (point("from")?, point("to")?);
            let mut e = pcb_core::WidthException { net: net.to_string(), a, b, width: 0.0 };
            let along: Vec<usize> = (0..board.tracks.len()).filter(|&i| e.covers(&board.tracks[i])).collect();
            e.width = match opt(&kv, "except_width") {
                Some(w) => {
                    let w = num(w)?;
                    if w + 1e-9 < floor.min_track_width {
                        return Err(format!("except_width={w}: thinner than the board's minimum track width {} mm", floor.min_track_width));
                    }
                    for &i in &along {
                        board.tracks[i].width = w;
                    }
                    w
                }
                None => along.first().map(|&i| board.tracks[i].width).ok_or("no track lies along from..to: give except_width=mm")?,
            };
            board.net_classes[k].width_exceptions.push(e);
        }
        let c = &board.net_classes[k];
        Ok(obj(class_json(c)))
    }

    pub(super) fn board_drc(&mut self, _rest: &[String]) -> Res {
        let board = self.board_mut()?;
        let v = pcb_core::drc::check_final(board);
        let list: Vec<Value> = v.iter().take(60).map(|x| json!({"message": x.message, "at": [x.at.x, x.at.y]})).collect();
        Ok(obj(json!({"violations": v.len(), "shown": list.len(), "list": list})))
    }

    pub(super) fn board_quality(&mut self, rest: &[String]) -> Res {
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

    pub(super) fn board_status(&mut self, _rest: &[String]) -> Res {
        let board = self.board_mut()?;
        let wires = pcb_core::ratsnest::open_connections(board);
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

    pub(super) fn board_currents(&mut self, rest: &[String]) -> Res {
        let (words, kv) = options(rest);
        let widen = words.iter().any(|w| w.eq_ignore_ascii_case("widen"));
        let use_tran = words.iter().any(|w| w.eq_ignore_ascii_case("tran"));
        // `declared`: keep the currents the board already has (from the table of requirements or by hand) and only look at the widths,
        // so a circuit whose DC does not converge can still be widened
        let declared = words.iter().any(|w| w.eq_ignore_ascii_case("declared"));
        let c = if declared { None } else { Some(self.circuit()?) };
        let mut stats: Vec<Value> = vec![];
        let currents = if declared {
            vec![]
        } else if use_tran {
            let c = c.as_ref().unwrap();
            let tr = self.last_tran.as_ref().ok_or("board currents tran: run 'tran' first (the current is read from its result)")?;
            let from = opt(&kv, "from").map(|f| f.parse::<f64>().ok().filter(|v| (0.0..0.95).contains(v)).ok_or("from=F is the part of the run to skip, 0 to 0.9 (default 0.5: the second half, without the start-up)")).transpose()?.unwrap_or(0.5);
            let st = sim_core::net_currents_tran(c, tr, from);
            let r3 = |x: f64| (x * 1e3).round() / 1e3;
            stats = st
                .iter()
                .map(|s| json!({"net": s.net, "rms": r3(s.rms), "burst_rms_10ms": r3(s.burst_rms), "peak": r3(s.peak), "spiky": s.spiky()}))
                .collect();
            // the RMS of the settled part is what the copper heats with; the peak is only shown
            st.iter().map(|s| (s.net.clone(), s.rms)).collect::<Vec<_>>()
        } else {
            let c = c.as_ref().unwrap();
            let dc = sim_core::dc_op(c).map_err(|e| e.to_string())?;
            sim_core::net_currents(c, &dc)
        };
        let board = self.board_mut()?;
        let n = if declared { 0 } else { pcb_core::current::set_net_currents(board, &currents) };
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

    pub(super) fn board_postlayout(&mut self, rest: &[String]) -> Res {
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

    /// `board heatmap [temp=27] [standing] [alt=M] [cell=MM]`: the board's temperature map (H7) from the schematic's self-heating powers.
    pub(super) fn board_heatmap(&mut self, rest: &[String]) -> Res {
        let usage = "board heatmap [temp=27] [standing] [alt=M] [cell=MM] [tran=SECONDS]   (tran = also the board warming up from the moment the parts are switched on: its heat capacity, time constant and each part's temperature at some moments; the board's temperature on a grid from the parts' self-heating power: copper spreads the heat, both faces give it to the air by natural convection and radiation; standing = the board on an edge, alt = height above the sea in metres, cell = grid pitch in mm, 0.5 by default)";
        let (pos, kv) = options(rest);
        let num = |k: &str, d: f64| opt(&kv, k).map_or(Ok(d), |s| sim_core::parse_value(s).filter(|x| x.is_finite()).ok_or_else(|| format!("{k}={s} is not a number
{usage}")));
        let spec = pcb_core::thermal::ThermalSpec {
            ambient_c: num("temp", 27.0)?,
            orientation: if pos.iter().any(|w| w.eq_ignore_ascii_case("standing")) { pcb_core::thermal::Orientation::Standing } else { pcb_core::thermal::Orientation::Flat },
            altitude_m: num("alt", 0.0)?,
            cell_mm: num("cell", 0.0)?,
            ..Default::default()
        };
        let c = self.circuit()?;
        let heaters = sim_core::heaters_from_schematic(&self.sch, &c, &Default::default());
        let r = sim_core::self_heat(&c, &heaters, spec.ambient_c).map_err(|e| e.to_string())?;
        let sources: Vec<pcb_core::thermal::HeatSource> = r
            .parts
            .iter()
            .filter(|p| p.power > 1e-9)
            .filter_map(|p| {
                let comp = self.sch.components.iter().find(|x| x.refdes == p.name)?;
                let mut s = pcb_core::thermal::HeatSource::new(&p.name, p.power, sim_core::own_theta(comp), sim_core::theta_jc(comp).0, comp.heatsink);
                s.cth = comp.cth;
                Some(s)
            })
            .collect();
        if sources.is_empty() {
            return Err("no part gives off heat: run 'heat' to see the self-heating of the schematic".into());
        }
        let tran = opt(&kv, "tran").map(|s| sim_core::parse_value(s).filter(|x| *x > 0.0 && x.is_finite()).ok_or_else(|| format!("tran={s}: a time in seconds above zero
{usage}"))).transpose()?;
        let board = self.board_mut()?;
        let t0 = std::time::Instant::now();
        let m = pcb_core::thermal::thermal_map(board, &sources, board.stackup.copper_um, &spec)?;
        // the parts with real power that warm each other noticeably, with a distance to keep them apart
        let top = sources.iter().map(|s| s.power).fold(0.0, f64::max);
        let mut hot: Vec<pcb_core::thermal::HeatSource> = sources.iter().filter(|s| s.power >= 1e-3 && s.power >= 0.01 * top).cloned().collect();
        hot.sort_by(|a, b| b.power.total_cmp(&a.power));
        hot.truncate(sim_core::BOARD_HEATERS_MAX);
        let pairs: Vec<Value> = pcb_core::thermal::hot_pairs(board, &hot, board.stackup.copper_um, &spec)?
            .iter()
            .map(|p| json!({"a": p.a, "b": p.b, "dist_mm": p.dist_mm, "a_from_b_k": p.a_from_b, "b_from_a_k": p.b_from_a, "rise_a_k": p.rise_a, "rise_b_k": p.rise_b, "suggest_mm": p.suggest_mm}))
            .collect();
        let (t, at) = m.hottest();
        let parts: Vec<Value> = m
            .parts
            .iter()
            .map(|p| {
                let own = r.parts.iter().find(|x| x.name == p.refdes).map(|x| x.temp_c);
                json!({"part": p.refdes, "power_w": p.power, "junction_c": p.junction, "pads_c": p.pad_temp, "pads_max_c": p.pad_max, "self_heat_c": own})
            })
            .collect();
        let warm_up = match tran {
            Some(end) => {
                let tr = pcb_core::thermal::thermal_transient(board, &sources, board.stackup.copper_um, &spec, end, 8)?;
                let moments: Vec<Value> = tr
                    .frames
                    .iter()
                    .map(|(t, _)| {
                        let k = tr.times.iter().position(|x| (x - t).abs() < 1e-9 * t.max(1.0)).unwrap_or(tr.times.len() - 1);
                        json!({"t_s": t, "parts": tr.parts.iter().map(|(n, v)| json!({"part": n, "junction_c": v[k]})).collect::<Vec<Value>>()})
                    })
                    .collect();
                json!({"end_s": end, "board_heat_capacity_j_per_k": tr.board_cth, "board_time_constant_s": tr.board_tau, "moments": moments})
            }
            None => Value::Null,
        };
        Ok(obj(json!({"warm_up": warm_up, "hottest_c": t, "hottest_at": [at.x, at.y], "ambient_c": m.ambient_c, "h_top_w_m2k": m.h_top, "h_bottom_w_m2k": m.h_bottom, "cell_mm": m.grid.d, "cells": m.grid.w * m.grid.h, "layers": m.layers.len(), "rounds": m.rounds, "ms": t0.elapsed().as_secs_f64() * 1e3, "parts": parts, "hot_pairs": pairs,
            "note": "A guide (about ±20-30 %): natural convection and radiation from both faces, no forced air or neighbours' shading. junction_c is the part's inside, through θjc to the copper under its pads and through its body to the air; self_heat_c is the schematic's self-heating with θ alone, to compare. hot_pairs: parts that get at least 3 K and 15 % of their rise from each other through the board, with suggest_mm, a distance (pad centre to centre) where that falls to half (a guide from the board's heat spreading length); 'req apart NAME a=A b=B min=MM' keeps them apart as a requirement."})))
    }

    pub(super) fn board_check(&mut self, rest: &[String]) -> Res {
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
}
