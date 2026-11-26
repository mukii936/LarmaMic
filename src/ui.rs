//! Giao diện egui: kết nối + QR bên trái, đồng hồ và bộ chỉnh bên phải.
//! Thông số đồng bộ hai chiều với trang điện thoại qua `Shared::params_ver`.

use crate::dsp::{eq_response_db, BANDS};
use crate::engine::{self, load_f, Cmd, Shared};
use crate::i18n::{self, t, tf, Lang};
use crate::soundboard;
use crate::settings::{
    self, apply_preset, apply_profile, matches_preset, matches_profile, Params, Profile, Saved, DEFAULT_HOTKEY,
    PRESET_NAMES,
};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Layout,
    Rect, Response, RichText, Sense, Shape, Slider, Stroke, TextStyle, Ui,
};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager,
};
use std::ops::RangeInclusive;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(0x0d, 0x13, 0x17);
const PANEL: Color32 = Color32::from_rgb(0x11, 0x19, 0x1e);
const RAISED: Color32 = Color32::from_rgb(0x1a, 0x27, 0x30);
const INK: Color32 = Color32::from_rgb(0xe8, 0xf0, 0xf3);
const MUTE: Color32 = Color32::from_rgb(0x8f, 0xa6, 0xb1);
const LINE: Color32 = Color32::from_rgb(0x2a, 0x3a, 0x44);
const SIG: Color32 = Color32::from_rgb(0x38, 0xe0, 0xc8);
const WARN: Color32 = Color32::from_rgb(0xff, 0x6b, 0x6b);
const OFF: Color32 = Color32::from_rgb(0x17, 0x23, 0x2a);
const GHOST: Color32 = Color32::from_rgb(0x34, 0x4b, 0x57);
const ON_SIG: Color32 = Color32::from_rgb(0x04, 0x20, 0x1c);

/// Nạp font: Segoe UI làm chính (có đủ Latin, dấu tiếng Việt, Cyrillic). Riêng tiếng Trung/Nhật/Hàn thì
/// nạp thêm một font hệ thống của Windows, và CHỈ khi đang dùng ngôn ngữ đó (các file này nặng hàng chục
/// MB nên không nạp sẵn cho mọi người). `menu` = đang mở ô chọn ngôn ngữ: nạp tạm cả ba font ở cuối danh
/// sách để tên 中文 / 日本語 / 한국어 hiện đúng, đóng ô chọn thì nạp lại như cũ.
pub fn apply_fonts(ctx: &egui::Context, lang: Lang, menu: bool) {
    let mut fonts = FontDefinitions::default();
    if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
        if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
            f.insert(0, "segoe".into());
        }
    }
    let groups: [(Lang, &[&str]); 3] = [
        (Lang::Zh, &["msyh.ttc", "msyh.ttf", "simsun.ttc"]),
        (Lang::Ja, &["YuGothR.ttc", "YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"]),
        (Lang::Ko, &["malgun.ttf", "gulim.ttc"]),
    ];
    for (g, files) in groups {
        let primary = g == lang;
        if !primary && !menu {
            continue;
        }
        for file in files {
            if let Ok(bytes) = std::fs::read(format!("C:\\Windows\\Fonts\\{file}")) {
                let name = format!("cjk-{}", g.code());
                fonts.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
                if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
                    if primary {
                        // đứng đầu: chữ Latin và CJK trong cùng một dòng dùng chung font nên không bị lệch dòng
                        f.insert(0, name);
                    } else {
                        f.push(name);
                    }
                }
                break;
            }
        }
    }
    ctx.set_fonts(fonts);
}

pub fn setup(ctx: &egui::Context) {
    apply_fonts(ctx, i18n::current(), false);

    let r = CornerRadius::same(10);
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(INK);
    v.panel_fill = BG;
    v.window_fill = BG;
    v.extreme_bg_color = OFF;
    v.selection.bg_fill = SIG;
    v.selection.stroke = Stroke::new(1.0_f32, ON_SIG);
    v.hyperlink_color = SIG;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.inactive.bg_fill = GHOST;
    v.widgets.inactive.weak_bg_fill = RAISED;
    v.widgets.hovered.bg_fill = GHOST;
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x22, 0x33, 0x3e);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(0x2a, 0x3d, 0x49);
    // Ép theme tối: nếu Windows đang ở giao diện sáng, eframe sẽ áp bộ màu sáng đè lên.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v);

    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.spacing.item_spacing = vec2(10.0, 8.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.button_padding = vec2(14.0, 7.0);
        s.spacing.slider_rail_height = 8.0;
        s.text_styles = [
            (TextStyle::Heading, FontId::proportional(28.0)),
            (TextStyle::Body, FontId::proportional(16.0)),
            (TextStyle::Button, FontId::proportional(16.0)),
            (TextStyle::Small, FontId::proportional(13.0)),
            (TextStyle::Monospace, FontId::monospace(15.0)),
        ]
        .into();
    });
}

struct MeterState {
    d_in: f32,
    d_out: f32,
    peak: f32,
    peak_t: f32,
}

pub struct App {
    sh: Arc<Shared>,
    tx: Sender<Cmd>,
    devices: Vec<String>,
    out_sel: String,
    /// Chỉ lưu thiết bị đầu ra khi người dùng tự chọn; còn lại để app tự tìm VB-Cable mỗi lần mở.
    out_explicit: bool,
    p: Params,
    last_ver: u64,
    saved: Saved,
    last_save: Instant,
    last_monitor: bool,
    m: MeterState,
    qr_url: String,
    qr: Option<(usize, Vec<bool>)>,
    autostart: bool,
    start_hidden: bool,
    frames: u32,
    hook_installed: bool,
    // hồ sơ riêng
    new_profile: String,
    saving_profile: bool,
    focus_name: bool,
    // phổ âm đã làm mượt, ghi chú cài đặt, chẩn đoán, cài đặt vào máy
    spec_smooth: [f32; BANDS],
    note_dismissed: bool,
    diag_at: Option<Instant>,
    installed: bool,
    install_msg: Arc<parking_lot::Mutex<String>>,
    // phím tắt toàn hệ thống (đăng ký trên luồng giao diện, sự kiện xử lý ở luồng khay)
    hk_mgr: Option<GlobalHotKeyManager>,
    hk: Option<HotKey>,
    hk_err: Option<String>,
    hotkey: String,
    no_beep: bool,
    /// "auto" hoặc mã ngôn ngữ ("vi", "en", ...).
    lang_pref: String,
    /// "wifi" | "usb" | "bt": đổi địa chỉ đứng đầu trong QR và hướng dẫn hiện ra.
    conn_mode: String,
    mix_loudest: bool,
    no_discovery: bool,
    tab: Tab,
    update_dismissed: bool,
    auto_profile: bool,
    no_update_check: bool,
    new_rule_exe: String,
    new_rule_profile: String,
    board_url: String,
    sound_keys: Vec<HotKey>,
    last_board_ver: u64,
    /// Ô chọn ngôn ngữ đang mở: đã nạp tạm font CJK để hiện tên các ngôn ngữ.
    menu_fonts: bool,
}

/// Các phím tắt cho người dùng chọn: (chuỗi đăng ký, tên hiển thị). "off" được dịch riêng.
const HOTKEYS: [(&str, &str); 6] = [
    ("ctrl+alt+KeyM", "Ctrl + Alt + M"),
    ("ctrl+shift+KeyM", "Ctrl + Shift + M"),
    ("F9", "F9"),
    ("F10", "F10"),
    ("Pause", "Pause / Break"),
    ("off", ""),
];

fn hotkey_label(s: &str) -> String {
    if s == "off" {
        return t("hotkey.off");
    }
    HOTKEYS.iter().find(|(k, _)| *k == s).map(|(_, l)| l.to_string()).unwrap_or_else(|| s.to_string())
}

/// Đăng ký phím tắt; trả về (phím đã đăng ký, lỗi nếu có).
fn register_hotkey(mgr: &Option<GlobalHotKeyManager>, s: &str) -> (Option<HotKey>, Option<String>) {
    if s == "off" {
        return (None, None);
    }
    let Some(m) = mgr else {
        return (None, Some(t("hk.err.global")));
    };
    match s.parse::<HotKey>() {
        Err(e) => (None, Some(tf("hk.err.invalid", &[&e.to_string()]))),
        Ok(hk) => match m.register(hk) {
            Ok(()) => (Some(hk), None),
            Err(e) => (None, Some(tf("hk.err.busy", &[&e.to_string()]))),
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Connect,
    Sound,
    Board,
    Settings,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Connect, Tab::Sound, Tab::Board, Tab::Settings];
    fn code(self) -> &'static str {
        match self {
            Tab::Connect => "connect",
            Tab::Sound => "sound",
            Tab::Board => "board",
            Tab::Settings => "settings",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Tab::Connect => "tab.connect",
            Tab::Sound => "tab.sound",
            Tab::Board => "tab.board",
            Tab::Settings => "tab.settings",
        }
    }
    fn from_code(s: &str) -> Tab {
        Tab::ALL.into_iter().find(|t| t.code() == s).unwrap_or(Tab::Connect)
    }
}

