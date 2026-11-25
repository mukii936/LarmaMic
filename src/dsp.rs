//! Chuỗi xử lý giọng nói ở 48 kHz, mono, khung 480 mẫu (10 ms):
//! RNNoise -> cắt ù -> cổng ồn (nhìn trước 5 ms) -> EQ 4 băng -> tự cân âm lượng -> nén -> âm lượng
//! -> hiệu ứng giọng (đổi tông, robot, vang) -> chống vỡ.

use crate::settings::Params;
use nnnoiseless::DenoiseState;
use std::f32::consts::PI;

pub const SR: f32 = 48_000.0;
pub const FRAME: usize = 480;
/// Cổng ồn nhìn trước: âm thanh bị trễ 5 ms để cổng kịp mở trước khi đầu chữ tới.
pub const LOOK: usize = 240;
/// Số băng của biểu đồ phổ và số mẫu cho mỗi lần phân tích.
pub const BANDS: usize = 48;
pub const FFT_N: usize = 1024;

const AGC_TARGET_DB: f32 = -20.0;
const AGC_FLOOR_DB: f32 = -50.0;

fn db2lin(d: f32) -> f32 {
    10f32.powf(d / 20.0)
}
pub fn lin2db(v: f32) -> f32 {
    20.0 * v.max(1e-5).log10()
}

// ---------- biquad (RBJ cookbook), dạng transposed direct form II ----------
#[derive(Clone, Copy)]
struct Coef {
    b0: f32, b1: f32, b2: f32, a1: f32, a2: f32,
}

pub struct Biquad {
    c: Coef,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn new() -> Self {
        Self { c: Coef { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 }, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let y = self.c.b0 * x + self.z1;
        self.z1 = self.c.b1 * x - self.c.a1 * y + self.z2;
        self.z2 = self.c.b2 * x - self.c.a2 * y;
        y
    }

    fn set(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        self.c = Coef { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 };
    }

    pub fn highpass(&mut self, f: f32, q: f32) {
        let w = 2.0 * PI * f / SR;
        let (cw, sw) = (w.cos(), w.sin());
        let al = sw / (2.0 * q);
        self.set((1.0 + cw) / 2.0, -(1.0 + cw), (1.0 + cw) / 2.0, 1.0 + al, -2.0 * cw, 1.0 - al);
    }

    pub fn peaking(&mut self, f: f32, q: f32, db: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * PI * f / SR;
        let (cw, sw) = (w.cos(), w.sin());
        let al = sw / (2.0 * q);
        self.set(1.0 + al * a, -2.0 * cw, 1.0 - al * a, 1.0 + al / a, -2.0 * cw, 1.0 - al / a);
    }

    pub fn lowshelf(&mut self, f: f32, db: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * PI * f / SR;
        let (cw, sw) = (w.cos(), w.sin());
        let al = sw / 2.0 * 2f32.sqrt(); // S = 1
        let t = 2.0 * a.sqrt() * al;
        self.set(
            a * ((a + 1.0) - (a - 1.0) * cw + t),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
            a * ((a + 1.0) - (a - 1.0) * cw - t),
            (a + 1.0) + (a - 1.0) * cw + t,
            -2.0 * ((a - 1.0) + (a + 1.0) * cw),
            (a + 1.0) + (a - 1.0) * cw - t,
        );
    }

    pub fn highshelf(&mut self, f: f32, db: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * PI * f / SR;
        let (cw, sw) = (w.cos(), w.sin());
        let al = sw / 2.0 * 2f32.sqrt();
        let t = 2.0 * a.sqrt() * al;
        self.set(
            a * ((a + 1.0) + (a - 1.0) * cw + t),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
            a * ((a + 1.0) + (a - 1.0) * cw - t),
            (a + 1.0) - (a - 1.0) * cw + t,
            2.0 * ((a - 1.0) - (a + 1.0) * cw),
            (a + 1.0) - (a - 1.0) * cw - t,
        );
    }

    /// Đáp ứng biên độ (dB) tại tần số `f`, để vẽ đường cong EQ.
    pub fn response_db(&self, f: f32) -> f32 {
        let w = 2.0 * PI * f / SR;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let c = &self.c;
        let (nr, ni) = (c.b0 + c.b1 * c1 + c.b2 * c2, -(c.b1 * s1 + c.b2 * s2));
        let (dr, di) = (1.0 + c.a1 * c1 + c.a2 * c2, -(c.a1 * s1 + c.a2 * s2));
        let num = (nr * nr + ni * ni).max(1e-20);
        let den = (dr * dr + di * di).max(1e-20);
        10.0 * (num / den).log10()
    }
}

/// Đường cong EQ tổng (cắt ù + 4 băng) tại tần số `f`, tính từ thông số hiện tại.
pub fn eq_response_db(p: &Params, f: f32) -> f32 {
    let (mut hp, mut low, mut mud, mut pres, mut air) =
        (Biquad::new(), Biquad::new(), Biquad::new(), Biquad::new(), Biquad::new());
    hp.highpass(p.hp, 0.707);
    low.lowshelf(120.0, p.low);
    mud.peaking(280.0, 1.0, p.mud);
    pres.peaking(3200.0, 0.9, p.pres);
    air.highshelf(9000.0, p.air);
    hp.response_db(f) + low.response_db(f) + mud.response_db(f) + pres.response_db(f) + air.response_db(f)
}

// ---------- hiệu ứng giọng ----------

/// Đổi tông bằng hai đầu đọc lệch pha trên đường trễ, chuyển mờ bằng cửa sổ Hann.
struct Pitch {
    buf: Vec<f32>,
    w: usize,
    phase: f32,
}

const PITCH_W: usize = 2048;
const PITCH_N: usize = 8192;

impl Pitch {
    fn new() -> Self {
        Self { buf: vec![0.0; PITCH_N], w: 0, phase: 0.0 }
    }

