//! `mount REF auto|direct|leaded|wired|lands`: how a part goes onto a board built by hand (`Component::mount`, L12), and
//! `proto perf|strip`: the schematic laid out on a perfboard or a stripboard as the app's prototype tab does (`sch_core::perfboard`,
//! `sch_core::stripboard`): each part's holes and way on, the wires on top, the bare wires or cuts, the supplies off the board, what to
//! get for the sockets and adapters, the check against the schematic and the requirements table's rules, and the drawing to print.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use sch_core::mount::MountChoice;
use sch_core::stripboard::{hole_name, strip_check, strip_layout_with, strip_rules, strip_unjoined, StripOptions};
use serde_json::{json, Value};

const MOUNT_USAGE: &str = "mount REF auto|direct|leaded|wired|lands   (how the part goes onto a stripboard, perfboard or breadboard: auto = from its footprint (a DIP chip in a socket, a SOP on an adapter, a chip resistor as the leaded part); direct = soldered straight in; leaded = the leaded part of the same value instead of the chip; wired = the chip with a lead soldered to each end, put through two holes (stays the chip on the parts list); lands = the chip on the copper side across two holes side by side)";

const PROTO_USAGE: &str = "proto perf|strip [cols=N] [rows=N] [holes=1] [back=1] [svg=FILE.svg] [face=parts|copper]   (the schematic laid out on a perfboard or a stripboard as the app's prototype tab does: cols / rows the board's size in holes (or the size saved by the app), holes=1 mounting holes in the corners kept free, back=1 the 0805 / 1206 chips on the copper side; svg= writes the drawing to print, from the parts' side or the copper side)";

impl Session {
    pub fn mount(&mut self, args: &[String]) -> Res {
        let [r, how] = args else { return Err(MOUNT_USAGE.into()) };
        let c = self.sch.components.iter_mut().find(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        let leads = c.pins().len();
        let fits: Vec<&str> = MountChoice::ALL.into_iter().filter(|m| m.fits(c, leads)).map(|m| m.key()).collect();
        if how.eq_ignore_ascii_case("auto") {
            c.mount = None;
        } else {
            let m = MountChoice::from_key(how).ok_or(MOUNT_USAGE)?;
            if !m.fits(c, leads) {
                return Err(format!("{} cannot go on as '{how}' (for this part: auto, {}; leaded, wired and lands are for a two-lead chip, lands for a chip resistor or capacitor)", c.refdes, fits.join(", ")));
            }
            c.mount = Some(m);
        }
        let c = &*c;
        let way = sch_core::mount::mount_for(c, leads);
        Ok(obj(json!({"ref": c.refdes, "mount": c.mount.map(|m| m.key()).unwrap_or("auto"), "how": way.describe(), "choices": fits})))
    }

    /// The board's size and mounting holes saved by the app with the schematic (`strip_size` mm, `strip_mount_holes`), as the defaults.
    fn saved_board(&self) -> (Option<(u16, u16)>, bool) {
        let holes = |v: f64| (v / 2.54).floor().max(1.0) as u16;
        let size = self.extras.get("strip_size").and_then(|v| Some((holes(v.get(0)?.as_f64()?), holes(v.get(1)?.as_f64()?))));
        (size, self.extras.get("strip_mount_holes").and_then(Value::as_bool).unwrap_or(false))
    }

    pub fn proto(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let kind = pos.first().map(|s| s.to_ascii_lowercase()).ok_or(PROTO_USAGE)?;
        let perf = match kind.as_str() {
            "perf" | "perfboard" => true,
            "strip" | "stripboard" => false,
            _ => return Err(PROTO_USAGE.into()),
        };
        let num = |k: &str| -> Result<Option<u16>, String> { opt(&kv, k).map(|v| v.parse::<u16>().ok().filter(|n| *n > 0).ok_or_else(|| format!("{k}={v}: a number of holes"))).transpose() };
        let (saved, saved_holes) = self.saved_board();
        let opt_ = StripOptions {
            cols: num("cols")?.or(saved.map(|s| s.0)),
            rows: num("rows")?.or(saved.map(|s| s.1)),
            chips_back: opt(&kv, "back") == Some("1"),
            mount_holes: opt(&kv, "holes").map_or(saved_holes, |v| v == "1"),
        };
        let l = if perf { sch_core::perfboard::perf_layout_with(&self.sch, opt_) } else { strip_layout_with(&self.sch, opt_) };
        let mounts: Vec<(String, sch_core::mount::Mount)> = l.parts.iter().map(|p| (p.refdes.clone(), p.mount.clone())).collect();
        let choice = |r: &str| self.sch.components.iter().find(|c| c.refdes == r).and_then(|c| c.mount).map(|m| m.key()).unwrap_or("auto");
        let parts: Vec<Value> = l
            .parts
            .iter()
            .map(|p| json!({"part": p.refdes, "mount": choice(&p.refdes), "how": p.mount.describe(), "leads": p.leads.iter().map(|(pin, a)| json!({"pin": pin, "hole": hole_name(*a)})).collect::<Vec<_>>()}))
            .collect();
        let links: Vec<Value> = l.links.iter().map(|k| json!({"from": hole_name(k.a), "to": hole_name(k.b), "net": k.net, "side": if k.back { "copper" } else { "parts" }, "tinned": k.tinned})).collect();
        let wires = |v: &[(sch_core::stripboard::At, sch_core::stripboard::At)]| -> Vec<Value> { v.iter().map(|(a, b)| json!({"from": hole_name(*a), "to": hole_name(*b)})).collect() };
        let bare = if perf { wires(&l.perf_spans()).into_iter().chain(wires(&l.bare)).collect::<Vec<_>>() } else { wires(&l.bare) };
        let off: Vec<Value> = l.off.iter().map(|(r, v)| json!({"part": r, "pins": v.iter().map(|(pin, net, a)| json!({"pin": pin, "net": net, "hole": hole_name(*a)})).collect::<Vec<_>>()})).collect();
        let get: Vec<Value> = sch_core::mount::shopping(&mounts).into_iter().map(|(item, n, ps)| json!({"item": item, "count": n, "parts": ps})).collect();
        let problems = strip_check(&self.sch, &l);
        let unjoined: Vec<String> = strip_unjoined(&self.sch, &l).into_iter().map(|(net, a, b)| format!("{net}: {} - {}", hole_name(a), hole_name(b))).collect();
        let rules: Vec<Value> = strip_rules(&self.sch, &l).into_iter().map(|r| json!({"ok": r.ok, "message": r.message})).collect();
        let mut answer = json!({
            "board": if perf { "perfboard" } else { "stripboard" }, "cols": l.cols, "rows": l.rows,
            "parts": parts, "links": links, "bare": bare, "off_board": off, "get": get,
            "problems": problems, "unjoined": unjoined, "rules": rules, "keepout_used": l.in_keepout().len(),
        });
        if !perf {
            answer["cuts"] = json!(l.cuts.iter().map(|a| hole_name(*a)).collect::<Vec<_>>());
        }
        if let Some(f) = opt(&kv, "svg") {
            let face = match opt(&kv, "face").unwrap_or("parts") {
                "parts" | "top" => sch_core::strip_print::Face::Parts,
                "copper" | "bottom" => sch_core::strip_print::Face::Copper,
                other => return Err(format!("face={other}: parts or copper")),
            };
            std::fs::write(f, sch_core::strip_print::strip_svg(&l, face, perf, "ccad")).map_err(|e| format!("{f}: {e}"))?;
            answer["svg"] = json!(f);
        }
        Ok(obj(answer))
    }
}
