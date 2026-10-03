//! Record mode, the live part: the microphone and the global key listener run side by side
//! and both are captured to memory only. Everything after capture happens in
//! `record::process`; this layer only moves data and cannot be tested without a microphone
//! and Input Monitoring permission (its pure pieces are tested below).
//!
//! Only microphones are opened. cpal silently turns an input stream on an output-only device
//! into system-audio (loopback) capture, so such devices are refused, as are well-known
//! loopback and monitor devices: this tool records keyboards, not other apps' audio.

use crate::record::{ClockMap, KeyMark};
use crate::signal::{self, Audio, CLIP_LEVEL, ClipRuns};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, FromSample, InputCallbackInfo, Sample, SampleFormat, SizedSample, StreamConfig,
    SupportedBufferSize, SupportedStreamConfig,
};
use rtrb::{Consumer, Producer, RingBuffer};
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use taktak_core::clock;
use taktak_core::input::{self, KeyAction, KeyEvent};
use taktak_core::key::Key;

/// Requested input buffer: small buffers mean dense capture timestamps.
const BUFFER_FRAMES: u32 = 256;
/// Audio the capture ring holds before the main thread must have drained it.
const RING_S: usize = 10;
const ANCHOR_RING: usize = 1 << 14;
const KEY_RING: usize = 1 << 13;
/// Upper bound on one session; about 115 MB of audio at 48 kHz.
pub const MAX_SECONDS: f64 = 600.0;
const POLL: Duration = Duration::from_millis(20);
const STATUS_EVERY: Duration = Duration::from_millis(100);
/// Three Escape presses within this window end the session.
const FINISH_PRESSES: usize = 3;
const FINISH_WINDOW_NS: u64 = 1_500_000_000;
/// This much audio of exact zeros means the OS is withholding the microphone.
const SILENCE_CHECK_S: f64 = 2.0;
/// No audio for this long means the device went away.
const STALL: Duration = Duration::from_secs(2);
/// Substrings (lowercase) of device names that capture system audio rather than a microphone.
const LOOPBACK_HINTS: [&str; 9] = [
    "monitor of",
    ".monitor",
    "loopback",
    "blackhole",
    "soundflower",
    "stereo mix",
    "what u hear",
    "vb-audio",
    "cable output",
];

pub struct CaptureOptions {
    /// Stop after this much audio; `None` records until Ctrl+C or Escape x3.
    pub seconds: Option<f64>,
    /// Input device name (case-insensitive, exact or unique substring); `None` is the default.
    pub device: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    Seconds,
    Escape,
    Interrupted,
    InputStopped,
}

pub struct Capture {
    pub audio: Audio,
    /// Key transitions on the recording's timeline, in memory only. The finishing Escape
    /// presses are already removed.
    pub marks: Vec<KeyMark>,
    pub ended: Ended,
}

impl std::fmt::Debug for Capture {
    /// Counts only: neither the audio nor the key events belong in any output.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capture")
            .field("frames", &self.audio.samples.len())
            .field("marks", &self.marks.len())
            .field("ended", &self.ended)
            .finish()
    }
}

