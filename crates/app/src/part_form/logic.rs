//! "かんたん作成" for a logic IC: input pins and one Boolean expression per output (`Y = A & !B`). The truth table is worked
//! out here and becomes one table cell (`Y ... lut ... table=`) per output, with a propagation delay; the pins get VCC and GND.

use super::*;

/// What is typed into the logic box.
#[derive(Clone, Debug, PartialEq)]
pub struct LogicInput {
    /// Input pin names, separated by spaces or commas.
    pub inputs: String,
    /// One output per line: `NAME = expression`.
    pub outputs: String,
    /// Propagation delay (ns, rising and falling alike).
    pub delay_ns: f64,
    pub message: String,
}

impl Default for LogicInput {
    fn default() -> Self {
        Self { inputs: "A B".into(), outputs: "Y = A & B".into(), delay_ns: 10.0, message: String::new() }
    }
}

/// An expression: names, `0` / `1`, `!` (not), `&` (and), `^` (xor), `|` (or), parentheses; the words `not and xor or` too.
#[derive(Clone, Debug, PartialEq)]
enum Ex {
    Var(usize),
    Const(bool),
    Not(Box<Ex>),
    Bin(char, Box<Ex>, Box<Ex>),
}

impl Ex {
    fn eval(&self, v: &[bool]) -> bool {
        match self {
            Ex::Var(i) => v[*i],
            Ex::Const(b) => *b,
            Ex::Not(e) => !e.eval(v),
            Ex::Bin('&', a, b) => a.eval(v) && b.eval(v),
            Ex::Bin('^', a, b) => a.eval(v) != b.eval(v),
            Ex::Bin(_, a, b) => a.eval(v) || b.eval(v),
        }
    }

    fn vars(&self, out: &mut Vec<usize>) {
        match self {
            Ex::Var(i) => {
                if !out.contains(i) {
                    out.push(*i);
                }
            }
            Ex::Const(_) => {}
            Ex::Not(e) => e.vars(out),
            Ex::Bin(_, a, b) => {
                a.vars(out);
                b.vars(out);
            }
        }
    }
}

/// Words of an expression: names, single-character operators and parentheses.
fn tokens(s: &str) -> Result<Vec<String>, String> {
    let mut out = vec![];
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() || c == '_' {
            cur.push(c);
            continue;
        }
        if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        match c {
            ' ' | '\t' => {}
            '!' | '~' | '&' | '*' | '|' | '+' | '^' | '(' | ')' => out.push(c.to_string()),
            other => return Err(i18n::trf!("式に使えない文字「{}」があります。使えるのは 名前・0・1・! (否定)・& (かつ)・| (または)・^ (排他的論理和)・括弧 です", other)),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    // the words as operators
    Ok(out
        .into_iter()
        .map(|t| match t.to_ascii_lowercase().as_str() {
            "not" | "~" => "!".into(),
            "and" | "*" => "&".into(),
            "or" | "+" => "|".into(),
            "xor" => "^".into(),
            _ => t,
        })
        .collect())
}

struct Parser<'a> {
    t: Vec<String>,
    pos: usize,
    names: &'a [String],
}

