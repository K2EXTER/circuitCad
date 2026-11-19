//! The sketch read into a tree: global variables, functions, and the statements and expressions of a small part of Arduino's C
//! (`int` / `long` / `float` / `bool` variables and arrays of them, `if` / `else`, `while`, `for`, `do`, `break`, `continue`,
//! `return`, the arithmetic, comparison, logic and bit operators, `?:`, calls).

use super::lex::Tok;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Int,
    Float,
    Void,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Str(String),
    Var(String),
    /// `name[index]`
    Index(String, Box<Expr>),
    /// A call: the name (`Serial.println` for a method), the arguments.
    Call(String, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    /// `target op= value` (`=` for a plain assignment); `++x` is `x += 1`.
    Assign(&'static str, Box<Expr>, Box<Expr>),
    /// `x++` / `x--`: the value before.
    PostInc(Box<Expr>, i64),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug)]
pub enum Stmt {
    /// A variable (or an array of `len` with its first values) and where it starts.
    Decl { ty: Ty, name: String, len: Option<usize>, init: Vec<Expr>, line: u32 },
    Expr(Expr, u32),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>, u32),
    While(Expr, Box<Stmt>, u32),
    DoWhile(Box<Stmt>, Expr, u32),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>, u32),
    Block(Vec<Stmt>),
    Return(Option<Expr>, u32),
    Break(u32),
    Continue(u32),
}

#[derive(Clone, Debug)]
pub struct Func {
    pub name: String,
    pub ret: Ty,
    pub params: Vec<(Ty, String)>,
    pub body: Vec<Stmt>,
    pub line: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub globals: Vec<Stmt>,
    pub funcs: Vec<Func>,
}

type Res<T> = Result<T, (u32, String)>;

struct P {
    t: Vec<(Tok, u32)>,
    i: usize,
}

/// The type names (with `unsigned`, `const`, `static` and `volatile` in front allowed).
fn type_of(word: &str) -> Option<Ty> {
    Some(match word {
        "int" | "long" | "short" | "byte" | "char" | "bool" | "boolean" | "word" | "uint8_t" | "uint16_t" | "uint32_t" | "int8_t" | "int16_t" | "int32_t" | "size_t" => Ty::Int,
        "float" | "double" => Ty::Float,
        "void" => Ty::Void,
        _ => return None,
    })
}

const QUALIFIERS: [&str; 5] = ["unsigned", "signed", "const", "static", "volatile"];