    fn read(&self, delay: f32) -> f32 {
        let pos = (self.w + PITCH_N) as f32 - delay;
        let i = pos.floor();
        let fr = pos - i;
        let i0 = (i as usize) % PITCH_N;
        let i1 = (i0 + 1) % PITCH_N;
        self.buf[i0] * (1.0 - fr) + self.buf[i1] * fr
    }

    fn process(&mut self, x: f32, ratio: f32) -> f32 {
        self.buf[self.w] = x;
        self.phase += (1.0 - ratio) / PITCH_W as f32;
        self.phase -= self.phase.floor();
        let p1 = self.phase;
        let p2 = (self.phase + 0.5).fract();
        let g1 = 0.5 - 0.5 * (2.0 * PI * p1).cos();
        let g2 = 0.5 - 0.5 * (2.0 * PI * p2).cos();
        let y = self.read(p1 * PITCH_W as f32) * g1 + self.read(p2 * PITCH_W as f32) * g2;
        self.w = (self.w + 1) % PITCH_N;
        y
    }
}

/// Giọng robot: nhân với sóng 55 Hz rồi cho qua bộ lọc lược 5,5 ms.
struct Robot {
    phase: f32,
    comb: Vec<f32>,
    i: usize,
}

impl Robot {
    fn new() -> Self {
        Self { phase: 0.0, comb: vec![0.0; 264], i: 0 }
    }
    fn process(&mut self, x: f32) -> f32 {
        self.phase += 2.0 * PI * 55.0 / SR;
        if self.phase > 2.0 * PI {
            self.phase -= 2.0 * PI;
        }
        let rm = x * self.phase.sin();
        let c = rm + 0.5 * self.comb[self.i];
        self.comb[self.i] = c;
        self.i = (self.i + 1) % self.comb.len();
        0.3 * x + 0.7 * c
    }
}

/// Vang kiểu Freeverb rút gọn: 4 bộ lược + 2 bộ thông-tất.
struct Comb {
    buf: Vec<f32>,
    i: usize,
    store: f32,
}
impl Comb {
    fn new(n: usize) -> Self {
        Self { buf: vec![0.0; n], i: 0, store: 0.0 }
    }
    fn process(&mut self, x: f32) -> f32 {
        let out = self.buf[self.i];
        self.store = out * 0.8 + self.store * 0.2;
        self.buf[self.i] = x + self.store * 0.84;
        self.i = (self.i + 1) % self.buf.len();
        out
    }
}
struct Allpass {
    buf: Vec<f32>,
    i: usize,
}
impl Allpass {
    fn new(n: usize) -> Self {
        Self { buf: vec![0.0; n], i: 0 }
    }
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        let out = -x + b;
        self.buf[self.i] = x + b * 0.5;
        self.i = (self.i + 1) % self.buf.len();
        out
    }
}
struct Reverb {
    combs: [Comb; 4],
    aps: [Allpass; 2],
}
impl Reverb {
    fn new() -> Self {
        Self {
            combs: [Comb::new(1187), Comb::new(1277), Comb::new(1393), Comb::new(1491)],
            aps: [Allpass::new(225), Allpass::new(556)],
        }
    }
    fn process(&mut self, x: f32) -> f32 {
        let inp = x * 0.25;
        let mut s: f32 = self.combs.iter_mut().map(|c| c.process(inp)).sum();
        for ap in self.aps.iter_mut() {
            s = ap.process(s);
        }
        s
    }
}

