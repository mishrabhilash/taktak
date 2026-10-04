# TakTak architecture

## Crates

| Path | What |
|---|---|
| `src-tauri/core` (`taktak-core`) | Everything latency-sensitive, no UI deps: input hooks, key map, audio engine, packs, latency stats. Unit-testable on any OS. |
| `src-tauri` (Milestones 3–4) | Tauri 2 app shell: tray, settings and onboarding windows, commands that drive `taktak-core`, per-app rules and auto-mute (Milestone 4: `rules.rs`, `automute.rs`, `apps.rs`). |
| `tools/synth-packs` | CLI that measures any pack's typing loudness (`loudness`), which keeps the bundled packs level-matched, and generates experimental synthesized packs (not bundled; written to `target/synth-packs`). |
| `tools/pack-maker` | CLI that records (microphone + key listener) or slices (an existing WAV) keyboard audio into a sound pack. |
| `tools/mechvibes-import` (Milestone 5) | CLI that reuses `taktak-core`'s pack writer. |

## Threads

```
 ┌───────────────────────┐  Trigger (SPSC ring, wait-free)  ┌─────────────────────────────┐
 │ taktak-input          │ ───────────────────────────────▶ │ audio callback (OS RT thd)  │
 │ OS hook / run loop    │   { key, down/up, timestamps }   │ Mixer: SoundMap → sample    │
 │ - native code → Key   │                                  │ per key, humanized; 32 key  │
 │ - auto-repeat filter  │                                  │ voices + 1 preview voice    │
 │ - rules gate (atomic) │                                  └────▲───────────────────┬────┘
 └───────────────────────┘      atomics: gains, humanize, mode   │                   │  old banks, old
                                command ring: bank swap,         │                   │  preview clips
                                preview, stop preview            │                   │  (garbage ring);
                                                                 │                   │  latency samples
                            ┌────────────────────────────────────┴───────────────────▼────┐
                            │ control (app threads, not main/UI): pack registry + loader, │
                            │ settings, output-device watch, stats                        │
                            └────────────────────────────▲────────────────────────────────┘
                                                         │ Shared::update → derive() → gate
 ┌───────────────────────────────────────────────────────┴─────────────────────────────────┐
 │ main thread (Tauri/AppKit): commands, windows, tray; NSWorkspace observers (frontmost   │
 │ app, session) and CF distributed observers (screen lock), event-driven, never polled    │
 └─────────────────────────────────────────────────────────────────────────────────────────┘
```

- **Input thread** (`taktak-input`): owns the OS hook (macOS: listen-only `CGEventTap` on a
  dedicated CFRunLoop; Windows: `WH_KEYBOARD_LL` hook in a `GetMessageW` loop; Linux: XInput2
  raw events on X11 or `/dev/input` evdev devices on Wayland, in a `poll(2)` loop; see
  [`platform-notes.md`](platform-notes.md)). Per event it maps the native code to a `Key`, drops auto-repeat via
  `PressState`, checks a single `AtomicBool` "sounds allowed right now" (maintained off this
  path, see "The gate, per-app rules and auto-mute" below) and pushes a `Trigger` (key,
  down/up, event and receive timestamps) into a lock-free ring. It does not choose or humanize sounds. No allocation, no
  locks, no logging: when macOS disables the tap (timeout or user input), the callback
  re-enables it, clears the held keys and bumps a counter that the control side reads with
  `Listener::take_reenabled` and logs (Linux evdev counts kernel `SYN_DROPPED` overruns the
  same way; Windows removes a slow hook without any signal, so it reports 0).