pub fn record(opts: &CaptureOptions) -> Result<Capture, String> {
    check_input_monitoring()?;
    let device = pick_device(opts.device.as_deref())?;
    let supported = device
        .default_input_config()
        .map_err(|e| format!("cannot use input device {:?}: {e}", device_name(&device)))?;
    let rate = supported.sample_rate();
    eprintln!("microphone: {} ({rate} Hz, {} ch)", device_name(&device), supported.channels());
    if cfg!(target_os = "macos") {
        eprintln!(
            "The first time, macOS asks whether this terminal may use the microphone. If the \
             level meter stays empty, allow it in System Settings > Privacy & Security > \
             Microphone and run again."
        );
    }

    let (mut key_tx, mut key_rx) = RingBuffer::<KeyEvent>::new(KEY_RING);
    let listener = input::start(move |e| {
        // A full ring loses the event; the main thread drains it every 20 ms.
        let _ = key_tx.push(e);
    })
    .map_err(|e| e.to_string())?;

    let shared = Arc::new(Shared::default());
    let (frames_tx, mut frames_rx) = RingBuffer::new(rate as usize * RING_S);
    let (anchors_tx, mut anchors_rx) = RingBuffer::new(ANCHOR_RING);
    // Initializes the clock (a lazily set epoch on some platforms) off the audio thread.
    clock::now_ns();
    let feed = Feed {
        frames: frames_tx,
        anchors: anchors_tx,
        frame: 0,
        channels: supported.channels().max(1) as usize,
        shared: shared.clone(),
    };
    let stream = open_stream(&device, &supported, feed, shared.clone())?;
    stream.play().map_err(|e| format!("cannot start the microphone: {e}"))?;

    let limit_s = opts.seconds.unwrap_or(MAX_SECONDS).min(MAX_SECONDS);
    let limit = signal::frames(rate, limit_s);
    eprintln!(
        "\nRecording{}. Type each key slowly, 3-5 times, in any window.\n\
         Finish with Ctrl+C or by pressing Escape 3 times quickly.\n",
        opts.seconds.map(|s| format!(" for {s} s")).unwrap_or_default()
    );

    let mut session = Session::new(rate);
    let quiet = Terminal::quiet();
    interrupt::install();
    let started = Instant::now();
    let mut last_audio = started;
    let mut status = StatusLine::new(std::io::stderr().is_terminal(), started);
    if !status.live {
        eprintln!("(no live status: stderr is not a terminal)");
    }
    let mut silence_checked = false;
    let ended = loop {
        std::thread::sleep(POLL);
        if session.drain(&mut frames_rx, &mut anchors_rx, &mut key_rx) > 0 {
            last_audio = Instant::now();
        }
        if shared.overflow.load(Ordering::Relaxed) {
            break Err("the computer fell behind the microphone and audio was lost; \
                       close busy apps and try again"
                .to_owned());
        }
        if let Some(kind) = shared.error.lock().ok().and_then(|mut e| e.take()) {
            eprintln!("{}microphone warning: {kind}", status.clear());
        }
        if session.finish_ns.is_some() {
            break Ok(Ended::Escape);
        }
        if interrupt::requested() {
            break Ok(Ended::Interrupted);
        }
        if session.samples.len() >= limit {
            break Ok(Ended::Seconds);
        }
        if last_audio.elapsed() > STALL {
            break Ok(Ended::InputStopped);
        }
        let heard = session.samples.len() as f64 / rate as f64;
        if !silence_checked && heard >= SILENCE_CHECK_S {
            silence_checked = true;
            if session.samples.iter().all(|&s| s == 0.0) {
                eprintln!(
                    "{}the microphone delivers pure silence: allow Microphone access for this \
                     terminal (System Settings > Privacy & Security > Microphone)",
                    status.clear()
                );
            }
        }
        status.tick(&mut std::io::stderr(), Instant::now(), || {
            let peak = f32::from_bits(shared.peak.swap(0, Ordering::Relaxed));
            status_line(heard, session.presses, peak)
        });
    };
    drop(listener);
    drop(stream);
    interrupt::restore();
    drop(quiet);
    status.end(&mut std::io::stderr());
    let ended = ended?;
    session.drain(&mut frames_rx, &mut anchors_rx, &mut key_rx);
    session.samples.truncate(limit);
    if session.samples.is_empty() {
        return Err("the microphone delivered no audio; check that it is connected and that \
                    this terminal may use it"
            .to_owned());
    }
    if ended == Ended::InputStopped {
        eprintln!("the microphone stopped delivering audio; using what was recorded");
    }
    Ok(session.finish(ended))
}

fn check_input_monitoring() -> Result<(), String> {
    if input::has_permission() || input::request_permission() {
        return Ok(());
    }
    Err("pack-maker needs Input Monitoring permission to know which key made each sound \
         (key codes only; nothing you type is stored or sent anywhere).\n\
         Enable it for the app you run this from (Terminal, iTerm, VS Code, ...):\n  \
         open \"x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent\"\n\
         then quit and reopen that app and run this again."
        .to_owned())
}

fn device_name(d: &cpal::Device) -> String {
    d.description().map(|d| d.name().to_owned()).unwrap_or_else(|_| d.to_string())
}

