//! Diagnostics text copied to the clipboard so it can be sent when support is needed.
//! It contains no pairing code and no personal data other than audio device names.
//! The labels follow the interface language (see i18n.rs).

use crate::engine::{self, Shared};
use crate::i18n::{t, tf};
use std::os::windows::process::CommandExt;
use std::sync::atomic::Ordering::Relaxed;

fn windows_version() -> String {
    std::process::Command::new("cmd")
        .args(["/c", "ver"])
        .creation_flags(0x0800_0000)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| t("diag.unknown"))
}

fn yes_no(b: bool) -> String {
    t(if b { "diag.yes" } else { "diag.no" })
}

pub fn build(sh: &Shared, devices: &[String], out_sel: &str, hk_err: Option<&str>, hotkey: &str) -> String {
    let (y, mo, d, h, mi, s) = crate::win::local_time();
    let when = format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}");
    let p = *sh.params.read();
    let url = sh.url.lock().clone();
    let mut text = String::new();
    let mut line = |s: String| {
        text.push_str(&s);
        text.push('\n');
    };

    line(tf("diag.title", &[env!("CARGO_PKG_VERSION"), &when]));
    line(tf("diag.windows", &[&windows_version()]));
    let installed = if crate::win::is_installed() { t("diag.yes") } else { t("diag.direct") };
    line(tf("diag.installed", &[&installed]));
    line(String::new());
    line(t("diag.sec.conn"));
    line(tf("diag.server", &[&if url.is_empty() { t("diag.notrunning") } else { url }]));
    if let Some(e) = sh.server_err.lock().clone() {
        line(tf("diag.server.err", &[&e]));
    }
    let state = t(if sh.connected.load(Relaxed) { "diag.conn" } else { "diag.wait" });
    line(tf("diag.iphone", &[&state]));
    let why = sh.last_disconnect.lock().clone();
    if !why.is_empty() {
        line(tf("diag.lastdisc", &[&why]));
    }
    line(tf(
        "diag.stats",
        &[
            &sh.buf_ms.load(Relaxed).to_string(),
            &sh.under.load(Relaxed).to_string(),
            &sh.drops.load(Relaxed).to_string(),
            &sh.rtt_ms.load(Relaxed).to_string(),
            &sh.latency_ms().to_string(),
        ],
    ));
    line(String::new());
    line(t("diag.sec.audio"));
    line(tf("diag.out", &[out_sel]));
    line(tf("diag.iscable", &[&yes_no(engine::is_virtual(out_sel)), &sh.out_rate.load(Relaxed).to_string()]));
    if let Some(e) = sh.out_err.lock().clone() {
        line(tf("diag.out.err", &[&e]));
    }
    line(tf("diag.devices", &[&devices.len().to_string()]));
    for dev in devices {
        line(format!("  - {dev}"));
    }
    line(String::new());
    line(t("diag.sec.other"));
    let hk_extra = hk_err.map(|e| tf("diag.hotkey.err", &[e])).unwrap_or_default();
    line(tf("diag.hotkey", &[hotkey, &hk_extra]));
    line(tf("diag.rec", &[&t(if sh.rec_on.load(Relaxed) { "diag.rec.on" } else { "diag.rec.off" })]));
    if let Some(e) = sh.rec_err.lock().clone() {
        line(tf("diag.rec.err", &[&e]));
    }
    let note = sh.settings_note.lock().clone();
    if !note.is_empty() {
        line(tf("diag.note", &[&t(&note)]));
    }
    line(tf("diag.dir", &[&crate::settings::dir().display().to_string()]));
    line(tf("diag.profiles", &[&sh.profiles.read().len().to_string()]));
    line(tf("diag.params", &[&serde_json::to_string(&p).unwrap_or_default()]));
    text
}
