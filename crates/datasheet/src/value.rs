//! Reads a number with its unit from a word clicked in a data sheet, and turns it into the unit a field of the form
//! uses (millimetres, volts, amperes, watts). Nothing is guessed silently: when the unit is not written or the
//! number is a range, the note says what was assumed.

/// A number as the data sheet writes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Value {
    pub number: f64,
    /// The unit written with it ("mm", "in", "mA" ...), if any.
    pub unit: Option<String>,
    /// It was a range ("0.30-0.50"): `number` is the middle.
    pub range: bool,
}

/// What a field of the form takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    /// Millimetres.
    Length,
    Voltage,
    Current,
    Power,
}

/// A number in the unit of the field, and what was assumed to get it.
#[derive(Clone, Debug, PartialEq)]
pub struct Converted {
    pub value: f64,
    pub note: Option<String>,
}

/// Words that follow a number but are not units ("0.65 BSC").
const NOT_UNITS: &[&str] = &["bsc", "typ", "max", "min", "ref", "nom", "basic", "typical"];

fn is_unit_char(c: char) -> bool {
    c.is_alphabetic() || "\"\u{2033}\u{2032}'\u{B5}\u{3BC}".contains(c)
}

/// Reads the number at the start of `text`: "1.27", ".050", "0.65BSC", "1.27±0.1" (the tolerance is left out),
/// "0.30-0.50" (a range), "5.7A", "10 mil". None when the text does not start with a number.
pub fn parse_value(text: &str) -> Option<Value> {
    let t = text.trim().trim_start_matches(['(', '\u{2248}', '~', '\u{7D04}']);
    let (sign, t) = match t.strip_prefix('-').or_else(|| t.strip_prefix('\u{2212}')) {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    let (n1, rest) = number(t)?;
    let mut rest = rest;
    let mut value = n1;
    let mut range = false;
    if let Some(r) = rest.strip_prefix('\u{B1}') {
        // "1.27±0.1": the tolerance is dropped
        rest = number(r).map_or(r, |(_, after)| after);
    } else if let Some(r) = rest.strip_prefix(['-', '\u{2013}', '\u{2014}', '~', '\u{301C}'])
        && let Some((n2, after)) = number(r)
    {
        value = (n1 + n2) / 2.0;
        range = true;
        rest = after;
    }
    let unit: String = rest.chars().take_while(|&c| is_unit_char(c)).collect();
    let unit = (!unit.is_empty() && !NOT_UNITS.contains(&unit.to_lowercase().as_str())).then_some(unit);
    Some(Value { number: sign * value, unit, range })
}

/// "12.5abc" -> (12.5, "abc"); ".05" works; None when there is no digit.
fn number(s: &str) -> Option<(f64, &str)> {
    let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, rest) = s.split_at(end);
    // "1.2.3" and "." are not numbers
    if num.matches('.').count() > 1 || !num.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((num.parse().ok()?, rest))
}

/// Is the word a unit that can follow a number ("mm", "in", "V", "mA" ...)? Brackets are ignored: "(mm)".
pub fn is_unit_word(word: &str) -> bool {
    let w = word.trim_matches(['(', ')', '[', ']']).to_lowercase();
    ["mm", "cm", "um", "\u{B5}m", "\u{3BC}m", "in", "inch", "inches", "mil", "mils", "\"", "\u{2033}"].contains(&w.as_str())
        || prefixed(&w, 'v').is_some()
        || prefixed(&w, 'a').is_some()
        || prefixed(&w, 'w').is_some()
}

/// "mv" -> 1e-3 for 'v'; "v" -> 1; None when it is not that unit.
fn prefixed(unit: &str, symbol: char) -> Option<f64> {
    let u = unit.to_lowercase();
    let prefix = u.strip_suffix(symbol)?;
    match prefix {
        "" => Some(1.0),
        "m" => Some(1e-3),
        "k" => Some(1e3),
        "u" | "\u{B5}" | "\u{3BC}" => Some(1e-6),
        _ => None,
    }
}

