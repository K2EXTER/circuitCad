//! A sketch as blocks (B6: Arduino written by stacking blocks, as in Scratch): the variables, the blocks run once at the start
//! (`setup`) and the ones run over and over (`loop`). The sketch's text stays what is saved and run: `to_code` writes the blocks
//! as Arduino's C, `from_code` reads a sketch back into blocks (or says the line it cannot show as blocks, and why). Reading
//! what `to_code` wrote gives the same blocks back. Comments and `#define`s are not kept as blocks.

use super::lex::lex;
use super::parse::{Expr, Program, Stmt, Ty, parse};

/// A sketch as blocks.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BlockSketch {
    pub vars: Vec<VarDecl>,
    pub setup: Vec<Block>,
    pub looped: Vec<Block>,
}

/// A variable made at the top (`int count = 0;`).
#[derive(Clone, Debug, PartialEq)]
pub struct VarDecl {
    pub name: String,
    /// A number with a fraction (`float`) rather than a whole number (`int`).
    pub float: bool,
    pub init: BExpr,
}

/// How a pin is set up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinMode {
    Output,
    Input,
    InputPullup,
}

impl PinMode {
    pub const ALL: [PinMode; 3] = [PinMode::Output, PinMode::Input, PinMode::InputPullup];
    pub fn code(self) -> &'static str {
        match self {
            PinMode::Output => "OUTPUT",
            PinMode::Input => "INPUT",
            PinMode::InputPullup => "INPUT_PULLUP",
        }
    }
    fn of(name: &str) -> Option<PinMode> {
        PinMode::ALL.into_iter().find(|m| m.code() == name)
    }
}

/// One block: a statement of the sketch.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    PinMode { pin: BExpr, mode: PinMode },
    DigitalWrite { pin: BExpr, value: BExpr },
    AnalogWrite { pin: BExpr, value: BExpr },
    Delay(BExpr),
    SerialBegin(BExpr),
    /// `Serial.println` (`newline`) or `Serial.print`.
    Print { value: BExpr, newline: bool },
    /// `name = value;`
    Set { name: String, value: BExpr },
    /// `name += by;` (`name++` reads as by 1)
    Change { name: String, by: BExpr },
    If { cond: BExpr, then: Vec<Block>, els: Option<Vec<Block>> },
    /// `for (int counter = 0; counter < times; counter++) { … }`
    Repeat { counter: String, times: BExpr, body: Vec<Block> },
    While { cond: BExpr, body: Vec<Block> },
}

/// A value in a block's slot.
#[derive(Clone, Debug, PartialEq)]
pub enum BExpr {
    Int(i64),
    Float(f64),
    Text(String),
    /// A variable, or a name Arduino gives a number (`HIGH`, `A0`, `LED_BUILTIN`, `true`).
    Name(String),
    DigitalRead(Box<BExpr>),
    AnalogRead(Box<BExpr>),
    Millis,
    /// `random(low, high)`: a whole number from `low` up to (not with) `high`.
    Random(Box<BExpr>, Box<BExpr>),
    /// `map(value, from_low, from_high, to_low, to_high)`
    Map(Box<[BExpr; 5]>),
    /// Arithmetic, a comparison, `&&` or `||` (`BINARY_OPS`).
    Bin(String, Box<BExpr>, Box<BExpr>),
    Not(Box<BExpr>),
}

/// The operators a `BExpr::Bin` may have.
pub const BINARY_OPS: [&str; 13] = ["+", "-", "*", "/", "%", "==", "!=", "<", "<=", ">", ">=", "&&", "||"];

/// What a block cannot be made from: the line of the sketch (from 1) and why.
#[derive(Clone, Debug, PartialEq)]
pub struct NotBlocks {
    pub line: u32,
    pub message: String,
}

impl std::fmt::Display for NotBlocks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} 行目: {}", self.line, self.message)
    }
}

/// The first line of a sketch written from blocks.
pub const HEADER: &str = "// ブロックで作ったスケッチ";