- **Audio callback** (owned by CoreAudio / WASAPI / ALSA via cpal). At the start of every
  buffer it:
  1. loads the settings: the master, press and release gains and the humanize amount from
     `AtomicU32`s (`f32` bits), and the variant mode from an `AtomicU8`. The `Engine::set_*`
     methods store them (`EngineConfig` gives the initial variant mode and humanize). Settings
     are atomics, not commands, so dragging a slider can never fill the command ring and
     crowd out a bank swap;
  2. drains the command ring: `ReplaceBank` (stops the key voices, which index the old bank;
     the preview keeps playing), `Preview(clip)` and `StopPreview`;
  3. drains the trigger ring. The mixer does all sound choice: it looks the key and action up
     in the current `SoundBank`'s `SoundMap` (a precomputed key × action → candidate sample
     ids table, built by the loader from the pack's resolution chain) and picks a candidate
     by the `VariantMode`:
     - `Consistent` (the default): every key keeps one candidate. h = SplitMix64 of the 64-bit
       FNV-1a hash of the key's `KeyboardEvent.code` name (`audio::consistent_index`, from a
       const table): a fixed function of the name, never of the key's position in the `Key`
       enum, so a key sounds the same on every run, platform and build. The press
       plays candidate h mod (press candidates), the release h mod (release candidates). The
       shared h keeps a key's press and release from the same take when the pools are
       equal-sized matched takes (`office-classic`, `tactile`); different keys spread over
       the pool.
     - `Random`: uniformly at random per keystroke.

     It then draws the keystroke's pitch and volume from the bank's `Variation` scaled by the
     humanize amount (0–1, default 0.25; 0 plays every press at exactly rate 1 and the base
     gain): rate = 1 ± pitch × humanize, bounded to [0.25, 4]; voice gain = press or release
     gain × pack `volume` × (1 ± volume × humanize), bounded to [0, 4]. A key with no
     candidates is silent;
  4. mixes the 32 key voices (stealing the oldest when all are busy; linear interpolation for
     the pitch variation) and the separate preview voice (never stolen, unaffected by the
     press/release gains), applies the master gain and clamps to ±1 (NaN → 0).

  It never allocates, frees, locks or logs. Everything it replaces (the old `SoundBank`, the
  old preview clip) goes back to the control side through the garbage ring, sized
  `COMMAND_RING + 2` so that push can never fail; the control side drains it before every
  command and on `Engine::collect_garbage`. Latency samples go back on their own ring.
  cpal's stream-error callback (which can run on the audio IO thread) also only touches
  atomics: an xrun counter, a "worst fault" code and a "rerouted" flag, read with
  `Engine::take_xruns`, `Engine::take_stream_fault` and `Engine::take_rerouted`. On a fault
  (`Invalidated`, `DeviceGone`, `Failed`) the control side rebuilds the engine. A reroute is
  not a fault: macOS moves a default-device stream to the new default device by itself (cpal
  reports `DeviceChanged`) and it keeps playing, but at the old device's settings, with the
  requested buffer size no longer applied. The control side then reopens the engine on the new
  device too. `Engine::stop` hands back the bank that was playing, so a reopen at the same
  rate needs no decode.
- **Control side**: owns the `PackRegistry` (scans the bundled and user folders, resolves id
  overrides, hot-reloads the user folder and the targets of symlinked packs through a
  `notify` watcher, and reports `Added`/`Updated`/`Removed`/`Invalid`/`InvalidCleared`
  events) and loads packs. A load
  (`pack::load`) decodes every referenced file once to mono f32 at the device rate, then sends
  the finished `SoundBank` with `Engine::replace_bank` and, for "click to hear", the decoded
  preview clip with `Engine::preview`. In the app (`src-tauri`) this is the `taktak-control`
  thread (engine, listener, registry and watcher; asleep unless a message, a ≤ 4 Hz engine
  poll while the output is open, the 0.2 Hz default-device look while it is closed, or the
  one-shot rule-block wake-up is due). It opens the engine and the listener only while a key
  press can make a sound (sounds on, not muted, not auto-muted, not rule-blocked for 5 s or
  longer, permission granted) or a preview plays, and closes them otherwise. It also feeds
  every output device it opens or looks at into `DeviceWatch` (`automute.rs`), which arms the
  `outputChanged` auto-mute. Decoding runs on `taktak-loader`, settings writes on
  `taktak-settings` and `state-changed` broadcasts on `taktak-events`; at startup a
  short-lived `taktak-onboarding` thread waits (≤ 5 s) for the first permission check and then
  opens the welcome window if it is due. The Tauri main thread only runs commands, window and
  tray work and the macOS observers, and never waits on the other threads (see
  `docs/ui-contract.md`, `docs/app.md`).