impl Parser<'_> {
    fn peek(&self) -> Option<&str> {
        self.t.get(self.pos).map(|s| s.as_str())
    }

    /// or < xor < and < not
    fn expr(&mut self, level: u8) -> Result<Ex, String> {
        if level == 3 {
            return self.unary();
        }
        let op = ['|', '^', '&'][level as usize];
        let mut left = self.expr(level + 1)?;
        while self.peek() == Some(op.to_string().as_str()) {
            self.pos += 1;
            let right = self.expr(level + 1)?;
            left = Ex::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Ex, String> {
        match self.peek() {
            Some("!") => {
                self.pos += 1;
                Ok(Ex::Not(Box::new(self.unary()?)))
            }
            Some("(") => {
                self.pos += 1;
                let e = self.expr(0)?;
                if self.peek() != Some(")") {
                    return Err(i18n::tr("括弧が閉じていません").to_string());
                }
                self.pos += 1;
                Ok(e)
            }
            Some("0") => {
                self.pos += 1;
                Ok(Ex::Const(false))
            }
            Some("1") => {
                self.pos += 1;
                Ok(Ex::Const(true))
            }
            Some(n) if n.chars().all(|c| c.is_alphanumeric() || c == '_') => {
                let i = self.names.iter().position(|x| x.eq_ignore_ascii_case(n)).ok_or_else(|| i18n::trf!("「{}」は入力ピンにありません。入力ピンの欄に足すか、名前を直してください", n))?;
                self.pos += 1;
                Ok(Ex::Var(i))
            }
            Some(other) => Err(i18n::trf!("「{}」の前に、名前か 0・1 か「(」が要ります", other)),
            None => Err(i18n::tr("式が途中で終わっています").to_string()),
        }
    }
}

fn parse(expr: &str, names: &[String]) -> Result<Ex, String> {
    let mut p = Parser { t: tokens(expr)?, pos: 0, names };
    if p.t.is_empty() {
        return Err(i18n::tr("式が空です").to_string());
    }
    let e = p.expr(0)?;
    if let Some(rest) = p.peek() {
        return Err(i18n::trf!("「{}」から後ろが読めません (演算子が抜けていませんか)", rest));
    }
    Ok(e)
}

/// The inputs and the outputs with their expressions, checked.
#[derive(Debug)]
pub(super) struct Logic {
    pub inputs: Vec<String>,
    /// (name, expression, the inputs it reads by index, first = the table's lowest address bit)
    outputs: Vec<(String, Ex, Vec<usize>)>,
}

/// The most inputs one output may read (a table of 2^12 rows).
const MAX_INPUTS: usize = 12;

impl LogicInput {
    pub(super) fn read(&self) -> Result<Logic, String> {
        let inputs: Vec<String> = self.inputs.split([' ', ',', '\t']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
        if inputs.is_empty() {
            return Err(i18n::tr("入力ピンの名前を 1 つ以上入れてください (例: A B)").to_string());
        }
        let bad = |n: &str| !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || n.chars().all(|c| c.is_ascii_digit());
        let mut outputs = vec![];
        for line in self.outputs.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (name, expr) = line.split_once('=').ok_or_else(|| i18n::trf!("「{}」に「=」がありません。出力は 1 行に 1 つ、「Y = A & B」の形で書きます", line))?;
            let name = name.trim().to_string();
            let ex = parse(expr, &inputs).map_err(|e| i18n::trf!("{} の式: {}", name, e))?;
            let mut vars = vec![];
            ex.vars(&mut vars);
            vars.sort_unstable();
            if vars.len() > MAX_INPUTS {
                return Err(i18n::trf!("{} は入力を {} 本読んでいます。1 つの出力が読めるのは {} 本までです", name, vars.len(), MAX_INPUTS));
            }
            outputs.push((name, ex, vars));
        }
        if outputs.is_empty() {
            return Err(i18n::tr("出力を 1 行に 1 つ、「Y = A & B」の形で書いてください").to_string());
        }
        let mut all: Vec<&String> = inputs.iter().chain(outputs.iter().map(|o| &o.0)).collect();
        if let Some(n) = all.iter().find(|n| bad(n)) {
            return Err(i18n::trf!("ピンの名前「{}」は、英数字と _ だけで、数字だけでない名前にしてください", n));
        }
        all.sort_by_key(|n| n.to_ascii_uppercase());
        if let Some(w) = all.windows(2).find(|w| w[0].eq_ignore_ascii_case(w[1])) {
            return Err(i18n::trf!("ピンの名前「{}」が 2 回あります", w[0]));
        }
        if let Some(n) = all.iter().find(|n| ["VCC", "GND"].contains(&n.to_ascii_uppercase().as_str())) {
            return Err(i18n::trf!("「{}」は電源のピンとして自動で付くので、入力・出力には使えません", n));
        }
        Ok(Logic { inputs, outputs })
    }
}

impl Logic {
    /// The truth table: every combination of the inputs (the first input is the lowest bit) and the outputs.
    pub(super) fn table(&self) -> Vec<(Vec<bool>, Vec<bool>)> {
        let n = self.inputs.len();
        (0..1usize << n)
            .map(|a| {
                let v: Vec<bool> = (0..n).map(|k| a >> k & 1 == 1).collect();
                let o = self.outputs.iter().map(|(_, e, _)| e.eval(&v)).collect();
                (v, o)
            })
            .collect()
    }

    /// The model: one table cell per output, reading only the inputs its expression uses.
    pub(super) fn subckt(&self, name: &str, delay_ns: f64) -> String {
        let low = |s: &str| s.to_ascii_lowercase();
        let pins: Vec<String> = self.inputs.iter().map(|s| low(s)).chain(self.outputs.iter().map(|o| low(&o.0))).collect();
        let mut l = vec![
            format!("* {name}: made from Boolean expressions in the part form (one table cell per output, delay {delay_ns} ns)"),
            format!(".subckt {name} {} gnd vcc", pins.join(" ")),
        ];
        for (out, ex, vars) in &self.outputs {
            let o = low(out);
            l.push(format!("* {out}"));
            if vars.is_empty() {
                // a constant: a buffer from the supply or from ground
                let src = if ex.eval(&vec![false; self.inputs.len()]) { "vcc" } else { "gnd" };
                l.push(format!("Y{o} buf {o} 0 gnd vcc 60 0.5 0 0 0 {src}"));
                continue;
            }
            let words: Vec<&str> = (0..1usize << vars.len())
                .map(|a| {
                    let mut v = vec![false; self.inputs.len()];
                    for (k, i) in vars.iter().enumerate() {
                        v[*i] = a >> k & 1 == 1;
                    }
                    if ex.eval(&v) { "1" } else { "0" }
                })
                .collect();
            let ins: Vec<String> = vars.iter().map(|i| low(&self.inputs[*i])).collect();
            l.push(format!("Y{o} lut {o} 0 gnd vcc 60 0.5 0 0 0 td={delay_ns}n table={} {}", words.join(","), ins.join(" ")));
        }
        l.push(format!(".ends {name}"));
        l.join("\n")
    }
}

impl PartForm {
    /// Fill the pins, the model and the description from the logic box.
    pub(super) fn apply_logic(&mut self) {
        let name = self.name.trim().replace(' ', "_");
        if name.is_empty() {
            self.logic.message = i18n::tr("先に、上の「部品名」を入れてください (モデルの名前になります)").to_string();
            return;
        }
        let g = match self.logic.read() {
            Ok(g) => g,
            Err(e) => {
                self.logic.message = e;
                return;
            }
        };
        let mut pins: Vec<String> = vec![];
        for n in &g.inputs {
            pins.push(format!("{} {n} left", pins.len() + 1));
        }
        for (n, _, _) in &g.outputs {
            pins.push(format!("{} {n} right", pins.len() + 1));
        }
        let gnd = pins.len() + 1;
        pins.push(format!("{gnd} GND bottom"));
        pins.push(format!("{} VCC top", gnd + 1));
        self.pins_text = pins.join("\n");
        self.sim = SimChoice::Subckt;
        self.sim_name = name.clone();
        self.sim_text = g.subckt(&name, self.logic.delay_ns);
        // the subcircuit's nodes are the pins in their own order: inputs, outputs, gnd, vcc
        self.sim_pins = (1..=gnd + 1).map(|k| k.to_string()).collect::<Vec<_>>().join(" ");
        let exprs: Vec<String> = self.logic.outputs.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_string).collect();
        self.description = i18n::trf!("論理式から作ったロジック IC ({})。電源は VCC と GND で、しきい値は電源の半分です", exprs.join("、"));
        self.sim_limits = [
            i18n::tr("論理式から作った動作モデルです (真理値表のセル、出力抵抗 60Ω、しきい値は電源の半分)。実在の IC の入力電流・消費電流・出力の電流の限界は表しません。").to_string(),
            i18n::trf!("遅延は {} ns の一定値で、それより短いパルスは出力に出ません。立ち上がり・立ち下がりの違い、電源電圧や負荷による遅延の変化は表しません。", self.logic.delay_ns),
        ]
        .join("\n");
        self.sim_origin = Some((sch_core::ModelOrigin::new(sch_core::OriginKind::Generic, ""), self.sim_text.clone()));
        if self.category.trim().is_empty() || self.category == "ユーザー" {
            self.category = "ロジックIC".into();
        }
        self.logic.message = i18n::trf!("ピン {} 本とモデルを作りました。このあと、下の「パッケージ」を選び、ピン番号をデータシートの番号に合わせて保存します", pins.len());
    }

    /// The half adder in the box, made into pins and a model (the screenshot of `--form-logic-example`).
    pub fn logic_example(&mut self) {
        if self.name.trim().is_empty() {
            self.name = "HALF_ADDER".into();
        }
        self.logic.inputs = "A B".into();
        self.logic.outputs = "S = A ^ B
C = A & B".into();
        self.apply_logic();
    }

    pub(super) fn logic_ui(&mut self, ui: &mut egui::Ui) {
        // open once something was made (so the result shows), closed at first like the other templates
        let open = !self.logic.message.is_empty();
        egui::CollapsingHeader::new(i18n::tr("論理式からピンとモデルを作る (ロジック IC)")).id_salt("tpl_logic").default_open(open).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(i18n::tr("例から始める:"));
                let examples = [
                    ("半加算器", "1 ビットの足し算: S = A と B の排他的論理和、C = 桁上がり", "A B", "S = A ^ B\nC = A & B"),
                    ("多数決 (3 入力)", "3 つのうち 2 つ以上が 1 なら 1", "A B C", "Y = (A & B) | (B & C) | (A & C)"),
                    ("2 → 1 セレクタ", "S が 0 なら A、1 なら B を出す", "A B S", "Y = (A & !S) | (B & S)"),
                ];
                for (label, tip, ins, outs) in examples {
                    if ui.small_button(i18n::tr(label)).on_hover_text(i18n::tr(tip)).clicked() {
                        self.logic.inputs = ins.into();
                        self.logic.outputs = outs.into();
                    }
                }
            });
            egui::Grid::new("logic_grid").num_columns(2).show(ui, |ui| {
                ui.label(i18n::tr("入力ピン"));
                ui.add(egui::TextEdit::singleline(&mut self.logic.inputs).desired_width(260.0))
                    .on_hover_text(i18n::tr("入力ピンの名前を、空白かカンマで区切って並べます (例: A B C)。VCC と GND は自動で付きます"));
                ui.end_row();
                ui.label(i18n::tr("出力 (1 行に 1 つ)"));
                ui.add(egui::TextEdit::multiline(&mut self.logic.outputs).desired_rows(3).desired_width(260.0).font(egui::TextStyle::Monospace))
                    .on_hover_text(i18n::tr("「出力の名前 = 式」を 1 行に 1 つ。式に使えるのは 入力ピンの名前・0・1・! (否定)・& (かつ)・^ (排他的論理和)・| (または)・括弧。not・and・xor・or と書いてもかまいません。強さの順は ! > & > ^ > |"));
                ui.end_row();
                ui.label(i18n::tr("遅延"));
                ui.add(egui::DragValue::new(&mut self.logic.delay_ns).speed(0.5).range(0.0..=10000.0).suffix(" ns"))
                    .on_hover_text(i18n::tr("入力が変わってから出力が変わるまでの時間。74HC なら 10ns 前後 (データシートの tpd)。これより短いパルスは出力に出ません"));
                ui.end_row();
            });
            // the truth table as it is typed (up to 5 inputs: 32 rows)
            match self.logic.read() {
                Ok(g) if g.inputs.len() <= 5 => {
                    ui.label(egui::RichText::new(i18n::tr("真理値表")).small().weak());
                    egui::Grid::new("logic_table").striped(true).spacing([10.0, 0.0]).show(ui, |ui| {
                        for n in g.inputs.iter().chain(g.outputs.iter().map(|o| &o.0)) {
                            ui.label(egui::RichText::new(n).monospace().strong());
                        }
                        ui.end_row();
                        for (ins, outs) in g.table() {
                            for b in ins {
                                ui.label(egui::RichText::new(if b { "1" } else { "0" }).monospace());
                            }
                            for b in outs {
                                ui.label(egui::RichText::new(if b { "1" } else { "0" }).monospace().color(crate::theme::INFO));
                            }
                            ui.end_row();
                        }
                    });
                }
                Ok(g) => {
                    ui.small(i18n::trf!("入力が {} 本あるので、真理値表 ({} 行) は出しません", g.inputs.len(), 1usize << g.inputs.len()));
                }
                Err(e) => {
                    ui.colored_label(crate::theme::WARN, e);
                }
            }
            ui.horizontal(|ui| {
                if ui
                    .button(i18n::tr("ピンとモデルを作る"))
                    .on_hover_text(i18n::tr("入力・出力・VCC・GND のピンの一覧と、真理値表のシミュレーションモデルを作って、下の欄に書き込みます (すでにある内容は置き換わります)"))
                    .clicked()
                {
                    self.apply_logic();
                }
                if !self.logic.message.is_empty() {
                    ui.small(i18n::tr_dyn(&self.logic.message));
                }
            });
        })
        .header_response
        .on_hover_text(i18n::tr("入力と、出力ごとの論理式を書くだけで、ロジック IC (ゲートの組み合わせ) の部品を作ります。真理値表が自動で出るので、式が合っているか確かめられます"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logic(ins: &str, outs: &str) -> Result<Logic, String> {
        LogicInput { inputs: ins.into(), outputs: outs.into(), ..Default::default() }.read()
    }

    #[test]
    fn expressions_follow_the_usual_precedence_and_the_words_work_too() {
        let g = logic("A B C", "Y = A | B & C\nZ = not A and (B or C)\nX = A ^ B ^ C").unwrap();
        for (ins, outs) in g.table() {
            let (a, b, c) = (ins[0], ins[1], ins[2]);
            assert_eq!(outs, vec![a || (b && c), !a && (b || c), a ^ b ^ c], "{ins:?}");
        }
    }

    #[test]
    fn mistakes_say_what_and_where() {
        assert!(logic("A B", "Y = A & D").unwrap_err().contains("D"));
        assert!(logic("A B", "Y = A & (B").unwrap_err().contains("括弧"));
        assert!(logic("A B", "Y A & B").unwrap_err().contains("="));
        assert!(logic("A B", "Y = A B").unwrap_err().contains("B"));
        assert!(logic("A VCC", "Y = A").unwrap_err().contains("VCC"));
        assert!(logic("A B", "A = B").unwrap_err().contains("2 回"));
        assert!(logic("A B", "Y = A % B").unwrap_err().contains("%"));
    }

    #[test]
    fn the_half_adder_makes_a_part_that_adds_in_the_simulator() {
        let mut f = PartForm { name: "HALFADD".into(), ..Default::default() };
        f.logic.inputs = "A B".into();
        f.logic.outputs = "S = A ^ B\nC = A & B".into();
        f.apply_logic();
        assert!(f.logic.message.contains("ピン 6 本"), "{}", f.logic.message);
        assert!(f.sim_text.contains("Ys lut s 0 gnd vcc 60 0.5 0 0 0 td=10n table=0,1,1,0 a b"), "{}", f.sim_text);
        assert!(f.sim_text.contains("table=0,0,0,1 a b"), "{}", f.sim_text);
        // the model runs: A = 1, B = 1 gives S = 0, C = 1
        let mut c = sim_core::Circuit::new();
        let n: Vec<usize> = ["a", "b", "s", "c", "vcc"].iter().map(|x| c.node(x)).collect();
        sim_core::add_subckt_instance(&mut c, "U1", "HALFADD", &f.sim_text, &[n[0], n[1], n[2], n[3], 0, n[4]]).unwrap();
        for (k, name) in [(4, "Vcc"), (0, "Va"), (1, "Vb")] {
            c.add(sim_core::Element::vdc(name, n[k], 0, 5.0));
        }
        c.add(sim_core::Element::Resistor { name: "Rs".into(), a: n[2], b: 0, r: 10e3 });
        c.add(sim_core::Element::Resistor { name: "Rc".into(), a: n[3], b: 0, r: 10e3 });
        let r = sim_core::dc_op(&c).unwrap();
        assert!(r.voltage("s").unwrap() < 0.5 && r.voltage("c").unwrap() > 4.5, "{:?}", (r.voltage("s"), r.voltage("c")));
    }

    #[test]
    fn a_constant_output_comes_from_the_supply() {
        let g = logic("A", "H = 1\nL = 0").unwrap();
        let t = g.subckt("K", 5.0);
        assert!(t.contains("Yh buf h 0 gnd vcc 60 0.5 0 0 0 vcc") && t.contains("Yl buf l 0 gnd vcc 60 0.5 0 0 0 gnd"), "{t}");
    }
}
