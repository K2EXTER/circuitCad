//! The buzzer's sound on the breadboard (candidate B6 ②, the sound of B4): while the circuit runs, a buzzer with enough voltage
//! across it sounds through the speakers. The library's buzzers are active ones (an oscillator inside): they give one pitch when
//! their + is a few volts above their −, louder with more voltage, and nothing the other way round. The sound is a tone made here
//! as a WAV in memory and looped by Windows (`PlaySoundW`, no extra crate); one tone plays for all the buzzers sounding.

/// The voltage (+ above −) an active buzzer starts to sound at: real ones need about 3 V (the library part's notes).
pub const BUZZ_FROM: f64 = 2.5;
/// The pitch of the library's buzzers (an active 12 mm buzzer gives about 2.3 kHz).
pub const BUZZ_HZ: f64 = 2300.0;
const RATE: u32 = 22050;
/// The loudness steps: a new step starts the tone again, so the loudness is kept to a few.
const STEPS: u8 = 4;

/// How loud a buzzer with `volts` from + to − sounds: None when silent (too little voltage, or the wrong way round), else a step
/// 1..=STEPS (the rated 5 V and above the loudest).
pub fn loudness(volts: f64) -> Option<u8> {
    if !volts.is_finite() || volts < BUZZ_FROM {
        return None;
    }
    let k = ((volts - BUZZ_FROM) / (5.0 - BUZZ_FROM) * (STEPS - 1) as f64).round();
    Some((k as u8 + 1).min(STEPS))
}

/// A tone that loops without a click: a whole number of cycles of a buzzer's sound (a square wave softened to its first odd
/// harmonics) at loudness `step`, as a 16-bit mono WAV.
pub fn tone_wav(hz: f64, step: u8) -> Vec<u8> {
    // one second of RATE samples holds a whole number of cycles of a whole-hertz pitch
    let n = RATE as usize;
    let hz = hz.round();
    let amp = 0.06 + 0.06 * step.min(STEPS) as f64;
    let mut data = Vec::with_capacity(n * 2);
    for i in 0..n {
        let x = std::f64::consts::TAU * hz * i as f64 / RATE as f64;
        let v = (x.sin() + (3.0 * x).sin() / 3.0 + (5.0 * x).sin() / 5.0) * amp;
        data.extend_from_slice(&((v.clamp(-1.0, 1.0) * i16::MAX as f64) as i16).to_le_bytes());
    }
    let mut w = Vec::with_capacity(44 + data.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    w
}

/// The tone playing now: its loudness step and the WAV Windows reads it from (kept alive while it plays).
#[derive(Default)]
pub struct Beeper {
    playing: Option<(u8, Vec<u8>)>,
}

impl Beeper {
    /// Sound at loudness `step`, or stop (None). The same step as now leaves the tone running.
    pub fn set(&mut self, step: Option<u8>) {
        if self.playing.as_ref().map(|(s, _)| *s) == step {
            return;
        }
        stop();
        self.playing = None;
        if let Some(s) = step {
            let wav = tone_wav(BUZZ_HZ, s);
            play_loop(&wav);
            self.playing = Some((s, wav));
        }
    }

    #[cfg(test)]
    pub fn is_playing(&self) -> bool {
        self.playing.is_some()
    }
}

impl Drop for Beeper {
    fn drop(&mut self) {
        if self.playing.is_some() {
            stop();
        }
    }
}

// Tests run silently: only the app itself plays.
#[cfg(all(windows, not(test)))]
mod sys {
    #[link(name = "winmm")]
    unsafe extern "system" {
        fn PlaySoundW(sound: *const u16, module: *mut core::ffi::c_void, flags: u32) -> i32;
    }
    const SND_ASYNC: u32 = 0x1;
    const SND_NODEFAULT: u32 = 0x2;
    const SND_MEMORY: u32 = 0x4;
    const SND_LOOP: u32 = 0x8;

    pub fn play_loop(wav: &[u8]) {
        // SAFETY: the WAV stays alive (held by the Beeper) until `stop` is called before it is dropped or replaced
        unsafe {
            PlaySoundW(wav.as_ptr() as *const u16, core::ptr::null_mut(), SND_ASYNC | SND_NODEFAULT | SND_MEMORY | SND_LOOP);
        }
    }

    pub fn stop() {
        // SAFETY: a null sound stops whatever this process plays
        unsafe {
            PlaySoundW(core::ptr::null(), core::ptr::null_mut(), 0);
        }
    }
}

#[cfg(not(all(windows, not(test))))]
mod sys {
    pub fn play_loop(_: &[u8]) {}
    pub fn stop() {}
}

use sys::{play_loop, stop};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buzzer_sounds_only_the_right_way_round_with_enough_voltage() {
        assert_eq!(loudness(5.0), Some(STEPS));
        assert_eq!(loudness(4.95), Some(STEPS), "a little under 5 V through the switch");
        assert_eq!(loudness(2.7), Some(1));
        assert_eq!(loudness(2.0), None, "below the voltage it starts at");
        assert_eq!(loudness(-5.0), None, "the wrong way round");
        assert_eq!(loudness(f64::NAN), None);
        assert!(loudness(4.0) < loudness(5.0));
    }

    #[test]
    fn the_tone_is_a_wav_that_loops_without_a_click() {
        let w = tone_wav(BUZZ_HZ, 2);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        let len = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
        assert_eq!(len + 44, w.len());
        let s = |i: usize| i16::from_le_bytes([w[44 + 2 * i], w[45 + 2 * i]]);
        // the first sample is 0 (the sine starts there) and the last one leads back to it in one small step
        assert_eq!(s(0), 0);
        let last = s(len / 2 - 1);
        assert!(last.unsigned_abs() < s(1).unsigned_abs() * 2, "{last}");
        // a louder step is louder
        let peak = |w: &[u8]| w[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs()).max().unwrap();
        assert!(peak(&tone_wav(BUZZ_HZ, 4)) > peak(&w));
    }
}
