//! Board commands. `board_cmd` picks the command; each one lives in the file of its topic (sync, place, route, copper, checks, practice, output).

use crate::session::{obj, opt, options, Session};
use crate::Res;
use pcb_core::autoroute::RouteOptions;
use pcb_core::model::Board;
use pcb_core::netlist::{self, NetlistNet, NetlistPart};
use serde_json::{json, Value};

mod sync;
mod place;
mod route;
mod copper;
mod checks;
mod practice;
mod output;
mod silk;

const DEFAULT_SIZE: (f64, f64) = (60.0, 40.0);

impl Session {
    pub fn board_cmd(&mut self, args: &[String]) -> Res {
        let Some(sub) = args.first() else { return Err("board new|import|pour|autoplace|move|route|improve|labels|class|break|health|reveal|impedance|drc|status|postlayout|currents|save|open|fab".into()) };
        let rest = &args[1..];
        match sub.to_ascii_lowercase().as_str() {
            "new" => self.board_new(rest),
            "import" => self.board_import(rest),
            "swap" => self.board_swap(rest),
            "path" | "fork" => self.board_path_fork(sub, rest),
            "backannotate" => self.board_backannotate(rest),
            "renumber" => self.board_renumber(rest),
            "breakout" => self.board_breakout(rest),
            "orient" => self.board_orient(rest),
            "matchpairs" => self.board_matchpairs(rest),
            "autoplace" => self.board_autoplace(rest),
            "lock" => self.board_lock(rest, true),
            "unlock" => self.board_lock(rest, false),
            "pour" => self.board_pour(rest),
            "stitch" => self.board_stitch(rest),
            "fiducials" => self.board_fiducials(rest),
            "dim" => self.board_dim(rest),
            "forbid" => self.board_forbid(rest),
            "pinlabels" => self.board_pinlabels(rest),
            "guard" => self.board_guard(rest),
            "picture" => self.board_picture(rest),
            "track" => self.board_track(rest),
            "via" => self.board_via(rest),
            "net" => self.board_net(rest),
            "nets" => self.board_nets(rest),
            "zones" => self.board_zones(rest),
            "keepout" => self.board_keepout(rest),
            "copper" => self.board_copper(rest),
            "nudge" => self.board_nudge(rest),
            "req" => self.board_req(),
            "slits" => self.board_slits(rest),
            "layers" => self.board_layers(rest),
            "move" => self.board_move(rest),
            "impedance" => self.board_impedance(rest),
            "class" => self.board_class(rest),
            "break" => self.board_break(rest),
            "health" => self.board_health(rest),
            "reveal" => self.board_reveal(rest),
            "labels" => self.board_labels(rest),
            "text" => self.board_text(rest),
            "label" => self.board_label(rest),
            "silkline" => self.board_silkline(rest),
            "thermal" => self.board_thermal(rest),
            "improve" => self.board_improve(rest),
            "route" => self.board_route(rest),
            "drc" => self.board_drc(rest),
            "quality" => self.board_quality(rest),
            "status" => self.board_status(rest),
            "currents" => self.board_currents(rest),
            "postlayout" => self.board_postlayout(rest),
            "save" => self.board_save(rest),
            "open" => self.board_open(rest),
            "check" => self.board_check(rest),
            "shopping" => self.board_shopping(rest),
            "svg" => self.board_svg(rest),
            "case" => self.board_case(rest),
            "dxf_import" => self.board_dxf_import(rest),
            "dxf" => self.board_dxf(rest),
            "fab" => self.board_fab(rest),
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
    sch.flat().components.iter().map(|c| (c.refdes.clone(), c.shop_numbers())).collect()
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

/// A net class as the commands answer it.
fn class_json(c: &pcb_core::NetClass) -> Value {
    let exceptions: Vec<Value> = c.width_exceptions.iter().map(|e| json!({"net": e.net, "from": [e.a.x, e.a.y], "to": [e.b.x, e.b.y], "width": e.width})).collect();
    json!({"name": c.name, "nets": c.nets, "width": c.track_width, "clearance": c.clearance, "via": [c.via_diameter, c.via_drill], "role": c.role.name(), "order": c.route_order(), "no_vias": c.forbids_vias(), "layers": c.layers.iter().map(|s| s.key()).collect::<Vec<_>>(), "max_length": c.max_length, "max_vias": c.max_vias, "max_neck": c.max_neck, "width_exceptions": exceptions})
}

/// What the checks find, as the answers of `board break` and `board health` show it.
fn health_json(h: &pcb_core::breakage::Health) -> serde_json::Value {
    json!({"drc": h.drc, "unrouted": h.unrouted, "outside": h.outside, "bypass": h.bypass, "total": h.total()})
}
