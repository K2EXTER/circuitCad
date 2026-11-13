//! Rules the board panel keeps for every tool, checked over all of them at once so that a new tool or a new setting cannot
//! quietly break one:
//! - the tool bars stay where they are, whatever tool is picked and whatever is selected (the panel of a picked item, or a
//!   tool's settings, must not push them down);
//! - every tool either has a settings box or is listed as having none;
//! - what a tool's settings box shows is what the tool uses when it puts something down.

use super::*;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// Every text drawn this frame, with where its top-left is.
fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Pos2)> {
    fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
        match s {
            egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos)),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    for c in shapes {
        walk(&c.shape, &mut out);
    }
    out
}

/// Draws the side panel in the "手で描く" tab (a few frames, so folds and sizes settle) and gives the y of each tool-group heading.
fn bar_rows(e: &mut BoardEditor, sch: &crate::schematic::SchEditor) -> Vec<f32> {
    let ctx = egui::Context::default();
    let mut found = vec![];
    for _ in 0..3 {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() }, |ui| {
            egui::Panel::left("board_tools").default_size(crate::PANEL_WIDTH).show(ui, |ui| e.side_panel(ui, sch));
        });
        out.textures_delta.clear();
        let t = texts(&out.shapes);
        found = TOOL_GROUPS.iter().map(|(name, _)| t.iter().find(|(s, _)| s == name).map_or(f32::NAN, |(_, at)| at.y)).collect();
    }
    found
}

/// A board with one of everything the panel can show as selected.
fn busy_editor() -> BoardEditor {
    let mut e = BoardEditor::default();
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
    e.board.tracks.push(Track { side: Side::Top, a: p(5.0, 5.0), b: p(15.0, 5.0), width: 0.25 });
    for (tool, at) in [(Tool::Via, p(20.0, 20.0)), (Tool::PadTh, p(25.0, 20.0)), (Tool::PadSmd, p(30.0, 20.0)), (Tool::Hole, p(35.0, 20.0)), (Tool::Text, p(20.0, 25.0))] {
        e.set_tool(tool);
        e.click(at);
    }
    e.set_tool(Tool::Silk);
    e.click(p(5.0, 28.0));
    e.click(p(15.0, 28.0));
    e.set_tool(Tool::Zone);
    e.zone_net = "GND".into();
    e.poly = vec![p(1.0, 1.0), p(9.0, 1.0), p(9.0, 9.0)];
    e.poly_verts = e.poly.iter().map(|q| pcb_core::shape::Vertex::line(*q)).collect();
    e.finish_polygon();
    e
}

#[test]
fn the_tool_bars_stay_put_whatever_is_picked_or_selected() {
    let sch = crate::schematic::SchEditor::default();
    let mut e = busy_editor();
    e.panel_tab = PanelTab::Draw;
    e.set_tool(Tool::Select);
    e.selected = None;
    let home = bar_rows(&mut e, &sch);
    assert!(home.iter().all(|y| y.is_finite()), "the group headings are drawn: {home:?}");
    // every tool
    for (tool, name, _) in TOOLS {
        e.set_tool(*tool);
        assert_eq!(bar_rows(&mut e, &sch), home, "picking 「{name}」 moved the tool bars");
    }
    // everything that can be selected, with the select tool and with the tool that made it
    let b = &e.board;
    let label = b.texts.iter().position(|t| !t.refdes.is_empty()).expect("the part's designator");
    let text = b.texts.iter().position(|t| t.refdes.is_empty()).expect("the free text");
    let picks = [
        (Item::Part(0), Tool::Select),
        (Item::Label(label), Tool::Select),
        (Item::Track(0), Tool::Track),
        (Item::Via(0), Tool::Via),
        (Item::Pad(b.pads.len() - 2), Tool::PadTh),
        (Item::Pad(b.pads.len() - 1), Tool::PadSmd),
        (Item::Hole(0), Tool::Hole),
        (Item::Text(text), Tool::Text),
        (Item::Silk(b.silk.len() - 1), Tool::Silk),
        (Item::Zone(0), Tool::Zone),
    ];
    for (item, tool) in picks {
        for t in [Tool::Select, tool] {
            e.set_tool(t);
            e.selected = Some(item);
            assert_eq!(bar_rows(&mut e, &sch), home, "selecting {item:?} with the tool {t:?} moved the tool bars");
        }
    }
}

