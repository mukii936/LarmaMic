//! Đa ngôn ngữ cho app PC. Mọi chữ người dùng nhìn thấy đều đi qua `t("khóa")` / `tf("khóa", &[..])`.
//!
//! Thêm một ngôn ngữ: (1) thêm biến thể vào `Lang`, (2) thêm một bảng `static XX: &[(&str, &str)]` có đủ mọi
//! khóa như `VI`, (3) nối vào `table()`. Bài kiểm tra tự động sẽ báo khóa thiếu hoặc thừa, và báo chuỗi tiếng
//! Việt còn viết cứng trong mã nguồn.
//!
//! Chỗ giữ chỗ: `{0}`, `{1}`… được thay bằng tham số của `tf`.

use std::sync::atomic::{AtomicU8, Ordering::Relaxed};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Vi,
    En,
    Zh,
    Ja,
    Ko,
    Id,
    Es,
    Fr,
    De,
    Ru,
}

impl Lang {
    /// Các ngôn ngữ có trong app, theo thứ tự hiện trong ô chọn.
    pub const ALL: [Lang; 10] =
        [Lang::Vi, Lang::En, Lang::Zh, Lang::Ja, Lang::Ko, Lang::Id, Lang::Es, Lang::Fr, Lang::De, Lang::Ru];

    pub fn code(self) -> &'static str {
        match self {
            Lang::Vi => "vi",
            Lang::En => "en",
            Lang::Zh => "zh",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
            Lang::Id => "id",
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::De => "de",
            Lang::Ru => "ru",
        }
    }

    pub fn from_code(s: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == s)
    }

    /// Tên ngôn ngữ viết bằng chính ngôn ngữ đó (để người dùng nhận ra dù đang ở ngôn ngữ nào).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Vi => "Tiếng Việt",
            Lang::En => "English",
            Lang::Zh => "中文 (简体)",
            Lang::Ja => "日本語",
            Lang::Ko => "한국어",
            Lang::Id => "Bahasa Indonesia",
            Lang::Es => "Español",
            Lang::Fr => "Français",
            Lang::De => "Deutsch",
            Lang::Ru => "Русский",
        }
    }

    fn index(self) -> u8 {
        self as u8
    }

    fn from_index(i: u8) -> Lang {
        const V: [Lang; 10] = [
            Lang::Vi,
            Lang::En,
            Lang::Zh,
            Lang::Ja,
            Lang::Ko,
            Lang::Id,
            Lang::Es,
            Lang::Fr,
            Lang::De,
            Lang::Ru,
        ];
        V.get(i as usize).copied().unwrap_or(Lang::Vi)
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn current() -> Lang {
    Lang::from_index(CURRENT.load(Relaxed))
}

pub fn set(l: Lang) {
    CURRENT.store(l.index(), Relaxed);
}

#[link(name = "kernel32")]
extern "system" {
    fn GetUserDefaultLocaleName(buf: *mut u16, len: i32) -> i32;
}

/// Ngôn ngữ của Windows theo tên locale ("vi-VN" → Vi, "ja-JP" → Ja...). Không có thì dùng English.
pub fn detect_system() -> Lang {
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    let name = if n > 1 { String::from_utf16_lossy(&buf[..(n as usize - 1)]) } else { String::new() };
    from_locale(&name)
}

/// Đổi tên locale ("zh-CN", "id-ID", "in-ID"...) sang ngôn ngữ có sẵn; ngôn ngữ chưa có thì English.
pub fn from_locale(name: &str) -> Lang {
    let primary = name.to_lowercase();
    let primary = primary.split(['-', '_']).next().unwrap_or("");
    let primary = if primary == "in" { "id" } else { primary }; // mã cũ của tiếng Indonesia
    Lang::from_code(primary).unwrap_or(Lang::En)
}

/// "auto" (hoặc rỗng) → theo Windows; "vi"/"en" → đúng ngôn ngữ đó.
pub fn resolve(pref: &str) -> Lang {
    Lang::from_code(pref).unwrap_or_else(detect_system)
}

fn table(l: Lang) -> &'static [(&'static str, &'static str)] {
    match l {
        Lang::Vi => VI,
        Lang::En => EN,
        Lang::Zh => zh::TABLE,
        Lang::Ja => ja::TABLE,
        Lang::Ko => ko::TABLE,
        Lang::Id => id::TABLE,
        Lang::Es => es::TABLE,
        Lang::Fr => fr::TABLE,
        Lang::De => de::TABLE,
        Lang::Ru => ru::TABLE,
    }
}

// Mỗi ngôn ngữ thêm nằm trong một file riêng ở src/lang/
#[path = "lang/zh.rs"]
mod zh;
#[path = "lang/ja.rs"]
mod ja;
#[path = "lang/ko.rs"]
mod ko;
#[path = "lang/id.rs"]
mod id;
#[path = "lang/es.rs"]
mod es;
#[path = "lang/fr.rs"]
mod fr;
#[path = "lang/de.rs"]
mod de;
#[path = "lang/ru.rs"]
mod ru;

fn lookup(l: Lang, key: &str) -> Option<&'static str> {
    table(l).iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Chữ theo ngôn ngữ hiện tại. Thiếu khóa thì lùi về tiếng Việt, rồi mới trả lại chính khóa.
pub fn t(key: &str) -> String {
    lookup(current(), key).or_else(|| lookup(Lang::Vi, key)).unwrap_or(key).to_string()
}

/// Như `t` nhưng thay `{0}`, `{1}`… bằng các tham số.
pub fn tf(key: &str, args: &[&str]) -> String {
    let mut s = t(key);
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), a);
    }
    s
}