impl App {
    /// Tự đo phòng: nói ~10 giây, app gợi ý ngưỡng cổng ồn và EQ (xem room.rs).
    fn room_card(&mut self, ui: &mut Ui) {
        let state = self.sh.room_state.load(Relaxed);
        let connected = self.sh.connected.load(Relaxed);
        egui::Frame::default().fill(OFF).corner_radius(10.0).inner_margin(12.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(t("room.title")).strong());
                match state {
                    1 => {
                        ui.label(RichText::new(t("room.measuring")).color(SIG));
                        let pr = f32::from_bits(self.sh.room_progress.load(Relaxed));
                        ui.add(egui::ProgressBar::new(pr).desired_width(160.0));
                    }
                    _ => {
                        if ui.add_enabled(connected, egui::Button::new(t("room.btn"))).on_hover_text(t("room.hint")).clicked() {
                            self.sh.room_req.store(true, Relaxed);
                            self.sh.room_state.store(1, Relaxed);
                        }
                        if !connected {
                            ui.label(RichText::new(t("room.need")).small().color(MUTE));
                        }
                    }
                }
            });
            if state == 3 {
                ui.colored_label(WARN, t("room.fail"));
            }
            if state == 2 {
                let result = *self.sh.room_result.lock();
                if let Some(r) = result {
                    ui.label(RichText::new(tf("room.result", &[&format!("{:.0}", r.noise_db), &format!("{:.0}", r.speech_db)])).small().color(MUTE));
                    let sg = |v: f32| format!("{:+.0}", v);
                    ui.label(tf("room.sugg", &[&format!("{:.0}", r.thr), &sg(r.low), &sg(r.mud), &sg(r.pres), &sg(r.air)]));
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(RichText::new(t("room.apply")).color(ON_SIG)).fill(SIG)).clicked() {
                            self.p.gate_on = true;
                            self.p.thr = r.thr;
                            self.p.low = r.low;
                            self.p.mud = r.mud;
                            self.p.pres = r.pres;
                            self.p.air = r.air;
                            self.sh.room_state.store(0, Relaxed);
                        }
                        if ui.button(t("room.dismiss")).clicked() {
                            self.sh.room_state.store(0, Relaxed);
                        }
                    });
                    ui.label(RichText::new(t("room.note")).small().color(MUTE));
                }
            }
        });
        ui.add_space(10.0);
    }

    /// Đăng ký Ctrl+Alt+1..9 cho chín âm đầu tiên (hoặc gỡ nếu tắt).
    fn sync_sound_hotkeys(&mut self) {
        if let Some(m) = &self.hk_mgr {
            for hk in self.sound_keys.drain(..) {
                let _ = m.unregister(hk);
            }
        }
        self.sh.sound_hk.lock().clear();
        let (sounds, on) = {
            let b = self.sh.board.lock();
            (b.sounds.iter().take(9).map(|s| s.id).collect::<Vec<u32>>(), b.hotkeys)
        };
        if !on {
            return;
        }
        let codes = [Code::Digit1, Code::Digit2, Code::Digit3, Code::Digit4, Code::Digit5, Code::Digit6, Code::Digit7, Code::Digit8, Code::Digit9];
        if let Some(m) = &self.hk_mgr {
            for (id, code) in sounds.into_iter().zip(codes) {
                let hk = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), code);
                if m.register(hk).is_ok() {
                    self.sh.sound_hk.lock().insert(hk.id(), id);
                    self.sound_keys.push(hk);
                }
            }
        }
    }

    /// Tab Soundboard: thêm âm thanh (file, kéo thả, link), lưới nút phát, âm lượng, hạ giọng, phím tắt.
    fn board_panel(&mut self, ui: &mut Ui) {
        ui.set_max_width(900.0);
        // kéo thả file vào cửa sổ
        let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            soundboard::add_paths_async(self.sh.clone(), dropped);
        }
        ui.label(RichText::new(t("board.title")).size(20.0).strong());
        ui.label(RichText::new(t("board.lead")).small().color(MUTE));
        ui.add_space(10.0);

        let busy = *self.sh.board_busy.lock();
        ui.horizontal_wrapped(|ui| {
            if ui.button(t("board.add.file")).clicked() {
                soundboard::pick_files(self.sh.clone());
            }
            ui.add(egui::TextEdit::singleline(&mut self.board_url).hint_text(t("board.url.hint")).desired_width(340.0));
            let can = !busy && !self.board_url.trim().is_empty();
            if ui.add_enabled(can, egui::Button::new(t("board.add.url"))).clicked() {
                soundboard::add_url_async(self.sh.clone(), self.board_url.trim().to_string());
                self.board_url.clear();
            }
            if busy {
                ui.label(RichText::new(t("board.downloading")).small().color(SIG));
            }
        });
        ui.label(RichText::new(t("board.drop")).small().color(MUTE));
        if let Some(e) = self.sh.board_msg.lock().clone() {
            ui.colored_label(WARN, e);
        }
        ui.add_space(8.0);

        // âm lượng, hạ giọng, phím tắt, dừng
        let (mut volume, mut duck, mut hotkeys) = {
            let b = self.sh.board.lock();
            (b.volume, b.duck, b.hotkeys)
        };
        let mut changed = false;
        let mut done = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(t("board.volume"));
            let r = ui.add(egui::Slider::new(&mut volume, 0.0..=1.5).show_value(false));
            changed |= r.changed();
            done |= r.drag_stopped();
            if ui.button(t("board.stop")).clicked() {
                soundboard::stop_all(&self.sh);
            }
        });
        changed |= ui.checkbox(&mut duck, t("board.duck")).changed();
        done |= changed && !ui.ctx().input(|i| i.pointer.any_down());
        let hk_changed = ui.checkbox(&mut hotkeys, t("board.hotkeys")).changed();
        if changed || hk_changed {
            {
                let mut b = self.sh.board.lock();
                b.volume = volume;
                b.duck = duck;
                b.hotkeys = hotkeys;
            }
            if done || hk_changed {
                soundboard::save(&self.sh);
            }
        }
        if !self.p.monitor {
            ui.label(RichText::new(t("board.listen")).small().color(MUTE));
        }
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // lưới nút
        let sounds: Vec<soundboard::Sound> = self.sh.board.lock().sounds.clone();
        if sounds.is_empty() {
            ui.label(RichText::new(t("board.empty")).color(MUTE));
        }
        let mut remove: Option<u32> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for (i, s) in sounds.iter().enumerate() {
                let label = if i < 9 && hotkeys { format!("{}  ·  {}", i + 1, s.name) } else { s.name.clone() };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if ui.add(egui::Button::new(RichText::new(label).size(15.0)).min_size(vec2(130.0, 36.0))).clicked() {
                        soundboard::play(&self.sh, s.id);
                    }
                    if ui.small_button("×").on_hover_text(t("board.remove")).clicked() {
                        remove = Some(s.id);
                    }
                });
            }
        });
        if let Some(id) = remove {
            soundboard::remove(&self.sh, id);
        }
    }

    pub fn new(sh: Arc<Shared>, tx: Sender<Cmd>, saved: Saved, background: bool) -> Self {
        let devices = engine::list_output_devices();
        let out_explicit = !saved.out_device.is_empty() && devices.contains(&saved.out_device);
        let out_sel = if out_explicit {
            saved.out_device.clone()
        } else {
            engine::find_virtual(&devices).or_else(engine::default_output_name).unwrap_or_default()
        };
        if !out_sel.is_empty() {
            let _ = tx.send(Cmd::SetOut(out_sel.clone()));
        }
        *sh.out_name.lock() = out_sel.clone();
        sh.set_params(saved.params);
        let p = *sh.params.read();
        if p.monitor {
            let _ = tx.send(Cmd::SetMonitor(true));
        }
        let last_ver = sh.params_ver.load(Relaxed);
        let hotkey = if saved.hotkey.is_empty() { DEFAULT_HOTKEY.to_string() } else { saved.hotkey.clone() };
        let hk_mgr = GlobalHotKeyManager::new().ok();
        let (hk, hk_err) = register_hotkey(&hk_mgr, &hotkey);
        let no_beep = saved.no_beep;
        let conn_mode = if saved.conn_mode == "usb" || saved.conn_mode == "bt" { saved.conn_mode.clone() } else { "wifi".to_string() };
        let lang_pref = if Lang::from_code(&saved.lang).is_some() { saved.lang.clone() } else { "auto".to_string() };
        Self {
            hk_mgr,
            hk,
            hk_err,
            hotkey,
            no_beep,
            lang_pref,
            conn_mode,
            mix_loudest: saved.mix_loudest,
            no_discovery: saved.no_discovery,
            tab: Tab::from_code(&saved.tab),
            update_dismissed: false,
            auto_profile: saved.auto_profile,
            no_update_check: saved.no_update_check,
            new_rule_exe: String::new(),
            new_rule_profile: "game".to_string(),
            board_url: String::new(),
            sound_keys: Vec::new(),
            last_board_ver: 0,
            menu_fonts: false,
            sh,
            tx,
            devices,
            out_sel,
            out_explicit,
            p,
            last_ver,
            saved,
            last_save: Instant::now(),
            last_monitor: p.monitor,
            m: MeterState { d_in: -99.0, d_out: -99.0, peak: -99.0, peak_t: 0.0 },
            qr_url: String::new(),
            qr: None,
            autostart: crate::win::autostart_enabled(),
            start_hidden: background,
            frames: 0,
            hook_installed: false,
            new_profile: String::new(),
            saving_profile: false,
            focus_name: false,
            spec_smooth: [-100.0; BANDS],
            note_dismissed: false,
            diag_at: None,
            installed: crate::win::is_installed(),
            install_msg: Arc::new(parking_lot::Mutex::new(String::new())),
        }
    }

    /// Lưu cấu hình hiện tại thành hồ sơ riêng (trùng tên thì ghi đè).
    fn save_profile(&mut self) {
        let name: String = self.new_profile.trim().chars().take(24).collect();
        if name.is_empty() {
            return;
        }
        let params = Params { mute: false, monitor: false, ..self.p };
        {
            let mut profs = self.sh.profiles.write();
            if let Some(existing) = profs.iter_mut().find(|p| p.name == name) {
                existing.params = params;
            } else if profs.len() < 12 {
                profs.push(Profile { name, params });
            }
        }
        self.sh.params_ver.fetch_add(1, Relaxed); // để điện thoại nhận danh sách hồ sơ mới
        self.saving_profile = false;
        self.persist(true);
    }

    fn delete_profile(&mut self, name: &str) {
        self.sh.profiles.write().retain(|p| p.name != name);
        self.sh.params_ver.fetch_add(1, Relaxed);
        self.persist(true);
    }

    fn persist(&mut self, force: bool) {
        let cur = Saved {
            params: self.p,
            out_device: if self.out_explicit { self.out_sel.clone() } else { String::new() },
            token: self.sh.token.read().clone(),
            hotkey: self.hotkey.clone(),
            no_beep: self.no_beep,
            conn_mode: self.conn_mode.clone(),
            mix_loudest: self.mix_loudest,
            no_discovery: self.no_discovery,
            tab: self.tab.code().to_string(),
            auto_profile: self.auto_profile,
            app_rules: self.sh.app_rules.lock().clone(),
            no_update_check: self.no_update_check,
            lang: if Lang::from_code(&self.lang_pref).is_some() { self.lang_pref.clone() } else { String::new() },
            profiles: self.sh.profiles.read().clone(),
            note: String::new(),
        };
        let s = &self.saved;
        let dirty = cur.params != s.params
            || cur.out_device != s.out_device
            || cur.token != s.token
            || cur.hotkey != s.hotkey
            || cur.no_beep != s.no_beep
            || cur.lang != s.lang
            || cur.profiles != s.profiles;
        if dirty && (force || self.last_save.elapsed() > Duration::from_secs(1)) {
            settings::save(&cur);
            self.saved = cur;
            self.last_save = Instant::now();
        }
    }

    /// Ô chọn ngôn ngữ (góc trên bên phải cột trái). "auto" = theo Windows.
    /// Thanh trên cùng: tên + phiên bản, các tab, trạng thái kết nối và chọn ngôn ngữ.
    fn tab_bar(&mut self, ui: &mut Ui) {
        let upd = self.sh.update.lock().clone();
        if let Some((ver, url)) = upd.filter(|_| !self.update_dismissed) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tf("upd.available", &[&ver])).color(SIG));
                if ui.button(t("upd.get")).clicked() {
                    {
                        use std::os::windows::process::CommandExt;
                        let _ = std::process::Command::new("cmd").args(["/c", "start", "", &url]).creation_flags(0x0800_0000).spawn();
                    }
                }
                if ui.small_button("×").clicked() {
                    self.update_dismissed = true;
                }
            });
            ui.add_space(4.0);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("LarmaMic v{}", env!("CARGO_PKG_VERSION"))).size(14.0).color(MUTE));
            ui.add_space(14.0);
            for tab in Tab::ALL {
                let label = RichText::new(t(tab.key())).size(15.0);
                if ui.selectable_label(self.tab == tab, label).clicked() && self.tab != tab {
                    self.tab = tab;
                    self.persist(true);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.language_picker(ui);
                let n = self.sh.sources.lock().len();
                let (txt, col) = match n {
                    0 => (t("status.waiting"), MUTE),
                    1 => (t("status.connected"), SIG),
                    n => (tf("status.many", &[&n.to_string()]), SIG),
                };
                ui.label(RichText::new(txt).size(13.0).color(col));
                let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 5.0, if n > 0 { SIG } else { GHOST });
            });
        });
    }

    fn language_picker(&mut self, ui: &mut Ui) {
        let shown = match Lang::from_code(&self.lang_pref) {
            Some(l) => l.native_name().to_string(),
            None => t("lang.auto.short"),
        };
        let mut chosen: Option<String> = None;
        let resp = egui::ComboBox::from_id_salt("lang")
            .width(130.0)
            .height(520.0)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(Lang::from_code(&self.lang_pref).is_none(), t("lang.auto")).clicked() {
                    chosen = Some("auto".to_string());
                }
                for l in Lang::ALL {
                    if ui.selectable_label(self.lang_pref == l.code(), l.native_name()).clicked() {
                        chosen = Some(l.code().to_string());
                    }
                }
            });
        let open = resp.inner.is_some();
        if open != self.menu_fonts {
            self.menu_fonts = open;
            apply_fonts(ui.ctx(), i18n::current(), open);
            ui.ctx().request_repaint();
        }
        if let Some(c) = chosen {
            self.set_language(ui.ctx(), &c);
        }
    }

    fn set_language(&mut self, ctx: &egui::Context, pref: &str) {
        self.lang_pref = pref.to_string();
        let l = i18n::resolve(pref);
        i18n::set(l);
        apply_fonts(ctx, l, false);
        self.menu_fonts = false;
        crate::win::relabel_tray();
        self.sh.params_ver.fetch_add(1, Relaxed); // để trang điện thoại nhận ngôn ngữ mới
        self.persist(true);
    }

    // ---------------- cột trái: kết nối ----------------
    fn connection_panel(&mut self, ui: &mut Ui) {
        ui.set_max_width(620.0);
        let sources: Vec<Arc<engine::Source>> = self.sh.sources.lock().clone();
        let connected = !sources.is_empty();
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(14.0, 30.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 6.5, if connected { SIG } else { GHOST });
            let txt = match sources.len() {
                0 => t("status.waiting"),
                1 => t("status.connected"),
                n => tf("status.many", &[&n.to_string()]),
            };
            ui.label(RichText::new(txt).size(22.0).strong().color(if connected { SIG } else { INK }));
        });
        // điện thoại trong mạng xin ghép nối: hỏi người dùng
        let pending: Vec<Arc<engine::PairReq>> = self.sh.pending.lock().clone();
        for r in &pending {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tf("pair.req", &[&r.device])).color(SIG));
                if ui.add(egui::Button::new(RichText::new(t("pair.allow")).color(ON_SIG)).fill(SIG)).clicked() {
                    r.decision.store(1, Relaxed);
                }
                if ui.button(t("pair.deny")).clicked() {
                    r.decision.store(2, Relaxed);
                }
            });
        }
        self.sh.mix_loudest.store(self.mix_loudest, Relaxed);
        if sources.len() > 1 {
            ui.horizontal(|ui| {
                for (loud, key) in [(false, "mix.all"), (true, "mix.loudest")] {
                    if ui.selectable_label(self.mix_loudest == loud, t(key)).on_hover_text(t("mix.hint")).clicked() && self.mix_loudest != loud {
                        self.mix_loudest = loud;
                        self.persist(true);
                    }
                }
            });
        }
        // Danh sách các điện thoại đang nối: tên, mức tiếng, tắt tiếng riêng, ngắt
        for s in &sources {
            ui.horizontal(|ui| {
                let name = s.name.lock().clone();
                ui.label(RichText::new(name).size(14.0));
                let peak = f32::from_bits(s.peak.load(Relaxed));
                let level = ((20.0 * peak.max(1e-5).log10() + 60.0) / 60.0).clamp(0.0, 1.0);
                let (rect, _) = ui.allocate_exact_size(vec2(50.0, 8.0), Sense::hover());
                ui.painter().rect_filled(rect, 3.0, OFF);
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, vec2(rect.width() * level, rect.height())), 3.0, SIG);
                let muted = s.muted.load(Relaxed);
                if ui.selectable_label(muted, t("chip.mute")).clicked() {
                    s.muted.store(!muted, Relaxed);
                }
                // độ lợi riêng của máy này
                let mut db = 20.0 * f32::from_bits(s.gain.load(Relaxed)).max(0.001).log10();
                if ui.add(egui::DragValue::new(&mut db).range(-12.0..=12.0).speed(0.1).fixed_decimals(0).suffix(" dB")).changed() {
                    s.gain.store(10f32.powf(db / 20.0).to_bits(), Relaxed);
                }
                if ui.small_button("×").on_hover_text(t("src.kick")).clicked() {
                    s.kicked.store(true, Relaxed);
                }
            });
        }
        if let Some(e) = self.sh.server_err.lock().clone() {
            ui.colored_label(WARN, e);
        }
        // file cài đặt từng bị hỏng: báo cho người dùng biết thay vì lặng lẽ đổi cấu hình
        let note = self.sh.settings_note.lock().clone(); // lưu dưới dạng khóa dịch
        if !note.is_empty() && !self.note_dismissed {
            ui.colored_label(WARN, t(&note));
            if ui.small_button(t("btn.understood")).clicked() {
                self.note_dismissed = true;
            }
        }

        let base = self.sh.url.lock().clone();
        ui.add_space(8.0);
        // Cách điện thoại nối tới PC: Wi-Fi, cáp USB (chia sẻ mạng USB) hoặc Bluetooth (chia sẻ mạng Bluetooth)
        ui.horizontal(|ui| {
            for (m, label) in [("wifi", "Wi-Fi".to_string()), ("usb", t("mode.usb")), ("bt", "Bluetooth".to_string())] {
                if ui.selectable_label(self.conn_mode == m, label).clicked() && self.conn_mode != m {
                    self.conn_mode = m.to_string();
                    self.persist(true);
                }
            }
        });
        self.sh.usb_auto.store(self.conn_mode == "usb", Relaxed);
        let hint_key = match self.conn_mode.as_str() {
            "usb" => "mode.usb.hint",
            "bt" => "mode.bt.hint",
            _ => "mode.wifi.hint",
        };
        ui.label(RichText::new(t(hint_key)).small().color(MUTE));
        if self.conn_mode == "usb" {
            // Cách tự nhận qua cáp: gỡ lỗi USB + adb (xem adb.rs)
            match self.sh.adb_state.load(Relaxed) {
                crate::adb::READY => {
                    ui.label(RichText::new(t("adb.ready")).small().color(SIG));
                }
                crate::adb::UNAUTHORIZED => {
                    ui.colored_label(WARN, t("adb.unauth"));
                }
                crate::adb::MISSING => {
                    ui.label(RichText::new(t("adb.missing")).small().color(MUTE));
                    let dl = self.sh.adb_dl.lock().clone();
                    match dl {
                        Some(s) if s.is_empty() => {
                            ui.label(RichText::new(t("adb.getting")).small().color(SIG));
                        }
                        other => {
                            if let Some(e) = other {
                                ui.colored_label(WARN, tf("adb.fail", &[&e]));
                            }
                            if ui.button(t("adb.get")).clicked() {
                                crate::adb::download(self.sh.clone());
                            }
                        }
                    }
                }
                _ => {
                    ui.label(RichText::new(t("adb.wait")).small().color(MUTE));
                }
            }
        }
        ui.add_space(6.0);
        if base.is_empty() {
            ui.label(RichText::new(t("srv.starting")).color(MUTE));
        } else {
            // Mã ghép nối nằm trong địa chỉ (và QR): không có mã đúng thì điện thoại không gửi được tiếng vào PC
            let token = self.sh.token.read().clone();
            let alt = self.sh.alt_hosts.lock().clone();
            // Ở chế độ USB/Bluetooth, địa chỉ của card mạng chia sẻ đứng đầu trong QR (trang web trên iPhone chỉ dùng địa chỉ đầu);
            // "h" liệt kê các địa chỉ còn lại để app Android thử lần lượt
            let (primary, port) = base.trim_start_matches("https://").split_once(':').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
            let tether = self.conn_mode != "wifi";
            let (host, others): (String, Vec<String>) = if tether && !alt.is_empty() {
                let mut o: Vec<String> = alt[1..].to_vec();
                o.push(primary.clone());
                (alt[0].clone(), o)
            } else {
                (primary.clone(), alt.clone())
            };
            let base = format!("https://{host}:{port}");
            if tether && alt.is_empty() {
                ui.colored_label(WARN, t("mode.none"));
            }
            let url = if others.is_empty() { format!("{base}/?k={token}") } else { format!("{base}/?k={token}&h={}", others.join(",")) };
            self.draw_qr(ui, &url);
            ui.add_space(4.0);
            ui.label(RichText::new(t("pair.scan")).size(14.0).color(MUTE));
            ui.horizontal_wrapped(|ui| {
                let shown = format!("{base}/?k={}…", token.chars().take(4).collect::<String>());
                ui.label(RichText::new(shown).monospace().color(SIG));
            });
            ui.horizontal(|ui| {
                if ui.button(t("pair.copy")).clicked() {
                    ui.ctx().copy_text(url.clone());
                }
                if ui.button(t("pair.rotate")).on_hover_text(t("pair.rotate.hint")).clicked()
                {
                    *self.sh.token.write() = settings::new_token();
                    self.sh.kick_all(); // ngắt mọi kết nối cũ
                    self.persist(true);
                }
            });
            ui.add_space(2.0);
            if !alt.is_empty() && !tether {
                ui.label(RichText::new(tf("pair.alt", &[&alt.join(", ")])).small().color(SIG));
            }
            ui.label(RichText::new(t("pair.cert")).small().color(MUTE));
            let mut allow = !self.no_discovery;
            if ui.checkbox(&mut allow, RichText::new(t("disc.toggle")).small()).on_hover_text(t("disc.hint")).changed() {
                self.no_discovery = !allow;
                self.sh.discovery.store(allow, Relaxed);
                self.persist(true);
            }
        }

    }

    /// Tab "Cài đặt": mic ảo (đầu ra), nghe thử, phím tắt, chạy cùng Windows, độ trễ, công cụ.
    fn settings_panel(&mut self, ui: &mut Ui) {
        ui.set_max_width(620.0);
        let connected = self.sh.connected.load(Relaxed);
        ui.label(RichText::new(t("dev.title")).size(18.0).strong());
        let mut changed = false;
        egui::ComboBox::from_id_salt("outdev")
            .width(ui.available_width())
            .selected_text(short(&self.out_sel))
            .show_ui(ui, |ui| {
                for d in &self.devices {
                    if ui.selectable_value(&mut self.out_sel, d.clone(), d).changed() {
                        changed = true;
                    }
                }
            });
        if changed {
            self.out_explicit = true;
            let _ = self.tx.send(Cmd::SetOut(self.out_sel.clone()));
            self.persist(true);
        }
        if ui.button(t("dev.refresh")).clicked() {
            self.devices = engine::list_output_devices();
        }
        match engine::find_virtual(&self.devices) {
            None => {
                ui.add_space(2.0);
                ui.colored_label(WARN, t("dev.nocable"));
                ui.hyperlink_to(t("dev.getcable"), "https://vb-audio.com/Cable/");
            }
            Some(cable) if !engine::is_virtual(&self.out_sel) => {
                // Đang xuất ra loa/tai nghe: Discord, OBS, game sẽ không nghe thấy gì
                ui.add_space(4.0);
                ui.colored_label(WARN, tf("dev.wrong", &[&short(&self.out_sel)]));
                if ui.add(egui::Button::new(RichText::new(t("dev.usecable")).color(ON_SIG)).fill(SIG)).clicked() {
                    self.out_sel = cable.clone();
                    self.out_explicit = false; // quay lại chế độ tự tìm VB-Cable
                    let _ = self.tx.send(Cmd::SetOut(cable));
                    self.persist(true);
                }
            }
            Some(_) => {
                let mic = engine::virt_kind(&self.out_sel).unwrap_or(engine::Virt::Cable).mic_name();
                ui.label(RichText::new(tf("dev.ok", &[mic])).small().color(SIG));
            }
        }
        if let Some(e) = self.sh.out_err.lock().clone() {
            ui.colored_label(WARN, e);
        }

        ui.add_space(10.0);
        switch_row(ui, &t("monitor.title"), &t("monitor.hint"), &mut self.p.monitor);

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(RichText::new(t("hk.title")).size(18.0).strong());
        let mut hk_changed = false;
        egui::ComboBox::from_id_salt("hotkey")
            .width(ui.available_width())
            .selected_text(hotkey_label(&self.hotkey))
            .show_ui(ui, |ui| {
                for (key, label) in HOTKEYS {
                    let text = if key == "off" { t("hotkey.off") } else { label.to_string() };
                    if ui.selectable_value(&mut self.hotkey, key.to_string(), text).changed() {
                        hk_changed = true;
                    }
                }
            });
        if hk_changed {
            if let (Some(m), Some(old)) = (&self.hk_mgr, self.hk.take()) {
                let _ = m.unregister(old);
            }
            let (hk, err) = register_hotkey(&self.hk_mgr, &self.hotkey);
            self.hk = hk;
            self.hk_err = err;
            self.persist(true);
        }
        if let Some(e) = &self.hk_err {
            ui.colored_label(WARN, e);
        } else {
            ui.label(RichText::new(t("hk.hint")).small().color(MUTE));
        }
        let before_beep = self.no_beep;
        let mut beep = !self.no_beep;
        switch_row(ui, &t("beep.title"), &t("beep.hint"), &mut beep);
        self.no_beep = !beep;
        if self.no_beep != before_beep {
            self.sh.beep.store(beep, Relaxed);
            self.persist(true);
        }

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(4.0);
        let before = self.autostart;
        switch_row(ui, &t("autostart.title"), &t("autostart.hint"), &mut self.autostart);
        if self.autostart != before && !crate::win::set_autostart(self.autostart) {
            self.autostart = before; // không ghi được vào Windows thì trả công tắc về như cũ
        }
        ui.label(RichText::new(t("tray.note")).small().color(MUTE));

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(RichText::new(t("lat.title")).size(18.0).strong());
        if connected {
            ui.label(RichText::new(tf("lat.value", &[&self.sh.latency_ms().to_string()])).color(INK));
        } else {
            ui.label(RichText::new(t("lat.waiting")).color(MUTE));
        }
        let rtt = self.sh.rtt_ms.load(Relaxed);
        let mut stats = tf(
            "lat.stats",
            &[
                &engine::stable(&self.sh.buf_shown, f32::from_bits(self.sh.buf_smooth.load(Relaxed))).to_string(),
                &self.sh.under.load(Relaxed).to_string(),
                &self.sh.drops.load(Relaxed).to_string(),
            ],
        );
        if connected && rtt > 0 {
            stats.push_str(&tf("lat.rtt", &[&rtt.to_string()]));
        }
        ui.label(RichText::new(stats).small().color(MUTE));
        ui.label(RichText::new(t("lat.note")).small().color(MUTE));

        // ---- cấu hình theo ứng dụng ----
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(RichText::new(t("app.title")).size(18.0).strong());
        ui.label(RichText::new(t("app.lead")).small().color(MUTE));
        if ui.checkbox(&mut self.auto_profile, t("app.toggle")).changed() {
            self.sh.auto_profile.store(self.auto_profile, Relaxed);
            self.persist(true);
        }
        if let Some(exe) = self.sh.app_active.lock().clone() {
            ui.label(RichText::new(tf("app.active", &[&exe])).small().color(SIG));
        }
        let profile_label = |key: &str, profs: &[String]| -> String {
            match PRESET_NAMES.iter().find(|(k, _)| *k == key) {
                Some((_, l)) => t(l),
                None => profs.iter().find(|n| n.as_str() == key).cloned().unwrap_or_else(|| key.to_string()),
            }
        };
        let profile_names: Vec<String> = self.sh.profiles.read().iter().map(|p| p.name.clone()).collect();
        let rules = self.sh.app_rules.lock().clone();
        let mut remove: Option<usize> = None;
        for (i, r) in rules.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&r.exe).monospace());
                ui.label(RichText::new("→").color(MUTE));
                ui.label(profile_label(&r.profile, &profile_names));
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.sh.app_rules.lock().remove(i);
            self.persist(true);
        }
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_rule_exe).hint_text("Discord.exe").desired_width(150.0));
            egui::ComboBox::from_id_salt("rule_profile")
                .selected_text(profile_label(&self.new_rule_profile, &profile_names))
                .show_ui(ui, |ui| {
                    for (k, l) in PRESET_NAMES.iter() {
                        ui.selectable_value(&mut self.new_rule_profile, k.to_string(), t(l));
                    }
                    for n in &profile_names {
                        ui.selectable_value(&mut self.new_rule_profile, n.clone(), n.clone());
                    }
                });
            if ui.add_enabled(!self.new_rule_exe.trim().is_empty(), egui::Button::new(t("app.add"))).clicked() {
                let exe = self.new_rule_exe.trim().to_string();
                self.sh.app_rules.lock().push(crate::appmatch::AppRule { exe, profile: self.new_rule_profile.clone() });
                self.new_rule_exe.clear();
                self.persist(true);
            }
        });
        ui.label(RichText::new(t("app.hint")).small().color(MUTE));

        // ---- kiểm tra bản mới ----
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(6.0);
        let mut check = !self.no_update_check;
        if ui.checkbox(&mut check, t("upd.toggle")).changed() {
            self.no_update_check = !check;
            self.sh.update_check.store(check, Relaxed);
            self.persist(true);
        }

        // ---- công cụ: chẩn đoán, cài đặt vào máy ----
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(RichText::new(t("tools.title")).size(18.0).strong());
        let copied = self.diag_at.is_some_and(|at| at.elapsed() < Duration::from_secs(3));
        if ui
            .button(if copied { t("diag.copied") } else { t("diag.btn") })
            .on_hover_text(t("diag.hint"))
            .clicked()
        {
            let text = crate::diag::build(&self.sh, &self.devices, &self.out_sel, self.hk_err.as_deref(), &self.hotkey);
            ui.ctx().copy_text(text);
            self.diag_at = Some(Instant::now());
        }
        ui.add_space(4.0);
        if self.installed {
            ui.label(RichText::new(t("inst.done")).small().color(SIG));
            if ui.button(t("inst.uninstall")).clicked() {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).arg("--uninstall").spawn();
                }
            }
        } else {
            if ui.button(t("inst.btn")).on_hover_text(t("inst.hint")).clicked() {
                let msg = self.install_msg.clone();
                *msg.lock() = t("inst.running");
                std::thread::spawn(move || {
                    *msg.lock() = match crate::win::install(false) {
                        Ok(m) => tf("inst.ok", &[&m]),
                        Err(e) => tf("inst.err", &[&e]),
                    };
                });
            }
            let m = self.install_msg.lock().clone();
            if !m.is_empty() {
                ui.label(RichText::new(m).small().color(MUTE));
            }
        }
        ui.add_space(16.0);
    }

    fn draw_qr(&mut self, ui: &mut Ui, url: &str) {
        if self.qr_url != url {
            self.qr = qrcode::QrCode::new(url.as_bytes()).ok().map(|c| {
                (c.width(), c.to_colors().iter().map(|k| *k == qrcode::Color::Dark).collect())
            });
            self.qr_url = url.to_string();
        }
        if let Some((w, mods)) = &self.qr {
            let size = ui.available_width().min(230.0);
            let quiet = 3.0;
            let cell = size / (*w as f32 + 2.0 * quiet);
            let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, 12.0, Color32::WHITE);
            for y in 0..*w {
                for x in 0..*w {
                    if mods[y * w + x] {
                        let min = rect.min + vec2((quiet + x as f32) * cell, (quiet + y as f32) * cell);
                        p.rect_filled(Rect::from_min_size(min, vec2(cell + 0.3, cell + 0.3)), 0.0, BG);
                    }
                }
            }
        }
    }

    // ---------------- cột phải: đồng hồ + bộ chỉnh ----------------
    fn main_panel(&mut self, ui: &mut Ui, dt: f32) {
        // Đồng hồ, cấu hình nhanh, hồ sơ, ghi âm: chỉ ở tab Âm thanh (các tab khác cần chỗ cho nội dung của chúng)
        if self.tab == Tab::Sound {
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("meter.title")).size(18.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(t("meter.hint")).small().color(MUTE));
            });
        });
        meter(ui, &self.sh, &mut self.p, &mut self.m, dt);

        // hàng số đo: vẽ trong một khung cố định để không chiếm hết chiều cao
        let gate = if !self.p.gate_on {
            (t("gate.off"), MUTE)
        } else if self.sh.gate_open.load(Relaxed) {
            (t("gate.open"), SIG)
        } else {
            (t("gate.closed"), WARN)
        };
        let (rr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let pt = ui.painter_at(rr);
        pt.text(rr.left_center(), Align2::LEFT_CENTER, tf("meter.in", &[&fmt_db(self.m.d_in)]), FontId::proportional(17.0), INK);
        pt.text(rr.center(), Align2::CENTER_CENTER, gate.0, FontId::proportional(16.0), gate.1);
        pt.text(rr.right_center(), Align2::RIGHT_CENTER, tf("meter.out", &[&fmt_db(self.m.d_out)]), FontId::proportional(17.0), INK);

        spectrum(ui, &self.sh, &mut self.spec_smooth, dt);

        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            for (key, label_key) in PRESET_NAMES {
                if chip(ui, &t(label_key), matches_preset(&self.p, key), SIG).clicked() {
                    self.p = apply_preset(&self.p, key);
                }
            }
            ui.add_space(14.0);
            if chip(ui, &t("chip.mute"), self.p.mute, WARN).clicked() {
                self.p.mute = !self.p.mute;
            }
        });

        // hồ sơ riêng: lưu cấu hình của bạn, đổi nhanh ở đây hoặc trên điện thoại
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(t("prof.label")).color(MUTE));
            let profs: Vec<Profile> = self.sh.profiles.read().clone();
            for pr in &profs {
                if chip(ui, &pr.name, matches_profile(&self.p, pr), SIG).clicked() {
                    self.p = apply_profile(&self.p, pr);
                }
            }
            if self.saving_profile {
                let r = ui.add(egui::TextEdit::singleline(&mut self.new_profile).desired_width(150.0).hint_text(t("prof.name")));
                if self.focus_name {
                    r.request_focus(); // gõ được ngay, không phải bấm thêm vào ô
                    self.focus_name = false;
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button(t("btn.save")).clicked() || enter {
                    self.save_profile();
                }
                if ui.button(t("btn.cancel")).clicked() {
                    self.saving_profile = false;
                }
            } else {
                if ui.button(t("prof.save")).clicked() {
                    self.saving_profile = true;
                    self.focus_name = true;
                    self.new_profile.clear();
                }
                if let Some(active) = profs.iter().find(|pr| matches_profile(&self.p, pr)) {
                    if ui.button(tf("prof.delete", &[&active.name])).clicked() {
                        let name = active.name.clone();
                        self.delete_profile(&name);
                    }
                }
            }
        });

        // ghi âm giọng đã lọc ra file WAV
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let on = self.sh.rec_on.load(Relaxed);
            if chip(ui, &t(if on { "rec.stop" } else { "rec.start" }), on, WARN).on_hover_text(t("rec.hint")).clicked()
            {
                self.sh.rec_req.store(if on { 2 } else { 1 }, Relaxed);
            }
            if on {
                let secs = self.sh.rec_frames.load(Relaxed) / 100;
                ui.label(RichText::new(format!("{:02}:{:02}", secs / 60, secs % 60)).monospace().color(WARN));
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            } else {
                let path = self.sh.rec_path.lock().clone();
                if !path.is_empty() {
                    let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    ui.label(RichText::new(tf("rec.saved", &[&name])).small().color(MUTE));
                    if ui.small_button(t("rec.open")).clicked() {
                        let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
                    }
                }
            }
            if let Some(e) = self.sh.rec_err.lock().clone() {
                ui.colored_label(WARN, e);
            }
        });

        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            match self.tab {
                Tab::Connect => self.connection_panel(ui),
                Tab::Settings => self.settings_panel(ui),
                Tab::Board => self.board_panel(ui),
                Tab::Sound => {
                    self.room_card(ui);
                    let wide = ui.available_width() > 940.0;
                    let p = &mut self.p;
                    let sh = &self.sh;
                    let connected = sh.connected.load(Relaxed);
                    if wide {
                        ui.columns(3, |c| {
                            section_noise(&mut c[0], p, sh, connected);
                            section_eq(&mut c[1], p);
                            section_volume(&mut c[2], p);
                            section_fx(&mut c[2], p);
                        });
                    } else {
                        section_noise(ui, p, sh, connected);
                        section_eq(ui, p);
                        section_volume(ui, p);
                        section_fx(ui, p);
                    }
                }
            }
            ui.add_space(16.0);
        });
    }
}

