//! Waveforms as sound (B4): a transient run's signal made into a WAV to listen to or save (`to_wav`), and a WAV file made into a
//! source's waveform (`read_wav`, the `WAV(file …)` source). The simulated time is the sound's time: a 1 kHz signal sounds at 1 kHz.

/// The sample rate of the sound made (Hz).
pub const RATE: u32 = 44_100;
/// The loudest the made sound goes (a share of the full scale): loud enough to hear, not to startle.
pub const PEAK: f64 = 0.5;

/// What `to_wav` made: the WAV and how it got there.
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub wav: Vec<u8>,
    /// Seconds of sound.
    pub seconds: f64,
    /// How many times the run was repeated to make it long enough to hear.
    pub repeats: usize,
    /// The DC level taken off (the signal's mean, V) and the scale (V per full scale) it was played at.
    pub offset: f64,
    pub volts_full_scale: f64,
}

/// A WAV of 16-bit mono samples (each -1..1).
pub fn wav_bytes(samples: &[f64], rate: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        data.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f64) as i16).to_le_bytes());
    }
    let mut w = Vec::with_capacity(44 + data.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    w
}

/// The signal `v` over the times `t` (from the window `[t0, t1]`), as sound at `RATE`: its mean taken off, scaled so its peak is
/// `PEAK`, and repeated until it lasts `min_seconds` (a run of a few milliseconds is too short to hear).
pub fn to_wav(t: &[f64], v: &[f64], t0: f64, t1: f64, min_seconds: f64) -> Result<Sound, String> {
    if t.len() < 2 || v.len() != t.len() {
        return Err("波形の点が足りません".into());
    }
    let (a, b) = (t0.max(t[0]), t1.min(*t.last().unwrap()));
    let span = b - a;
    if span < 2.0 / RATE as f64 {
        return Err(format!("時間の範囲が短すぎます (音にするには {:.0} µs 以上)", 2e6 / RATE as f64));
    }
    if span > 30.0 {
        return Err("時間の範囲が長すぎます (30 秒まで)".into());
    }
    let n = (span * RATE as f64).round() as usize;
    // linear interpolation on the run's own (uneven) steps
    let mut k = 0;
    let mut one = Vec::with_capacity(n);
    for i in 0..n {
        let x = a + i as f64 / RATE as f64;
        while k + 1 < t.len() - 1 && t[k + 1] < x {
            k += 1;
        }
        let f = if t[k + 1] > t[k] { ((x - t[k]) / (t[k + 1] - t[k])).clamp(0.0, 1.0) } else { 0.0 };
        one.push(v[k] + (v[k + 1] - v[k]) * f);
    }
    let mean = one.iter().sum::<f64>() / n as f64;
    let peak = one.iter().fold(0.0f64, |m, x| m.max((x - mean).abs()));
    if peak < 1e-9 {
        return Err("この信号は変化していないので、音になりません (直流だけです)".into());
    }
    let scale = PEAK / peak;
    let repeats = ((min_seconds / span).ceil() as usize).max(1);
    let mut samples = Vec::with_capacity(n * repeats);
    for _ in 0..repeats {
        samples.extend(one.iter().map(|x| (x - mean) * scale));
    }
    // a short fade at each end, so it starts and stops without a click
    let fade = (RATE as usize / 200).min(samples.len() / 4);
    let len = samples.len();
    for i in 0..fade {
        let g = i as f64 / fade as f64;
        samples[i] *= g;
        samples[len - 1 - i] *= g;
    }
    Ok(Sound { seconds: len as f64 / RATE as f64, wav: wav_bytes(&samples, RATE), repeats, offset: mean, volts_full_scale: 1.0 / scale })
}

/// The samples of a WAV (8-, 16-, 24- or 32-bit PCM, or 32-bit float; stereo is averaged), each -1..1, and its rate.
pub fn read_wav(bytes: &[u8]) -> Result<(u32, Vec<f64>), String> {
    let bad = || "WAV の形が読めません (PCM か 32 ビットの浮動小数点の WAV にしてください)".to_string();
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad());
    }
    let u16_at = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let (mut fmt, mut data) = (None, None);
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32_at(i + 4).ok_or_else(bad)? as usize;
        let body = i + 8;
        if id == b"fmt " {
            fmt = Some(body);
        } else if id == b"data" {
            data = Some((body, len.min(bytes.len().saturating_sub(body))));
        }
        i = body + len + (len & 1);
    }
    let (f, (d, dlen)) = (fmt.ok_or_else(bad)?, data.ok_or_else(bad)?);
    let (format, channels, rate, bits) = (u16_at(f).ok_or_else(bad)?, u16_at(f + 2).ok_or_else(bad)? as usize, u32_at(f + 4).ok_or_else(bad)?, u16_at(f + 14).ok_or_else(bad)?);
    // WAVE_FORMAT_EXTENSIBLE carries the real format in its sub-format; PCM and float are told apart by the bits
    let float = format == 3 || (format == 0xFFFE && bits == 32 && u16_at(f + 24) == Some(3));
    if !(format == 1 || format == 3 || format == 0xFFFE) || channels == 0 || rate == 0 {
        return Err(bad());
    }
    let width = bits as usize / 8;
    if !matches!(width, 1..=4) {
        return Err(bad());
    }
    let frame = width * channels;
    let frames = dlen / frame;
    let mut out = Vec::with_capacity(frames);
    for k in 0..frames {
        let mut sum = 0.0;
        for c in 0..channels {
            let at = d + k * frame + c * width;
            let b = &bytes[at..at + width];
            sum += match (width, float) {
                (1, _) => (b[0] as f64 - 128.0) / 128.0,
                (2, _) => i16::from_le_bytes([b[0], b[1]]) as f64 / 32768.0,
                (3, _) => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f64 / 8_388_608.0,
                (4, true) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                _ => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64 / 2_147_483_648.0,
            };
        }
        out.push(sum / channels as f64);
    }
    Ok((rate, out))
}

