//! USB cable link without network tethering, through Android's `adb`:
//! when a phone with USB debugging is plugged in, `adb reverse tcp:PORT tcp:PORT` makes the phone's
//! 127.0.0.1:PORT reach this PC, so the Android app connects over the cable only. The PC keeps its own internet
//! and no extra network adapter appears.
//!
//! adb is looked up in PATH, the Android SDK and next to the exe; if it is missing the user can download it from
//! Google with one click (it is never fetched without that click).

use crate::engine::Shared;
use std::collections::HashSet;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;

pub const MISSING: u8 = 0;
pub const NO_DEVICE: u8 = 1;
pub const UNAUTHORIZED: u8 = 2;
pub const READY: u8 = 3;

const URL: &str = "https://dl.google.com/android/repository/platform-tools-latest-windows.zip";

fn downloaded_dir() -> PathBuf {
    crate::settings::dir().join("platform-tools")
}

/// Path of adb.exe, if there is one.
pub fn find() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("LARMA_ADB") {
        c.push(PathBuf::from(p));
    }
    c.push(downloaded_dir().join("adb.exe"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            c.push(d.join("adb.exe"));
        }
    }
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(h) = std::env::var_os(var) {
            c.push(PathBuf::from(h).join("platform-tools").join("adb.exe"));
        }
    }
    for var in ["LOCALAPPDATA", "USERPROFILE"] {
        if let Some(h) = std::env::var_os(var) {
            let base = PathBuf::from(h);
            c.push(base.join("Android").join("Sdk").join("platform-tools").join("adb.exe"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            c.push(d.join("adb.exe"));
        }
    }
    c.into_iter().find(|p| p.is_file())
}

fn run(adb: &PathBuf, args: &[&str]) -> Option<String> {
    let out = Command::new(adb).args(args).creation_flags(0x0800_0000).output().ok()?;
    let mut s = String::from_utf8_lossy(&out.stdout).to_string();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(s).filter(|_| out.status.success())
}

/// (ready serials, unauthorised count) from the `adb devices` output.
fn parse_devices(text: &str) -> (Vec<String>, usize) {
    let (mut ok, mut unauth) = (Vec::new(), 0);
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if let (Some(serial), Some(state)) = (it.next(), it.next()) {
            match state {
                "device" => ok.push(serial.to_string()),
                "unauthorized" => unauth += 1,
                _ => {}
            }
        }
    }
    (ok, unauth)
}

fn port_of(url: &str) -> Option<u16> {
    url.rsplit(':').next()?.trim_end_matches('/').parse().ok()
}