impl P {
    fn line(&self) -> u32 {
        self.t.get(self.i).or(self.t.last()).map_or(1, |x| x.1)
    }
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i).map(|x| &x.0)
    }
    fn peek_at(&self, k: usize) -> Option<&Tok> {
        self.t.get(self.i + k).map(|x| &x.0)
    }
    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
    }
    fn eat(&mut self, op: &str) -> bool {
        if self.is_op(op) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, op: &str) -> Res<()> {
        if self.eat(op) {
            Ok(())
        } else {
            let near = match self.peek() {
                Some(Tok::Ident(n)) => format!("「{n}」"),
                Some(Tok::Op(o)) => format!("「{o}」"),
                Some(Tok::Int(n)) => format!("「{n}」"),
                Some(_) => "ここ".into(),
                None => "スケッチの終わり".into(),
            };
            Err((self.line(), format!("「{op}」が要るところに {near} があります (; や括弧の閉じ忘れ?)")))
        }
    }
    fn ident(&mut self) -> Res<String> {
        match self.peek().cloned() {
            Some(Tok::Ident(n)) => {
                self.i += 1;
                Ok(n)
            }
            _ => Err((self.line(), "名前が要るところです".into())),
        }
    }

    /// A type at the cursor (after its qualifiers): the type and how many tokens it takes.
    fn type_here(&self) -> Option<(Ty, usize)> {
        let mut k = 0;
        let mut unsigned = false;
        while let Some(Tok::Ident(w)) = self.peek_at(k) {
            if QUALIFIERS.contains(&w.as_str()) {
                unsigned |= w == "unsigned";
                k += 1;
            } else {
                break;
            }
        }
        match self.peek_at(k) {
            Some(Tok::Ident(w)) => type_of(w).map(|t| (t, k + 1)),
            // `unsigned x` alone is an unsigned int
            _ if unsigned && k > 0 => Some((Ty::Int, k)),
            _ => None,
        }
    }

    fn program(&mut self) -> Res<Program> {
        let mut prog = Program::default();
        while self.i < self.t.len() {
            let line = self.line();
            let Some((ty, n)) = self.type_here() else {
                return Err((line, "関数か変数の宣言 (int x = 0; や void loop() { … }) が要るところです".into()));
            };
            self.i += n;
            let name = self.ident()?;
            if self.eat("(") {
                let mut params = vec![];
                if !self.eat(")") {
                    loop {
                        let Some((pt, pn)) = self.type_here() else {
                            if matches!(self.peek(), Some(Tok::Ident(w)) if w == "void") {
                                self.i += 1;
                                self.expect(")")?;
                                break;
                            }
                            return Err((self.line(), "引数の型 (int など) が要ります".into()));
                        };
                        self.i += pn;
                        if pt == Ty::Void && self.eat(")") {
                            break;
                        }
                        params.push((pt, self.ident()?));
                        if self.eat(")") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                if self.eat(";") {
                    continue; // a prototype
                }
                self.expect("{")?;
                let body = self.block_rest()?;
                prog.funcs.push(Func { name, ret: ty, params, body, line });
            } else {
                self.decl_rest(ty, name, line, &mut prog.globals)?;
            }
        }
        Ok(prog)
    }

    /// After `type name`: `= value`, `[n] = {...}`, more names after commas, and the `;`.
    fn decl_rest(&mut self, ty: Ty, first: String, line: u32, out: &mut Vec<Stmt>) -> Res<()> {
        let mut name = first;
        loop {
            let mut len = None;
            let mut init = vec![];
            if self.eat("[") {
                if let Some(Tok::Int(n)) = self.peek().cloned() {
                    self.i += 1;
                    len = Some(n.max(0) as usize);
                } else {
                    len = Some(0);
                }
                self.expect("]")?;
            }
            if self.eat("=") {
                if len.is_some() {
                    self.expect("{")?;
                    if !self.eat("}") {
                        loop {
                            init.push(self.expr()?);
                            if self.eat("}") {
                                break;
                            }
                            self.expect(",")?;
                        }
                    }
                    if len == Some(0) {
                        len = Some(init.len());
                    }
                } else {
                    init.push(self.expr()?);
                }
            }
            if len == Some(0) {
                return Err((line, format!("配列 {name} の大きさが決まりません ([5] か = {{1, 2}} で決めます)")));
            }
            out.push(Stmt::Decl { ty, name, len, init, line });
            if self.eat(";") {
                return Ok(());
            }
            self.expect(",")?;
            name = self.ident()?;
        }
    }

    fn block_rest(&mut self) -> Res<Vec<Stmt>> {
        let mut v = vec![];
        while !self.eat("}") {
            if self.peek().is_none() {
                return Err((self.line(), "{ に対応する } がありません".into()));
            }
            v.push(self.stmt()?);
        }
        Ok(v)
    }

    fn stmt(&mut self) -> Res<Stmt> {
        let line = self.line();
        if self.eat("{") {
            return Ok(Stmt::Block(self.block_rest()?));
        }
        if self.eat(";") {
            return Ok(Stmt::Block(vec![]));
        }
        if let Some((ty, n)) = self.type_here() {
            self.i += n;
            let name = self.ident()?;
            let mut v = vec![];
            self.decl_rest(ty, name, line, &mut v)?;
            return Ok(if v.len() == 1 { v.pop().unwrap() } else { Stmt::Block(v) });
        }
        if let Some(Tok::Ident(w)) = self.peek().cloned() {
            match w.as_str() {
                "if" => {
                    self.i += 1;
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    let then = Box::new(self.stmt()?);
                    let els = if matches!(self.peek(), Some(Tok::Ident(e)) if e == "else") {
                        self.i += 1;
                        Some(Box::new(self.stmt()?))
                    } else {
                        None
                    };
                    return Ok(Stmt::If(c, then, els, line));
                }
                "while" => {
                    self.i += 1;
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    return Ok(Stmt::While(c, Box::new(self.stmt()?), line));
                }
                "do" => {
                    self.i += 1;
                    let body = Box::new(self.stmt()?);
                    if !matches!(self.peek(), Some(Tok::Ident(e)) if e == "while") {
                        return Err((self.line(), "do { … } の後に while (条件); が要ります".into()));
                    }
                    self.i += 1;
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    self.expect(";")?;
                    return Ok(Stmt::DoWhile(body, c, line));
                }
                "for" => {
                    self.i += 1;
                    self.expect("(")?;
                    let init = if self.eat(";") {
                        None
                    } else if let Some((ty, n)) = self.type_here() {
                        self.i += n;
                        let name = self.ident()?;
                        let mut v = vec![];
                        self.decl_rest(ty, name, line, &mut v)?;
                        Some(Box::new(Stmt::Block(v)))
                    } else {
                        let e = self.expr()?;
                        self.expect(";")?;
                        Some(Box::new(Stmt::Expr(e, line)))
                    };
                    let cond = if self.is_op(";") { None } else { Some(self.expr()?) };
                    self.expect(";")?;
                    let step = if self.is_op(")") { None } else { Some(self.expr()?) };
                    self.expect(")")?;
                    return Ok(Stmt::For(init, cond, step, Box::new(self.stmt()?), line));
                }
                "return" => {
                    self.i += 1;
                    let v = if self.is_op(";") { None } else { Some(self.expr()?) };
                    self.expect(";")?;
                    return Ok(Stmt::Return(v, line));
                }
                "break" => {
                    self.i += 1;
                    self.expect(";")?;
                    return Ok(Stmt::Break(line));
                }
                "continue" => {
                    self.i += 1;
                    self.expect(";")?;
                    return Ok(Stmt::Continue(line));
                }
                "switch" | "goto" | "struct" | "class" | "String" => {
                    return Err((line, format!("「{w}」は、このスケッチの動かし方ではまだ使えません (if / else で書きかえてください)")));
                }
                _ => {}
            }
        }
        let e = self.expr()?;
        self.expect(";")?;
        Ok(Stmt::Expr(e, line))
    }

    fn expr(&mut self) -> Res<Expr> {
        let lhs = self.cond()?;
        for op in ["=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="] {
            if self.eat(op) {
                if !matches!(lhs, Expr::Var(_) | Expr::Index(..)) {
                    return Err((self.line(), format!("「{op}」の左は変数です")));
                }
                let rhs = self.expr()?;
                return Ok(Expr::Assign(op, Box::new(lhs), Box::new(rhs)));
            }
        }
        Ok(lhs)
    }

    fn cond(&mut self) -> Res<Expr> {
        let c = self.binary(0)?;
        if self.eat("?") {
            let a = self.expr()?;
            self.expect(":")?;
            let b = self.cond()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    fn binary(&mut self, level: usize) -> Res<Expr> {
        const LEVELS: [&[&str]; 10] = [&["||"], &["&&"], &["|"], &["^"], &["&"], &["==", "!="], &["<", "<=", ">", ">="], &["<<", ">>"], &["+", "-"], &["*", "/", "%"]];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        'outer: loop {
            for op in LEVELS[level] {
                // `&` alone, not the start of `&&` (the lexer already made `&&` one token)
                if self.eat(op) {
                    let rhs = self.binary(level + 1)?;
                    lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
                    continue 'outer;
                }
            }
            return Ok(lhs);
        }
    }

    fn unary(&mut self) -> Res<Expr> {
        for op in ["-", "!", "~", "+"] {
            if self.eat(op) {
                let e = self.unary()?;
                return Ok(if op == "+" { e } else { Expr::Unary(op, Box::new(e)) });
            }
        }
        for (op, d) in [("++", "+="), ("--", "-=")] {
            if self.eat(op) {
                let e = self.unary()?;
                return Ok(Expr::Assign(d, Box::new(e), Box::new(Expr::Int(1))));
            }
        }
        // a cast: (int) x, (float) x
        if self.is_op("(")
            && let Some(Tok::Ident(w)) = self.peek_at(1)
            && let Some(t) = type_of(w)
            && matches!(self.peek_at(2), Some(Tok::Op(")")))
        {
            self.i += 3;
            let e = self.unary()?;
            return Ok(Expr::Call(if t == Ty::Float { "float".into() } else { "int".into() }, vec![e]));
        }
        let e = self.postfix()?;
        Ok(e)
    }

    fn postfix(&mut self) -> Res<Expr> {
        let e = self.primary()?;
        if self.eat("++") {
            return Ok(Expr::PostInc(Box::new(e), 1));
        }
        if self.eat("--") {
            return Ok(Expr::PostInc(Box::new(e), -1));
        }
        Ok(e)
    }

    fn primary(&mut self) -> Res<Expr> {
        let line = self.line();
        match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.i += 1;
                Ok(Expr::Int(n))
            }
            Some(Tok::Float(x)) => {
                self.i += 1;
                Ok(Expr::Float(x))
            }
            Some(Tok::Str(s)) => {
                self.i += 1;
                Ok(Expr::Str(s))
            }
            Some(Tok::Op("(")) => {
                self.i += 1;
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(Tok::Ident(mut name)) => {
                self.i += 1;
                // Serial.println, and the like: one name
                if self.eat(".") {
                    name = format!("{name}.{}", self.ident()?);
                }
                if self.eat("(") {
                    let mut args = vec![];
                    if !self.eat(")") {
                        loop {
                            args.push(self.expr()?);
                            if self.eat(")") {
                                break;
                            }
                            self.expect(",")?;
                        }
                    }
                    return Ok(Expr::Call(name, args));
                }
                if self.eat("[") {
                    let i = self.expr()?;
                    self.expect("]")?;
                    return Ok(Expr::Index(name, Box::new(i)));
                }
                Ok(Expr::Var(name))
            }
            _ => Err((line, "値 (数・変数・関数の呼び出し) が要るところです".into())),
        }
    }
}

/// Reads the tokens, `#define`s replaced by their values, into a program.
pub fn parse(tokens: Vec<(Tok, u32)>) -> Res<Program> {
    // #define: the name stands for its tokens from there on
    let mut defs: Vec<(String, Vec<(Tok, u32)>)> = vec![];
    let mut t = vec![];
    for (tok, line) in tokens {
        match tok {
            Tok::Define(name, value) => defs.push((name, value)),
            Tok::Ident(ref n) if defs.iter().any(|(d, _)| d == n) => {
                let (_, v) = defs.iter().rev().find(|(d, _)| d == n).unwrap();
                t.extend(v.iter().map(|(x, _)| (x.clone(), line)));
            }
            other => t.push((other, line)),
        }
    }
    P { t, i: 0 }.program()
}
