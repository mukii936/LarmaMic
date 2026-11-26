//! Text to speech: the sentence is spoken by a Windows voice (System.Speech, through PowerShell), written to a temporary
//! WAV file, decoded and played into the microphone like a soundboard sound. Nothing leaves the PC.

use crate::engine::Shared;
use crate::soundboard::{self, Playing};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

pub const MAX_CHARS: usize = 400;

/// Names of the voices installed in Windows (for the chooser). Empty if PowerShell cannot list them.
pub fn voices() -> Vec<String> {
    let script = "Add-Type -AssemblyName System.Speech;$s=New-Object System.Speech.Synthesis.SpeechSynthesizer;\
                  $s.GetInstalledVoices()|Where-Object{$_.Enabled}|ForEach-Object{[Console]::Out.WriteLine($_.VoiceInfo.Name)}";
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Cleans the text before it reaches the voice: control characters out, length capped.
pub fn clean_text(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\n').collect::<String>().trim().chars().take(MAX_CHARS).collect()
}

fn speak_to_file(text: &str, voice: &str, rate: i32, out: &std::path::Path) -> Result<(), String> {
    let txt = out.with_extension("txt");
    std::fs::write(&txt, text).map_err(|e| e.to_string())?;
    // the text and the voice name travel in files / environment variables, never inside the script, so they cannot inject code
    let script = "Add-Type -AssemblyName System.Speech;$s=New-Object System.Speech.Synthesis.SpeechSynthesizer;\
                  if($env:LARMA_VOICE){try{$s.SelectVoice($env:LARMA_VOICE)}catch{}};\
                  $s.Rate=[int]$env:LARMA_RATE;$s.SetOutputToWaveFile($env:LARMA_OUT);\
                  $s.Speak([IO.File]::ReadAllText($env:LARMA_TEXT,[Text.Encoding]::UTF8));$s.Dispose()";
    let res = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("LARMA_VOICE", voice)
        .env("LARMA_RATE", rate.clamp(-5, 5).to_string())
        .env("LARMA_OUT", out)
        .env("LARMA_TEXT", &txt)
        .creation_flags(0x0800_0000)
        .output();
    let _ = std::fs::remove_file(&txt);
    let o = res.map_err(|e| e.to_string())?;
    if !o.status.success() || !out.is_file() {
        let msg = String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("").trim().to_string();
        return Err(if msg.is_empty() { "the voice could not speak".to_string() } else { msg });
    }
    Ok(())
}

/// Speaks `text` into the microphone on a worker thread. Errors land in `tts_msg`.
pub fn say_async(sh: Arc<Shared>, text: String) {
    let text = clean_text(&text);
    if text.is_empty() || sh.tts_busy.swap(true, Relaxed) {
        return;
    }
    sh.wake_ui();
    let _ = std::thread::Builder::new().name("tts".into()).spawn(move || {
        let tag = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() % 10_000_000).unwrap_or(0);
        let wav = std::env::temp_dir().join(format!("larma-tts-{tag}.wav"));
        let voice = sh.tts_voice.lock().clone();
        let rate = sh.tts_rate.load(Relaxed);
        let res = speak_to_file(&text, &voice, rate, &wav).and_then(|_| soundboard::decode(&wav));
        let _ = std::fs::remove_file(&wav);
        match res {
            Ok(buf) => {
                *sh.tts_msg.lock() = None;
                let end = buf.len();
                let mut pl = sh.playing.lock();
                if pl.len() >= 16 {
                    pl.remove(0);
                }
                pl.push(Playing { id: None, buf: Arc::new(buf), pos: 0, start: 0, end, vol: 1.0, looping: false });
            }
            Err(e) => *sh.tts_msg.lock() = Some(crate::i18n::tf("tts.err", &[&e])),
        }
        sh.tts_busy.store(false, Relaxed);
        sh.wake_ui();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_cleaned_and_capped() {
        assert_eq!(clean_text("  hi\u{7} there \r"), "hi there");
        assert_eq!(clean_text(&"a".repeat(1000)).chars().count(), MAX_CHARS);
        assert_eq!(clean_text("\u{1}\u{2}"), "");
    }

    /// Needs a Windows voice and PowerShell; skipped unless asked for:
    /// `cargo test --release -- --ignored speaks_a_sentence`
    #[test]
    #[ignore]
    fn speaks_a_sentence() {
        let wav = std::env::temp_dir().join("larma-tts-test.wav");
        speak_to_file("Hello from LarmaMic", "", 0, &wav).expect("speak");
        let v = soundboard::decode(&wav).expect("decode");
        let _ = std::fs::remove_file(&wav);
        assert!(v.len() > 24_000, "only {} samples", v.len());
    }
}
