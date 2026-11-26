//! Checks GitHub for a newer release and tells the user. It installs only when the user clicks "Install": the new
//! larma-mic.exe is downloaded from the project's GitHub release, its SHA-256 is checked against the published
//! SHA256.txt, and only then does it replace the running program.
//! One small request to api.github.com at startup and then every six hours; it sends no user data. Can be turned off.

use crate::engine::Shared;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;

const API: &str = "https://api.github.com/repos/mukii936/LarmaMic/releases/latest";

/// "v0.7.1" / "0.7.1" -> (0, 7, 1)
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let mut it = s.split(['.', '-', '+']);
    let a = it.next()?.parse().ok()?;
    let b = it.next().unwrap_or("0").parse().ok()?;
    let c = it.next().unwrap_or("0").parse().ok()?;
    Some((a, b, c))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

const DOWNLOAD: &str = "https://github.com/mukii936/LarmaMic/releases/download/";

/// The hash for `larma-mic.exe` in a SHA256.txt ("<hash>  larma-mic.exe" per line).
pub fn parse_sha256(txt: &str, file: &str) -> Option<String> {
    txt.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (h, name) = (it.next()?, it.next()?);
        (name.trim_start_matches('*') == file && h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| h.to_lowercase())
    })
}

/// SHA-256 of a file through Windows' own certutil (no extra crate).
fn file_sha256(path: &std::path::Path) -> Option<String> {
    let o = Command::new("certutil").args(["-hashfile"]).arg(path).arg("SHA256").creation_flags(0x0800_0000).output().ok()?;
    String::from_utf8_lossy(&o.stdout).lines().map(|l| l.replace(' ', "").to_lowercase()).find(|l| l.len() == 64 && l.chars().all(|c| c.is_ascii_hexdigit()))
}

fn download(url: &str, to: &std::path::Path) -> Result<(), String> {
    let o = Command::new("curl.exe")
        .args(["-L", "--fail", "--silent", "--show-error", "--max-time", "300", "-A", "LarmaMic", "-o"])
        .arg(to)
        .arg(url)
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("download failed").trim().to_string())
    }
}

/// Downloads version `ver`, verifies it and swaps it in: the running exe is renamed to `.old` (Windows allows that),
/// the new one takes its place and a small helper starts it after this process has exited.
fn install(ver: &str) -> Result<(), String> {
    let cur = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = crate::settings::dir().join("update");
    install_with(&format!("{DOWNLOAD}v{ver}/"), &cur, &dir, true)
}