// ---------- phân tích phổ (FFT radix-2) ----------
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (i + k, i + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// Mức (dBFS) của 48 băng log từ 40 Hz đến 16 kHz, từ 1024 mẫu gần nhất.
pub fn spectrum_bands(samples: &[f32; FFT_N], out: &mut [f32; BANDS]) {
    let mut re = [0f32; FFT_N];
    let mut im = [0f32; FFT_N];
    for i in 0..FFT_N {
        let w = 0.5 - 0.5 * (2.0 * PI * i as f32 / FFT_N as f32).cos();
        re[i] = samples[i] * w;
    }
    fft(&mut re, &mut im);
    let mag: Vec<f32> = (0..FFT_N / 2).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt()).collect();
    let bin_hz = SR / FFT_N as f32;
    let (f_lo, f_hi) = (40.0f32, 16_000.0f32);
    let ratio = (f_hi / f_lo).powf(1.0 / BANDS as f32);
    for b in 0..BANDS {
        let (a, c) = (f_lo * ratio.powi(b as i32), f_lo * ratio.powi(b as i32 + 1));
        let (ba, bc) = (a / bin_hz, c / bin_hz);
        let m = if bc - ba < 1.0 {
            // băng hẹp hơn một bin: nội suy tại tâm
            let pos = (ba + bc) / 2.0;
            let i0 = (pos.floor() as usize).min(FFT_N / 2 - 2);
            let fr = pos - i0 as f32;
            mag[i0] * (1.0 - fr) + mag[i0 + 1] * fr
        } else {
            (ba.floor() as usize..=(bc.ceil() as usize).min(FFT_N / 2 - 1))
                .map(|k| mag[k])
                .fold(0.0, f32::max)
        };
        // chuẩn hóa: sóng sin biên độ A cho đỉnh A·N/4 với cửa sổ Hann
        out[b] = 20.0 * (m / (FFT_N as f32 / 4.0)).max(1e-6).log10();
    }
}

/// Thông tin đo được của một khung, để vẽ đồng hồ.
pub struct FrameInfo {
    pub in_peak: f32,
    pub out_peak: f32,
    pub gate_open: bool,
    pub vad: f32,
    /// Đỉnh tín hiệu ngay trước cổng ồn (sau RNNoise và cắt ù), dùng để tự đặt ngưỡng.
    pub gate_peak: f32,
}

