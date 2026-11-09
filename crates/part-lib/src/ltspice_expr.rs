//! The arithmetic of LTspice's `.param` lines and `{...}` values, enough to turn them into plain numbers when a sheet is
//! imported: numbers with SPICE suffixes (`4.7k`, `10u`, `1Meg`; `m` and `M` are both milli), `+ - * /`, powers `**` and `^`,
//! brackets, `pi`, the names of earlier parameters, and `sqrt exp ln log log10 abs pow min max sin cos tan atan`. `mc(x, tol)`
//! and `flat(x)` (Monte Carlo spreads) are taken at their nominal value.

use std::collections::HashMap;

/// A number with a SPICE suffix (`4.7k`, `1Meg`, `10µ`, `2.2uF`): the letters after the suffix are a unit and are ignored.
pub fn spice_number(s: &str) -> Option<f64> {
    let s = s.trim();
    // `4k7` / `2R2`: the letter stands for the decimal point (read the same way as a part's value)
    let rkm = crate::kicad_import::normalise_value(s);
    if rkm != s {
        return spice_number(&rkm);
    }
    let end = s
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || ((c == 'e' || c == 'E') && s[i + 1..].starts_with(|d: char| d.is_ascii_digit() || d == '-' || d == '+')) || ((c == '-' || c == '+') && (i == 0 || matches!(s.as_bytes()[i - 1], b'e' | b'E')))))
        .map_or(s.len(), |(i, _)| i);
    let x: f64 = s[..end].parse().ok()?;
    let rest = s[end..].to_ascii_lowercase().replace('µ', "u");
    let mult = if rest.starts_with("meg") {
        1e6
    } else {
        match rest.chars().next() {
            Some('t') => 1e12,
            Some('g') => 1e9,
            Some('k') => 1e3,
            Some('m') => 1e-3,
            Some('u') => 1e-6,
            Some('n') => 1e-9,
            Some('p') => 1e-12,
            Some('f') => 1e-15,
            _ => 1.0,
        }
    };
    Some(x * mult)
}

/// A number written the way circuitCad reads values: 6 significant digits and a suffix (`0.15915` → `159.15m`).
pub fn format_spice(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return format!("{v}");
    }
    let steps = [(1e12, "T"), (1e9, "G"), (1e6, "Meg"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "u"), (1e-9, "n"), (1e-12, "p"), (1e-15, "f")];
    let (scale, suffix) = steps.iter().find(|(s, _)| v.abs() >= *s * 0.999_999_5).copied().unwrap_or((1e-15, "f"));
    let x = v / scale;
    let digits = 5usize.saturating_sub(x.abs().log10().floor().max(0.0) as usize);
    let mut t = format!("{x:.digits$}");
    if t.contains('.') {
        t = t.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    format!("{t}{suffix}")
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    vars: &'a HashMap<String, f64>,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, t: &str) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(t.as_bytes()) {
            self.i += t.len();
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat("+") {
                v += self.term()?;
            } else if self.eat("-") {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            // `**` is a power, not two products
            self.ws();
            if self.s[self.i..].starts_with(b"**") {
                return Some(v);
            }
            if self.eat("*") {
                v *= self.power()?;
            } else if self.eat("/") {
                v /= self.power()?;
            } else {
                return Some(v);
            }
        }
    }
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.eat("**") || self.eat("^") {
            let e = self.power()?;
            return Some(base.powf(e));
        }
        Some(base)
    }
    fn unary(&mut self) -> Option<f64> {
        if self.eat("-") {
            return Some(-self.unary()?);
        }
        if self.eat("+") {
            return self.unary();
        }
        self.atom()
    }
    fn atom(&mut self) -> Option<f64> {
        self.ws();
        if self.eat("(") {
            let v = self.expr()?;
            return self.eat(")").then_some(v);
        }
        let start = self.i;
        let c = *self.s.get(self.i)?;
        if c.is_ascii_digit() || c == b'.' {
            // a number and its suffix letters
            while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'.' || ((self.s[self.i] == b'-' || self.s[self.i] == b'+') && matches!(self.s[self.i - 1], b'e' | b'E') && self.s[self.i - 2].is_ascii_digit())) {
                self.i += 1;
            }
            return spice_number(std::str::from_utf8(&self.s[start..self.i]).ok()?);
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
                self.i += 1;
            }
            let name = std::str::from_utf8(&self.s[start..self.i]).ok()?.to_ascii_lowercase();
            if self.eat("(") {
                let mut args = vec![self.expr()?];
                while self.eat(",") {
                    args.push(self.expr()?);
                }
                if !self.eat(")") {
                    return None;
                }
                let a = args[0];
                return Some(match (name.as_str(), args.len()) {
                    ("sqrt", 1) => a.sqrt(),
                    ("exp", 1) => a.exp(),
                    ("ln" | "log", 1) => a.ln(),
                    ("log10", 1) => a.log10(),
                    ("abs", 1) => a.abs(),
                    ("sin", 1) => a.sin(),
                    ("cos", 1) => a.cos(),
                    ("tan", 1) => a.tan(),
                    ("atan", 1) => a.atan(),
                    ("pow" | "pwr", 2) => a.powf(args[1]),
                    ("min", 2) => a.min(args[1]),
                    ("max", 2) => a.max(args[1]),
                    // Monte Carlo spreads: the nominal value
                    ("mc", 2) => a,
                    ("flat", 1) => 0.0,
                    _ => return None,
                });
            }
            return match name.as_str() {
                "pi" => Some(std::f64::consts::PI),
                _ => self.vars.get(&name).copied(),
            };
        }
        None
    }
}

