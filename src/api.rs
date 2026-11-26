//! Local control API for Stream Deck, OBS, AutoHotkey and the like: plain HTTP on 127.0.0.1 only (never on the network),
//! every request needs the pairing token (`k=`), so a web page or another program cannot drive the app by guessing.
//!
//!   http://127.0.0.1:PORT/api/status?k=TOKEN
//!   /api/mute?k=TOKEN&on=1|0|toggle      /api/preset?name=voice|game|podcast|raw      /api/profile?name=NAME
//!   /api/voice?n=0..9                    /api/play?id=N or name=TEXT                  /api/stop
//!   /api/say?text=HELLO                  /api/sounds                                  /api/test
//!
//! The port and the ready-made links are shown in Settings > Tools.

use crate::engine::Shared;
use crate::settings::{apply_preset, apply_voice, PRESET_NAMES, VOICES};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;

pub const FIRST_PORT: u16 = 8470;

fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |r, (x, y)| r | (x ^ y)) == 0
}

fn reply(code: StatusCode, v: Value) -> Response {
    (code, [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], v.to_string()).into_response()
}

fn err(code: StatusCode, msg: &str) -> Response {
    reply(code, json!({ "ok": false, "error": msg }))
}

fn status(sh: &Shared) -> Value {
    let p = *sh.params.read();
    let voice = VOICES.iter().position(|v| v.is_on(&p));
    let preset = PRESET_NAMES.iter().find(|(k, _)| crate::settings::matches_preset(&p, k)).map(|(k, _)| *k);
    json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "mute": p.mute,
        "phones": sh.sources.lock().len(),
        "preset": preset,
        "voice": voice,
        "pc_audio": sh.pc_on.load(Relaxed),
        "test": sh.test_state.load(Relaxed),
        "speaking": sh.tts_busy.load(Relaxed),
        "buffer_ms": sh.params.read().buf_ms,
    })
}

async fn handle(State(sh): State<Arc<Shared>>, Path(cmd): Path<String>, Query(q): Query<HashMap<String, String>>) -> Response {
    if !q.get("k").is_some_and(|k| ct_eq(k, &sh.token.read())) {
        return err(StatusCode::FORBIDDEN, "bad token");
    }
    // everything below only flips settings in memory or starts a sound: cheap, so it runs inline
    match cmd.as_str() {
        "status" => reply(StatusCode::OK, status(&sh)),
        "mute" => {
            let mut p = *sh.params.read();
            p.mute = match q.get("on").map(|s| s.as_str()) {
                Some("1") | Some("true") | Some("on") => true,
                Some("0") | Some("false") | Some("off") => false,
                _ => !p.mute,
            };
            sh.set_params(p);
            if sh.beep.load(Relaxed) {
                crate::win::beep(p.mute);
            }
            reply(StatusCode::OK, status(&sh))
        }
        "preset" => {
            let Some(name) = q.get("name").filter(|n| PRESET_NAMES.iter().any(|(k, _)| k == n)) else {
                return err(StatusCode::BAD_REQUEST, "unknown preset");
            };
            let cur = *sh.params.read();
            sh.set_params(apply_preset(&cur, name));
            reply(StatusCode::OK, status(&sh))
        }
        "profile" => {
            if !q.get("name").is_some_and(|n| sh.apply_profile_named(n)) {
                return err(StatusCode::NOT_FOUND, "no such profile");
            }
            reply(StatusCode::OK, status(&sh))
        }
        "voice" => {
            let Some(n) = q.get("n").and_then(|n| n.parse::<usize>().ok()).filter(|n| *n < VOICES.len()) else {
                return err(StatusCode::BAD_REQUEST, "voice must be 0..9");
            };
            let cur = *sh.params.read();
            sh.set_params(apply_voice(&cur, n));
            reply(StatusCode::OK, status(&sh))
        }
        "sounds" => {
            let list: Vec<Value> = sh.board.lock().sounds.iter().map(|s| json!({ "id": s.id, "name": s.name, "group": s.group })).collect();
            reply(StatusCode::OK, json!({ "ok": true, "sounds": list }))
        }
        "play" => {
            let id = match (q.get("id").and_then(|v| v.parse::<u32>().ok()), q.get("name")) {
                (Some(id), _) => Some(id),
                (None, Some(name)) => {
                    let name = name.to_lowercase();
                    sh.board.lock().sounds.iter().find(|s| s.name.to_lowercase() == name).map(|s| s.id)
                }
                _ => None,
            };
            match id {
                Some(id) if sh.board.lock().sounds.iter().any(|s| s.id == id) => {
                    crate::soundboard::play(&sh, id);
                    reply(StatusCode::OK, json!({ "ok": true }))
                }
                _ => err(StatusCode::NOT_FOUND, "no such sound"),
            }
        }
        "stop" => {
            crate::soundboard::stop_all(&sh);
            reply(StatusCode::OK, json!({ "ok": true }))
        }
        "say" => {
            let text = q.get("text").cloned().unwrap_or_default();
            if crate::speech::clean_text(&text).is_empty() {
                return err(StatusCode::BAD_REQUEST, "text is empty");
            }
            crate::speech::say_async(sh.clone(), text);
            reply(StatusCode::OK, json!({ "ok": true }))
        }
        "test" => {
            sh.test_req.store(true, Relaxed);
            reply(StatusCode::OK, json!({ "ok": true }))
        }
        _ => err(StatusCode::NOT_FOUND, "unknown command"),
    }
}

