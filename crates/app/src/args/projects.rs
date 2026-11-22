//! The multi-board sample and `.ccproj` projects (a schematic and its board together).

use super::*;

/// `--multiboard-sample`: the multi-board sample with its window of boards and links.
pub(super) fn multiboard_sample(app: &mut App, a: &Args) {
    // --cable-sample: the boards joined by a cable, with the cable's wire list open
    if a.has("--cable-sample") {
        app.open_cable_sample();
    } else if a.has("--multiboard-sample") {
        app.open_multiboard_sample();
    } else {
        return;
    }
    if a.has("--system-dc") {
        app.run_system_dc();
    }
    // --system-tran / --system-ac: the whole system's transient (the sample's 20 µs) or AC sweep in the window
    if a.has("--system-tran") {
        app.run_system_wave(true);
    }
    if a.has("--system-ac") {
        app.run_system_wave(false);
    }
    // with --3d: the main board's 3D view with the stacked sensor board in it
    if a.has("--3d") {
        app.boards.open = false;
        app.mode = Mode::Board;
        board::apply(app, a);
    }
}

/// `file.ccproj` and what acts on it.
pub(super) fn apply(app: &mut App, a: &Args) {
    let Some(p) = a.file(".ccproj") else { return };
    app.load_project(p.into());
    // `--boards`: the window of the boards and their links (the check and its fixes)
    if a.has("--boards") {
        app.boards.open = true;
    }
    // `--select R1` also picks the part of a project's schematic (the board and the 3D view show it: the linked pick)
    if let Some(r) = a.val("--select") {
        app.sch.select_part(r);
    }
    // a project has both halves: the board's copper can go into the schematic's transient run (`--post-layout --tran stop step`)
    if a.has("--post-layout") {
        app.apply_post_layout(true);
    }
    sheet::tran(app, a);
    // the heat runs (`--selfheat`, `--heattran PARTS TSTOP`, `--heat-paint` ...): the 3D view's heat colours (`--3d-heat`) use them
    sheet::heat(app, a);
    sheet::section(app, a);
    if ["--drc", "--finish", "--3d", "--order", "--export", "--ir", "--select-track", "--net-window", "--nets", "--field", "--thermal-map"].iter().any(|f| a.has(f)) {
        app.mode = Mode::Board;
        board::apply(app, a);
    }
}