pub struct Chain {
    dn: Box<DenoiseState<'static>>,
    first: bool,
    dly: Vec<f32>, // trễ cho nhánh khô, bằng độ trễ của RNNoise (1 khung)
    dly_i: usize,
    hp: Biquad,
    low: Biquad,
    mud: Biquad,
    pres: Biquad,
    air: Biquad,
    prev: Option<Params>,
    // cổng ồn + đường trễ nhìn trước
    env: f32,
    g: f32,
    open: bool,
    hold: u32,
    look: Vec<f32>,
    look_i: usize,
    // tự cân âm lượng
    agc_pow: f32,
    agc_db: f32,
    agc_k: f32,
    // nén
    cenv: f32,
    // âm lượng + chống vỡ
    gain_cur: f32,
    lim_g: f32,
    // hiệu ứng
    pitch: Pitch,
    robot: Robot,
    reverb: Reverb,
    // hằng số thời gian
    env_decay: f32,
    a_up: f32,
    a_down: f32,
    hold_samples: u32,
    c_att: f32,
    c_rel: f32,
    lim_rel: f32,
}

impl Chain {
    pub fn new() -> Self {
        let k = |t: f32| (-1.0 / (SR * t)).exp();
        Self {
            dn: DenoiseState::new(),
            first: true,
            dly: vec![0.0; FRAME],
            dly_i: 0,
            hp: Biquad::new(),
            low: Biquad::new(),
            mud: Biquad::new(),
            pres: Biquad::new(),
            air: Biquad::new(),
            prev: None,
            env: 0.0,
            g: 1.0,
            open: true,
            hold: 0,
            look: vec![0.0; LOOK],
            look_i: 0,
            agc_pow: 0.0,
            agc_db: 0.0,
            agc_k: 1.0 - k(0.3),
            cenv: 0.0,
            gain_cur: 1.0,
            lim_g: 1.0,
            pitch: Pitch::new(),
            robot: Robot::new(),
            reverb: Reverb::new(),
            env_decay: k(0.02),
            a_up: k(0.0015), // mở rất nhanh: nhờ nhìn trước 5 ms nên đầu chữ vẫn nguyên vẹn
            a_down: k(0.18),
            hold_samples: (SR * 0.12) as u32,
            c_att: 1.0 - k(0.006),
            c_rel: 1.0 - k(0.2),
            lim_rel: k(0.05),
        }
    }

    fn update_coefs(&mut self, p: &Params) {
        let pv = self.prev;
        let changed = |f: fn(&Params) -> f32| pv.map_or(true, |o| f(&o) != f(p));
        if changed(|p| p.hp) {
            self.hp.highpass(p.hp, 0.707);
        }
        if changed(|p| p.low) {
            self.low.lowshelf(120.0, p.low);
        }
        if changed(|p| p.mud) {
            self.mud.peaking(280.0, 1.0, p.mud);
        }
        if changed(|p| p.pres) {
            self.pres.peaking(3200.0, 0.9, p.pres);
        }
        if changed(|p| p.air) {
            self.air.highshelf(9000.0, p.air);
        }
        self.prev = Some(*p);
    }

