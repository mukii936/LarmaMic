//! Thông số xử lý âm thanh + lưu/đọc cấu hình trong %APPDATA%\LarmaMic.
//!
//! Lưu an toàn: ghi vào file tạm rồi đổi tên đè lên file chính, và giữ một bản sao lưu `.bak`.
//! Nếu file chính bị rỗng/hỏng thì đọc bản sao lưu thay vì lặng lẽ quay về mặc định.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Params {
    pub ai: bool,       // khử ồn AI (RNNoise)
    pub ai_mix: f32,    // 0..100 %
    pub gate_on: bool,  // cổng ồn
    pub thr: f32,       // ngưỡng cổng, dB
    pub depth: f32,     // độ giảm khi cổng đóng, dB (dương)
    pub hp: f32,        // cắt tiếng ù, Hz
    pub low: f32,       // trầm 120 Hz, dB
    pub mud: f32,       // độ đục 280 Hz, dB
    pub pres: f32,      // rõ giọng 3.2 kHz, dB
    pub air: f32,       // độ sáng 9 kHz, dB
    pub comp: f32,      // nén giọng 0..100 %
    pub gain: f32,      // âm lượng, dB
    pub buf_ms: f32,    // đệm chống giật, ms
    pub mute: bool,
    pub monitor: bool,  // nghe thử trên loa/tai nghe mặc định
    pub agc: bool,      // tự cân âm lượng
    pub aec: bool,      // khử vọng (echo): trừ đi âm thanh PC đang phát
    pub fx_pitch: f32,  // đổi tông, bán cung -12..12
    pub fx_reverb: f32, // vang 0..100 %
    pub fx_robot: bool, // giọng robot
    pub fx_echo: f32,   // vọng lặp (echo/delay) 0..100 %
    pub fx_chorus: f32, // nhân đôi giọng (chorus) 0..100 %
    pub fx_dist: f32,   // méo tiếng 0..100 %
    pub fx_band: u8,    // bộ lọc dải: 0 không, 1 điện thoại, 2 loa phóng thanh
    pub deess: f32,     // giảm âm "s" chói 0..100 %
    pub plosive: bool,  // lọc tiếng "bụp" khi nói chữ p, b
}

impl Default for Params {
    fn default() -> Self {
        preset("voice")
    }
}

impl Params {
    /// Ép mọi giá trị về khoảng hợp lệ (điện thoại gửi gì lên cũng không làm hỏng bộ lọc).
    pub fn sanitized(mut self) -> Self {
        fn c(v: f32, lo: f32, hi: f32) -> f32 {
            if v.is_finite() { v.clamp(lo, hi) } else { lo }
        }
        self.ai_mix = c(self.ai_mix, 0.0, 100.0);
        self.thr = c(self.thr, -72.0, -20.0);
        self.depth = c(self.depth, 6.0, 80.0);
        self.hp = c(self.hp, 40.0, 250.0);
        self.low = c(self.low, -12.0, 12.0);
        self.mud = c(self.mud, -12.0, 12.0);
        self.pres = c(self.pres, -12.0, 12.0);
        self.air = c(self.air, -12.0, 12.0);
        self.comp = c(self.comp, 0.0, 100.0);
        self.gain = c(self.gain, -12.0, 24.0);
        self.buf_ms = c(self.buf_ms, 30.0, 300.0);
        self.fx_pitch = c(self.fx_pitch, -12.0, 12.0).round();
        self.fx_reverb = c(self.fx_reverb, 0.0, 100.0);
        self.fx_echo = c(self.fx_echo, 0.0, 100.0);
        self.fx_chorus = c(self.fx_chorus, 0.0, 100.0);
        self.fx_dist = c(self.fx_dist, 0.0, 100.0);
        self.fx_band = self.fx_band.min(2);
        self.deess = c(self.deess, 0.0, 100.0);
        self
    }
}

