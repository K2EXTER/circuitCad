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

/// The inverse of [`parse_value`]: `4700.0` → `4.7k`, `1e-7` → `100n`. Three significant digits.
pub fn format_value(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    let a = v.abs();
    let (scale, suffix) = [(1e12, "T"), (1e9, "G"), (1e6, "Meg"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "u"), (1e-9, "n"), (1e-12, "p"), (1e-15, "f")]
        .into_iter()
        .find(|(sc, _)| a >= sc * 0.9995)
        .unwrap_or((1e-15, "f"));
    let mut t = format!("{:.2}", v / scale);
    if t.contains('.') {
        t = t.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    format!("{t}{suffix}")
}

/// A part value with optional parasitics: `100n`, `100n ESR=20m ESL=5n`, `10u DCR=0.1`.
/// Returns the main value and the (upper-case key, value) pairs.
pub fn parse_part_value(s: &str) -> Result<(f64, Vec<(String, f64)>), String> {
    let mut it = s.split_whitespace();
    let first = it.next().ok_or("値が空です")?;
    let main = parse_value(first).ok_or_else(|| format!("値「{first}」を数値にできません"))?;
    let mut extra = vec![];
    for tok in it {
        let (k, v) = tok.split_once('=').ok_or_else(|| format!("「{tok}」は 名前=値 の形式ではありません (例: ESR=20m)"))?;
        let val = parse_value(v).ok_or_else(|| format!("{k} の値「{v}」を数値にできません"))?;
        if val < 0.0 {
            return Err(format!("{k} は0以上にしてください"));
        }
        extra.push((k.to_ascii_uppercase(), val));
    }
    Ok((main, extra))
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
    fn format_value_round_trips() {
        assert_eq!(format_value(4700.0), "4.7k");
        assert_eq!(format_value(1e-7), "100n");
        assert_eq!(format_value(1e6), "1Meg");
        assert_eq!(format_value(0.0022), "2.2m");
        assert_eq!(format_value(999.9), "1k");
        for v in [1.0, 47.0, 330.0, 4.7e3, 2.2e-6, 1e-12, 8.2e7] {
            let back = parse_value(&format_value(v)).unwrap();
            assert!((back - v).abs() <= 0.005 * v, "{v} -> {back}");
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_value("").is_none());
        assert!(parse_value("abc").is_none());
        assert!(parse_value("12 3").is_none());
        assert!(parse_value("1k2").is_none());
    }

    #[test]
    fn part_values_with_parasitics() {
        let (c0, p0) = parse_part_value("100n").unwrap();
        assert!((c0 - 100e-9).abs() < 1e-21 && p0.is_empty());
        let (c, p) = parse_part_value("10u ESR=20m ESL=5n").unwrap();
        assert!((c - 10e-6).abs() < 1e-18);
        assert_eq!(p.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["ESR", "ESL"]);
        assert!((p[0].1 - 0.02).abs() < 1e-15 && (p[1].1 - 5e-9).abs() < 1e-21);
        assert!(parse_part_value("10u ESR").is_err());
        assert!(parse_part_value("10u ESR=x").is_err());
        assert!(parse_part_value("10u ESR=-1").is_err());
        assert!(parse_part_value("").is_err());
    }
}
