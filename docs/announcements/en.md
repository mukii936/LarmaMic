# LarmaMic announcement (English)

> For: Reddit (r/rust, r/opensource, r/androidapps, r/discordapp), Hacker News (Show HN), Lobsters, forums.
> Read each community's self-promotion and AI-disclosure rules first. Post only after the release you mention is on the Releases page and you have tried the features you describe.

---

## Title ideas

- **Show HN:** LarmaMic – use your phone as a PC microphone (Rust, RNNoise, Wi-Fi/USB/Bluetooth)
- r/opensource: I built a free app that turns an iPhone or Android phone into a PC microphone, with AI noise removal
- r/rust: A Rust + egui app that streams phone audio to a Windows virtual microphone (RNNoise, WASAPI, axum)

## Main post

I built **LarmaMic**, a free, MIT-licensed app that turns a phone (iPhone or Android) into a microphone for a Windows PC.

**Why:** a phone mic is often better than a cheap USB mic, but existing apps are paid, give little control over the sound, or have weak noise removal.

**What it does**
- Connects over **Wi-Fi, a USB cable or Bluetooth**. With USB debugging on, plugging in the cable is enough on Android: no QR code.
- Processing chain at 48 kHz: **RNNoise** AI noise removal, noise gate with lookahead, 4-band EQ, AGC, compressor, limiter, plus voice effects.
- Outputs to a **virtual microphone** (VB-Cable or Steam Streaming Microphone) so Discord, OBS, games and calls can use it.
- Everything is tuned **on the PC**; the phone page/app shows the same controls and both stay in sync.
- **Several phones at once** (up to 4) with per-phone mute and gain, and a "loudest talker" mode.
- Android app keeps streaming with the screen off; iPhone needs no app (open the page in Safari after scanning a QR code).
- A single portable `larma-mic.exe` (~9 MB), about 0% CPU while hidden in the tray. 10 interface languages.

**Tech:** Rust (eframe/egui, cpal/WASAPI, axum + rustls, nnnoiseless), React + TypeScript for the phone page, Kotlin + Jetpack Compose for the Android app, GitHub Actions for builds and releases.

**Links**
- Repo: https://github.com/mukii936/LarmaMic
- Downloads: https://github.com/mukii936/LarmaMic/releases

**Honest notes**
- I built this **together with an AI assistant (Claude)**. I test, fix and maintain it, but I want that to be clear up front.
- The **Android app is new and has had little real-device testing**; bug reports with the device model are very welcome.
- **Echo cancellation is experimental**: verified with simulated echo paths, not yet in many real rooms.
- The exe is **not code-signed**, so SmartScreen may warn on first run. SHA-256 files are attached and the source is public.
- The translations were written with AI and have not been reviewed by native speakers.

Feedback, issues and pull requests are welcome. A star helps others find it.

---

## Show HN first comment (post right after submitting)

Author here. I wanted a free way to use my phone as a PC microphone with real control over the sound. The part I am least sure about is real-world behaviour across Android devices and rooms (echo cancellation in particular), so I would value reports from other setups. Architecture notes: the phone sends 16-bit PCM over a WebSocket (TLS, self-signed with a pairing code in the QR address); the PC resamples, mixes sources, runs the DSP chain and writes to a virtual audio device. Built with Claude's help; I am happy to answer questions about what was and was not verified.

---

## Short version (comments, Discord, Telegram)

I made **LarmaMic**: a free, open-source app that turns an iPhone/Android phone into a PC mic (Wi-Fi/USB/Bluetooth) with AI noise removal, EQ, several phones at once and a virtual-mic output for Discord/OBS/games. Built with an AI assistant; the Android app is new, so feedback is welcome: https://github.com/mukii936/LarmaMic

---

## Reply template when someone asks "how do I use my phone as a mic on PC?"

A few options. If you want something free with noise removal and EQ, you could try LarmaMic (I am the author, built with an AI assistant, so treat it as young software): it works over Wi-Fi, USB cable or Bluetooth and outputs to a virtual mic for Discord/OBS. Setup is in the README: https://github.com/mukii936/LarmaMic

## Tips

- Add a screenshot and a 15–30 second demo video or GIF: phone → virtual mic → Discord hears it.
- Post at a busy hour for that community and answer comments in the first hours.
- Do not post the same text everywhere; adapt it to each community's rules and tone.
- Do not ask for or buy stars; it is against GitHub's rules and gets projects flagged.
