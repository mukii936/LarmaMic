//! Acoustic echo cancellation.
//!
//! Problem: the phone's microphone hears what the PC plays (a friend's voice from a call) and sends it back,
//! so the other side hears themselves. We capture what the PC plays (WASAPI loopback, the "reference") and
//! subtract the part of the microphone signal that it explains.
//!
//! 1. Delay search: the echo lags the reference by the speaker, the air, the phone, the network and our jitter
//!    buffer (easily 100-300 ms), far more than an adaptive filter can span. Every second the lag is found by
//!    cross-correlating 4 kHz versions of both signals.
//! 2. NLMS filter (1024 taps = 21 ms) around that lag models the speaker-to-microphone path and is subtracted.
//! 3. A light suppressor lowers what is left while only the far end is talking (no near-end speech).
//!
//! Streams are aligned by absolute sample counters, not by when each thread happens to run, so processing
//! jitter does not disturb the filter.

const SR: usize = 48_000;
const TAPS: usize = 1024;
/// Filter taps before the estimated delay (absorbs the delay-search resolution).
const PRE: i64 = 128;
/// Decimation of the delay search (48 kHz -> 4 kHz).
const DS: usize = 12;
/// Delay search range, in samples at 48 kHz: the echo may lag by up to 600 ms. A small negative start allows for
/// the two streams starting a little apart.
const LAG_MIN: i64 = -(SR as i64) / 10;
const LAG_MAX: i64 = (SR as i64) * 6 / 10;
const WINDOW: usize = SR / 2; // 0.5 s of microphone used for each search
const KEEP: usize = SR * 3;

pub struct Aec {
    r: Vec<f32>,
    r_base: u64,
    m: Vec<f32>,
    m_base: u64,
    mic_total: u64,
    /// Reference index of microphone sample n is `n - lag`.
    lag: i64,
    has_lag: bool,
    w: Vec<f32>, // taps, oldest reference first
    since_search: u32,
    gain: f32,
    prev_near: bool,
    /// Smoothed echo return loss enhancement (dB) while the far end talks.
    erle: f32,
    near_run: u32,
}

impl Default for Aec {
    fn default() -> Self {
        Self::new()
    }
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        0.0
    } else {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }
}

impl Aec {
    pub fn new() -> Self {
        Self {
            r: Vec::new(),
            r_base: 0,
            m: Vec::new(),
            m_base: 0,
            mic_total: 0,
            lag: 0,
            has_lag: false,
            w: vec![0.0; TAPS],
            since_search: 0,
            gain: 1.0,
            prev_near: false,
            erle: 0.0,
            near_run: 0,
        }
    }

    /// Forget everything (when the feature is switched off or the reference stream restarts).
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn push_ref(&mut self, s: &[f32]) {
        self.r.extend_from_slice(s);
        if self.r.len() > KEEP * 2 {
            let cut = self.r.len() - KEEP;
            self.r.drain(..cut);
            self.r_base += cut as u64;
        }
    }

    fn r_end(&self) -> u64 {
        self.r_base + self.r.len() as u64
    }

    /// Block means over `DS` samples of `src[off..off + n * DS]`.
    fn decimate(src: &[f32], off: usize, n: usize) -> Vec<f32> {
        (0..n).map(|t| src[off + t * DS..off + (t + 1) * DS].iter().sum::<f32>() / DS as f32).collect()
    }

