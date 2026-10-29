//! Script state and the command table.

use crate::{Out, Res};
use part_lib::PartDef;
use pcb_core::model::Board;
use sch_core::Schematic;
use serde_json::{json, Value};

pub const HELP: &str = "\
Commands (one per line, '#' starts a comment; every answer is one JSON line)

 Schematic
  new                                   empty schematic
  open FILE.schjson / save FILE.schjson
  parts [WORDS]                         search the part library (name, category, description)
  part NAME                             pins (number + name) of a library part or a basic type (R C L D LED V NPN)
  part import FILE [name=NAME] [subckt=SUB] [pins=A,B,..] [map=1,2,..] [package=PART]
                                        make a library part from a maker's SPICE .subckt (a plain-text file you downloaded):
                                        pins = the symbol's pin names in the subcircuit's node order, map = their pin numbers,
                                        package = a library part whose footprint is borrowed (default TL072, SOIC-8). Encrypted models are refused.
  lib [reload]                          the library folder and the number of parts; 'lib reload' reads it again (after part import or editing a file)
  add TYPE [VALUE] [as=REF] [rot=0..3] [at=X,Y]
        TYPE: R C L D LED V NPN HEADER2 HEADER4 ... or a library part name (see 'parts')
  value REF VALUE                       change the value (a value with spaces, like PULSE(0 5 1m 1u), needs no quotes)
  nc REF.PIN [REF.PIN ...] [remove=1]    pins left unconnected on purpose (an unused output, a pin the data sheet says to leave open): ERC does not ask about them; no pins = the list
  pinnames REF NAME1,NAME2,...          the names of the pins of a pin header (F+,F-,SA,SB,GND): written beside its symbol and, with `board pinlabels`, on the silk (`-` clears)
  simonly REF [off]                     the part is only for the simulation (a test source, a stand-in for the device under test): it is left off the board and the bill of materials
  net NAME REF.PIN [REF.PIN ...]        connect pins by name; REF.PIN is the pin number or its name (any case)
                                        (GND pins of a part that share a name are all connected)
                                        The net named GND is the 0 V reference: every circuit needs one.
  remove REF
  list                                  parts, pins and the net of every pin (of the active sheet)
  sheet [add [NAME] | use N|NAME | name NAME | move REF.. to=N|NAME | tidy [all]]
                                        several sheets: no argument lists them; add makes an empty one (and makes it the active one);
                                        move sends parts (with the net labels on their pins) to another sheet; tidy packs the parts of the
                                        active sheet (or 'tidy all') into compact rows, so that the whole sheet fits one screen. Nets join across
                                        sheets by the same net label or power symbol; add / net / remove / list act on the active sheet
  erc                                   electrical rule check (message + hint). 'ok' only says the command ran:
                                        read 'errors' (or run with --strict, which fails the script on an ERC error)
  spice                                 the SPICE netlist text
 Simulation
  dc [full] [probe=V(out),I(R1)]        operating point: node voltages, element currents (probe= returns only those)
                                        (elements inside a library part's macro model are hidden unless 'full')
                                        'ratings' lists parts over / near their package power limit (like the app);
                                        'warnings' also flags an op-amp whose IN+ and IN- differ (saturated output)
                                        'solver' = how it was solved (strategy, iterations, seconds). A circuit with flip-flops lists them in 'memory':
                                        ic=NAME=1,NAME=0 sets their starting states. A latch built from gates has more than one answer:
                                        guess=NET=V,NET=V picks the one near those voltages (like .nodeset); 'dc states' also solves
                                        from other starting points and lists the nets that settle elsewhere ('other_states')
  tran TSTOP [TSTEP] [probe=V(out),I(R1)] [samples=6] [uic]
                                        starts from the DC operating point (a capacitor starts charged);
                                        add 'uic' to start from zero instead (step responses, oscillators)
                                        'ratings' / 'warnings' (second half of the run): resistor rms power, LED peak current,
                                        electrolytic ripple current, op-amp inputs that stay apart (saturation)
                                        settle=V(out) [from=20u] [tol=1e-2,1e-3,1e-4]: from the instant `from`, the time until the signal
                                        stays within each tolerance (a fraction of the step) of its final value; also the overshoot. ic= and guess= as in dc
 Maker SPICE models (the zips are the maker's own download, e.g. Murata 'SPICE netlist'; not shipped with the app)
  models [WORDS] [zip=PATH] [size=0603] [diel=X7R] [cmin=1u] [cmax=10u] [vmin=16]
                                        search the models by name; size (inch code), dielectric, capacitance and rating
                                        work for models a maker reader describes (Murata; Nippon Chemi-Con and Nichicon give capacitance and rating only). zip defaults to the ones added in the app.
                                        'models model=PN' prints the subcircuit (alias: murata)
 Design help
  calc [KEY NUMBER ...]                 value calculators (led, divider, rc, regulator, buck, boost, opamp, ...): no
                                        arguments lists them with their inputs; the answer has the value, steps, cautions
  ac FSTART FSTOP [ppd=10] [probe=V(out)]
                                        frequency response (needs a source with an AC magnitude, e.g. value V1 \"0 AC 1\"):
                                        gain and phase per decade, -3 dB frequency, phase margin
  sweep REF START STOP [steps=11] [log=1] probe=V(out),I(R1)   (or sweep REF list=1,2,5)
  sens probe=V(out) [step=1] [parts=R1,R2 | kinds=R,V,I,C,L] [top=15]
                                        sensitivity: moves each part's value by step % and says how far the probe moves (per_percent,
                                        elasticity), the biggest first; names the parts that decide an accuracy
                                        the DC point for a range of values of one part (a V source, R, C, L): one row per value; the part keeps its value
  noise OUT[,REF] FSTART FSTOP [ppd=10] [input=V1] [temp=27] [top=8]
                                        thermal / shot noise at net OUT (or the difference OUT,REF): rms over the band,
                                        density per decade, the biggest contributors; input=V1 also refers it to that source
  spec KIND TARGET LO HI [at=Hz]        pass / fail of the last dc / tran / ac answer against a range ('-' = no limit).
                                        'spec' alone lists the kinds (dc_voltage, tran_pp, ac_cutoff, ...)
                                        TARGET: dc_voltage = net name (MID); dc_current = part (D1, amps: 8m 12m);
                                        tran_* / ac_* = signal name as the answer shows it (V(OUT)); at= takes 1k, 10k ...
 Board
  board new [W H]                       empty board (mm), default 60 40
  board import                          parts and nets from the schematic (creates the board if needed)
  board pour [net=GND]                  the GND pour of the editor (bottom copper + a via per surface pad); do it BEFORE autoplace: the poured net then drops out of the placement and the parts sit closer
  board autoplace
  board move REF X Y [rot=0..3]        put one part at X Y mm (its origin; wiring is not moved)
  board route [net=NAME] [lead=MM] [bridge=0]   (neighbouring pads of one part and net, e.g. two LX pins, are strapped into one piece first; bridge=0 leaves that out)
  board drc
  board status
  board currents [tran] [from=F] [widen]  take the net currents (DC point, or with tran the RMS of the settled part of the last tran), show the track width each needs, optionally thicken thin tracks
  board thermal                         thermal vias in the heat pads that lack them (QFN / ESOP exposed pads): a grid of 0.3 mm vias inside the pad;
                                        pour the back (board pour) first so that they join copper there
  board postlayout on|off               dc/tran/ac/noise also see the copper resistance of the board (post-layout simulation)
  board check [fab=jlcpcb|pcbway|oshpark|standard]
                                        the pre-order check of the app: ready true/false, and every error / warning / note with the reason
                                        and the place (also: board no longer matches the schematic, copper drop, price tier of the fab)
  board picture [layer=top|bottom|inner1] [rect=x0,y0,x1,y1] [px=1600] [net=A,B] [part=R1,U2] [names=1] [marks=1] [others=1] [finish=1] [ghost=A,B] [ghostguard=A:GRDA] [out=FILE.png]
                                        a PNG of one copper layer to LOOK at (over MCP it comes back as an image): grid with mm numbers, nets in colours, net names, numbered DRC findings and open
                                        connections, shadows of how the router would wire a net; use rect= to zoom (a small rect gives more pixels per mm)
  board net NAME                        what the board has of a net: its pads (places), tracks per layer (length, widths), vias, pours, open connections
  board track add x1,y1 x2,y2 ... [layer=top] [width=0.25] [net=NAME]   lay a track through the points; with net= it is taken back unless it ends on that net (a short or a floating track is refused)
  board track remove net=NAME [layer=top] [vias=1] | rect=x0,y0,x1,y1 | index=N
  board via add x,y [net=NAME] [dia=0.6] [drill=0.3] / board via remove x,y [r=0.3]
  board zones [merge|bake]              the pours as lists of shapes that add and shapes that take away; merge joins overlapping pours of a net, bake turns the shapes into final corners
  board keepout layer=inner1 [net=NET] rect=x0,y0,x1,y1 | circle=x,y,r | pad=J3E,J3F [margin=1.0]   a hole in the pours of a layer (clear: board keepout clear layer=...)
  board guard net=A guard=GRDA          guard tracks on both sides of a sense net's top tracks; board copper net=DRN rect=... patches of copper; board nudge moves small parts to open a route; board layers 4; board class ...
  board save FILE.pcbjson / board open FILE.pcbjson
  board shopping [boards=N] [spare=0] [file=PATH]  what to buy: per value and footprint the quantity (qty), spares of chip R/C/L/LED (spare, 2 or a tenth; spare=0 off), their sum (buy) and, for each shop (akizuki, digikey, lcsc), the part number if the library part has one, else a search link
  board fab DIR                         Gerber, drill, BOM, placement and the zip
";

pub struct Session {
    pub sch: Schematic,
    pub lib: Vec<PartDef>,
    /// The folder the library was read from (`lib reload` reads it again).
    pub lib_dir: std::path::PathBuf,
    pub board: Option<Board>,
    /// Where the next part goes: (x, y, tallest part of the row).
    pub cursor: (i32, i32, i32),
    pub lib_errors: Vec<String>,
    /// `--strict`: an ERC error stops the script (and `dc`, `tran`, `board import` refuse to run on a sheet with ERC errors).
    pub strict: bool,
    /// The latest analysis answers, which `spec` reads. Any edit of the schematic clears them.
    pub last_dc: Option<sim_core::DcResult>,
    pub last_tran: Option<sim_core::TranResult>,
    pub last_ac: Option<sim_core::AcResult>,
    /// `board postlayout on`: analyses add the board's copper resistance in series with the pads.
    pub post_layout: bool,
}

pub fn split_words(line: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Split `key=value` words from the positional ones.
pub fn options(words: &[String]) -> (Vec<String>, Vec<(String, String)>) {
    let mut pos = vec![];
    let mut kv = vec![];
    for w in words {
        match w.split_once('=') {
            Some((k, v)) if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphabetic() || c == '_') => kv.push((k.to_ascii_lowercase(), v.to_string())),
            _ => pos.push(w.clone()),
        }
    }
    (pos, kv)
}

pub fn opt<'a>(kv: &'a [(String, String)], key: &str) -> Option<&'a str> {
    kv.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

pub fn obj(v: Value) -> Out {
    match v {
        Value::Object(m) => m,
        _ => Out::new(),
    }
}

impl Session {
    pub fn new(lib_dir: Option<std::path::PathBuf>) -> Self {
        let dir = lib_dir.unwrap_or_else(part_lib::default_dir);
        // like the app: the bundled parts are copied to the library folder the first time
        let _ = part_lib::install_examples(&dir);
        let (lib, lib_errors) = part_lib::load_dir(&dir);
        Session { sch: Schematic::default(), lib, lib_dir: dir, board: None, cursor: (8, 8, 0), lib_errors, strict: false, last_dc: None, last_tran: None, last_ac: None, post_layout: false }
    }

    pub fn run(&mut self, w: &[String]) -> Res {
        let args = &w[1..];
        // an answer about the old circuit must not be judged against a new target
        if matches!(w[0].to_ascii_lowercase().as_str(), "new" | "open" | "add" | "value" | "net" | "remove" | "sheet") {
            (self.last_dc, self.last_tran, self.last_ac) = (None, None, None);
        }
        match w[0].to_ascii_lowercase().as_str() {
            "new" => {
                self.sch = Schematic::default();
                self.cursor = (8, 8, 0);
                Ok(obj(json!({})))
            }
            "open" => {
                let f = args.first().ok_or("open FILE.schjson")?;
                let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
                let mut v: Value = serde_json::from_str(&text).map_err(|e| format!("{f}: {e}"))?;
                // the app keeps its spec table in the same file; this tool does not use it
                if let Value::Object(m) = &mut v {
                    m.remove("specs");
                }
                self.sch = serde_json::from_value(v).map_err(|e| format!("{f}: {e}"))?;
                self.cursor = self.cursor_below();
                Ok(obj(json!({"parts": self.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).count()})))
            }
            "save" => {
                let f = args.first().ok_or("save FILE.schjson")?;
                let text = serde_json::to_string_pretty(&self.sch).map_err(|e| e.to_string())?;
                std::fs::write(f, text).map_err(|e| format!("{f}: {e}"))?;
                Ok(obj(json!({"file": f})))
            }
            "parts" => Ok(self.parts(args)),
            "part" if args.first().is_some_and(|a| a.eq_ignore_ascii_case("import")) => self.part_import(&args[1..]),
            "part" => self.part(args),
            "lib" => self.lib_cmd(args),
            "sheet" => self.sheet(args),
            "sweep" => self.sweep(args),
            "sens" => self.sens(args),
            "add" => self.add(args),
            "value" => self.set_value(args),
            "simonly" => self.sim_only(args),
            "pinnames" => self.pin_names(args),
            "net" => self.net(args),
            "nc" => self.nc(args),
            "remove" => self.remove(args),
            "list" => Ok(self.list()),
            "erc" => self.erc(),
            "coverage" => self.coverage(),
            "spice" => Ok(obj(json!({"text": sch_core::to_spice(&self.sch, "ccad")}))),
            "dc" => self.dc(args),
            "tran" => self.tran(args),
            "ac" => self.ac(args),
            "noise" => self.noise(args),
            "models" | "murata" => self.models(args),
            "calc" => self.calc(args),
            "spec" => self.spec(args),
            "req" => self.req(args),
            "board" => self.board_cmd(args),
            "help" => Ok(obj(json!({"text": HELP}))),
            other => Err(format!("unknown command '{other}' (try 'help')")),
        }
    }

    /// A cursor below everything already on the sheet (after `open`).
    pub(crate) fn cursor_below(&self) -> (i32, i32, i32) {
        let y = self.sch.components.iter().flat_map(|c| c.pins()).map(|(_, p)| p.1).max().unwrap_or(0);
        (8, y + 14, 0)
    }
}
