//! Lõi âm thanh: hàng đợi nhận từ điện thoại -> luồng DSP -> hàng đợi xuất -> thiết bị (VB-Cable).
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
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering::Relaxed};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

static START: OnceLock<Instant> = OnceLock::new();
/// Mili-giây kể từ lúc app khởi động (đồng hồ chung cho các luồng).
/// Làm tròn tới bội số của 5.
pub fn round5(v: f32) -> u32 {
    ((v / 5.0).round() * 5.0).max(0.0) as u32
}

/// Số hiển thị ổn định: chỉ đổi khi giá trị thật lệch hẳn khỏi số đang hiện (chống nhấp nháy ở ranh giới làm tròn).
pub fn stable(shown: &AtomicU32, v: f32) -> u32 {
    let cur = shown.load(Relaxed);
    if cur == 0 || (v - cur as f32).abs() >= 4.0 {
        let n = round5(v);
        shown.store(n, Relaxed);
        n
    } else {
        cur
    }
}

pub fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Một điện thoại đang xin ghép nối (tìm thấy PC qua mạng, chưa có mã). Người dùng bấm Cho phép / Từ chối trên PC.
pub struct PairReq {
    pub device: String,
    /// 0 chờ, 1 cho phép, 2 từ chối.
    pub decision: std::sync::atomic::AtomicU8,
}

/// Số điện thoại tối đa nối cùng lúc (máy thứ 5 vào thì máy cũ nhất bị ngắt).
pub const MAX_SOURCES: usize = 4;

struct SrcQ {
    q: VecDeque<f32>,
    primed: bool,
}

/// Một điện thoại đang nối: hàng đợi nhận riêng (bộ đệm chống giật riêng), tắt tiếng riêng.
/// Âm thanh của các máy được trộn lại trước khi vào chuỗi xử lý.
pub struct Source {
    pub id: u64,
    pub name: Mutex<String>,
    pub muted: AtomicBool,
    /// Giao diện bấm ngắt máy này: luồng của nó tự thoát.
    pub kicked: AtomicBool,
    /// Đỉnh (tuyến tính, bit của f32) của gói mới nhất, để giao diện vẽ thanh mức.
    pub peak: AtomicU32,
    /// Độ lợi riêng của máy này (tuyến tính, bit của f32), mặc định 1.0.
    pub gain: AtomicU32,
    q: Mutex<SrcQ>,
}

pub type Queue = Arc<Mutex<VecDeque<f32>>>;