/// (khóa cấu hình, khóa dịch của tên hiển thị)
pub const PRESET_NAMES: [(&str, &str); 4] = [
    ("voice", "preset.voice"),
    ("game", "preset.game"),
    ("podcast", "preset.podcast"),
    ("raw", "preset.raw"),
];

pub fn preset(name: &str) -> Params {
    let base = Params {
        ai: true, ai_mix: 85.0, gate_on: true, thr: -52.0, depth: 40.0, hp: 100.0,
        low: -1.0, mud: -3.0, pres: 3.0, air: 1.0, comp: 40.0, gain: 6.0, buf_ms: 60.0,
        mute: false, monitor: false, agc: false, aec: false, fx_pitch: 0.0, fx_reverb: 0.0, fx_robot: false, fx_echo: 0.0, fx_chorus: 0.0, fx_dist: 0.0, fx_band: 0, deess: 0.0, plosive: false,
    };
    match name {
        "game" => Params {
            ai_mix: 100.0, thr: -44.0, depth: 55.0, hp: 120.0, low: -3.0, mud: -3.0,
            pres: 4.0, air: 0.0, comp: 55.0, gain: 8.0, buf_ms: 50.0, ..base
        },
        "podcast" => Params {
            ai_mix: 70.0, thr: -56.0, depth: 30.0, hp: 80.0, low: 2.0, mud: -2.0,
            pres: 2.0, air: 2.0, comp: 45.0, gain: 5.0, buf_ms: 90.0, ..base
        },
        "raw" => Params {
            ai: false, gate_on: false, hp: 40.0, low: 0.0, mud: 0.0, pres: 0.0,
            air: 0.0, comp: 0.0, gain: 0.0, ..base
        },
        _ => base,
    }
}

/// Áp preset nhưng giữ nguyên tắt tiếng, nghe thử, tự cân âm lượng và hiệu ứng giọng
/// (những thứ này độc lập với cấu hình nhanh).
pub fn apply_preset(cur: &Params, name: &str) -> Params {
    Params {
        mute: cur.mute,
        monitor: cur.monitor,
        agc: cur.agc,
        aec: cur.aec,
        fx_pitch: cur.fx_pitch,
        fx_reverb: cur.fx_reverb,
        fx_robot: cur.fx_robot,
        fx_echo: cur.fx_echo,
        fx_chorus: cur.fx_chorus,
        fx_dist: cur.fx_dist,
        fx_band: cur.fx_band,
        deess: cur.deess,
        plosive: cur.plosive,
        ..preset(name)
    }
}

pub fn matches_preset(p: &Params, name: &str) -> bool {
    apply_preset(p, name) == *p
}

/// Giọng đổi sẵn: ghép từ các hiệu ứng giọng (đổi tông, vang, robot, vọng, nhân đôi, méo, lọc dải).
pub struct Voice {
    pub key: &'static str,
    pub pitch: f32,
    pub reverb: f32,
    pub robot: bool,
    pub echo: f32,
    pub chorus: f32,
    pub dist: f32,
    pub band: u8,
}
const fn voice(key: &'static str, pitch: f32, reverb: f32, robot: bool, echo: f32, chorus: f32, dist: f32, band: u8) -> Voice {
    Voice { key, pitch, reverb, robot, echo, chorus, dist, band }
}
pub const VOICES: [Voice; 10] = [
    voice("fx.none", 0.0, 0.0, false, 0.0, 0.0, 0.0, 0),
    voice("fx.deep", -5.0, 0.0, false, 0.0, 0.0, 0.0, 0),
    voice("fx.small", 7.0, 0.0, false, 0.0, 0.0, 0.0, 0),
    voice("fx.robot", 0.0, 0.0, true, 0.0, 0.0, 0.0, 0),
    voice("fx.ghost", -2.0, 65.0, false, 35.0, 0.0, 0.0, 0),
    voice("fx.alien", 4.0, 20.0, true, 0.0, 40.0, 0.0, 0),
    voice("fx.monster", -9.0, 25.0, false, 0.0, 0.0, 35.0, 0),
    voice("fx.cave", 0.0, 85.0, false, 45.0, 0.0, 0.0, 0),
    voice("fx.radio", 0.0, 0.0, false, 0.0, 0.0, 15.0, 1),
    voice("fx.megaphone", 0.0, 0.0, false, 0.0, 0.0, 40.0, 2),
];