fn pick_device(wanted: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    let device = match wanted {
        None => host.default_input_device().ok_or("no microphone found")?,
        Some(w) => {
            let devices: Vec<(String, cpal::Device)> = host
                .input_devices()
                .map_err(|e| format!("cannot list input devices: {e}"))?
                .map(|d| (device_name(&d), d))
                .collect();
            let names: Vec<&str> = devices.iter().map(|(n, _)| n.as_str()).collect();
            let i = match_name(&names, w).map_err(|why| {
                format!("--device {w:?}: {why}; input devices: {}", names.join(", "))
            })?;
            devices.into_iter().nth(i).map(|(_, d)| d).ok_or("device disappeared")?
        }
    };
    let name = device_name(&device);
    if !device.supports_input() || looks_like_loopback(&name) {
        return Err(format!(
            "{name:?} is not a microphone. pack-maker records a keyboard through a microphone; \
             it does not capture audio played by other apps. To use a recording you have the \
             rights to, run `pack-maker slice --input FILE.wav ...`."
        ));
    }
    Ok(device)
}

/// Exact (case-insensitive) match first, else a unique substring.
fn match_name(names: &[&str], wanted: &str) -> Result<usize, &'static str> {
    let w = wanted.to_lowercase();
    if let Some(i) = names.iter().position(|n| n.to_lowercase() == w) {
        return Ok(i);
    }
    let mut hits = names.iter().enumerate().filter(|(_, n)| n.to_lowercase().contains(&w));
    match (hits.next(), hits.next()) {
        (Some((i, _)), None) => Ok(i),
        (Some(_), Some(_)) => Err("matches more than one device"),
        (None, _) => Err("no such device"),
    }
}

fn looks_like_loopback(name: &str) -> bool {
    let n = name.to_lowercase();
    LOOPBACK_HINTS.iter().any(|h| n.contains(h))
}

/// State the audio callbacks share with the main thread.
#[derive(Default)]
struct Shared {
    /// Peak since the meter last read it, as `f32` bits (non-negative floats order like
    /// their bits, so `fetch_max` works).
    peak: AtomicU32,
    overflow: AtomicBool,
    /// The latest stream error. The error callback only ever `try_lock`s it, so it never
    /// blocks even when a backend reports errors from the audio thread.
    error: Mutex<Option<cpal::ErrorKind>>,
}

/// The audio callback's half: mono-mixes each buffer into the frame ring and records one
/// (first frame, capture time) anchor per buffer. No allocation, locks or I/O.
struct Feed {
    frames: Producer<(f32, bool)>,
    anchors: Producer<(u64, u64)>,
    /// Frames delivered so far, including any dropped on overflow.
    frame: u64,
    channels: usize,
    shared: Arc<Shared>,
}

impl Feed {
    fn push<T>(&mut self, data: &[T], info: &InputCallbackInfo)
    where
        T: Sample,
        f32: FromSample<T>,
    {
        let n = data.len() / self.channels;
        if self.frames.slots() < n || self.anchors.is_full() {
            self.shared.overflow.store(true, Ordering::Relaxed);
            self.frame += n as u64;
            return;
        }
        let _ = self.anchors.push((self.frame, capture_ns(info)));
        let mut peak = 0.0f32;
        for frame in data.chunks_exact(self.channels) {
            let mut sum = 0.0;
            let mut loudest = 0.0f32;
            for &s in frame {
                let v = f32::from_sample(s);
                sum += v;
                loudest = loudest.max(v.abs());
            }
            peak = peak.max(loudest);
            let _ = self.frames.push((sum / self.channels as f32, loudest >= CLIP_LEVEL));
        }
        self.frame += n as u64;
        self.shared.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
    }
}

/// When the buffer's first frame was captured, on the `taktak_core::clock` timebase.
/// CoreAudio stamps buffers with mach host time, which is that timebase already.
#[cfg(target_os = "macos")]
fn capture_ns(info: &InputCallbackInfo) -> u64 {
    info.timestamp().capture.as_nanos() as u64
}

/// Elsewhere the stream clock has its own origin, so the offset is re-established on every
/// callback from how long ago the capture happened.
#[cfg(not(target_os = "macos"))]
fn capture_ns(info: &InputCallbackInfo) -> u64 {
    let ts = info.timestamp();
    clock::now_ns().saturating_sub(ts.callback.duration_since(ts.capture).as_nanos() as u64)
}