pub struct Shared {
    pub params: RwLock<Params>,
    /// Tăng mỗi khi thông số đổi (từ app PC hoặc từ điện thoại) để các bên biết mà đồng bộ.
    pub params_ver: AtomicU64,
    pub sources: Mutex<Vec<Arc<Source>>>,
    /// true = chỉ lấy máy đang nói to nhất thay vì trộn tất cả.
    pub mix_loudest: AtomicBool,
    /// Tự đo phòng (xem room.rs): yêu cầu, trạng thái (0 rảnh, 1 đang đo, 2 xong, 3 không đo được), tiến độ, kết quả.
    pub room_req: AtomicBool,
    pub room_state: AtomicU8,
    pub room_progress: AtomicU32,
    pub room_result: Mutex<Option<crate::room::RoomResult>>,
    /// Cấu hình theo ứng dụng.
    pub auto_profile: AtomicBool,
    pub app_rules: Mutex<Vec<crate::appmatch::AppRule>>,
    pub app_active: Mutex<Option<String>>,
    /// Bản mới trên GitHub: (phiên bản, đường dẫn).
    pub update: Mutex<Option<(String, String)>>,
    pub update_check: AtomicBool,
    /// Soundboard: thư viện âm thanh, các âm đang phát, thông báo lỗi/đang tải.
    pub board: Mutex<crate::soundboard::Board>,
    pub board_ver: AtomicU64,
    /// id phím tắt toàn cục -> id âm thanh (Ctrl+Alt+1..9).
    pub sound_hk: Mutex<std::collections::HashMap<u32, u32>>,
    pub board_msg: Mutex<Option<String>>,
    pub board_busy: Mutex<bool>,
    pub playing: Mutex<Vec<crate::soundboard::Playing>>,
    /// Độ trễ và mức đệm đã làm mượt (bit của f32); số thô nhảy theo từng gói nên nhìn rất giật.
    pub lat_smooth: AtomicU32,
    pub buf_smooth: AtomicU32,
    pub lat_shown: AtomicU32,
    pub buf_shown: AtomicU32,
    /// Âm thanh PC đang phát (loopback, 48 kHz mono) làm tín hiệu tham chiếu cho khử vọng.
    pub ref_q: Queue,
    pub ref_on: AtomicBool,
    /// Âm thanh PC (cùng nguồn loopback) để trộn vào mic: bật/tắt, mức 0..100 (bit của f32), hạ nhỏ khi bạn nói.
    pub pc_q: Queue,
    pub pc_on: AtomicBool,
    pub pc_level: AtomicU32,
    pub pc_duck: AtomicBool,
    /// Tự chỉnh bộ đệm chống giật.
    pub auto_buf: AtomicBool,
    /// Phím Ctrl+Alt+F1..F10 đổi giọng: id phím -> số giọng.
    pub voice_hotkeys: AtomicBool,
    pub voice_hk: Mutex<std::collections::HashMap<u32, usize>>,
    /// API điều khiển cục bộ: bật/tắt và cổng đang nghe (0 = chưa nghe).
    pub api_on: AtomicBool,
    pub api_port: AtomicU32,
    /// Chữ thành tiếng (giọng đọc Windows): giọng, tốc độ, đang đọc, lỗi.
    pub tts_voice: Mutex<String>,
    pub tts_rate: AtomicI32,
    pub tts_busy: AtomicBool,
    pub tts_msg: Mutex<Option<String>>,
    /// Thử mic: yêu cầu, trạng thái (0 rảnh, 1 đang thu, 2 đang phát lại, 3 lỗi), tiến độ 0..1, lỗi.
    pub test_req: AtomicBool,
    pub test_state: AtomicU8,
    pub test_progress: AtomicU32,
    pub test_msg: Mutex<Option<String>>,
    /// Hướng dẫn lần đầu đã xem chưa.
    pub welcomed: AtomicBool,
    /// Tự tải bản mới: đang chạy, thông báo lỗi.
    pub upd_busy: AtomicBool,
    pub upd_msg: Mutex<Option<String>>,
    /// Lý do khử vọng không chạy được: None = ổn; Some("virtual") = loa mặc định là mic ảo; Some(khác) = lỗi mở loopback.
    pub aec_err: Mutex<Option<String>>,
    /// Điện thoại đang chờ được cho phép ghép nối.
    pub pending: Mutex<Vec<Arc<PairReq>>>,
    /// Âm thanh đã xử lý (i16, 48 kHz) gửi ngược về điện thoại nào bật "nghe lại" (tai nghe).
    pub talk: tokio::sync::broadcast::Sender<Vec<i16>>,
    /// Cho phép điện thoại trong mạng tự tìm thấy PC và xin ghép nối.
    pub discovery: AtomicBool,
    pub last_pair: Mutex<Option<Instant>>,
    next_source: AtomicU64,
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
    /// Các địa chỉ khác của PC (chia sẻ mạng qua USB/Bluetooth, điểm phát sóng): đưa vào QR để điện thoại tự thử lần lượt.
    pub alt_hosts: Mutex<Vec<String>>,
    /// Trạng thái adb (xem adb.rs): 0 chưa có adb, 1 chưa thấy điện thoại, 2 chưa cho phép gỡ lỗi USB, 3 đã nối.
    pub adb_state: std::sync::atomic::AtomicU8,
    /// Tải adb: None = rảnh, Some("") = đang tải, Some(lỗi) = thất bại.
    pub adb_dl: Mutex<Option<String>>,
    /// Đang ở chế độ "Cáp USB": khi thấy điện thoại, tự mở app Android và gửi sẵn mã ghép nối (không cần quét QR).
    pub usb_auto: std::sync::atomic::AtomicBool,
    pub server_err: Mutex<Option<String>>,
    pub out_err: Mutex<Option<String>>,
    /// Tên thiết bị đầu ra đang chọn (để lưu khi thoát từ khay).
    pub out_name: Mutex<String>,
    /// Mã ghép nối hiện hành (đổi được từ giao diện, điện thoại phải gửi đúng mã mới được nối).
    pub token: RwLock<String>,
    /// Phát tiếng bíp khi bật/tắt tiếng bằng phím tắt hoặc menu khay.
    pub beep: AtomicBool,
    // tự đặt ngưỡng cổng ồn: 0 rảnh, 1 đang đo, 2 xong, 3 thất bại
    pub cal_req: AtomicBool,
    pub cal_state: AtomicU8,
    pub cal_progress: AtomicU32,
    pub cal_noise: AtomicU32,
    /// Hồ sơ riêng (nguồn chính; giao diện và điện thoại cùng đọc/ghi).
    pub profiles: RwLock<Vec<Profile>>,
    /// Phổ âm đầu ra (dBFS theo 48 băng). Chỉ được cập nhật khi giao diện đang xem (`spec_until`).
    pub spectrum: Mutex<[f32; BANDS]>,
    pub spec_until: AtomicU64,
    /// Thời gian khứ hồi tới điện thoại (ms, làm mượt), 0 = chưa đo.
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
            sources: Mutex::new(Vec::new()),
            mix_loudest: AtomicBool::new(false),
            room_req: AtomicBool::new(false),
            room_state: AtomicU8::new(0),
            room_progress: AtomicU32::new(0),
            room_result: Mutex::new(None),
            auto_profile: AtomicBool::new(false),
            app_rules: Mutex::new(Vec::new()),
            app_active: Mutex::new(None),
            update: Mutex::new(None),
            update_check: AtomicBool::new(true),
            board: Mutex::new(crate::soundboard::Board::default()),
            board_ver: AtomicU64::new(1),
            sound_hk: Mutex::new(std::collections::HashMap::new()),
            board_msg: Mutex::new(None),
            board_busy: Mutex::new(false),
            playing: Mutex::new(Vec::new()),
            lat_smooth: AtomicU32::new(0),
            buf_smooth: AtomicU32::new(0),
            lat_shown: AtomicU32::new(0),
            buf_shown: AtomicU32::new(0),
            ref_q: Arc::new(Mutex::new(VecDeque::new())),
            ref_on: AtomicBool::new(false),
            pc_q: Arc::new(Mutex::new(VecDeque::new())),
            pc_on: AtomicBool::new(false),
            pc_level: AtomicU32::new(50f32.to_bits()),
            pc_duck: AtomicBool::new(true),
            auto_buf: AtomicBool::new(false),
            voice_hotkeys: AtomicBool::new(false),
            voice_hk: Mutex::new(std::collections::HashMap::new()),
            api_on: AtomicBool::new(false),
            api_port: AtomicU32::new(0),
            tts_voice: Mutex::new(String::new()),
            tts_rate: AtomicI32::new(0),
            tts_busy: AtomicBool::new(false),
            tts_msg: Mutex::new(None),
            test_req: AtomicBool::new(false),
            test_state: AtomicU8::new(0),
            test_progress: AtomicU32::new(0),
            test_msg: Mutex::new(None),
            welcomed: AtomicBool::new(true),
            upd_busy: AtomicBool::new(false),
            upd_msg: Mutex::new(None),
            aec_err: Mutex::new(None),
            pending: Mutex::new(Vec::new()),
            talk: tokio::sync::broadcast::channel(8).0,
            discovery: AtomicBool::new(true),
            last_pair: Mutex::new(None),
            next_source: AtomicU64::new(1),
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
            alt_hosts: Mutex::new(Vec::new()),
            adb_state: std::sync::atomic::AtomicU8::new(0),
            adb_dl: Mutex::new(None),
            usb_auto: std::sync::atomic::AtomicBool::new(false),
            server_err: Mutex::new(None),
            out_err: Mutex::new(None),
            out_name: Mutex::new(String::new()),
        })
    }

    /// Có điện thoại mới nối vào (sau tin "hello"). Quá MAX_SOURCES thì ngắt máy cũ nhất.
    pub fn add_source(&self, name: &str) -> Arc<Source> {
        let id = self.next_source.fetch_add(1, Relaxed);
        let src = Arc::new(Source {
            id,
            name: Mutex::new(name.to_string()),
            muted: AtomicBool::new(false),
            kicked: AtomicBool::new(false),
            peak: AtomicU32::new(0),
            gain: AtomicU32::new(1.0f32.to_bits()),
            q: Mutex::new(SrcQ { q: VecDeque::new(), primed: false }),
        });
        {
            let mut v = self.sources.lock();
            if v.is_empty() {
                self.under.store(0, Relaxed);
                self.drops.store(0, Relaxed);
            }
            while v.len() >= MAX_SOURCES {
                let old = v.remove(0);
                old.kicked.store(true, Relaxed);
            }
            v.push(src.clone());
            self.connected.store(true, Relaxed);
        }
        self.wake_ui();
        src
    }

    pub fn remove_source(&self, id: u64) {
        let mut v = self.sources.lock();
        v.retain(|s| s.id != id);
        self.connected.store(!v.is_empty(), Relaxed);
        if v.is_empty() {
            self.rtt_ms.store(0, Relaxed);
            self.lat_smooth.store(0, Relaxed);
            self.buf_smooth.store(0, Relaxed);
            self.lat_shown.store(0, Relaxed);
            self.buf_shown.store(0, Relaxed);
        }
        drop(v);
        self.wake_ui();
    }

    /// Ngắt mọi điện thoại (đổi mã ghép nối).
    pub fn kick_all(&self) {
        self.conn_gen.fetch_add(1, Relaxed);
        let mut v = self.sources.lock();
        for s in v.iter() {
            s.kicked.store(true, Relaxed);
        }
        v.clear();
        self.connected.store(false, Relaxed);
    }

    /// Server gọi khi một điện thoại gửi âm thanh 48 kHz mono tới.
    pub fn push_input(&self, src: &Source, samples: &[f32]) {
        let target = (self.params.read().buf_ms / 1000.0 * SR) as usize;
        let peak = samples.iter().fold(0f32, |m, x| m.max(x.abs()));
        src.peak.store(peak.to_bits(), Relaxed);
        let mut iq = src.q.lock();
        iq.q.extend(samples.iter().copied());
        let limit = target + (SR * 0.15) as usize;
        if iq.q.len() > limit {
            // mạng dồn gói: bỏ phần cũ để không tích trễ
            let cut = iq.q.len() - target;
            iq.q.drain(..cut);
            self.drops.fetch_add(1, Relaxed);
        }
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

    /// Độ trễ ước tính từ lúc nói đến lúc tiếng ra thiết bị (ms). Gồm: lấy mẫu trên điện thoại,
    /// đường truyền (nửa khứ hồi), đệm chống giật, hàng đợi xuất, xử lý và bộ đệm của thiết bị.
    /// Độ trễ ước tính đã làm mượt (cập nhật ở luồng DSP) và làm tròn 5 ms cho đỡ nhấp nháy.
    pub fn latency_ms(&self) -> u32 {
        let v = f32::from_bits(self.lat_smooth.load(Relaxed));
        if v <= 0.0 { self.latency_raw() } else { stable(&self.lat_shown, v) }
    }

    fn latency_raw(&self) -> u32 {
        let p = *self.params.read();
        let rate = self.out_rate.load(Relaxed).max(1) as f32;
        let out_q = self.out_q.lock().len() as f32 * 1000.0 / rate;
        let rtt = self.rtt_ms.load(Relaxed) as f32;
        let net = if rtt > 0.0 { rtt / 2.0 } else { 5.0 };
        let dsp = 5.0 + if p.ai { 10.0 } else { 0.0 } + if p.fx_pitch.abs() > 0.4 { 21.0 } else { 0.0 };
        let total = 30.0 + net + self.buf_ms.load(Relaxed) as f32 + out_q + dsp + 15.0;
        total.round() as u32
    }

    /// Gộp một phần thông số từ điện thoại (JSON). Không cho điện thoại đổi `monitor` (nghe thử trên PC).
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
            // ghi âm nhiều máy: mỗi máy thêm một file riêng (id máy -> file)
            let mut tracks: std::collections::HashMap<u64, WavWriter> = std::collections::HashMap::new();
            let mut rec_stem = String::new();
            let mut last_loud: u64 = 0;
            let mut aec = crate::aec::Aec::new();
            let mut aec_active = false;
            let mut duck_g = 1.0f32;
            let mut room_meas: Option<(crate::room::RoomMeter, Instant)> = None;
            let mut in_ring = [0f32; FFT_N];
            let mut in_pos = 0usize;
            let mut room_tick = 0u32;
            let mut room_bands = [-120f32; BANDS];
            let mut ref_tmp: Vec<f32> = Vec::with_capacity(4096);
            let mut pc_active = false;
            let mut pc_duck_g = 1.0f32;
            let mut pc_hold = 0u32;
            let mut test: Option<(Instant, Vec<f32>)> = None;
            let mut autobuf = AutoBuf::new();

            loop {
                let p = *sh.params.read();
                let rate = sh.out_rate.load(Relaxed);

                match sh.rec_req.swap(0, Relaxed) {
                    1 if rec.is_none() => match start_recording() {
                        Ok((w, path)) => {
                            rec = Some(w);
                            rec_stem = path.trim_end_matches(".wav").to_string();
                            *sh.rec_path.lock() = path;
                            *sh.rec_err.lock() = None;
                            sh.rec_frames.store(0, Relaxed);
                            sh.rec_on.store(true, Relaxed);
                        }
                        Err(e) => *sh.rec_err.lock() = Some(e),
                    },
                    2 => {
                        for (_, w) in tracks.drain() {
                            let _ = w.finish();
                        }
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
                // đang đo mà quá 7 giây không có âm thanh (điện thoại ngắt kết nối): báo thất bại
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

                if sh.test_req.swap(false, Relaxed) && test.is_none() {
                    test = Some((Instant::now(), Vec::with_capacity(TEST_SAMPLES)));
                    sh.test_state.store(1, Relaxed);
                    store_f(&sh.test_progress, 0.0);
                    *sh.test_msg.lock() = None;
                }
                // thu mà quá 15 giây không có âm thanh (không có điện thoại nối): báo lỗi
                if test.as_ref().is_some_and(|(t0, _)| t0.elapsed() > Duration::from_secs(15)) {
                    test = None;
                    sh.test_state.store(3, Relaxed);
                    *sh.test_msg.lock() = Some(t("test.err.nomic"));
                    sh.wake_ui();
                }

                // Chỉ chạy khi hàng đợi xuất còn dưới ~30 ms
                let room = rate != 0 && sh.out_q.lock().len() < (rate as usize * 30) / 1000;
                let mut got = false;
                if room {
                    let target = ((p.buf_ms / 1000.0 * SR) as usize).max(FRAME);
                    // trộn các điện thoại đang nối: mỗi máy có hàng đợi và bộ đệm riêng
                    let srcs: Vec<Arc<Source>> = sh.sources.lock().clone();
                    let mut parts: Vec<(Arc<Source>, Vec<f32>)> = Vec::new();
                    let mut max_buf = 0usize;
                    for src in &srcs {
                        let mut iq = src.q.lock();
                        if !iq.primed && iq.q.len() >= target {
                            iq.primed = true;
                        }
                        if iq.primed {
                            if iq.q.len() >= FRAME {
                                let v: Vec<f32> = (0..FRAME).map(|_| iq.q.pop_front().unwrap_or(0.0)).collect();
                                parts.push((src.clone(), v));
                            } else {
                                iq.primed = false;
                                sh.under.fetch_add(1, Relaxed);
                            }
                        }
                        max_buf = max_buf.max(iq.q.len());
                    }
                    // đang ghi âm và có từ 2 máy trở lên: mỗi máy thêm một track riêng (tiếng gốc, chưa xử lý)
                    if rec.is_some() && srcs.len() > 1 {
                        for (s, v) in &parts {
                            let w = tracks.entry(s.id).or_insert_with(|| {
                                let name: String = s.name.lock().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                                WavWriter::create(std::path::Path::new(&format!("{rec_stem}-{name}-{}.wav", s.id)), 48_000)
                                    .unwrap_or_else(|_| WavWriter::create(std::path::Path::new(&format!("{rec_stem}-track{}.wav", s.id)), 48_000).expect("track file"))
                            });
                            let _ = w.write(v);
                        }
                    }
                    frame.iter_mut().for_each(|s| *s = 0.0);
                    let active: Vec<&(Arc<Source>, Vec<f32>)> = parts.iter().filter(|(s, _)| !s.muted.load(Relaxed)).collect();
                    let gain_of = |s: &Source| f32::from_bits(s.gain.load(Relaxed));
                    if sh.mix_loudest.load(Relaxed) && active.len() > 1 {
                        // chỉ lấy máy đang nói to nhất; giữ máy cũ nếu nó chưa kém hẳn để khỏi nhảy qua lại
                        let rms = |v: &Vec<f32>, g: f32| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt() * g;
                        let scored: Vec<(u64, f32)> = active.iter().map(|(s, v)| (s.id, rms(v, gain_of(s)))).collect();
                        let best = scored.iter().copied().fold((0u64, -1.0f32), |a, b| if b.1 > a.1 { b } else { a });
                        let keep = scored.iter().find(|(id, r)| *id == last_loud && *r >= best.1 * 0.6).map(|x| x.0);
                        last_loud = keep.unwrap_or(best.0);
                        for (s, v) in active.iter().filter(|(s, _)| s.id == last_loud) {
                            let g = gain_of(s);
                            for (o, x) in frame.iter_mut().zip(v) {
                                *o += x * g;
                            }
                        }
                    } else {
                        for (s, v) in &active {
                            let g = gain_of(s);
                            for (o, x) in frame.iter_mut().zip(v) {
                                *o += x * g;
                            }
                        }
                        if active.len() > 1 {
                            // nhiều người cùng nói: giảm theo căn bậc hai để khỏi vỡ tiếng
                            let g = 1.0 / (active.len() as f32).sqrt();
                            frame.iter_mut().for_each(|s| *s *= g);
                        }
                    }
                    got = !parts.is_empty();
                    let buf_now = max_buf as f32 * 1000.0 / SR;
                    sh.buf_ms.store(buf_now as u32, Relaxed);
                    // làm mượt ~0,5 giây: hiển thị không còn nhảy theo từng gói
                    let prev_buf = f32::from_bits(sh.buf_smooth.load(Relaxed));
                    let buf_sm = if prev_buf <= 0.0 { buf_now } else { prev_buf + (buf_now - prev_buf) * 0.02 };
                    sh.buf_smooth.store(buf_sm.to_bits(), Relaxed);
                    let prev_lat = f32::from_bits(sh.lat_smooth.load(Relaxed));
                    let lat_now = sh.latency_raw() as f32;
                    let lat_sm = if prev_lat <= 0.0 { lat_now } else { prev_lat + (lat_now - prev_lat) * 0.02 };
                    sh.lat_smooth.store(lat_sm.to_bits(), Relaxed);
                }

                // soundboard đang phát mà chưa có điện thoại nào nối: vẫn xử lý một khung im lặng để âm thanh ra mic ảo
                if !got && room && !sh.playing.lock().is_empty() {
                    frame.iter_mut().for_each(|s| *s = 0.0);
                    got = true;
                }
                // âm thanh PC đang phát vào mic mà chưa có điện thoại nào nối
                let pc_run = sh.pc_on.load(Relaxed) && sh.ref_on.load(Relaxed);
                if !got && room && pc_run && sh.pc_q.lock().len() >= FRAME {
                    frame.iter_mut().for_each(|s| *s = 0.0);
                    got = true;
                }

                if !got {
                    // Không có âm thanh: đo mức rơi dần về im lặng, đẩy im lặng ra thiết bị
                    // để mic ảo không bị treo tiếng cuối.
                    idle += 1;
                    if idle > 30 {
                        store_f(&sh.in_db, -99.0);
                        store_f(&sh.out_db, -99.0);
                    }
                    // Có điện thoại nối thì thức dậy dày để bắt kịp gói; chưa có thì ngủ lâu cho đỡ tốn điện
                    let busy = sh.connected.load(Relaxed) || cal.is_some() || pc_run;
                    std::thread::sleep(Duration::from_millis(if busy { 3 } else { 30 }));
                    continue;
                }
                idle = 0;

                // khử vọng: trừ phần micro thu lại âm thanh mà PC đang phát
                if p.aec && sh.ref_on.load(Relaxed) {
                    if !aec_active {
                        aec.reset();
                        sh.ref_q.lock().clear();
                        aec_active = true;
                    }
                    ref_tmp.clear();
                    ref_tmp.extend(sh.ref_q.lock().drain(..));
                    aec.push_ref(&ref_tmp);
                    aec.process(&mut frame);
                } else if aec_active {
                    aec.reset();
                    aec_active = false;
                }

                // tự đo phòng: nghe ~10 giây giọng nói rồi gợi ý ngưỡng cổng ồn và EQ
                if sh.room_req.swap(false, Relaxed) {
                    room_meas = Some((crate::room::RoomMeter::new(), Instant::now()));
                    sh.room_state.store(1, Relaxed);
                    sh.room_progress.store(0f32.to_bits(), Relaxed);
                    *sh.room_result.lock() = None;
                }
                if let Some((meter, started)) = room_meas.as_mut() {
                    for &v in frame.iter() {
                        in_ring[in_pos] = v;
                        in_pos = (in_pos + 1) % FFT_N;
                    }
                    room_tick += 1;
                    if room_tick % 2 == 0 {
                        let mut ordered = [0f32; FFT_N];
                        for (i, o) in ordered.iter_mut().enumerate() {
                            *o = in_ring[(in_pos + i) % FFT_N];
                        }
                        spectrum_bands(&ordered, &mut room_bands);
                    }
                    let rms = (frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
                    meter.feed(20.0 * rms.max(1e-6).log10(), &room_bands);
                    sh.room_progress.store((meter.len() as f32 / crate::room::FRAMES_TARGET as f32).min(1.0).to_bits(), Relaxed);
                    if meter.len() >= crate::room::FRAMES_TARGET {
                        match meter.finish() {
                            Some(r) => {
                                *sh.room_result.lock() = Some(r);
                                sh.room_state.store(2, Relaxed);
                            }
                            None => sh.room_state.store(3, Relaxed),
                        }
                        room_meas = None;
                        sh.wake_ui();
                    } else if started.elapsed() > Duration::from_secs(40) {
                        sh.room_state.store(3, Relaxed);
                        room_meas = None;
                        sh.wake_ui();
                    }
                }

                let info = chain.process(&frame, &p, &mut out);
                let mut sound_peak = 0f32;

                // bộ đệm tự động: tăng khi bị giật, giảm dần khi ổn định
                if sh.auto_buf.load(Relaxed) && sh.connected.load(Relaxed) {
                    if let Some(v) = autobuf.step(now_ms() as f64 / 1000.0, sh.under.load(Relaxed), p.buf_ms) {
                        let mut np = *sh.params.read();
                        np.buf_ms = v;
                        sh.set_params(np);
                        sh.wake_ui();
                    }
                } else {
                    autobuf.reset();
                }

                // âm thanh PC (nhạc, video) trộn vào mic, hạ nhỏ khi bạn đang nói
                if pc_run {
                    if !pc_active {
                        sh.pc_q.lock().clear();
                        pc_active = true;
                    }
                    let mut pc = [0f32; FRAME];
                    {
                        let mut q = sh.pc_q.lock();
                        while q.len() > FRAME * 12 {
                            q.pop_front(); // giữ độ trễ dưới ~120 ms
                        }
                        for v in pc.iter_mut() {
                            match q.pop_front() {
                                Some(x) => *v = x,
                                None => break,
                            }
                        }
                    }
                    let voice = info.gate_open && info.out_peak > 0.03;
                    if voice {
                        pc_hold = 40; // giữ nhỏ thêm ~0,4 giây sau khi bạn ngừng nói
                    } else if pc_hold > 0 {
                        pc_hold -= 1;
                    }
                    let duck_t = if sh.pc_duck.load(Relaxed) && pc_hold > 0 { 0.2 } else { 1.0 };
                    pc_duck_g += (duck_t - pc_duck_g) * if duck_t < pc_duck_g { 0.25 } else { 0.03 };
                    let g = load_f(&sh.pc_level) / 100.0 * pc_duck_g;
                    for (o, m) in out.iter_mut().zip(&pc) {
                        *o = (*o + *m * g).clamp(-0.98, 0.98);
                        sound_peak = sound_peak.max(o.abs());
                    }
                } else if pc_active {
                    pc_active = false;
                    pc_duck_g = 1.0;
                    sh.pc_q.lock().clear();
                }

                // soundboard: trộn các âm đang phát vào đầu ra (sau bộ lọc, để lọc ồn không nuốt hiệu ứng âm thanh)
                {
                    let mut pl = sh.playing.lock();
                    let active = !pl.is_empty();
                    let (volume, duck) = {
                        let b = sh.board.lock();
                        (b.volume, b.duck)
                    };
                    let duck_target = if active && duck { 0.4 } else { 1.0 };
                    duck_g += (duck_target - duck_g) * 0.05;
                    if (duck_g - 1.0).abs() > 0.002 {
                        out.iter_mut().for_each(|o| *o *= duck_g);
                    }
                    if active {
                        let mut mix = [0f32; FRAME];
                        pl.retain_mut(|p| p.mix_into(&mut mix));
                        for (o, m) in out.iter_mut().zip(&mix) {
                            *o = (*o + *m * volume).clamp(-0.98, 0.98);
                            sound_peak = sound_peak.max(o.abs());
                        }
                        if pl.is_empty() {
                            drop(pl);
                            sh.wake_ui();
                        }
                    }
                }
                // thử mic: giữ lại 5 giây đầu ra rồi phát lại qua loa
                if let Some((_, buf)) = test.as_mut() {
                    buf.extend_from_slice(&out);
                    store_f(&sh.test_progress, (buf.len() as f32 / TEST_SAMPLES as f32).min(1.0));
                    if buf.len() >= TEST_SAMPLES {
                        if let Some((_, b)) = test.take() {
                            sh.test_state.store(2, Relaxed);
                            play_local(sh.clone(), b);
                        }
                        sh.wake_ui();
                    }
                }
                store_f(&sh.in_db, lin2db(info.in_peak));
                store_f(&sh.out_db, lin2db(info.out_peak.max(sound_peak)));
                store_f(&sh.vad, info.vad);
                sh.gate_open.store(info.gate_open, Relaxed);

                if sh.talk.receiver_count() > 0 {
                    let v: Vec<i16> = out.iter().map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
                    let _ = sh.talk.send(v);
                }

                if let Some(w) = rec.as_mut() {
                    if let Err(e) = w.write(&out) {
                        *sh.rec_err.lock() = Some(tf("rec.err.write", &[&e.to_string()]));
                        rec = None;
                        for (_, w) in tracks.drain() {
                            let _ = w.finish();
                        }
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

/// Thử mic: 5 giây.
const TEST_SAMPLES: usize = 48_000 * 5;

/// Phát một đoạn 48 kHz mono ra loa/tai nghe mặc định (không qua mic ảo) rồi đóng luồng.
fn play_local(sh: Arc<Shared>, samples: Vec<f32>) {
    let _ = std::thread::Builder::new().name("test-play".into()).spawn(move || {
        let fail = |sh: &Shared, msg: String| {
            *sh.test_msg.lock() = Some(msg);
            sh.test_state.store(3, Relaxed);
            sh.wake_ui();
        };
        let host = cpal::default_host();
        let Some(dev) = host.default_output_device() else {
            return fail(&sh, t("test.err.nodev"));
        };
        if dev.name().map(|n| is_virtual(&n)).unwrap_or(false) {
            return fail(&sh, t("test.err.virtual"));
        }
        let q: Queue = Arc::new(Mutex::new(VecDeque::new()));
        let (stream, rate) = match open(&dev, q.clone()) {
            Ok(x) => x,
            Err(e) => return fail(&sh, e),
        };
        let mut rs = Resampler::new(48_000, rate);
        let mut conv = Vec::with_capacity(samples.len() * rate as usize / 48_000 + 64);
        rs.process(&samples, &mut conv);
        let secs = conv.len() as f32 / rate as f32;
        q.lock().extend(conv);
        std::thread::sleep(Duration::from_secs_f32(secs + 0.5));
        drop(stream);
        sh.test_state.store(0, Relaxed);
        sh.wake_ui();
    });
}

/// Tự chỉnh bộ đệm chống giật: bị giật thì tăng ngay, ổn định lâu thì hạ từ từ để bớt trễ.
pub struct AutoBuf {
    last_under: u32,
    last_change: f64,
    last_check: f64,
}

impl AutoBuf {
    pub const MIN: f32 = 40.0;
    pub const MAX: f32 = 300.0;

    pub fn new() -> Self {
        Self { last_under: 0, last_change: 0.0, last_check: 0.0 }
    }

    pub fn reset(&mut self) {
        self.last_check = 0.0;
    }

    /// `now` tính bằng giây, `under` là số lần hết dữ liệu từ lúc điện thoại nối. Trả về bộ đệm mới nếu cần đổi.
    pub fn step(&mut self, now: f64, under: u32, cur: f32) -> Option<f32> {
        if self.last_check == 0.0 {
            self.last_check = now;
            self.last_change = now;
            self.last_under = under;
            return None;
        }
        if now - self.last_check < 1.0 {
            return None;
        }
        self.last_check = now;
        if under < self.last_under {
            self.last_under = under; // bộ đếm được đặt lại khi có điện thoại mới nối
        }
        if under > self.last_under {
            self.last_under = under;
            self.last_change = now;
            let v = (cur + 20.0).min(Self::MAX);
            return (v > cur).then_some(v);
        }
        if now - self.last_change >= 30.0 {
            self.last_change = now;
            let v = (cur - 5.0).max(Self::MIN);
            return (v < cur).then_some(v);
        }
        None
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
/// Opens the default playback device in loopback mode (what the PC plays) and feeds `q` with 48 kHz mono samples.
fn open_loopback(dev: &cpal::Device, qs: Vec<Queue>) -> Result<cpal::Stream, String> {
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    let rate = cfg.sample_rate().0;
    let ch = cfg.channels() as usize;
    let fmt = cfg.sample_format();
    let sc: cpal::StreamConfig = cfg.into();
    let mut rs = Resampler::new(rate, 48_000);
    let mut mono: Vec<f32> = Vec::with_capacity(2048);
    let mut conv: Vec<f32> = Vec::with_capacity(2048);
    let err = |e| eprintln!("loopback error: {e}");
    macro_rules! build {
        ($t:ty) => {
            dev.build_input_stream(
                &sc,
                move |data: &[$t], _| {
                    mono.clear();
                    mono.extend(data.chunks(ch).map(|f| f.iter().map(|s| <f32 as FromSample<$t>>::from_sample_(*s)).sum::<f32>() / ch as f32));
                    conv.clear();
                    if rate == 48_000 {
                        conv.extend_from_slice(&mono);
                    } else {
                        rs.process(&mono, &mut conv);
                    }
                    for q in &qs {
                        let mut g = q.lock();
                        g.extend(conv.iter().copied());
                        while g.len() > 96_000 {
                            g.pop_front(); // keep at most 2 s
                        }
                    }
                },
                err,
                None,
            )
        };
    }
    let stream = match fmt {
        SampleFormat::F32 => build!(f32),
        SampleFormat::I16 => build!(i16),
        SampleFormat::I32 => build!(i32),
        f => return Err(tf("audio.err.format", &[&format!("{f:?}")])),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Thread that keeps the loopback reference stream running while echo cancellation is on, following the default playback device.
pub fn spawn_aec_reference(sh: Arc<Shared>) {
    let _ = std::thread::Builder::new().name("aec-ref".into()).spawn(move || {
        let host = cpal::default_host();
        let mut stream: Option<cpal::Stream> = None;
        let mut cur = String::new();
        loop {
            let want = sh.params.read().aec || sh.pc_on.load(Relaxed);
            let name = if want { host.default_output_device().and_then(|d| d.name().ok()).unwrap_or_default() } else { String::new() };
            let usable = want && !name.is_empty() && !is_virtual(&name);
            if stream.is_some() && (!usable || name != cur) {
                stream = None;
                cur.clear();
                sh.ref_on.store(false, Relaxed);
                sh.ref_q.lock().clear();
            }
            if want && !name.is_empty() && is_virtual(&name) {
                *sh.aec_err.lock() = Some("virtual".to_string());
            } else if usable && stream.is_none() {
                match host.default_output_device().ok_or_else(|| "no device".to_string()).and_then(|d| open_loopback(&d, vec![sh.ref_q.clone(), sh.pc_q.clone()])) {
                    Ok(s) => {
                        if std::env::var_os("LARMA_DEBUG").is_some() {
                            crate::win::log_line(&format!("echo cancellation reference opened on: {name}"));
                        }
                        stream = Some(s);
                        cur = name;
                        sh.ref_on.store(true, Relaxed);
                        *sh.aec_err.lock() = None;
                    }
                    Err(e) => {
                        if std::env::var_os("LARMA_DEBUG").is_some() {
                            crate::win::log_line(&format!("echo cancellation reference failed: {e}"));
                        }
                        *sh.aec_err.lock() = Some(e);
                    }
                }
            } else if !want {
                *sh.aec_err.lock() = None;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

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
mod autobuf_tests {
    use super::*;

    #[test]
    fn grows_on_glitches_and_shrinks_when_calm() {
        let mut a = AutoBuf::new();
        let mut buf = 60.0f32;
        let mut under = 0u32;
        assert_eq!(a.step(10.0, under, buf), None); // first call only starts the clock
        under += 1;
        buf = a.step(12.0, under, buf).expect("glitch grows the buffer");
        assert_eq!(buf, 80.0);
        // calm for 31 s: one small step down, not before
        assert_eq!(a.step(30.0, under, buf), None);
        buf = a.step(43.0, under, buf).expect("calm shrinks it");
        assert_eq!(buf, 75.0);
        // never beyond the limits
        for i in 0..40 {
            under += 1;
            if let Some(v) = a.step(100.0 + i as f64 * 2.0, under, buf) {
                buf = v;
            }
        }
        assert_eq!(buf, AutoBuf::MAX);
        let mut low = 41.0f32;
        let mut t = 1000.0;
        for _ in 0..10 {
            t += 31.0;
            if let Some(v) = a.step(t, under, low) {
                low = v;
            }
        }
        assert_eq!(low, AutoBuf::MIN);
    }
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