/// Runs on the server's tokio runtime. Starts and stops listening as the switch in Settings changes.
pub async fn run(sh: Arc<Shared>) {
    loop {
        if !sh.api_on.load(Relaxed) {
            tokio::time::sleep(Duration::from_millis(700)).await;
            continue;
        }
        let first: u16 = std::env::var("LARMA_API_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(FIRST_PORT);
        let mut bound = None;
        for port in first..first.saturating_add(17) {
            if let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
                bound = Some((l, port));
                break;
            }
        }
        let Some((listener, port)) = bound else {
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        };
        sh.api_port.store(port as u32, Relaxed);
        sh.wake_ui();
        let app = Router::new().route("/api/{cmd}", get(handle)).with_state(sh.clone());
        let sh2 = sh.clone();
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                while sh2.api_on.load(Relaxed) {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            })
            .await;
        sh.api_port.store(0, Relaxed);
        sh.wake_ui();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison() {
        assert!(ct_eq("abc", "abc"));
        assert!(!ct_eq("abc", "abd"));
        assert!(!ct_eq("abc", "abcd"));
    }

    /// Starts the real listener on a spare port and drives it over HTTP (curl.exe).
    #[test]
    fn answers_only_with_the_right_token() {
        use std::process::Command;
        let sh = Shared::new(crate::settings::Params::default(), "secrettoken0123456789abcdef0123".into());
        sh.api_on.store(true, Relaxed);
        std::env::set_var("LARMA_API_PORT", "8590");
        let sh2 = sh.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
            rt.block_on(run(sh2));
        });
        for _ in 0..50 {
            if sh.api_port.load(Relaxed) != 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let port = sh.api_port.load(Relaxed);
        assert!(port >= 8590, "no port");
        let get = |path: &str| {
            let o = Command::new("curl.exe")
                .args(["-s", "-w", "\n%{http_code}", &format!("http://127.0.0.1:{port}{path}")])
                .output()
                .expect("curl");
            let s = String::from_utf8_lossy(&o.stdout).to_string();
            let (body, code) = s.rsplit_once('\n').unwrap_or((&s, ""));
            (code.trim().to_string(), body.to_string())
        };
        assert_eq!(get("/api/status").0, "403");
        assert_eq!(get("/api/status?k=wrong").0, "403");
        let (code, body) = get("/api/status?k=secrettoken0123456789abcdef0123");
        assert_eq!(code, "200", "{body}");
        assert!(body.contains("\"mute\":false"), "{body}");
        let (_, body) = get("/api/mute?k=secrettoken0123456789abcdef0123&on=1");
        assert!(body.contains("\"mute\":true"), "{body}");
        assert!(sh.params.read().mute);
        let (_, body) = get("/api/voice?k=secrettoken0123456789abcdef0123&n=3");
        assert!(body.contains("\"voice\":3"), "{body}");
        assert!(sh.params.read().fx_robot);
        assert_eq!(get("/api/voice?k=secrettoken0123456789abcdef0123&n=99").0, "400");
        assert_eq!(get("/api/nothing?k=secrettoken0123456789abcdef0123").0, "404");
        // switching it off closes the port
        sh.api_on.store(false, Relaxed);
        for _ in 0..40 {
            if sh.api_port.load(Relaxed) == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(sh.api_port.load(Relaxed), 0);
    }
}
