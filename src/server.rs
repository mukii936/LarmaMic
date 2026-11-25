//! Máy chủ HTTPS + WebSocket cho iPhone (Safari chỉ cho dùng micro trên trang HTTPS).
//!
//! Giao thức WebSocket:
//!   iPhone -> PC  (text) {"t":"hello","rate":48000}   bắt đầu, báo tần số lấy mẫu
//!                        {"t":"set","p":{"thr":-50}}   đổi một phần thông số
//!                        {"t":"preset","name":"game"}  áp cấu hình nhanh
//!   iPhone -> PC  (binary) PCM 16-bit little-endian, mono
//!   PC -> iPhone  (text) {"t":"ready"} | {"t":"state","p":{...}} | {"t":"meter",...}

use crate::dsp::Resampler;
use crate::i18n::{self, t, tf};
use crate::engine::{load_f, now_ms, Shared};
use crate::settings::{self, apply_preset, matches_preset, matches_profile, PRESET_NAMES};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::Ordering::{Relaxed, SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};

// Trang iPhone viết bằng React + TypeScript (thư mục web/), build bằng `npm run build`
// rồi nhúng thẳng vào file .exe.
const PHONE_HTML: &str = include_str!("../web/dist/index.html");
const PHONE_JS: &str = include_str!("../web/dist/assets/app.js");
const PHONE_CSS: &str = include_str!("../web/dist/assets/app.css");
const PHONE_WORKLET: &str = include_str!("../web/dist/phone-worklet.js");

const ICON_180: &[u8] = include_bytes!("../web/dist/icon-180.png");
const ICON_192: &[u8] = include_bytes!("../web/dist/icon-192.png");
const ICON_512: &[u8] = include_bytes!("../web/dist/icon-512.png");

fn png(body: &'static [u8]) -> Response {
    ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "public, max-age=86400")], body).into_response()
}

/// Manifest để "Thêm vào Màn hình chính" (iOS/Android): start_url mang sẵn mã ghép nối, nên mở từ biểu tượng
/// là nối được luôn, không phải quét lại QR. Chỉ trả khi mã đúng; đổi mã ở PC thì lối tắt cũ hết hiệu lực.
async fn manifest_route(State(sh): State<Arc<Shared>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(k) = q.get("k").filter(|k| ct_eq(k, &sh.token.read())) else {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    };
    let body = json!({
        "name": "LarmaMic",
        "short_name": "LarmaMic",
        "description": t("app.tagline"),
        "start_url": format!("/?k={}", k),
        "scope": "/",
        "display": "standalone",
        "orientation": "portrait",
        "background_color": "#0d1317",
        "theme_color": "#0d1317",
        "icons": [
            { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any maskable" },
            { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any maskable" }
        ]
    })
    .to_string();
    ([(header::CONTENT_TYPE, "application/manifest+json"), (header::CACHE_CONTROL, "no-store")], body).into_response()
}

fn page(body: &'static str, kind: &'static str) -> Response {
    ([(header::CONTENT_TYPE, kind), (header::CACHE_CONTROL, "no-store")], body).into_response()
}

#[derive(Deserialize)]
struct Msg {
    t: String,
    rate: Option<u32>,
    p: Option<serde_json::Value>,
    name: Option<String>,
    ts: Option<u64>,
}

/// So sánh hai chuỗi trong thời gian không phụ thuộc vị trí khác biệt.
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |r, (x, y)| r | (x ^ y)) == 0
}

async fn ws_route(
    ws: WebSocketUpgrade,
    State(sh): State<Arc<Shared>>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let ok = q.get("k").is_some_and(|k| ct_eq(k, &sh.token.read()));
    ws.on_upgrade(move |sock| async move {
        if ok {
            handle(sock, sh).await;
        } else {
            deny(sock).await;
        }
    })
}

