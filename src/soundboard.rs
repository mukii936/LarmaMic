//! Soundboard: short sound effects (mp3, wav, ogg, flac) that are mixed into the microphone output on a button or a hotkey.
//! Sounds are copied into %APPDATA%\LarmaMic\sounds so they keep working if the original file moves. They can be added
//! from a file dialog, by drag and drop, or from a direct link to an audio file (downloaded once with curl.exe).

use crate::dsp::Resampler;
use crate::engine::Shared;
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

pub const MAX_SOUNDS: usize = 60;
const MAX_SECONDS: usize = 60;
const EXTS: [&str; 5] = ["mp3", "wav", "ogg", "flac", "oga"];

#[derive(Clone, Serialize, Deserialize)]
pub struct Sound {
    pub id: u32,
    pub name: String,
    /// File name inside the sounds folder.
    pub file: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    pub sounds: Vec<Sound>,
    pub next_id: u32,
    /// Loudness of the sounds relative to the voice, 0..1.5 (1.0 = as decoded and normalised).
    pub volume: f32,
    /// Lower the voice while a sound plays.
    pub duck: bool,
    /// Ctrl+Alt+1..9 play the first nine sounds.
    pub hotkeys: bool,
    #[serde(skip)]
    pub cache: std::collections::HashMap<u32, Arc<Vec<f32>>>,
}

/// A sound that is playing right now.
pub struct Playing {
    pub buf: Arc<Vec<f32>>,
    pub pos: usize,
}

pub fn dir() -> PathBuf {
    crate::settings::dir().join("sounds")
}

fn index_file() -> PathBuf {
    crate::settings::dir().join("sounds.json")
}

pub fn load(sh: &Shared) {
    let mut b: Board = std::fs::read_to_string(index_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if b.volume <= 0.0 {
        b.volume = 0.8;
    }
    if b.next_id == 0 {
        b.next_id = 1;
    }
    b.sounds.retain(|s| dir().join(&s.file).is_file());
    *sh.board.lock() = b;
    sh.board_ver.fetch_add(1, Relaxed);
}

pub fn save(sh: &Shared) {
    let text = {
        let b = sh.board.lock();
        serde_json::to_string_pretty(&*b).unwrap_or_default()
    };
    let _ = std::fs::create_dir_all(crate::settings::dir());
    let _ = std::fs::write(index_file(), text);
    sh.board_ver.fetch_add(1, Relaxed);
}

/// Decodes any supported file to 48 kHz mono f32, normalised to a peak of 0.9 and capped at one minute.
pub fn decode(path: &Path) -> Result<Vec<f32>, String> {
    use symphonia::core::{
        audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions, io::MediaSourceStream,
        meta::MetadataOptions, probe::Hint,
    };
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| e.to_string())?;
    let mut format = probed.format;
    let track = format.default_track().ok_or("no audio track")?;
    let track_id = track.id;
    let rate = track.codec_params.sample_rate.ok_or("unknown sample rate")? as usize;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| e.to_string())?;
    let mut mono: Vec<f32> = Vec::new();
    let mut sbuf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(_)) | Err(Error::ResetRequired) => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let spec = *buf.spec();
                let ch = spec.channels.count().max(1);
                let sb = sbuf.get_or_insert_with(|| SampleBuffer::<f32>::new(buf.capacity() as u64, spec));
                sb.copy_interleaved_ref(buf);
                for f in sb.samples().chunks(ch) {
                    mono.push(f.iter().sum::<f32>() / ch as f32);
                }
                if mono.len() > rate * (MAX_SECONDS + 5) {
                    break;
                }
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    if mono.is_empty() {
        return Err("empty audio".into());
    }
    let mut out = if rate != 48_000 {
        let mut rs = Resampler::new(rate as u32, 48_000);
        let mut o = Vec::with_capacity(mono.len() * 48_000 / rate + 16);
        rs.process(&mono, &mut o);
        o
    } else {
        mono
    };
    out.truncate(48_000 * MAX_SECONDS);
    let peak = out.iter().fold(0f32, |m, x| m.max(x.abs()));
    if peak > 1e-4 {
        let g = 0.9 / peak;
        out.iter_mut().for_each(|x| *x *= g);
    }
    Ok(out)
}

fn clean_name(s: &str) -> String {
    let t: String = s.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(40).collect();
    if t.is_empty() { "sound".to_string() } else { t }
}