/// The blocks as the text of a sketch (indented 2 spaces a level, one statement a line).
pub fn to_code(s: &BlockSketch) -> String {
    let mut out = format!("{HEADER}\n");
    for v in &s.vars {
        out += &format!("{} {} = {};\n", if v.float { "float" } else { "int" }, v.name, expr(&v.init));
    }
    if !s.vars.is_empty() {
        out.push('\n');
    }
    for (name, body) in [("setup", &s.setup), ("loop", &s.looped)] {
        out += &format!("void {name}() {{\n");
        stmts(body, 1, &mut out);
        out += "}\n";
        if name == "setup" {
            out.push('\n');
        }
    }
    out
}

fn stmts(body: &[Block], depth: usize, out: &mut String) {
    for b in body {
        stmt(b, depth, out);
    }
}

fn stmt(b: &Block, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    let line = match b {
        Block::PinMode { pin, mode } => format!("pinMode({}, {});", expr(pin), mode.code()),
        Block::DigitalWrite { pin, value } => format!("digitalWrite({}, {});", expr(pin), expr(value)),
        Block::AnalogWrite { pin, value } => format!("analogWrite({}, {});", expr(pin), expr(value)),
        Block::Delay(ms) => format!("delay({});", expr(ms)),
        Block::SerialBegin(baud) => format!("Serial.begin({});", expr(baud)),
        Block::Print { value, newline } => format!("Serial.{}({});", if *newline { "println" } else { "print" }, expr(value)),
        Block::Set { name, value } => format!("{name} = {};", expr(value)),
        Block::Change { name, by } => format!("{name} += {};", expr(by)),
        Block::If { cond, then, els } => {
            *out += &format!("{pad}if ({}) {{\n", expr(cond));
            stmts(then, depth + 1, out);
            if let Some(els) = els {
                *out += &format!("{pad}}} else {{\n");
                stmts(els, depth + 1, out);
            }
            *out += &format!("{pad}}}\n");
            return;
        }
        Block::Repeat { counter, times, body } => {
            *out += &format!("{pad}for (int {counter} = 0; {counter} < {}; {counter}++) {{\n", expr(times));
            stmts(body, depth + 1, out);
            *out += &format!("{pad}}}\n");
            return;
        }
        Block::While { cond, body } => {
            *out += &format!("{pad}while ({}) {{\n", expr(cond));
            stmts(body, depth + 1, out);
            *out += &format!("{pad}}}\n");
            return;
        }
    };
    *out += &format!("{pad}{line}\n");
}

/// A value as code (a calculation inside another in brackets, so it reads back as the same tree).
pub fn expr(e: &BExpr) -> String {
    let inner = |e: &BExpr| match e {
        BExpr::Bin(..) => format!("({})", expr(e)),
        _ => expr(e),
    };
    match e {
        BExpr::Int(n) => n.to_string(),
        BExpr::Float(x) if x.fract() == 0.0 && x.abs() < 1e15 => format!("{x:.1}"),
        BExpr::Float(x) => x.to_string(),
        BExpr::Text(t) => format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")),
        BExpr::Name(n) => n.clone(),
        BExpr::DigitalRead(p) => format!("digitalRead({})", expr(p)),
        BExpr::AnalogRead(p) => format!("analogRead({})", expr(p)),
        BExpr::Millis => "millis()".into(),
        BExpr::Random(a, b) => format!("random({}, {})", expr(a), expr(b)),
        BExpr::Map(a) => format!("map({})", a.iter().map(expr).collect::<Vec<_>>().join(", ")),
        BExpr::Bin(op, a, b) => format!("{} {op} {}", inner(a), inner(b)),
        BExpr::Not(a) => format!("!{}", inner(a)),
    }
}

/// A sketch read into blocks, or the first line that is no block and why.
pub fn from_code(src: &str) -> Result<BlockSketch, NotBlocks> {
    from_code_with_lines(src).map(|(s, _)| s)
}

