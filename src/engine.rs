//! Lõi âm thanh: hàng đợi nhận từ iPhone -> luồng DSP -> hàng đợi xuất -> thiết bị (VB-Cable).
//!
//! Luồng DSP chỉ xử lý thêm khi hàng đợi xuất còn ít, nên bộ đệm chống giật nằm ở hàng đợi
//! nhận và độ trễ tổng ≈ buf_ms + ~30 ms.

use crate::i18n::{t, tf};
use crate::dsp::{lin2db, spectrum_bands, Chain, Resampler, BANDS, FFT_N, FRAME, SR};
use crate::settings::{apply_profile, Params, Profile};
use crate::wav::WavWriter;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use parking_lot::{Mutex, RwLock};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering::Relaxed};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

static START: OnceLock<Instant> = OnceLock::new();
/// Mili-giây kể từ lúc app khởi động (đồng hồ chung cho các luồng).
pub fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub struct InputQ {
    pub q: VecDeque<f32>,
    pub primed: bool,
}

pub type Queue = Arc<Mutex<VecDeque<f32>>>;

pub struct Shared {
    pub params: RwLock<Params>,
    /// Tăng mỗi khi thông số đổi (từ app PC hoặc từ iPhone) để các bên biết mà đồng bộ.
    pub params_ver: AtomicU64,
    pub input: Mutex<InputQ>,
    pub out_q: Queue,
    pub out_rate: AtomicU32, // 0 = chưa có thiết bị xuất
    pub mon_q: Queue,
    pub mon_rate: AtomicU32,
    pub connected: AtomicBool,
    pub conn_gen: AtomicU64,
    // đồng hồ
    pub in_db: AtomicU32,
    pub out_db: AtomicU32,
    pub vad: AtomicU32,
    pub gate_open: AtomicBool,
    // thống kê
    pub buf_ms: AtomicU32,
    pub under: AtomicU32,
    pub drops: AtomicU32,
    // trạng thái hiển thị
    pub url: Mutex<String>,
    pub server_err: Mutex<Option<String>>,
    pub out_err: Mutex<Option<String>>,
    /// Tên thiết bị đầu ra đang chọn (để lưu khi thoát từ khay).
    pub out_name: Mutex<String>,
    /// Mã ghép nối hiện hành (đổi được từ giao diện, iPhone phải gửi đúng mã mới được nối).
    pub token: RwLock<String>,
    /// Phát tiếng bíp khi bật/tắt tiếng bằng phím tắt hoặc menu khay.
    pub beep: AtomicBool,
    // tự đặt ngưỡng cổng ồn: 0 rảnh, 1 đang đo, 2 xong, 3 thất bại
    pub cal_req: AtomicBool,
    pub cal_state: AtomicU8,
    pub cal_progress: AtomicU32,
    pub cal_noise: AtomicU32,
    /// Hồ sơ riêng (nguồn chính; giao diện và iPhone cùng đọc/ghi).
    pub profiles: RwLock<Vec<Profile>>,
    /// Phổ âm đầu ra (dBFS theo 48 băng). Chỉ được cập nhật khi giao diện đang xem (`spec_until`).
    pub spectrum: Mutex<[f32; BANDS]>,
    pub spec_until: AtomicU64,
    /// Thời gian khứ hồi tới iPhone (ms, làm mượt), 0 = chưa đo.
    pub rtt_ms: AtomicU32,
    // ghi âm WAV: 1 = bắt đầu, 2 = dừng (luồng DSP xử lý)
    pub rec_req: AtomicU8,
    pub rec_on: AtomicBool,
    pub rec_frames: AtomicU64,
    pub rec_path: Mutex<String>,
    pub rec_err: Mutex<Option<String>>,
    /// Ghi chú khi đọc cài đặt (ví dụ đã khôi phục từ bản sao lưu).
    pub settings_note: Mutex<String>,
    /// Lý do của lần ngắt kết nối gần nhất (để chẩn đoán).
    pub last_disconnect: Mutex<String>,
    /// Đánh thức giao diện để vẽ lại khi có thay đổi (do luồng khác gọi). Chỉ có tác dụng khi cửa sổ đang hiện.
    pub wake: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

pub fn store_f(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Relaxed);
}
pub fn load_f(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Relaxed))
}