impl Voice {
    pub fn is_on(&self, p: &Params) -> bool {
        p.fx_pitch == self.pitch
            && p.fx_reverb == self.reverb
            && p.fx_robot == self.robot
            && p.fx_echo == self.echo
            && p.fx_chorus == self.chorus
            && p.fx_dist == self.dist
            && p.fx_band == self.band
    }
}

/// Áp giọng số `i` (0 = giọng thường) lên thông số hiện tại.
pub fn apply_voice(cur: &Params, i: usize) -> Params {
    let Some(v) = VOICES.get(i) else { return *cur };
    Params { fx_pitch: v.pitch, fx_reverb: v.reverb, fx_robot: v.robot, fx_echo: v.echo, fx_chorus: v.chorus, fx_dist: v.dist, fx_band: v.band, ..*cur }
}

/// Hồ sơ riêng do người dùng đặt tên.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Profile {
    pub name: String,
    pub params: Params,
}

/// Áp hồ sơ nhưng giữ nguyên tắt tiếng và nghe thử.
pub fn apply_profile(cur: &Params, prof: &Profile) -> Params {
    // khử vọng phụ thuộc loa/phòng của máy này chứ không phải cấu hình giọng nên giữ nguyên
    Params { mute: cur.mute, monitor: cur.monitor, aec: cur.aec, ..prof.params }.sanitized()
}