    /// Cross-correlate the last `WINDOW` microphone samples with the reference to find the echo delay.
    fn search_lag(&mut self) {
        let m_end = self.mic_total;
        if (self.m.len() as usize) < WINDOW || self.r.len() < WINDOW {
            return;
        }
        let m0 = m_end - WINDOW as u64; // absolute index where the microphone window starts
        // microphone window (decimated)
        let m_off = (m0 - self.m_base) as usize;
        let n = WINDOW / DS;
        let md = Self::decimate(&self.m, m_off, n);
        let m_energy: f32 = md.iter().map(|v| v * v).sum();
        if m_energy.sqrt() / (n as f32).sqrt() < 2e-4 {
            return;
        }
        // reference span needed: [m0 - LAG_MAX, m_end - LAG_MIN)
        let r_lo = m0 as i64 - LAG_MAX;
        let r_hi = m_end as i64 - LAG_MIN;
        let lo = r_lo.max(self.r_base as i64);
        let hi = r_hi.min(self.r_end() as i64);
        if hi - lo < (WINDOW + DS * 4) as i64 {
            return;
        }
        let lo_al = lo + ((DS as i64 - (lo % DS as i64)) % DS as i64); // align to a multiple of DS (absolute)
        let cnt = ((hi - lo_al) / DS as i64) as usize;
        let rd = Self::decimate(&self.r, (lo_al as u64 - self.r_base) as usize, cnt);
        // prefix sums of squares for the normalisation
        let mut pre = vec![0f64; rd.len() + 1];
        for (i, v) in rd.iter().enumerate() {
            pre[i + 1] = pre[i] + (*v as f64) * (*v as f64);
        }
        // candidate lag l (in DS units): reference position of md[t] is (m0 / DS + t) - l  (absolute DS units)
        let m0_ds = (m0 / DS as u64) as i64; // md[0] corresponds to absolute DS index m0_ds (m0 is DS-agnostic: small offset is absorbed by taps)
        let lo_ds = lo_al / DS as i64;
        let mut best = (0i64, 0f32);
        let mut sum_abs = 0f32;
        let mut count = 0f32;
        let (lmin, lmax) = (LAG_MIN / DS as i64, LAG_MAX / DS as i64);
        for l in lmin..=lmax {
            let start = m0_ds - l - lo_ds; // index in rd of reference sample aligned with md[0]
            if start < 0 || (start as usize) + n > rd.len() {
                continue;
            }
            let s = start as usize;
            let mut c = 0f32;
            for t in 0..n {
                c += md[t] * rd[s + t];
            }
            let re = (pre[s + n] - pre[s]) as f32;
            let norm = (m_energy * re).sqrt() + 1e-12;
            let v = c / norm;
            sum_abs += v.abs();
            count += 1.0;
            if v > best.1 {
                best = (l, v);
            }
        }
        if count < 1.0 {
            return;
        }
        let mean_abs = sum_abs / count;
        // accept only a clear peak
        if best.1 > 0.12 && best.1 > 3.0 * mean_abs {
            let new_lag = best.0 * DS as i64 + (DS as i64) / 2;
            if !self.has_lag || (new_lag - self.lag).abs() > 48 {
                if self.has_lag {
                    self.w.iter_mut().for_each(|x| *x = 0.0);
                }
                self.lag = new_lag;
                self.has_lag = true;
            }
        }
    }

    /// Remove the echo from one microphone block (in place).
    pub fn process(&mut self, mic: &mut [f32]) {
        let n = mic.len();
        self.m.extend_from_slice(mic);
        if self.m.len() > KEEP * 2 {
            let cut = self.m.len() - KEEP;
            self.m.drain(..cut);
            self.m_base += cut as u64;
        }
        let start = self.mic_total;
        self.mic_total += n as u64;

        self.since_search += 1;
        if !self.has_lag && self.since_search >= 30 || self.since_search >= 100 {
            self.since_search = 0;
            self.search_lag();
        }
        if !self.has_lag {
            return;
        }

        let r_end = self.r_end() as i64;
        let r_base = self.r_base as i64;
        let ref_rms = rms(&self.r[self.r.len().saturating_sub(n)..]);
        let mic_rms = rms(mic);
        let far_active = ref_rms > 1e-3;
        let converged = self.erle > 8.0;

        // Look first (no adaptation): is there near-end speech in this block? Judged with the current filter so a block
        // of speech cannot damage the filter before we notice it.
        let (mut pre_y, mut pre_e) = (0f32, 0f32);
        for i in 0..n {
            let idx = (start + i as u64) as i64;
            let a = idx - self.lag + PRE;
            let first = a - (TAPS as i64 - 1);
            if first < r_base || a >= r_end {
                continue;
            }
            let off = (first - r_base) as usize;
            let y: f32 = self.w.iter().zip(&self.r[off..off + TAPS]).map(|(w, v)| w * v).sum();
            pre_y += y * y;
            pre_e += (mic[i] - y) * (mic[i] - y);
        }
        let (pre_y, pre_e) = ((pre_y / n as f32).sqrt(), (pre_e / n as f32).sqrt());
        // after convergence the residue should be about `erle` dB below the echo estimate; far above that = someone speaks near the phone
        let expected = pre_y * 10f32.powf(-self.erle.clamp(8.0, 25.0) / 20.0);
        let near_now = if converged { pre_e > (2.0 * expected).max(5e-4) } else { mic_rms > 6.0 * ref_rms + 1e-3 };
        self.near_run = if near_now { self.near_run + 1 } else { 0 };
        // Adaptation: moderate while only the far end talks, frozen (almost) while near-end speech is present
        // (if "near-end speech" lasts for seconds it is probably a changed echo path, so keep adapting slowly)
        let mu = if !far_active { 0.0 } else if near_now { if self.near_run > 300 { 0.1 } else { 0.01 } } else { 0.3 };

        let mut y_energy = 0f32;
        let mut e_energy = 0f32;
        let mut m_energy = 0f32;
        let mut pow = 0f32;
        let mut pow_valid_for: i64 = i64::MIN;
        for i in 0..n {
            let idx = (start + i as u64) as i64;
            let a = idx - self.lag + PRE; // newest tap's reference index
            let first = a - (TAPS as i64 - 1);
            if first < r_base || a >= r_end {
                continue; // reference not available for this sample
            }
            let off = (first - r_base) as usize;
            let x = &self.r[off..off + TAPS];
            // running power of the tap window
            if pow_valid_for == a - 1 {
                let old = self.r[off - 1];
                let new = x[TAPS - 1];
                pow += new * new - old * old;
            } else {
                pow = x.iter().map(|v| v * v).sum();
            }
            pow_valid_for = a;
            let y: f32 = self.w.iter().zip(x).map(|(w, v)| w * v).sum();
            let e = mic[i] - y;
            m_energy += mic[i] * mic[i];
            y_energy += y * y;
            e_energy += e * e;
            if mu > 0.0 {
                let step = mu * e / (pow.max(0.0) + 1e-6 * TAPS as f32);
                for (w, v) in self.w.iter_mut().zip(x) {
                    *w += step * v;
                }
            }
            mic[i] = e;
        }

        // How well is the echo being removed? (only judged while the far end is talking and nobody near is)
        if far_active && !self.prev_near && e_energy > 0.0 && m_energy > 0.0 {
            let erle = 10.0 * (m_energy / e_energy).log10();
            self.erle += (erle - self.erle) * 0.05;
        }
        let _ = (y_energy, e_energy);
        // near-end speech: what is left is clearly more than the filter can explain
        let near = near_now;
        self.prev_near = near;
        // light suppressor: only once the filter has converged, while the far end talks and no near-end speech is present
        let target = if converged && far_active && !near { 0.35 } else { 1.0 };
        for s in mic.iter_mut() {
            // move gradually (about 20 ms) to avoid clicks
            self.gain += (target - self.gain) * 0.002;
            *s *= self.gain;
        }
    }

