//! Expressions for behavioural sources (`B` elements): `V = min(60*(4.2 - V(bat)), 2.4)`.
//!
//! An expression is parsed once into an `Expr`. Before a run it is compiled against the circuit
//! (`Expr::compile`), which fixes the list of unknowns it reads. Evaluation returns the value and
//! its partial derivatives with respect to those unknowns (forward-mode differentiation), which
//! is what Newton's method needs.
//!
//! Syntax: numbers with SPICE suffixes (`4.7k`, `100u`), `+ - * / ^`, comparisons `< > <= >=`
//! (1 or 0), `time`, `temp` (circuit temperature in °C), `V(node)`, `V(a,b)`, `I(source)` (current through a V source, inductor or
//! another voltage-type B source) and the functions
//! `abs min max limit sqrt exp ln pow tanh if step`.

use crate::value::parse_value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Func {
    Abs,
    Min,
    Max,
    Limit,
    Sqrt,
    Exp,
    Ln,
    Log10,
    Pow,
    Tanh,
    Sin,
    Cos,
    Floor,
    Sgn,
    Pwrs,
    Tan,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Asinh,
    If,
    Step,
}

impl Func {
    fn from_name(n: &str) -> Option<(Func, std::ops::RangeInclusive<usize>)> {
        Some(match n {
            "abs" => (Func::Abs, 1..=1),
            "min" => (Func::Min, 2..=2),
            "max" => (Func::Max, 2..=2),
            "limit" => (Func::Limit, 3..=3),
            "sqrt" => (Func::Sqrt, 1..=1),
            "exp" | "expl" => (Func::Exp, 1..=1),
            "ln" | "log" => (Func::Ln, 1..=1),
            "log10" => (Func::Log10, 1..=1),
            "pow" | "pwr" => (Func::Pow, 2..=2),
            "tanh" => (Func::Tanh, 1..=1),
            "sin" => (Func::Sin, 1..=1),
            "cos" => (Func::Cos, 1..=1),
            "floor" => (Func::Floor, 1..=1),
            "sgn" | "sign" => (Func::Sgn, 1..=1),
            "pwrs" => (Func::Pwrs, 2..=2),
            "tan" => (Func::Tan, 1..=1),
            "asin" | "arcsin" => (Func::Asin, 1..=1),
            "acos" | "arccos" => (Func::Acos, 1..=1),
            "atan" | "arctan" => (Func::Atan, 1..=1),
            "sinh" => (Func::Sinh, 1..=1),
            "cosh" => (Func::Cosh, 1..=1),
            "asinh" => (Func::Asinh, 1..=1),
            "if" => (Func::If, 3..=3),
            "step" | "u" => (Func::Step, 1..=2),
            _ => return None,
        })
    }
}

/// A parsed expression; nodes are circuit node indices, branch currents are element names.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    Time,
    /// Circuit temperature in °C: 27 until `Circuit::at_temperature` substitutes the real value.
    Temp,
    Node(usize),
    Branch(String),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
}

impl Expr {
    /// The nodes the expression reads with `V(..)`.
    pub fn read_nodes(&self, out: &mut Vec<usize>) {
        match self {
            Expr::Node(n) => out.push(*n),
            Expr::Neg(e) => e.read_nodes(out),
            Expr::Bin(_, a, b) => {
                a.read_nodes(out);
                b.read_nodes(out);
            }
            Expr::Call(_, args) => args.iter().for_each(|e| e.read_nodes(out)),
            Expr::Num(_) | Expr::Time | Expr::Temp | Expr::Branch(_) => {}
        }
    }
}

// ------------------------------------------------------------------ parsing

struct Parser<'a, 'b> {
    s: &'a [u8],
    text: &'a str,
    pos: usize,
    node: &'b mut dyn FnMut(&str) -> usize,
    branch: &'b dyn Fn(&str) -> String,
}

type PResult<T> = Result<T, String>;

impl Parser<'_, '_> {
    fn err<T>(&self, why: &str) -> PResult<T> {
        let near: String = self.text.chars().skip(self.text[..self.pos.min(self.text.len())].chars().count()).take(12).collect();
        Err(format!("式「{}」を読み取れません: {why} (「{near}」の付近)", self.text))
    }