impl Shared {
    pub fn new(p: Params, token: String) -> Arc<Self> {
        Arc::new(Self {
            token: RwLock::new(token),
            beep: AtomicBool::new(true),
            cal_req: AtomicBool::new(false),
            cal_state: AtomicU8::new(0),
            cal_progress: AtomicU32::new(0),
            cal_noise: AtomicU32::new((-99f32).to_bits()),
            profiles: RwLock::new(Vec::new()),
            spectrum: Mutex::new([-100.0; BANDS]),
            spec_until: AtomicU64::new(0),
            rtt_ms: AtomicU32::new(0),
            rec_req: AtomicU8::new(0),
            rec_on: AtomicBool::new(false),
            rec_frames: AtomicU64::new(0),
            rec_path: Mutex::new(String::new()),
            rec_err: Mutex::new(None),
            settings_note: Mutex::new(String::new()),
            last_disconnect: Mutex::new(String::new()),
            wake: OnceLock::new(),
            params: RwLock::new(p),
            params_ver: AtomicU64::new(1),
            input: Mutex::new(InputQ { q: VecDeque::new(), primed: false }),
            out_q: Arc::new(Mutex::new(VecDeque::new())),
            out_rate: AtomicU32::new(0),
            mon_q: Arc::new(Mutex::new(VecDeque::new())),
            mon_rate: AtomicU32::new(0),
            connected: AtomicBool::new(false),
            conn_gen: AtomicU64::new(0),
            in_db: AtomicU32::new((-99f32).to_bits()),
            out_db: AtomicU32::new((-99f32).to_bits()),
            vad: AtomicU32::new(0),
            gate_open: AtomicBool::new(true),
            buf_ms: AtomicU32::new(0),
            under: AtomicU32::new(0),
            drops: AtomicU32::new(0),
            url: Mutex::new(String::new()),
            server_err: Mutex::new(None),
            out_err: Mutex::new(None),
            out_name: Mutex::new(String::new()),
        })
    }

    /// Server gọi khi nhận được âm thanh 48 kHz mono.
    pub fn push_input(&self, samples: &[f32]) {
        let target = (self.params.read().buf_ms / 1000.0 * SR) as usize;
        let mut iq = self.input.lock();
        iq.q.extend(samples.iter().copied());
        let limit = target + (SR * 0.15) as usize;
        if iq.q.len() > limit {
            // mạng dồn gói: bỏ phần cũ để không tích trễ
            let cut = iq.q.len() - target;
            iq.q.drain(..cut);
            self.drops.fetch_add(1, Relaxed);
        }
    }

    pub fn reset_input(&self) {
        let mut iq = self.input.lock();
        iq.q.clear();
        iq.primed = false;
        self.under.store(0, Relaxed);
        self.drops.store(0, Relaxed);
    }

    pub fn set_params(&self, p: Params) {
        *self.params.write() = p.sanitized();
        self.params_ver.fetch_add(1, Relaxed);
        self.wake_ui();
    }

    /// Báo giao diện vẽ lại (nếu cửa sổ đang hiện). Cửa sổ ẩn thì không gọi: nó không bao giờ vẽ được
    /// nên yêu cầu vẽ treo sẽ làm vòng lặp sự kiện quay không nghỉ.
    pub fn wake_ui(&self) {
        if let Some(f) = self.wake.get() {
            if crate::win::is_visible() {
                f();
            }
        }
    }

    /// Áp một hồ sơ riêng theo tên. Trả về false nếu không có hồ sơ đó.
    pub fn apply_profile_named(&self, name: &str) -> bool {
        let prof = self.profiles.read().iter().find(|p| p.name == name).cloned();
        match prof {
            Some(prof) => {
                let cur = *self.params.read();
                self.set_params(apply_profile(&cur, &prof));
                true
            }
            None => false,
        }
    }