/// A sketch read into blocks, with the line (from 1) each block starts on in the text: the blocks in the order they are drawn
/// (the start, then the loop; each block before the blocks inside it, an if's own before its else's). The line the sketch runs now
/// (`Machine::line`) names the block to light up.
pub fn from_code_with_lines(src: &str) -> Result<(BlockSketch, Vec<u32>), NotBlocks> {
    let mut lines = vec![];
    let err = |(line, message): (u32, String)| NotBlocks { line, message };
    let prog: Program = parse(lex(src).map_err(err)?).map_err(err)?;
    let mut s = BlockSketch::default();
    // the lines of setup's blocks and of loop's (whichever the text has first, setup's are drawn first)
    let mut lines_of: [Vec<u32>; 2] = [vec![], vec![]];
    for g in &prog.globals {
        let Stmt::Decl { ty, name, len, init, line } = g else { continue };
        if len.is_some() {
            return Err(no(*line, format!("配列 {name} はブロックにありません (変数を 1 つずつ作ります)")));
        }
        let init = match init.first() {
            Some(e) => bexpr(e, *line)?,
            None => BExpr::Int(0),
        };
        s.vars.push(VarDecl { name: name.clone(), float: *ty == Ty::Float, init });
    }
    for f in &prog.funcs {
        let body = match f.name.as_str() {
            "setup" => &mut s.setup,
            "loop" => &mut s.looped,
            other => return Err(no(f.line, format!("関数 {other}() はブロックにありません (ブロックは setup と loop の 2 つだけです)"))),
        };
        if !f.params.is_empty() || f.ret != Ty::Void {
            return Err(no(f.line, format!("{}() は void {}() と書きます", f.name, f.name)));
        }
        *body = blocks(&f.body, &mut lines_of[usize::from(f.name == "loop")])?;
    }
    let [setup_lines, loop_lines] = &mut lines_of;
    lines.append(setup_lines);
    lines.append(loop_lines);
    Ok((s, lines))
}

fn no(line: u32, message: String) -> NotBlocks {
    NotBlocks { line, message }
}

fn blocks(body: &[Stmt], lines: &mut Vec<u32>) -> Result<Vec<Block>, NotBlocks> {
    let mut v = vec![];
    for s in body {
        match s {
            // `{ … }` alone and an empty `;`: their statements in place
            Stmt::Block(inner) => v.extend(blocks(inner, lines)?),
            _ => v.push(block(s, lines)?),
        }
    }
    Ok(v)
}

/// The statements of a body (`{ … }` or one statement).
fn body_of(s: &Stmt, lines: &mut Vec<u32>) -> Result<Vec<Block>, NotBlocks> {
    match s {
        Stmt::Block(v) => blocks(v, lines),
        other => Ok(vec![block(other, lines)?]),
    }
}

/// A statement as a block; its line goes in `lines` before the lines of the blocks inside it.
fn block(s: &Stmt, lines: &mut Vec<u32>) -> Result<Block, NotBlocks> {
    if let Stmt::Expr(_, l) | Stmt::If(.., l) | Stmt::While(_, _, l) | Stmt::For(.., l) = s {
        lines.push(*l);
    }
    Ok(match s {
        Stmt::Expr(e, line) => call_block(e, *line)?,
        Stmt::If(c, then, els, line) => {
            let cond = bexpr(c, *line)?;
            let then = body_of(then, lines)?;
            let els = els.as_ref().map(|e| body_of(e, lines)).transpose()?;
            Block::If { cond, then, els }
        }
        Stmt::While(c, body, line) => {
            let cond = bexpr(c, *line)?;
            Block::While { cond, body: body_of(body, lines)? }
        }
        Stmt::For(init, cond, step, body, line) => repeat(init.as_deref(), cond.as_ref(), step.as_ref(), body, *line, lines)?,
        Stmt::Decl { name, line, .. } => {
            return Err(no(*line, format!("関数の中の変数 {name} はブロックにありません (上の「変数」で作り、「〜にする」ブロックで値を入れます)")));
        }
        Stmt::DoWhile(_, _, line) => return Err(no(*line, "do … while はブロックにありません (「〜の間くり返す」を使います)".into())),
        Stmt::Return(_, line) => return Err(no(*line, "return はブロックにありません".into())),
        Stmt::Break(line) | Stmt::Continue(line) => return Err(no(*line, "break・continue はブロックにありません".into())),
        Stmt::Block(_) => unreachable!("flattened by blocks()"),
    })
}