    fn ws(&mut self) {
        while self.pos < self.s.len() && self.s[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.pos < self.s.len() && self.s[self.pos] == c {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// A value used as a logic level: above 0.5 is true (as in LTspice).
    fn truth(e: Expr) -> Expr {
        Expr::Bin(Op::Gt, Box::new(e), Box::new(Expr::Num(0.5)))
    }

    fn logic(cond: Expr, yes: f64, no: f64) -> Expr {
        Expr::Call(Func::If, vec![cond, Expr::Num(yes), Expr::Num(no)])
    }

    /// `cond ? a : b` (the lowest precedence).
    fn expr(&mut self) -> PResult<Expr> {
        let cond = self.or_expr()?;
        if !self.eat(b'?') {
            return Ok(cond);
        }
        let yes = self.expr()?;
        // a missing `: else` (some vendors leave it out) means 0
        let no = if self.eat(b':') { self.expr()? } else { Expr::Num(0.0) };
        Ok(Expr::Call(Func::If, vec![cond, yes, no]))
    }

    /// `a | b`: true if either is.
    fn or_expr(&mut self) -> PResult<Expr> {
        let mut a = self.and_expr()?;
        loop {
            self.ws();
            if self.s.get(self.pos) != Some(&b'|') {
                return Ok(a);
            }
            self.pos += if self.s.get(self.pos + 1) == Some(&b'|') { 2 } else { 1 };
            let b = self.and_expr()?;
            a = Self::logic(Self::truth(a), 1.0, 0.0);
            a = Expr::Call(Func::If, vec![Self::truth(a), Expr::Num(1.0), Self::logic(Self::truth(b), 1.0, 0.0)]);
        }
    }

    /// `a & b`: true if both are.
    fn and_expr(&mut self) -> PResult<Expr> {
        let mut a = self.cmp()?;
        loop {
            self.ws();
            if self.s.get(self.pos) != Some(&b'&') {
                return Ok(a);
            }
            self.pos += if self.s.get(self.pos + 1) == Some(&b'&') { 2 } else { 1 };
            let b = self.cmp()?;
            a = Expr::Call(Func::If, vec![Self::truth(a), Self::logic(Self::truth(b), 1.0, 0.0), Expr::Num(0.0)]);
        }
    }

    fn cmp(&mut self) -> PResult<Expr> {
        let a = self.sum()?;
        self.ws();
        let op = match (self.s.get(self.pos), self.s.get(self.pos + 1)) {
            (Some(b'='), Some(b'=')) => Some((Op::Eq, 2)),
            (Some(b'!'), Some(b'=')) => Some((Op::Ne, 2)),
            (Some(b'<'), Some(b'=')) => Some((Op::Le, 2)),
            (Some(b'>'), Some(b'=')) => Some((Op::Ge, 2)),
            (Some(b'<'), _) => Some((Op::Lt, 1)),
            (Some(b'>'), _) => Some((Op::Gt, 1)),
            _ => None,
        };
        if let Some((op, len)) = op {
            self.pos += len;
            let b = self.sum()?;
            return Ok(Expr::Bin(op, Box::new(a), Box::new(b)));
        }
        Ok(a)
    }

    fn sum(&mut self) -> PResult<Expr> {
        let mut a = self.term()?;
        loop {
            self.ws();
            let op = match self.s.get(self.pos) {
                Some(b'+') => Op::Add,
                Some(b'-') => Op::Sub,
                _ => return Ok(a),
            };
            self.pos += 1;
            let b = self.term()?;
            a = Expr::Bin(op, Box::new(a), Box::new(b));
        }
    }

    fn term(&mut self) -> PResult<Expr> {
        let mut a = self.unary()?;
        loop {
            self.ws();
            let op = match self.s.get(self.pos) {
                Some(b'*') if self.s.get(self.pos + 1) != Some(&b'*') => Op::Mul,
                Some(b'/') => Op::Div,
                _ => return Ok(a),
            };
            self.pos += 1;
            let b = self.unary()?;
            a = Expr::Bin(op, Box::new(a), Box::new(b));
        }
    }

    fn unary(&mut self) -> PResult<Expr> {
        self.ws();
        if (self.s.get(self.pos) == Some(&b'!') && self.s.get(self.pos + 1) != Some(&b'=')) || self.s.get(self.pos) == Some(&b'~') {
            self.pos += 1;
            let e = self.unary()?;
            return Ok(Self::logic(Self::truth(e), 0.0, 1.0));
        }
        if self.eat(b'-') {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat(b'+') {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> PResult<Expr> {
        let a = self.primary()?;
        self.ws();
        let caret = self.s.get(self.pos) == Some(&b'^');
        let stars = self.s.get(self.pos) == Some(&b'*') && self.s.get(self.pos + 1) == Some(&b'*');
        if caret || stars {
            self.pos += if caret { 1 } else { 2 };
            let b = self.unary()?; // right associative: 2^-1 and 2^3^2 both work
            return Ok(Expr::Bin(Op::Pow, Box::new(a), Box::new(b)));
        }
        Ok(a)
    }

    fn primary(&mut self) -> PResult<Expr> {
        self.ws();
        let Some(&c) = self.s.get(self.pos) else { return self.err("式が途中で終わっています") };
        if c == b'(' {
            self.pos += 1;
            let e = self.expr()?;
            if !self.eat(b')') {
                return self.err("閉じ括弧 ) がありません");
            }
            return Ok(e);
        }
        if c.is_ascii_digit() || c == b'.' {
            return self.number();
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            return self.name();
        }
        self.err("ここに置けない文字です")
    }

    fn number(&mut self) -> PResult<Expr> {
        let start = self.pos;
        while self.pos < self.s.len() && (self.s[self.pos].is_ascii_digit() || self.s[self.pos] == b'.') {
            self.pos += 1;
        }
        // exponent: 1e-3 (only when digits follow, so that `1meg` and `2e` are not mistaken)
        if matches!(self.s.get(self.pos), Some(b'e' | b'E')) {
            let mut q = self.pos + 1;
            if matches!(self.s.get(q), Some(b'+' | b'-')) {
                q += 1;
            }
            if self.s.get(q).is_some_and(u8::is_ascii_digit) {
                while self.s.get(q).is_some_and(u8::is_ascii_digit) {
                    q += 1;
                }
                self.pos = q;
            }
        }
        // unit suffix: k, meg, u, n, p, f, m, g, t (and trailing unit letters such as V or A)
        while self.s.get(self.pos).is_some_and(u8::is_ascii_alphabetic) {
            self.pos += 1;
        }
        let tok = &self.text[start..self.pos];
        match parse_value(tok) {
            Some(v) => Ok(Expr::Num(v)),
            None => {
                self.pos = start;
                self.err(&format!("数値「{tok}」を読み取れません"))
            }
        }
    }

    fn name(&mut self) -> PResult<Expr> {
        let start = self.pos;
        while self.s.get(self.pos).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
            self.pos += 1;
        }
        let id = self.text[start..self.pos].to_ascii_lowercase();
        self.ws();
        let open = self.s.get(self.pos) == Some(&b'(');
        if !open {
            return match id.as_str() {
                "time" | "t" => Ok(Expr::Time),
                "temp" => Ok(Expr::Temp),
                "pi" => Ok(Expr::Num(std::f64::consts::PI)),
                _ => {
                    self.pos = start;
                    self.err(&format!("「{id}」は使えません。電圧は V(ノード名)、電流は I(電源名)、時間は time、温度(℃)は temp と書きます"))
                }
            };
        }
        if id == "v" || id == "i" {
            // raw argument text up to the closing parenthesis
            let close = match self.text[self.pos..].find(')') {
                Some(i) => self.pos + i,
                None => return self.err("閉じ括弧 ) がありません"),
            };
            let inner = self.text[self.pos + 1..close].to_string();
            self.pos = close + 1;
            let args: Vec<&str> = inner.split(',').map(str::trim).filter(|a| !a.is_empty()).collect();
            return match (id.as_str(), args.as_slice()) {
                ("v", [a]) => Ok(Expr::Node((self.node)(a))),
                ("v", [a, b]) => {
                    let (x, y) = ((self.node)(a), (self.node)(b));
                    Ok(Expr::Bin(Op::Sub, Box::new(Expr::Node(x)), Box::new(Expr::Node(y))))
                }
                ("i", [a]) => Ok(Expr::Branch((self.branch)(a))),
                _ => {
                    self.pos = start;
                    self.err("V(ノード) / V(ノード1,ノード2) / I(電源名) の形で書いてください")
                }
            };
        }
        if id == "uramp" {
            // LTspice: the ramp max(x, 0)
            self.pos += 1; // (
            let a = self.expr()?;
            if !self.eat(b')') {
                return self.err("閉じ括弧 ) がありません");
            }
            return Ok(Expr::Call(Func::Max, vec![a, Expr::Num(0.0)]));
        }
        if id == "buf" || id == "inv" {
            // LTspice logic buffer / inverter: the argument is a logic level (above 0.5 is true)
            self.pos += 1; // (
            let a = self.expr()?;
            if !self.eat(b')') {
                return self.err("閉じ括弧 ) がありません");
            }
            return Ok(if id == "buf" { Self::logic(Self::truth(a), 1.0, 0.0) } else { Self::logic(Self::truth(a), 0.0, 1.0) });
        }
        if id == "tbl" || id == "table" {
            // tbl(x, x1,y1, x2,y2, ...): piecewise-linear in x, flat beyond the ends
            self.pos += 1; // (
            let mut args = vec![];
            loop {
                args.push(self.expr()?);
                if self.eat(b',') {
                    continue;
                }
                if self.eat(b')') {
                    break;
                }
                return self.err("引数は , で区切り、最後に ) で閉じます");
            }
            let pts: Option<Vec<(f64, f64)>> = args[1..].chunks(2).map(|c| match c {
                [Expr::Num(x), Expr::Num(y)] => Some((*x, *y)),
                [Expr::Neg(x), Expr::Num(y)] => match **x { Expr::Num(x) => Some((-x, *y)), _ => None },
                [Expr::Num(x), Expr::Neg(y)] => match **y { Expr::Num(y) => Some((*x, -y)), _ => None },
                [Expr::Neg(x), Expr::Neg(y)] => match (&**x, &**y) { (Expr::Num(x), Expr::Num(y)) => Some((-x, -y)), _ => None },
                _ => None,
            }).collect();
            let Some(mut pts) = pts.filter(|p| p.len() >= 2 && args.len() % 2 == 1) else {
                self.pos = start;
                return self.err("tbl(x, x1,y1, x2,y2, ...) は、数値の点を 2 つ以上並べて書きます");
            };
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let x = args[0].clone();
            let lt = |v: f64| Expr::Bin(Op::Lt, Box::new(x.clone()), Box::new(Expr::Num(v)));
            let mut e = Expr::Num(pts[pts.len() - 1].1);
            for k in (0..pts.len() - 1).rev() {
                let ((x0, y0), (x1, y1)) = (pts[k], pts[k + 1]);
                let seg = if x1 > x0 {
                    Expr::Bin(Op::Add, Box::new(Expr::Num(y0)), Box::new(Expr::Bin(Op::Mul, Box::new(Expr::Num((y1 - y0) / (x1 - x0))), Box::new(Expr::Bin(Op::Sub, Box::new(x.clone()), Box::new(Expr::Num(x0)))))))
                } else {
                    Expr::Num(y0)
                };
                e = Expr::Call(Func::If, vec![lt(x1), seg, e]);
            }
            return Ok(Expr::Call(Func::If, vec![lt(pts[0].0), Expr::Num(pts[0].1), e]));
        }
        let Some((func, arity)) = Func::from_name(&id) else {
            self.pos = start;
            return self.err(&format!("関数「{id}」は使えません (abs min max limit sqrt exp ln pow tanh if step)"));
        };
        self.pos += 1; // (
        let mut args = vec![];
        if !self.eat(b')') {
            loop {
                args.push(self.expr()?);
                if self.eat(b',') {
                    continue;
                }
                if self.eat(b')') {
                    break;
                }
                return self.err("引数は , で区切り、最後に ) で閉じます");
            }
        }
        if !arity.contains(&args.len()) {
            self.pos = start;
            return self.err(&format!("関数「{id}」の引数は {} 個必要です (いまは {} 個)", arity.end(), args.len()));
        }
        Ok(Expr::Call(func, args))
    }
}

impl Expr {
    /// Parse `text`. `node` maps a node name to its circuit index; `branch` maps the name given
    /// in `I(name)` to the element name used in the circuit.
    pub fn parse(text: &str, node: &mut dyn FnMut(&str) -> usize, branch: &dyn Fn(&str) -> String) -> Result<Expr, String> {
        let mut p = Parser { s: text.as_bytes(), text, pos: 0, node, branch };
        let e = p.expr()?;
        p.ws();
        if p.pos < p.s.len() {
            return p.err("式の後ろに余分な文字があります");
        }
        Ok(e)
    }

    /// Names of the elements whose current the expression reads.
    pub fn branches(&self, out: &mut Vec<String>) {
        match self {
            Expr::Branch(n) => out.push(n.clone()),
            Expr::Neg(a) => a.branches(out),
            Expr::Bin(_, a, b) => {
                a.branches(out);
                b.branches(out);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.branches(out)),
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ compiled form and evaluation

/// What an expression reads from the solution vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Var {
    /// Voltage of a circuit node (never 0).
    Node(usize),
    /// An unknown by its index in the solution vector (a branch current).
    Unknown(usize),
}

/// One step of a compiled expression, run on a stack of values with their derivatives (postfix order: the operands of a step are
/// the values on top of the stack, and its result takes their place).
#[derive(Clone, Copy, Debug)]
enum Ins {
    Num(f64),
    Time,
    /// A node 0 reference: a constant 0 V.
    Ground,
    /// Variable `k` of `CExpr::vars`, with the derivative 1 with respect to itself.
    Var(usize),
    Neg,
    Bin(Op),
    /// A function of the `usize` values on top of the stack.
    Call(Func, usize),
}

#[derive(Clone, Debug)]
pub struct CExpr {
    code: Vec<Ins>,
    /// The most values on the stack at once.
    depth: usize,
    pub vars: Vec<Var>,
}

impl CExpr {
    pub fn empty() -> CExpr {
        CExpr { code: vec![Ins::Num(0.0)], depth: 1, vars: vec![] }
    }
}

/// An element name with the `B_` that the flattener puts before a controlled source it turns into a behavioural one
/// (`U1.B_E_Edg1` -> `u1.e_edg1`), so that `I(E_Edg1)` finds it.
pub fn canon_branch(s: &str) -> String {
    s.to_ascii_lowercase().split('.').map(|seg| seg.strip_prefix("b_").unwrap_or(seg)).collect::<Vec<_>>().join(".")
}

impl Expr {
    /// The same expression with `temp` replaced by `temp_c` (°C).
    pub fn at_temperature(&self, temp_c: f64) -> Expr {
        match self {
            Expr::Temp => Expr::Num(temp_c),
            Expr::Neg(a) => Expr::Neg(Box::new(a.at_temperature(temp_c))),
            Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(a.at_temperature(temp_c)), Box::new(b.at_temperature(temp_c))),
            Expr::Call(f, args) => Expr::Call(*f, args.iter().map(|a| a.at_temperature(temp_c)).collect()),
            other => other.clone(),
        }
    }

    /// `branch_index(name)` gives the solution-vector index of the current through that element.
    pub fn compile(&self, branch_index: &dyn Fn(&str) -> Option<usize>) -> CExpr {
        let mut vars: Vec<Var> = vec![];
        let mut code = vec![];
        self.build(&mut vars, &mut code, branch_index);
        // the stack's height after each step: +1 for a value, -1 for an operator of two, 1 - n for a function of n
        let (mut height, mut depth) = (0usize, 0usize);
        for ins in &code {
            height = match ins {
                Ins::Num(_) | Ins::Time | Ins::Ground | Ins::Var(_) => height + 1,
                Ins::Neg => height,
                Ins::Bin(_) => height - 1,
                Ins::Call(_, n) => height + 1 - n,
            };
            depth = depth.max(height);
        }
        CExpr { code, depth, vars }
    }

    fn build(&self, vars: &mut Vec<Var>, code: &mut Vec<Ins>, bi: &dyn Fn(&str) -> Option<usize>) {
        let slot = |v: Var, vars: &mut Vec<Var>| match vars.iter().position(|x| *x == v) {
            Some(i) => i,
            None => {
                vars.push(v);
                vars.len() - 1
            }
        };
        match self {
            Expr::Num(v) => code.push(Ins::Num(*v)),
            Expr::Time => code.push(Ins::Time),
            Expr::Temp => code.push(Ins::Num(crate::temperature::T_NOM_C)),
            Expr::Node(0) => code.push(Ins::Ground),
            Expr::Node(n) => code.push(Ins::Var(slot(Var::Node(*n), vars))),
            Expr::Branch(name) => code.push(match bi(name) {
                Some(i) => Ins::Var(slot(Var::Unknown(i), vars)),
                None => Ins::Num(0.0),
            }),
            Expr::Neg(a) => {
                a.build(vars, code, bi);
                code.push(Ins::Neg);
            }
            Expr::Bin(op, a, b) => {
                a.build(vars, code, bi);
                b.build(vars, code, bi);
                code.push(Ins::Bin(*op));
            }
            Expr::Call(f, args) => {
                for a in args {
                    a.build(vars, code, bi);
                }
                code.push(Ins::Call(*f, args.len()));
            }
        }
    }
}

/// The stack of an evaluation, kept on the thread for its capacity: a value and its derivatives (`n` of them, the expression's
/// variables) per entry.
#[derive(Default)]
struct Stack {
    v: Vec<f64>,
    g: Vec<f64>,
}

thread_local! {
    static STACK: std::cell::RefCell<Stack> = std::cell::RefCell::new(Stack::default());
}

fn smooth(s: f64) -> (f64, f64) {
    let s = s.clamp(0.0, 1.0);
    (s * s * (3.0 - 2.0 * s), 6.0 * s * (1.0 - s))
}

/// The derivatives `g` scaled by `dv` (the chain rule of a function of one value).
fn scale(g: &mut [f64], dv: f64) {
    for e in g {
        *e *= dv;
    }
}

/// `g = ka * g + kb * gb` (the chain rule of a function of two values).
fn lin(g: &mut [f64], ka: f64, gb: &[f64], kb: f64) {
    for (e, b) in g.iter_mut().zip(gb) {
        *e = ka * *e + kb * b;
    }
}

/// `a ^ b`, its value into `av` and its derivatives into `ag`.
fn pow(av: &mut f64, ag: &mut [f64], bv: f64, bg: &[f64]) {
    let a = *av;
    if a > 0.0 {
        let r = a.powf(bv).min(1e300);
        // d/da = b a^(b-1), d/db = a^b ln a
        lin(ag, bv * a.powf(bv - 1.0), bg, r * a.ln());
        *av = r;
    } else if a == 0.0 && bv > 0.0 {
        *av = 0.0;
        ag.fill(0.0);
    } else if bv.fract() == 0.0 && a != 0.0 {
        // a negative base with a whole exponent
        let r = a.powf(bv);
        scale(ag, bv * a.powf(bv - 1.0));
        *av = r;
    } else {
        *av = 0.0;
        ag.fill(0.0);
    }
}

impl CExpr {
    fn read(&self, x: &[f64], k: usize) -> f64 {
        match self.vars[k] {
            Var::Node(n) => x[n - 1],
            Var::Unknown(i) => x[i],
        }
    }

    /// Value and the derivative with respect to each of `self.vars`.
    pub fn eval(&self, x: &[f64], t: f64) -> (f64, Vec<f64>) {
        self.eval_with(x, t, |v, g| (v, g.to_vec()))
    }

    /// `eval`, handing the derivatives to `k` as a slice. The matrix assembly calls this for every behavioural source at every
    /// Newton iteration: the steps run on one stack of values and one of derivatives, kept on the thread, with no allocation and
    /// no recursion (walking a tree that built a value with eight derivatives at every node was a quarter of a microsecond a
    /// source, a quarter of an assembly of the 41-part demo).
    pub fn eval_with<R>(&self, x: &[f64], t: f64, k: impl FnOnce(f64, &[f64]) -> R) -> R {
        let n = self.vars.len();
        let mut st = STACK.with(|s| std::mem::take(&mut *s.borrow_mut()));
        if st.v.len() < self.depth {
            st.v.resize(self.depth, 0.0);
        }
        if st.g.len() < self.depth * n {
            st.g.resize(self.depth * n, 0.0);
        }
        self.run(&mut st, x, t, n);
        let r = k(st.v[0], &st.g[..n]);
        STACK.with(|s| *s.borrow_mut() = st);
        r
    }

    fn run(&self, st: &mut Stack, x: &[f64], t: f64, n: usize) {
        let (vs, gs) = (&mut st.v, &mut st.g);
        // the number of values on the stack
        let mut sp = 0usize;
        for ins in &self.code {
            match *ins {
                Ins::Num(v) => {
                    vs[sp] = v;
                    gs[sp * n..(sp + 1) * n].fill(0.0);
                    sp += 1;
                }
                Ins::Time | Ins::Ground => {
                    vs[sp] = if matches!(ins, Ins::Time) { t } else { 0.0 };
                    gs[sp * n..(sp + 1) * n].fill(0.0);
                    sp += 1;
                }
                Ins::Var(k) => {
                    vs[sp] = self.read(x, k);
                    let g = &mut gs[sp * n..(sp + 1) * n];
                    g.fill(0.0);
                    g[k] = 1.0;
                    sp += 1;
                }
                Ins::Neg => {
                    let a = sp - 1;
                    vs[a] = -vs[a];
                    scale(&mut gs[a * n..(a + 1) * n], -1.0);
                }
                Ins::Bin(op) => {
                    sp -= 1;
                    let (a, b) = (sp - 1, sp);
                    let (av, bv) = (vs[a], vs[b]);
                    let (lo, hi) = gs.split_at_mut(b * n);
                    let (ag, bg) = (&mut lo[a * n..], &hi[..n]);
                    let konst = |v: f64, ag: &mut [f64]| {
                        ag.fill(0.0);
                        v
                    };
                    vs[a] = match op {
                        Op::Add => {
                            lin(ag, 1.0, bg, 1.0);
                            av + bv
                        }
                        Op::Sub => {
                            lin(ag, 1.0, bg, -1.0);
                            av - bv
                        }
                        Op::Mul => {
                            lin(ag, bv, bg, av);
                            av * bv
                        }
                        Op::Div => {
                            // a division by (almost) nothing is held at a large finite value
                            let den = if bv.abs() < 1e-9 { 1e-9f64.copysign(if bv == 0.0 { 1.0 } else { bv }) } else { bv };
                            lin(ag, 1.0 / den, bg, -av / (den * den));
                            av / den
                        }
                        Op::Pow => {
                            let mut v = av;
                            pow(&mut v, ag, bv, bg);
                            v
                        }
                        Op::Lt => konst((av < bv) as u8 as f64, ag),
                        Op::Gt => konst((av > bv) as u8 as f64, ag),
                        Op::Le => konst((av <= bv) as u8 as f64, ag),
                        Op::Ge => konst((av >= bv) as u8 as f64, ag),
                        Op::Eq => konst((av == bv) as u8 as f64, ag),
                        Op::Ne => konst((av != bv) as u8 as f64, ag),
                    };
                }
                Ins::Call(f, argc) => {
                    sp -= argc;
                    let a0 = sp;
                    sp += 1;
                    let v0 = vs[a0];
                    // the result replaces argument 0: a function of it alone scales its derivatives, one that picks an argument copies it
                    let pick = |vs: &mut [f64], gs: &mut [f64], i: usize| {
                        if i != 0 {
                            vs[a0] = vs[a0 + i];
                            gs.copy_within((a0 + i) * n..(a0 + i + 1) * n, a0 * n);
                        }
                    };
                    let one = |vs: &mut [f64], gs: &mut [f64], v: f64, dv: f64| {
                        vs[a0] = v;
                        scale(&mut gs[a0 * n..(a0 + 1) * n], dv);
                    };
                    match f {
                        Func::Abs => one(vs, gs, v0.abs(), if v0 < 0.0 { -1.0 } else { 1.0 }),
                        Func::Min => {
                            let i = if v0 <= vs[a0 + 1] { 0 } else { 1 };
                            pick(vs, gs, i);
                        }
                        Func::Max => {
                            let i = if v0 >= vs[a0 + 1] { 0 } else { 1 };
                            pick(vs, gs, i);
                        }
                        Func::Limit => {
                            // LTspice: the middle one of the three, so the two limits may be written in either order
                            let (lo, hi) = if vs[a0 + 1] <= vs[a0 + 2] { (1, 2) } else { (2, 1) };
                            let i = if v0 < vs[a0 + lo] { lo } else if v0 > vs[a0 + hi] { hi } else { 0 };
                            pick(vs, gs, i);
                        }
                        Func::Sqrt => {
                            if v0 > 1e-30 {
                                let r = v0.sqrt();
                                one(vs, gs, r, 0.5 / r);
                            } else {
                                vs[a0] = 0.0;
                                gs[a0 * n..(a0 + 1) * n].fill(0.0);
                            }
                        }
                        Func::Exp => {
                            let r = v0.min(80.0).exp();
                            one(vs, gs, r, if v0 < 80.0 { r } else { 0.0 });
                        }
                        Func::Ln => {
                            let v = v0.max(1e-30);
                            one(vs, gs, v.ln(), if v0 > 1e-30 { 1.0 / v } else { 0.0 });
                        }
                        Func::Log10 => {
                            let v = v0.max(1e-30);
                            one(vs, gs, v.log10(), if v0 > 1e-30 { 1.0 / (v * std::f64::consts::LN_10) } else { 0.0 });
                        }
                        Func::Pow => {
                            let bv = vs[a0 + 1];
                            let (lo, hi) = gs.split_at_mut((a0 + 1) * n);
                            let mut v = v0;
                            pow(&mut v, &mut lo[a0 * n..], bv, &hi[..n]);
                            vs[a0] = v;
                        }
                        Func::Tanh => {
                            let r = v0.tanh();
                            one(vs, gs, r, 1.0 - r * r);
                        }
                        Func::Sin => one(vs, gs, v0.sin(), v0.cos()),
                        Func::Floor => one(vs, gs, v0.floor(), 0.0),
                        Func::Sgn => one(vs, gs, if v0 > 0.0 { 1.0 } else if v0 < 0.0 { -1.0 } else { 0.0 }, 0.0),
                        Func::Pwrs => {
                            // sgn(x) * |x|^y
                            let s = if v0 < 0.0 { -1.0 } else { 1.0 };
                            let bv = vs[a0 + 1];
                            let (lo, hi) = gs.split_at_mut((a0 + 1) * n);
                            let ag = &mut lo[a0 * n..];
                            scale(ag, s);
                            let mut v = v0.abs();
                            pow(&mut v, ag, bv, &hi[..n]);
                            scale(ag, s);
                            vs[a0] = s * v;
                        }
                        Func::Tan => {
                            let r = v0.tan();
                            one(vs, gs, r, 1.0 + r * r);
                        }
                        Func::Asin | Func::Acos => {
                            // clamped at +-1, where the slope would be infinite
                            let x = v0.clamp(-1.0 + 1e-9, 1.0 - 1e-9);
                            let d = 1.0 / (1.0 - x * x).sqrt();
                            if f == Func::Asin { one(vs, gs, x.asin(), d) } else { one(vs, gs, x.acos(), -d) }
                        }
                        Func::Atan => one(vs, gs, v0.atan(), 1.0 / (1.0 + v0 * v0)),
                        Func::Sinh => one(vs, gs, v0.clamp(-80.0, 80.0).sinh(), v0.clamp(-80.0, 80.0).cosh()),
                        Func::Cosh => one(vs, gs, v0.clamp(-80.0, 80.0).cosh(), v0.clamp(-80.0, 80.0).sinh()),
                        Func::Asinh => one(vs, gs, v0.asinh(), 1.0 / (v0 * v0 + 1.0).sqrt()),
                        Func::Cos => one(vs, gs, v0.cos(), -v0.sin()),
                        Func::If => pick(vs, gs, if v0 != 0.0 { 1 } else { 2 }),
                        Func::Step => {
                            // smooth 0 -> 1 across [-w, w] (w = 1 mV when omitted)
                            let w = if argc > 1 { vs[a0 + 1].abs().max(1e-12) } else { 1e-3 };
                            let (s, ds) = smooth((v0 + w) / (2.0 * w));
                            let inside = v0 > -w && v0 < w;
                            one(vs, gs, s, if inside { ds / (2.0 * w) } else { 0.0 });
                        }
                    }
                }
            }
        }
        debug_assert_eq!(sp, 1, "a compiled expression leaves one value");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Expr, String> {
        let mut node = |n: &str| match n {
            "a" => 1,
            "b" => 2,
            "0" | "gnd" => 0,
            _ => 3,
        };
        Expr::parse(s, &mut node, &|n| n.to_string())
    }

    fn eval(s: &str, x: &[f64]) -> (f64, Vec<f64>) {
        let e = parse(s).unwrap();
        let c = e.compile(&|n| if n == "vs" { Some(2) } else { None });
        c.eval(x, 0.5)
    }

    #[test]
    fn arithmetic_precedence_and_suffixes() {
        assert_eq!(eval("1+2*3", &[]).0, 7.0);
        assert_eq!(eval("(1+2)*3", &[]).0, 9.0);
        assert_eq!(eval("2^3^2", &[]).0, 512.0);
        assert_eq!(eval("-2^2", &[]).0, -4.0);
        assert!((eval("4.7k + 100u", &[]).0 - 4700.0001).abs() < 1e-9);
        assert_eq!(eval("1e-3*1000", &[]).0, 1.0);
        assert_eq!(eval("2meg/1k", &[]).0, 2000.0);
        assert_eq!(eval("time*2", &[]).0, 1.0);
        assert_eq!(eval("2**3", &[]).0, 8.0);
    }

    #[test]
    fn node_voltages_and_their_derivatives() {
        let (v, g) = eval("3*V(a) - V(a,b)*V(b)", &[2.0, 5.0]);
        // 3*2 - (2-5)*5 = 21; d/da = 3 - 5 = -2, d/db = -( -1*5 + (2-5) ) = 8
        assert!((v - 21.0).abs() < 1e-12, "{v}");
        let e = parse("3*V(a) - V(a,b)*V(b)").unwrap();
        let c = e.compile(&|_| None);
        assert_eq!(c.vars.len(), 2);
        let idx = |n| c.vars.iter().position(|x| *x == Var::Node(n)).unwrap();
        assert!((g[idx(1)] + 2.0).abs() < 1e-12 && (g[idx(2)] - 8.0).abs() < 1e-12, "{g:?}");
    }

    #[test]
    fn derivatives_with_many_variables() {
        // 10 variables, each with its own derivative on the evaluation stack
        let names: Vec<String> = (0..10).map(|i| format!("v{i}")).collect();
        let text = names.iter().enumerate().map(|(i, n)| format!("{}*V({n})", i + 1)).collect::<Vec<_>>().join(" + ");
        let mut node = |n: &str| names.iter().position(|x| x == n).map_or(0, |i| i + 1);
        let e = Expr::parse(&text, &mut node, &|n| n.to_string()).unwrap();
        let c = e.compile(&|_| None);
        assert_eq!(c.vars.len(), 10);
        let x: Vec<f64> = (1..=10).map(|i| i as f64).collect();
        let (v, g) = c.eval(&x, 0.0);
        assert!((v - (1..=10).map(|i| (i * i) as f64).sum::<f64>()).abs() < 1e-9, "{v}");
        for (k, var) in c.vars.iter().enumerate() {
            let Var::Node(n) = var else { panic!() };
            assert!((g[k] - *n as f64).abs() < 1e-12, "d/dv{n} = {}", g[k]);
        }
    }

    #[test]
    fn ground_is_zero_and_branch_currents_are_unknowns() {
        assert_eq!(eval("V(0) + V(gnd) + 1", &[]).0, 1.0);
        let (v, g) = eval("I(vs)*2", &[0.0, 0.0, 1.5]);
        assert_eq!((v, g), (3.0, vec![2.0]));
        // an unknown element reads as zero
        assert_eq!(eval("I(nope)+1", &[]).0, 1.0);
    }

    #[test]
    fn functions_select_and_limit() {
        assert_eq!(eval("min(3,2)+max(3,2)", &[]).0, 5.0);
        assert_eq!(eval("limit(5,0,2)", &[]).0, 2.0);
        assert_eq!(eval("limit(-5,0,2)", &[]).0, 0.0);
        assert_eq!(eval("abs(-3)", &[]).0, 3.0);
        assert_eq!(eval("if(2>1, 10, 20)", &[]).0, 10.0);
        assert_eq!(eval("if(2<=1, 10, 20)", &[]).0, 20.0);
        assert!((eval("exp(ln(5))", &[]).0 - 5.0).abs() < 1e-12);
        assert!((eval("sqrt(16)+pow(2,10)", &[]).0 - 1028.0).abs() < 1e-9);
        // step is smooth: half way at 0, saturated outside the width
        assert!((eval("step(0, 0.1)", &[]).0 - 0.5).abs() < 1e-12);
        assert_eq!(eval("step(1, 0.1)", &[]).0, 1.0);
        assert_eq!(eval("step(-1, 0.1)", &[]).0, 0.0);
    }

    #[test]
    fn derivative_of_limit_min_and_step_follow_the_active_branch() {
        let (v, g) = eval("limit(2*V(a), -1, 1)", &[0.2]);
        assert!((v - 0.4).abs() < 1e-12 && (g[0] - 2.0).abs() < 1e-12);
        let (v, g) = eval("limit(2*V(a), -1, 1)", &[3.0]);
        assert!((v - 1.0).abs() < 1e-12 && g[0] == 0.0);
        let (_, g) = eval("min(V(a), 5)", &[1.0]);
        assert_eq!(g[0], 1.0);
        let (_, g) = eval("min(V(a), 5)", &[9.0]);
        assert_eq!(g[0], 0.0);
        let (s, g) = eval("step(V(a), 0.1)", &[0.0]);
        assert!((s - 0.5).abs() < 1e-12 && (g[0] - 7.5).abs() < 1e-9, "{s} {g:?}");
    }

    #[test]
    fn the_numeric_derivative_agrees_with_the_exact_one() {
        let text = "V(a)*V(b)/(1+V(a)^2) + exp(V(b)/3) - tanh(V(a)) + sqrt(V(b)+4)";
        let x = [0.7, 1.3];
        let (_, g) = eval(text, &x);
        let e = parse(text).unwrap();
        let c = e.compile(&|_| None);
        for (k, var) in c.vars.iter().enumerate() {
            let Var::Node(n) = var else { unreachable!() };
            let h = 1e-6;
            let (mut hi, mut lo) = (x, x);
            hi[n - 1] += h;
            lo[n - 1] -= h;
            let num = (c.eval(&hi, 0.0).0 - c.eval(&lo, 0.0).0) / (2.0 * h);
            assert!((num - g[k]).abs() < 1e-6, "var {k}: {num} vs {}", g[k]);
        }
    }

    #[test]
    fn mistakes_come_back_as_readable_messages() {
        for (bad, hint) in [
            ("1 +", "途中で終わって"),
            ("(1+2", "閉じ括弧"),
            ("foo(1)", "関数「foo」"),
            ("min(1)", "引数は 2 個"),
            ("vcc*2", "「vcc」は使えません"),
            ("V()", "V(ノード)"),
            ("1 2", "余分な文字"),
            ("3 $ 4", "余分な文字"),
            ("$", "置けない文字"),
        ] {
            let e = parse(bad).unwrap_err();
            assert!(e.contains(hint), "{bad}: {e}");
        }
    }

    #[test]
    fn current_references_are_collected() {
        let e = parse("I(vs)*2 + limit(I(l1), 0, 1)").unwrap();
        let mut b = vec![];
        e.branches(&mut b);
        assert_eq!(b, ["vs", "l1"]);
    }
}
