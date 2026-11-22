//! The window-wide keys of `actions::ACTIONS` (Ctrl+K, Ctrl+F, Ctrl+S, Ctrl+Shift+S, Ctrl+O, Ctrl+N, F5) and the
//! palette: what it lists, and what a chosen row does.

use super::*;
use crate::actions::{self, Act, Entry, Run};

/// How many recent rows the palette keeps.
const RECENT: usize = 5;

impl App {
    /// The keys that work wherever the focus is (a Ctrl key also from a text field).
    pub(super) fn global_keys(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut as K, Modifiers as M};
        let shift_cmd = M::COMMAND | M::SHIFT;
        let (palette, find, save_as, save, open, new, f5) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&K::new(M::COMMAND, Key::K)),
                i.consume_shortcut(&K::new(M::COMMAND, Key::F)),
                i.consume_shortcut(&K::new(shift_cmd, Key::S)),
                i.consume_shortcut(&K::new(M::COMMAND, Key::S)),
                i.consume_shortcut(&K::new(M::COMMAND, Key::O)),
                i.consume_shortcut(&K::new(M::COMMAND, Key::N)),
                i.consume_key(M::NONE, Key::F5),
            )
        });
        if palette {
            self.open_palette(false);
        }
        if find {
            self.open_palette(true);
        }
        for (hit, act) in [(save_as, Act::SaveAs), (save, Act::Save), (open, Act::Open), (new, Act::New)] {
            if hit {
                self.run_act(act);
            }
        }
        if f5 && self.mode == Mode::Schematic {
            self.run_act(Act::Rerun);
        }
    }

    fn open_palette(&mut self, find: bool) {
        self.palette = actions::Palette { open: true, find, ..Default::default() };
    }

    /// Everything the palette can offer now: the operations, the analyses, the glossary's words; with Ctrl+F the parts
    /// and nets of the schematic.
    fn palette_entries(&self) -> Vec<Entry> {
        let mut out = vec![];
        if self.palette.find {
            let (parts, nets) = self.sch.find_targets();
            for (r, v) in parts {
                out.push(Entry { id: format!("part:{r}"), name: r.clone(), desc: v, group: "部品".into(), key: String::new(), why_not: None, run: Run::Part(r) });
            }
            for n in nets {
                out.push(Entry { id: format!("net:{n}"), name: n.clone(), desc: i18n::tr("ネット").to_string(), group: "ネット".into(), key: String::new(), why_not: None, run: Run::Net(n) });
            }
            return out;
        }
        for x in actions::ACTIONS {
            let why_not = match x.place {
                actions::Place::Schematic => self.sch.why_not(x.act),
                actions::Place::Board => self.board_ed.why_not(x.act),
                actions::Place::Both if self.mode == Mode::Schematic => self.sch.why_not(x.act),
                actions::Place::Both => self.board_ed.why_not(x.act),
            };
            let name = if x.act == Act::Rerun { i18n::trf!("もう一度計算 ({})", i18n::tr(self.sch.last_run_name())) } else { i18n::tr(x.name).to_string() };
            out.push(Entry { id: x.id.into(), name, desc: x.desc.into(), group: x.group.into(), key: x.key.into(), why_not: why_not.map(String::from), run: Run::Act(x.act) });
        }
        for (key, title, hint) in crate::schematic::analysis_sections() {
            out.push(Entry { id: format!("analysis:{key}"), name: title.to_string(), desc: hint.to_string(), group: "解析".into(), key: String::new(), why_not: None, run: Run::Analysis(key) });
        }
        for t in crate::glossary::TERMS {
            out.push(Entry { id: format!("term:{}", t.name), name: t.name.into(), desc: t.short.into(), group: "用語集".into(), key: String::new(), why_not: None, run: Run::Term(t.name) });
        }
        out
    }

    /// The moving dots were turned on or off beside 「電流表示」 (or on the prototype tab): keep it in the settings file (only that key).
    pub(super) fn save_flow_dots(&mut self) {
        let on = crate::flow_dots::on();
        if on == self.prefs.flow_dots {
            return;
        }
        self.prefs.flow_dots = on;
        let file = prefs::Prefs::default_file();
        if let Err(e) = (prefs::Prefs { flow_dots: on, ..prefs::Prefs::load(&file) }).save(&file) {
            self.sch.status = e;
        }
    }

    /// The ★ of the analysis tab changed: keep it in the settings file (only that key).
    pub(super) fn save_analysis_pins(&mut self) {
        if !std::mem::take(&mut self.sch.analysis_pins_changed) {
            return;
        }
        self.prefs.analysis_pins = self.sch.analysis_pins.clone();
        let file = prefs::Prefs::default_file();
        if let Err(e) = (prefs::Prefs { analysis_pins: self.prefs.analysis_pins.clone(), ..prefs::Prefs::load(&file) }).save(&file) {
            self.sch.status = e;
        }
    }

    /// Draws the palette while it is open and does what was chosen.
    pub(super) fn palette_ui(&mut self, ctx: &egui::Context) {
        if !self.palette.open {
            return;
        }
        let all = self.palette_entries();
        let pins = self.prefs.palette_pins.clone();
        match actions::show(ctx, &mut self.palette, &all, &self.palette_recent, &pins) {
            actions::Outcome::None => {}
            actions::Outcome::TogglePin(id) => {
                let mut p = self.prefs.clone();
                if p.palette_pins.contains(&id) {
                    p.palette_pins.retain(|x| x != &id);
                } else {
                    p.palette_pins.push(id);
                }
                let file = prefs::Prefs::default_file();
                if let Err(e) = (prefs::Prefs { palette_pins: p.palette_pins.clone(), ..prefs::Prefs::load(&file) }).save(&file) {
                    self.sch.status = e;
                }
                self.prefs = p;
            }
            actions::Outcome::Run(e) => {
                if !self.palette.find {
                    self.palette_recent.retain(|x| x != &e.id);
                    self.palette_recent.insert(0, e.id.clone());
                    self.palette_recent.truncate(RECENT);
                }
                match e.run {
                    Run::Act(a) => self.run_act(a),
                    Run::Analysis(key) => {
                        self.mode = Mode::Schematic;
                        self.sch.open_analysis_section(key);
                    }
                    Run::Part(r) => {
                        self.mode = Mode::Schematic;
                        self.sch.goto_part_pub(&r);
                    }
                    Run::Net(n) => {
                        self.mode = Mode::Schematic;
                        self.sch.goto_net_pub(&n);
                    }
                    Run::Term(t) => self.glossary.open_at(t),
                }
            }
        }
    }

    /// Does one operation of the table, in the editor it belongs to (switching to it when it is the other one).
    pub(super) fn run_act(&mut self, act: Act) {
        // the bench tab's own data (the spec table's readings) is the schematic's
        let sch_mode = self.mode != Mode::Board;
        match act {
            Act::Palette => self.open_palette(false),
            Act::Find => self.open_palette(true),
            Act::New => {
                if self.confirm_unsaved_one(sch_mode, "新規にすると、いまの図は消えます。") {
                    if sch_mode { self.sch.new_file() } else { self.board_ed.new_file() }
                }
            }
            Act::Open => {
                if sch_mode { self.sch.open() } else { self.board_ed.open() }
            }
            Act::Save => {
                if sch_mode { self.sch.save(false) } else { self.board_ed.save(false) }
            }
            Act::SaveAs => {
                if sch_mode { self.sch.save(true) } else { self.board_ed.save(true) }
            }
            Act::Help => self.help.show(if sch_mode { help::HelpTab::Schematic } else { help::HelpTab::Board }),
            Act::Glossary => self.glossary.open_at(""),
            Act::PowerOn => self.power_on.open_for(&self.sch),
            Act::Settings => self.open_settings(),
            Act::FlowDots => {
                crate::flow_dots::set_on(!crate::flow_dots::on());
                self.sch.status = if crate::flow_dots::on() { "電流の動く点を出します".into() } else { "電流の動く点を消しました".into() };
            }
            Act::Calc => {
                self.mode = Mode::Schematic;
                self.sch.open_wizard(0);
            }
            _ => {
                let place = actions::action(act).place;
                let on_sch = match place {
                    actions::Place::Schematic => true,
                    actions::Place::Board => false,
                    actions::Place::Both => sch_mode,
                };
                self.mode = if on_sch { Mode::Schematic } else { Mode::Board };
                let done = if on_sch { self.sch.run_act(act) } else { self.board_ed.run_act(act) };
                debug_assert!(done, "{act:?} is handled by its editor");
            }
        }
    }
}
