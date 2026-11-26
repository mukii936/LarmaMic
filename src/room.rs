//! "Measure my room": the user talks for about 10 seconds and the app suggests a noise gate threshold and an EQ
//! correction. The gate comes from the background level (the quietest moments) and the speech level; the EQ compares the
//! average speech spectrum with a typical voice tilt. These are suggestions the user can apply or ignore.

use crate::dsp::BANDS;

pub const FRAMES_TARGET: usize = 1000; // 10 s of 10 ms frames

#[derive(Clone, Copy, Debug)]
pub struct RoomResult {
    pub thr: f32,
    pub low: f32,
    pub mud: f32,
    pub pres: f32,
    pub air: f32,
    pub noise_db: f32,
    pub speech_db: f32,
}

#[derive(Default)]
pub struct RoomMeter {
    frames: Vec<(f32, [f32; 5])>,
}

/// Band index ranges (of the 48 log bands from 40 Hz to 16 kHz) for: low 120 Hz, mud 280 Hz, mid 1 kHz, presence 3.2 kHz, air 9 kHz.
const RANGES: [(usize, usize); 5] = [(7, 10), (14, 17), (24, 28), (33, 37), (41, 45)];
/// Typical speech balance in dB relative to the 1 kHz area (heuristic): low, mud, presence, air.
const TARGET: [f32; 4] = [-4.0, -3.0, -2.0, -10.0];

fn mean(a: &[f32]) -> f32 {
    a.iter().sum::<f32>() / a.len().max(1) as f32
}

impl RoomMeter {
    pub fn new() -> Self {
        Self { frames: Vec::with_capacity(FRAMES_TARGET) }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// `level_db`: level of this 10 ms frame in dBFS; `bands`: input spectrum in dBFS.
    pub fn feed(&mut self, level_db: f32, bands: &[f32; BANDS]) {
        let mut b = [0f32; 5];
        for (i, (lo, hi)) in RANGES.iter().enumerate() {
            b[i] = mean(&bands[*lo..=*hi]);
        }
        self.frames.push((level_db, b));
    }

    pub fn finish(&self) -> Option<RoomResult> {
        if self.frames.len() < 300 {
            return None;
        }
        let mut levels: Vec<f32> = self.frames.iter().map(|f| f.0).collect();
        levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let noise_db = levels[levels.len() * 15 / 100];
        let speech_db = levels[levels.len() * 80 / 100];
        // not enough difference between the quiet moments and the loud ones: the user did not speak (or it is all noise)
        if speech_db - noise_db < 12.0 {
            return None;
        }
        let speech: Vec<&(f32, [f32; 5])> = self.frames.iter().filter(|f| f.0 > noise_db + 12.0).collect();
        if speech.len() < 50 {
            return None;
        }
        let avg = |i: usize| mean(&speech.iter().map(|f| f.1[i]).collect::<Vec<f32>>());
        let (low, mud, mid, pres, air) = (avg(0), avg(1), avg(2), avg(3), avg(4));
        let rel = [low - mid, mud - mid, pres - mid, air - mid];
        let sugg = |i: usize| (-(rel[i] - TARGET[i]) * 0.5).clamp(-6.0, 6.0).round();
        let thr = (noise_db + 9.0).min(speech_db - 15.0).clamp(-72.0, -20.0).round();
        Some(RoomResult { thr, low: sugg(0), mud: sugg(1), pres: sugg(2), air: sugg(3), noise_db, speech_db })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bands_with(tilt: [f32; 4]) -> [f32; BANDS] {
        // build a spectrum whose area levels relative to the mid area are `tilt` (low, mud, presence, air)
        let mut b = [-50.0f32; BANDS];
        for i in RANGES[2].0..=RANGES[2].1 {
            b[i] = -30.0;
        }
        for (k, idx) in [0usize, 1, 3, 4].iter().enumerate() {
            for i in RANGES[*idx].0..=RANGES[*idx].1 {
                b[i] = -30.0 + tilt[k];
            }
        }
        b
    }

    fn run(speech_tilt: [f32; 4]) -> Option<RoomResult> {
        let mut m = RoomMeter::new();
        for i in 0..1000 {
            let speaking = i % 10 < 6;
            let level = if speaking { -24.0 } else { -62.0 };
            m.feed(level, &bands_with(speech_tilt));
        }
        m.finish()
    }

    #[test]
    fn suggests_a_gate_between_the_noise_and_the_speech() {
        let r = run(TARGET).expect("a result");
        assert!(r.thr > -62.0 && r.thr < -24.0, "thr {}", r.thr);
        assert!((r.noise_db + 62.0).abs() < 1.0 && (r.speech_db + 24.0).abs() < 1.0);
        // a voice already on target needs (almost) no EQ
        assert!(r.low.abs() <= 1.0 && r.mud.abs() <= 1.0 && r.pres.abs() <= 1.0 && r.air.abs() <= 1.0, "{r:?}");
    }

    #[test]
    fn boomy_voice_gets_less_bass_and_dull_voice_more_presence() {
        let boomy = run([TARGET[0] + 10.0, TARGET[1] + 6.0, TARGET[2], TARGET[3]]).unwrap();
        assert!(boomy.low <= -3.0 && boomy.mud <= -2.0, "{boomy:?}");
        let dull = run([TARGET[0], TARGET[1], TARGET[2] - 8.0, TARGET[3] - 6.0]).unwrap();
        assert!(dull.pres >= 3.0 && dull.air >= 2.0, "{dull:?}");
    }

    #[test]
    fn needs_speech_and_enough_time() {
        let mut m = RoomMeter::new();
        for _ in 0..1000 {
            m.feed(-60.0, &bands_with(TARGET));
        }
        assert!(m.finish().is_none(), "silence gives no result");
        let mut short = RoomMeter::new();
        for i in 0..100 {
            short.feed(if i % 2 == 0 { -20.0 } else { -60.0 }, &bands_with(TARGET));
        }
        assert!(short.finish().is_none(), "too short");
    }
}