/// The value of an expression, with the parameters known so far. None when it uses something unknown (a parameter not set, a
/// function not listed, a node voltage).
pub fn eval(text: &str, vars: &HashMap<String, f64>) -> Option<f64> {
    let t = text.trim().replace(['µ', 'μ'], "u");
    let t = t.as_str();
    let t = t.strip_prefix('{').and_then(|r| r.strip_suffix('}')).unwrap_or(t);
    let mut p = Parser { s: t.as_bytes(), i: 0, vars };
    let v = p.expr()?;
    p.ws();
    (p.i == p.s.len() && v.is_finite()).then_some(v)
}

/// The numeric parameters of `.param` lines, in order (a later one may use an earlier one). `a=1k b = {a*2}` both work; a
/// parameter whose value cannot be worked out is left out.
pub fn params(directives: &[String]) -> HashMap<String, f64> {
    let mut vars = HashMap::new();
    let mut all: Vec<(String, String)> = vec![];
    for d in directives {
        // `.step param X list 1 10 100` / `.step param X 0 20 1` / `.step dec param X 1 1k 3`: the first value stands for the sweep
        let w: Vec<&str> = d.split(';').next().unwrap_or("").split_whitespace().collect();
        if w.first().is_some_and(|x| x.eq_ignore_ascii_case(".step"))
            && let Some(i) = w.iter().position(|x| x.eq_ignore_ascii_case("param"))
            && let Some(name) = w.get(i + 1)
        {
            let first = if w.get(i + 2).is_some_and(|x| x.eq_ignore_ascii_case("list")) { w.get(i + 3) } else { w.get(i + 2) };
            if let Some(v) = first {
                all.push((name.to_string(), v.to_string()));
            }
        }
    }
    for d in directives {
        let lower = d.to_ascii_lowercase();
        // LTspice takes `.params` as well as `.param`
        let Some(rest) = lower.strip_prefix(".params ").map(|_| &d[8..]).or_else(|| lower.strip_prefix(".param").map(|_| &d[6..])) else { continue };
        // cut at the comment, then split `name = value` pairs (values may hold spaces inside brackets)
        let rest = rest.split(';').next().unwrap_or("");
        let mut pairs: Vec<(String, String)> = vec![];
        let chars: Vec<char> = rest.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let s = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[s..i].iter().collect();
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if name.is_empty() || i >= chars.len() || chars[i] != '=' {
                break;
            }
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            // the value runs to the next `name =` at bracket depth 0
            let vs = i;
            let mut depth = 0i32;
            while i < chars.len() {
                match chars[i] {
                    '(' | '{' => depth += 1,
                    ')' | '}' => depth -= 1,
                    c if c.is_whitespace() && depth == 0 => {
                        let mut j = i;
                        while j < chars.len() && chars[j].is_whitespace() {
                            j += 1;
                        }
                        let ns = j;
                        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                            j += 1;
                        }
                        while j < chars.len() && chars[j].is_whitespace() {
                            j += 1;
                        }
                        if j > ns && j < chars.len() && chars[j] == '=' {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            pairs.push((name, chars[vs..i].iter().collect::<String>().trim().to_string()));
        }
        all.extend(pairs);
    }
    // a parameter may use one set further down the sheet: go round until nothing more can be worked out
    loop {
        let before = vars.len();
        for (k, v) in &all {
            let key = k.to_ascii_lowercase();
            if !vars.contains_key(&key)
                && let Some(x) = eval(v, &vars)
            {
                vars.insert(key, x);
            }
        }
        if vars.len() == before {
            break;
        }
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_suffixes_and_arithmetic() {
        let v = HashMap::new();
        assert_eq!(spice_number("4.7k"), Some(4700.0));
        assert_eq!(spice_number("1Meg"), Some(1e6));
        assert!((spice_number("10µF").unwrap() - 10e-6).abs() < 1e-18);
        assert_eq!(eval("100µ", &v), Some(100.0 * 1e-6));
        assert_eq!(spice_number("2M"), Some(2e-3), "M is milli in SPICE");
        assert_eq!(spice_number("4k7"), Some(4700.0));
        assert_eq!(spice_number("1e-3"), Some(1e-3));
        assert_eq!(eval("{2*(3+4)}", &v), Some(14.0));
        assert_eq!(eval("2**3**2", &v), Some(512.0));
        assert!((eval("1/(2*pi*1k*100n)", &v).unwrap() - 1591.549).abs() < 1e-3);
        assert_eq!(eval("{mc(1n, tol)}", &HashMap::from([("tol".into(), 0.05)])), Some(1e-9));
        assert_eq!(eval("V(out)*2", &v), None);
        assert_eq!(eval("{unknown}", &v), None);
    }

    #[test]
    fn params_follow_each_other() {
        let p = params(&[".param f0=1e3 Q=0.5".into(), ".param R1=1k".into(), ".param L1 = R1*Q/(2*pi*f0)".into(), ".param C1 = 1/(L1*(2*pi*f0)**2)".into(), ".param tol=.05 ; +/- 5%".into()]);
        let l1 = p["l1"];
        assert!((l1 - 0.0795775).abs() < 1e-6, "{l1}");
        assert!((p["c1"] - 1.0 / (l1 * (2.0 * std::f64::consts::PI * 1e3f64).powi(2))).abs() < 1e-12);
        assert_eq!(p["tol"], 0.05);
        // used before it is set, and a swept one at its first value
        let q = params(&[".param b = a*2".into(), ".params a=3".into(), ".step param X list 1 10 100".into(), ".step param Y 5 20 1".into()]);
        assert_eq!((q["b"], q["x"], q["y"]), (6.0, 1.0, 5.0));
    }

    #[test]
    fn values_are_written_back_with_suffixes() {
        assert_eq!(format_spice(0.0795775), "79.5775m");
        assert_eq!(format_spice(4700.0), "4.7k");
        assert_eq!(format_spice(1.5e6), "1.5Meg");
        assert_eq!(format_spice(3.183e-7), "318.3n");
        assert_eq!(format_spice(5.0), "5");
    }
}