/// `for (int i = 0; i < n; i++)` (or `i += 1`, `++i`) as a repeat block; any other `for` is no block.
fn repeat(init: Option<&Stmt>, cond: Option<&Expr>, step: Option<&Expr>, body: &Stmt, line: u32, lines: &mut Vec<u32>) -> Result<Block, NotBlocks> {
    let bad = || no(line, "この for はブロックにありません (「n 回くり返す」は for (int i = 0; i < n; i++) の形だけです)".into());
    let counter = match init {
        Some(Stmt::Block(v)) if v.len() == 1 => match &v[0] {
            Stmt::Decl { ty: Ty::Int, name, len: None, init, .. } if matches!(init.as_slice(), [Expr::Int(0)]) => name.clone(),
            _ => return Err(bad()),
        },
        _ => return Err(bad()),
    };
    let times = match cond {
        Some(Expr::Binary("<", a, n)) if matches!(&**a, Expr::Var(x) if *x == counter) => bexpr(n, line)?,
        _ => return Err(bad()),
    };
    let is_counter = |e: &Expr| matches!(e, Expr::Var(x) if *x == counter);
    let steps_by_one = match step {
        Some(Expr::PostInc(e, 1)) => is_counter(e),
        Some(Expr::Assign("+=", e, one)) => is_counter(e) && matches!(&**one, Expr::Int(1)),
        _ => false,
    };
    if !steps_by_one {
        return Err(bad());
    }
    Ok(Block::Repeat { counter, times, body: body_of(body, lines)? })
}

/// A statement that is a call or an assignment.
fn call_block(e: &Expr, line: u32) -> Result<Block, NotBlocks> {
    let arg = |args: &[Expr], k: usize| bexpr(&args[k], line);
    let var_name = |t: &Expr| match t {
        Expr::Var(n) => Ok(n.clone()),
        _ => Err(no(line, "配列の要素はブロックにありません".into())),
    };
    Ok(match e {
        Expr::Call(name, args) => match (name.as_str(), args.len()) {
            ("pinMode", 2) => {
                let mode = match &args[1] {
                    Expr::Var(m) => PinMode::of(m),
                    _ => None,
                };
                let Some(mode) = mode else {
                    return Err(no(line, "pinMode の 2 つ目は OUTPUT・INPUT・INPUT_PULLUP のどれかです".into()));
                };
                Block::PinMode { pin: arg(args, 0)?, mode }
            }
            ("digitalWrite", 2) => Block::DigitalWrite { pin: arg(args, 0)?, value: arg(args, 1)? },
            ("analogWrite", 2) => Block::AnalogWrite { pin: arg(args, 0)?, value: arg(args, 1)? },
            ("delay", 1) => Block::Delay(arg(args, 0)?),
            ("Serial.begin", 1) => Block::SerialBegin(arg(args, 0)?),
            ("Serial.println", 1) => Block::Print { value: arg(args, 0)?, newline: true },
            ("Serial.println", 0) => Block::Print { value: BExpr::Text(String::new()), newline: true },
            ("Serial.print", 1) => Block::Print { value: arg(args, 0)?, newline: false },
            (other, _) => return Err(no(line, format!("{other}(…) はブロックにありません"))),
        },
        Expr::Assign("=", t, v) => Block::Set { name: var_name(t)?, value: bexpr(v, line)? },
        Expr::Assign("+=", t, v) => Block::Change { name: var_name(t)?, by: bexpr(v, line)? },
        // `x -= 3` with a number: changed by -3
        Expr::Assign("-=", t, v) if matches!(&**v, Expr::Int(_)) => {
            let Expr::Int(n) = &**v else { unreachable!() };
            Block::Change { name: var_name(t)?, by: BExpr::Int(-n) }
        }
        Expr::PostInc(t, d) => Block::Change { name: var_name(t)?, by: BExpr::Int(*d) },
        Expr::Assign(op, ..) => return Err(no(line, format!("「{op}」はブロックにありません (「〜にする」か「〜だけ変える」を使います)"))),
        _ => return Err(no(line, "この行はブロックにありません (値を計算するだけの行です)".into())),
    })
}