## The gate, per-app rules and auto-mute (Milestone 4)

- **One atomic on the hot path.** Every state change goes through `Shared::update`, which runs
  `derive()` (`service.rs`): `autoMute` from the separate reasons (`AutoMuteReasons`:
  screen locked, session inactive, output changed), `ruleBlocked` from `settings.appRule` and
  `frontmostApp` (`rules::blocks`), `playing` and `onboarding.offer`. It then stores three
  atomics: the hook's gate (`rules::gate_open`: enabled && !muted && no auto-mute &&
  !ruleBlocked), `sounds` (the gate without the rule term) and `rule_blocked`. A change to any
  of them wakes the control thread. The hook still does exactly one `AtomicBool` load.
- **Frontmost app, event-driven.** `apps.rs` registers block observers on
  `NSWorkspace.notificationCenter` for `didActivateApplication` and session
  resign/become-active, on the main thread at startup, and reads `frontmostApplication` once.
  Activations of TakTak itself are ignored, so its own windows follow the app the user came
  from. The handler calls `Shared::update` directly from the main thread (a mutex and a few
  atomics; no I/O). The frontmost app is never logged, persisted or kept as a history.
- **Screen lock.** `com.apple.screenIsLocked` / `screenIsUnlocked` through the CoreFoundation
  distributed notification center with `DeliverImmediately`, because AppKit holds distributed
  notifications back from inactive apps and TakTak (an accessory app) is almost never active.
  All observers are removed in `RunEvent::Exit`.
- **Power.** Auto-mute closes the output at once. A rule block closes the gate at once but the
  output only after it has lasted 5 s (`rules::RuleBlock`): the control thread schedules one
  wake-up for that moment instead of polling, so ⌘Tab through a blocked app does not reopen the
  device.
- **Output-change auto-mute.** `DeviceWatch::see` reports a change only when the device name
  differs from the last one seen (not on the first device after launch, and not when the same
  device comes back after a fault). `automute::output_changed` then arms the reason only if
  `muteOnOutputChange && enabled && !muted`. It clears on any unmute (`set_muted`, the hotkey,
  the tray item), on `set_enabled(true)` and when the setting is turned off.
- **Windows and Linux.** `apps.rs` is a stub there: `rulesSupported` stays false, `frontmostApp`
  null and `ruleBlocked` false; the rule list is kept and editable. See
  [`platform-notes.md`](platform-notes.md).
- **Relaunch.** `relaunch.rs` starts the new instance (`open -n <bundle> --args --relaunch
  [--onboarding]` on macOS) before the old one exits normally; the new one waits up to 5 s for
  the old instance lock instead of handing over to it.

## A key press, end to end

1. Key goes down → OS input pipeline timestamps it (`event_ns`).
2. Our hook callback runs (`received_ns`): code → `Key` → not a repeat → gate open →
   `TriggerSender::send` (wait-free).
3. Next audio callback (≤ one buffer period later) picks it up, chooses the key's sample from
   the bank's `SoundMap`, humanizes pitch and volume slightly, and starts a voice at frame 0
   of the buffer it is filling.
4. That buffer reaches the speaker after the backend's output latency
   (buffer + device latency + safety offset on CoreAudio).

Latency = input + queue + output; each stage is measured separately (`latency.rs`) using one
monotonic clock (mach host time on macOS, which CGEvent and CoreAudio timestamps share;
`Instant` elsewhere). On Linux the OS event time is converted from `CLOCK_MONOTONIC` (evdev:
µs; X11 server time: ms) by its age at receipt. On Windows the hook's timestamp has ~15.6 ms
resolution, so `event_ns` is the receive time and the input stage reads 0.
Measurements record timings only.

## Sound packs

The format is specified in [`pack-format.md`](pack-format.md). Loading (`pack::load`, on the
control side, never on the audio or input thread):

1. Open the folder or zip, parse and validate `pack.json`, check that every referenced file
   exists (`pack::inspect` stops here; the registry uses it to list packs without decoding).
