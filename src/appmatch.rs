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

#[link(name = "user32")]
extern "system" {
    fn EnumWindows(cb: unsafe extern "system" fn(isize, isize) -> i32, lparam: isize) -> i32;
    fn IsWindowVisible(hwnd: isize) -> i32;
    fn GetWindowTextLengthW(hwnd: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
    fn GetWindowLongW(hwnd: isize, index: i32) -> i32;
}

unsafe extern "system" fn collect_window(hwnd: isize, lparam: isize) -> i32 {
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: i32 = 0x0000_0080;
    if IsWindowVisible(hwnd) != 0 && GetWindowTextLengthW(hwnd) > 0 && GetWindowLongW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW == 0 {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let set = &mut *(lparam as *mut std::collections::HashSet<u32>);
        set.insert(pid);
    }
    1
}

/// Programs that have a visible window (what the user thinks of as "running apps"), with their real capitalisation,
/// sorted and without duplicates. This excludes background services, so the list stays short.
pub fn windowed_programs() -> Vec<String> {
    let mut pids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    unsafe {
        EnumWindows(collect_window, &mut pids as *mut _ as isize);
    }
    // shell and system helpers that own windows but are never what the user means
    const SKIP: [&str; 10] = [
        "applicationframehost.exe", "textinputhost.exe", "systemsettings.exe", "shellexperiencehost.exe", "searchhost.exe",
        "startmenuexperiencehost.exe", "lockapp.exe", "explorer.exe", "larma-mic.exe", "larma_mic.exe",
    ];
    let mut out: Vec<String> = process_list()
        .into_iter()
        .filter(|(pid, n)| pids.contains(pid) && !SKIP.contains(&n.to_lowercase().as_str()))
        .map(|(_, n)| n)
        .collect();
    out.sort_by_key(|n| n.to_lowercase());
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
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
    process_list().into_iter().map(|(_, n)| n.to_lowercase()).collect()
}

/// (process id, exe name) of every running process.
fn process_list() -> Vec<(u32, String)> {
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
            names.push((e.pid, String::from_utf16_lossy(&e.exe[..len])));
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
    fn windowed_programs_is_sorted_and_without_duplicates() {
        let v = windowed_programs();
        eprintln!("windowed programs: {v:?}");
        let mut sorted = v.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(v, sorted);
        let mut lower: Vec<String> = v.iter().map(|n| n.to_lowercase()).collect();
        lower.dedup();
        assert_eq!(lower.len(), v.len());
    }

    #[test]
    fn the_process_list_is_not_empty_and_contains_this_program() {
        let names = running_programs();
        assert!(names.len() > 5);
        assert!(names.iter().any(|n| n.starts_with("larma_mic") || n.starts_with("larma-mic")), "{names:?}");
    }
}