    pub fn process(&mut self, inp: &[f32; FRAME], p: &Params, out: &mut [f32; FRAME]) -> FrameInfo {
        self.update_coefs(p);
        let in_peak = inp.iter().fold(0f32, |m, &v| m.max(v.abs()));

        // ---- 1. RNNoise (trộn khô/ướt, nhánh khô được trễ cho khớp pha) ----
        let mut x = [0f32; FRAME];
        let mut vad = 0.0;
        if p.ai {
            let mut scaled = [0f32; FRAME];
            for i in 0..FRAME {
                scaled[i] = inp[i] * 32768.0;
            }
            let mut den = [0f32; FRAME];
            vad = self.dn.process_frame(&mut den, &scaled);
            if self.first {
                self.first = false; // khung đầu của RNNoise có tiếng "bụp", bỏ đi
                den = [0.0; FRAME];
            }
            let mix = (p.ai_mix / 100.0).clamp(0.0, 1.0);
            for i in 0..FRAME {
                let dry = self.dly[self.dly_i];
                self.dly[self.dly_i] = inp[i];
                self.dly_i = (self.dly_i + 1) % FRAME;
                x[i] = dry * (1.0 - mix) + (den[i] / 32768.0) * mix;
            }
        } else {
            self.first = true;
            for i in 0..FRAME {
                self.dly[self.dly_i] = inp[i];
                self.dly_i = (self.dly_i + 1) % FRAME;
            }
            x = *inp;
        }

        // ---- 2..9 theo từng mẫu ----
        let thr = db2lin(p.thr);
        let floor = db2lin(-p.depth);
        let a = (p.comp / 100.0).clamp(0.0, 1.0);
        let (c_thr, ratio, knee) = (-10.0 - a * 30.0, 1.0 + a * 5.0, 12.0f32);
        let gain_t = db2lin(p.gain);
        let lim_thr = db2lin(-3.0);
        let use_pitch = p.fx_pitch.abs() > 0.4;
        let pitch_ratio = 2f32.powf(p.fx_pitch / 12.0);
        let wet = (p.fx_reverb / 100.0) * 1.6;
        let mut out_peak = 0f32;
        let mut gate_peak = 0f32;

        for i in 0..FRAME {
            let mut s = self.hp.run(x[i]);

            // cổng ồn: bộ dò nhìn tín hiệu hiện tại, còn âm thanh đi qua đường trễ 5 ms
            let ax = s.abs();
            gate_peak = gate_peak.max(ax);
            self.env = if ax > self.env { ax } else { self.env * self.env_decay };
            if p.gate_on {
                if self.env > thr {
                    self.open = true;
                    self.hold = self.hold_samples;
                } else if self.env < thr * 0.7 {
                    if self.hold > 0 {
                        self.hold -= 1;
                    } else {
                        self.open = false;
                    }
                }
            } else {
                self.open = true;
            }
            let target = if self.open { 1.0 } else { floor };
            let coef = if target > self.g { self.a_up } else { self.a_down };
            self.g = target + coef * (self.g - target);
            let delayed = self.look[self.look_i];
            self.look[self.look_i] = s;
            self.look_i = (self.look_i + 1) % LOOK;
            s = delayed * self.g;

            // EQ
            s = self.low.run(s);
            s = self.mud.run(s);
            s = self.pres.run(s);
            s = self.air.run(s);

            // tự cân âm lượng: đo mức RMS chậm, chỉ chỉnh khi có tiếng nói (trên -50 dB)
            if p.agc {
                self.agc_pow += (s * s - self.agc_pow) * self.agc_k;
                let rms_db = 10.0 * self.agc_pow.max(1e-12).log10();
                if rms_db > AGC_FLOOR_DB {
                    let want = (AGC_TARGET_DB - rms_db).clamp(-12.0, 18.0);
                    let rate = if want < self.agc_db { 10.0 / SR } else { 2.0 / SR };
                    self.agc_db += (want - self.agc_db).clamp(-rate, rate);
                }
                s *= db2lin(self.agc_db);
            } else {
                self.agc_db = 0.0;
                self.agc_pow = 0.0;
            }

            // nén (đầu gối mềm)
            if ratio > 1.001 {
                let ax = s.abs();
                let k = if ax > self.cenv { self.c_att } else { self.c_rel };
                self.cenv += (ax - self.cenv) * k;
                let xdb = lin2db(self.cenv);
                let over = xdb - c_thr;
                let ydb = if 2.0 * over < -knee {
                    xdb
                } else if 2.0 * over.abs() <= knee {
                    xdb + (1.0 / ratio - 1.0) * (over + knee / 2.0).powi(2) / (2.0 * knee)
                } else {
                    c_thr + over / ratio
                };
                s *= db2lin(ydb - xdb);
            }

            // âm lượng (làm mượt để kéo thanh không bị lạo xạo)
            self.gain_cur += (gain_t - self.gain_cur) * 0.002;
            s *= self.gain_cur;

            // hiệu ứng giọng
            if use_pitch {
                s = self.pitch.process(s, pitch_ratio);
            }
            if p.fx_robot {
                s = self.robot.process(s);
            }
            if wet > 0.01 {
                s += self.reverb.process(s) * wet;
            }

            // chống vỡ tiếng
            let need = (lim_thr / s.abs().max(1e-9)).min(1.0);
            if need < self.lim_g {
                self.lim_g = need;
            } else {
                self.lim_g = 1.0 + self.lim_rel * (self.lim_g - 1.0);
            }
            s = (s * self.lim_g).clamp(-1.0, 1.0);

            if p.mute {
                s = 0.0;
            }
            out[i] = s;
            out_peak = out_peak.max(s.abs());
        }

        FrameInfo { in_peak, out_peak, gate_open: self.open, vad, gate_peak }
    }
}