2. Decode every distinct file (key sounds and preview) **in parallel**: the calling thread
   reads the files in order (the pack source, folder or zip, needs `&mut`) and hands them over a bounded
   channel to up to 8 decode threads (at least 4 files per thread; small packs decode on the
   calling thread), so only a few encoded files are in memory at once. Each file is decoded
   (symphonia), downmixed to mono, checked against the 2 s limit, trimmed of leading silence
   (before resampling, so the onset lands exactly on the 0.5 ms pre-roll) and resampled to the
   device rate (rubato sinc; each thread reuses its resampler). A shared counter enforces the
   16 M-sample limit while decoding. Where panics unwind, a decoder panic on a hostile file
   becomes that file's error. All errors are reported together.
3. Build the `SoundMap` by running the resolution chain (`manifest::resolve`) for every key
   and action, and the `SoundBank` (samples, map, `volume` as the pack gain, `variation`).
   The preview clip is scaled by the pack's `volume`, so "click to hear" plays as loud as
   typing in the pack.

Bundled packs are loudness-matched: each pack's `volume` puts its typing loudness at TakTak's
reference typing level (a fixed −25.8 LK; typing loudness is the K-weighted energy of the first
100 ms of the sample each alphanumeric key plays on press by default, power-averaged over the
keys), within 0.5 dB, unless that would break the −1 dBFS true-peak headroom rule.
`cargo run -p synth-packs --release -- loudness packs/*` measures them; its test
`bundled_packs_are_loudness_matched` keeps them there.

Measured load times, release build, `taktak-pack info packs/<id>` (decodes at 48 kHz;
median of 5 runs on an Apple M3 Max, 14 cores, warm file cache):

| Pack | Files | Audio at 48 kHz | Memory (f32) | Source rate | Load time |
|---|---:|---:|---:|---|---:|
| `buckling-spring` | 168 | 31.1 s | 5.7 MB | 44.1 kHz | 25 ms |
| `key-press` | 25 | 5.1 s | 0.9 MB | 48 kHz | 2 ms |
| `linear-red` | 25 | 3.0 s | 0.5 MB | 48 kHz | 1 ms |
| `office-classic` | 63 | 7.0 s | 1.3 MB | 44.1 kHz | 12 ms |
| `spring-lite` | 28 | 4.4 s | 0.8 MB | 44.1 kHz | 9 ms |
| `tactile` | 25 | 3.5 s | 0.6 MB | 44.1 kHz (4 files 48 kHz) | 10 ms |
| `typewriter` | 70 | 24.8 s | 4.5 MB | 48 kHz | 4 ms |
| `ui-clicks` | 37 | 4.3 s | 0.8 MB | 44.1 kHz | 10 ms |
| `vintage-keyboard` | 11 | 2.9 s | 0.5 MB | 44.1 kHz | 10 ms |

Packs whose files already match the device rate skip resampling and load in a few
milliseconds. A 44.1 kHz pack loaded for a 48 kHz device pays for resampling: 9–12 ms even
for the small ones, since every decode thread first builds its sinc resampler. On a 44.1 kHz
device the cost moves to the 48 kHz packs instead. Either way a pack switch stays far below
what a user notices, and it happens off the audio thread: the old bank keeps playing until the
new one is swapped in.

## Privacy by construction

- Hooks read key codes and down/up only. No layout/character APIs are called
  (on macOS: no `CGEventKeyboardGetUnicodeString`, no TIS calls; on Windows: scan codes only,
  never `vkCode`, `ToUnicode`/`ToUnicodeEx`; on Linux: key codes only, no keysyms or XKB).
  The hooks are listen-only: the Windows hook always calls `CallNextHookEx`, evdev devices
  are never grabbed.
- `Key`'s `Debug` prints `Key(<redacted>)`, so key identities cannot leak into logs by accident.
- No networking code or networking crates.
- Per-app rules (Milestone 4) see only the frontmost app's bundle id and name, keep only the
  current value in `AppState`, and never log it. Only the user's rule list is persisted.

## Measured (Milestone 1, MacBook Pro speakers, 44.1 kHz)

Audio side only (`--synthetic`, so input stage excluded):