/// Không có mã ghép nối đúng: báo cho trang hiểu để hiện hướng dẫn, rồi đóng.
async fn deny(mut sock: WebSocket) {
    let _ = sock.send(Message::Text(r#"{"t":"denied"}"#.into())).await;
    let _ = sock.send(Message::Close(None)).await;
}

async fn handle(mut sock: WebSocket, sh: Arc<Shared>) {
    let gen = sh.conn_gen.fetch_add(1, SeqCst) + 1;
    sh.reset_input();
    let mut rs: Option<Resampler> = None;
    let mut buf: Vec<f32> = Vec::with_capacity(2048);
    let mut ready = false;
    let mut last_ver = u64::MAX;
    let mut last_rx = Instant::now();
    let mut last_ping = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(66));
    sh.rtt_ms.store(0, Relaxed);
    #[allow(unused_assignments)]
    let mut why = t("srv.why.unknown");

    loop {
        tokio::select! {
            msg = sock.recv() => {
                let msg = match msg {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => { why = tf("srv.why.recv", &[&e.to_string()]); break }
                    None => { why = t("srv.why.closed"); break }
                };
                last_rx = Instant::now();
                if sh.conn_gen.load(SeqCst) != gen {
                    why = t("srv.why.replaced");
                    break; // ngắt kết nối cũ
                }
                match msg {
                    Message::Text(t) => {
                        let Ok(m) = serde_json::from_str::<Msg>(t.as_str()) else { continue };
                        match m.t.as_str() {
                            "hello" => {
                                let rate = m.rate.unwrap_or(48_000).clamp(8_000, 192_000);
                                rs = (rate != 48_000).then(|| Resampler::new(rate, 48_000));
                                sh.connected.store(true, SeqCst);
                                sh.wake_ui();
                                ready = true;
                                last_ver = u64::MAX; // gửi trạng thái ngay ở nhịp tới
                                let _ = sock.send(Message::Text(r#"{"t":"ready"}"#.into())).await;
                            }
                            "set" => {
                                if let Some(p) = m.p {
                                    sh.merge_json(&p);
                                }
                            }
                            "calibrate" => {
                                sh.cal_req.store(true, Relaxed);
                            }
                            "profile" => {
                                if let Some(n) = m.name {
                                    sh.apply_profile_named(&n);
                                }
                            }
                            "pong" => {
                                // đo thời gian khứ hồi tới iPhone, làm mượt
                                if let Some(ts) = m.ts {
                                    let rtt = now_ms().saturating_sub(ts).min(5_000) as f32;
                                    let old = sh.rtt_ms.load(Relaxed) as f32;
                                    let new = if old == 0.0 { rtt } else { old * 0.7 + rtt * 0.3 };
                                    sh.rtt_ms.store(new.round().max(1.0) as u32, Relaxed);
                                }
                            }
                            "preset" => {
                                if let Some(n) = m.name {
                                    let cur = *sh.params.read();
                                    sh.set_params(apply_preset(&cur, &n));
                                }
                            }
                            _ => {}
                        }
                    }
                    Message::Binary(b) => {
                        buf.clear();
                        let raw: Vec<f32> = b
                            .chunks_exact(2)
                            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                            .collect();
                        match rs.as_mut() {
                            Some(r) => r.process(&raw, &mut buf),
                            None => buf.extend_from_slice(&raw),
                        }
                        sh.push_input(&buf);
                    }
                    Message::Close(_) => { why = t("srv.why.closecmd"); break }
                    _ => {}
                }
            }
            _ = tick.tick() => {
                if last_rx.elapsed() > Duration::from_secs(6) {
                    why = t("srv.why.timeout");
                    break;
                }
                if !ready {
                    continue;
                }
                let ver = sh.params_ver.load(Relaxed);
                if ver != last_ver {
                    last_ver = ver;
                    let p = *sh.params.read();
                    let preset = PRESET_NAMES.iter().find(|(k, _)| matches_preset(&p, k)).map(|(k, _)| *k);
                    let (names, cur_prof): (Vec<String>, Option<String>) = {
                        let profs = sh.profiles.read();
                        (
                            profs.iter().map(|pr| pr.name.clone()).collect(),
                            profs.iter().find(|pr| matches_profile(&p, pr)).map(|pr| pr.name.clone()),
                        )
                    };
                    let m = json!({
                        "t": "state", "p": p, "preset": preset, "profiles": names, "profile": cur_prof,
                        "lang": i18n::current().code()
                    })
                    .to_string();
                    if let Err(e) = sock.send(Message::Text(m.into())).await { why = tf("srv.why.send.state", &[&e.to_string()]); break; }
                }
                if last_ping.elapsed() >= Duration::from_secs(1) {
                    last_ping = Instant::now();
                    let m = json!({ "t": "ping", "ts": now_ms() }).to_string();
                    if let Err(e) = sock.send(Message::Text(m.into())).await { why = tf("srv.why.send.ping", &[&e.to_string()]); break; }
                }
                let m = json!({
                    "t": "meter",
                    "in": load_f(&sh.in_db),
                    "out": load_f(&sh.out_db),
                    "gate": sh.gate_open.load(Relaxed),
                    "buf": sh.buf_ms.load(Relaxed),
                    "under": sh.under.load(Relaxed),
                    "drops": sh.drops.load(Relaxed),
                    "cal": sh.cal_state.load(Relaxed),
                    "calp": load_f(&sh.cal_progress),
                    "caln": load_f(&sh.cal_noise),
                    "lat": sh.latency_ms(),
                })
                .to_string();
                if let Err(e) = sock.send(Message::Text(m.into())).await { why = tf("srv.why.send.meter", &[&e.to_string()]); break; }
            }
        }
    }
    *sh.last_disconnect.lock() = why.clone();
    crate::win::log_line(&tf("srv.log.disc", &[&why]));
    if sh.conn_gen.load(SeqCst) == gen {
        sh.connected.store(false, SeqCst);
    }
    sh.wake_ui();
}

fn load_or_make_cert(ip: &str) -> Result<(String, String), String> {
    let dir = settings::dir().join("certs");
    let _ = std::fs::create_dir_all(&dir);
    let safe = ip.replace(['.', ':'], "_");
    let (cp, kp) = (dir.join(format!("cert-{safe}.pem")), dir.join(format!("key-{safe}.pem")));
    if let (Ok(c), Ok(k)) = (std::fs::read_to_string(&cp), std::fs::read_to_string(&kp)) {
        return Ok((c, k));
    }
    let ck = rcgen::generate_simple_self_signed(vec![ip.to_string(), "localhost".to_string()])
        .map_err(|e| tf("srv.err.cert", &[&e.to_string()]))?;
    let (c, k) = (ck.cert.pem(), ck.key_pair.serialize_pem());
    let _ = std::fs::write(&cp, &c);
    let _ = std::fs::write(&kp, &k);
    Ok((c, k))
}

pub fn spawn(sh: Arc<Shared>) {
    std::thread::Builder::new()
        .name("server".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("could not create the tokio runtime");
            let err = rt.block_on(run(sh.clone()));
            if let Err(e) = err {
                *sh.server_err.lock() = Some(e);
            }
        })
        .expect("could not spawn the server thread");
}

async fn run(sh: Arc<Shared>) -> Result<(), String> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let ip: IpAddr = local_ip_address::local_ip().map_err(|_| t("srv.err.noip"))?;
    let (cert, key) = load_or_make_cert(&ip.to_string())?;
    let cfg = RustlsConfig::from_pem(cert.into_bytes(), key.into_bytes())
        .await
        .map_err(|e| tf("srv.err.tls", &[&e.to_string()]))?;

    // LARMA_PORT: cổng bắt đầu dò (mặc định 8443); dùng để chạy bản thử tách biệt với bản đang dùng
    let first: u16 = std::env::var("LARMA_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(8443);
    let port = (first..first.saturating_add(17))
        .find(|p| std::net::TcpListener::bind(("0.0.0.0", *p)).is_ok())
        .ok_or_else(|| t("srv.err.ports"))?;

    let app = Router::new()
        .route("/", get(|| async { page(PHONE_HTML, "text/html; charset=utf-8") }))
        .route("/assets/app.js", get(|| async { page(PHONE_JS, "application/javascript") }))
        .route("/assets/app.css", get(|| async { page(PHONE_CSS, "text/css; charset=utf-8") }))
        .route("/phone-worklet.js", get(|| async { page(PHONE_WORKLET, "application/javascript") }))
        .route("/icon-180.png", get(|| async { png(ICON_180) }))
        .route("/icon-192.png", get(|| async { png(ICON_192) }))
        .route("/icon-512.png", get(|| async { png(ICON_512) }))
        .route("/manifest.webmanifest", get(manifest_route))
        .route("/ws", get(ws_route))
        .with_state(sh.clone());

    *sh.url.lock() = format!("https://{ip}:{port}");
    axum_server::bind_rustls(SocketAddr::new("0.0.0.0".parse().unwrap(), port), cfg)
        .serve(app.into_make_service())
        .await
        .map_err(|e| tf("srv.err.stopped", &[&e.to_string()]))
}
