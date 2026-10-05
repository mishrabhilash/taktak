//! Low-latency output engine: a cpal stream whose callback drains lock-free rings and runs
//! the [`Mixer`].
//!
//! Threads talking to the callback:
//! - the input hook thread, via [`TriggerSender`] (one SPSC ring, wait-free push). It only
//!   forwards key events; the mixer chooses and humanizes the sound.
//! - the control side (UI / pack loader), via [`Engine`]: bank swaps and previews over a
//!   command ring; gains, the variant mode and the humanize amount through atomics.
//!
//! The callback sends back latency samples and everything it replaced (banks, preview clips),
//! so nothing is ever freed or logged on the real-time thread. The stream's error callback can
//! run on that thread too (macOS reports overloads from the IO thread), so it only counts and
//! flags into atomics; the control side reads them with [`Engine::take_xruns`],
//! [`Engine::take_stream_fault`] and [`Engine::take_rerouted`] and does the logging and
//! recovery.

mod bank;
mod device_name;
mod mixer;

pub use bank::{SoundBank, SoundMap, Variation};
pub use mixer::{
    DEFAULT_HUMANIZE, MAX_GAIN, MAX_VOICES, Mixer, Trigger, VariantMode, consistent_index,
};

use crate::clock;
use crate::input::KeyAction;
use crate::latency::LatencySample;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, SampleFormat, SizedSample, StreamConfig, SupportedBufferSize};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::time::{Duration, Instant};

const TRIGGER_RING: usize = 256;
const COMMAND_RING: usize = 64;
/// Invariant: the callback's garbage push never finds this ring full. Each command hands back
/// at most one allocation, and the control side drains this ring before every push. When a
/// drain sees it empty, the callback can still hand back up to `COMMAND_RING` queued commands
/// plus one it has popped but not finished (popping frees the command slot, so the queue can
/// be full again meanwhile). `ready()` checks for a free slot only after the drain, so one more
/// command can be pushed before the next drain: `COMMAND_RING + 2` in total. The test
/// `garbage_ring_holds_the_worst_interleaving` steps through that case.
const GARBAGE_RING: usize = COMMAND_RING + 2;
const METRICS_RING: usize = 1024;
/// Mix buffer for non-f32 devices, in frames; larger callbacks are mixed in chunks.
const SCRATCH_FRAMES: usize = 4096;

enum Command {
    ReplaceBank(Box<SoundBank>),
    Preview(Box<[f32]>),
    StopPreview,
}

/// Allocations the callback let go of, returned to be dropped on the control side (or, for
/// the last bank, handed back by [`Engine::stop`]).
#[allow(dead_code, reason = "clips are never read, only dropped")]
enum Garbage {
    Bank(Box<SoundBank>),
    Clip(Box<[f32]>),
}

/// Settings the callback picks up at the start of every buffer: the gains and the humanize
/// amount as `f32` bits, the variant mode as a code. Atomics rather than commands, so a dragged
/// slider can never fill the command ring and crowd out a bank swap.
struct Settings {
    master: AtomicU32,
    press: AtomicU32,
    release: AtomicU32,
    humanize: AtomicU32,
    variant_mode: AtomicU8,
}

impl Settings {
    fn new(config: &EngineConfig) -> Settings {
        let one = || AtomicU32::new(1.0f32.to_bits());
        Settings {
            master: one(),
            press: one(),
            release: one(),
            humanize: AtomicU32::new(config.humanize.to_bits()),
            variant_mode: AtomicU8::new(mode_code(config.variant_mode)),
        }
    }
}

fn store(a: &AtomicU32, value: f32) {
    a.store(value.to_bits(), Ordering::Relaxed);
}

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

const fn mode_code(mode: VariantMode) -> u8 {
    match mode {
        VariantMode::Consistent => 0,
        VariantMode::Random => 1,
    }
}

fn load_mode(a: &AtomicU8) -> VariantMode {
    match a.load(Ordering::Relaxed) {
        1 => VariantMode::Random,
        _ => VariantMode::Consistent,
    }
}

/// Why the output stream stopped working. Any of these means the stream is silent or playing
/// to a device the user no longer chose: drop the [`Engine`] and start a new one, which opens
/// the current default device at its own rate (decode the pack again at that rate). When there
/// is no output device at all, retry later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamFault {
    /// The backend says the stream must be rebuilt (on Windows: the default device changed,
    /// and the stream keeps playing to the old one).
    Invalidated,
    /// The device went away (unplugged, or no default output device any more).
    DeviceGone,
    /// Any other backend failure.
    Failed,
}

impl StreamFault {
    const fn code(self) -> u8 {
        match self {
            StreamFault::Invalidated => 1,
            StreamFault::DeviceGone => 2,
            StreamFault::Failed => 3,
        }
    }

    fn from_code(code: u8) -> Option<StreamFault> {
        match code {
            0 => None,
            1 => Some(StreamFault::Invalidated),
            2 => Some(StreamFault::DeviceGone),
            _ => Some(StreamFault::Failed),
        }
    }