// ---------- đổi tần số lấy mẫu tuyến tính (có trạng thái) ----------
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self { step: from as f64 / to as f64, pos: 0.0, prev: 0.0 }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if (self.step - 1.0).abs() < 1e-9 {
            out.extend_from_slice(input);
            return;
        }
        let n = input.len();
        let mut idx = self.pos;
        while idx < n as f64 {
            let i0 = idx.floor() as usize;
            let frac = (idx - i0 as f64) as f32;
            let a = if i0 == 0 { self.prev } else { input[i0 - 1] };
            let b = input[i0];
            out.push(a + (b - a) * frac);
            idx += self.step;
        }
        self.pos = idx - n as f64;
        self.prev = input[n - 1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::preset;

    fn clean() -> Params {
        // không AI, không cổng, không nén, không EQ: tín hiệu đi thẳng để dễ đối chiếu
        Params { ai: false, gate_on: false, comp: 0.0, gain: 0.0, low: 0.0, mud: 0.0, pres: 0.0, air: 0.0, hp: 40.0, ..preset("raw") }
    }

    /// Chạy `secs` giây qua chuỗi, trả về toàn bộ đầu ra.
    fn run(p: &Params, secs: f32, mut gen: impl FnMut(usize) -> f32) -> Vec<f32> {
        let mut ch = Chain::new();
        let mut all = Vec::new();
        let (mut inp, mut out) = ([0f32; FRAME], [0f32; FRAME]);
        let frames = (secs * SR) as usize / FRAME;
        for f in 0..frames {
            for i in 0..FRAME {
                inp[i] = gen(f * FRAME + i);
            }
            ch.process(&inp, p, &mut out);
            all.extend_from_slice(&out);
        }
        all
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    fn sine(f: f32, a: f32) -> impl FnMut(usize) -> f32 {
        move |n| a * (2.0 * PI * f * n as f32 / SR).sin()
    }

    fn crossings(v: &[f32]) -> usize {
        v.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    #[test]
    fn pitch_len_len_doubles_frequency() {
        let p = Params { fx_pitch: 12.0, ..clean() };
        let out = run(&p, 1.5, sine(440.0, 0.3));
        let tail = &out[out.len() - 24_000..]; // 0,5 s cuối
        let hz = crossings(tail) as f32 / 0.5;
        assert!((hz - 880.0).abs() < 60.0, "tần số sau +12 bán cung: {hz}");
        let p = Params { fx_pitch: -12.0, ..clean() };
        let out = run(&p, 1.5, sine(440.0, 0.3));
        let hz = crossings(&out[out.len() - 24_000..]) as f32 / 0.5;
        assert!((hz - 220.0).abs() < 40.0, "tần số sau -12 bán cung: {hz}");
    }

    #[test]
    fn pitch_off_is_transparent() {
        let out = run(&clean(), 0.5, sine(440.0, 0.3));
        let tail = &out[out.len() - 4800..];
        assert!(rms(tail) > 0.19, "đầu ra khi tắt hiệu ứng: {}", rms(tail));
    }

    #[test]
    fn agc_raises_quiet_voice_and_leaves_noise_alone() {
        // giọng nhỏ (~ -38 dBFS RMS): AGC phải nâng lên rõ rệt
        let p = Params { agc: true, ..clean() };
        let out = run(&p, 18.0, sine(500.0, 0.02));
        let (first, last) = (rms(&out[..24_000]), rms(&out[out.len() - 24_000..]));
        assert!(last > first * 3.0, "AGC chưa nâng đủ: {first} -> {last}");
        assert!(lin2db(last) < -12.0, "AGC nâng quá tay: {} dB", lin2db(last));
        // nền ồn rất nhỏ (< -50 dB): không được khuếch đại
        let out = run(&p, 6.0, sine(500.0, 0.001));
        assert!(rms(&out[out.len() - 24_000..]) < 0.002, "AGC khuếch đại cả nền ồn");
    }

    #[test]
    fn lookahead_gate_keeps_word_onset() {
        let p = Params { gate_on: true, thr: -40.0, depth: 60.0, ..clean() };
        // 0,5 s im lặng rồi sóng 1 kHz biên độ 0,3: ngay khi tiếng tới đầu ra phải gần đủ biên độ
        let onset = 24_000;
        let out = run(&p, 1.0, |n| if n < onset { 0.0 } else { 0.3 * (2.0 * PI * 1000.0 * (n - onset) as f32 / SR).sin() });
        let win = &out[onset + LOOK + 48..onset + LOOK + 480];
        let peak = win.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.25, "đầu chữ bị cổng cắt: đỉnh {peak}");
        // và lúc im lặng thì cổng đóng
        let quiet = run(&p, 1.0, |n| 0.002 * (2.0 * PI * 300.0 * n as f32 / SR).sin());
        assert!(rms(&quiet[quiet.len() - 4800..]) < 0.0005, "cổng không đóng với nền ồn");
    }

    #[test]
    fn reverb_is_stable_and_adds_tail() {
        let p = Params { fx_reverb: 100.0, ..clean() };
        // xung đơn
        let out = run(&p, 2.0, |n| if n == 0 { 0.5 } else { 0.0 });
        assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        let early = rms(&out[2_000..12_000]);
        let late = rms(&out[80_000..96_000]);
        assert!(early > 1e-4, "vang không có đuôi: {early}");
        assert!(late < early, "đuôi vang không tắt dần: {early} -> {late}");
    }

    #[test]
    fn robot_stays_bounded() {
        let p = Params { fx_robot: true, ..clean() };
        let out = run(&p, 1.0, sine(300.0, 0.5));
        assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        assert!(rms(&out[24_000..]) > 0.05);
    }

    #[test]
    fn spectrum_finds_the_tone() {
        let mut s = [0f32; FFT_N];
        for (i, v) in s.iter_mut().enumerate() {
            *v = 0.5 * (2.0 * PI * 1000.0 * i as f32 / SR).sin();
        }
        let mut bands = [0f32; BANDS];
        spectrum_bands(&s, &mut bands);
        let (best, &lvl) = bands.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).unwrap();
        let ratio = (16_000f32 / 40.0).powf(1.0 / BANDS as f32);
        let centre = 40.0 * ratio.powf(best as f32 + 0.5);
        assert!((centre / 1000.0 - 1.0).abs() < 0.2, "đỉnh phổ ở {centre} Hz");
        assert!((lvl - (-6.0)).abs() < 3.5, "mức đo được {lvl} dBFS (kỳ vọng ≈ -6)");
    }

    #[test]
    fn eq_curve_matches_settings() {
        let p = Params { low: 6.0, pres: -4.0, hp: 100.0, ..clean() };
        let at = |f| eq_response_db(&p, f);
        assert!(at(30.0) < -10.0, "cắt ù tại 30 Hz: {}", at(30.0));
        assert!((at(3200.0) - (-4.0)).abs() < 1.5, "rõ giọng tại 3,2 kHz: {}", at(3200.0));
        assert!(at(8000.0).abs() < 2.0, "băng cao tại 8 kHz: {}", at(8000.0));
    }
}