// ---------------- các khối bộ chỉnh ----------------
fn heading(ui: &mut Ui, title: &str, lead: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).size(20.0).strong());
    if !lead.is_empty() {
        ui.label(RichText::new(lead).small().color(MUTE));
    }
    ui.add_space(2.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, LINE);
}

/// Nút "Tự đặt ngưỡng": bạn im lặng 3 giây, app đo nền ồn rồi đặt ngưỡng cổng.
fn calibrate_row(ui: &mut Ui, sh: &Shared, connected: bool, thr: f32) {
    ui.add_space(6.0);
    match sh.cal_state.load(Relaxed) {
        1 => {
            ui.label(RichText::new(t("cal.measuring")).color(SIG));
            ui.add(egui::ProgressBar::new(load_f(&sh.cal_progress)).desired_height(8.0).fill(SIG));
            ui.ctx().request_repaint();
        }
        2 => {
            let noise = load_f(&sh.cal_noise);
            ui.label(RichText::new(tf("cal.done", &[fmt_db(noise).trim(), fmt_db(thr).trim()])).color(SIG));
            if noise > -35.0 {
                ui.label(RichText::new(t("cal.noisy")).small().color(WARN));
            }
        }
        3 => {
            ui.colored_label(WARN, t("cal.fail"));
        }
        _ => {
            let b = egui::Button::new(t("cal.btn")).min_size(vec2(ui.available_width(), 34.0));
            let r = ui.add_enabled(connected, b).on_hover_text(t("cal.btn.hint"));
            if r.clicked() {
                sh.cal_req.store(true, Relaxed);
            }
            if !connected {
                ui.label(RichText::new(t("cal.need")).small().color(MUTE));
            }
        }
    }
}