fn safe_file(base: &str, ext: &str) -> String {
    let stem: String = base.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let stem: String = stem.chars().take(40).collect();
    format!("{}-{}.{ext}", if stem.is_empty() { "sound" } else { &stem }, now_tag())
}

fn now_tag() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64 % 10_000_000).unwrap_or(0)
}

/// Copies `src` into the sounds folder, checks that it decodes and adds it to the board.
pub fn add_file(sh: &Arc<Shared>, src: &Path, name: Option<&str>) -> Result<(), String> {
    if sh.board.lock().sounds.len() >= MAX_SOUNDS {
        return Err(crate::i18n::tf("board.err.full", &[&MAX_SOUNDS.to_string()]));
    }
    let ext = src.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).unwrap_or_default();
    if !EXTS.contains(&ext.as_str()) {
        return Err(crate::i18n::t("board.err.format"));
    }
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("sound").to_string();
    let display = clean_name(name.unwrap_or(&stem));
    let samples = decode(src).map_err(|e| crate::i18n::tf("board.err.decode", &[&e]))?;
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let file = safe_file(&stem, &ext);
    std::fs::copy(src, dir().join(&file)).map_err(|e| e.to_string())?;
    {
        let mut b = sh.board.lock();
        let id = b.next_id.max(1);
        b.next_id = id + 1;
        b.sounds.push(Sound { id, name: display, file });
        b.cache.insert(id, Arc::new(samples));
    }
    save(sh);
    Ok(())
}

/// Downloads a direct link to an audio file (http/https, up to 15 MB) and adds it.
pub fn add_url(sh: &Arc<Shared>, url: &str) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.contains(|c: char| c.is_whitespace() || c == '"') {
        return Err(crate::i18n::t("board.err.url"));
    }
    let path_part = url.split(['?', '#']).next().unwrap_or(url);
    let last = path_part.rsplit('/').next().unwrap_or("sound");
    let (stem, ext) = match last.rsplit_once('.') {
        Some((s, e)) if EXTS.contains(&e.to_lowercase().as_str()) => (s.to_string(), e.to_lowercase()),
        _ => (last.to_string(), "mp3".to_string()),
    };
    let stem = percent_decode(&stem);
    let tmp = std::env::temp_dir().join(format!("larma-dl-{}.{ext}", now_tag()));
    let out = Command::new("curl.exe")
        .args(["-L", "--fail", "--silent", "--show-error", "--max-filesize", "15000000", "--max-time", "40", "-A", "LarmaMic", "-o"])
        .arg(&tmp)
        .arg(url)
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        let msg = String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("download failed").trim().to_string();
        return Err(crate::i18n::tf("board.err.download", &[&msg]));
    }
    let res = add_file(sh, &tmp, Some(&stem));
    let _ = std::fs::remove_file(&tmp);
    res
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).replace('_', " ")
}

pub fn remove(sh: &Arc<Shared>, id: u32) {
    let file = {
        let mut b = sh.board.lock();
        b.cache.remove(&id);
        let f = b.sounds.iter().find(|s| s.id == id).map(|s| s.file.clone());
        b.sounds.retain(|s| s.id != id);
        f
    };
    if let Some(f) = file {
        let _ = std::fs::remove_file(dir().join(f));
    }
    save(sh);
}

/// Starts a sound (decoding it first if it is not cached yet).
pub fn play(sh: &Arc<Shared>, id: u32) {
    let (buf, file) = {
        let b = sh.board.lock();
        (b.cache.get(&id).cloned(), b.sounds.iter().find(|s| s.id == id).map(|s| s.file.clone()))
    };
    let buf = match (buf, file) {
        (Some(b), _) => b,
        (None, Some(f)) => match decode(&dir().join(f)) {
            Ok(v) => {
                let a = Arc::new(v);
                sh.board.lock().cache.insert(id, a.clone());
                a
            }
            Err(e) => {
                *sh.board_msg.lock() = Some(crate::i18n::tf("board.err.decode", &[&e]));
                return;
            }
        },
        _ => return,
    };
    let mut pl = sh.playing.lock();
    if pl.len() >= 16 {
        pl.remove(0);
    }
    pl.push(Playing { buf, pos: 0 });
    drop(pl);
    sh.wake_ui();
}

pub fn stop_all(sh: &Shared) {
    sh.playing.lock().clear();
    sh.wake_ui();
}