fn open_stream(
    device: &cpal::Device,
    supported: &SupportedStreamConfig,
    feed: Feed,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, String> {
    let buffer_size = match supported.buffer_size() {
        SupportedBufferSize::Range { min, max } => {
            BufferSize::Fixed(BUFFER_FRAMES.clamp(*min, *max))
        }
        SupportedBufferSize::Unknown => BufferSize::Default,
    };
    let config = StreamConfig {
        channels: supported.channels(),
        sample_rate: supported.sample_rate(),
        buffer_size,
    };
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(device, config, feed, shared),
        SampleFormat::F64 => build::<f64>(device, config, feed, shared),
        SampleFormat::I8 => build::<i8>(device, config, feed, shared),
        SampleFormat::I16 => build::<i16>(device, config, feed, shared),
        SampleFormat::I32 => build::<i32>(device, config, feed, shared),
        SampleFormat::U8 => build::<u8>(device, config, feed, shared),
        SampleFormat::U16 => build::<u16>(device, config, feed, shared),
        SampleFormat::U32 => build::<u32>(device, config, feed, shared),
        other => return Err(format!("unsupported microphone sample format {other}")),
    };
    stream.map_err(|e| format!("cannot open the microphone: {e}"))
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut feed: Feed,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device.build_input_stream::<T, _, _>(
        config,
        move |data: &[T], info: &InputCallbackInfo| feed.push(data, info),
        move |e| {
            if let Ok(mut slot) = shared.error.try_lock() {
                *slot = Some(e.kind());
            }
        },
        None,
    )
}

/// The main thread's half: everything captured so far, in memory.
struct Session {
    rate: u32,
    samples: Vec<f32>,
    clipped: Vec<std::ops::Range<usize>>,
    runs: ClipRuns,
    clock: ClockMap,
    events: Vec<KeyEvent>,
    /// Key-down count for the status line.
    presses: usize,
    finish: FinishGesture,
    /// Time of the first of the finishing Escape presses, once they happened.
    finish_ns: Option<u64>,
}

impl Session {
    fn new(rate: u32) -> Session {
        Session {
            rate,
            samples: Vec::with_capacity(rate as usize * 60),
            clipped: Vec::new(),
            runs: ClipRuns::default(),
            clock: ClockMap::new(rate),
            events: Vec::new(),
            presses: 0,
            finish: FinishGesture::default(),
            finish_ns: None,
        }
    }

    /// Moves everything the rings hold into the session; returns the number of new frames.
    fn drain(
        &mut self,
        frames: &mut Consumer<(f32, bool)>,
        anchors: &mut Consumer<(u64, u64)>,
        keys: &mut Consumer<KeyEvent>,
    ) -> usize {
        let before = self.samples.len();
        while let Ok((s, clip)) = frames.pop() {
            let i = self.samples.len();
            self.clipped.extend(self.runs.feed(i, clip));
            self.samples.push(s);
        }
        while let Ok((frame, ns)) = anchors.pop() {
            self.clock.push(frame, ns);
        }
        while let Ok(e) = keys.pop() {
            // Keys pressed after the finishing gesture are not part of the session.
            if self.finish_ns.is_some() {
                continue;
            }
            if e.action == KeyAction::Down {
                self.presses += 1;
            }
            self.finish_ns = self.finish.feed(&e);
            self.events.push(e);
        }
        self.samples.len() - before
    }

    fn finish(mut self, ended: Ended) -> Capture {
        let len = self.samples.len();
        self.clipped.extend(self.runs.finish(len));
        self.clipped.retain(|r| r.start < len);
        if let Some(last) = self.clipped.last_mut() {
            last.end = last.end.min(len);
        }
        if let Some(from) = self.finish_ns {
            strip_finish_gesture(&mut self.events, from);
        }
        let marks = to_marks(&self.events, &self.clock, len);
        let audio = Audio { rate: self.rate, samples: self.samples, clipped: self.clipped };
        Capture { audio, marks, ended }
    }
}

/// Detects Escape pressed [`FINISH_PRESSES`] times within [`FINISH_WINDOW_NS`].
#[derive(Default)]
struct FinishGesture {
    downs: Vec<u64>,
}

impl FinishGesture {
    /// Feeds one event; returns the time of the gesture's first press once it is complete.
    fn feed(&mut self, e: &KeyEvent) -> Option<u64> {
        if e.key != Key::Escape || e.action != KeyAction::Down {
            return None;
        }
        self.downs.push(e.event_ns);
        let first = self.downs.len().checked_sub(FINISH_PRESSES)?;
        let from = self.downs[first];
        (e.event_ns.saturating_sub(from) <= FINISH_WINDOW_NS).then_some(from)
    }
}

/// Removes the finishing Escape presses (and their releases) so they never become samples.
fn strip_finish_gesture(events: &mut Vec<KeyEvent>, from_ns: u64) {
    events.retain(|e| e.key != Key::Escape || e.event_ns < from_ns);
}