    /// Độ trễ ước tính từ lúc nói đến lúc tiếng ra thiết bị (ms). Gồm: lấy mẫu trên iPhone,
    /// đường truyền (nửa khứ hồi), đệm chống giật, hàng đợi xuất, xử lý và bộ đệm của thiết bị.
    pub fn latency_ms(&self) -> u32 {
        let p = *self.params.read();
        let rate = self.out_rate.load(Relaxed).max(1) as f32;
        let out_q = self.out_q.lock().len() as f32 * 1000.0 / rate;
        let rtt = self.rtt_ms.load(Relaxed) as f32;
        let net = if rtt > 0.0 { rtt / 2.0 } else { 5.0 };
        let dsp = 5.0 + if p.ai { 10.0 } else { 0.0 } + if p.fx_pitch.abs() > 0.4 { 21.0 } else { 0.0 };
        let total = 30.0 + net + self.buf_ms.load(Relaxed) as f32 + out_q + dsp + 15.0;
        total.round() as u32
    }

    /// Gộp một phần thông số từ iPhone (JSON). Không cho iPhone đổi `monitor` (nghe thử trên PC).
    pub fn merge_json(&self, v: &serde_json::Value) {
        let cur = *self.params.read();
        let (Ok(mut base), Some(inc)) = (serde_json::to_value(cur), v.as_object()) else { return };
        let Some(obj) = base.as_object_mut() else { return };
        for (k, val) in inc {
            if k != "monitor" && obj.contains_key(k) {
                obj.insert(k.clone(), val.clone());
            }
        }
        if let Ok(p) = serde_json::from_value::<Params>(base) {
            self.set_params(p);
        }
    }
}

