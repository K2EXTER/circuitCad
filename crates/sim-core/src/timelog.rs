//! A meter's log over time (candidate L2): a CSV or a pasted log whose rows carry a time (a clock `10:00:01`, an ISO stamp, or seconds
//! in the first column) and readings, read with the names of its columns from the header line, and summed up per column: the mean,
//! the spread, the drift per hour (a straight line fitted against time) and, against a temperature column, the temperature
//! coefficient (a straight line fitted against the temperature). The app draws it on its own time axis (`schematic/time_log.rs`);
//! `ccad timelog` answers the same numbers.

use crate::value::parse_value;

/// A log: the names of the value columns, the time of each row (s from the first), and the value columns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeLog {
    pub names: Vec<String>,
    pub t: Vec<f64>,
    pub cols: Vec<Vec<f64>>,
    /// When the first row was taken, for a log with clock times: seconds of the day, plus whole days since 1970 when the cells carry
    /// a date (`2026-11-21T10:00:01`). None for a log timed in seconds. A log added on to it later is put at its own time (`append`).
    pub start: Option<f64>,
}

/// One column summed up.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnStats {
    pub name: String,
    pub n: usize,
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    /// The standard deviation round the mean.
    pub std: f64,
    /// The slope of a straight line fitted against time, per hour.
    pub drift_per_hour: f64,
    /// Against the temperature column (when one is chosen and it is not this one): the slope per °C, and as ppm of the mean per °C.
    pub tempco: Option<f64>,
    pub tempco_ppm: Option<f64>,
}

/// The date before a clock time's T (`2026-11-19T…`) as days since 1970-01-01.
fn date_days(cell: &str) -> Option<f64> {
    let (date, _) = cell.split_once('T')?;
    let p: Vec<i64> = date.split('-').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let [y, m, d] = p[..] else { return None };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // days from civil (Howard Hinnant's algorithm)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) as f64)
}

/// A clock time in a cell as seconds of the day: `10:00:01`, `10:00:01.250`, `2026-11-19T10:00:01Z` (the time after the T).
fn clock_seconds(cell: &str) -> Option<f64> {
    let t = cell.rsplit('T').next()?;
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let (h, m, s) = (parts[0].parse::<f64>().ok()?, parts[1].parse::<f64>().ok()?, parts[2].trim_end_matches('Z').parse::<f64>().ok()?);
    (h < 24.0 && m < 60.0 && s < 61.0).then_some(h * 3600.0 + m * 60.0 + s)
}

fn cells(line: &str) -> Vec<&str> {
    let sep: &[char] = if line.contains(',') { &[','] } else if line.contains('\t') { &['\t'] } else if line.contains(';') { &[';'] } else { &[' '] };
    line.split(sep).map(|c| c.trim().trim_matches('"')).filter(|c| !c.is_empty()).collect()
}

fn number(c: &str) -> Option<f64> {
    c.parse::<f64>().ok().or_else(|| parse_value(c)).filter(|v| v.is_finite())
}