    /// The fault a stream error means, if any. Glitches (xruns), automatic rerouting to a new
    /// default device (see [`Engine::take_rerouted`]) and a refused real-time priority leave
    /// the stream running.
    fn of(kind: cpal::ErrorKind) -> Option<StreamFault> {
        use cpal::ErrorKind;
        match kind {
            ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => None,
            ErrorKind::StreamInvalidated => Some(StreamFault::Invalidated),
            ErrorKind::DeviceNotAvailable => Some(StreamFault::DeviceGone),
            // On WASAPI any other error ends the stream thread.
            _ => Some(StreamFault::Failed),
        }
    }
}

/// What the stream's error callback saw, shared with the control side.
struct StreamHealth {
    xruns: AtomicU32,
    /// The worst [`StreamFault`] since it was last taken, as its code (0 = none). Worst wins,
    /// so a later "rerouted" notice cannot hide that the device went away.
    fault: AtomicU8,
    /// The backend moved the stream to a new default device since this was last taken. Kept
    /// apart from `fault`: the stream still plays, so it never hides or outranks a fault.
    rerouted: AtomicBool,
}

impl StreamHealth {
    fn new() -> StreamHealth {
        StreamHealth {
            xruns: AtomicU32::new(0),
            fault: AtomicU8::new(0),
            rerouted: AtomicBool::new(false),
        }
    }

    /// The error callback. It may run on the real-time IO thread: atomics only, no logging,
    /// formatting or allocation. (Dropping `error` frees nothing for the errors raised there,
    /// which carry no message.)
    fn record(&self, error: cpal::Error) {
        let kind = error.kind();
        if kind == cpal::ErrorKind::Xrun {
            self.xruns.fetch_add(1, Ordering::Relaxed);
        } else if kind == cpal::ErrorKind::DeviceChanged {
            self.rerouted.store(true, Ordering::Relaxed);
        } else if let Some(fault) = StreamFault::of(kind) {
            self.fault.fetch_max(fault.code(), Ordering::Relaxed);
        }
    }

    fn take_xruns(&self) -> u32 {
        self.xruns.swap(0, Ordering::Relaxed)
    }

    fn take_fault(&self) -> Option<StreamFault> {
        StreamFault::from_code(self.fault.swap(0, Ordering::Relaxed))
    }

    fn take_rerouted(&self) -> bool {
        self.rerouted.swap(false, Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct AudioError(pub String);

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "audio: {}", self.0)
    }
}

impl std::error::Error for AudioError {}

fn err(e: impl std::fmt::Display) -> AudioError {
    AudioError(e.to_string())
}

#[derive(Clone, Copy, Debug)]
pub struct EngineConfig {
    /// Requested callback size in frames (clamped to what the device allows).
    /// `None` keeps the device default, which is usually much larger.
    pub buffer_frames: Option<u32>,
    /// Report per-keystroke latency samples (see [`Engine::take_metrics`]).
    pub measure_latency: bool,
    /// Initial [`Engine::set_variant_mode`].
    pub variant_mode: VariantMode,
    /// Initial [`Engine::set_humanize`].
    pub humanize: f32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            buffer_frames: Some(64),
            measure_latency: false,
            variant_mode: VariantMode::default(),
            humanize: DEFAULT_HUMANIZE,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// The buffer size actually requested, or `None` if the device default was used.
    pub buffer_frames: Option<u32>,
}

/// Wait-free handle for the input thread. Dropping triggers when the ring is full is
/// preferable to blocking the OS hook.
pub struct TriggerSender {
    tx: Producer<Trigger>,
}

impl TriggerSender {
    pub fn send(&mut self, t: Trigger) -> bool {
        self.tx.push(t).is_ok()
    }
}

/// The running output stream plus its control handle. `Send` (asserted below) but not `Sync`:
/// an app that shares it between threads keeps it behind a mutex.
pub struct Engine {
    _stream: cpal::Stream,
    ctl: Control,
    info: DeviceInfo,
    health: Arc<StreamHealth>,
}

// The app (Milestone 3) holds the engine in shared state; keep that possible on every target.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Engine>();
    assert_send::<TriggerSender>();
};

/// The default output device and how [`Engine::start`] would open it.
struct Output {
    device: cpal::Device,
    sample_format: SampleFormat,
    stream_config: StreamConfig,
    info: DeviceInfo,
}

impl Output {
    fn default(buffer_frames: Option<u32>) -> Result<Output, AudioError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| err("no output device"))?;
        // Not `description()`: on macOS that also queries the device's input side.
        let name = device_name::of(&device);
        let supported = device.default_output_config().map_err(err)?;

        let buffer_frames = buffer_frames.map(|want| match supported.buffer_size() {
            SupportedBufferSize::Range { min, max } => want.clamp(*min, *max),
            SupportedBufferSize::Unknown => want,
        });
        let stream_config = StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: buffer_frames.map_or(BufferSize::Default, BufferSize::Fixed),
        };
        let info = DeviceInfo {
            name,
            sample_rate: stream_config.sample_rate,
            channels: stream_config.channels,
            buffer_frames,
        };
        Ok(Output { device, sample_format: supported.sample_format(), stream_config, info })
    }
}

