//! The sketch turned into a list of simple instructions for a stack machine, and the machine that runs it. The machine stops at
//! `delay` and goes on when the simulated time has come, so the circuit is solved between (the run never waits for the screen).

use super::parse::{Expr, Func, Program, Stmt, Ty};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    I(i64),
    F(f64),
    S(String),
}

impl Val {
    fn f(&self) -> f64 {
        match self {
            Val::I(i) => *i as f64,
            Val::F(f) => *f,
            Val::S(_) => 0.0,
        }
    }
    fn i(&self) -> i64 {
        match self {
            Val::I(i) => *i,
            Val::F(f) => *f as i64,
            Val::S(_) => 0,
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Val::I(i) => *i != 0,
            Val::F(f) => *f != 0.0,
            Val::S(s) => !s.is_empty(),
        }
    }
    fn of(ty: Ty, v: Val) -> Val {
        match (ty, v) {
            (Ty::Float, v) => Val::F(v.f()),
            (_, Val::F(f)) => Val::I(f as i64),
            (_, v) => v,
        }
    }
    /// As `Serial.print` writes it: integers plainly, floats with two decimals.
    pub fn text(&self) -> String {
        match self {
            Val::I(i) => i.to_string(),
            Val::F(f) => format!("{f:.2}"),
            Val::S(s) => s.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Slot {
    G(usize),
    L(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Var {
    slot: Slot,
    ty: Ty,
    /// An array's length (None for a plain variable).
    len: Option<usize>,
}

/// The functions the sketch may call that the machine does itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bi {
    PinMode,
    DigitalWrite,
    DigitalRead,
    AnalogRead,
    AnalogWrite,
    Delay,
    DelayUs,
    Millis,
    Micros,
    SerialBegin,
    SerialPrint,
    SerialPrintln,
    Map,
    Constrain,
    Abs,
    Min,
    Max,
    Random,
    ToInt,
    ToFloat,
}

fn builtin(name: &str) -> Option<(Bi, std::ops::RangeInclusive<usize>)> {
    Some(match name {
        "pinMode" => (Bi::PinMode, 2..=2),
        "digitalWrite" => (Bi::DigitalWrite, 2..=2),
        "digitalRead" => (Bi::DigitalRead, 1..=1),
        "analogRead" => (Bi::AnalogRead, 1..=1),
        "analogWrite" => (Bi::AnalogWrite, 2..=2),
        "delay" => (Bi::Delay, 1..=1),
        "delayMicroseconds" => (Bi::DelayUs, 1..=1),
        "millis" => (Bi::Millis, 0..=0),
        "micros" => (Bi::Micros, 0..=0),
        "Serial.begin" => (Bi::SerialBegin, 1..=2),
        "Serial.print" => (Bi::SerialPrint, 1..=2),
        "Serial.println" => (Bi::SerialPrintln, 0..=2),
        "map" => (Bi::Map, 5..=5),
        "constrain" => (Bi::Constrain, 3..=3),
        "abs" => (Bi::Abs, 1..=1),
        "min" => (Bi::Min, 2..=2),
        "max" => (Bi::Max, 2..=2),
        "random" => (Bi::Random, 1..=2),
        "int" => (Bi::ToInt, 1..=1),
        "float" => (Bi::ToFloat, 1..=1),
        _ => return None,
    })
}

/// The names Arduino gives to numbers.
fn constant(name: &str) -> Option<i64> {
    Some(match name {
        "HIGH" | "true" | "OUTPUT" => 1,
        "LOW" | "false" | "INPUT" => 0,
        "INPUT_PULLUP" => 2,
        "LED_BUILTIN" => 13,
        "A0" => 14,
        "A1" => 15,
        "A2" => 16,
        "A3" => 17,
        "A4" => 18,
        "A5" => 19,
        "A6" => 20,
        "A7" => 21,
        "DEC" => 10,
        "HEX" => 16,
        "BIN" => 2,
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub(super) enum Op {
    Push(Val),
    Load(Var),
    /// Pops the value, stores it as the variable's type and pushes it back (an assignment is a value).
    Store(Var),
    /// Pops the index, pushes the element.
    LoadIdx(Var),
    /// Pops the value and the index (below it), stores, pushes the value back.
    StoreIdx(Var),
    Bin(&'static str),
    Un(&'static str),
    Pop,
    Dup,
    Jmp(usize),
    Jz(usize),
    Call(usize, usize),
    Builtin(Bi, usize),
    Ret,
}

struct FuncInfo {
    entry: usize,
    params: Vec<(Ty, String)>,
    slots: usize,
}

/// A compiled sketch.
pub struct Code {
    pub(super) ops: Vec<Op>,
    pub(super) lines: Vec<u32>,
    pub(super) globals: usize,
    funcs: Vec<FuncInfo>,
}

impl Code {
    pub(super) fn slots_of(&self, f: usize) -> usize {
        self.funcs[f].slots
    }
    pub(super) fn params_of(&self, f: usize) -> usize {
        self.funcs[f].params.len()
    }
    pub(super) fn param_ty(&self, f: usize, k: usize) -> Ty {
        self.funcs[f].params[k].0
    }
}

type Res<T> = Result<T, (u32, String)>;

struct C<'a> {
    ops: Vec<Op>,
    lines: Vec<u32>,
    line: u32,
    globals: HashMap<String, Var>,
    nglobals: usize,
    scopes: Vec<HashMap<String, Var>>,
    nlocals: usize,
    max_locals: usize,
    names: HashMap<&'a str, usize>,
    funcs: &'a [Func],
    /// Open loops: where `continue` goes (patched later) and the jumps of `break`.
    loops: Vec<(Vec<usize>, Vec<usize>)>,
    ret: Ty,
}

impl<'a> C<'a> {
    fn emit(&mut self, op: Op) -> usize {
        self.ops.push(op);
        self.lines.push(self.line);
        self.ops.len() - 1
    }
    fn here(&self) -> usize {
        self.ops.len()
    }
    fn patch(&mut self, at: usize, to: usize) {
        match &mut self.ops[at] {
            Op::Jmp(t) | Op::Jz(t) => *t = to,
            _ => unreachable!(),
        }
    }
    fn var(&self, name: &str) -> Option<Var> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied()).or_else(|| self.globals.get(name).copied())
    }
    fn declare(&mut self, ty: Ty, name: &str, len: Option<usize>, global: bool) -> Var {
        let n = len.unwrap_or(1);
        if global {
            let v = Var { slot: Slot::G(self.nglobals), ty, len };
            self.nglobals += n;
            self.globals.insert(name.to_string(), v);
            v
        } else {
            let v = Var { slot: Slot::L(self.nlocals), ty, len };
            self.nlocals += n;
            self.max_locals = self.max_locals.max(self.nlocals);
            self.scopes.last_mut().unwrap().insert(name.to_string(), v);
            v
        }
    }

    fn decl(&mut self, ty: Ty, name: &str, len: Option<usize>, init: &[Expr], global: bool) -> Res<()> {
        if ty == Ty::Void {
            return Err((self.line, format!("変数 {name} の型に void は使えません")));
        }
        let v = self.declare(ty, name, len, global);
        match len {
            None => {
                match init.first() {
                    Some(e) => self.expr(e)?,
                    None => {
                        self.emit(Op::Push(Val::I(0)));
                    }
                }
                self.emit(Op::Store(v));
                self.emit(Op::Pop);
            }
            Some(n) => {
                if init.len() > n {
                    return Err((self.line, format!("配列 {name} の大きさ {n} より、最初の値が多くあります")));
                }
                for k in 0..n {
                    self.emit(Op::Push(Val::I(k as i64)));
                    match init.get(k) {
                        Some(e) => self.expr(e)?,
                        None => {
                            self.emit(Op::Push(Val::I(0)));
                        }
                    }
                    self.emit(Op::StoreIdx(v));
                    self.emit(Op::Pop);
                }
            }
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Res<()> {
        match s {
            Stmt::Decl { ty, name, len, init, line } => {
                self.line = *line;
                self.decl(*ty, name, *len, init, false)?;
            }
            Stmt::Expr(e, line) => {
                self.line = *line;
                self.expr(e)?;
                self.emit(Op::Pop);
            }
            Stmt::Block(v) => {
                self.scopes.push(HashMap::new());
                let mark = self.nlocals;
                for s in v {
                    self.stmt(s)?;
                }
                self.scopes.pop();
                self.nlocals = mark;
            }
            Stmt::If(c, a, b, line) => {
                self.line = *line;
                self.expr(c)?;
                let jz = self.emit(Op::Jz(0));
                self.stmt(a)?;
                match b {
                    Some(b) => {
                        let jmp = self.emit(Op::Jmp(0));
                        let at = self.here();
                        self.patch(jz, at);
                        self.stmt(b)?;
                        let at = self.here();
                        self.patch(jmp, at);
                    }
                    None => {
                        let at = self.here();
                        self.patch(jz, at);
                    }
                }
            }
            Stmt::While(c, body, line) => {
                self.line = *line;
                let top = self.here();
                self.expr(c)?;
                let jz = self.emit(Op::Jz(0));
                self.loops.push((vec![], vec![]));
                self.stmt(body)?;
                self.close_loop(top, top, jz);
            }
            Stmt::DoWhile(body, c, line) => {
                self.line = *line;
                let top = self.here();
                self.loops.push((vec![], vec![]));
                self.stmt(body)?;
                let cont = self.here();
                self.expr(c)?;
                let jz = self.emit(Op::Jz(0));
                self.close_loop(top, cont, jz);
            }
            Stmt::For(init, cond, step, body, line) => {
                self.line = *line;
                self.scopes.push(HashMap::new());
                let mark = self.nlocals;
                // the loop's own variables (`for (int i = 0; ...)`) live in the loop's scope, not in a block of their own
                match init.as_deref() {
                    Some(Stmt::Block(v)) => {
                        for s in v {
                            self.stmt(s)?;
                        }
                    }
                    Some(i) => self.stmt(i)?,
                    None => {}
                }
                let top = self.here();
                match cond {
                    Some(c) => self.expr(c)?,
                    None => {
                        self.emit(Op::Push(Val::I(1)));
                    }
                }
                let jz = self.emit(Op::Jz(0));
                self.loops.push((vec![], vec![]));
                self.stmt(body)?;
                let cont = self.here();
                if let Some(st) = step {
                    self.expr(st)?;
                    self.emit(Op::Pop);
                }
                self.close_loop(top, cont, jz);
                self.scopes.pop();
                self.nlocals = mark;
            }
            Stmt::Return(v, line) => {
                self.line = *line;
                match v {
                    Some(e) => {
                        self.expr(e)?;
                        if self.ret == Ty::Void {
                            return Err((*line, "void の関数は値を返せません".into()));
                        }
                        if self.ret == Ty::Float {
                            self.emit(Op::Builtin(Bi::ToFloat, 1));
                        } else {
                            self.emit(Op::Builtin(Bi::ToInt, 1));
                        }
                    }
                    None => {
                        self.emit(Op::Push(Val::I(0)));
                    }
                }
                self.emit(Op::Ret);
            }
            Stmt::Break(line) => {
                self.line = *line;
                let j = self.emit(Op::Jmp(0));
                self.loops.last_mut().ok_or((*line, "break はループの中で使います".to_string()))?.1.push(j);
            }
            Stmt::Continue(line) => {
                self.line = *line;
                let j = self.emit(Op::Jmp(0));
                self.loops.last_mut().ok_or((*line, "continue はループの中で使います".to_string()))?.0.push(j);
            }
        }
        Ok(())
    }

    /// The end of a loop: back to `top`, `continue` to `cont`, out of it from `jz` and the `break`s.
    fn close_loop(&mut self, top: usize, cont: usize, jz: usize) {
        self.emit(Op::Jmp(top));
        let end = self.here();
        self.patch(jz, end);
        let (conts, breaks) = self.loops.pop().unwrap();
        for c in conts {
            self.patch(c, cont);
        }
        for b in breaks {
            self.patch(b, end);
        }
    }

    fn lvalue(&mut self, e: &Expr) -> Res<(Var, bool)> {
        match e {
            Expr::Var(n) => {
                let v = self.var(n).ok_or((self.line, format!("変数 {n} が宣言されていません (int {n} = 0; のように)")))?;
                if v.len.is_some() {
                    return Err((self.line, format!("{n} は配列です。{n}[0] のように番号を付けます")));
                }
                Ok((v, false))
            }
            Expr::Index(n, i) => {
                let v = self.var(n).ok_or((self.line, format!("配列 {n} が宣言されていません")))?;
                if v.len.is_none() {
                    return Err((self.line, format!("{n} は配列ではありません")));
                }
                self.expr(i)?;
                Ok((v, true))
            }
            _ => Err((self.line, "代入の左は変数です".into())),
        }
    }

    fn expr(&mut self, e: &Expr) -> Res<()> {
        match e {
            Expr::Int(i) => {
                self.emit(Op::Push(Val::I(*i)));
            }
            Expr::Float(f) => {
                self.emit(Op::Push(Val::F(*f)));
            }
            Expr::Str(s) => {
                self.emit(Op::Push(Val::S(s.clone())));
            }
            Expr::Var(n) => match self.var(n) {
                Some(v) if v.len.is_none() => {
                    self.emit(Op::Load(v));
                }
                Some(_) => return Err((self.line, format!("{n} は配列です。{n}[0] のように番号を付けます"))),
                None => match constant(n) {
                    Some(c) => {
                        self.emit(Op::Push(Val::I(c)));
                    }
                    None => return Err((self.line, format!("「{n}」が何か分かりません (変数の宣言忘れか、書き間違い?)"))),
                },
            },
            Expr::Index(..) => {
                let (v, _) = self.lvalue(e)?;
                self.emit(Op::LoadIdx(v));
            }
            Expr::Unary(op, a) => {
                self.expr(a)?;
                self.emit(Op::Un(op));
            }
            Expr::Binary(op, a, b) if *op == "&&" || *op == "||" => {
                self.expr(a)?;
                self.emit(Op::Un("bool"));
                self.emit(Op::Dup);
                if *op == "||" {
                    self.emit(Op::Un("!"));
                }
                let jz = self.emit(Op::Jz(0));
                self.emit(Op::Pop);
                self.expr(b)?;
                self.emit(Op::Un("bool"));
                let at = self.here();
                self.patch(jz, at);
            }
            Expr::Binary(op, a, b) => {
                self.expr(a)?;
                self.expr(b)?;
                self.emit(Op::Bin(op));
            }
            Expr::Assign(op, target, value) => {
                let (v, indexed) = self.lvalue(target)?;
                if *op == "=" {
                    self.expr(value)?;
                } else {
                    if indexed {
                        self.emit(Op::Dup);
                        self.emit(Op::LoadIdx(v));
                    } else {
                        self.emit(Op::Load(v));
                    }
                    self.expr(value)?;
                    self.emit(Op::Bin(&op[..op.len() - 1]));
                }
                self.emit(if indexed { Op::StoreIdx(v) } else { Op::Store(v) });
            }
            Expr::PostInc(target, d) => {
                // the new value less the step: the value before
                let op = if *d > 0 { "+=" } else { "-=" };
                self.expr(&Expr::Assign(op, target.clone(), Box::new(Expr::Int(1))))?;
                self.emit(Op::Push(Val::I(*d)));
                self.emit(Op::Bin("-"));
            }
            Expr::Cond(c, a, b) => {
                self.expr(c)?;
                let jz = self.emit(Op::Jz(0));
                self.expr(a)?;
                let jmp = self.emit(Op::Jmp(0));
                let at = self.here();
                self.patch(jz, at);
                self.expr(b)?;
                let at = self.here();
                self.patch(jmp, at);
            }
            Expr::Call(name, args) => {
                if let Some(&f) = self.names.get(name.as_str()) {
                    let want = self.funcs[f].params.len();
                    if args.len() != want {
                        return Err((self.line, format!("{name} は引数が {want} 個の関数です ({} 個で呼んでいます)", args.len())));
                    }
                    for a in args {
                        self.expr(a)?;
                    }
                    self.emit(Op::Call(f, args.len()));
                } else if let Some((b, n)) = builtin(name) {
                    if !n.contains(&args.len()) {
                        return Err((self.line, format!("{name} の引数の数が違います")));
                    }
                    for a in args {
                        self.expr(a)?;
                    }
                    self.emit(Op::Builtin(b, args.len()));
                } else {
                    let hint = if name.starts_with("Serial.") { " (Serial は begin・print・println だけ使えます)" } else { " (関数の名前の書き間違い? このスケッチの動かし方で使えない関数かもしれません)" };
                    return Err((self.line, format!("関数「{name}」が見つかりません{hint}")));
                }
            }
        }
        Ok(())
    }
}

/// Compiles the program: the global variables first, then `setup()` once and `loop()` for ever; then the functions.
pub(super) fn compile(p: &Program) -> Res<Code> {
    let names: HashMap<&str, usize> = p.funcs.iter().enumerate().map(|(k, f)| (f.name.as_str(), k)).collect();
    for want in ["setup", "loop"] {
        if !names.contains_key(want) {
            return Err((1, format!("void {want}() {{ … }} がありません (Arduino のスケッチには setup と loop の 2 つが要ります)")));
        }
    }
    let mut c = C { ops: vec![], lines: vec![], line: 1, globals: HashMap::new(), nglobals: 0, scopes: vec![], nlocals: 0, max_locals: 0, names, funcs: &p.funcs, loops: vec![], ret: Ty::Void };
    for g in &p.globals {
        let Stmt::Decl { ty, name, len, init, line } = g else { continue };
        c.line = *line;
        c.decl(*ty, name, *len, init, true)?;
    }
    let (setup, lp) = (c.names["setup"], c.names["loop"]);
    c.emit(Op::Call(setup, 0));
    c.emit(Op::Pop);
    let top = c.here();
    c.emit(Op::Call(lp, 0));
    c.emit(Op::Pop);
    c.emit(Op::Jmp(top));
    let mut infos = vec![];
    for f in p.funcs.iter() {
        let entry = c.here();
        c.line = f.line;
        c.scopes = vec![HashMap::new()];
        c.nlocals = 0;
        c.max_locals = 0;
        c.ret = f.ret;
        for (ty, name) in &f.params {
            c.declare(*ty, name, None, false);
        }
        for s in &f.body {
            c.stmt(s)?;
        }
        // falling off the end returns 0
        c.emit(Op::Push(Val::I(0)));
        c.emit(Op::Ret);
        infos.push(FuncInfo { entry, params: f.params.clone(), slots: c.max_locals });
    }
    Ok(Code { ops: c.ops, lines: c.lines, globals: c.nglobals, funcs: infos })
}

impl Code {
    pub(super) fn entry(&self, f: usize) -> usize {
        self.funcs[f].entry
    }
}

/// Integer or float arithmetic, as C does it (an integer division truncates).
pub(super) fn binary(op: &str, a: &Val, b: &Val) -> Result<Val, String> {
    if let (Val::S(x), _) | (_, Val::S(x)) = (a, b) {
        let _ = x;
        return Err("文字列は計算に使えません (Serial.print で出すだけです)".into());
    }
    let float = matches!(a, Val::F(_)) || matches!(b, Val::F(_));
    let cmp = |r: bool| Val::I(r as i64);
    Ok(match op {
        "==" => cmp(a.f() == b.f()),
        "!=" => cmp(a.f() != b.f()),
        "<" => cmp(a.f() < b.f()),
        "<=" => cmp(a.f() <= b.f()),
        ">" => cmp(a.f() > b.f()),
        ">=" => cmp(a.f() >= b.f()),
        "&" => Val::I(a.i() & b.i()),
        "|" => Val::I(a.i() | b.i()),
        "^" => Val::I(a.i() ^ b.i()),
        "<<" => Val::I(a.i().wrapping_shl(b.i().clamp(0, 63) as u32)),
        ">>" => Val::I(a.i() >> b.i().clamp(0, 63)),
        _ if float => {
            let (x, y) = (a.f(), b.f());
            Val::F(match op {
                "+" => x + y,
                "-" => x - y,
                "*" => x * y,
                "/" => x / y,
                "%" => x % y,
                _ => return Err(format!("「{op}」は使えません")),
            })
        }
        _ => {
            let (x, y) = (a.i(), b.i());
            Val::I(match op {
                "+" => x.wrapping_add(y),
                "-" => x.wrapping_sub(y),
                "*" => x.wrapping_mul(y),
                "/" | "%" if y == 0 => return Err("0 で割りました".into()),
                "/" => x / y,
                "%" => x % y,
                _ => return Err(format!("「{op}」は使えません")),
            })
        }
    })
}

pub(super) fn unary(op: &str, a: &Val) -> Val {
    match op {
        "-" => match a {
            Val::F(f) => Val::F(-f),
            v => Val::I(-v.i()),
        },
        "!" => Val::I(!a.truthy() as i64),
        "~" => Val::I(!a.i()),
        _ => Val::I(a.truthy() as i64),
    }
}

pub(super) fn store(ty: Ty, v: Val) -> Val {
    Val::of(ty, v)
}

pub(super) fn val_i(v: &Val) -> i64 {
    v.i()
}

pub(super) fn val_f(v: &Val) -> f64 {
    v.f()
}

pub(super) fn truthy(v: &Val) -> bool {
    v.truthy()
}

/// A variable's place: (global?, index) and its type and array length.
pub(super) fn place(v: &Var) -> (bool, usize, Ty, Option<usize>) {
    match v.slot {
        Slot::G(i) => (true, i, v.ty, v.len),
        Slot::L(i) => (false, i, v.ty, v.len),
    }
}