/// The points of a source from a WAV file: `amp` volts at the full scale, the first `seconds` of it.
pub fn wav_points(path: &str, amp: f64, seconds: f64) -> Result<Vec<(f64, f64)>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("WAV のファイル「{path}」が開けません: {e}"))?;
    let (rate, s) = read_wav(&bytes)?;
    let n = ((seconds * rate as f64) as usize).min(s.len()).max(2.min(s.len()));
    if n < 2 {
        return Err(format!("WAV のファイル「{path}」に音が入っていません"));
    }
    Ok(s[..n].iter().enumerate().map(|(i, x)| (i as f64 / rate as f64, x * amp)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_sine_becomes_a_second_of_sound_and_reads_back() {
        // 10 ms of a 1 kHz sine of 2 V around 2.5 V
        let t: Vec<f64> = (0..=1000).map(|i| i as f64 * 1e-5).collect();
        let v: Vec<f64> = t.iter().map(|x| 2.5 + 2.0 * (std::f64::consts::TAU * 1e3 * x).sin()).collect();
        let s = to_wav(&t, &v, 0.0, 0.01, 1.0).unwrap();
        assert_eq!(s.repeats, 100);
        assert!((s.seconds - 1.0).abs() < 0.01, "{}", s.seconds);
        assert!((s.offset - 2.5).abs() < 0.01 && (s.volts_full_scale - 4.0).abs() < 0.05, "{s:?}");
        let (rate, back) = read_wav(&s.wav).unwrap();
        assert_eq!(rate, RATE);
        // the peak is PEAK, and a quarter period in (with no fade there) it is the top of the sine
        let peak = back.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        assert!((peak - PEAK).abs() < 0.01, "{peak}");
        let at = (RATE as f64 * (0.5 + 0.25e-3)) as usize;
        assert!((back[at] - PEAK).abs() < 0.02, "{}", back[at]);
    }

    #[test]
    fn a_flat_signal_or_a_bad_file_says_why() {
        let t = [0.0, 1e-3, 2e-3];
        assert!(to_wav(&t, &[1.0, 1.0, 1.0], 0.0, 2e-3, 1.0).unwrap_err().contains("直流"));
        assert!(read_wav(b"not a wav").is_err());
        // 8-bit stereo: the two channels averaged
        let mut w = wav_bytes(&[0.5, -0.5], 8000);
        assert_eq!(read_wav(&w).unwrap().1.len(), 2);
        w.clear();
        w.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&[1, 0, 2, 0]);
        w.extend_from_slice(&8000u32.to_le_bytes());
        w.extend_from_slice(&16000u32.to_le_bytes());
        w.extend_from_slice(&[2, 0, 8, 0]);
        w.extend_from_slice(b"data");
        w.extend_from_slice(&2u32.to_le_bytes());
        w.extend_from_slice(&[255, 128]);
        let (_, s) = read_wav(&w).unwrap();
        assert!((s[0] - 127.0 / 256.0).abs() < 1e-9, "{s:?}");
    }

    #[test]
    fn a_wav_file_is_a_source() {
        let dir = std::env::temp_dir().join(format!("ccad_wav_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two samples.wav");
        std::fs::write(&path, wav_bytes(&[0.5, -0.25, 1.0], 1000)).unwrap();
        let text = format!("WAV(\"{}\" 2 1)", path.display());
        match crate::parse_source(&text).unwrap() {
            crate::Waveform::Pwl(p) => {
                assert_eq!(p.len(), 3);
                assert!((p[1].0 - 1e-3).abs() < 1e-12 && (p[0].1 - 1.0).abs() < 1e-3 && (p[1].1 + 0.5).abs() < 1e-3, "{p:?}");
            }
            w => panic!("{w:?}"),
        }
        assert!(crate::parse_source("WAV(no_such_file.wav)").unwrap_err().contains("開けません"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