/// Reads a log. A row is a line with a clock time (the time; the other cells that are numbers are the values, words such as a
/// date or a unit are left out) or a line of numbers only (the first is the time in seconds). The header is the last line before
/// the first row that is not a row: its cells name the columns (the time's own name left out); without one the columns are named
/// 列 1, 列 2 ... Rows with another number of values than the most common are left out (a footer, a settings line).
pub fn read_log(text: &str) -> Result<TimeLog, String> {
    let mut header: Option<Vec<String>> = None;
    // each row: its time, its values, and where in the line the values were (to name them from the header), and the line's cells
    let mut rows: Vec<(f64, Vec<f64>, Vec<usize>, usize)> = vec![];
    let mut clock_start: Option<f64> = None;
    let mut clock_prev: Option<f64> = None;
    let mut first_date: Option<f64> = None;
    let mut day = 0.0;
    for line in text.lines() {
        let cs = cells(line);
        if cs.is_empty() {
            continue;
        }
        if let Some((k, clock)) = cs.iter().enumerate().find_map(|(k, c)| clock_seconds(c).map(|s| (k, s))) {
            if clock_start.is_none() {
                first_date = date_days(cs[k]);
            }
            // past midnight the clock starts again: add a day
            if clock_prev.is_some_and(|p| clock + 3600.0 < p) {
                day += 86400.0;
            }
            clock_prev = Some(clock);
            let at = clock + day;
            let start = *clock_start.get_or_insert(at);
            let found: Vec<(usize, f64)> = cs.iter().enumerate().filter(|(j, c)| *j != k && !(c.contains('-') && !c.starts_with('-') && c.len() > 3)).filter_map(|(j, c)| number(c).map(|v| (j, v))).collect();
            if !found.is_empty() {
                rows.push((at - start, found.iter().map(|x| x.1).collect(), found.iter().map(|x| x.0).collect(), cs.len()));
            }
            continue;
        }
        let nums: Option<Vec<f64>> = cs.iter().map(|c| number(c)).collect();
        match nums {
            Some(n) if n.len() >= 2 => rows.push((n[0], n[1..].to_vec(), (1..n.len()).collect(), n.len())),
            _ if rows.is_empty() => header = Some(cs.iter().map(|c| c.to_string()).collect()),
            _ => {}
        }
    }
    let mut counts: Vec<(usize, usize)> = vec![];
    for (_, r, _, _) in &rows {
        match counts.iter_mut().find(|(n, _)| *n == r.len()) {
            Some((_, c)) => *c += 1,
            None => counts.push((r.len(), 1)),
        }
    }
    let Some(&(width, _)) = counts.iter().max_by_key(|(n, c)| (*c, *n)) else {
        return Err("記録の行が見つかりません。1 行に「時刻 (10:00:01 か秒), 値, 値 …」を並べてください".into());
    };
    rows.retain(|(_, r, _, _)| r.len() == width);
    if rows.len() < 2 {
        return Err("記録の行が 2 つ以上要ります".into());
    }
    // the header's names of the values: the cells over the values when the header has a cell for each cell of the line, else its
    // last `width` cells (the time's name comes first)
    let (_, _, at, cells_n) = &rows[0];
    let names: Vec<String> = match header {
        Some(h) if h.len() == *cells_n => at.iter().map(|p| h[*p].clone()).collect(),
        Some(h) if h.len() >= width => h[h.len() - width..].to_vec(),
        _ => (1..=width).map(|k| format!("列 {k}")).collect(),
    };
    let t: Vec<f64> = rows.iter().map(|r| r.0).collect();
    let cols: Vec<Vec<f64>> = (0..width).map(|k| rows.iter().map(|r| r.1[k]).collect()).collect();
    let start = clock_start.map(|c| c + first_date.unwrap_or(0.0) * 86400.0);
    Ok(TimeLog { names, t, cols, start })
}

/// Log `b` added on after log `a` (the same columns by name, in any order): with clock times on both, at its own time (a log
/// without dates that starts at an earlier time of day is taken as a later day); else right after `a`'s last row, one of `a`'s
/// usual steps on.
pub fn append(a: &TimeLog, b: &TimeLog) -> Result<TimeLog, String> {
    let map: Vec<usize> = a
        .names
        .iter()
        .map(|n| b.names.iter().position(|m| m == n).ok_or_else(|| format!("足す記録に列「{n}」がありません (列: {})。同じ測り方の記録を足します", b.names.join(", "))))
        .collect::<Result<_, _>>()?;
    let last = a.t.last().copied().unwrap_or(0.0);
    let offset = match (a.start, b.start) {
        (Some(sa), Some(sb)) => {
            let mut o = sb - sa;
            while o <= last {
                o += 86400.0;
            }
            o
        }
        _ => {
            let mut steps: Vec<f64> = a.t.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 0.0).collect();
            steps.sort_by(f64::total_cmp);
            let step = steps.get(steps.len() / 2).copied().unwrap_or(1.0);
            last + step - b.t.first().copied().unwrap_or(0.0)
        }
    };
    let mut out = a.clone();
    out.t.extend(b.t.iter().map(|t| t + offset));
    for (k, j) in map.iter().enumerate() {
        out.cols[k].extend(b.cols[*j].iter().copied());
    }
    Ok(out)
}

