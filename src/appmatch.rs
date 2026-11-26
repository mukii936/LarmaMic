//! Per-app profiles: when a listed program is running (for example Discord.exe or a game), switch to the profile or
//! preset chosen for it, and go back to the previous settings when the program closes.

use crate::engine::Shared;
use crate::settings::{apply_preset, Params, PRESET_NAMES};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct AppRule {
    /// Program name as shown in Task Manager, for example "Discord.exe" (compared without case).
    pub exe: String,
    /// A preset key ("voice", "game", "podcast", "raw") or the name of one of the user's saved profiles.
    pub profile: String,
}

#[repr(C)]
struct ProcessEntry32W {
    size: u32,
    usage: u32,
    pid: u32,
    heap: usize,
    module: u32,
    threads: u32,
    parent: u32,
    pri: i32,
    flags: u32,
    exe: [u16; 260],
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
    fn Process32FirstW(snap: isize, entry: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(snap: isize, entry: *mut ProcessEntry32W) -> i32;
    fn CloseHandle(h: isize) -> i32;
}

/// Lower-case names of the programs that are running now.
pub fn running_programs() -> Vec<String> {
    const TH32CS_SNAPPROCESS: u32 = 2;
    let mut names = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == -1 || snap == 0 {
            return names;
        }
        let mut e: ProcessEntry32W = std::mem::zeroed();
        e.size = std::mem::size_of::<ProcessEntry32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let len = e.exe.iter().position(|c| *c == 0).unwrap_or(260);
            names.push(String::from_utf16_lossy(&e.exe[..len]).to_lowercase());
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    names
}

/// First rule whose program is running.
pub fn first_match<'a>(rules: &'a [AppRule], running: &[String]) -> Option<&'a AppRule> {
    rules.iter().find(|r| {
        let want = r.exe.trim().to_lowercase();
        !want.is_empty() && running.iter().any(|p| *p == want)
    })
}

fn apply(sh: &Shared, cur: &Params, name: &str) {
    if PRESET_NAMES.iter().any(|(k, _)| *k == name) {
        sh.set_params(apply_preset(cur, name));
    } else {
        sh.apply_profile_named(name);
    }
}

pub fn spawn(sh: Arc<Shared>) {
    let _ = std::thread::Builder::new().name("appmatch".into()).spawn(move || {
        let mut active: Option<String> = None; // rule exe currently applied
        let mut before: Option<Params> = None;
        loop {
            std::thread::sleep(Duration::from_secs(3));
            if !sh.auto_profile.load(Relaxed) {
                if active.take().is_some() {
                    if let Some(p) = before.take() {
                        sh.set_params(p);
                    }
                    *sh.app_active.lock() = None;
                    sh.wake_ui();
                }
                continue;
            }
            let rules = sh.app_rules.lock().clone();
            let hit = first_match(&rules, &running_programs()).cloned();
            match (&active, hit) {
                (None, Some(r)) => {
                    before = Some(*sh.params.read());
                    let cur = *sh.params.read();
                    apply(&sh, &cur, &r.profile);
                    active = Some(r.exe.clone());
                    *sh.app_active.lock() = Some(r.exe);
                    sh.wake_ui();
                }
                (Some(cur_exe), Some(r)) if *cur_exe != r.exe => {
                    // another listed program took over
                    let cur = before.unwrap_or(*sh.params.read());
                    apply(&sh, &cur, &r.profile);
                    active = Some(r.exe.clone());
                    *sh.app_active.lock() = Some(r.exe);
                    sh.wake_ui();
                }
                (Some(_), None) => {
                    if let Some(p) = before.take() {
                        sh.set_params(p);
                    }
                    active = None;
                    *sh.app_active.lock() = None;
                    sh.wake_ui();
                }
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_first_running_rule_ignoring_case() {
        let rules = vec![
            AppRule { exe: "VALORANT.exe".into(), profile: "game".into() },
            AppRule { exe: "Discord.exe".into(), profile: "voice".into() },
        ];
        let running = vec!["explorer.exe".to_string(), "discord.exe".to_string()];
        assert_eq!(first_match(&rules, &running).map(|r| r.profile.as_str()), Some("voice"));
        let both = vec!["discord.exe".to_string(), "valorant.exe".to_string()];
        assert_eq!(first_match(&rules, &both).map(|r| r.profile.as_str()), Some("game"), "list order decides");
        assert!(first_match(&rules, &["notepad.exe".to_string()]).is_none());
        assert!(first_match(&[AppRule { exe: " ".into(), profile: "raw".into() }], &running).is_none());
    }

    #[test]
    fn the_process_list_is_not_empty_and_contains_this_program() {
        let names = running_programs();
        assert!(names.len() > 5);
        assert!(names.iter().any(|n| n.starts_with("larma_mic") || n.starts_with("larma-mic")), "{names:?}");
    }
}