fn section_noise(ui: &mut Ui, p: &mut Params, sh: &Shared, connected: bool) {
    heading(ui, &t("sec.noise"), &t("sec.noise.lead"));
    switch_row(ui, &t("ctl.ai"), "", &mut p.ai);
    ctl(ui, &t("ctl.ai_mix"), &t("ctl.ai_mix.hint"), &mut p.ai_mix, 0.0..=100.0, "%");
    ui.add_space(6.0);
    switch_row(ui, &t("ctl.gate"), &t("ctl.gate.hint"), &mut p.gate_on);
    ctl(ui, &t("ctl.thr"), &t("ctl.thr.hint"), &mut p.thr, -72.0..=-20.0, " dB");
    calibrate_row(ui, sh, connected, p.thr);
    ctl(ui, &t("ctl.depth"), &t("ctl.depth.hint"), &mut p.depth, 6.0..=80.0, " dB");
    ctl(ui, &t("ctl.hp"), &t("ctl.hp.hint"), &mut p.hp, 40.0..=250.0, " Hz");
    ui.add_space(6.0);
    switch_row(ui, &t("ctl.aec"), &t("ctl.aec.hint"), &mut p.aec);
    if p.aec {
        match sh.aec_err.lock().clone() {
            Some(e) if e == "virtual" => {
                ui.colored_label(WARN, t("aec.err.virtual"));
            }
            Some(e) => {
                ui.colored_label(WARN, tf("aec.err", &[&e]));
            }
            None => {}
        }
    }
}