    pub fn lag_samples(&self) -> Option<i64> {
        self.has_lag.then_some(self.lag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        }
    }

    /// Far-end "speech": coloured noise with an on/off envelope.
    fn far_end(len: usize, seed: u64) -> Vec<f32> {
        let mut rng = Lcg(seed);
        let (mut lp, mut env, mut hold) = (0f32, 0f32, 0usize);
        let mut on = true;
        (0..len)
            .map(|i| {
                if i > len.saturating_sub(SR * 4) {
                    on = true; // the measuring window must contain far-end speech
                    hold = hold.max(2);
                }
                if hold == 0 {
                    on = !on || rng.next() > 0.0;
                    hold = 4000 + ((rng.next().abs() * 14_000.0) as usize);
                }
                hold -= 1;
                env += ((if on { 0.25 } else { 0.0 }) - env) * 0.002;
                lp += (rng.next() - lp) * 0.35;
                lp * env
            })
            .collect()
    }

    /// Room response: `delay` samples of silence then a decaying random tail.
    fn echo_path(delay: usize, taps: usize, gain: f32) -> Vec<f32> {
        let mut rng = Lcg(77);
        let mut h = vec![0.0; delay];
        for k in 0..taps {
            h.push(gain * rng.next() * (-(k as f32) / 90.0).exp());
        }
        h
    }

    fn convolve(x: &[f32], h: &[f32]) -> Vec<f32> {
        // sparse-friendly: skip the leading zeros of h
        let first = h.iter().position(|v| *v != 0.0).unwrap_or(0);
        let mut y = vec![0f32; x.len()];
        for (k, hv) in h.iter().enumerate().skip(first) {
            for n in k..x.len() {
                y[n] += hv * x[n - k];
            }
        }
        y
    }

