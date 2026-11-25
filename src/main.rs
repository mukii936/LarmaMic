#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diag;
mod dsp;
mod engine;
mod i18n;
mod icon;
mod server;
mod settings;
mod ui;
mod wav;
mod win;

use eframe::egui;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);

    // Cài đặt / gỡ cài đặt chạy riêng, không mở cửa sổ (vẫn dùng ngôn ngữ đã chọn)
    if has("--uninstall") || has("--install") {
        i18n::set(i18n::resolve(&settings::load().lang));
    }
    if has("--uninstall") {
        win::cli_uninstall(has("--test"), has("--quiet"));
        return Ok(());
    }
    if has("--install") {
        win::cli_install(has("--test"), has("--quiet"));
        return Ok(());
    }

    // Chỉ cho một bản chạy. Mở lần hai thì chỉ gọi cửa sổ của bản đang chạy lên.
    // LARMA_LOCK_PORT: để chạy được một bản thử riêng song song với bản đang dùng
    let lock_port: u16 = std::env::var("LARMA_LOCK_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(47831);
    let _single = match std::net::TcpListener::bind(("127.0.0.1", lock_port)) {
        Ok(l) => l,
        Err(_) => {
            win::show_other();
            return Ok(());
        }
    };
    let background = has("--background");

    let saved = settings::load();
    i18n::set(i18n::resolve(&saved.lang)); // "auto"/rỗng thì theo ngôn ngữ của Windows
    let shared = engine::Shared::new(saved.params, saved.token.clone());
    shared.beep.store(!saved.no_beep, Relaxed);
    *shared.profiles.write() = saved.profiles.clone();
    *shared.settings_note.lock() = saved.note.clone();
    let tx = engine::spawn_audio_manager(shared.clone());
    engine::spawn_dsp(shared.clone());
    server::spawn(shared.clone());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(win::TITLE)
            .with_inner_size([1280.0, 900.0])
            .with_min_inner_size([880.0, 560.0])
            .with_visible(!background)
            .with_icon(egui::IconData { rgba: icon::rgba(64), width: 64, height: 64 }),
        ..Default::default()
    };
    eframe::run_native(
        win::TITLE,
        options,
        Box::new(move |cc| {
            ui::setup(&cc.egui_ctx);
            {
                // các luồng khác báo giao diện vẽ lại khi có thay đổi (kết nối, thông số, đo ngưỡng...)
                let ctx = cc.egui_ctx.clone();
                let _ = shared.wake.set(Box::new(move || ctx.request_repaint()));
            }
            spawn_events(shared.clone(), cc.egui_ctx.clone(), make_tray());
            Ok(Box::new(ui::App::new(shared, tx, saved, background)))
        }),
    )
}

struct TrayIds {
    open: MenuId,
    mute: MenuId,
    unmute: MenuId,
    quit: MenuId,
}

/// Icon ở khay hệ thống: bấm trái để mở, bấm phải ra menu.
/// Icon được giữ trong hàm đổi tooltip trên luồng chính (nó không chuyển luồng được).
fn make_tray() -> Option<TrayIds> {
    let open = MenuItem::new(i18n::t("tray.open"), true, None);
    let mute = MenuItem::new(i18n::t("tray.mute"), true, None);
    let unmute = MenuItem::new(i18n::t("tray.unmute"), true, None);
    let quit = MenuItem::new(i18n::t("tray.quit"), true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &open,
        &PredefinedMenuItem::separator(),
        &mute,
        &unmute,
        &PredefinedMenuItem::separator(),
        &quit,
    ])
    .ok()?;
    let icon = tray_icon::Icon::from_rgba(icon::rgba(32), 32, 32).ok()?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(format!("LarmaMic v{}", env!("CARGO_PKG_VERSION")))
        .with_icon(icon)
        .build()
        .ok()?;
    let ids = TrayIds { open: open.id().clone(), mute: mute.id().clone(), unmute: unmute.id().clone(), quit: quit.id().clone() };
    win::set_tip_handler(move |text| {
        match tray.set_tooltip(Some(text.to_string())) {
            Err(e) => win::log_line(&i18n::tf("tray.err.tip", &[&e.to_string()])),
            Ok(()) if std::env::var_os("LARMA_DEBUG").is_some() => {
                win::log_line(&i18n::tf("tray.log.tip", &[&text.replace('\n', " | ")]))
            }
            Ok(()) => {}
        }
    });
    // Đổi ngôn ngữ trong app thì đổi luôn chữ trên menu khay (chạy trên luồng chính, nơi giữ các mục menu)
    win::set_relabel_handler(move || {
        if std::env::var_os("LARMA_DEBUG").is_some() {
            win::log_line(&format!("tray menu relabeled: {} -> {}", i18n::current().code(), i18n::t("tray.quit")));
        }
        open.set_text(i18n::t("tray.open"));
        mute.set_text(i18n::t("tray.mute"));
        unmute.set_text(i18n::t("tray.unmute"));
        quit.set_text(i18n::t("tray.quit"));
    });
    Some(ids)
}