/// Opens the Windows file dialog (multi-select) on a worker thread and adds what was chosen.
pub fn pick_files(sh: Arc<Shared>) {
    let _ = std::thread::Builder::new().name("board-pick".into()).spawn(move || {
        let script = "Add-Type -AssemblyName System.Windows.Forms;$d=New-Object System.Windows.Forms.OpenFileDialog;\
                      $d.Filter='Audio|*.mp3;*.wav;*.ogg;*.oga;*.flac|All files|*.*';$d.Multiselect=$true;\
                      if($d.ShowDialog() -eq 'OK'){$d.FileNames | ForEach-Object { [Console]::Out.WriteLine($_) }}";
        let out = Command::new("powershell")
            .args(["-NoProfile", "-STA", "-NonInteractive", "-Command", script])
            .creation_flags(0x0800_0000)
            .output();
        if let Ok(o) = out {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let p = line.trim();
                if !p.is_empty() {
                    report(&sh, add_file(&sh, Path::new(p), None));
                }
            }
        }
        sh.wake_ui();
    });
}

pub fn add_url_async(sh: Arc<Shared>, url: String) {
    *sh.board_busy.lock() = true;
    sh.wake_ui();
    let _ = std::thread::Builder::new().name("board-url".into()).spawn(move || {
        report(&sh, add_url(&sh, &url));
        *sh.board_busy.lock() = false;
        sh.wake_ui();
    });
}

pub fn add_paths_async(sh: Arc<Shared>, paths: Vec<PathBuf>) {
    let _ = std::thread::Builder::new().name("board-drop".into()).spawn(move || {
        for p in paths {
            report(&sh, add_file(&sh, &p, None));
        }
        sh.wake_ui();
    });
}

fn report(sh: &Shared, r: Result<(), String>) {
    *sh.board_msg.lock() = r.err();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a tiny 16-bit mono WAV (a 440 Hz tone) for the decoder test.
    fn write_wav(path: &Path, rate: u32, secs: f32) {
        let n = (rate as f32 * secs) as usize;
        let mut data = Vec::new();
        for i in 0..n {
            let s = ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI / rate as f32).sin() * 12000.0) as i16;
            data.extend_from_slice(&s.to_le_bytes());
        }
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&rate.to_le_bytes());
        f.extend_from_slice(&(rate * 2).to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&(data.len() as u32).to_le_bytes());
        f.extend_from_slice(&data);
        std::fs::write(path, f).unwrap();
    }

    #[test]
    fn decodes_wav_to_48k_mono_and_normalises() {
        let p = std::env::temp_dir().join("larma-test-tone.wav");
        write_wav(&p, 22_050, 0.5);
        let v = decode(&p).expect("decode");
        let _ = std::fs::remove_file(&p);
        // 0.5 s at 22.05 kHz becomes about 0.5 s at 48 kHz
        assert!((v.len() as i64 - 24_000).abs() < 400, "len {}", v.len());
        let peak = v.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!((peak - 0.9).abs() < 0.05, "peak {peak}");
    }

    #[test]
    fn rejects_unknown_formats_and_bad_links() {
        assert_eq!(percent_decode("a%20b_c"), "a b c");
        assert!(decode(Path::new("C:\\definitely\\missing.mp3")).is_err());
    }

    /// Needs curl.exe and loopback networking; changes APPDATA, so it is skipped unless asked for:
    /// `cargo test --release -- --ignored download_from_a_local_link`
    #[test]
    #[ignore]
    fn download_from_a_local_link() {
        use std::io::{Read, Write};
        let tmp = std::env::temp_dir().join("larma-board-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("APPDATA", &tmp);
        let wav = tmp.join("src.wav");
        write_wav(&wav, 44_100, 0.4);
        let body = std::fs::read(&wav).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut c, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = c.read(&mut buf);
                let _ = c.write_all(format!("HTTP/1.1 200 OK
Content-Type: audio/wav
Content-Length: {}
Connection: close

", body.len()).as_bytes());
                let _ = c.write_all(&body);
            }
        });
        let sh = crate::engine::Shared::new(crate::settings::Params::default(), "t".into());
        add_url(&sh, &format!("http://127.0.0.1:{port}/Funny%20Honk.wav")).expect("download and add");
        let b = sh.board.lock();
        assert_eq!(b.sounds.len(), 1);
        assert_eq!(b.sounds[0].name, "Funny Honk");
        assert!(dir().join(&b.sounds[0].file).is_file());
        let bad = add_url(&sh, "ftp://example.com/x.mp3");
        assert!(bad.is_err());
    }
}