    /// Runs the canceller over `secs` seconds. Returns (echo-only ERLE dB over the last 2 s, output, mic, near-end).
    fn run(delay: usize, secs: usize, near: bool) -> (f32, Vec<f32>, Vec<f32>, Vec<f32>) {
        let len = SR * secs;
        let far = far_end(len, 5);
        let echo = convolve(&far, &echo_path(delay, 300, 0.6));
        let near_sig: Vec<f32> = (0..len)
            .map(|i| if near && i > len - SR * 2 { 0.15 * (2.0 * std::f32::consts::PI * 600.0 * i as f32 / SR as f32).sin() } else { 0.0 })
            .collect();
        let mut noise = Lcg(1234);
        let mic_all: Vec<f32> = echo.iter().zip(&near_sig).map(|(e, s)| e + s + 0.002 * noise.next()).collect();
        let mut aec = Aec::new();
        let mut out_all = Vec::with_capacity(len);
        let mut pushed = 0usize;
        let mut jitter = Lcg(9);
        for f in 0..len / 480 {
            // the reference arrives in uneven chunks, up to +-200 samples from the microphone clock
            let upto = ((f + 1) * 480).min(len);
            let target = (upto as i64 + (jitter.next() * 200.0) as i64).clamp(0, len as i64) as usize;
            if target > pushed {
                aec.push_ref(&far[pushed..target]);
                pushed = target;
            }
            let mut frame = mic_all[f * 480..(f + 1) * 480].to_vec();
            aec.process(&mut frame);
            out_all.extend_from_slice(&frame);
        }
        let tail = out_all.len() - SR * 2;
        // Echo left in the output: take the best-fitting 600 Hz tone (the near-end speech stand-in) out of the output
        // first, so a small gain change of the tone is not counted as echo.
        let w = 2.0 * std::f64::consts::PI * 600.0 / SR as f64;
        let (mut ss, mut cc, mut sc, mut os, mut oc) = (0f64, 0f64, 0f64, 0f64, 0f64);
        for i in tail..out_all.len() {
            let (sn, cs) = ((w * i as f64).sin(), (w * i as f64).cos());
            ss += sn * sn;
            cc += cs * cs;
            sc += sn * cs;
            os += out_all[i] as f64 * sn;
            oc += out_all[i] as f64 * cs;
        }
        let det = ss * cc - sc * sc;
        let (qa, qb) = if near { ((os * cc - oc * sc) / det, (oc * ss - os * sc) / det) } else { (0.0, 0.0) };
        let (mut pe, mut po) = (0f64, 0f64);
        for i in tail..out_all.len() {
            let e = echo[i] as f64;
            pe += e * e;
            let fit = qa * (w * i as f64).sin() + qb * (w * i as f64).cos();
            let o = out_all[i] as f64 - fit;
            po += o * o;
        }
        (10.0 * ((pe + 1e-12) / (po + 1e-12)).log10() as f32, out_all, mic_all, near_sig)
    }

    #[test]
    fn finds_the_echo_delay() {
        for delay in [3000usize, 7200, 14_400] {
            let len = SR * 5;
            let far = far_end(len, 3);
            let echo = convolve(&far, &echo_path(delay, 200, 0.5));
            let mut aec = Aec::new();
            aec.push_ref(&far);
            for f in 0..len / 480 {
                let mut fr = echo[f * 480..(f + 1) * 480].to_vec();
                aec.process(&mut fr);
            }
            let lag = aec.lag_samples().expect("a lag should have been found");
            assert!((lag - delay as i64).abs() < 400, "delay {delay}: found {lag}");
        }
    }

    #[test]
    fn cancels_a_delayed_echo() {
        let (erle, _, _, _) = run(7200, 10, false); // 150 ms
        assert!(erle > 18.0, "ERLE only {erle:.1} dB");
    }

    #[test]
    fn keeps_near_end_speech_during_the_echo() {
        let (erle, out, _mic, near) = run(7200, 10, true);
        assert!(erle > 15.0, "ERLE during near-end {erle:.1} dB");
        // the 600 Hz near-end tone must survive: compare its power in the output with the original
        let tail = out.len() - SR * 2;
        let (mut p_out, mut p_ref) = (0f64, 0f64);
        for i in tail..out.len() {
            // correlate with the known tone
            p_out += (out[i] as f64) * (near[i] as f64);
            p_ref += (near[i] as f64) * (near[i] as f64);
        }
        let kept = p_out / p_ref;
        assert!(kept > 0.5 && kept < 1.5, "near-end kept {kept:.2}");
    }

    #[test]
    fn passes_audio_untouched_without_any_reference() {
        let mut aec = Aec::new();
        let sig = far_end(SR * 2, 11);
        let mut out = Vec::new();
        for f in 0..sig.len() / 480 {
            let mut fr = sig[f * 480..(f + 1) * 480].to_vec();
            aec.process(&mut fr);
            out.extend(fr);
        }
        assert_eq!(out.len(), sig.len());
        assert!(out.iter().zip(&sig).all(|(a, b)| (a - b).abs() < 1e-6));
    }
}