/// The default output device and the format [`Engine::start`] would open it with (for
/// `buffer_frames`, see [`EngineConfig::buffer_frames`]), without opening a stream: for showing
/// the device while no engine runs. Opening may still pick another device if the default
/// changes in between.
pub fn default_output(buffer_frames: Option<u32>) -> Result<DeviceInfo, AudioError> {
    Output::default(buffer_frames).map(|output| output.info)
}

impl Engine {
    /// Opens the default output device. `make_bank` receives the device sample rate so
    /// samples can be resampled once, up front.
    pub fn start(
        config: EngineConfig,
        make_bank: impl FnOnce(u32) -> SoundBank,
    ) -> Result<(Engine, TriggerSender), AudioError> {
        let Output { device, sample_format, stream_config, info } =
            Output::default(config.buffer_frames)?;

        // The clock initializes its timebase on first use; do that here, not in the callback.
        let _ = clock::now_ns();
        let mixer = Mixer::new(Box::new(make_bank(info.sample_rate)));
        let (ctl, triggers, state) = connect(mixer, info.channels as usize, &config);

        let health = Arc::new(StreamHealth::new());
        let stream = match sample_format {
            SampleFormat::F32 => build::<f32>(&device, &stream_config, state, health.clone()),
            SampleFormat::I16 => build::<i16>(&device, &stream_config, state, health.clone()),
            SampleFormat::I32 => build::<i32>(&device, &stream_config, state, health.clone()),
            SampleFormat::U16 => build::<u16>(&device, &stream_config, state, health.clone()),
            other => return Err(err(format!("unsupported sample format {other:?}"))),
        }?;
        stream.play().map_err(err)?;

        Ok((Engine { _stream: stream, ctl, info, health }, triggers))
    }

    /// Buffer under/overruns (audible glitches) since the last call. Poll it from a timer on
    /// the control side and log there; the stream's error callback cannot.
    pub fn take_xruns(&self) -> u32 {
        self.health.take_xruns()
    }

    /// Whether the stream stopped working since the last call (the worst fault, if several).
    /// Poll it from a timer; on `Some`, rebuild the engine (see [`StreamFault`]).
    pub fn take_stream_fault(&self) -> Option<StreamFault> {
        self.health.take_fault()
    }

    /// Whether the backend moved the stream to a new default output device since the last
    /// call (macOS does this by itself when the user switches outputs, e.g. plugs in
    /// headphones). The stream keeps playing, but [`Engine::info`] still describes the old
    /// device and the requested buffer size may no longer apply: rebuild the engine to open the
    /// new device at its own rate and buffer size.
    pub fn take_rerouted(&self) -> bool {
        self.health.take_rerouted()
    }

    /// Stops the stream and hands back the bank it was playing, so the next engine (after a
    /// pause, or on a new device with the same rate) can start with it instead of decoding the
    /// pack again. Waits up to `timeout` for the callback to give the bank up; `None` if it does
    /// not (a stream that has failed no longer calls back). The stream stops either way.
    pub fn stop(mut self, timeout: Duration) -> Option<SoundBank> {
        let bank = self.ctl.take_bank(timeout);
        drop(self);
        bank
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// Overall volume, previews included (0 = mute, 1 = unchanged, at most [`MAX_GAIN`]).
    /// Applied from the next audio buffer.
    pub fn set_master_gain(&self, gain: f32) {
        self.ctl.set_master_gain(gain);
    }

    /// Volume of key-down sounds started from the next audio buffer on.
    pub fn set_press_gain(&self, gain: f32) {
        self.ctl.set_press_gain(gain);
    }

    /// Volume of key-up sounds started from the next audio buffer on.
    pub fn set_release_gain(&self, gain: f32) {
        self.ctl.set_release_gain(gain);
    }

    /// How keys pick among their candidate sounds (see [`VariantMode`]), for sounds started
    /// from the next audio buffer on.
    pub fn set_variant_mode(&self, mode: VariantMode) {
        self.ctl.set_variant_mode(mode);
    }

    /// How much of the pack's pitch and volume variation each keystroke gets, for sounds
    /// started from the next audio buffer on: 0 = no pitch or volume variation (so in
    /// [`VariantMode::Consistent`] every press of a key sounds exactly the same), 1 = the pack's
    /// full ranges, default [`DEFAULT_HUMANIZE`]. Values outside `0..=1` are limited to it, NaN
    /// means 0.
    pub fn set_humanize(&self, amount: f32) {
        self.ctl.set_humanize(amount);
    }

    /// Swaps in a new bank. Key sounds that are playing stop; a preview keeps playing.
    /// If the command ring is full (the audio callback has stalled), the bank is handed back
    /// so the caller can retry.
    pub fn replace_bank(&mut self, bank: SoundBank) -> Result<(), SoundBank> {
        self.ctl.replace_bank(bank)
    }

    /// Plays `clip` (mono, at the device rate) once, replacing any preview that is playing.
    /// Unaffected by press/release gains. Hands the clip back if the command ring is full.
    pub fn preview(&mut self, clip: Box<[f32]>) -> Result<(), Box<[f32]>> {
        self.ctl.preview(clip)
    }

    /// Stops the preview. `false` if the command ring is full.
    pub fn stop_preview(&mut self) -> bool {
        self.ctl.stop_preview()
    }

    /// Frees banks and clips the callback has handed back. Every control call above does this
    /// first; call it on its own (e.g. from a UI timer) to release a replaced bank promptly.
    pub fn collect_garbage(&mut self) {
        self.ctl.collect_garbage();
    }

    /// The consumer end of the latency metrics ring (only if `measure_latency` was set).
    pub fn take_metrics(&mut self) -> Option<Consumer<LatencySample>> {
        self.ctl.metrics.take()
    }
}

/// The control side's ends of the callback's rings, kept apart from the cpal stream so the
/// protocol can be tested without an audio device.
struct Control {
    commands: Producer<Command>,
    garbage: Consumer<Garbage>,
    settings: Arc<Settings>,
    metrics: Option<Consumer<LatencySample>>,
    /// Bank swaps whose replaced bank has not come back through `garbage` yet (each swap
    /// returns exactly one), so [`Control::take_bank`] knows which bank came back last.
    banks_out: usize,
}

impl Control {
    fn set_master_gain(&self, gain: f32) {
        store(&self.settings.master, gain);
    }

