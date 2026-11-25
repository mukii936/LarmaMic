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
    pub fx_pitch: f32,  // đổi tông, bán cung -12..12
    pub fx_reverb: f32, // vang 0..100 %
    pub fx_robot: bool, // giọng robot
}

impl Default for Params {
    fn default() -> Self {
        preset("voice")
    }
}

impl Params {
    /// Ép mọi giá trị về khoảng hợp lệ (iPhone gửi gì lên cũng không làm hỏng bộ lọc).
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
        mute: false, monitor: false, agc: false, fx_pitch: 0.0, fx_reverb: 0.0, fx_robot: false,
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
        fx_pitch: cur.fx_pitch,
        fx_reverb: cur.fx_reverb,
        fx_robot: cur.fx_robot,
        ..preset(name)
    }
}

pub fn matches_preset(p: &Params, name: &str) -> bool {
    apply_preset(p, name) == *p
}

/// Hồ sơ riêng do người dùng đặt tên.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Profile {
    pub name: String,
    pub params: Params,
}

/// Áp hồ sơ nhưng giữ nguyên tắt tiếng và nghe thử.
pub fn apply_profile(cur: &Params, prof: &Profile) -> Params {
    Params { mute: cur.mute, monitor: cur.monitor, ..prof.params }.sanitized()
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
    /// Mã ghép nối nằm trong QR; iPhone không có mã đúng thì không gửi được tiếng vào PC.
    pub token: String,
    /// Phím tắt tắt/bật tiếng, ví dụ "ctrl+alt+KeyM". Rỗng = mặc định; "off" = không dùng.
    pub hotkey: String,
    /// true = không phát tiếng bíp khi bật/tắt tiếng bằng phím tắt.
    pub no_beep: bool,
    /// Hồ sơ riêng.
    pub profiles: Vec<Profile>,
    /// Ngôn ngữ giao diện: "vi", "en"... Rỗng = tự theo ngôn ngữ của Windows.
    pub lang: String,
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
    fn sanitize_clamps_new_fields() {
        let p = Params { fx_pitch: 99.0, fx_reverb: -5.0, ..Params::default() }.sanitized();
        assert_eq!(p.fx_pitch, 12.0);
        assert_eq!(p.fx_reverb, 0.0);
        let p = Params { fx_pitch: f32::NAN, ..Params::default() }.sanitized();
        assert!(p.fx_pitch.is_finite());
    }
}