/// Tắt/bật tiếng (None = đảo trạng thái), có tiếng bíp báo nếu bật.
fn set_mute(sh: &engine::Shared, to: Option<bool>) {
    let mut p = *sh.params.read();
    p.mute = to.unwrap_or(!p.mute);
    sh.set_params(p);
    if sh.beep.load(Relaxed) {
        win::beep(p.mute);
    }
}

/// Chữ hiện khi rê chuột lên icon ở khay.
fn tray_tip(sh: &engine::Shared) -> String {
    let mut s = format!("LarmaMic v{}\n", env!("CARGO_PKG_VERSION"));
    s.push_str(&i18n::t(if sh.connected.load(Relaxed) { "status.connected" } else { "status.waiting" }));
    if sh.params.read().mute {
        s.push_str(&i18n::t("tip.muted"));
    }
    if sh.rec_on.load(Relaxed) {
        s.push_str(&i18n::t("tip.rec"));
    }
    s
}

/// Luồng riêng xử lý sự kiện khay và phím tắt, vì khi cửa sổ ẩn thì giao diện không chạy nữa.
/// Luồng này cũng nhờ luồng chính đổi tooltip của icon khay theo trạng thái.
fn spawn_events(sh: Arc<engine::Shared>, ctx: egui::Context, ids: Option<TrayIds>) {
    let _ = std::thread::Builder::new().name("events".into()).spawn(move || {
        let (mut last_tip, mut tick) = (String::new(), 0u32);
        loop {
            while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                let show = matches!(
                    ev,
                    TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                        | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }
                );
                if show {
                    win::show_main();
                    ctx.request_repaint();
                }
            }
            if let Some(ids) = &ids {
                while let Ok(ev) = MenuEvent::receiver().try_recv() {
                    if ev.id == ids.open {
                        win::show_main();
                        ctx.request_repaint();
                    } else if ev.id == ids.mute {
                        set_mute(&sh, Some(true));
                    } else if ev.id == ids.unmute {
                        set_mute(&sh, Some(false));
                    } else if ev.id == ids.quit {
                        // đóng file ghi âm đang mở để không để lại file WAV dở dang
                        sh.rec_req.store(2, Relaxed);
                        std::thread::sleep(Duration::from_millis(250));
                        // giữ nguyên thiết bị đã lưu, chỉ cập nhật thông số và hồ sơ (iPhone có thể vừa chỉnh)
                        let mut s = settings::load();
                        s.params = *sh.params.read();
                        s.profiles = sh.profiles.read().clone();
                        settings::save(&s);
                        std::process::exit(0);
                    }
                }
            }
            while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
                if ev.state == HotKeyState::Pressed {
                    set_mute(&sh, None);
                    ctx.request_repaint();
                }
            }
            // cập nhật tooltip (khoảng nửa giây một lần, chỉ khi đổi)
            tick += 1;
            if tick % 6 == 0 {
                let tip = tray_tip(&sh);
                if tip != last_tip && win::request_tip(&tip) {
                    last_tip = tip; // chỉ nhớ khi đã gửi được; chưa được thì thử lại ở lần sau
                }
            }
            std::thread::sleep(Duration::from_millis(80));
        }
    });
}