/// Does the work of `install` for any release folder `base` (ends with '/'), program path `cur` and scratch folder `work`.
fn install_with(base: &str, cur: &std::path::Path, work: &std::path::Path, restart: bool) -> Result<(), String> {
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let (exe, sums) = (work.join("larma-mic-new.exe"), work.join("SHA256.txt"));
    let result = (|| -> Result<(), String> {
        download(&format!("{base}SHA256.txt"), &sums)?;
        download(&format!("{base}larma-mic.exe"), &exe)?;
        let want = parse_sha256(&std::fs::read_to_string(&sums).map_err(|e| e.to_string())?, "larma-mic.exe")
            .ok_or_else(|| crate::i18n::t("upd.err.nosum"))?;
        if file_sha256(&exe).as_deref() != Some(want.as_str()) {
            return Err(crate::i18n::t("upd.err.sum"));
        }
        let old = cur.with_extension("exe.old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(cur, &old).map_err(|e| crate::i18n::tf("upd.err.swap", &[&e.to_string()]))?;
        if let Err(e) = std::fs::copy(&exe, cur) {
            let _ = std::fs::rename(&old, cur); // put the working program back
            return Err(crate::i18n::tf("upd.err.swap", &[&e.to_string()]));
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(&sums);
    result?;
    if restart {
        // start the new version a moment after this process has quit (the single-instance lock is released then)
        let _ = Command::new("cmd")
            .arg("/c")
            .raw_arg("ping -n 3 127.0.0.1 >nul & start \"\" \"%LARMA_EXE%\"")
            .env("LARMA_EXE", cur)
            .creation_flags(0x0800_0000)
            .spawn();
    }
    Ok(())
}

/// Install button: runs on a worker thread, then quits so the new version can start.
pub fn install_async(sh: Arc<Shared>, ver: String) {
    if sh.upd_busy.swap(true, Relaxed) {
        return;
    }
    *sh.upd_msg.lock() = None;
    sh.wake_ui();
    let _ = std::thread::Builder::new().name("update-install".into()).spawn(move || {
        match install(&ver) {
            Ok(()) => crate::quit_now(&sh),
            Err(e) => {
                *sh.upd_msg.lock() = Some(e);
                sh.upd_busy.store(false, Relaxed);
                sh.wake_ui();
            }
        }
    });
}

/// Removes the previous version left next to the exe by an update (at startup).
pub fn cleanup_old() {
    if let Ok(cur) = std::env::current_exe() {
        let _ = std::fs::remove_file(cur.with_extension("exe.old"));
    }
}

fn fetch_latest() -> Option<(String, String)> {
    let out = Command::new("curl.exe")
        .args(["-s", "--fail", "--max-time", "15", "-A", "LarmaMic", "-H", "Accept: application/vnd.github+json", API])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let url = v.get("html_url")?.as_str()?.to_string();
    if !url.starts_with("https://github.com/") {
        return None;
    }
    Some((tag, url))
}

pub fn spawn(sh: Arc<Shared>) {
    let _ = std::thread::Builder::new().name("update".into()).spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        loop {
            if sh.update_check.load(Relaxed) {
                if let Some((tag, url)) = fetch_latest() {
                    if is_newer(&tag, env!("CARGO_PKG_VERSION")) {
                        *sh.update.lock() = Some((tag.trim_start_matches(['v', 'V']).to_string(), url));
                        sh.wake_ui();
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(6 * 3600));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("v0.7.0", "0.6.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("v0.6.0", "0.6.0"));
        assert!(!is_newer("0.5.9", "0.6.0"));
        assert!(!is_newer("nonsense", "0.6.0"));
        assert_eq!(parse_version("v1.2"), Some((1, 2, 0)));
    }

    #[test]
    fn reads_the_published_checksum_file() {
        let h = "a".repeat(64);
        let txt = format!("{h}  larma-mic.exe\n");
        assert_eq!(parse_sha256(&txt, "larma-mic.exe"), Some(h.clone()));
        assert_eq!(parse_sha256(&format!("{h} *larma-mic.exe"), "larma-mic.exe"), Some(h));
        assert_eq!(parse_sha256("deadbeef  larma-mic.exe", "larma-mic.exe"), None, "short hash");
        assert_eq!(parse_sha256(&format!("{}  other.exe", "b".repeat(64)), "larma-mic.exe"), None);
    }

    /// Serves a fake release (an "exe" and its SHA256.txt) on loopback and installs it over a stand-in program file.
    #[test]
    fn installs_a_verified_download_and_refuses_a_tampered_one() {
        use std::io::{Read, Write};
        let root = std::env::temp_dir().join(format!("larma-upd-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let new_bytes = b"NEW PROGRAM BYTES".to_vec();
        std::fs::write(root.join("probe.bin"), &new_bytes).unwrap();
        let good = file_sha256(&root.join("probe.bin")).expect("hash");
        let serve = |sum: String, body: Vec<u8>| {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            std::thread::spawn(move || {
                for _ in 0..2 {
                    let Ok((mut c, _)) = l.accept() else { return };
                    let mut buf = [0u8; 2048];
                    let n = c.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let payload = if req.contains("SHA256.txt") { format!("{sum}  larma-mic.exe
").into_bytes() } else { body.clone() };
                    let _ = c.write_all(format!("HTTP/1.1 200 OK
Content-Length: {}
Connection: close

", payload.len()).as_bytes());
                    let _ = c.write_all(&payload);
                }
            });
            format!("http://127.0.0.1:{port}/v9.9.9/")
        };
        // good download replaces the stand-in and keeps the old one as .old
        let cur = root.join("larma-mic.exe");
        std::fs::write(&cur, b"OLD").unwrap();
        install_with(&serve(good.clone(), new_bytes.clone()), &cur, &root.join("work"), false).expect("install");
        assert_eq!(std::fs::read(&cur).unwrap(), new_bytes);
        assert_eq!(std::fs::read(cur.with_extension("exe.old")).unwrap(), b"OLD");
        // wrong bytes (checksum mismatch) must leave the program untouched
        std::fs::write(&cur, b"OLD2").unwrap();
        let err = install_with(&serve(good, b"TAMPERED".to_vec()), &cur, &root.join("work"), false).expect_err("must refuse");
        assert!(!err.is_empty());
        assert_eq!(std::fs::read(&cur).unwrap(), b"OLD2");
        assert!(!root.join("work").join("larma-mic-new.exe").exists(), "downloaded file is discarded");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hashes_a_file_like_the_release_does() {
        let p = std::env::temp_dir().join("larma-hash-test.bin");
        std::fs::write(&p, b"abc").unwrap();
        let h = file_sha256(&p);
        let _ = std::fs::remove_file(&p);
        // SHA-256("abc")
        assert_eq!(h.as_deref(), Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
    }
}