    fn set_press_gain(&self, gain: f32) {
        store(&self.settings.press, gain);
    }

    fn set_release_gain(&self, gain: f32) {
        store(&self.settings.release, gain);
    }

    fn set_variant_mode(&self, mode: VariantMode) {
        self.settings.variant_mode.store(mode_code(mode), Ordering::Relaxed);
    }

    fn set_humanize(&self, amount: f32) {
        store(&self.settings.humanize, amount);
    }

    fn replace_bank(&mut self, bank: SoundBank) -> Result<(), SoundBank> {
        if !self.ready() {
            return Err(bank);
        }
        self.push(Command::ReplaceBank(Box::new(bank)));
        self.banks_out += 1;
        Ok(())
    }

    /// Swaps an empty bank in and waits up to `timeout` for the callback to return the bank
    /// that was playing: the last one to come back once every swap in flight has answered.
    /// `None` if the command ring is full or the callback does not answer in time.
    fn take_bank(&mut self, timeout: Duration) -> Option<SoundBank> {
        self.replace_bank(SoundBank::default()).ok()?;
        let deadline = Instant::now() + timeout;
        let mut last = None;
        loop {
            while let Ok(garbage) = self.garbage.pop() {
                if let Garbage::Bank(bank) = garbage {
                    self.banks_out = self.banks_out.saturating_sub(1);
                    last = Some(bank);
                }
            }
            if self.banks_out == 0 {
                return last.map(|bank| *bank);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn preview(&mut self, clip: Box<[f32]>) -> Result<(), Box<[f32]>> {
        if !self.ready() {
            return Err(clip);
        }
        self.push(Command::Preview(clip));
        Ok(())
    }

    fn stop_preview(&mut self) -> bool {
        if !self.ready() {
            return false;
        }
        self.push(Command::StopPreview);
        true
    }

    fn collect_garbage(&mut self) {
        while let Ok(garbage) = self.garbage.pop() {
            if matches!(garbage, Garbage::Bank(_)) {
                self.banks_out = self.banks_out.saturating_sub(1);
            }
        }
    }

    /// Drains the garbage ring (which is what upholds the `GARBAGE_RING` invariant), then
    /// says whether a command fits.
    fn ready(&mut self) -> bool {
        self.collect_garbage();
        !self.commands.is_full()
    }

    /// Only after `ready()`: this is the sole producer, so a slot seen free stays free.
    fn push(&mut self, cmd: Command) {
        let pushed = self.commands.push(cmd).is_ok();
        debug_assert!(pushed, "command slot vanished");
    }
}

/// Creates every ring between the control side, the input thread and the callback, and the
/// shared settings with `config`'s initial values.
fn connect(
    mixer: Mixer,
    channels: usize,
    config: &EngineConfig,
) -> (Control, TriggerSender, CallbackState) {
    let (trigger_tx, trigger_rx) = RingBuffer::new(TRIGGER_RING);
    let (command_tx, command_rx) = RingBuffer::new(COMMAND_RING);
    let (garbage_tx, garbage_rx) = RingBuffer::new(GARBAGE_RING);
    let (metrics_tx, metrics_rx) = if config.measure_latency {
        let (tx, rx) = RingBuffer::new(METRICS_RING);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let settings = Arc::new(Settings::new(config));
    let channels = channels.max(1);

    let ctl = Control {
        commands: command_tx,
        garbage: garbage_rx,
        settings: settings.clone(),
        metrics: metrics_rx,
        banks_out: 0,
    };
    let state = CallbackState {
        mixer,
        triggers: trigger_rx,
        commands: command_rx,
        garbage: garbage_tx,
        metrics: metrics_tx,
        settings,
        channels,
        scratch: vec![0.0; SCRATCH_FRAMES * channels].into_boxed_slice(),
    };
    (ctl, TriggerSender { tx: trigger_tx }, state)
}

/// Everything the audio callback owns. Built on the control thread, then moved into the
/// stream; after that it is only touched by the callback.
struct CallbackState {
    mixer: Mixer,
    triggers: Consumer<Trigger>,
    commands: Consumer<Command>,
    garbage: Producer<Garbage>,
    metrics: Option<Producer<LatencySample>>,
    settings: Arc<Settings>,
    channels: usize,
    /// Mix buffer for non-f32 devices, sized up front.
    scratch: Box<[f32]>,
}

impl CallbackState {
    /// Picks up settings and commands, then starts sounds for queued key events.
    /// `output_ns` is the backend's callback → speaker estimate for this buffer.
    fn drain(&mut self, output_ns: u64) {
        let settings = &self.settings;
        self.mixer.set_master_gain(load(&settings.master));
        self.mixer.set_press_gain(load(&settings.press));
        self.mixer.set_release_gain(load(&settings.release));
        self.mixer.set_humanize(load(&settings.humanize));
        self.mixer.set_variant_mode(load_mode(&settings.variant_mode));

        while let Ok(cmd) = self.commands.pop() {
            self.apply(cmd);
        }

        if self.triggers.is_empty() {
            return;
        }
        let now = clock::now_ns();
        while let Ok(t) = self.triggers.pop() {
            let played = self.mixer.start(t.key, t.action);
            // Latency is measured per key press that makes a sound.
            if played
                && t.action == KeyAction::Down
                && t.received_ns != 0
                && let Some(m) = self.metrics.as_mut()
            {
                let _ = m.push(LatencySample {
                    input_ns: t.received_ns.saturating_sub(t.event_ns),
                    queue_ns: now.saturating_sub(t.received_ns),
                    output_ns,
                });
            }
        }
    }

    fn apply(&mut self, cmd: Command) {
        let old = match cmd {
            Command::ReplaceBank(bank) => Some(Garbage::Bank(self.mixer.replace_bank(bank))),
            Command::Preview(clip) => self.mixer.set_preview(clip).map(Garbage::Clip),
            Command::StopPreview => self.mixer.stop_preview().map(Garbage::Clip),
        };
        // Cannot fail (see `GARBAGE_RING`). If it somehow did, leaking beats freeing here.
        if let Some(old) = old
            && let Err(PushError::Full(old)) = self.garbage.push(old)
        {
            std::mem::forget(old);
        }
    }

    /// Mixes one interleaved buffer in the device's sample format.
    fn render<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T]) {
        let channels = self.channels;
        if T::FORMAT == SampleFormat::F32 {
            // SAFETY: FORMAT == F32 means T is f32.
            let out = unsafe { &mut *(data as *mut [T] as *mut [f32]) };
            self.mixer.render(out, channels);
            return;
        }
        // Chunks are whole frames: the scratch length is a multiple of `channels`.
        for part in data.chunks_mut(self.scratch.len()) {
            let mix = &mut self.scratch[..part.len()];
            self.mixer.render(mix, channels);
            for (d, s) in part.iter_mut().zip(mix.iter()) {
                *d = T::from_sample(*s);
            }
        }
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut state: CallbackState,
    health: Arc<StreamHealth>,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + FromSample<f32>,
{
    device
        .build_output_stream::<T, _, _>(
            *config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let ts = info.timestamp();
                let output_ns = ts.playback.duration_since(ts.callback).as_nanos() as u64;
                state.drain(output_ns);
                state.render(data);
            },
            move |e| health.record(e),
            None,
        )
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Key;
    use cpal::Sample;
    use std::sync::atomic::AtomicBool;

    const FLAT: Variation = Variation { pitch: 0.0, volume: 0.0 };

    fn clip(len: usize, value: f32) -> Box<[f32]> {
        vec![value; len].into_boxed_slice()
    }

    /// Every key plays a constant `value` of `len` samples on press; no release sound.
    fn bank(len: usize, value: f32) -> SoundBank {
        SoundBank { variation: FLAT, ..SoundBank::uniform(clip(len, value), None) }
    }

    fn rig(initial: SoundBank, channels: usize) -> (Control, TriggerSender, CallbackState) {
        let config = EngineConfig { measure_latency: true, ..EngineConfig::default() };
        connect(Mixer::with_seed(Box::new(initial), 1), channels, &config)
    }

    fn trig(key: Key, action: KeyAction, received_ns: u64) -> Trigger {
        Trigger { key, action, event_ns: 1, received_ns }
    }

    /// Lengths of the banks/clips waiting in the garbage ring, in order.
    fn garbage(ctl: &mut Control) -> Vec<(char, usize)> {
        std::iter::from_fn(|| ctl.garbage.pop().ok())
            .map(|g| match g {
                Garbage::Bank(b) => ('b', b.samples[0].len()),
                Garbage::Clip(c) => ('c', c.len()),
            })
            .collect()
    }

    #[test]
    fn latency_is_measured_only_for_played_key_downs() {
        let mut map = SoundMap::new();
        map.set(Key::KeyA, KeyAction::Down, &[0]);
        map.set(Key::KeyA, KeyAction::Up, &[0]);
        let b = SoundBank { samples: vec![clip(100, 0.1)], map, gain: 1.0, variation: FLAT };
        let (mut ctl, mut tx, mut state) = rig(b, 1);
        let mut metrics = ctl.metrics.take().unwrap();

        assert!(tx.send(trig(Key::KeyA, KeyAction::Down, 3))); // measured
        assert!(tx.send(trig(Key::KeyA, KeyAction::Up, 3))); // played, but a release
        assert!(tx.send(trig(Key::KeyB, KeyAction::Down, 3))); // silent
        assert!(tx.send(trig(Key::KeyA, KeyAction::Down, 0))); // unmeasured
        state.drain(5_000);

        assert_eq!(state.mixer.active_voices(), 3);
        let s = metrics.pop().unwrap();
        assert_eq!((s.input_ns, s.output_ns), (2, 5_000));
        assert!(metrics.pop().is_err());
    }

    #[test]
    fn gains_reach_the_callback_on_the_next_buffer() {
        let (ctl, mut tx, mut state) = rig(bank(1000, 0.5), 1);
        ctl.set_press_gain(0.5);
        ctl.set_master_gain(0.5);
        tx.send(trig(Key::KeyA, KeyAction::Down, 0));
        state.drain(0);
        let mut out = [0.0f32; 4];
        state.render(&mut out);
        assert_eq!(out, [0.125; 4]);
        ctl.set_master_gain(1.0);
        state.drain(0);
        state.render(&mut out);
        assert_eq!(out, [0.25; 4]);
    }

    /// Every key presses one of five two-sample clips whose levels 0.1–0.5 tell them apart,
    /// with pitch variation 0.05 and volume variation 0.2.
    fn five_takes() -> SoundBank {
        let mut map = SoundMap::new();
        for &key in Key::ALL {
            map.set(key, KeyAction::Down, &[0, 1, 2, 3, 4]);
        }
        let samples = (1..=5).map(|i| clip(2, i as f32 / 10.0)).collect();
        SoundBank { samples, map, gain: 1.0, variation: Variation { pitch: 0.05, volume: 0.2 } }
    }

    /// Presses `key` once per buffer, `n` times, and returns what each press rendered.
    fn press_levels(
        tx: &mut TriggerSender,
        state: &mut CallbackState,
        key: Key,
        n: usize,
    ) -> Vec<f32> {
        (0..n)
            .map(|_| {
                assert!(tx.send(trig(key, KeyAction::Down, 0)));
                state.drain(0);
                // A two-sample clip plays for one or two frames (two when humanized below
                // rate 1); its first frame is its level times the voice gain, exactly.
                let mut out = [0.0f32; 4];
                state.render(&mut out);
                assert_eq!(out[2..], [0.0; 2], "the press must end before the next one");
                out[0]
            })
            .collect()
    }

    fn distinct(levels: &[f32]) -> Vec<f32> {
        let mut d = levels.to_vec();
        d.sort_by(f32::total_cmp);
        d.dedup();
        d
    }

    const TAKES: [f32; 5] = [0.1, 0.2, 0.3, 0.4, 0.5];

    #[test]
    fn variant_mode_and_humanize_reach_the_callback() {
        let (ctl, mut tx, mut state) = rig(five_takes(), 1);

        // Default: consistent variants, lightly humanized. One take, within a quarter of the
        // pack's ±20 % volume variation.
        let levels = press_levels(&mut tx, &mut state, Key::KeyA, 200);
        let nearest = |l: f32| {
            TAKES.into_iter().min_by(|a, b| (l - a).abs().total_cmp(&(l - b).abs())).unwrap()
        };
        let take = nearest(levels[0]);
        assert!(levels.iter().all(|&l| (l - take).abs() <= take * 0.05 + 1e-6), "{levels:?}");
        assert!(distinct(&levels).len() > 1, "default humanize should vary the level");

        // No humanization: the same take at exactly its level, 200 times.
        ctl.set_humanize(0.0);
        let levels = press_levels(&mut tx, &mut state, Key::KeyA, 200);
        assert_eq!(distinct(&levels), [take]);

        // Different keys still land on different takes, each one exact.
        let per_key: Vec<f32> =
            Key::ALL.iter().flat_map(|&k| press_levels(&mut tx, &mut state, k, 1)).collect();
        assert_eq!(distinct(&per_key), TAKES);

        // Random variants: one key now plays every take.
        ctl.set_variant_mode(VariantMode::Random);
        assert_eq!(distinct(&press_levels(&mut tx, &mut state, Key::KeyA, 200)), TAKES);

        // Back to consistent at full humanization: the key's take, within the full ±20 %.
        ctl.set_variant_mode(VariantMode::Consistent);
        ctl.set_humanize(1.0);
        let levels = press_levels(&mut tx, &mut state, Key::KeyA, 200);
        assert!(levels.iter().all(|&l| (l - take).abs() <= take * 0.2 + 1e-6), "{levels:?}");
        assert!(levels.iter().any(|&l| (l - take).abs() > take * 0.1), "{levels:?}");

        // Bad values are bounded on the callback side: NaN means no humanization.
        ctl.set_humanize(f32::NAN);
        assert_eq!(distinct(&press_levels(&mut tx, &mut state, Key::KeyA, 50)), [take]);
    }

    #[test]
    fn engine_config_sets_the_initial_variant_mode_and_humanize() {
        let config = EngineConfig {
            variant_mode: VariantMode::Random,
            humanize: 0.0,
            ..EngineConfig::default()
        };
        let mixer = Mixer::with_seed(Box::new(five_takes()), 3);
        let (_ctl, mut tx, mut state) = connect(mixer, 1, &config);
        assert_eq!(distinct(&press_levels(&mut tx, &mut state, Key::KeyA, 200)), TAKES);

        let d = EngineConfig::default();
        assert_eq!((d.variant_mode, d.humanize), (VariantMode::Consistent, DEFAULT_HUMANIZE));
    }

    #[test]
    fn integer_devices_are_mixed_in_whole_frame_chunks() {
        let (_ctl, mut tx, mut state) = rig(bank(3 * SCRATCH_FRAMES, 0.5), 2);
        tx.send(trig(Key::KeyA, KeyAction::Down, 0));
        state.drain(0);
        let mut out = vec![0i16; 2 * (SCRATCH_FRAMES + 7)];
        state.render(&mut out);
        assert!(out.iter().all(|&s| s == i16::from_sample(0.5f32)));
    }

    #[test]
    fn replaced_banks_and_clips_come_back_as_garbage() {
        let (mut ctl, _tx, mut state) = rig(bank(10, 0.5), 1);
        assert!(ctl.preview(clip(3, 0.1)).is_ok());
        assert!(ctl.replace_bank(bank(20, 0.5)).is_ok());
        assert!(ctl.preview(clip(4, 0.1)).is_ok());
        assert!(ctl.stop_preview());
        assert!(ctl.stop_preview());
        state.drain(0);
        assert_eq!(garbage(&mut ctl), [('b', 10), ('c', 3), ('c', 4)]);
        assert!(!state.mixer.preview_playing());
    }

    #[test]
    fn full_command_ring_hands_the_payload_back() {
        let (mut ctl, _tx, mut state) = rig(bank(1, 0.5), 1);
        for i in 0..COMMAND_RING {
            assert!(ctl.replace_bank(bank(i + 2, 0.5)).is_ok());
        }
        let back = ctl.replace_bank(bank(999, 0.5)).unwrap_err();
        assert_eq!(back.samples[0].len(), 999);
        assert_eq!(ctl.preview(clip(7, 0.1)).unwrap_err().len(), 7);
        assert!(!ctl.stop_preview());

        // The callback catches up: every old bank comes back, and commands fit again.
        state.drain(0);
        assert_eq!(ctl.garbage.slots(), COMMAND_RING);
        assert!(ctl.replace_bank(back).is_ok());
        assert_eq!(ctl.garbage.slots(), 0);
    }

    /// The narrowest case of the `GARBAGE_RING` invariant, step by step: one more garbage
    /// item than there are command slots, plus the one in flight.
    #[test]
    fn garbage_ring_holds_the_worst_interleaving() {
        let (mut ctl, _tx, mut state) = rig(bank(1, 0.5), 1);
        // While the callback stalls, the command ring fills up.
        for i in 0..COMMAND_RING {
            assert!(ctl.replace_bank(bank(i + 2, 0.5)).is_ok());
        }
        // The callback pops the first command: its slot is free before it is applied.
        let first = state.commands.pop().ok().unwrap();
        // The control side fills the slot again; its drain finds no garbage yet.
        assert!(ctl.replace_bank(bank(100, 0.5)).is_ok());
        // The next control call starts `ready()`: the drain finds the garbage ring empty...
        ctl.collect_garbage();
        // ...the callback finishes the first command and pops the second...
        state.apply(first);
        let second = state.commands.pop().ok().unwrap();
        // ...so the slot check that follows the drain sees room, and a command goes in.
        assert!(!ctl.commands.is_full());
        ctl.push(Command::ReplaceBank(Box::new(bank(101, 0.5))));

        // The callback works through the rest while the control side stays idle.
        assert!(!state.garbage.is_full(), "garbage push would fail");
        state.apply(second);
        while let Ok(cmd) = state.commands.pop() {
            assert!(!state.garbage.is_full(), "garbage push would fail");
            state.apply(cmd);
        }
        // Every replaced bank came back instead of being leaked.
        let returned = garbage(&mut ctl);
        assert_eq!(returned.len(), COMMAND_RING + 2);
        assert_eq!(returned[0], ('b', 1));
        assert_eq!(returned[COMMAND_RING + 1], ('b', 100));
    }

    #[test]
    fn stream_errors_become_counters_and_faults() {
        use cpal::ErrorKind;
        let health = StreamHealth::new();
        assert_eq!((health.take_xruns(), health.take_fault()), (0, None));

        health.record(ErrorKind::Xrun.into());
        health.record(ErrorKind::Xrun.into());
        // Still playing: rerouted to the new default device, or no real-time priority.
        health.record(ErrorKind::DeviceChanged.into());
        health.record(ErrorKind::RealtimeDenied.into());
        assert_eq!((health.take_xruns(), health.take_fault()), (2, None));
        assert_eq!(health.take_xruns(), 0);
        // A reroute is reported once, on its own.
        assert!(health.take_rerouted());
        assert!(!health.take_rerouted());

        health.record(ErrorKind::StreamInvalidated.into());
        assert_eq!(health.take_fault(), Some(StreamFault::Invalidated));
        assert!(!health.take_rerouted());
        // The worst fault survives later, milder notices until it is taken.
        health.record(ErrorKind::DeviceNotAvailable.into());
        health.record(ErrorKind::DeviceChanged.into());
        health.record(ErrorKind::StreamInvalidated.into());
        assert_eq!(health.take_fault(), Some(StreamFault::DeviceGone));
        assert_eq!(health.take_fault(), None);
        assert!(health.take_rerouted(), "a reroute next to a fault is still reported");
        for kind in [ErrorKind::BackendError, ErrorKind::HostUnavailable, ErrorKind::Other] {
            health.record(cpal::Error::with_message(kind, "details"));
            assert_eq!(health.take_fault(), Some(StreamFault::Failed), "{kind:?}");
        }
    }

    /// Runs the "callback" (command handling only) on a thread until the returned flag is set.
    fn run_callback(mut state: CallbackState) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                state.drain(0);
                std::thread::sleep(Duration::from_micros(200));
            }
        });
        (stop, thread)
    }

