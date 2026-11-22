//! `file.pcbjson` and the options that act on the board (`--drc`, `--finish`, `--3d`, `--order` ...). They are read after a
//! `.pcbjson` is opened, and after a `.ccproj` (a project with the board in it) when one of them is given.

use super::*;

/// `file.pcbjson`, with `--post-layout [--dc] [--schematic]`.
pub(super) fn apply_file(app: &mut App, a: &Args) {
    let Some(p) = a.file(".pcbjson") else { return };
    app.mode = Mode::Board;
    app.board_ed.load_path(p.into());
    apply(app, a);
    if a.has("--post-layout") {
        // the board's copper resistance goes into the schematic's analyses; show the schematic (with --dc, solved again)
        app.apply_post_layout(true);
        if a.has("--dc") {
            app.sch.analyze_dc();
        }
        if a.has("--schematic") {
            app.mode = Mode::Schematic;
        }
    }
}

pub(super) fn apply(app: &mut App, a: &Args) {
    if a.has("--drc") {
        app.board_ed.run_drc();
    }
    // --net-window NET [TAB]: the net's settings window, on the tab 0 (width), 1 (class) or 2 (finish)
    if let Some(i) = a.pos("--net-window")
        && let Some(net) = a.get(i + 1)
    {
        app.board_ed.open_net_window(net);
        if let Some(t) = a.get(i + 2).and_then(|t| t.parse::<u8>().ok()) {
            app.board_ed.set_net_tab(t);
        }
    }
    if let Some(n) = a.num::<usize>("--select-track") {
        app.board_ed.select_track(n);
    }
    // --board-tool track | via | zone ...: the "手で描く" tab with that tool picked (its settings open)
    if let Some(name) = a.val("--board-tool")
        && !app.board_ed.pick_tool_named(name)
    {
        eprintln!("--board-tool {name}: no such tool (select, track, via, measure, zone, keepout, padth, padsmd, hole, rect, poly, silk, text)");
    }
    // --board-step place | route | check | order: that step of the "手順" tab open
    if let Some(name) = a.val("--board-step")
        && !app.board_ed.open_step_named(name)
    {
        eprintln!("--board-step {name}: no such step (place, route, check, order)");
    }
    // --board-view-open: the "表示とロック" box over the board unfolded
    if a.has("--board-view-open") {
        app.board_ed.open_view_box = true;
    }
    // --select-board text:N | label:R1 | silk:N | R1 | R1,R2,R3: something on the board picked (its settings in the panel), or several
    // parts (the align buttons)
    if let Some(w) = a.val("--select-board") {
        app.board_ed.select_named(w);
    }
    // --view <x mm> <y mm> <pixels per mm>: where to look (for a screenshot of a spot)
    if let Some([x, y, z]) = a.vals::<3>("--view")
        && let (Ok(x), Ok(y), Ok(z)) = (x.parse::<f32>(), y.parse::<f32>(), z.parse::<f32>())
    {
        app.board_ed.set_view(x, y, z);
    }
    if a.has("--finish") {
        app.board_ed.set_finish(true);
    }
    if a.has("--autoplace") {
        app.board_ed.autoplace();
    }
    if let Some(r) = a.num::<f64>("--round") {
        app.board_ed.round_outline(r);
    }
    // --ir <net> <amps>  (repeatable): declare the net's current and show the voltage drop of its copper
    for i in a.all("--ir") {
        if let Some([net, amps]) = a.words(i + 1)
            && let Ok(amps) = amps.parse::<f64>()
        {
            app.board_ed.show_ir_drop(net, amps);
        }
    }
    thermal_map(app, a);
    if a.has("--3d") {
        view3d(app, a);
    }
    if a.has("--order") {
        app.board_ed.open_order();
    }
    // --nets: the window with the table of the nets
    if a.has("--nets") {
        app.board_ed.net_table.open = true;
    }
    // `--field [pair|inner|cpw]`: the cross-section window (a 0.2 mm pair, an inner layer, ground copper beside the track)
    if a.has("--field") {
        let f = &mut app.board_ed.field_view;
        f.open = true;
        match a.val("--field") {
            Some("pair") => {
                f.pair = true;
                f.width = 0.2;
            }
            Some("inner") => {
                f.inner = true;
                f.pair = true;
                f.width = 0.1;
                f.space = 0.15;
            }
            Some("cpw") => {
                f.coplanar = true;
                f.width = 1.0;
                f.gap = 0.2;
            }
            _ => {}
        }
    }
    if let Some(dir) = a.val("--export") {
        app.board_ed.status = match app.board_ed.write_fab_files(std::path::Path::new(dir)) {
            Ok(n) => format!("書き出し完了: {dir} ({n} ファイル + zip)"),
            Err(e) => format!("書き出し失敗: {e}"),
        };
    }
}

