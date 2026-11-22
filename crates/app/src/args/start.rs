//! Lessons, challenges and the windows opened from the command line (mostly for screenshots).

use super::*;

/// `--lesson`, `--board-lesson`, `--repair`, `--challenge`, `--settings`, `--new-part`.
pub(super) fn lessons(app: &mut App, a: &Args) {
    if let Some(id) = a.val("--lesson")
        && let Some(n) = schematic::lesson_ids().iter().position(|l| *l == id)
    {
        app.sch.start_lesson(n);
        // `--lesson repair_zener --probe VOUT,VIN`: readings of the virtual meter (the repair lessons)
        if let Some(nets) = a.val("--probe") {
            app.sch.probe_nets(nets);
        }
        // `--lesson <id> --dc`: the first analysis is done already (to see the second step of a lesson)
        if a.has("--dc") {
            app.sch.analyze_dc();
        }
    }
    // --board-lesson [0..7]: 0 = schematic to order files, 1 = copper width and voltage drop ...
    if a.has("--board-lesson") {
        app.start_board_lesson(a.num("--board-lesson").unwrap_or(0));
    }
    // --board-lesson 7 --repair [seed]: put a fault into the board of the repair lesson (the seed picks the kind: 0..4 are the five kinds)
    if a.has("--repair") {
        app.board_ed.repair_inject(a.num("--repair").unwrap_or(1));
    }
    // --challenge <id> [level] [--judge]
    if let Some(i) = a.pos("--challenge")
        && let Some(id) = a.get(i + 1)
    {
        app.sch.start_challenge(id, a.get(i + 2).and_then(|l| l.parse().ok()).unwrap_or(0));
        if a.has("--judge") {
            app.sch.run_judge();
        }
    }
    // --settings [look | input | files | board | colors]: the settings window open at that tab
    if a.has("--settings") {
        app.settings_tab = match a.val("--settings") {
            Some("input") => settings_window::SettingsTab::Input,
            Some("files") => settings_window::SettingsTab::Files,
            Some("board") => settings_window::SettingsTab::Board,
            Some("colors") => settings_window::SettingsTab::Colors,
            _ => settings_window::SettingsTab::Look,
        };
        app.open_settings();
    }
    // --new-part [ic|transistor|nmos|pmos|npn|pnp|diode|chip (= ceramic)|ceramic|electrolytic|tantalum|film|resistor|inductor|connector|connector2 (2-row header)]
    if a.has("--new-part") {
        app.sch.open_part_form_for(a.after("--new-part").unwrap_or(""));
    }
}

/// The windows and looks that are not tied to a file: `--welcome`, `--glossary`, `--learn`, `--palette`, `--paper`, `--screenshot` ...
pub(super) fn windows(app: &mut App, a: &Args) {
    // --welcome: the first-launch card (it is up only on a first launch with no arguments)
    if a.has("--welcome") {
        app.welcome = true;
    }
    // --recovery-demo: the question about an unsaved copy, with a made-up copy (the real autosave folder is not read or touched)
    if a.has("--recovery-demo") {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        app.recovery = Some(crate::autosave::Found { sch: true, board: true, sch_path: Some("led_board.schjson".into()), board_path: None, project: None, saved_at: now.saturating_sub(300) });
    }
    // --title-block: the "図枠" window
    if a.has("--title-block") {
        app.sch.open_title_block();
    }
    if a.has("--measure-guide") {
        app.measure.open = true;
    }
    // --glossary [term]: the glossary window, on a term
    if a.has("--glossary") {
        app.glossary.open_at(a.after("--glossary").unwrap_or(""));
    }
    if a.has("--icons-only") {
        crate::icons::set_icon_only(true);
    }
    // `--learn [arduino_board]`: the learning paths, on that path
    if a.has("--learn") {
        app.learn.open = true;
        if let Some(id) = a.after("--learn") {
            app.learn.select(id);
        }
    }
    // --quick [word]: the quick-add box (key A) with a word already typed
    if a.has("--quick") {
        app.sch.open_quick_add_with(a.after("--quick").unwrap_or(""));
    }
    // --lib-part SERIES VALUE: the library list on a part (a series' name with a value: "ERJ-3EKF 4.8k")
    if let Some(i) = a.pos("--lib-part") {
        app.sch.open_library_part(a.word(i + 1).unwrap_or(""), a.word(i + 2).unwrap_or(""));
    }
    // --lib-search [words]: the library list's search box
    if a.has("--lib-search") {
        app.sch.set_library_search(a.after("--lib-search").unwrap_or(""));
    }
    if let Some(w) = a.val("--guide-search") {
        app.help.search(w);
    }
    // --guide [schematic|waveform|board|3d]
    if a.has("--guide") {
        app.help.show(match a.val("--guide") {
            Some("waveform") => help::HelpTab::Waveform,
            Some("board") => help::HelpTab::Board,
            Some("3d") => help::HelpTab::View3d,
            _ => help::HelpTab::Schematic,
        });
    }
    if let Some(p) = a.val("--screenshot") {
        app.auto_shot = Some((p.into(), 40));
    }
    // `--palette [words]`: the command palette open (with the words typed)
    if a.has("--palette") {
        app.palette.open = true;
        app.palette.query = a.after("--palette").unwrap_or("").to_string();
    }
    // the pictures for the README are on paper, whatever theme the user has (`--paper`)
    // `--light-panel`: the panels and windows on a light ground for this launch (screenshots), whatever the setting
    if a.has("--light-panel") {
        crate::theme::set_light(true);
    }
    if a.has("--paper") {
        crate::canvas_theme::set_palette(crate::canvas_theme::PAPER);
    }
}
