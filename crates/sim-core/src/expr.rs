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
            "exp" => (Func::Exp, 1..=1),
            "ln" | "log" => (Func::Ln, 1..=1),
            "log10" => (Func::Log10, 1..=1),
            "pow" | "pwr" => (Func::Pow, 2..=2),
            "tanh" => (Func::Tanh, 1..=1),
            "sin" => (Func::Sin, 1..=1),
            "cos" => (Func::Cos, 1..=1),
            "floor" => (Func::Floor, 1..=1),
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
        if !self.eat(b':') {
            return self.err("「? :」の「:」がありません");
        }
        let no = self.expr()?;
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

#[derive(Clone, Debug)]
enum Tree {
    Num(f64),
    Time,
    Var(usize),
    /// A node 0 reference: a constant 0 V.
    Ground,
    Neg(Box<Tree>),
    Bin(Op, Box<Tree>, Box<Tree>),
    Call(Func, Vec<Tree>),
}

#[derive(Clone, Debug)]
pub struct CExpr {
    tree: Tree,
    pub vars: Vec<Var>,
}

impl CExpr {
    pub fn empty() -> CExpr {
        CExpr { tree: Tree::Num(0.0), vars: vec![] }
    }
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
        let tree = self.build(&mut vars, branch_index);
        CExpr { tree, vars }
    }

    fn build(&self, vars: &mut Vec<Var>, bi: &dyn Fn(&str) -> Option<usize>) -> Tree {
        let slot = |v: Var, vars: &mut Vec<Var>| match vars.iter().position(|x| *x == v) {
            Some(i) => i,
            None => {
                vars.push(v);
                vars.len() - 1
            }
        };
        match self {
            Expr::Num(v) => Tree::Num(*v),
            Expr::Time => Tree::Time,
            Expr::Temp => Tree::Num(crate::temperature::T_NOM_C),
            Expr::Node(0) => Tree::Ground,
            Expr::Node(n) => Tree::Var(slot(Var::Node(*n), vars)),
            Expr::Branch(name) => match bi(name) {
                Some(i) => Tree::Var(slot(Var::Unknown(i), vars)),
                None => Tree::Num(0.0),
            },
            Expr::Neg(a) => Tree::Neg(Box::new(a.build(vars, bi))),
            Expr::Bin(op, a, b) => Tree::Bin(*op, Box::new(a.build(vars, bi)), Box::new(b.build(vars, bi))),
            Expr::Call(f, args) => Tree::Call(*f, args.iter().map(|a| a.build(vars, bi)).collect()),
        }
    }
}

/// A value and its derivatives with respect to the expression's variables.
#[derive(Clone, Debug)]
struct D {
    v: f64,
    g: Vec<f64>,
}

impl D {
    fn konst(v: f64, n: usize) -> D {
        D { v, g: vec![0.0; n] }
    }
    fn map(&self, v: f64, dv: f64) -> D {
        D { v, g: self.g.iter().map(|x| x * dv).collect() }
    }
    fn lin(a: &D, ka: f64, b: &D, kb: f64, v: f64) -> D {
        D { v, g: a.g.iter().zip(&b.g).map(|(x, y)| ka * x + kb * y).collect() }
    }
}