fn section_eq(ui: &mut Ui, p: &mut Params) {
    heading(ui, &t("sec.eq"), &t("sec.eq.lead"));
    eq_curve(ui, p);
    ctl(ui, &t("ctl.low"), &t("ctl.low.hint"), &mut p.low, -12.0..=12.0, " dB");
    ctl(ui, &t("ctl.mud"), &t("ctl.mud.hint"), &mut p.mud, -12.0..=12.0, " dB");
    ctl(ui, &t("ctl.pres"), &t("ctl.pres.hint"), &mut p.pres, -12.0..=12.0, " dB");
    ctl(ui, &t("ctl.air"), &t("ctl.air.hint"), &mut p.air, -12.0..=12.0, " dB");
}

/// Giọng đổi sẵn: ghép từ các hiệu ứng có sẵn (đổi tông, vang, robot).
const VOICES: [(&str, f32, f32, bool); 5] = [
    ("fx.none", 0.0, 0.0, false),
    ("fx.deep", -5.0, 0.0, false),
    ("fx.small", 7.0, 0.0, false),
    ("fx.robot", 0.0, 0.0, true),
    ("fx.ghost", -2.0, 65.0, false),
];

fn section_fx(ui: &mut Ui, p: &mut Params) {
    heading(ui, &t("sec.fx"), &t("sec.fx.lead"));
    ui.horizontal_wrapped(|ui| {
        for (key, pitch, reverb, robot) in VOICES {
            let on = p.fx_pitch == pitch && p.fx_reverb == reverb && p.fx_robot == robot;
            if ui.selectable_label(on, t(key)).clicked() {
                p.fx_pitch = pitch;
                p.fx_reverb = reverb;
                p.fx_robot = robot;
            }
        }
    });
    ui.add_space(4.0);
    ctl(ui, &t("ctl.pitch"), &t("ctl.pitch.hint"), &mut p.fx_pitch, -12.0..=12.0, &t("ctl.pitch.unit"));
    if p.fx_pitch.abs() > 0.4 {
        ui.label(RichText::new(t("fx.latency")).small().color(MUTE));
    }
    ctl(ui, &t("ctl.reverb"), "", &mut p.fx_reverb, 0.0..=100.0, "%");
    ui.add_space(6.0);
    switch_row(ui, &t("ctl.robot"), "", &mut p.fx_robot);
}