/// The tools that have nothing to set: what they do needs no number or choice.
const NO_SETTINGS: &[Tool] = &[Tool::Select, Tool::Measure];

#[test]
fn every_tool_has_a_settings_box_or_is_listed_as_having_none() {
    for (tool, name, _) in TOOLS {
        let mut e = BoardEditor::default();
        e.set_tool(*tool);
        assert_ne!(e.has_tool_options(), NO_SETTINGS.contains(tool), "「{name}」: give it a settings box (tool_options_body), or list it in NO_SETTINGS");
    }
    // the bars show every tool once
    let in_bars: Vec<Tool> = TOOL_GROUPS.iter().flat_map(|(_, t)| t.iter().map(|(x, _)| *x)).collect();
    for (tool, name, _) in TOOLS {
        assert_eq!(in_bars.iter().filter(|x| *x == tool).count(), 1, "「{name}」 is in the tool bars once");
    }
}

/// The tools whose settings are not about the one thing a click puts down, so the next test does not cover them: the wire tool
/// (how a route is found, tested with the routing), the pours and keep-outs (a polygon, tested with them) and the outline
/// (numbers turned into an outline by buttons, tested in `view.rs`).
const SETTINGS_TESTED_ELSEWHERE: &[Tool] = &[Tool::Track, Tool::Zone, Tool::Keepout, Tool::OutlineRect, Tool::OutlinePoly];

#[test]
fn what_a_tools_settings_show_is_what_it_puts_down() {
    // (tool, set the box to unusual values, check what one click put down)
    type Case = (Tool, fn(&mut BoardEditor), fn(&BoardEditor) -> Result<(), String>);
    let cases: &[Case] = &[
        (Tool::Via, |e| (e.via_dia, e.via_drill) = (1.3, 0.55), |e| {
            let v = e.board.vias.last().ok_or("no via")?;
            ((v.diameter, v.drill) == (1.3, 0.55)).then_some(()).ok_or(format!("via {} / {}", v.diameter, v.drill))
        }),
        (Tool::PadTh, |e| (e.pad_size, e.pad_drill) = (2.2, 1.1), |e| {
            let d = e.board.pads.last().ok_or("no pad")?;
            (d.shape == PadShape::Circle { d: 2.2 } && d.drill == Some(1.1)).then_some(()).ok_or(format!("pad {:?} / {:?}", d.shape, d.drill))
        }),
        (Tool::PadSmd, |e| (e.smd_w, e.smd_h, e.smd_oval, e.pad_drill) = (0.7, 1.9, true, 1.1), |e| {
            let d = e.board.pads.last().ok_or("no pad")?;
            (d.shape == PadShape::Oval { w: 0.7, h: 1.9 } && d.drill.is_none()).then_some(()).ok_or(format!("pad {:?} / {:?}", d.shape, d.drill))
        }),
        (Tool::Hole, |e| e.hole_drill = 3.4, |e| {
            let h = e.board.holes.last().ok_or("no hole")?;
            (h.drill == 3.4).then_some(()).ok_or(format!("hole {}", h.drill))
        }),
        (Tool::Silk, |e| e.silk_width = 0.37, |e| {
            let s = e.board.silk.last().ok_or("no silk line")?;
            (s.width == 0.37).then_some(()).ok_or(format!("silk {}", s.width))
        }),
        (Tool::Text, |e| e.text_height = 2.5, |e| {
            let t = e.board.texts.last().ok_or("no text")?;
            (t.height == 2.5).then_some(()).ok_or(format!("text {}", t.height))
        }),
    ];
    for (tool, name, _) in TOOLS {
        let e = &mut BoardEditor::default();
        e.set_tool(*tool);
        if !e.has_tool_options() || SETTINGS_TESTED_ELSEWHERE.contains(tool) {
            continue;
        }
        let Some((_, set, check)) = cases.iter().find(|(t, _, _)| t == tool) else {
            panic!("「{name}」 has settings: add a case here (or list it in SETTINGS_TESTED_ELSEWHERE with where it is tested)");
        };
        set(e);
        // a silk line takes two clicks
        e.click(p(10.0, 10.0));
        if *tool == Tool::Silk {
            e.click(p(20.0, 10.0));
        }
        if let Err(what) = check(e) {
            panic!("「{name}」 did not put down what its settings say: {what}");
        }
    }
}