// ---------------- luồng DSP ----------------
pub fn spawn_dsp(sh: Arc<Shared>) {
    std::thread::Builder::new()
        .name("dsp".into())
        .spawn(move || {
            let mut chain = Chain::new();
            let mut rs_out = Resampler::new(48_000, 48_000);
            let mut rs_mon = Resampler::new(48_000, 48_000);
            let (mut r_out, mut r_mon) = (48_000u32, 48_000u32);
            let mut frame = [0f32; FRAME];
            let mut out = [0f32; FRAME];
            let mut tmp: Vec<f32> = Vec::with_capacity(2048);
            let mut idle = 0u32;
            // tự đặt ngưỡng: (lúc bắt đầu, mức đỉnh dB của từng khung)
            let mut cal: Option<(Instant, Vec<f32>)> = None;
            let mut cal_done_at: Option<Instant> = None;
            // phổ âm và ghi âm
            let mut spec_ring = [0f32; FFT_N];
            let mut spec_pos = 0usize;
            let mut spec_tick = 0u32;
            let mut rec: Option<WavWriter> = None;

            loop {
                let p = *sh.params.read();
                let rate = sh.out_rate.load(Relaxed);

                match sh.rec_req.swap(0, Relaxed) {
                    1 if rec.is_none() => match start_recording() {
                        Ok((w, path)) => {
                            rec = Some(w);
                            *sh.rec_path.lock() = path;
                            *sh.rec_err.lock() = None;
                            sh.rec_frames.store(0, Relaxed);
                            sh.rec_on.store(true, Relaxed);
                        }
                        Err(e) => *sh.rec_err.lock() = Some(e),
                    },
                    2 => {
                        if let Some(w) = rec.take() {
                            if let Err(e) = w.finish() {
                                *sh.rec_err.lock() = Some(tf("rec.err.close", &[&e.to_string()]));
                            }
                            sh.rec_on.store(false, Relaxed);
                        }
                    }
                    _ => {}
                }

                if sh.cal_req.swap(false, Relaxed) {
                    cal = Some((Instant::now(), Vec::new()));
                    cal_done_at = None;
                    sh.cal_state.store(1, Relaxed);
                    store_f(&sh.cal_progress, 0.0);
                }
                // đang đo mà quá 7 giây không có âm thanh (iPhone ngắt kết nối): báo thất bại
                if cal.as_ref().is_some_and(|(t0, _)| t0.elapsed() > Duration::from_secs(7)) {
                    cal = None;
                    sh.cal_state.store(3, Relaxed);
                    cal_done_at = Some(Instant::now());
                }
                // kết quả hiện 8 giây rồi tự xóa
                if cal_done_at.is_some_and(|t| t.elapsed() > Duration::from_secs(8)) {
                    sh.cal_state.store(0, Relaxed);
                    cal_done_at = None;
                }

                // Chỉ chạy khi hàng đợi xuất còn dưới ~30 ms
                let room = rate != 0 && sh.out_q.lock().len() < (rate as usize * 30) / 1000;
                let mut got = false;
                if room {
                    let target = ((p.buf_ms / 1000.0 * SR) as usize).max(FRAME);
                    let mut iq = sh.input.lock();
                    if !iq.primed && iq.q.len() >= target {
                        iq.primed = true;
                    }
                    if iq.primed {
                        if iq.q.len() >= FRAME {
                            for s in frame.iter_mut() {
                                *s = iq.q.pop_front().unwrap_or(0.0);
                            }
                            got = true;
                        } else {
                            iq.primed = false;
                            sh.under.fetch_add(1, Relaxed);
                        }
                    }
                    sh.buf_ms.store((iq.q.len() as f32 * 1000.0 / SR) as u32, Relaxed);
                }

                if !got {
                    // Không có âm thanh: đo mức rơi dần về im lặng, đẩy im lặng ra thiết bị
                    // để mic ảo không bị treo tiếng cuối.
                    idle += 1;
                    if idle > 30 {
                        store_f(&sh.in_db, -99.0);
                        store_f(&sh.out_db, -99.0);
                    }
                    // Có iPhone nối thì thức dậy dày để bắt kịp gói; chưa có thì ngủ lâu cho đỡ tốn điện
                    let busy = sh.connected.load(Relaxed) || cal.is_some();
                    std::thread::sleep(Duration::from_millis(if busy { 3 } else { 30 }));
                    continue;
                }
                idle = 0;

                let info = chain.process(&frame, &p, &mut out);
                store_f(&sh.in_db, lin2db(info.in_peak));
                store_f(&sh.out_db, lin2db(info.out_peak));
                store_f(&sh.vad, info.vad);
                sh.gate_open.store(info.gate_open, Relaxed);

                if let Some(w) = rec.as_mut() {
                    if let Err(e) = w.write(&out) {
                        *sh.rec_err.lock() = Some(tf("rec.err.write", &[&e.to_string()]));
                        rec = None;
                        sh.rec_on.store(false, Relaxed);
                    } else {
                        sh.rec_frames.fetch_add(1, Relaxed);
                    }
                }

                // phổ âm: gom 1024 mẫu đầu ra gần nhất, tính ~33 lần/giây và chỉ khi giao diện đang xem
                for &v in out.iter() {
                    spec_ring[spec_pos] = v;
                    spec_pos = (spec_pos + 1) % FFT_N;
                }
                spec_tick += 1;
                if spec_tick >= 3 {
                    spec_tick = 0;
                    if now_ms() < sh.spec_until.load(Relaxed) {
                        let mut ordered = [0f32; FFT_N];
                        for i in 0..FFT_N {
                            ordered[i] = spec_ring[(spec_pos + i) % FFT_N];
                        }
                        let mut bands = [0f32; BANDS];
                        spectrum_bands(&ordered, &mut bands);
                        *sh.spectrum.lock() = bands;
                    }
                }

                if let Some((t0, db)) = cal.as_mut() {
                    db.push(lin2db(info.gate_peak));
                    let el = t0.elapsed().as_secs_f32();
                    store_f(&sh.cal_progress, (el / 3.0).min(1.0));
                    if el >= 3.0 {
                        if let Some((_, db)) = cal.take() {
                            finish_calibration(&sh, db);
                            cal_done_at = Some(Instant::now());
                        }
                    }
                }

                if rate != r_out {
                    rs_out = Resampler::new(48_000, rate);
                    r_out = rate;
                }
                tmp.clear();
                rs_out.process(&out, &mut tmp);
                push_q(&sh.out_q, &tmp, r_out);

                // nghe thử
                let mrate = sh.mon_rate.load(Relaxed);
                if p.monitor && mrate != 0 {
                    if mrate != r_mon {
                        rs_mon = Resampler::new(48_000, mrate);
                        r_mon = mrate;
                    }
                    tmp.clear();
                    rs_mon.process(&out, &mut tmp);
                    push_q(&sh.mon_q, &tmp, r_mon);
                }
            }
        })
        .expect("could not spawn the DSP thread");
}