// =====================================================================================================
//                                          TIẾNG VIỆT
// =====================================================================================================
static VI: &[(&str, &str)] = &[
    // --- chung
    ("app.tagline", "LarmaMic: dùng điện thoại làm mic cho PC"),
    ("lang.title", "Ngôn ngữ"),
    ("lang.auto", "Tự động (theo Windows)"),
    ("lang.auto.short", "Tự động"),
    ("btn.understood", "Đã hiểu"),
    ("btn.save", "Lưu"),
    ("btn.cancel", "Hủy"),
    // --- trạng thái kết nối
    ("status.connected", "Điện thoại đã kết nối"),
    ("status.waiting", "Đang chờ điện thoại"),
    ("srv.starting", "Đang khởi động máy chủ…"),
    ("pair.scan", "Quét mã bằng điện thoại hoặc mở:"),
    ("pair.copy", "Chép địa chỉ"),
    ("pair.rotate", "Đổi mã"),
    ("pair.rotate.hint", "Tạo mã ghép nối mới và ngắt điện thoại đang nối. Cần quét lại QR."),
    ("pair.cert", "Lần đầu, trình duyệt báo kết nối “không riêng tư” (chứng chỉ tự ký): chọn Nâng cao hoặc Hiện chi tiết, rồi tiếp tục vào trang web. Nếu điện thoại không vào được, cho phép LarmaMic qua Windows Firewall."),
    ("pair.alt", "Cũng truy cập được qua (USB / Bluetooth / điểm phát sóng): {0}"),
    ("mode.usb", "Cáp USB"),
    ("mode.wifi.hint", "Điện thoại và PC phải cùng Wi-Fi."),
    ("mode.usb.hint", "Cắm cáp vào PC, rồi bật Gỡ lỗi USB trên điện thoại (tự nhận) hoặc bật Chia sẻ kết nối qua USB (Mạng > Điểm phát sóng và chia sẻ). Không cần Wi-Fi."),
    ("mode.bt.hint", "Ghép đôi điện thoại với PC trong cài đặt Bluetooth, bật chia sẻ kết nối qua Bluetooth trên điện thoại và vào mạng đó trên PC, rồi quét mã QR. Chậm hơn Wi-Fi: âm thanh gửi ở 24 kHz."),
    ("mode.none", "Chưa thấy mạng USB hoặc Bluetooth. Làm theo các bước trên rồi chờ vài giây."),
    ("adb.ready", "Đã nhận điện thoại qua cáp USB. LarmaMic tự mở trên điện thoại và tự kết nối, không cần quét QR."),
    ("adb.unauth", "Thấy điện thoại nhưng chưa được cho phép. Mở khóa và bấm Cho phép ở hộp thoại gỡ lỗi USB."),
    ("adb.missing", "Để tự nhận điện thoại qua cáp, LarmaMic cần công cụ adb của Android (hoặc dùng chia sẻ mạng USB)."),
    ("adb.get", "Tải adb (từ Google, khoảng 6 MB)"),
    ("adb.getting", "Đang tải adb…"),
    ("adb.fail", "Không tải được adb: {0}"),
    ("adb.wait", "Muốn tự nhận: bật Gỡ lỗi USB trên điện thoại (một lần) rồi cắm cáp."),
    // --- thiết bị
    ("dev.title", "Mic ảo (đầu ra)"),
    ("dev.refresh", "Làm mới danh sách"),
    ("dev.nocable", "Chưa thấy mic ảo (VB-Cable hoặc Steam)."),
    ("dev.getcable", "Tải VB-Cable (miễn phí)"),
    ("dev.wrong", "Đang xuất ra “{0}”, nên Discord/OBS/game sẽ không nghe thấy tiếng."),
    ("dev.usecable", "Dùng mic ảo"),
    ("dev.ok", "Đang xuất vào mic ảo. Trong Discord, OBS, game: chọn “{0}” làm micro."),
    ("dev.none", "Chưa chọn"),
    ("monitor.title", "Nghe thử"),
    ("monitor.hint", "Phát ra loa/tai nghe mặc định. Nên dùng tai nghe."),
    // --- phím tắt
    ("hk.title", "Phím tắt tắt/bật tiếng"),
    ("hk.hint", "Dùng được cả khi đang chơi game hoặc app bị thu vào khay."),
    ("hotkey.off", "Không dùng"),
    ("hk.err.global", "Không đăng ký được phím tắt toàn hệ thống."),
    ("hk.err.invalid", "Phím tắt không hợp lệ: {0}"),
    ("hk.err.busy", "Phím này đang được chương trình khác dùng ({0}). Chọn phím khác."),
    ("beep.title", "Phát tiếng bíp"),
    ("beep.hint", "Thấp = đã tắt tiếng, cao = đã bật tiếng."),
    // --- khởi động, khay
    ("autostart.title", "Chạy cùng Windows"),
    ("autostart.hint", "Khởi động ẩn ở khay hệ thống."),
    ("tray.note", "Đóng cửa sổ chỉ thu app vào khay hệ thống, mic vẫn chạy. Muốn tắt hẳn: bấm phải icon ở khay, chọn Thoát."),
    ("tray.open", "Mở LarmaMic"),
    ("tray.mute", "Tắt tiếng mic"),
    ("tray.unmute", "Bật tiếng mic"),
    ("tray.quit", "Thoát"),
    ("tip.muted", " · TẮT TIẾNG"),
    ("tip.rec", " · đang ghi âm"),
    ("tray.err.tip", "không đổi được tooltip khay: {0}"),
    ("tray.log.tip", "tooltip khay: {0}"),
    // --- độ trễ
    ("lat.title", "Độ trễ"),
    ("lat.value", "≈ {0} ms từ lúc nói đến lúc ra thiết bị"),
    ("lat.waiting", "Hiện khi điện thoại đã nối."),
    ("lat.stats", "Đệm {0} ms · giật {1} lần · bỏ gói {2} lần"),
    ("lat.rtt", " · khứ hồi {0} ms"),
    ("lat.note", "Con số là ước tính, chưa tính độ trễ của Discord hay game."),
    // --- công cụ, cài đặt vào máy
    ("tools.title", "Công cụ"),
    ("diag.copied", "Đã chép vào clipboard"),
    ("diag.btn", "Chép thông tin chẩn đoán"),
    ("diag.hint", "Gom phiên bản, thiết bị âm thanh, lỗi gần đây vào clipboard để gửi khi cần hỗ trợ. Không chứa mã ghép nối."),
    ("inst.done", "Đã cài vào máy: có trong Start Menu và Settings > Apps."),
    ("inst.uninstall", "Gỡ cài đặt…"),
    ("inst.btn", "Cài vào máy…"),
    ("inst.hint", "Chép app vào thư mục của bạn, thêm lối tắt Start Menu và mục gỡ cài đặt. Không cần quyền quản trị."),
    ("inst.running", "Đang cài…"),
    ("inst.ok", "{0}. Lần sau mở LarmaMic từ Start Menu."),
    ("inst.err", "Lỗi: {0}"),
    ("inst.title", "Cài đặt LarmaMic"),
    ("inst.fail", "Cài đặt thất bại: {0}"),
    ("inst.already", "Đã cài từ trước."),
    ("inst.installed", "Đã cài vào {0}"),
    ("inst.err.exe", "Không biết exe đang ở đâu: {0}"),
    ("inst.err.dir", "Không tạo được thư mục cài đặt: {0}"),
    ("inst.err.copy", "Không chép được file vào {0} ({1}). Nếu LarmaMic đã cài đang chạy, hãy thoát nó rồi thử lại."),
    ("inst.err.reg", "Không ghi được mục gỡ cài đặt ({0}) vào registry."),
    ("inst.warn.lnk", "cảnh báo: không tạo được lối tắt Start Menu"),
    ("un.title", "Gỡ cài đặt LarmaMic"),
    ("un.confirm", "Gỡ LarmaMic khỏi máy này?\n\nLối tắt Start Menu và mục trong Settings > Apps sẽ bị xóa. Cài đặt của bạn vẫn được giữ lại."),
    ("un.cancelled", "Đã hủy."),
    ("un.ask.settings", "Xóa luôn cài đặt, hồ sơ và mã ghép nối của bạn?\n\nChọn Không nếu sau này muốn cài lại và dùng tiếp."),
    ("un.deleted.settings", " Đã xóa cả cài đặt."),
    ("un.done", "Đã gỡ LarmaMic.{0}"),
    ("un.fail", "Gỡ cài đặt thất bại: {0}"),
    // --- đồng hồ, cổng
    ("meter.title", "Mức âm"),
    ("meter.hint", "Kéo vạch trắng để đặt ngưỡng cổng ồn"),
    ("meter.in", "Vào  {0}"),
    ("meter.out", "Ra  {0}"),
    ("meter.thr", "ngưỡng {0}"),
    ("gate.off", "Cổng tắt"),
    ("gate.open", "Cổng mở"),
    ("gate.closed", "Cổng đóng"),
    ("spec.title", "Phổ âm đầu ra"),
    ("eq.title", "Đường cong EQ"),
    // --- cấu hình nhanh, hồ sơ, ghi âm
    ("preset.voice", "Giọng nói"),
    ("preset.game", "Game"),
    ("preset.podcast", "Podcast"),
    ("preset.raw", "Gốc"),
    ("chip.mute", "Tắt tiếng"),
    ("prof.label", "Hồ sơ của bạn"),
    ("prof.name", "Tên hồ sơ"),
    ("prof.save", "Lưu cấu hình hiện tại…"),
    ("prof.delete", "Xóa “{0}”"),
    ("rec.start", "● Ghi âm"),
    ("rec.stop", "■ Dừng ghi"),
    ("rec.hint", "Ghi giọng đã qua bộ lọc vào file WAV trong Documents\\LarmaMic."),
    ("rec.saved", "Đã lưu {0}"),
    ("rec.open", "Mở thư mục"),
    ("rec.err.close", "Lỗi khi đóng file ghi âm: {0}"),
    ("rec.err.write", "Lỗi ghi file: {0}"),
    ("rec.err.home", "Không tìm thấy thư mục người dùng."),
    ("rec.err.dir", "Không tạo được thư mục ghi âm: {0}"),
    ("rec.err.file", "Không tạo được file ghi âm: {0}"),
    // --- tự đặt ngưỡng
    ("cal.measuring", "Đang đo nền ồn… hãy giữ im lặng"),
    ("cal.done", "Nền ồn {0} → đã đặt ngưỡng {1}"),
    ("cal.noisy", "Nền khá ồn hoặc bạn vừa nói. Thử lại khi im lặng hoàn toàn."),
    ("cal.fail", "Không đo được. Cần điện thoại đang nối và đang gửi tiếng."),
    ("cal.btn", "Tự đặt ngưỡng (im lặng 3 giây)"),
    ("cal.btn.hint", "Đo tiếng ồn nền rồi đặt ngưỡng cổng ngay phía trên nó. Cần điện thoại đang nối."),
    ("cal.need", "Cần điện thoại đang nối và đang gửi tiếng."),
    // --- các nhóm chỉnh
    ("sec.noise", "Lọc ồn"),
    ("sec.noise.lead", "RNNoise là mạng nơ-ron tách giọng khỏi quạt, bàn phím, tiếng phố."),
    ("ctl.ai", "Khử ồn AI (RNNoise)"),
    ("ctl.ai_mix", "Mức khử ồn AI"),
    ("ctl.ai_mix.hint", "100% sạch nhất, 70–90% nghe tự nhiên hơn"),
    ("ctl.gate", "Cổng ồn"),
    ("ctl.gate.hint", "Cắt nền khi bạn ngừng nói"),
    ("ctl.thr", "Ngưỡng cổng"),
    ("ctl.thr.hint", "đặt ngay trên mức ồn nền"),
    ("ctl.depth", "Độ giảm khi đóng"),
    ("ctl.depth.hint", "cao hơn thì nền im hơn"),
    ("ctl.hp", "Cắt tiếng ù"),
    ("ctl.hp.hint", "bỏ gió, rung bàn, ù điện"),
    ("sec.eq", "Âm sắc"),
    ("sec.eq.lead", "Mỗi thanh chạy từ −12 đến +12 dB."),
    ("ctl.low", "Trầm"),
    ("ctl.low.hint", "120 Hz"),
    ("ctl.mud", "Độ đục"),
    ("ctl.mud.hint", "hạ xuống nếu giọng bị ồm"),
    ("ctl.pres", "Rõ giọng"),
    ("ctl.pres.hint", "nâng lên để nghe rõ chữ"),
    ("ctl.air", "Độ sáng"),
    ("ctl.air.hint", "9 kHz"),
    ("sec.fx", "Hiệu ứng giọng"),
    ("sec.fx.lead", "Dùng cho vui hoặc streaming. Tắt hết thì giọng giữ nguyên."),
    ("ctl.pitch", "Đổi tông"),
    ("ctl.pitch.hint", "âm: trầm hơn, dương: cao hơn"),
    ("ctl.pitch.unit", " bán cung"),
    ("fx.latency", "Đổi tông thêm khoảng 21 ms độ trễ."),
    ("ctl.reverb", "Vang"),
    ("ctl.robot", "Giọng robot"),
    ("sec.vol", "Âm lượng và độ trễ"),
    ("sec.vol.lead", "Bộ chống vỡ tiếng luôn bật ở −3 dB."),
    ("ctl.agc", "Tự cân âm lượng"),
    ("ctl.agc.hint", "Giữ giọng đều dù ngồi xa hay gần. Chỉ chỉnh khi có tiếng nói, không khuếch đại nền ồn."),
    ("ctl.comp", "Nén giọng"),
    ("ctl.comp.hint", "giọng đều hơn, nhỏ không chìm"),
    ("ctl.gain", "Âm lượng"),
    ("ctl.buf", "Đệm chống giật"),
    ("ctl.buf.hint", "thấp thì nhanh nhưng dễ giật; Wi-Fi 5 GHz: 40–80 ms"),
    // --- ghi chú khi đọc cài đặt
    ("note.backup", "File cài đặt bị hỏng, đã khôi phục từ bản sao lưu."),
    ("note.defaults", "File cài đặt bị hỏng và không có bản sao lưu, đã dùng cài đặt mặc định (cần quét lại QR)."),
    // --- lỗi thiết bị / máy chủ
    ("audio.err.format", "định dạng âm thanh {0} chưa được hỗ trợ"),
    ("out.err.missing", "Không thấy thiết bị “{0}”."),
    ("out.err.open", "Không mở được “{0}”: {1}"),
    ("srv.err.cert", "không tạo được chứng chỉ: {0}"),
    ("srv.err.noip", "Không tìm thấy địa chỉ Wi-Fi/LAN của PC."),
    ("srv.err.tls", "chứng chỉ HTTPS lỗi: {0}"),
    ("srv.err.ports", "Cổng 8443–8459 đều đang bận."),
    ("srv.err.stopped", "server dừng: {0}"),
    ("srv.why.unknown", "không rõ"),
    ("srv.why.recv", "lỗi khi nhận dữ liệu: {0}"),
    ("srv.why.closed", "Điện thoại đóng kết nối"),
    ("srv.why.replaced", "có điện thoại khác kết nối hoặc đã đổi mã"),
    ("srv.why.closecmd", "Điện thoại gửi lệnh đóng"),
    ("srv.why.timeout", "không nhận được gì từ điện thoại quá 6 giây"),
    ("srv.why.send.state", "gửi trạng thái thất bại: {0}"),
    ("srv.why.send.ping", "gửi ping thất bại: {0}"),
    ("srv.why.send.meter", "gửi số đo thất bại: {0}"),
    ("srv.log.disc", "Điện thoại ngắt kết nối: {0}"),
    // --- chẩn đoán
    ("diag.title", "LarmaMic v{0} (chẩn đoán lúc {1})"),
    ("diag.windows", "Windows: {0}"),
    ("diag.unknown", "không rõ"),
    ("diag.installed", "Đã cài vào máy: {0}"),
    ("diag.direct", "không (chạy trực tiếp)"),
    ("diag.yes", "có"),
    ("diag.no", "không"),
    ("diag.sec.conn", "== Kết nối =="),
    ("diag.server", "Máy chủ: {0}"),
    ("diag.notrunning", "chưa chạy"),
    ("diag.server.err", "Lỗi máy chủ: {0}"),
    ("diag.phone", "Điện thoại: {0}"),
    ("diag.conn", "đã kết nối"),
    ("diag.wait", "đang chờ"),
    ("diag.lastdisc", "Lần ngắt kết nối gần nhất: {0}"),
    ("diag.stats", "Đệm {0} ms · giật {1} · bỏ gói {2} · khứ hồi {3} ms · độ trễ ước tính {4} ms"),
    ("diag.sec.audio", "== Âm thanh =="),
    ("diag.out", "Đầu ra đang chọn: {0}"),
    ("diag.iscable", "Là VB-Cable: {0} · tần số thiết bị: {1} Hz"),
    ("diag.out.err", "Lỗi đầu ra: {0}"),
    ("diag.devices", "Các thiết bị phát ({0}):"),
    ("diag.sec.other", "== Khác =="),
    ("diag.hotkey", "Phím tắt: {0}{1}"),
    ("diag.hotkey.err", " (lỗi: {0})"),
    ("diag.rec", "Ghi âm: {0}"),
    ("diag.rec.on", "đang ghi"),
    ("diag.rec.off", "tắt"),
    ("diag.rec.err", "Lỗi ghi âm: {0}"),
    ("diag.note", "Ghi chú cài đặt: {0}"),
    ("diag.dir", "Thư mục cài đặt: {0}"),
    ("diag.profiles", "Hồ sơ riêng: {0}"),
    ("diag.params", "Thông số: {0}"),
];

