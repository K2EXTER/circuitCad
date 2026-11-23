//! The part form from the command line (`--edit-part`, `--form-*`): screenshots of each section, and `--form-save DIR` to make a
//! library part without the screen (`ccad --lib DIR`). The order matters: the PDF is loaded before it is read, and the save quits.

use super::*;

pub(super) fn apply(app: &mut App, a: &Args) {
    // --edit-part <name>: a library part in the part form; --form-shape builds its 3D shape from the template (SolidCraft)
    if let Some(n) = a.val("--edit-part") {
        app.sch.edit_part_in_form(n);
    }
    if a.has("--form-shape") {
        app.sch.form_shape_from_template();
    }
    if let Some(t) = a.val("--form-tab") {
        app.sch.set_form_tab(t);
    }
    if a.has("--form-pad-edit") {
        app.sch.form_pad_edit(a.after("--form-pad-edit"));
    }
    if let Some(f) = a.val("--form-step") {
        app.sch.load_step_into_form(std::path::Path::new(f));
    }
    if let Some(f) = a.val("--form-pdf") {
        app.sch.load_pdf_into_form(std::path::Path::new(f));
    }
    if let Some(t) = a.val("--form-find") {
        app.sch.find_in_form_pdf(t);
    }
    // --form-guide N: the data sheet window's reading guide at step N (from 1)
    if let Some(k) = a.val("--form-guide").and_then(|k| k.parse::<usize>().ok()) {
        app.sch.guide_in_form_pdf(k);
    }
    // --kicad-lib <file>: the "KiCadから取り込み" window on that library
    if let Some(f) = a.val("--kicad-lib") {
        app.sch.open_kicad_import_file(std::path::Path::new(f));
    }
    // --sim-settings: the "シミュレーション設定" window
    if a.has("--sim-settings") {
        app.sch.open_sim_settings();
    }
    // --form-drawing top|side "page,x0,y0,x1,y1": an outline drawing of the form's PDF cut out for SolidCraft (repeat for the other view)
    for i in a.all("--form-drawing") {
        if let Some([view, spec]) = a.words(i + 1) {
            app.sch.drawing_in_form_pdf(view, spec);
        }
    }
    // --form-draft: "データシートから下書き" with everything found put in (after --form-pdf); --form-draft-package FAMILY picks the
    // package of that family ("wson", "qfn") instead of the one written most
    if a.has("--form-draft") {
        app.sch.draft_in_form(a.val("--form-draft-package"));
    }
    // --drawing-big: the big window of the cut-out drawing, with two points and a length in inches
    if a.has("--drawing-big") {
        app.sch.drawing_big_in_form();
    }
    if let Some(t) = a.val("--form-table") {
        app.sch.table_in_form_pdf(t);
    }
    // --form-graph "page|x=0.2,0.8|y=1.0E-04,1.0E+00|pts=0.55:1e-3": reading points off a graph of the form's PDF
    if let Some(t) = a.val("--form-graph") {
        app.sch.graph_in_form_pdf(t);
    }
    // --form-graph-for DarlHfe: a field's "グラフから読む", the PDF taken to that field's graph by its caption
    if let Some(t) = a.val("--form-graph-for") {
        app.sch.graph_for_in_form_pdf(t);
    }
    // --form-graph-all bjt: a section's 「表とグラフから全部読む」 (bjt, darl, diode, opto, pd, reg)
    let graph_all = a.val("--form-graph-all");
    if let Some(t) = graph_all {
        app.sch.graph_all_in_form_pdf(t);
        // --form-save DIR: then the model put to use and the part saved into DIR, and quit
        if let Some(dir) = a.val("--form-save") {
            save_and_quit(app, t, dir);
        }
    }
    // --form-companions: 「推奨の周辺部品」 read from the form's PDF
    if a.has("--form-companions") {
        app.sch.companions_in_form();
    }
    // --form-mos-pdf: the MOSFET section read from the form's PDF and fitted
    if a.has("--form-mos-pdf") {
        app.sch.mos_from_form_pdf();
    }
    // --form-bjt-pdf: the transistor section (--new-part npn) read from the PDF's table, fitted
    if a.has("--form-bjt-pdf") {
        app.sch.bjt_in_form(true);
    }
    // --form-opamp-example / --form-opamp-pdf / --form-regulator-example [adj]: the op-amp or regulator box (--new-part ic) made
    if a.has("--form-opamp-example") {
        app.sch.linear_in_form("opamp");
    }
    if a.has("--form-opamp-pdf") {
        app.sch.linear_in_form("opamp-pdf");
    }
    if a.has("--form-regulator-example") {
        app.sch.linear_in_form(if a.val("--form-regulator-example") == Some("adj") { "adj" } else { "fixed" });
    }
    // --form-variants [DIR]: the regulator's voltage versions found in the form's PDF and made at once (after the regulator's
    // values are in: --form-regulator-example or --form-graph-all reg); with DIR the parts are saved there and the app quits
    if a.has("--form-variants")
        && let Some(ok) = app.sch.variants_in_form(a.after("--form-variants").map(std::path::Path::new))
    {
        std::process::exit(if ok { 0 } else { 1 });
    }
    // --form-lineup [PN,PN]: the electrolytic capacitor's lineup table of the form's PDF read (--new-part electrolytic), these rows checked
    if a.has("--form-lineup") {
        app.sch.lineup_in_form(a.after("--form-lineup").unwrap_or(""));
    }
    // --form-opto-example (--new-part ic) / --form-photodiode-example (--new-part diode) / --form-darlington-example (--new-part npn) /
    // --form-ntc-example (--new-part resistor) / --form-crystal-example / --form-tvs-example (--new-part diode) / --form-tvs-pdf (its table) / --form-relay-example (--new-part ic) /
    // --form-reference-example / --form-setldo-example (--new-part ic: the REF5025 or LT3045 example with its pins)
    for (flag, what) in [("--form-opto-example", "opto"), ("--form-photodiode-example", "photodiode"), ("--form-darlington-example", "darlington"), ("--form-ntc-example", "ntc"), ("--form-crystal-example", "crystal"), ("--form-tvs-example", "tvs"), ("--form-tvs-pdf", "tvs-pdf"), ("--form-relay-example", "relay"), ("--form-reference-example", "reference"), ("--form-setldo-example", "setldo")] {
        if a.has(flag) {
            app.sch.linear_in_form(what);
        }
    }
    // --form-bjt-example: the transistor section with the 2N3904 example, fitted
    if a.has("--form-bjt-example") {
        app.sch.bjt_in_form(false);
    }
    // --form-logic-example [counter]: the logic IC box (--new-part ic) with the half adder (or a counter), made into pins and a model
    if a.has("--form-logic-example") {
        app.sch.logic_in_form(a.val("--form-logic-example") == Some("counter"));
    }
    // --form-save DIR without --form-graph-all: the part as the form has it (after the examples above) saved, and quit
    if !a.has("--form-graph-all")
        && let Some(dir) = a.val("--form-save")
    {
        save_and_quit(app, "", dir);
    }
    if let Some(t) = a.val("--form-pick") {
        app.sch.pick_in_form_pdf(t);
    }
}

fn save_and_quit(app: &mut App, section: &str, dir: &str) -> ! {
    let ok = app.sch.save_form_part(section, std::path::Path::new(dir));
    std::process::exit(if ok { 0 } else { 1 });
}