/// Mở file WAV mới trong Documents\LarmaMic, tên theo giờ hiện tại.
fn start_recording() -> Result<(WavWriter, String), String> {
    let docs = std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| t("rec.err.home"))?
        .join("Documents")
        .join("LarmaMic");
    std::fs::create_dir_all(&docs).map_err(|e| tf("rec.err.dir", &[&e.to_string()]))?;
    let (y, mo, d, h, mi, s) = crate::win::local_time();
    let path = docs.join(format!("rec-{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}.wav"));
    let w = WavWriter::create(&path, 48_000).map_err(|e| tf("rec.err.file", &[&e.to_string()]))?;
    Ok((w, path.display().to_string()))
}

/// Nền ồn = mức đỉnh ở phân vị 95 của các khung trong 3 giây im lặng; ngưỡng cổng đặt cao hơn 7 dB.
fn finish_calibration(sh: &Shared, mut db: Vec<f32>) {
    if db.len() < 100 {
        sh.cal_state.store(3, Relaxed);
        return;
    }
    db.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise = db[(db.len() * 95 / 100).min(db.len() - 1)];
    store_f(&sh.cal_noise, noise);
    let mut p = *sh.params.read();
    p.thr = (noise + 7.0).clamp(-72.0, -20.0).round();
    sh.set_params(p);
    sh.cal_state.store(2, Relaxed);
    sh.wake_ui();
}

fn push_q(q: &Queue, data: &[f32], rate: u32) {
    let mut q = q.lock();
    q.extend(data.iter().copied());
    let cap = rate as usize / 5; // tối đa 200 ms
    if q.len() > cap {
        let cut = q.len() - cap;
        q.drain(..cut);
    }
}

// ---------------- thiết bị âm thanh ----------------
pub enum Cmd {
    SetOut(String),
    SetMonitor(bool),
}

pub fn list_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut v: Vec<String> = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    v.sort();
    v.dedup();
    v
}

pub fn default_output_name() -> Option<String> {
    cpal::default_host().default_output_device().and_then(|d| d.name().ok())
}

/// Loại mic ảo: VB-Cable, hoặc Steam Streaming Microphone (driver đi kèm Steam).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Virt {
    Cable,
    Steam,
}

impl Virt {
    /// Tên thiết bị thu mà Discord/OBS/game phải chọn làm micro.
    pub fn mic_name(self) -> &'static str {
        match self {
            Virt::Cable => "CABLE Output",
            Virt::Steam => "Steam Streaming Microphone",
        }
    }
}

/// Thiết bị phát này là đầu vào của một mic ảo nào?
/// VB-Cable: bản cài cũ tên "CABLE Input (VB-Audio Virtual Cable)", bản cài lại có thể thành
/// "Speakers (2- VB-Audio Virtual Cable)"; bỏ qua bản 16 kênh ("CABLE In 16 Ch").
/// Steam: "Steam Streaming Speakers" (cặp với micro "Steam Streaming Microphone").
pub fn virt_kind(name: &str) -> Option<Virt> {
    let l = name.to_lowercase();
    if l.contains("cable input") || (l.contains("virtual cable") && !l.contains("16")) {
        Some(Virt::Cable)
    } else if l.contains("steam streaming speakers") {
        Some(Virt::Steam)
    } else {
        None
    }
}

/// Chọn mic ảo trong danh sách thiết bị phát: ưu tiên VB-Cable, không có thì dùng Steam.
pub fn find_virtual(devs: &[String]) -> Option<String> {
    devs.iter()
        .find(|n| n.to_lowercase().contains("cable input"))
        .or_else(|| devs.iter().find(|n| virt_kind(n) == Some(Virt::Cable)))
        .or_else(|| devs.iter().find(|n| virt_kind(n) == Some(Virt::Steam)))
        .cloned()
}

pub fn is_virtual(name: &str) -> bool {
    virt_kind(name).is_some()
}

