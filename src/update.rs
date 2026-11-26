//! Checks GitHub for a newer release and tells the user (never installs anything by itself).
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
}