    #[test]
    fn take_bank_hands_back_the_newest_bank() {
        let (mut ctl, _tx, state) = rig(bank(1, 0.1), 1);
        // Two swaps the callback has not seen yet: the newest is the one that plays.
        assert!(ctl.replace_bank(bank(2, 0.2)).is_ok());
        assert!(ctl.replace_bank(bank(3, 0.3)).is_ok());
        let (stop, thread) = run_callback(state);
        let back = ctl.take_bank(Duration::from_secs(5)).expect("the callback answers");
        assert_eq!(back.samples[0].len(), 3);
        assert_eq!(ctl.banks_out, 0);
        // The empty bank it left behind comes back next time.
        let empty = ctl.take_bank(Duration::from_secs(5)).unwrap();
        assert!(empty.samples.is_empty());
        stop.store(true, Ordering::Release);
        thread.join().unwrap();
    }

    #[test]
    fn take_bank_gives_up_when_the_callback_is_gone() {
        let (mut ctl, _tx, state) = rig(bank(1, 0.1), 1);
        drop(state); // a failed stream: nobody takes commands any more
        let started = Instant::now();
        assert!(ctl.take_bank(Duration::from_millis(30)).is_none());
        assert!(started.elapsed() >= Duration::from_millis(30));
    }

    /// The control side hammers the command ring while the "callback" checks, before every
    /// garbage push, that the push would succeed.
    #[test]
    fn garbage_ring_never_fills_under_concurrency() {
        let (mut ctl, _tx, mut state) = rig(SoundBank::default(), 1);
        let done = Arc::new(AtomicBool::new(false));
        let control = {
            let done = done.clone();
            std::thread::spawn(move || {
                let mut rng = fastrand::Rng::with_seed(9);
                for _ in 0..20_000 {
                    loop {
                        let sent = match rng.u8(..3) {
                            0 => ctl.replace_bank(SoundBank::default()).is_ok(),
                            1 => ctl.preview(clip(2, 0.0)).is_ok(),
                            _ => ctl.stop_preview(),
                        };
                        if sent {
                            break;
                        }
                        std::thread::yield_now();
                    }
                }
                done.store(true, Ordering::Release);
            })
        };
        loop {
            let finished = done.load(Ordering::Acquire);
            while let Ok(cmd) = state.commands.pop() {
                assert!(!state.garbage.is_full(), "garbage push would fail");
                state.apply(cmd);
            }
            if finished {
                break;
            }
            std::thread::yield_now();
        }
        control.join().unwrap();
    }
}
