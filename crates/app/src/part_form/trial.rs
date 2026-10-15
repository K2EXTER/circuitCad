//! "Trial read" of the pasted SPICE model: put it into a scratch circuit and say, in plain words,
//! what the simulator cannot read. Saves the learner from finding out only after placing the part.

use super::*;
use sim_core::{add_model_instance, add_subckt_instance, dc_op, Circuit, Element};

/// How the trial read went.
#[derive(Clone, Debug)]
pub enum Trial {
    Ok(String),
    /// Read fine, but something is worth a look.
    Warn(String),
    Err(String),
}

/// Plain-language notes on constructs in `text` that the simulator does not read (empty = none found).
pub fn unsupported_notes(text: &str) -> Vec<String> {
    let mut notes: Vec<String> = vec![];
    let mut add = |s: String| {
        if !notes.contains(&s) {
            notes.push(s);
        }
    };
    let mut subckts = 0;
    for raw in text.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with(['*', ';', '+']) {
            continue;
        }
        let low = l.to_ascii_lowercase();
        if low.starts_with(".subckt") {
            subckts += 1;
        } else if low.starts_with(".param") || low.starts_with(".func") {
            add(format!("「{}」は読み飛ばされます。この名前を使う値 ({{…}}) は数値にできません。数値に書き換えてください", l.split_whitespace().next().unwrap_or("")));
        } else if low.starts_with(".include") || low.starts_with(".lib") {
            add(format!("「{}」で別ファイルを読む書き方は使えません。そのファイルの中身も貼ってください", l.split_whitespace().next().unwrap_or("")));
        }
        if l.contains('{') && !low.starts_with('.') {
            add("値に {式} や {名前} が使われています。数値に書き換えてください".into());
        }
        if low.contains("value=") || low.contains("table(") || low.contains(" table ") {
            add("VALUE= や TABLE を使う行があります。V=式 / I=式 の B 素子に書き換えてください".into());
        }
        if !low.starts_with('.') {
            match l.chars().next().map(|c| c.to_ascii_uppercase()) {
                Some('X') => add(format!("「{}」はサブサーキットの呼び出し (入れ子) です。未対応です。中身の素子を直接書くか、その部分を取り除いてください", l.split_whitespace().next().unwrap_or(""))),
                Some(c @ ('J' | 'K' | 'T' | 'F' | 'H' | 'W' | 'O' | 'U' | 'Z')) => add(format!("素子 {c} (「{}」) は未対応です。読めるのは R C L V I E G D Q M S B です", l.split_whitespace().next().unwrap_or(""))),
                _ => {}
            }
        }
    }
    if subckts > 1 {
        add(format!(".subckt が {subckts} 個あります。使うのは貼った文の最初の 1 つだけです。使いたい 1 つだけを残してください"));
    }
    notes
}

impl PartForm {
    /// Read the model into a scratch circuit and try a no-power operating point.
    pub fn trial_read(&self) -> Trial {
        if self.sim == SimChoice::None || self.sim_text.trim().is_empty() {
            return Trial::Err("モデルの文が空です。SPICE モデルを貼ってから押してください".into());
        }
        let ports: Vec<&str> = self.sim_pins.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
        if ports.is_empty() {
            return Trial::Err("「ノード順のピン番号」が空です。モデルの端子の数だけ、ピン番号を並べてください (「貼ったモデルから自動入力」で入ります)".into());
        }
        let mut c = Circuit::new();
        let nodes: Vec<usize> = ports.iter().map(|p| c.node(&format!("pin{p}"))).collect();
        let read = if self.sim == SimChoice::Model {
            add_model_instance(&mut c, "U1", self.sim_element.trim(), self.sim_name.trim(), &self.sim_text, &nodes)
        } else {
            add_subckt_instance(&mut c, "U1", self.sim_name.trim(), &self.sim_text, &nodes)
        };
        let notes = unsupported_notes(&self.sim_text);
        if let Err(e) = read {
            let mut m = e;
            if !notes.is_empty() {
                m += &format!("\n心当たり: {}", notes.join(" / "));
            }
            return Trial::Err(format!("このモデルは読めませんでした: {m}"));
        }
        for (i, &n) in nodes.iter().enumerate() {
            c.add(Element::Resistor { name: format!("Rtrial{i}"), a: n, b: 0, r: 1e6 });
        }
        let dc = dc_op(&c);
        let mut warn = notes;
        if dc.is_err() {
            warn.push("電源をつながない状態の動作点が求まりませんでした。電源が必要なモデルでは普通ですが、回路に置いたら動作点を計算して確かめてください".into());
        }
        if warn.is_empty() {
            Trial::Ok(format!("読めました。端子 {} 本、電源なしの動作点まで計算できました。回路に置いて、電源をつないで動きを確かめましょう", nodes.len()))
        } else {
            Trial::Warn(format!("読めましたが、気になる点があります: {}", warn.join(" / ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(text: &str, sim: SimChoice, name: &str, pins: &str) -> PartForm {
        PartForm { sim, sim_text: text.into(), sim_name: name.into(), sim_pins: pins.into(), ..PartForm::default() }
    }

    #[test]
    fn a_plain_subckt_reads_and_solves() {
        let f = form(".subckt AMP in out gnd
R1 in out 1k
R2 out gnd 1k
.ends AMP", SimChoice::Subckt, "AMP", "1 2 3");
        assert!(matches!(f.trial_read(), Trial::Ok(_)));
    }

    #[test]
    fn a_diode_model_reads() {
        let mut f = form(".model D1 D(IS=1n)", SimChoice::Model, "D1", "1 2");
        f.sim_element = "D".into();
        assert!(matches!(f.trial_read(), Trial::Ok(_)));
    }

    #[test]
    fn nested_subckt_calls_fail_with_a_reason() {
        let f = form(".subckt TOP a b
X1 a b INNER
.ends TOP", SimChoice::Subckt, "TOP", "1 2");
        let Trial::Err(m) = f.trial_read() else { panic!("expected an error") };
        assert!(m.contains("入れ子"), "{m}");
    }

    #[test]
    fn params_and_several_subckts_are_noticed() {
        let t = ".param Rx=1k
.subckt A 1 2
R1 1 2 {Rx}
.ends
.subckt B 1 2
R1 1 2 1k
.ends";
        let n = unsupported_notes(t);
        assert!(n.iter().any(|s| s.contains(".param")), "{n:?}");
        assert!(n.iter().any(|s| s.contains("{式}")), "{n:?}");
        assert!(n.iter().any(|s| s.contains("2 個")), "{n:?}");
    }

    #[test]
    fn empty_pin_list_is_explained() {
        let f = form(".subckt A 1 2
R1 1 2 1k
.ends", SimChoice::Subckt, "A", "");
        assert!(matches!(f.trial_read(), Trial::Err(m) if m.contains("ノード順")));
    }
}
