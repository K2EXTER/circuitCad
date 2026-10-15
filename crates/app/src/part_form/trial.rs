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
/// `.param`, `{expr}` and nested `X` calls are read (see `sim_core::spice_flat`), so they are not listed.
pub fn unsupported_notes(text: &str) -> Vec<String> {
    let mut notes: Vec<String> = vec![];
    let mut add = |s: String| {
        if !notes.contains(&s) {
            notes.push(s);
        }
    };
    for raw in text.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with(['*', ';', '+']) {
            continue;
        }
        let low = l.to_ascii_lowercase();
        if low.starts_with(".func") {
            add("「.func」(自作関数) は読み飛ばされます。関数を使う式は、式を直接書き下してください".into());
        } else if low.starts_with(".include") || low.starts_with(".lib") {
            add(format!("「{}」で別ファイルを読む書き方は使えません。そのファイルの中身も貼ってください", l.split_whitespace().next().unwrap_or("")));
        }
        // JFET / mutual inductor / transmission line / switches and the like: not read
        if !low.starts_with('.')
            && let Some(c @ ('K' | 'T' | 'W' | 'O' | 'U' | 'Z' | 'A')) = l.chars().next().map(|c| c.to_ascii_uppercase())
        {
            add(format!("素子 {c} (「{}」) は未対応です。読めるのは R C L V I E G F H D Q J M S B と、X (入れ子) です", l.split_whitespace().next().unwrap_or("")));
        }
    }
    notes
}

/// Vendor models that are encrypted (LTspice's "Encrypted File", PSpice's "$ENCRYPTED_LIB") cannot be read by anyone but
/// the simulator they were made for.
pub fn is_encrypted(text: &str) -> bool {
    let low = text.to_ascii_lowercase();
    low.contains("encrypted file") || low.contains("$encrypted_lib") || low.contains("encrypted_lib") || low.contains("begin:encrypted")
}

pub const ENCRYPTED_MSG: &str = "このモデルは暗号化されていて、ほかのシミュレータでは読めません (メーカーが中身を公開していないモデルです)。メーカーのサイトに、暗号化されていない版 (ngspice 用・LTspice 用の別ファイル) がないか探すか、データシートの数値から作れる部品 (かんたん作成) を使ってください";

impl PartForm {
    /// Read the model into a scratch circuit and try a no-power operating point.
    pub fn trial_read(&self) -> Trial {
        if self.sim == SimChoice::None || self.sim_text.trim().is_empty() {
            return Trial::Err("モデルの文が空です。SPICE モデルを貼ってから押してください".into());
        }
        if is_encrypted(&self.sim_text) {
            return Trial::Err(ENCRYPTED_MSG.into());
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
        let ignored = sim_core::ignored_model_params(&self.sim_text);
        if !ignored.is_empty() {
            let list: Vec<String> = ignored.iter().take(4).map(|(n, ps)| format!("{n}: {}", ps.iter().take(8).cloned().collect::<Vec<_>>().join(" "))).collect();
            warn.push(format!("素子モデルのパラメータのうち、使わないものがあります ({})。高注入・漏れ電流・ダイオードやMOSFETの容量などで、実物とのずれが出ます", list.join(" / ")));
        }
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
    fn nested_calls_and_params_are_read() {
        let t = ".param Rx=1k\n.subckt INNER p q\nR1 p q {Rx}\n.ends\n.subckt TOP a b\nX1 a b INNER\n.ends TOP";
        let f = form(t, SimChoice::Subckt, "TOP", "1 2");
        assert!(matches!(f.trial_read(), Trial::Ok(_)));
        assert!(unsupported_notes(t).is_empty());
    }

    #[test]
    fn a_missing_inner_subckt_is_explained() {
        let f = form(".subckt TOP a b\nX1 a b INNER\n.ends TOP", SimChoice::Subckt, "TOP", "1 2");
        let Trial::Err(m) = f.trial_read() else { panic!("expected an error") };
        assert!(m.contains("INNER") && m.contains("貼って"), "{m}");
    }

    #[test]
    fn unsupported_elements_and_functions_are_noticed() {
        let n = unsupported_notes(".func f(x) {x*2}
.subckt A 1 2
K1 L1 L2 0.9
G1 1 2 value={V(1)}
.ends");
        assert!(n.iter().any(|s| s.contains(".func")), "{n:?}");
        assert!(n.iter().any(|s| s.contains("K")), "{n:?}");
        // VALUE= is read now (as a behavioural source), so it is not listed
        assert!(!n.iter().any(|s| s.contains("VALUE")), "{n:?}");
    }

    #[test]
    fn encrypted_models_are_recognised_and_explained() {
        assert!(is_encrypted("* LTspice Encrypted File
* Begin:
 22 AB 91"));
        assert!(is_encrypted("* $ENCRYPTED_LIB
"));
        assert!(!is_encrypted(".subckt A 1 2
R1 1 2 1k
.ends"));
        let f = form("* LTspice Encrypted File
 22 AB", SimChoice::Subckt, "A", "1 2");
        assert!(matches!(f.trial_read(), Trial::Err(m) if m.contains("暗号化")));
        let mut f = PartForm { sim_text: "* LTspice Encrypted File
 22 AB".into(), ..PartForm::default() };
        assert!(f.autofill_from_spice().unwrap_err().contains("暗号化"));
    }

    #[test]
    fn empty_pin_list_is_explained() {
        let f = form(".subckt A 1 2
R1 1 2 1k
.ends", SimChoice::Subckt, "A", "");
        assert!(matches!(f.trial_read(), Trial::Err(m) if m.contains("ノード順")));
    }

    #[test]
    fn the_top_subckt_is_the_one_nobody_calls() {
        let t = ".subckt INNER p q\nR1 p q 1k\n.ends\n.subckt TOP a b c\nX1 a b INNER\n.ends TOP";
        let g = parse::guess_spice(t).unwrap();
        assert_eq!(g.name, "TOP");
        assert_eq!(g.roles.len(), 3);
    }
}