/// The slope and intercept of the straight line through (x, y) by least squares (None when x does not vary).
pub fn fit_line(x: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    let n = x.len().min(y.len()) as f64;
    if n < 2.0 {
        return None;
    }
    let (mx, my) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
    let sxx: f64 = x.iter().map(|a| (a - mx).powi(2)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let sxy: f64 = x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)).sum();
    let k = sxy / sxx;
    Some((k, my - k * mx))
}

/// The slopes (per x, per z) of the plane y = a + b·x + c·z by least squares; None when x and z move together (no way to tell their
/// shares apart: |correlation| above 0.98) or do not vary.
pub fn fit_plane(x: &[f64], z: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    let n = x.len().min(z.len()).min(y.len());
    if n < 3 {
        return None;
    }
    let mean = |v: &[f64]| v[..n].iter().sum::<f64>() / n as f64;
    let (mx, mz, my) = (mean(x), mean(z), mean(y));
    let (mut sxx, mut szz, mut sxz, mut sxy, mut szy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..n {
        let (a, b, c) = (x[i] - mx, z[i] - mz, y[i] - my);
        sxx += a * a;
        szz += b * b;
        sxz += a * b;
        sxy += a * c;
        szy += b * c;
    }
    if sxx <= 0.0 || szz <= 0.0 || sxz * sxz / (sxx * szz) > 0.98 * 0.98 {
        return None;
    }
    let det = sxx * szz - sxz * sxz;
    Some(((sxy * szz - szy * sxz) / det, (szy * sxx - sxy * sxz) / det))
}

/// The columns summed up; `temp` is the index of the temperature column, if one is chosen.
pub fn stats(log: &TimeLog, temp: Option<usize>) -> Vec<ColumnStats> {
    let hours: Vec<f64> = log.t.iter().map(|s| s / 3600.0).collect();
    log.cols
        .iter()
        .enumerate()
        .map(|(k, c)| {
            let n = c.len();
            let mean = c.iter().sum::<f64>() / n as f64;
            let (min, max) = c.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
            let std = (c.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
            // with a temperature column, time and temperature are fitted together (the room warms while the part drifts: a line
            // against each alone would give each the other's share); when the two move together the lines are fitted apart
            let both = temp.filter(|t| *t != k).and_then(|t| fit_plane(&hours, &log.cols[t], c));
            let drift_per_hour = both.map(|x| x.0).or_else(|| fit_line(&hours, c).map(|x| x.0)).unwrap_or(0.0);
            let tempco = both.map(|x| x.1).or_else(|| temp.filter(|t| *t != k).and_then(|t| fit_line(&log.cols[t], c)).map(|x| x.0));
            let tempco_ppm = tempco.filter(|_| mean != 0.0).map(|tc| tc / mean.abs() * 1e6);
            ColumnStats { name: log.names[k].clone(), n, mean, min, max, std, drift_per_hour, tempco, tempco_ppm }
        })
        .collect()
}

/// The column a name picks: its exact name (any case), else the first whose name contains it, else a number from 1.
pub fn column(log: &TimeLog, name: &str) -> Option<usize> {
    let low = name.to_lowercase();
    log.names
        .iter()
        .position(|n| n.to_lowercase() == low)
        .or_else(|| log.names.iter().position(|n| n.to_lowercase().contains(&low)))
        .or_else(|| name.parse::<usize>().ok().filter(|k| *k >= 1 && *k <= log.names.len()).map(|k| k - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DMM's log with a date, a clock, a unit and a temperature: the columns are named from the header, the time counts from the
    /// first row (across midnight), and the drift and the temperature coefficient come out of the straight lines.
    #[test]
    fn a_meters_log_is_read_by_name_and_its_drift_and_tempco_found() {
        let text = "\
# ladder lab DMM log
date,time,reading,unit,temp
2026-11-20,23:30:00,1.000000,V,20.0
2026-11-21,00:00:00,1.000010,V,21.0
2026-11-21,00:30:00,1.000020,V,22.0
2026-11-21,01:30:00,1.000040,V,24.0
";
        let log = read_log(text).unwrap();
        assert_eq!(log.names, ["reading", "temp"]);
        assert_eq!(log.t, [0.0, 1800.0, 3600.0, 7200.0]);
        let k = column(&log, "READING").unwrap();
        let t = column(&log, "temp").unwrap();
        let s = stats(&log, Some(t));
        // 40 uV in 2 h: 20 uV per hour; 10 uV per degree: 10 ppm/°C of 1 V
        assert!((s[k].drift_per_hour - 20e-6).abs() < 1e-9, "{:?}", s[k]);
        assert!((s[k].tempco.unwrap() - 10e-6).abs() < 1e-9 && (s[k].tempco_ppm.unwrap() - 10.0).abs() < 0.01, "{:?}", s[k]);
        assert!((s[k].max - s[k].min - 40e-6).abs() < 1e-12);
        assert!(s[t].tempco.is_none(), "the temperature against itself");
        assert_eq!(column(&log, "2"), Some(1));
    }

    /// A part that drifts 3 uV an hour and moves 8 uV per degree, in a room that warms and then cools a little: fitted together, the two
    /// come apart (a line against time alone would mix in the room's warming).
    #[test]
    fn drift_and_temperature_are_told_apart() {
        let mut text = String::from("t,v,temp
");
        for k in 0..=48 {
            let h = k as f64 / 6.0;
            let temp = 22.0 + 1.5 * (h / 8.0 * std::f64::consts::PI * 0.75).sin();
            text += &format!("{},{},{}
", h * 3600.0, 1.0 + 3e-6 * h + 8e-6 * (temp - 22.0), temp);
        }
        let log = read_log(&text).unwrap();
        let s = &stats(&log, Some(1))[0];
        assert!((s.drift_per_hour - 3e-6).abs() < 1e-10 && (s.tempco.unwrap() - 8e-6).abs() < 1e-10, "{s:?}");
        let alone = fit_line(&log.t.iter().map(|t| t / 3600.0).collect::<Vec<_>>(), &log.cols[0]).unwrap().0;
        assert!((alone - 3e-6).abs() > 1e-7, "time alone mixes in the warming: {alone}");
    }

    /// Numbers only: the first column is the time in seconds; a header without the time's name still names the values.
    #[test]
    fn a_table_of_seconds_and_values_is_read() {
        let log = read_log("t,V(OUT)\n0,1.5\n10,1.6\n20,1.7\n").unwrap();
        assert_eq!(log.names, ["V(OUT)"]);
        assert_eq!(log.t, [0.0, 10.0, 20.0]);
        let log = read_log("0 1 2\n1 1 2\n").unwrap();
        assert_eq!(log.names, ["列 1", "列 2"]);
        assert!(read_log("hello\n").is_err());
    }

    #[test]
    fn a_log_is_added_on_at_its_own_time() {
        // the next morning (no date: an earlier time of day is the next day), its columns in another order
        let a = read_log("time,v\n23:00:00,1.0\n23:30:00,1.1\n").unwrap();
        let b = read_log("time,temp,v\n07:00:00,22,1.2\n07:30:00,22,1.3\n").unwrap();
        let c = append(&a, &b).unwrap();
        assert_eq!(c.t, [0.0, 1800.0, 28800.0, 30600.0]);
        assert_eq!(c.cols[0], [1.0, 1.1, 1.2, 1.3]);
        // dated: two days later
        let a = read_log("2026-11-21T10:00:00,1\n2026-11-21T10:00:10,2\n").unwrap();
        let b = read_log("2026-11-23T10:00:00,3\n2026-11-23T10:00:10,4\n").unwrap();
        assert_eq!(append(&a, &b).unwrap().t, [0.0, 10.0, 172800.0, 172810.0]);
        // in seconds: one step after the last row
        let a = read_log("0 1\n10 2\n20 3\n").unwrap();
        let b = read_log("0 4\n10 5\n").unwrap();
        assert_eq!(append(&a, &b).unwrap().t, [0.0, 10.0, 20.0, 30.0, 40.0]);
        // a log without the column
        let b = read_log("time,x\n07:00:00,1\n07:00:01,2\n").unwrap();
        assert!(append(&read_log("time,v\n23:00:00,1.0\n23:30:00,1.1\n").unwrap(), &b).is_err());
    }
}