fn section_volume(ui: &mut Ui, p: &mut Params) {
    heading(ui, &t("sec.vol"), &t("sec.vol.lead"));
    switch_row(ui, &t("ctl.agc"), &t("ctl.agc.hint"), &mut p.agc);
    ctl(ui, &t("ctl.comp"), &t("ctl.comp.hint"), &mut p.comp, 0.0..=100.0, "%");
    ctl(ui, &t("ctl.gain"), "", &mut p.gain, -12.0..=24.0, " dB");
    ctl(ui, &t("ctl.buf"), &t("ctl.buf.hint"), &mut p.buf_ms, 30.0..=300.0, " ms");
}

/// Một thanh chỉnh: nhãn + giá trị ở trên, thanh trượt rộng hết cột, gợi ý nhỏ ở dưới.
fn ctl(ui: &mut Ui, label: &str, hint: &str, v: &mut f32, r: RangeInclusive<f32>, unit: &str) {
    ui.add_space(8.0);
    let bipolar = *r.start() < 0.0 && *r.end() > 0.0;
    let shown = {
        let n = v.round() as i32;
        if n < 0 {
            format!("−{}{}", -n, unit)
        } else if bipolar && n > 0 {
            format!("+{}{}", n, unit)
        } else {
            format!("{}{}", n, unit)
        }
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(16.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(shown).monospace().color(SIG));
        });
    });
    ui.spacing_mut().slider_width = ui.available_width() - 4.0;
    ui.add(Slider::new(v, r).show_value(false).step_by(1.0).trailing_fill(!bipolar));
    if !hint.is_empty() {
        ui.label(RichText::new(hint).small().color(MUTE));
    }
}