/// Places key events on the recording's timeline, dropping any outside it.
fn to_marks(events: &[KeyEvent], clock: &ClockMap, len: usize) -> Vec<KeyMark> {
    events
        .iter()
        .filter_map(|e| {
            let frame = clock.frame_at(e.event_ns)?;
            (0.0..len as f64).contains(&frame).then_some(KeyMark {
                key: e.key,
                action: e.action,
                frame,
            })
        })
        .collect()
}

/// The live status line. It is drawn only when stderr is a terminal, where every refresh
/// overwrites the previous one. Redirected to a file or a pipe, every refresh would be kept,
/// and that series of keystroke counts would record the typing rhythm.
struct StatusLine {
    live: bool,
    last: Instant,
}

impl StatusLine {
    fn new(live: bool, now: Instant) -> StatusLine {
        StatusLine { live, last: now }
    }

    /// Starts a one-off message: on a terminal it replaces the status line instead of
    /// following it on the same line.
    fn clear(&self) -> &'static str {
        if self.live { "\r\x1b[2K" } else { "" }
    }

    /// Redraws the line at most every [`STATUS_EVERY`]. `line` runs only when it is drawn.
    fn tick(&mut self, out: &mut impl Write, now: Instant, line: impl FnOnce() -> String) {
        if !self.live || now.duration_since(self.last) < STATUS_EVERY {
            return;
        }
        self.last = now;
        let _ = write!(out, "\r\x1b[2K{}", line());
        let _ = out.flush();
    }

    /// Moves past the status line once recording ends.
    fn end(&self, out: &mut impl Write) {
        if self.live {
            let _ = writeln!(out);
        }
    }
}