pub fn matches_profile(p: &Params, prof: &Profile) -> bool {
    apply_profile(p, prof) == *p
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Saved {
    pub params: Params,
    /// Rỗng = tự tìm VB-Cable.
    pub out_device: String,
    /// Mã ghép nối nằm trong QR; điện thoại không có mã đúng thì không gửi được tiếng vào PC.
    pub token: String,
    /// Phím tắt tắt/bật tiếng, ví dụ "ctrl+alt+KeyM". Rỗng = mặc định; "off" = không dùng.
    pub hotkey: String,
    /// true = không phát tiếng bíp khi bật/tắt tiếng bằng phím tắt.
    pub no_beep: bool,
    /// Hồ sơ riêng.
    pub profiles: Vec<Profile>,
    /// Ngôn ngữ giao diện: "vi", "en"... Rỗng = tự theo ngôn ngữ của Windows.
    pub lang: String,
    /// Cách điện thoại nối tới PC: "wifi" (mặc định), "usb" hoặc "bt". Quyết định địa chỉ nào đứng đầu trong QR.
    pub conn_mode: String,
    /// Nhiều điện thoại: true = chỉ lấy máy đang nói to nhất (mặc định trộn tất cả).
    pub mix_loudest: bool,
    /// true = không cho điện thoại tự tìm thấy PC (không quảng bá mDNS, từ chối xin ghép nối).
    pub no_discovery: bool,
    /// Tab đang mở lần trước: "connect", "sound", "board" hoặc "settings".
    pub tab: String,
    /// Cấu hình theo ứng dụng: tự đổi preset/hồ sơ khi chương trình trong danh sách đang chạy.
    pub auto_profile: bool,
    pub app_rules: Vec<crate::appmatch::AppRule>,
    /// true = không kiểm tra bản mới trên GitHub.
    pub no_update_check: bool,
    /// Đưa âm thanh PC đang phát (nhạc, video) vào mic, với mức 0..100 (0 = dùng 50) và có hạ nhỏ khi bạn nói hay không.
    pub pc_audio: bool,
    pub pc_level: f32,
    pub pc_no_duck: bool,
    /// Tự chỉnh bộ đệm chống giật theo đường truyền.
    pub auto_buf: bool,
    /// Đã xem hướng dẫn lần đầu.
    pub welcomed: bool,
    /// Ctrl+Alt+F1..F10 đổi giọng.
    pub voice_hotkeys: bool,
    /// Bật API điều khiển cục bộ (http://127.0.0.1) cho Stream Deck, OBS...
    pub api_on: bool,
    /// Giọng đọc của Windows dùng cho chữ thành tiếng ("" = mặc định) và tốc độ -5..5.
    pub tts_voice: String,
    pub tts_rate: i32,
    /// Khóa dịch của ghi chú về lần đọc cài đặt này (không lưu ra file), ví dụ "note.backup".
    #[serde(skip)]
    pub note: String,
}

pub const DEFAULT_HOTKEY: &str = "ctrl+alt+KeyM";

/// Mã ghép nối ngẫu nhiên 16 byte, viết dạng hex (32 ký tự).
pub fn new_token() -> String {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        // hệ thống không cấp được số ngẫu nhiên: dùng đồng hồ làm phương án cuối
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        b.copy_from_slice(&t.to_le_bytes());
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("LarmaMic")
}

fn file() -> PathBuf {
    dir().join("settings.json")
}
fn backup_file() -> PathBuf {
    dir().join("settings.json.bak")
}

/// Đọc và phân tích một file cài đặt; chỉ coi là hợp lệ nếu không rỗng và đọc được JSON.
fn read_valid(path: &PathBuf) -> Option<Saved> {
    let t = std::fs::read_to_string(path).ok()?;
    if t.trim().is_empty() {
        return None;
    }
    serde_json::from_str::<Saved>(&t).ok()
}

pub fn load() -> Saved {
    let main = file();
    let mut s = if let Some(s) = read_valid(&main) {
        s
    } else if let Some(mut b) = read_valid(&backup_file()) {
        // File chính thiếu/rỗng/hỏng: dùng bản sao lưu, giữ lại file hỏng để tra cứu
        if main.exists() {
            let _ = std::fs::rename(&main, dir().join("settings.json.corrupt"));
        }
        b.note = "note.backup".to_string(); // khóa dịch, giao diện sẽ dịch khi hiển thị
        b
    } else {
        let mut d = Saved::default();
        if main.exists() {
            let _ = std::fs::rename(&main, dir().join("settings.json.corrupt"));
            d.note = "note.defaults".to_string();
        }
        d
    };
    s.params = s.params.sanitized();
    s.params.mute = false;
    if s.token.len() < 16 {
        s.token = new_token();
        save(&s);
    }
    s
}

/// Ghi an toàn: file tạm -> sao lưu file cũ -> đổi tên đè lên file chính.
pub fn save(s: &Saved) {
    let _ = std::fs::create_dir_all(dir());
    let Ok(text) = serde_json::to_string_pretty(s) else { return };
    let (main, tmp) = (file(), dir().join("settings.json.tmp"));
    let wrote = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        Ok(())
    })();
    if wrote.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    // chỉ sao lưu khi file hiện tại còn dùng được, để không ghi đè bản tốt bằng bản hỏng
    if read_valid(&main).is_some() {
        let _ = std::fs::copy(&main, backup_file());
    }
    if std::fs::rename(&tmp, &main).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    /// Chạy `f` với APPDATA trỏ vào thư mục tạm, để không đụng cài đặt thật.
    fn with_tmp<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("larma-settings-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("APPDATA", &tmp);
        let r = f(&tmp);
        let _ = std::fs::remove_dir_all(&tmp);
        r
    }

    fn sample(gain: f32) -> Saved {
        Saved { params: Params { gain, ..Params::default() }, token: new_token(), ..Saved::default() }
    }

    #[test]
    fn save_roundtrips_and_keeps_backup_of_previous() {
        with_tmp(|_| {
            let mut s = sample(9.0);
            save(&s);
            s.params.gain = 10.0;
            save(&s);
            assert!(backup_file().exists(), "phải có bản sao lưu sau lần lưu thứ hai");
            assert!(!dir().join("settings.json.tmp").exists(), "không để lại file tạm");
            let l = load();
            assert_eq!(l.params.gain, 10.0);
            assert_eq!(l.token, s.token);
            assert!(l.note.is_empty());
        });
    }

    #[test]
    fn empty_main_file_is_restored_from_backup() {
        with_tmp(|_| {
            let mut s = sample(9.0);
            save(&s);
            s.params.gain = 10.0;
            save(&s);
            let old_token = s.token.clone();
            std::fs::write(file(), "").unwrap(); // đúng sự cố đã gặp: file bị ghi đè trống
            let l = load();
            assert_eq!(l.token, old_token, "mã ghép nối phải còn nguyên");
            assert_eq!(l.params.gain, 9.0, "dùng giá trị của bản sao lưu");
            assert!(!l.note.is_empty(), "phải báo cho người dùng biết");
            assert!(dir().join("settings.json.corrupt").exists(), "giữ lại file hỏng để tra cứu");
        });
    }

    #[test]
    fn corrupt_without_backup_falls_back_to_defaults_and_says_so() {
        with_tmp(|_| {
            std::fs::create_dir_all(dir()).unwrap();
            std::fs::write(file(), "{ không phải json").unwrap();
            let l = load();
            assert!(l.token.len() >= 16, "phải tạo mã mới");
            assert_eq!(l.note, "note.defaults", "ghi chú: {}", l.note);
        });
    }

    #[test]
    fn bad_save_never_replaces_good_backup_with_empty() {
        with_tmp(|_| {
            let s = sample(7.0);
            save(&s);
            std::fs::write(file(), "").unwrap(); // main hỏng
            save(&s); // lưu lại khi main đang hỏng: không được lấy main hỏng làm bản sao lưu
            let b = std::fs::read_to_string(backup_file()).unwrap_or_default();
            assert!(b.is_empty() || serde_json::from_str::<Saved>(&b).is_ok(), "bản sao lưu bị hỏng: {b:?}");
        });
    }

    #[test]
    fn profiles_roundtrip_and_apply() {
        with_tmp(|_| {
            let mut s = sample(5.0);
            let prof = Profile { name: "Phòng ồn".into(), params: Params { thr: -40.0, agc: true, fx_pitch: 3.0, ..Params::default() } };
            s.profiles.push(prof.clone());
            save(&s);
            let l = load();
            assert_eq!(l.profiles.len(), 1);
            assert_eq!(l.profiles[0].name, "Phòng ồn");
            let cur = Params { mute: true, monitor: true, ..Params::default() };
            let applied = apply_profile(&cur, &l.profiles[0]);
            assert_eq!(applied.thr, -40.0);
            assert!(applied.agc);
            assert!(applied.mute && applied.monitor, "giữ nguyên tắt tiếng và nghe thử");
            assert!(matches_profile(&applied, &l.profiles[0]));
        });
    }

    #[test]
    fn presets_do_not_touch_agc_or_effects() {
        let cur = Params { agc: true, fx_pitch: 5.0, fx_reverb: 30.0, fx_robot: true, ..Params::default() };
        let p = apply_preset(&cur, "game");
        assert!(p.agc && p.fx_robot && p.fx_pitch == 5.0 && p.fx_reverb == 30.0);
        assert!(matches_preset(&p, "game"));
    }

    #[test]
    fn voices_apply_and_are_recognised() {
        let base = Params::default();
        for (i, v) in VOICES.iter().enumerate() {
            let p = apply_voice(&base, i);
            assert!(v.is_on(&p), "voice {i}");
            assert_eq!(p.thr, base.thr, "only the effects change");
        }
        assert_eq!(apply_voice(&base, 99), base);
    }

    #[test]
    fn sanitize_clamps_new_fields() {
        let p = Params { fx_pitch: 99.0, fx_reverb: -5.0, ..Params::default() }.sanitized();
        assert_eq!(p.fx_pitch, 12.0);
        assert_eq!(p.fx_reverb, 0.0);
        let p = Params { fx_pitch: f32::NAN, ..Params::default() }.sanitized();
        assert!(p.fx_pitch.is_finite());
    }
}