fn smooth(s: f64) -> (f64, f64) {
    let s = s.clamp(0.0, 1.0);
    (s * s * (3.0 - 2.0 * s), 6.0 * s * (1.0 - s))
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
        let n = self.vars.len();
        let d = self.go(&self.tree, x, t, n);
        (d.v, d.g)
    }

    fn go(&self, tr: &Tree, x: &[f64], t: f64, n: usize) -> D {
        match tr {
            Tree::Num(v) => D::konst(*v, n),
            Tree::Time => D::konst(t, n),
            Tree::Ground => D::konst(0.0, n),
            Tree::Var(k) => {
                let mut g = vec![0.0; n];
                g[*k] = 1.0;
                D { v: self.read(x, *k), g }
            }
            Tree::Neg(a) => {
                let a = self.go(a, x, t, n);
                a.map(-a.v, -1.0)
            }
            Tree::Bin(op, a, b) => {
                let (a, b) = (self.go(a, x, t, n), self.go(b, x, t, n));
                match op {
                    Op::Add => D::lin(&a, 1.0, &b, 1.0, a.v + b.v),
                    Op::Sub => D::lin(&a, 1.0, &b, -1.0, a.v - b.v),
                    Op::Mul => D::lin(&a, b.v, &b, a.v, a.v * b.v),
                    Op::Div => {
                        // a division by (almost) nothing is held at a large finite value
                        let den = if b.v.abs() < 1e-9 { 1e-9f64.copysign(if b.v == 0.0 { 1.0 } else { b.v }) } else { b.v };
                        D::lin(&a, 1.0 / den, &b, -a.v / (den * den), a.v / den)
                    }
                    Op::Pow => pow(&a, &b),
                    Op::Lt => D::konst((a.v < b.v) as u8 as f64, n),
                    Op::Gt => D::konst((a.v > b.v) as u8 as f64, n),
                    Op::Le => D::konst((a.v <= b.v) as u8 as f64, n),
                    Op::Ge => D::konst((a.v >= b.v) as u8 as f64, n),
                    Op::Eq => D::konst((a.v == b.v) as u8 as f64, n),
                    Op::Ne => D::konst((a.v != b.v) as u8 as f64, n),
                }
            }
            Tree::Call(f, args) => {
                let a: Vec<D> = args.iter().map(|e| self.go(e, x, t, n)).collect();
                match f {
                    Func::Abs => a[0].map(a[0].v.abs(), if a[0].v < 0.0 { -1.0 } else { 1.0 }),
                    Func::Min => if a[0].v <= a[1].v { a[0].clone() } else { a[1].clone() },
                    Func::Max => if a[0].v >= a[1].v { a[0].clone() } else { a[1].clone() },
                    Func::Limit => {
                        if a[0].v < a[1].v {
                            a[1].clone()
                        } else if a[0].v > a[2].v {
                            a[2].clone()
                        } else {
                            a[0].clone()
                        }
                    }
                    Func::Sqrt => {
                        if a[0].v > 1e-30 {
                            let r = a[0].v.sqrt();
                            a[0].map(r, 0.5 / r)
                        } else {
                            D::konst(0.0, n)
                        }
                    }
                    Func::Exp => {
                        let r = a[0].v.min(80.0).exp();
                        a[0].map(r, if a[0].v < 80.0 { r } else { 0.0 })
                    }
                    Func::Ln => {
                        let v = a[0].v.max(1e-30);
                        a[0].map(v.ln(), if a[0].v > 1e-30 { 1.0 / v } else { 0.0 })
                    }
                    Func::Log10 => {
                        let v = a[0].v.max(1e-30);
                        a[0].map(v.log10(), if a[0].v > 1e-30 { 1.0 / (v * std::f64::consts::LN_10) } else { 0.0 })
                    }
                    Func::Pow => pow(&a[0], &a[1]),
                    Func::Tanh => {
                        let r = a[0].v.tanh();
                        a[0].map(r, 1.0 - r * r)
                    }
                    Func::Sin => a[0].map(a[0].v.sin(), a[0].v.cos()),
                    Func::Floor => a[0].map(a[0].v.floor(), 0.0),
                    Func::Tan => {
                        let r = a[0].v.tan();
                        a[0].map(r, 1.0 + r * r)
                    }
                    Func::Asin | Func::Acos => {
                        // clamped at +-1, where the slope would be infinite
                        let x = a[0].v.clamp(-1.0 + 1e-9, 1.0 - 1e-9);
                        let d = 1.0 / (1.0 - x * x).sqrt();
                        if *f == Func::Asin { a[0].map(x.asin(), d) } else { a[0].map(x.acos(), -d) }
                    }
                    Func::Atan => a[0].map(a[0].v.atan(), 1.0 / (1.0 + a[0].v * a[0].v)),
                    Func::Sinh => a[0].map(a[0].v.clamp(-80.0, 80.0).sinh(), a[0].v.clamp(-80.0, 80.0).cosh()),
                    Func::Cosh => a[0].map(a[0].v.clamp(-80.0, 80.0).cosh(), a[0].v.clamp(-80.0, 80.0).sinh()),
                    Func::Asinh => a[0].map(a[0].v.asinh(), 1.0 / (a[0].v * a[0].v + 1.0).sqrt()),
                    Func::Cos => a[0].map(a[0].v.cos(), -a[0].v.sin()),
                    Func::If => if a[0].v != 0.0 { a[1].clone() } else { a[2].clone() },
                    Func::Step => {
                        // smooth 0 -> 1 across [-w, w] (w = 1 mV when omitted)
                        let w = a.get(1).map_or(1e-3, |w| w.v.abs().max(1e-12));
                        let (s, ds) = smooth((a[0].v + w) / (2.0 * w));
                        let inside = a[0].v > -w && a[0].v < w;
                        a[0].map(s, if inside { ds / (2.0 * w) } else { 0.0 })
                    }
                }
            }
        }
    }
}

fn pow(a: &D, b: &D) -> D {
    let n = a.g.len();
    if a.v > 0.0 {
        let r = a.v.powf(b.v).min(1e300);
        // d/da = b a^(b-1), d/db = a^b ln a
        D::lin(a, b.v * a.v.powf(b.v - 1.0), b, r * a.v.ln(), r)
    } else if a.v == 0.0 && b.v > 0.0 {
        D::konst(0.0, n)
    } else if b.v.fract() == 0.0 && a.v != 0.0 {
        // a negative base with a whole exponent
        let r = a.v.powf(b.v);
        a.map(r, b.v * a.v.powf(b.v - 1.0))
    } else {
        D::konst(0.0, n)
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