/// Seconds, key-down count and input level. Never which keys.
fn status_line(secs: f64, presses: usize, peak: f32) -> String {
    const WIDTH: usize = 20;
    const FLOOR_DB: f32 = -60.0;
    let db = signal::amp_db(peak);
    let filled = (((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0) * WIDTH as f32).round() as usize;
    let level = if peak >= CLIP_LEVEL { "CLIP!".to_owned() } else { format!("{db:>4.0} dB") };
    format!(
        "REC {secs:6.1} s | keystrokes {presses:4} | level [{}{}] {level}",
        "#".repeat(filled),
        "-".repeat(WIDTH - filled)
    )
}

/// Ctrl+C ends the session gracefully, keeping what was recorded.
#[cfg(unix)]
mod interrupt {
    use std::sync::atomic::{AtomicBool, Ordering};

    static REQUESTED: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_sigint(_: libc::c_int) {
        REQUESTED.store(true, Ordering::Relaxed);
    }

    pub fn install() {
        let handler = on_sigint as extern "C" fn(libc::c_int);
        // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
        unsafe { libc::signal(libc::SIGINT, handler as *const () as libc::sighandler_t) };
    }

    /// A second Ctrl+C during processing then quits immediately.
    pub fn restore() {
        // SAFETY: restoring the default disposition.
        unsafe { libc::signal(libc::SIGINT, libc::SIG_DFL) };
    }

    pub fn requested() -> bool {
        REQUESTED.load(Ordering::Relaxed)
    }
}

#[cfg(not(unix))]
mod interrupt {
    pub fn install() {}
    pub fn restore() {}
    pub fn requested() -> bool {
        false
    }
}

/// While recording, the terminal neither echoes nor buffers lines, so keys typed into it do
/// not appear on screen; on drop, whatever was typed is discarded (so the shell never runs
/// it) and the original settings return.
struct Terminal {
    #[cfg(unix)]
    saved: Option<libc::termios>,
}

impl Terminal {
    #[cfg(unix)]
    fn quiet() -> Terminal {
        // SAFETY: plain termios calls on stdin with a zeroed, then OS-filled, struct.
        let saved = unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::isatty(libc::STDIN_FILENO) == 1
                && libc::tcgetattr(libc::STDIN_FILENO, &mut t) == 0
            {
                let mut quiet = t;
                quiet.c_lflag &= !(libc::ECHO | libc::ICANON);
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &quiet);
                Some(t)
            } else {
                None
            }
        };
        Terminal { saved }
    }

    #[cfg(not(unix))]
    fn quiet() -> Terminal {
        Terminal {}
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(t) = &self.saved {
            // SAFETY: restores the settings read in `quiet`.
            unsafe {
                libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::{InputStreamTimestamp, StreamInstant};

    fn event(key: Key, action: KeyAction, ms: u64) -> KeyEvent {
        let ns = 10_000_000_000 + ms * 1_000_000;
        KeyEvent { key, action, event_ns: ns, received_ns: ns }
    }

    fn info(capture_ns: u64) -> InputCallbackInfo {
        let capture = StreamInstant::from_nanos(capture_ns);
        let callback = StreamInstant::from_nanos(capture_ns + 5_000_000);
        InputCallbackInfo::new(InputStreamTimestamp { callback, capture })
    }

    #[test]
    fn finish_gesture_needs_three_quick_escapes() {
        let mut g = FinishGesture::default();
        use KeyAction::*;
        assert_eq!(g.feed(&event(Key::Escape, Down, 0)), None);
        assert_eq!(g.feed(&event(Key::Escape, Up, 50)), None);
        assert_eq!(g.feed(&event(Key::KeyA, Down, 100)), None);
        assert_eq!(g.feed(&event(Key::Escape, Down, 1_000)), None);
        // Third press 2 s after the first: too slow.
        assert_eq!(g.feed(&event(Key::Escape, Down, 2_000)), None);
        // But the last three (1.0 s, 2.0 s, 2.4 s) are within 1.5 s.
        let done = g.feed(&event(Key::Escape, Down, 2_400));
        assert_eq!(done, Some(event(Key::Escape, Down, 1_000).event_ns));
    }

    #[test]
    fn finishing_escapes_are_stripped_but_earlier_ones_kept() {
        use KeyAction::*;
        let mut events = vec![
            event(Key::Escape, Down, 0),
            event(Key::Escape, Up, 80),
            event(Key::KeyA, Down, 500),
            event(Key::Escape, Down, 3_000),
            event(Key::KeyA, Up, 3_050),
            event(Key::Escape, Up, 3_100),
            event(Key::Escape, Down, 3_300),
        ];
        strip_finish_gesture(&mut events, event(Key::Escape, Down, 3_000).event_ns);
        let kept: Vec<(bool, u64)> = events
            .iter()
            .map(|e| (e.key == Key::Escape, e.event_ns / 1_000_000 - 10_000))
            .collect();
        assert_eq!(kept, [(true, 0), (true, 80), (false, 500), (false, 3_050)]);
    }

    #[test]
    fn callback_feed_mixes_down_anchors_and_flags_clipping() {
        let shared = Arc::new(Shared::default());
        let (frames_tx, mut frames_rx) = RingBuffer::new(64);
        let (anchors_tx, mut anchors_rx) = RingBuffer::new(8);
        let (_, mut keys_rx) = RingBuffer::<KeyEvent>::new(1);
        let mut feed = Feed {
            frames: frames_tx,
            anchors: anchors_tx,
            frame: 0,
            channels: 2,
            shared: shared.clone(),
        };
        // 16-bit stereo; the right channel clips on the second frame.
        feed.push(&[16_384i16, 0, 0, i16::MAX], &info(1_000_000_000));
        feed.push(&[-16_384i16, -16_384], &info(1_000_041_667));
        let mut session = Session::new(48_000);
        assert_eq!(session.drain(&mut frames_rx, &mut anchors_rx, &mut keys_rx), 3);
        assert_eq!(session.samples.len(), 3);
        assert!((session.samples[0] - 0.25).abs() < 1e-4);
        assert!((session.samples[2] + 0.5).abs() < 1e-4);
        assert_eq!(f32::from_bits(shared.peak.load(Ordering::Relaxed)), 1.0 - 1.0 / 32_768.0);
        assert!(!shared.overflow.load(Ordering::Relaxed));
        let capture = session.finish(Ended::Seconds);
        assert_eq!(capture.audio.clipped, vec![1..2]);

        // A buffer that does not fit is dropped whole and reported.
        feed.push(&[0i16; 200], &info(1_000_100_000));
        assert!(shared.overflow.load(Ordering::Relaxed));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn events_land_on_the_frames_captured_at_the_same_instant() {
        let shared = Arc::new(Shared::default());
        let (frames_tx, mut frames_rx) = RingBuffer::new(48_000);
        let (anchors_tx, mut anchors_rx) = RingBuffer::new(1_024);
        let (mut keys_tx, mut keys_rx) = RingBuffer::new(16);
        let mut feed =
            Feed { frames: frames_tx, anchors: anchors_tx, frame: 0, channels: 1, shared };
        let t0 = 10_000_000_000u64;
        for b in 0..100u64 {
            feed.push(&[0.0f32; 256], &info(t0 + b * 256 * 1_000_000_000 / 48_000));
        }
        // 250 ms in is frame 12,000. An event before the recording started is dropped.
        keys_tx.push(event(Key::KeyA, KeyAction::Down, 250)).unwrap();
        keys_tx.push(event(Key::KeyB, KeyAction::Down, 1)).unwrap();
        keys_tx
            .push(KeyEvent { event_ns: t0 - 10_000_000, ..event(Key::KeyC, KeyAction::Down, 0) })
            .unwrap();
        let mut session = Session::new(48_000);
        session.drain(&mut frames_rx, &mut anchors_rx, &mut keys_rx);
        assert_eq!(session.presses, 3);
        let capture = session.finish(Ended::Interrupted);
        assert_eq!(capture.marks.len(), 2);
        assert!((capture.marks[0].frame - 12_000.0).abs() < 0.5, "{}", capture.marks[0].frame);
        assert!((capture.marks[1].frame - 48.0).abs() < 0.5);
    }

    #[test]
    fn keys_after_the_finish_gesture_are_ignored() {
        let (_, mut frames_rx) = RingBuffer::<(f32, bool)>::new(1);
        let (_, mut anchors_rx) = RingBuffer::<(u64, u64)>::new(1);
        let (mut keys_tx, mut keys_rx) = RingBuffer::new(16);
        for ms in [0, 200, 400] {
            keys_tx.push(event(Key::Escape, KeyAction::Down, ms)).unwrap();
        }
        keys_tx.push(event(Key::KeyA, KeyAction::Down, 600)).unwrap();
        let mut session = Session::new(48_000);
        session.drain(&mut frames_rx, &mut anchors_rx, &mut keys_rx);
        assert!(session.finish_ns.is_some());
        assert_eq!(session.events.len(), 3);
        strip_finish_gesture(&mut session.events, session.finish_ns.unwrap_or_default());
        assert!(session.events.is_empty());
    }

    #[test]
    fn device_matching_and_loopback_refusal() {
        let names = ["MacBook Pro Microphone", "USB Audio Device", "USB Mic"];
        assert_eq!(match_name(&names, "usb mic"), Ok(2));
        assert_eq!(match_name(&names, "macbook"), Ok(0));
        assert!(match_name(&names, "usb").is_err());
        assert!(match_name(&names, "webcam").is_err());
        assert!(looks_like_loopback("BlackHole 2ch"));
        assert!(looks_like_loopback("Monitor of Built-in Audio Analog Stereo"));
        assert!(looks_like_loopback("Stereo Mix (Realtek Audio)"));
        assert!(!looks_like_loopback("MacBook Pro Microphone"));
    }

    #[test]
    fn status_is_never_written_when_stderr_is_not_a_terminal() {
        let t0 = Instant::now();
        let mut out = Vec::new();
        let mut status = StatusLine::new(false, t0);
        for tick in 1..=600u64 {
            let now = t0 + STATUS_EVERY * tick as u32;
            status.tick(&mut out, now, || panic!("the line is built only to be drawn"));
        }
        status.end(&mut out);
        assert!(out.is_empty(), "{:?}", String::from_utf8_lossy(&out));
        assert_eq!(status.clear(), "");

        // On a terminal it redraws in place, at most every STATUS_EVERY.
        let mut status = StatusLine::new(true, t0);
        let mut drawn = 0;
        for ms in (0..1_000u64).step_by(20) {
            status.tick(&mut out, t0 + Duration::from_millis(ms), || {
                drawn += 1;
                format!("line {drawn}")
            });
        }
        status.end(&mut out);
        assert_eq!(drawn, 9);
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("\r\x1b[2Kline 1\r\x1b[2Kline 2"), "{text:?}");
        assert!(text.ends_with("line 9\n"), "{text:?}");
        assert_eq!(text.matches('\n').count(), 1);
    }

    #[test]
    fn status_line_shows_counts_and_level_only() {
        let s = status_line(12.34, 42, 0.1);
        assert!(
            s.contains("12.3 s") && s.contains("keystrokes   42") && s.contains("-20 dB"),
            "{s}"
        );
        assert!(status_line(1.0, 0, 1.0).contains("CLIP"));
        assert!(status_line(1.0, 0, 0.0).contains("[--------------------]"));
    }
}