fn bexpr(e: &Expr, line: u32) -> Result<BExpr, NotBlocks> {
    let b = |e: &Expr| bexpr(e, line).map(Box::new);
    Ok(match e {
        Expr::Int(n) => BExpr::Int(*n),
        Expr::Float(x) => BExpr::Float(*x),
        Expr::Str(s) => BExpr::Text(s.clone()),
        Expr::Var(n) => BExpr::Name(n.clone()),
        Expr::Unary("-", a) if matches!(&**a, Expr::Int(_) | Expr::Float(_)) => match &**a {
            Expr::Int(n) => BExpr::Int(-n),
            Expr::Float(x) => BExpr::Float(-x),
            _ => unreachable!(),
        },
        Expr::Unary("!", a) => BExpr::Not(b(a)?),
        Expr::Binary(op, x, y) if BINARY_OPS.contains(op) => BExpr::Bin(op.to_string(), b(x)?, b(y)?),
        Expr::Call(name, args) => match (name.as_str(), args.as_slice()) {
            ("digitalRead", [p]) => BExpr::DigitalRead(b(p)?),
            ("analogRead", [p]) => BExpr::AnalogRead(b(p)?),
            ("millis", []) => BExpr::Millis,
            ("random", [hi]) => BExpr::Random(Box::new(BExpr::Int(0)), b(hi)?),
            ("random", [lo, hi]) => BExpr::Random(b(lo)?, b(hi)?),
            ("map", [v, a, c, d, f]) => BExpr::Map(Box::new([bexpr(v, line)?, bexpr(a, line)?, bexpr(c, line)?, bexpr(d, line)?, bexpr(f, line)?])),
            (other, _) => return Err(no(line, format!("{other}(…) はブロックの値にありません"))),
        },
        Expr::Binary(op, ..) => return Err(no(line, format!("「{op}」はブロックにありません"))),
        Expr::Unary(op, _) => return Err(no(line, format!("「{op}」はブロックにありません"))),
        Expr::Index(n, _) => return Err(no(line, format!("配列 {n} はブロックにありません"))),
        Expr::Cond(..) => return Err(no(line, "「? :」はブロックにありません (「もし〜なら」を使います)".into())),
        Expr::Assign(..) | Expr::PostInc(..) => return Err(no(line, "値の中の代入はブロックにありません".into())),
    })
}

/// Whether a name may be a variable's: letters, digits and `_` (not first a digit), not a word of the language or of Arduino.
pub fn valid_name(name: &str) -> bool {
    const TAKEN: [&str; 30] = [
        "int", "long", "float", "double", "bool", "byte", "char", "void", "if", "else", "for", "while", "do", "return", "break", "continue", "true", "false", "HIGH", "LOW",
        "INPUT", "OUTPUT", "INPUT_PULLUP", "LED_BUILTIN", "setup", "loop", "switch", "case", "const", "unsigned",
    ];
    let mut c = name.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
        && !TAKEN.contains(&name)
        && !(name.starts_with('A') && name[1..].parse::<u32>().is_ok())
}