fn switch_row(ui: &mut Ui, label: &str, hint: &str, on: &mut bool) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // Công tắc đặt trước (sát phải), phần chữ lấy chỗ còn lại bên trái và tự xuống dòng
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            toggle(ui, on);
            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                ui.label(RichText::new(label).size(16.0));
                if !hint.is_empty() {
                    ui.label(RichText::new(hint).small().color(MUTE));
                }
            });
        });
    });
}

fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(48.0, 28.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let bg = GHOST.lerp_to_gamma(SIG, t);
    ui.painter().rect_filled(rect, 14.0, bg);
    let x = egui::lerp((rect.left() + 14.0)..=(rect.right() - 14.0), t);
    ui.painter().circle_filled(pos2(x, rect.center().y), 10.0, Color32::WHITE);
    resp
}

fn chip(ui: &mut Ui, text: &str, active: bool, accent: Color32) -> Response {
    let (fg, stroke, fill) = if active {
        (accent, Stroke::new(1.5_f32, accent), accent.gamma_multiply(0.14))
    } else {
        (INK, Stroke::new(1.5_f32, LINE), Color32::TRANSPARENT)
    };
    ui.add(
        egui::Button::new(RichText::new(text).color(fg).size(16.0))
            .stroke(stroke)
            .fill(fill)
            .corner_radius(10)
            .min_size(vec2(100.0, 38.0)),
    )
}

/// Biểu đồ phổ âm đầu ra (40 Hz – 16 kHz). Luồng DSP chỉ tính phổ khi giao diện đang xem.
fn spectrum(ui: &mut Ui, sh: &Shared, smooth: &mut [f32; BANDS], dt: f32) {
    let h = 84.0;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    sh.spec_until.store(engine::now_ms() + 1000, Relaxed);
    let cur = *sh.spectrum.lock();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, OFF);
    let bw = w / BANDS as f32;
    for b in 0..BANDS {
        // lên nhanh, xuống chậm cho dễ nhìn
        smooth[b] = if cur[b] > smooth[b] { cur[b] } else { (smooth[b] - 45.0 * dt).max(cur[b]) };
        let lvl = ((smooth[b] + 80.0) / 80.0).clamp(0.0, 1.0); // -80..0 dBFS
        if lvl <= 0.01 {
            continue;
        }
        let bar_h = lvl * (h - 8.0);
        let r = Rect::from_min_max(
            pos2(rect.min.x + b as f32 * bw + 1.5, rect.max.y - 4.0 - bar_h),
            pos2(rect.min.x + (b as f32 + 1.0) * bw - 1.5, rect.max.y - 4.0),
        );
        painter.rect_filled(r, 2.0, SIG.gamma_multiply(0.35 + 0.65 * lvl));
    }
    painter.text(rect.left_top() + vec2(8.0, 4.0), Align2::LEFT_TOP, t("spec.title"), FontId::proportional(12.0), MUTE);
    for (f, label) in [(100.0f32, "100 Hz"), (1000.0, "1 kHz"), (10_000.0, "10 kHz")] {
        let t = ((f / 40.0).ln() / (16_000.0f32 / 40.0).ln()).clamp(0.0, 1.0);
        painter.text(pos2(rect.min.x + t * w, rect.min.y + 4.0), Align2::CENTER_TOP, label, FontId::proportional(11.0), MUTE);
    }
}

/// Đường cong EQ tổng (cắt ù + 4 băng) theo thông số hiện tại, tính từ bộ lọc thật.
fn eq_curve(ui: &mut Ui, p: &Params) {
    ui.add_space(8.0);
    let w = ui.available_width();
    let h = 112.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, OFF);
    let (lo, hi) = (20.0f32.ln(), 20_000.0f32.ln());
    let x_of = |f: f32| rect.min.x + ((f.ln() - lo) / (hi - lo)) * w;
    let y_of = |db: f32| rect.center().y - (db.clamp(-15.0, 15.0) / 15.0) * (h / 2.0 - 8.0);
    for db in [-12.0f32, -6.0, 0.0, 6.0, 12.0] {
        let c = if db == 0.0 { GHOST } else { LINE };
        painter.line_segment([pos2(rect.min.x, y_of(db)), pos2(rect.max.x, y_of(db))], Stroke::new(1.0_f32, c));
    }
    for (f, label) in [(100.0f32, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
        painter.line_segment([pos2(x_of(f), rect.min.y), pos2(x_of(f), rect.max.y)], Stroke::new(1.0_f32, LINE));
        painter.text(pos2(x_of(f) + 3.0, rect.max.y - 2.0), Align2::LEFT_BOTTOM, label, FontId::proportional(11.0), MUTE);
    }
    let pts: Vec<egui::Pos2> = (0..=140)
        .map(|i| {
            let f = (lo + (hi - lo) * i as f32 / 140.0).exp();
            pos2(x_of(f), y_of(eq_response_db(p, f)))
        })
        .collect();
    painter.add(Shape::line(pts, Stroke::new(2.0_f32, SIG)));
    for (f, db) in [(120.0f32, p.low), (280.0, p.mud), (3200.0, p.pres), (9000.0, p.air)] {
        painter.circle_filled(pos2(x_of(f), y_of(eq_response_db(p, f))), 3.5, Color32::WHITE);
        let _ = db;
    }
    painter.text(rect.left_top() + vec2(8.0, 4.0), Align2::LEFT_TOP, t("eq.title"), FontId::proportional(12.0), MUTE);
}

