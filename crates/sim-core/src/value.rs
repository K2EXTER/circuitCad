//! SPICE-style numbers: `10k`, `4.7u`, `1Meg`, `100nF`, `1e-3`, `2.2K`.

/// Parse a number with an optional SPICE magnitude suffix. Trailing unit letters are ignored
/// (`10kohm`, `100nF`), as in SPICE. Note that a bare `F` therefore means femto.
pub fn parse_value(s: &str) -> Option<f64> {
    let t = s.trim();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
        i += 1;
    }
    if i == digits_start {
        return None;
    }
    // exponent only when `e` is followed by a (signed) digit, so `1meg` / `5e` are not eaten
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    let num: f64 = t[..i].parse().ok()?;
    let rest = t[i..].trim().to_lowercase();
    let mult = if rest.starts_with("meg") {
        1e6
    } else if rest.starts_with("mil") {
        25.4e-6
    } else {
        match rest.chars().next() {
            Some('t') => 1e12,
            Some('g') => 1e9,
            Some('k') => 1e3,
            Some('m') => 1e-3,
            Some('u') | Some('µ') | Some('μ') => 1e-6,
            Some('n') => 1e-9,
            Some('p') => 1e-12,
            Some('f') => 1e-15,
            _ => 1.0,
        }
    };
    if !rest.chars().all(|c| c.is_alphabetic() || c == 'Ω' || c == 'ω') {
        return None;
    }
    Some(num * mult)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffixes() {
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs().max(1.0);
        assert!(close(parse_value("10k").unwrap(), 10e3));
        assert!(close(parse_value("4.7u").unwrap(), 4.7e-6));
        assert!(close(parse_value("1Meg").unwrap(), 1e6));
        assert!(close(parse_value("1m").unwrap(), 1e-3));
        assert!(close(parse_value("100nF").unwrap(), 100e-9));
        assert!(close(parse_value("2.2K").unwrap(), 2200.0));
        assert!(close(parse_value("1e-3").unwrap(), 1e-3));
        assert!(close(parse_value("-5").unwrap(), -5.0));
        assert!(close(parse_value("3.3kohm").unwrap(), 3300.0));
        assert!(close(parse_value("  9 ").unwrap(), 9.0));
        assert!(close(parse_value("1.5e3k").unwrap(), 1.5e6));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_value("").is_none());
        assert!(parse_value("abc").is_none());
        assert!(parse_value("12 3").is_none());
        assert!(parse_value("1k2").is_none());
    }
}