// =====================================================================================================
//                                            ENGLISH
// =====================================================================================================
static EN: &[(&str, &str)] = &[
    // --- common
    ("app.tagline", "LarmaMic: use your phone as a PC microphone"),
    ("lang.title", "Language"),
    ("lang.auto", "Automatic (follow Windows)"),
    ("lang.auto.short", "Auto"),
    ("btn.understood", "Got it"),
    ("btn.save", "Save"),
    ("btn.cancel", "Cancel"),
    // --- connection status
    ("status.connected", "Phone connected"),
    ("status.waiting", "Waiting for phone"),
    ("srv.starting", "Starting server…"),
    ("pair.scan", "Scan with your phone or open:"),
    ("pair.copy", "Copy address"),
    ("pair.rotate", "New code"),
    ("pair.rotate.hint", "Creates a new pairing code and disconnects the current phone. You will need to scan the QR again."),
    ("pair.cert", "The first time, the browser warns that the connection is “not private” (the certificate is self-signed): choose Advanced or Show Details, then continue to the site. If the phone can't connect, allow LarmaMic through Windows Firewall."),
    ("pair.alt", "Also reachable via (USB / Bluetooth / hotspot): {0}"),
    ("mode.usb", "USB cable"),
    ("mode.wifi.hint", "The phone and the PC must be on the same Wi-Fi."),
    ("mode.usb.hint", "Plug the cable into the PC, then either turn on USB debugging on the phone (detected automatically) or turn on USB tethering (Network > Hotspot and tethering). No Wi-Fi needed."),
    ("mode.bt.hint", "Pair the phone with the PC in Bluetooth settings, turn on Bluetooth tethering on the phone and join its network on the PC, then scan the QR code. Slower than Wi-Fi: audio is sent at 24 kHz."),
    ("mode.none", "No USB or Bluetooth network found yet. Follow the steps above, then wait a few seconds."),
    ("adb.ready", "Phone detected over the USB cable. LarmaMic opens on the phone and connects by itself, no QR code needed."),
    ("adb.unauth", "Phone found but not authorised. Unlock it and tap Allow on the USB debugging prompt."),
    ("adb.missing", "To detect the phone automatically over the cable, LarmaMic needs Android's adb tool (or use USB tethering instead)."),
    ("adb.get", "Download adb (from Google, about 6 MB)"),
    ("adb.getting", "Downloading adb…"),
    ("adb.fail", "Could not download adb: {0}"),
    ("adb.wait", "For automatic detection: turn on USB debugging on the phone (once) and plug it in."),
    // --- device
    ("dev.title", "Virtual mic (output)"),
    ("dev.refresh", "Refresh list"),
    ("dev.nocable", "Virtual mic (VB-Cable or Steam) not found."),
    ("dev.getcable", "Get VB-Cable (free)"),
    ("dev.wrong", "Output is “{0}”, so Discord/OBS/games won't hear anything."),
    ("dev.usecable", "Use virtual mic"),
    ("dev.ok", "Output is the virtual mic. In Discord, OBS or games, choose “{0}” as the microphone."),
    ("dev.none", "Not selected"),
    ("monitor.title", "Listen"),
    ("monitor.hint", "Plays through the default speakers/headphones. Headphones recommended."),
    // --- hotkey
    ("hk.title", "Mute / unmute hotkey"),
    ("hk.hint", "Works while gaming or when the app is in the tray."),
    ("hotkey.off", "Disabled"),
    ("hk.err.global", "Could not register a system-wide hotkey."),
    ("hk.err.invalid", "Invalid hotkey: {0}"),
    ("hk.err.busy", "This key is already used by another program ({0}). Pick another."),
    ("beep.title", "Beep on toggle"),
    ("beep.hint", "Low = muted, high = unmuted."),
    // --- startup, tray
    ("autostart.title", "Start with Windows"),
    ("autostart.hint", "Starts hidden in the system tray."),
    ("tray.note", "Closing the window only sends the app to the system tray; the mic keeps running. To quit: right-click the tray icon and choose Exit."),
    ("tray.open", "Open LarmaMic"),
    ("tray.mute", "Mute mic"),
    ("tray.unmute", "Unmute mic"),
    ("tray.quit", "Exit"),
    ("tip.muted", " · MUTED"),
    ("tip.rec", " · recording"),
    ("tray.err.tip", "could not update the tray tooltip: {0}"),
    ("tray.log.tip", "tray tooltip: {0}"),
    // --- latency
    ("lat.title", "Latency"),
    ("lat.value", "≈ {0} ms from speaking to device output"),
    ("lat.waiting", "Shown once the phone is connected."),
    ("lat.stats", "Buffer {0} ms · {1} glitches · {2} drops"),
    ("lat.rtt", " · round trip {0} ms"),
    ("lat.note", "This is an estimate and excludes Discord's or the game's own latency."),
    // --- tools, installation
    ("tools.title", "Tools"),
    ("diag.copied", "Copied to clipboard"),
    ("diag.btn", "Copy diagnostics"),
    ("diag.hint", "Copies the version, audio devices and recent errors to the clipboard so you can send them when you need help. Does not include the pairing code."),
    ("inst.done", "Installed: available in the Start Menu and Settings > Apps."),
    ("inst.uninstall", "Uninstall…"),
    ("inst.btn", "Install…"),
    ("inst.hint", "Copies the app to your user folder and adds a Start Menu shortcut and an uninstall entry. No administrator rights needed."),
    ("inst.running", "Installing…"),
    ("inst.ok", "{0}. Next time, open LarmaMic from the Start Menu."),
    ("inst.err", "Error: {0}"),
    ("inst.title", "Install LarmaMic"),
    ("inst.fail", "Installation failed: {0}"),
    ("inst.already", "Already installed."),
    ("inst.installed", "Installed to {0}"),
    ("inst.err.exe", "Could not locate the running exe: {0}"),
    ("inst.err.dir", "Could not create the install folder: {0}"),
    ("inst.err.copy", "Could not copy the file to {0} ({1}). If the installed LarmaMic is running, quit it and try again."),
    ("inst.err.reg", "Could not write the uninstall entry ({0}) to the registry."),
    ("inst.warn.lnk", "warning: could not create the Start Menu shortcut"),
    ("un.title", "Uninstall LarmaMic"),
    ("un.confirm", "Remove LarmaMic from this computer?\n\nThe Start Menu shortcut and the Settings > Apps entry will be deleted. Your settings are kept."),
    ("un.cancelled", "Cancelled."),
    ("un.ask.settings", "Also delete your settings, profiles and pairing code?\n\nChoose No if you may reinstall and keep using them."),
    ("un.deleted.settings", " Settings were deleted too."),
    ("un.done", "LarmaMic was uninstalled.{0}"),
    ("un.fail", "Uninstall failed: {0}"),
    // --- meter, gate
    ("meter.title", "Level"),
    ("meter.hint", "Drag the white line to set the noise gate threshold"),
    ("meter.in", "In  {0}"),
    ("meter.out", "Out  {0}"),
    ("meter.thr", "threshold {0}"),
    ("gate.off", "Gate off"),
    ("gate.open", "Gate open"),
    ("gate.closed", "Gate closed"),
    ("spec.title", "Output spectrum"),
    ("eq.title", "EQ curve"),
    // --- presets, profiles, recording
    ("preset.voice", "Voice"),
    ("preset.game", "Game"),
    ("preset.podcast", "Podcast"),
    ("preset.raw", "Raw"),
    ("chip.mute", "Mute"),
    ("prof.label", "Your profiles"),
    ("prof.name", "Profile name"),
    ("prof.save", "Save current settings…"),
    ("prof.delete", "Delete “{0}”"),
    ("rec.start", "● Record"),
    ("rec.stop", "■ Stop"),
    ("rec.hint", "Records the filtered voice to a WAV file in Documents\\LarmaMic."),
    ("rec.saved", "Saved {0}"),
    ("rec.open", "Open folder"),
    ("rec.err.close", "Error while closing the recording file: {0}"),
    ("rec.err.write", "File write error: {0}"),
    ("rec.err.home", "Could not find the user folder."),
    ("rec.err.dir", "Could not create the recording folder: {0}"),
    ("rec.err.file", "Could not create the recording file: {0}"),
    // --- auto threshold
    ("cal.measuring", "Measuring background noise… stay quiet"),
    ("cal.done", "Noise floor {0} → threshold set to {1}"),
    ("cal.noisy", "The background is loud or you were speaking. Try again in complete silence."),
    ("cal.fail", "Could not measure. The phone must be connected and sending audio."),
    ("cal.btn", "Auto-set threshold (3 s of silence)"),
    ("cal.btn.hint", "Measures background noise and sets the gate threshold just above it. Needs a connected phone."),
    ("cal.need", "Needs a connected phone that is sending audio."),
    // --- control groups
    ("sec.noise", "Noise reduction"),
    ("sec.noise.lead", "RNNoise is a neural network that separates your voice from fans, keyboards and street noise."),
    ("ctl.ai", "AI noise removal (RNNoise)"),
    ("ctl.ai_mix", "AI noise removal amount"),
    ("ctl.ai_mix.hint", "100% is cleanest, 70–90% sounds more natural"),
    ("ctl.gate", "Noise gate"),
    ("ctl.gate.hint", "Cuts the background when you stop talking"),
    ("ctl.thr", "Gate threshold"),
    ("ctl.thr.hint", "set just above the background noise"),
    ("ctl.depth", "Reduction when closed"),
    ("ctl.depth.hint", "higher = quieter background"),
    ("ctl.hp", "Rumble filter"),
    ("ctl.hp.hint", "removes wind, desk thumps and mains hum"),
    ("sec.eq", "Tone"),
    ("sec.eq.lead", "Each slider ranges from −12 to +12 dB."),
    ("ctl.low", "Bass"),
    ("ctl.low.hint", "120 Hz"),
    ("ctl.mud", "Muddiness"),
    ("ctl.mud.hint", "lower it if the voice sounds boomy"),
    ("ctl.pres", "Clarity"),
    ("ctl.pres.hint", "raise it for clearer words"),
    ("ctl.air", "Brightness"),
    ("ctl.air.hint", "9 kHz"),
    ("sec.fx", "Voice effects"),
    ("sec.fx.lead", "For fun or streaming. With everything off, your voice is unchanged."),
    ("ctl.pitch", "Pitch"),
    ("ctl.pitch.hint", "negative: deeper, positive: higher"),
    ("ctl.pitch.unit", " semitones"),
    ("fx.latency", "Pitch shifting adds about 21 ms of latency."),
    ("ctl.reverb", "Reverb"),
    ("ctl.robot", "Robot voice"),
    ("sec.vol", "Volume and latency"),
    ("sec.vol.lead", "The clipping limiter is always on at −3 dB."),
    ("ctl.agc", "Auto level (AGC)"),
    ("ctl.agc.hint", "Keeps your voice even whether you sit near or far. Adjusts only while you speak and never boosts background noise."),
    ("ctl.comp", "Compression"),
    ("ctl.comp.hint", "evens out the voice; quiet parts don't get lost"),
    ("ctl.gain", "Volume"),
    ("ctl.buf", "Anti-glitch buffer"),
    ("ctl.buf.hint", "lower is faster but may glitch; on 5 GHz Wi-Fi use 40–80 ms"),
    // --- settings file notes
    ("note.backup", "The settings file was damaged and was restored from the backup."),
    ("note.defaults", "The settings file was damaged and there is no backup; defaults were used (you need to scan the QR again)."),
    // --- device / server errors
    ("audio.err.format", "audio format {0} is not supported"),
    ("out.err.missing", "Device “{0}” not found."),
    ("out.err.open", "Could not open “{0}”: {1}"),
    ("srv.err.cert", "could not create the certificate: {0}"),
    ("srv.err.noip", "Could not find this PC's Wi-Fi/LAN address."),
    ("srv.err.tls", "HTTPS certificate error: {0}"),
    ("srv.err.ports", "Ports 8443–8459 are all busy."),
    ("srv.err.stopped", "server stopped: {0}"),
    ("srv.why.unknown", "unknown"),
    ("srv.why.recv", "error while receiving data: {0}"),
    ("srv.why.closed", "the phone closed the connection"),
    ("srv.why.replaced", "another phone connected or the code was changed"),
    ("srv.why.closecmd", "the phone sent a close command"),
    ("srv.why.timeout", "nothing received from the phone for more than 6 seconds"),
    ("srv.why.send.state", "failed to send state: {0}"),
    ("srv.why.send.ping", "failed to send ping: {0}"),
    ("srv.why.send.meter", "failed to send levels: {0}"),
    ("srv.log.disc", "Phone disconnected: {0}"),
    // --- diagnostics
    ("diag.title", "LarmaMic v{0} (diagnostics at {1})"),
    ("diag.windows", "Windows: {0}"),
    ("diag.unknown", "unknown"),
    ("diag.installed", "Installed: {0}"),
    ("diag.direct", "no (running directly)"),
    ("diag.yes", "yes"),
    ("diag.no", "no"),
    ("diag.sec.conn", "== Connection =="),
    ("diag.server", "Server: {0}"),
    ("diag.notrunning", "not running"),
    ("diag.server.err", "Server error: {0}"),
    ("diag.phone", "Phone: {0}"),
    ("diag.conn", "connected"),
    ("diag.wait", "waiting"),
    ("diag.lastdisc", "Last disconnect: {0}"),
    ("diag.stats", "Buffer {0} ms · glitches {1} · drops {2} · round trip {3} ms · estimated latency {4} ms"),
    ("diag.sec.audio", "== Audio =="),
    ("diag.out", "Selected output: {0}"),
    ("diag.iscable", "Is VB-Cable: {0} · device rate: {1} Hz"),
    ("diag.out.err", "Output error: {0}"),
    ("diag.devices", "Playback devices ({0}):"),
    ("diag.sec.other", "== Other =="),
    ("diag.hotkey", "Hotkey: {0}{1}"),
    ("diag.hotkey.err", " (error: {0})"),
    ("diag.rec", "Recording: {0}"),
    ("diag.rec.on", "recording"),
    ("diag.rec.off", "off"),
    ("diag.rec.err", "Recording error: {0}"),
    ("diag.note", "Settings note: {0}"),
    ("diag.dir", "Settings folder: {0}"),
    ("diag.profiles", "Profiles: {0}"),
    ("diag.params", "Parameters: {0}"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn keys(tbl: &[(&str, &str)]) -> BTreeSet<String> {
        tbl.iter().map(|(k, _)| k.to_string()).collect()
    }

    /// Các chỗ giữ chỗ {0}, {1}… có trong một chuỗi.
    fn placeholders(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'{' {
                if let Some(j) = s[i..].find('}') {
                    let inner = &s[i + 1..i + j];
                    if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                        out.insert(inner.to_string());
                    }
                    i += j;
                }
            }
            i += 1;
        }
        out
    }

    #[test]
    fn every_language_has_exactly_the_same_keys() {
        let base = keys(VI);
        assert_eq!(base.len(), VI.len(), "khóa bị trùng trong bảng Việt");
        for l in Lang::ALL {
            let tbl = table(l);
            let k = keys(tbl);
            assert_eq!(k.len(), tbl.len(), "khóa bị trùng trong bảng {:?}", l);
            let missing: Vec<_> = base.difference(&k).collect();
            let extra: Vec<_> = k.difference(&base).collect();
            assert!(missing.is_empty(), "{:?} thiếu khóa: {missing:?}", l);
            assert!(extra.is_empty(), "{:?} thừa khóa: {extra:?}", l);
        }
    }

    #[test]
    fn placeholders_match_across_languages_and_nothing_is_empty() {
        for (k, vi) in VI {
            for l in Lang::ALL {
                let v = lookup(l, k).unwrap();
                assert_eq!(placeholders(vi), placeholders(v), "khóa “{k}” lệch chỗ giữ chỗ giữa Việt và {:?}", l);
                assert!(!v.trim().is_empty(), "khóa “{k}” rỗng ở {:?}", l);
            }
        }
    }

    #[test]
    fn other_languages_have_no_vietnamese_letters() {
        // chỉ các chữ có trong tiếng Việt mà các ngôn ngữ còn lại (Pháp, Tây Ban Nha...) không dùng
        let vn = "ăơưđạảấầẩẫậắằẳẵặẹẻẽếềểễệỉĩịọỏốồổỗộớờởỡợụủứừửữựỳỷỹỵ";
        for l in Lang::ALL.into_iter().filter(|l| *l != Lang::Vi) {
            for (k, v) in table(l) {
                let low = v.to_lowercase();
                let bad: Vec<char> = low.chars().filter(|c| vn.contains(*c)).collect();
                assert!(bad.is_empty(), "{:?}: khóa “{k}” còn chữ tiếng Việt: {v}", l);
            }
        }
    }

    #[test]
    fn locale_names_map_to_languages() {
        assert_eq!(from_locale("zh-CN"), Lang::Zh);
        assert_eq!(from_locale("ja-JP"), Lang::Ja);
        assert_eq!(from_locale("in-ID"), Lang::Id);
        assert_eq!(from_locale("pt-BR"), Lang::En); // chưa có thì English
        assert_eq!(from_locale("ru_RU"), Lang::Ru);
    }

    // ---------- đọc mã nguồn để kiểm tra ----------
    const SOURCES: &[(&str, &str)] = &[
        ("ui.rs", include_str!("ui.rs")),
        ("win.rs", include_str!("win.rs")),
        ("main.rs", include_str!("main.rs")),
        ("diag.rs", include_str!("diag.rs")),
        ("server.rs", include_str!("server.rs")),
        ("engine.rs", include_str!("engine.rs")),
        ("settings.rs", include_str!("settings.rs")),
        ("wav.rs", include_str!("wav.rs")),
        ("dsp.rs", include_str!("dsp.rs")),
        ("icon.rs", include_str!("icon.rs")),
    ];

    /// Phần mã không phải bài kiểm tra.
    fn production(src: &str) -> &str {
        src.split("#[cfg(test)]").next().unwrap_or(src)
    }

    /// Trích mọi chuỗi ký tự "..." (bỏ qua chú thích // và /* */), trả về (số dòng, nội dung).
    fn literals(src: &str) -> Vec<(usize, String)> {
        let c: Vec<char> = src.chars().collect();
        let (mut out, mut i, mut line) = (Vec::new(), 0usize, 1usize);
        while i < c.len() {
            match c[i] {
                '\n' => {
                    line += 1;
                    i += 1;
                }
                '/' if c.get(i + 1) == Some(&'/') => {
                    while i < c.len() && c[i] != '\n' {
                        i += 1;
                    }
                }
                '/' if c.get(i + 1) == Some(&'*') => {
                    i += 2;
                    while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                        if c[i] == '\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                    i += 2;
                }
                'r' if matches!(c.get(i + 1), Some('"') | Some('#')) && (i == 0 || !c[i - 1].is_alphanumeric()) => {
                    // chuỗi thô r#"..."#
                    let mut j = i + 1;
                    let mut hashes = 0;
                    while c.get(j) == Some(&'#') {
                        hashes += 1;
                        j += 1;
                    }
                    if c.get(j) != Some(&'"') {
                        i += 1;
                        continue;
                    }
                    j += 1;
                    let start = j;
                    let start_line = line;
                    loop {
                        if j >= c.len() {
                            break;
                        }
                        if c[j] == '"' && (0..hashes).all(|h| c.get(j + 1 + h) == Some(&'#')) {
                            break;
                        }
                        if c[j] == '\n' {
                            line += 1;
                        }
                        j += 1;
                    }
                    out.push((start_line, c[start..j.min(c.len())].iter().collect()));
                    i = j + 1 + hashes;
                }
                '\'' => {
                    // ký tự '"' hoặc '\'' : bỏ qua cho khỏi nhầm thành mở chuỗi
                    if c.get(i + 1) == Some(&'\\') {
                        i += 4;
                    } else if c.get(i + 2) == Some(&'\'') {
                        i += 3;
                    } else {
                        i += 1; // dấu nháy của lifetime
                    }
                }
                '"' => {
                    let start_line = line;
                    let mut s = String::new();
                    i += 1;
                    while i < c.len() && c[i] != '"' {
                        if c[i] == '\\' {
                            s.push(c[i]);
                            i += 1;
                        }
                        if i < c.len() {
                            if c[i] == '\n' {
                                line += 1;
                            }
                            s.push(c[i]);
                        }
                        i += 1;
                    }
                    out.push((start_line, s));
                    i += 1;
                }
                _ => i += 1,
            }
        }
        out
    }

    fn has_vietnamese(s: &str) -> bool {
        s.chars().any(|ch| "ăâêôơưđĂÂÊÔƠƯĐàáảãạằắẳẵặầấẩẫậèéẻẽẹềếểễệìíỉĩịòóỏõọồốổỗộờớởỡợùúủũụừứửữựỳýỷỹỵÀÁẢÃẠẰẮẲẴẶẦẤẨẪẬÈÉẺẼẸỀẾỂỄỆÌÍỈĨỊÒÓỎÕỌỒỐỔỖỘỜỚỞỠỢÙÚỦŨỤỪỨỬỮỰỲÝỶỸỴ".contains(ch))
    }

    #[test]
    fn no_vietnamese_text_is_hardcoded_outside_the_catalog() {
        let mut bad = Vec::new();
        for (file, src) in SOURCES {
            for (line, lit) in literals(production(src)) {
                if has_vietnamese(&lit) {
                    bad.push(format!("{file}:{line}: “{lit}”"));
                }
            }
        }
        assert!(bad.is_empty(), "còn chuỗi tiếng Việt viết cứng (hãy chuyển vào bảng dịch):\n{}", bad.join("\n"));
    }

    /// Mọi khóa dùng trong `t("..")` / `tf("..")` phải có trong bảng.
    #[test]
    fn every_key_used_in_code_exists_and_args_are_enough() {
        let known: BTreeMap<&str, &str> = VI.iter().copied().collect();
        let mut problems = Vec::new();
        let mut used = BTreeSet::new();
        for (file, src) in SOURCES {
            let s = production(src);
            let b = s.as_bytes();
            for (needle, is_tf) in [("t(\"", false), ("tf(\"", true)] {
                let mut from = 0;
                while let Some(p) = s[from..].find(needle) {
                    let at = from + p;
                    from = at + needle.len();
                    // chữ đứng trước phải không thuộc một tên khác (vd set(", get(")
                    let prev = if at == 0 { b' ' } else { b[at - 1] };
                    if prev.is_ascii_alphanumeric() || prev == b'_' {
                        continue;
                    }
                    let rest = &s[from..];
                    let Some(end) = rest.find('"') else { continue };
                    let key = &rest[..end];
                    used.insert(key.to_string());
                    match known.get(key) {
                        None => problems.push(format!("{file}: khóa không có trong bảng: “{key}”")),
                        Some(v) if is_tf => {
                            // tf(key, &[a, b, ..]): đếm số tham số trong ngoặc vuông ngay sau khóa
                            let after = &rest[end + 1..];
                            if let Some(lb) = after.find("&[") {
                                let inner_start = lb + 2;
                                if let Some(rb) = after[inner_start..].find(']') {
                                    let n = if after[inner_start..inner_start + rb].trim().is_empty() {
                                        0
                                    } else {
                                        after[inner_start..inner_start + rb].matches(',').count() + 1
                                    };
                                    let need = placeholders(v).len();
                                    if n < need {
                                        problems.push(format!("{file}: “{key}” cần {need} tham số, chỉ truyền {n}"));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        assert!(used.len() > 100, "chỉ thấy {} khóa được dùng, có thể bộ dò sai", used.len());
    }

    #[test]
    fn t_and_tf_work_and_fall_back() {
        set(Lang::En);
        assert_eq!(t("tray.quit"), "Exit");
        assert_eq!(tf("prof.delete", &["Studio"]), "Delete “Studio”");
        assert_eq!(t("không.có.khóa"), "không.có.khóa", "khóa lạ trả lại chính nó");
        set(Lang::Vi);
        assert_eq!(t("tray.quit"), "Thoát");
        assert_eq!(tf("cal.done", &["−60 dB", "−53 dB"]), "Nền ồn −60 dB → đã đặt ngưỡng −53 dB");
        assert_eq!(Lang::from_code("en"), Some(Lang::En));
        assert_eq!(Lang::from_code("auto"), None);
    }
}
