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
  add TYPE [VALUE] [as=REF] [rot=0..3] [at=X,Y]
        TYPE: R C L D LED V NPN HEADER2 HEADER4 ... or a library part name (see 'parts')
  value REF VALUE                       change the value
  net NAME REF.PIN [REF.PIN ...]        connect pins by name; REF.PIN is the pin number or its name (any case)
                                        (GND pins of a part that share a name are all connected)
                                        The net named GND is the 0 V reference: every circuit needs one.
  remove REF
  list                                  parts, pins and the net of every pin
  erc                                   electrical rule check (message + hint). 'ok' only says the command ran:
                                        read 'errors' (or run with --strict, which fails the script on an ERC error)
  spice                                 the SPICE netlist text
 Simulation
  dc [full]                             operating point: node voltages, element currents
                                        (elements inside a library part's macro model are hidden unless 'full')
  tran TSTOP TSTEP [probe=V(out),I(R1)] [samples=6] [uic]
                                        starts from the DC operating point (a capacitor starts charged);
                                        add 'uic' to start from zero instead (step responses, oscillators)
 Design help
  calc [KEY NUMBER ...]                 value calculators (led, divider, rc, regulator, buck, boost, opamp, ...): no
                                        arguments lists them with their inputs; the answer has the value, steps, cautions
  ac FSTART FSTOP [ppd=10] [probe=V(out)]
                                        frequency response (needs a source with an AC magnitude, e.g. value V1 \"0 AC 1\"):
                                        gain and phase per decade, -3 dB frequency, phase margin
  spec KIND TARGET LO HI [at=Hz]        pass / fail of the last dc / tran / ac answer against a range ('-' = no limit).
                                        'spec' alone lists the kinds (dc_voltage, tran_pp, ac_cutoff, ...)
                                        TARGET: dc_voltage = net name (MID); dc_current = part (D1, amps: 8m 12m);
                                        tran_* / ac_* = signal name as the answer shows it (V(OUT)); at= takes 1k, 10k ...
 Board
  board new [W H]                       empty board (mm), default 60 40
  board import                          parts and nets from the schematic (creates the board if needed)
  board autoplace
  board route [net=NAME]
  board drc
  board status
  board save FILE.pcbjson / board open FILE.pcbjson
  board fab DIR                         Gerber, drill, BOM, placement and the zip
";

pub struct Session {
    pub sch: Schematic,
    pub lib: Vec<PartDef>,
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
            Some((k, v)) if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphabetic()) => kv.push((k.to_ascii_lowercase(), v.to_string())),
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
        Session { sch: Schematic::default(), lib, board: None, cursor: (8, 8, 0), lib_errors, strict: false, last_dc: None, last_tran: None, last_ac: None }
    }

    pub fn run(&mut self, w: &[String]) -> Res {
        let args = &w[1..];
        // an answer about the old circuit must not be judged against a new target
        if matches!(w[0].to_ascii_lowercase().as_str(), "new" | "open" | "add" | "value" | "net" | "remove") {
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
            "part" => self.part(args),
            "add" => self.add(args),
            "value" => self.set_value(args),
            "net" => self.net(args),
            "remove" => self.remove(args),
            "list" => Ok(self.list()),
            "erc" => self.erc(),
            "spice" => Ok(obj(json!({"text": sch_core::to_spice(&self.sch, "ccad")}))),
            "dc" => self.dc(args),
            "tran" => self.tran(args),
            "ac" => self.ac(args),
            "calc" => self.calc(args),
            "spec" => self.spec(args),
            "board" => self.board_cmd(args),
            "help" => Ok(obj(json!({"text": HELP}))),
            other => Err(format!("unknown command '{other}' (try 'help')")),
        }
    }

    /// A cursor below everything already on the sheet (after `open`).
    fn cursor_below(&self) -> (i32, i32, i32) {
        let y = self.sch.components.iter().flat_map(|c| c.pins()).map(|(_, p)| p.1).max().unwrap_or(0);
        (8, y + 14, 0)
    }
}