fn make_stream<T>(dev: &cpal::Device, cfg: &cpal::StreamConfig, q: Queue, rate: u32) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let ch = cfg.channels as usize;
    let min = (rate / 100) as usize; // bắt đầu phát khi có ≥ 10 ms
    let mut primed = false;
    dev.build_output_stream(
        cfg,
        move |data: &mut [T], _| {
            let mut q = q.lock();
            if !primed && q.len() >= min {
                primed = true;
            }
            for frame in data.chunks_mut(ch) {
                let v = if primed {
                    match q.pop_front() {
                        Some(v) => v,
                        None => {
                            primed = false;
                            0.0
                        }
                    }
                } else {
                    0.0
                };
                let s = T::from_sample(v);
                for o in frame.iter_mut() {
                    *o = s;
                }
            }
        },
        |e| eprintln!("audio stream error: {e}"),
        None,
    )
    .map_err(|e| e.to_string())
}

fn open(dev: &cpal::Device, q: Queue) -> Result<(cpal::Stream, u32), String> {
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    let rate = cfg.sample_rate().0;
    let sc: cpal::StreamConfig = cfg.clone().into();
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => make_stream::<f32>(dev, &sc, q, rate),
        SampleFormat::I16 => make_stream::<i16>(dev, &sc, q, rate),
        SampleFormat::I32 => make_stream::<i32>(dev, &sc, q, rate),
        SampleFormat::U16 => make_stream::<u16>(dev, &sc, q, rate),
        f => Err(tf("audio.err.format", &[&format!("{f:?}")])),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}

fn find_device(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.output_devices().ok()?.find(|d| d.name().map(|n| n == name).unwrap_or(false))
}

/// cpal::Stream không chuyển luồng được trên Windows, nên một luồng riêng giữ chúng.
pub fn spawn_audio_manager(sh: Arc<Shared>) -> mpsc::Sender<Cmd> {
    let (tx, rx) = mpsc::channel::<Cmd>();
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            let host = cpal::default_host();
            let mut out_stream: Option<cpal::Stream> = None;
            let mut mon_stream: Option<cpal::Stream> = None;
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Cmd::SetOut(name) => {
                        out_stream = None;
                        sh.out_rate.store(0, Relaxed);
                        sh.out_q.lock().clear();
                        match find_device(&host, &name) {
                            None => *sh.out_err.lock() = Some(tf("out.err.missing", &[&name])),
                            Some(dev) => match open(&dev, sh.out_q.clone()) {
                                Ok((s, rate)) => {
                                    out_stream = Some(s);
                                    sh.out_rate.store(rate, Relaxed);
                                    *sh.out_err.lock() = None;
                                }
                                Err(e) => *sh.out_err.lock() = Some(tf("out.err.open", &[&name, &e.to_string()])),
                            },
                        }
                    }
                    Cmd::SetMonitor(on) => {
                        mon_stream = None;
                        sh.mon_rate.store(0, Relaxed);
                        sh.mon_q.lock().clear();
                        if on {
                            if let Some(dev) = host.default_output_device() {
                                if let Ok((s, rate)) = open(&dev, sh.mon_q.clone()) {
                                    mon_stream = Some(s);
                                    sh.mon_rate.store(rate, Relaxed);
                                }
                            }
                        }
                    }
                }
            }
            drop((out_stream, mon_stream));
        })
        .expect("could not spawn the audio thread");
    tx
}

#[cfg(test)]
mod virt_tests {
    use super::*;

    #[test]
    fn recognises_vb_cable_and_steam() {
        assert_eq!(virt_kind("CABLE Input (VB-Audio Virtual Cable)"), Some(Virt::Cable));
        assert_eq!(virt_kind("Speakers (2- VB-Audio Virtual Cable)"), Some(Virt::Cable));
        assert_eq!(virt_kind("CABLE In 16 Ch (VB-Audio Virtual Cable)"), None);
        assert_eq!(virt_kind("Steam Streaming Speakers"), Some(Virt::Steam));
        assert_eq!(virt_kind("Speakers (Realtek)"), None);
    }

    #[test]
    fn prefers_vb_cable_over_steam() {
        let d = vec!["Steam Streaming Speakers".to_string(), "CABLE Input (VB-Audio Virtual Cable)".to_string()];
        assert_eq!(find_virtual(&d).as_deref(), Some("CABLE Input (VB-Audio Virtual Cable)"));
        let only_steam = vec!["Speakers".to_string(), "Steam Streaming Speakers".to_string()];
        assert_eq!(find_virtual(&only_steam).as_deref(), Some("Steam Streaming Speakers"));
    }
}
