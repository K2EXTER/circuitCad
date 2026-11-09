//! Board commands that edit the silkscreen by hand: text (free texts), label (a part's designator), silkline (the width of a silk line).
//! Every size stays within what the fab prints (`pcb_core::silkedit`).

use super::*;
use pcb_core::model::{Point, Side};
use pcb_core::silkedit;

fn side_of(v: Option<&str>) -> Result<Option<Side>, String> {
    match v {
        None => Ok(None),
        Some("top") => Ok(Some(Side::Top)),
        Some("bottom") => Ok(Some(Side::Bottom)),
        Some(o) => Err(format!("side={o}: top か bottom")),
    }
}

fn rot_of(v: Option<&str>) -> Result<Option<u8>, String> {
    v.map(|r| match r.trim_end_matches("deg") {
        "0" => Ok(0),
        "1" | "90" => Ok(1),
        "2" | "180" => Ok(2),
        "3" | "270" => Ok(3),
        o => Err(format!("rot={o}: 0..3 (90° ずつ) か 0/90/180/270")),
    })
    .transpose()
}

fn num_of(v: Option<&str>) -> Result<Option<f64>, String> {
    v.map(|x| x.trim().parse::<f64>().map_err(|_| format!("{x} is not a number"))).transpose()
}

impl Session {
    pub(super) fn board_text(&mut self, rest: &[String]) -> Res {
        // board text add "TEXT" X Y [size=1.0] [rot=0..3] [side=top|bottom] / text set N [text="..."] [size=] [rot=] [side=] [x= y=] /
        // text remove N / text list: free texts on the silk (a board name, a version, a note). The sizes stay within the fab's limits
        // (`board rules`); the font prints letters, digits and some symbols. `list` gives every text with its index (`part` is set for a
        // designator, `pin` for a pin name of `board pinlabels`; those two are not free texts)
        let (words, kv) = options(rest);
        let sub = words.first().map(String::as_str).unwrap_or("list");
        let board = self.board_mut()?;
        let size = num_of(opt(&kv, "size"))?;
        let rot = rot_of(opt(&kv, "rot"))?;
        let side = side_of(opt(&kv, "side"))?;
        let index = |w: Option<&String>| -> Result<usize, String> { w.ok_or("text set|remove N (N: the index of `board text list`)")?.parse::<usize>().map_err(|_| "N is the index of `board text list`".to_string()) };
        match sub {
            "add" => {
                let text = words.get(1).ok_or("board text add \"TEXT\" X Y [size=1.0] [rot=0] [side=top]")?;
                let x = num_of(words.get(2).map(String::as_str))?.ok_or("board text add \"TEXT\" X Y: the place (mm)")?;
                let y = num_of(words.get(3).map(String::as_str))?.ok_or("board text add \"TEXT\" X Y: the place (mm)")?;
                let h = size.unwrap_or(1.0f64.max(board.rules.min_silk_text));
                let i = silkedit::add_text(board, text, Point::new(x, y), h, rot.unwrap_or(0), side.unwrap_or(Side::Top))?;
                Ok(obj(json!({"index": i, "text": board.texts[i].text, "size": h})))
            }
            "set" => {
                let i = index(words.get(1))?;
                let pos = match (num_of(opt(&kv, "x"))?, num_of(opt(&kv, "y"))?) {
                    (None, None) => None,
                    (x, y) => {
                        let at = board.texts.get(i).map(|t| t.pos).unwrap_or_default();
                        Some(Point::new(x.unwrap_or(at.x), y.unwrap_or(at.y)))
                    }
                };
                let c = silkedit::TextChange { text: opt(&kv, "text").map(str::to_string), height: size, rot, side, pos };
                silkedit::set_text(board, i, &c)?;
                let t = &board.texts[i];
                Ok(obj(json!({"index": i, "text": t.text, "size": t.height, "rot": t.rot, "x": t.pos.x, "y": t.pos.y})))
            }
            "remove" => {
                let i = index(words.get(1))?;
                silkedit::remove_text(board, i)?;
                Ok(obj(json!({"removed": i})))
            }
            "list" => {
                let list: Vec<Value> = board
                    .texts
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let mut v = json!({"index": i, "text": t.text, "x": t.pos.x, "y": t.pos.y, "size": t.height, "rot": t.rot, "side": if t.side == Side::Bottom { "bottom" } else { "top" }});
                        if let Some(p) = t.refdes.strip_suffix("#pin") {
                            v["pin"] = json!(p);
                        } else if !t.refdes.is_empty() {
                            v["part"] = json!(t.refdes);
                        }
                        v
                    })
                    .collect();
                Ok(obj(json!({"texts": list, "min_size": board.rules.min_silk_text})))
            }
            o => Err(format!("board text {o}: add / set / remove / list")),
        }
    }

    pub(super) fn board_label(&mut self, rest: &[String]) -> Res {
        // board label R1 [size=1.2] [rot=0..3] [hide=1|0] / board label all size=1.2: a part's designator on the silk: its height, its
        // turn, or not printed (`hide=1`); `all` sets the height of every designator. The height stays within the fab's limit
        let (words, kv) = options(rest);
        let which = words.first().ok_or("board label R1 [size=] [rot=] [hide=1|0] / board label all size=")?.clone();
        let size = num_of(opt(&kv, "size"))?;
        let rot = rot_of(opt(&kv, "rot"))?;
        let hide = match opt(&kv, "hide") {
            None => None,
            Some("1") => Some(true),
            Some("0") => Some(false),
            Some(o) => return Err(format!("hide={o}: 1 か 0")),
        };
        let board = self.board_mut()?;
        if which == "all" {
            let h = size.ok_or("board label all size=1.2")?;
            let n = silkedit::set_all_labels(board, h)?;
            return Ok(obj(json!({"changed": n, "size": h})));
        }
        silkedit::set_label(board, &which, size, rot, hide)?;
        let t = board.texts.iter().find(|t| t.refdes == which);
        Ok(obj(json!({"part": which, "hidden": board.hidden_labels.contains(&which), "size": t.map(|t| t.height), "rot": t.map(|t| t.rot)})))
    }

    pub(super) fn board_silkline(&mut self, rest: &[String]) -> Res {
        // board silkline add X1 Y1 X2 Y2 [width=0.15] [side=top|bottom] / silkline width N W / silkline list: silk lines that belong to no
        // part (a frame, a mark); the width stays within the fab's thinnest line
        let (words, kv) = options(rest);
        let sub = words.first().map(String::as_str).unwrap_or("list");
        let board = self.board_mut()?;
        match sub {
            "add" => {
                let v: Vec<f64> = words[1..].iter().take(4).map(|w| w.parse::<f64>().map_err(|_| format!("{w} is not a number"))).collect::<Result<_, _>>()?;
                if v.len() < 4 {
                    return Err("board silkline add X1 Y1 X2 Y2 [width=0.15] [side=top]".into());
                }
                let w = num_of(opt(&kv, "width"))?.unwrap_or(0.15f64.max(board.rules.min_silk_width));
                silkedit::check_width(board, w)?;
                let side = side_of(opt(&kv, "side"))?.unwrap_or(Side::Top);
                board.silk.push(pcb_core::model::SilkLine { side, a: Point::new(v[0], v[1]), b: Point::new(v[2], v[3]), width: w, refdes: String::new() });
                Ok(obj(json!({"index": board.silk.len() - 1, "width": w})))
            }
            "width" => {
                let i = words.get(1).and_then(|w| w.parse::<usize>().ok()).ok_or("board silkline width N W (N: the index of `board silkline list`)")?;
                let w = num_of(words.get(2).map(String::as_str))?.ok_or("board silkline width N W")?;
                silkedit::set_line_width(board, i, w)?;
                Ok(obj(json!({"index": i, "width": w})))
            }
            "list" => {
                let list: Vec<Value> = board.silk.iter().enumerate().filter(|(_, l)| l.refdes.is_empty()).map(|(i, l)| json!({"index": i, "a": [l.a.x, l.a.y], "b": [l.b.x, l.b.y], "width": l.width})).collect();
                Ok(obj(json!({"lines": list, "min_width": board.rules.min_silk_width})))
            }
            o => Err(format!("board silkline {o}: add / width / list")),
        }
    }
}