/// Watches for USB-debugging phones every few seconds (cheap: nothing is spawned while adb is missing).
pub fn spawn(sh: Arc<Shared>) {
    let _ = std::thread::Builder::new().name("adb".into()).spawn(move || {
        let mut linked: HashSet<String> = HashSet::new();
        let mut opened: HashSet<String> = HashSet::new(); // serial:port:token already sent to the phone
        loop {
            let url = sh.url.lock().clone();
            let state = match (find(), port_of(&url)) {
                (None, _) => MISSING,
                (Some(_), None) => NO_DEVICE, // server not up yet
                (Some(adb), Some(port)) => match run(&adb, &["devices"]) {
                    None => NO_DEVICE,
                    Some(text) => {
                        let (ready, unauth) = parse_devices(&text);
                        linked.retain(|k| ready.iter().any(|s| k.starts_with(&format!("{s}:"))));
                        opened.retain(|k| ready.iter().any(|s| k.starts_with(&format!("{s}:"))));
                        let mut any = false;
                        for serial in &ready {
                            let key = format!("{serial}:{port}");
                            if !linked.contains(&key) {
                                let tcp = format!("tcp:{port}");
                                if run(&adb, &["-s", serial, "reverse", &tcp, &tcp]).is_some() {
                                    linked.insert(key.clone());
                                }
                            }
                            if linked.contains(&key) {
                                any = true;
                                // No QR code over the cable: open the Android app on the phone and hand it the pairing.
                                // Skipped when a phone is already streaming, and when the app is not installed (am fails).
                                let token = sh.token.read().clone();
                                let sent = format!("{key}:{token}");
                                if sh.usb_auto.load(Relaxed) && !sh.connected.load(Relaxed) && !opened.contains(&sent) {
                                    opened.retain(|k| !k.starts_with(&format!("{key}:")));
                                    opened.insert(sent);
                                    let port_s = port.to_string();
                                    let _ = run(
                                        &adb,
                                        &[
                                            "-s", serial, "shell", "am", "start", "-a", "app.larmamic.CONNECT",
                                            "-n", "app.larmamic/.MainActivity",
                                            "--es", "host", "127.0.0.1", "--ei", "port", &port_s, "--es", "token", &token,
                                        ],
                                    );
                                }
                            }
                        }
                        if any {
                            READY
                        } else if unauth > 0 {
                            UNAUTHORIZED
                        } else {
                            NO_DEVICE
                        }
                    }
                },
            };
            if sh.adb_state.swap(state, Relaxed) != state {
                sh.wake_ui();
            }
            // adb found: poll quickly so plugging in feels instant; missing: check rarely
            std::thread::sleep(Duration::from_secs(if state == MISSING { 6 } else { 2 }));
        }
    });
}

/// Downloads Google's platform-tools and keeps adb.exe in the settings folder. Runs on a thread; the result is
/// reported through `Shared::adb_dl` (None = idle, Some("") = downloading, Some(err) = failed).
pub fn download(sh: Arc<Shared>) {
    if sh.adb_dl.lock().as_deref() == Some("") {
        return;
    }
    *sh.adb_dl.lock() = Some(String::new());
    sh.wake_ui();
    let _ = std::thread::Builder::new().name("adb-download".into()).spawn(move || {
        let dir = downloaded_dir();
        let script = format!(
            "$ErrorActionPreference='Stop';[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;\
             $t=Join-Path $env:TEMP 'larma-pt';Remove-Item $t -Recurse -Force -ErrorAction SilentlyContinue;\
             New-Item -ItemType Directory -Force $t | Out-Null;$z=Join-Path $t 'pt.zip';\
             Invoke-WebRequest -UseBasicParsing '{URL}' -OutFile $z;Expand-Archive $z $t -Force;\
             New-Item -ItemType Directory -Force '{dir}' | Out-Null;\
             foreach($f in 'adb.exe','AdbWinApi.dll','AdbWinUsbApi.dll'){{Copy-Item (Join-Path $t \"platform-tools\\$f\") '{dir}' -Force}};\
             Remove-Item $t -Recurse -Force",
            dir = dir.display().to_string().replace('\'', "''"),
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x0800_0000)
            .output();
        let result = match out {
            Ok(o) if o.status.success() && dir.join("adb.exe").is_file() => None,
            Ok(o) => Some(String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("error").trim().to_string()),
            Err(e) => Some(e.to_string()),
        };
        *sh.adb_dl.lock() = result.map(|e| if e.is_empty() { "error".to_string() } else { e });
        sh.wake_ui();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adb_devices_output() {
        let text = "* daemon not running; starting now at tcp:5037\n* daemon started successfully\nList of devices attached\nR58M123ABC\tdevice\nemulator-5554\tdevice\nXYZ999\tunauthorized\nOFF1\toffline\n";
        let (ok, unauth) = parse_devices(text);
        assert_eq!(ok, vec!["R58M123ABC", "emulator-5554"]);
        assert_eq!(unauth, 1);
    }

    #[test]
    fn reads_port_from_server_url() {
        assert_eq!(port_of("https://192.168.1.5:8443"), Some(8443));
        assert_eq!(port_of(""), None);
    }
}