/// Every variable the blocks use by name, renamed (`from` → `to`): the uses in values, `Set`, `Change` and the repeats' counters.
pub fn rename(s: &mut BlockSketch, from: &str, to: &str) {
    fn in_expr(e: &mut BExpr, from: &str, to: &str) {
        match e {
            BExpr::Name(n) if n == from => *n = to.to_string(),
            BExpr::DigitalRead(a) | BExpr::AnalogRead(a) | BExpr::Not(a) => in_expr(a, from, to),
            BExpr::Random(a, b) | BExpr::Bin(_, a, b) => {
                in_expr(a, from, to);
                in_expr(b, from, to);
            }
            BExpr::Map(a) => a.iter_mut().for_each(|x| in_expr(x, from, to)),
            _ => {}
        }
    }
    fn in_blocks(v: &mut [Block], from: &str, to: &str) {
        for b in v {
            let swap = |n: &mut String| {
                if n == from {
                    *n = to.to_string();
                }
            };
            match b {
                Block::PinMode { pin, .. } => in_expr(pin, from, to),
                Block::DigitalWrite { pin, value } | Block::AnalogWrite { pin, value } => {
                    in_expr(pin, from, to);
                    in_expr(value, from, to);
                }
                Block::Delay(e) | Block::SerialBegin(e) | Block::Print { value: e, .. } => in_expr(e, from, to),
                Block::Set { name, value: e } | Block::Change { name, by: e } => {
                    swap(name);
                    in_expr(e, from, to);
                }
                Block::If { cond, then, els } => {
                    in_expr(cond, from, to);
                    in_blocks(then, from, to);
                    if let Some(e) = els {
                        in_blocks(e, from, to);
                    }
                }
                Block::Repeat { counter, times, body } => {
                    swap(counter);
                    in_expr(times, from, to);
                    in_blocks(body, from, to);
                }
                Block::While { cond, body } => {
                    in_expr(cond, from, to);
                    in_blocks(body, from, to);
                }
            }
        }
    }
    for v in &mut s.vars {
        if v.name == from {
            v.name = to.to_string();
        }
        in_expr(&mut v.init, from, to);
    }
    in_blocks(&mut s.setup, from, to);
    in_blocks(&mut s.looped, from, to);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(n: i64) -> BExpr {
        BExpr::Int(n)
    }
    fn name(n: &str) -> BExpr {
        BExpr::Name(n.into())
    }

    /// Blocks that use every kind of block and value.
    fn every_block() -> BlockSketch {
        let cmp = |op: &str, a: BExpr, b: BExpr| BExpr::Bin(op.into(), Box::new(a), Box::new(b));
        BlockSketch {
            vars: vec![VarDecl { name: "count".into(), float: false, init: int(0) }, VarDecl { name: "level".into(), float: true, init: BExpr::Float(0.5) }],
            setup: vec![
                Block::PinMode { pin: int(13), mode: PinMode::Output },
                Block::PinMode { pin: int(2), mode: PinMode::InputPullup },
                Block::SerialBegin(int(9600)),
            ],
            looped: vec![
                Block::If {
                    cond: cmp("&&", cmp("==", BExpr::DigitalRead(Box::new(int(2))), name("LOW")), BExpr::Not(Box::new(cmp("<", name("count"), int(-3))))),
                    then: vec![Block::DigitalWrite { pin: name("LED_BUILTIN"), value: name("HIGH") }, Block::Change { name: "count".into(), by: int(1) }],
                    els: Some(vec![Block::DigitalWrite { pin: int(13), value: name("LOW") }]),
                },
                Block::Repeat {
                    counter: "i".into(),
                    times: int(3),
                    body: vec![Block::AnalogWrite { pin: int(9), value: BExpr::Map(Box::new([BExpr::AnalogRead(Box::new(name("A0"))), int(0), int(1023), int(0), int(255)])) }],
                },
                Block::While { cond: cmp(">", BExpr::Millis, int(1000)), body: vec![Block::Delay(BExpr::Random(Box::new(int(10)), Box::new(int(20))))] },
                Block::Set { name: "level".into(), value: cmp("*", cmp("+", name("level"), BExpr::Float(1.0)), BExpr::Float(0.25)) },
                Block::Print { value: BExpr::Text("値は \"ok\"".into()), newline: false },
                Block::Print { value: name("count"), newline: true },
            ],
        }
    }

    #[test]
    fn blocks_read_back_from_their_code_and_run() {
        let s = every_block();
        let code = to_code(&s);
        assert_eq!(from_code(&code), Ok(s), "{code}");
        // the code is a sketch the built-in reader runs
        super::super::compile(&code, super::super::Board::Nano).unwrap();
        assert!(code.contains("for (int i = 0; i < 3; i++) {\n"), "{code}");
        assert!(code.contains("  if ((digitalRead(2) == LOW) && !(count < -3)) {\n"), "{code}");
    }

    #[test]
    fn the_blink_sample_is_blocks() {
        let s = from_code("// LED
void setup() {
  pinMode(LED_BUILTIN, OUTPUT);
}

void loop() {
  digitalWrite(LED_BUILTIN, HIGH);
  delay(500);
  digitalWrite(LED_BUILTIN, LOW);
  delay(500);
}
").unwrap();
        assert_eq!(s.setup, vec![Block::PinMode { pin: name("LED_BUILTIN"), mode: PinMode::Output }]);
        assert_eq!(s.looped.len(), 4);
        assert_eq!(s.looped[1], Block::Delay(int(500)));
    }

    /// Each block's line in the text, in the order the blocks are drawn (an if before the blocks inside it).
    #[test]
    fn each_block_knows_its_line() {
        let (s, lines) = from_code_with_lines(&to_code(&every_block())).unwrap();
        let code = to_code(&s);
        let line_of = |text: &str| code.lines().position(|l| l.trim_start().starts_with(text)).unwrap() as u32 + 1;
        assert_eq!(lines.len(), 3 + 11, "{lines:?}");
        assert_eq!(lines[0], line_of("pinMode(13"));
        assert_eq!(lines[3], line_of("if ("));
        assert_eq!(lines[4], line_of("digitalWrite(LED_BUILTIN"));
        assert_eq!(lines[6], line_of("digitalWrite(13, LOW)"), "the else's block after the if's own");
        assert_eq!(lines[8], line_of("analogWrite"), "inside the repeat");
        assert_eq!(*lines.last().unwrap(), line_of("Serial.println(count)"));
    }

    #[test]
    fn what_is_no_block_says_its_line_and_why() {
        let e = from_code("void setup() {}\nvoid loop() {\n  int v = analogRead(A0);\n}\n").unwrap_err();
        assert_eq!(e.line, 3);
        assert!(e.message.contains("変数 v"), "{e}");
        let e = from_code("int f(int x) { return x; }\nvoid setup() {}\nvoid loop() {}\n").unwrap_err();
        assert!(e.message.contains("f()"), "{e}");
        let e = from_code("void setup() {}\nvoid loop() {\n  for (int i = 5; i > 0; i--) {}\n}\n").unwrap_err();
        assert_eq!(e.line, 3);
        // a mistake in the code is the reader's message
        assert_eq!(from_code("void setup() {\n  delay(1)\n}\n").unwrap_err().line, 3);
    }

    #[test]
    fn short_forms_read_as_the_same_blocks() {
        let s = from_code("int n = 0;\nvoid setup() {}\nvoid loop() {\n  n++;\n  n -= 2;\n  for (int k = 0; k < n; k += 1) delay(1);\n  Serial.println(random(6));\n}\n").unwrap();
        assert_eq!(s.looped[0], Block::Change { name: "n".into(), by: int(1) });
        assert_eq!(s.looped[1], Block::Change { name: "n".into(), by: int(-2) });
        assert_eq!(s.looped[2], Block::Repeat { counter: "k".into(), times: name("n"), body: vec![Block::Delay(int(1))] });
        assert_eq!(s.looped[3], Block::Print { value: BExpr::Random(Box::new(int(0)), Box::new(int(6))), newline: true });
    }

    #[test]
    fn a_variable_renamed_is_renamed_where_it_is_used() {
        let mut s = every_block();
        rename(&mut s, "count", "presses");
        let code = to_code(&s);
        assert!(!code.contains("count"), "{code}");
        assert!(code.contains("int presses = 0;") && code.contains("presses += 1;"), "{code}");
        assert!(valid_name("presses") && valid_name("_x1"));
        assert!(!valid_name("1x") && !valid_name("HIGH") && !valid_name("A3") && !valid_name("ん") && !valid_name(""));
    }
}