/// Turns the number into the unit of the field. Errors are written for the person who clicked the wrong number.
pub fn convert(v: &Value, q: Quantity) -> Result<Converted, String> {
    let wrong = |u: &str, what: &str| Err(format!("「{u}」は{what}の単位ではありません。別の数字をクリックしてください"));
    let mut notes: Vec<String> = vec![];
    let unit = v.unit.as_deref();
    let value = match q {
        Quantity::Length => match unit.map(str::to_lowercase).as_deref() {
            None => {
                notes.push("単位が書かれていないので mm として入れました。インチの値なら直してください".into());
                v.number
            }
            Some("mm") => v.number,
            Some("in" | "inch" | "inches" | "\"" | "\u{2033}") => {
                notes.push(format!("{} インチを mm に換算しました", v.number));
                v.number * 25.4
            }
            Some("mil" | "mils") => {
                notes.push(format!("{} mil を mm に換算しました", v.number));
                v.number * 0.0254
            }
            Some("um" | "\u{B5}m" | "\u{3BC}m") => {
                notes.push(format!("{} µm を mm に換算しました", v.number));
                v.number / 1000.0
            }
            Some("cm") => v.number * 10.0,
            Some(_) => return wrong(unit.unwrap_or(""), "長さ"),
        },
        Quantity::Voltage | Quantity::Current | Quantity::Power => {
            let (symbol, what) = match q {
                Quantity::Voltage => ('v', "電圧"),
                Quantity::Current => ('a', "電流"),
                _ => ('w', "電力"),
            };
            match unit {
                None => v.number,
                Some(u) => match prefixed(u, symbol) {
                    Some(k) => v.number * k,
                    None => return wrong(u, what),
                },
            }
        }
    };
    if v.range {
        notes.push("範囲 (最小〜最大) の真ん中を入れました。決めた値があれば直してください".into());
    }
    // 0.1 + 0.2 and the like: keep to the digits a data sheet has
    let value = (value * 1e6).round() / 1e6;
    Ok(Converted { value, note: (!notes.is_empty()).then(|| notes.join("。")) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Value {
        parse_value(s).unwrap_or_else(|| panic!("{s}"))
    }

    #[test]
    fn reads_numbers_as_data_sheets_write_them() {
        assert_eq!(p("1.27"), Value { number: 1.27, unit: None, range: false });
        assert_eq!(p(".050").number, 0.05);
        assert_eq!(p("0.65BSC"), Value { number: 0.65, unit: None, range: false });
        assert_eq!(p("1.27\u{B1}0.1").number, 1.27);
        assert_eq!(p("4.90mm").unit.as_deref(), Some("mm"));
        assert_eq!(p("5.7A").unit.as_deref(), Some("A"));
        assert_eq!(p("-55").number, -55.0);
        assert_eq!(p("(3.9)").number, 3.9);
        let r = p("0.30-0.50");
        assert!(r.range && (r.number - 0.4).abs() < 1e-9);
        assert_eq!(p("0.05\"").unit.as_deref(), Some("\""));
        for no in ["", "abc", "SOT-23", ".", "1.2.3x", "-"] {
            assert!(parse_value(no).is_none(), "{no}");
        }
    }

    #[test]
    fn converts_to_the_unit_of_the_field() {
        let mm = |s: &str| convert(&p(s), Quantity::Length);
        assert_eq!(mm("1.27mm").unwrap(), Converted { value: 1.27, note: None });
        // no unit: assumed mm, and said so
        assert!(mm("1.27").unwrap().note.unwrap().contains("mm"));
        // inches and mils
        let inch = mm(".050in").unwrap();
        assert_eq!(inch.value, 1.27);
        assert!(inch.note.unwrap().contains("換算"));
        assert_eq!(mm("50mil").unwrap().value, 1.27);
        assert_eq!(mm("500um").unwrap().value, 0.5);
        // a range is the middle, and said so
        assert!(mm("0.30-0.50mm").unwrap().note.unwrap().contains("範囲"));
        // the wrong kind of unit
        assert!(mm("30V").unwrap_err().contains("長さ"));
        // electrical
        let v = |s: &str| convert(&p(s), Quantity::Voltage);
        assert_eq!(v("30V").unwrap().value, 30.0);
        assert_eq!(v("500mV").unwrap().value, 0.5);
        assert_eq!(v("1kV").unwrap().value, 1000.0);
        assert_eq!(v("12").unwrap(), Converted { value: 12.0, note: None });
        assert!(v("5.7A").unwrap_err().contains("電圧"));
        assert_eq!(convert(&p("200mA"), Quantity::Current).unwrap().value, 0.2);
        assert_eq!(convert(&p("625mW"), Quantity::Power).unwrap().value, 0.625);
        assert!(convert(&p("1W"), Quantity::Current).is_err());
    }

    #[test]
    fn knows_unit_words() {
        for ok in ["mm", "(mm)", "in", "inch", "mil", "V", "mV", "A", "mA", "W", "mW", "\u{B5}m"] {
            assert!(is_unit_word(ok), "{ok}");
        }
        for no in ["BSC", "Typ", "pin", "max", "", "Vcc", "Dual"] {
            assert!(!is_unit_word(no), "{no}");
        }
    }
}