/// Rút gọn tên thiết bị dài cho vừa ô chọn.
fn short(name: &str) -> String {
    if name.is_empty() {
        return t("dev.none");
    }
    let n = 30;
    if name.chars().count() <= n {
        name.to_string()
    } else {
        format!("{}…", name.chars().take(n - 1).collect::<String>())
    }
}

fn fmt_db(v: f32) -> String {
    if v <= -90.0 {
        "  −∞".to_string()
    } else {
        format!("{:>3.0} dB", v).replace('-', "−")
    }
}

fn meter(ui: &mut Ui, sh: &Shared, p: &mut Params, m: &mut MeterState, dt: f32) {
    const MIN: f32 = -72.0;
    const SEG: usize = 56;
    let h = 150.0;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
    let resp = resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);

    m.d_in = load_f(&sh.in_db).max(m.d_in - 40.0 * dt);
    m.d_out = load_f(&sh.out_db).max(m.d_out - 48.0 * dt);
    if m.d_out >= m.peak {
        m.peak = m.d_out;
        m.peak_t = 0.0;
    } else {
        m.peak_t += dt;
        if m.peak_t > 0.9 {
            m.peak = m.d_out.max(m.peak - 30.0 * dt);
        }
    }

    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let db = MIN + (pos.x - rect.min.x) / w * -MIN;
            p.thr = db.clamp(-72.0, -20.0).round();
        }
    }

    let painter = ui.painter_at(rect.expand(2.0));
    let top = rect.min.y + 34.0;
    let bh = h - 34.0 - 26.0;
    let x_of = |db: f32| rect.min.x + ((db.clamp(MIN, 0.0) - MIN) / -MIN) * w;
    let sw = w / SEG as f32;
    let step = -MIN / SEG as f32;
    for i in 0..SEG {
        let db = MIN + (i as f32 + 1.0) * step;
        let col = if db <= m.d_out {
            if db > -3.0 { WARN } else { SIG }
        } else if db <= m.d_in {
            GHOST
        } else {
            OFF
        };
        let r = Rect::from_min_size(pos2(rect.min.x + i as f32 * sw + 1.5, top), vec2((sw - 3.0).max(1.0), bh));
        painter.rect_filled(r, 3.0, col);
    }
    if m.peak > MIN {
        let x = x_of(m.peak).clamp(rect.min.x, rect.max.x - 3.0);
        let col = if m.peak > -3.0 { WARN } else { INK };
        painter.rect_filled(Rect::from_min_size(pos2(x, top - 5.0), vec2(3.0, bh + 10.0)), 1.0, col);
    }
    // vạch ngưỡng cổng
    let tx = x_of(p.thr);
    painter.rect_filled(Rect::from_min_size(pos2(tx - 1.0, top - 10.0), vec2(2.0, bh + 20.0)), 0.0, Color32::WHITE);
    painter.add(Shape::convex_polygon(
        vec![pos2(tx - 7.0, top - 22.0), pos2(tx + 7.0, top - 22.0), pos2(tx, top - 10.0)],
        Color32::WHITE,
        Stroke::NONE,
    ));
    let label = tf("meter.thr", &[fmt_db(p.thr).trim()]);
    let (anchor, lx) = if tx > rect.max.x - 120.0 { (Align2::RIGHT_BOTTOM, tx - 10.0) } else { (Align2::LEFT_BOTTOM, tx + 10.0) };
    painter.text(pos2(lx, top - 12.0), anchor, label, FontId::proportional(13.0), INK);
    // thang dB
    for d in [-72.0f32, -60.0, -48.0, -36.0, -24.0, -12.0, 0.0] {
        let x = x_of(d);
        let a = if d == 0.0 {
            Align2::RIGHT_BOTTOM
        } else if d == -72.0 {
            Align2::LEFT_BOTTOM
        } else {
            Align2::CENTER_BOTTOM
        };
        painter.text(pos2(x, rect.max.y - 2.0), a, format!("{d:.0}").replace('-', "−"), FontId::proportional(13.0), MUTE);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // QUAN TRỌNG: khi ẩn cửa sổ thì phải `return` ngay, không hẹn vẽ lại. Cửa sổ ẩn không bao giờ
        // nhận lệnh vẽ, nên nếu còn hẹn thì vòng lặp sự kiện quay không nghỉ và ngốn trọn một nhân CPU.

        // Khởi động ẩn (--background): eframe tự hiện cửa sổ sau khung đầu, nên ẩn lại sau vài khung
        if self.start_hidden {
            self.frames += 1;
            if self.frames >= 3 {
                self.start_hidden = false;
                crate::win::hide_main();
                return;
            }
        }

        // Chặn nút đóng ở tầng Windows (lần đầu): xem win::install_close_hook
        if !self.hook_installed {
            let c = ctx.clone();
            self.hook_installed = crate::win::install_close_hook(move || c.request_repaint());
        }

        // Bấm nút đóng: không thoát, chỉ ẩn vào khay để mic tiếp tục chạy nền
        if crate::win::take_hide_request() {
            self.persist(true);
            crate::win::hide_main();
            return;
        }
        // Phương án dự phòng nếu không chặn được ở tầng Windows (cách này tốn CPU khi ẩn)
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.persist(true);
            crate::win::hide_main();
            return;
        }

        // Chỉ vẽ lại khi cần, để không tốn CPU/GPU/điện:
        //  - cửa sổ ẩn hoặc thu nhỏ: không hẹn vẽ (sự kiện khay/phím tắt sẽ đánh thức lại khi cần)
        //  - đang có tiếng hoặc đang đo: 30 khung/giây (15 nếu không phải cửa sổ đang chọn)
        //  - rảnh, chưa có điện thoại: vài lần mỗi giây
        if self.start_hidden || crate::win::is_visible() {
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
            let active = self.sh.connected.load(Relaxed)
                || self.m.d_in > -80.0
                || self.m.d_out > -80.0
                || self.sh.cal_state.load(Relaxed) != 0;
            // Rảnh thì vẽ thưa (1 lần/giây): mọi thay đổi quan trọng đã có luồng khác đánh thức (Shared::wake_ui)
            let ms = match (active, focused) {
                (true, true) => 33,
                (true, false) => 66,
                (false, _) => 1000,
            };
            ctx.request_repaint_after(Duration::from_millis(if self.start_hidden { 50 } else { ms }));
        }
        let dt = ctx.input(|i| i.stable_dt).min(0.1);

        if *self.sh.out_name.lock() != self.out_sel {
            *self.sh.out_name.lock() = self.out_sel.clone();
        }

        // thông số vừa đổi từ điện thoại: nhận về trước khi vẽ
        let ver = self.sh.params_ver.load(Relaxed);
        if ver != self.last_ver {
            self.p = *self.sh.params.read();
            self.last_ver = ver;
        }

        // đồng bộ phím tắt của soundboard khi danh sách đổi
        let bv = self.sh.board_ver.load(Relaxed);
        if bv != self.last_board_ver {
            self.last_board_ver = bv;
            self.sync_sound_hotkeys();
        }
        egui::TopBottomPanel::top("tabs")
            .frame(egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::symmetric(18, 10)))
            .show(ctx, |ui| self.tab_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(BG).inner_margin(22.0))
            .show(ctx, |ui| self.main_panel(ui, dt));

        // thay đổi từ app PC: đẩy sang bộ xử lý và điện thoại
        if *self.sh.params.read() != self.p {
            self.sh.set_params(self.p);
            self.last_ver = self.sh.params_ver.load(Relaxed);
        }
        if self.p.monitor != self.last_monitor {
            self.last_monitor = self.p.monitor;
            let _ = self.tx.send(Cmd::SetMonitor(self.p.monitor));
        }
        self.persist(false);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist(true);
    }
}