| Buffer | queue p50 | output | total p50 / p95 |
|---|---|---|---|
| 64 frames (default) | 0.8 ms | 4.6 ms | 5.4 / 6.0 ms |
| 128 frames | 2.1 ms | 6.1 ms | 8.1 / 8.9 ms |
| device default (512) | 5.4 ms | 14.8 ms | 20.2 / 26.3 ms |

CPU while streaming ≈ 0.2 %, RSS ≈ 17 MB.

Real typing, built-in keyboard → built-in speakers, 64 frames (two windows of 100 presses):

| Window | total p50 | p95 | max | input p50 / p95 / max |
|---|---|---|---|---|
| first 100 (includes startup) | 6.6 ms | 9.9 ms | 14.5 ms | 1.1 / 4.8 / 9.3 ms |
| next 100 | 5.9 ms | 6.7 ms | 10.0 ms | 0.25 / 1.3 / 4.5 ms |

Queue and output stages are steady. The tail comes from the input stage (OS event → our
hook thread being scheduled). The hook thread therefore runs at `QOS_CLASS_USER_INTERACTIVE`.

## Platform limitations

| | Latency | Global input | Per-app rules |
|---|---|---|---|
| macOS | < 10 ms on built-in speakers. Bluetooth output adds 100–250 ms (no fix possible). | Needs Input Monitoring. Secure Input (password fields, Terminal "Secure Keyboard Entry", lock screen) hides keystrokes, so it stays silent there by design. Unsigned dev builds lose the permission on every rebuild (code identity changes); `npm run app` with a "TakTak Development" certificate keeps it. | `NSWorkspace` frontmost app bundle ID, no extra permission. Spotlight/Raycast/Alfred panels report the *previous* app as frontmost. |
| Windows | cpal uses WASAPI shared mode at the default period (~10 ms) + mixer, typically 15–30 ms total. Hitting < 10 ms needs `IAudioClient3` low-latency shared mode (driver-dependent) or exclusive mode (blocks other apps' audio). Planned as a custom backend. | `WH_KEYBOARD_LL` (implemented, Milestone 5), no admin, no prompt. Cannot see keys typed into elevated (admin) windows unless TakTak is elevated (UIPI), nor the secure desktop. Injected keys (on-screen keyboards, automation tools) are ignored. Hook must return in < ~300 ms or Windows silently removes it (undetectable). Not yet run on real Windows. | `GetForegroundWindow` → process image path. Works, though a few protected processes refuse the query. |
| Linux X11 | PipeWire runs the graph at the smallest `node.latency` any client asks for (1024 frames, ~21 ms, only when nobody asks for less), so our small fixed buffer lowers the quantum for every app while the stream is open; plan to request 128/48000 there (see platform-notes.md). | XInput2 raw events (implemented, Milestone 5): listen-only, no root; falls back to evdev without a usable X server. Not yet run on real Linux. | `_NET_ACTIVE_WINDOW` → `WM_CLASS`. |
| Linux Wayland | as above | No global key API by design. Only `/dev/input` via evdev (implemented, Milestone 5; hotplug via inotify), which needs the `input` group (equivalent to keylogger rights): `InputError::PermissionDenied` until the user opts in. `TAKTAK_INPUT=x11` settles for XWayland (keys in X11 apps only). The app does not explain the opt-in yet. | No generic API. Compositor-specific only (Sway/Hyprland IPC, GNOME needs an extension). Rules are unavailable otherwise. |

Always-on stream: keeping the output stream open is what makes ~5 ms possible. Cold-starting
it on a keypress costs 30+ ms (seen as the first-trigger outlier during testing). The open
stream also keeps the audio device awake: measured on a MacBook Pro (built-in speakers,
macOS 26), `coreaudiod` uses 5–9 % of one core while any output stream runs, whatever the
buffer size (64 to 512 frames, silence), against 0 % with none; TakTak's own process stays at
≈ 0.3 %. The app therefore closes the stream (and the key listener) whenever no key press can
make a sound: sounds off, muted, or no permission. The decoded bank is kept, so reopening costs
only the ~0.1 s the device takes to open, at the moment the user turns sounds back on. Still
planned: suspend the stream after N minutes without keystrokes too, and accept one slow first
click on resume.