/// `--thermal-map [standing]`: the "熱の地図" window worked out from the schematic's self-heating, its colours over the board.
fn thermal_map(app: &mut App, a: &Args) {
    if !a.has("--thermal-map") {
        return;
    }
    if a.val("--thermal-map") == Some("standing") {
        app.board_ed.thermal.spec.orientation = pcb_core::thermal::Orientation::Standing;
    }
    app.board_ed.open_thermal_view();
    // --thermal-case W D TOP BOTTOM: the board in a closed case of that inside (mm)
    if let Some(j) = a.pos("--thermal-case") {
        let n: Vec<f64> = (j + 1..j + 5).filter_map(|k| a.get(k)?.parse().ok()).collect();
        if let [w, d, top, bottom] = n.as_slice() {
            let c = &mut app.board_ed.thermal.case;
            (c.inner_w, c.inner_d, c.top_mm, c.bottom_mm) = (*w, *d, *top, *bottom);
            app.board_ed.thermal.in_case = true;
        }
    }
    let run = app.sch.run_self_heat();
    let sources = run.as_ref().map(|v| app.sch.thermal_sources(v)).map_err(|e| e.clone());
    if let Ok(v) = &run {
        app.board_ed.thermal.traces = v.traces.clone();
    }
    app.sch.heat.view = Some(run);
    app.board_ed.run_thermal_map(sources);
    // --thermal-tran SECONDS [FRAME]: the board warming up, that frame (0..7) on the board
    if let Some(j) = a.pos("--thermal-tran")
        && let Some(t) = a.get(j + 1).and_then(|t| t.parse::<f64>().ok())
    {
        app.board_ed.thermal.tran_end = t;
        app.board_ed.run_thermal_transient(None);
        if let Some(f) = a.get(j + 2).and_then(|f| f.parse::<usize>().ok()) {
            app.board_ed.thermal.show_frame = Some(f);
        }
    }
}

/// `--3d` and how the 3D view looks (`--3d-cpu`, `--3d-look`, `--3d-ao`, `--3d-case`, `--3d-cam`, `--3d-heat`, `--3d-net`).
fn view3d(app: &mut App, a: &Args) {
    app.board_ed.open_view3d();
    // --3d-cpu: the software picture even when the GPU is there (to compare the two)
    if a.has("--3d-cpu") {
        app.board_ed.view3d_use_gpu(false);
    }
    // --3d-look clay|color and --3d-ao 0|1: how the GPU picture looks
    if a.val("--3d-look").is_some() || a.val("--3d-ao").is_some() {
        app.board_ed.view3d_look(a.val("--3d-look") == Some("clay"), a.val("--3d-ao") != Some("0"));
    }
    // --3d-case w,d,top,bottom: the case check with the case's inside (screw posts and wall openings in the picture)
    if let Some(c) = a.val("--3d-case") {
        let n: Vec<f64> = c.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if n.len() == 4 {
            app.board_ed.view3d_case(n[0], n[1], n[2], n[3]);
        }
    }
    // --3d-cam yaw,pitch[,zoom]: the angle of the 3D view in degrees (for screenshots from another side)
    if let Some(c) = a.val("--3d-cam") {
        let n: Vec<f32> = c.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if n.len() >= 2 {
            app.board_ed.view3d_camera(n[0], n[1], n.get(2).copied().unwrap_or(1.0));
        }
    }
    // --3d-heat: paint the parts by the schematic's self-heating temperatures (with `--selfheat`)
    if a.has("--3d-heat") {
        app.board_ed.view3d_heat(true);
    }
    // --3d-net <net>: light that net's copper in the 3D view
    if let Some(n) = a.val("--3d-net") {
        app.board_ed.set_view3d_net(n);
    }
}
