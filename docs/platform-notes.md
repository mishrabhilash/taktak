# Platform notes: Windows and Linux

TakTak ships on macOS first. This page collects what the Windows and Linux ports have to deal
with for key listening (permissions) and audio latency, so the work and the user-facing
explanations are ready when those listeners land. Today `taktak-core` has no key listener on
either OS: permission shows `unknown`, no key sounds play (previews do), and per-app rules are
kept but unsupported (`rulesSupported` is false). The macOS side is described in
[`app.md`](app.md) and the overall limits table in [`architecture.md`](architecture.md).

Whatever the platform, the listener only ever reads which physical key went down or up. No
characters, no keyboard layout lookups, nothing logged, stored or sent.

## Windows

### Key listening

- **API:** a low-level keyboard hook, `SetWindowsHookExW(WH_KEYBOARD_LL, …)`, on a dedicated
  thread that pumps messages. It needs no administrator rights and no permission prompt, so the
  onboarding window skips its permission step (`permissionRequired` is false).
- **Elevated windows (UIPI):** Windows does not deliver keys typed into a window of higher
  integrity to a hook in a lower-integrity process. While an elevated app has the focus (an
  administrator Command Prompt or PowerShell, Task Manager, Registry Editor, an installer, any
  app started with "Run as administrator"), TakTak sees nothing and stays silent. The only way
  around it is running TakTak elevated too, which we do not recommend or offer: everything
  TakTak starts (the file manager for the packs folder, for one) would run as administrator
  too, for a cosmetic feature. The UI should say "TakTak can't hear keys in apps running as
  administrator" rather than suggest elevation.
- **Secure desktop:** the sign-in screen, the lock screen and UAC prompts run on a separate
  desktop no hook reaches. That matches the macOS behaviour (silent while locked); auto-mute
  on lock would come from `WTSRegisterSessionNotification` (`WTS_SESSION_LOCK` /
  `WTS_SESSION_UNLOCK`).
- **Hook timeout:** Windows removes a low-level hook that does not return in time (about
  300 ms; the `LowLevelHooksTimeout` registry value, at most 1 s since Windows 7), silently and
  for good. The callback must do what the macOS tap does: check one atomic, push into the
  wait-free ring and return, never block, allocate or log. A removed hook cannot be detected
  from inside the process, so re-installing it on session unlock and on resume from sleep is
  the practical guard.
- **Injected keys:** events with `LLKHF_INJECTED` (on-screen keyboards, remote desktop tools,
  AutoHotkey) can be ignored or played; playing them is closer to what the user expects.
- **Per-app rules (future):** `GetForegroundWindow` → `GetWindowThreadProcessId` →
  `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → `QueryFullProcessImageNameW`, identified
  by the executable path (the `id` of an `AppRuleEntry`), with the file's description as the
  name. Event-driven with `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`, no polling. A few
  protected processes refuse the query; they would be "unknown".

### Audio latency

- **WASAPI shared mode** (what cpal uses): the engine's period is about 10 ms, and the audio
  engine's mixer adds its own buffer, so key-to-sound latency is typically 15–30 ms, against
  under 10 ms on macOS. The 64-frame buffer TakTak requests is not honoured in shared mode.
- **Lower latency** needs a custom backend:
  - `IAudioClient3::InitializeSharedAudioStream` with the device's minimum period (often
    2.67 ms at 48 kHz) keeps shared mode, so other apps still play. It depends on the driver:
    many USB and Bluetooth devices only offer 10 ms.
  - Exclusive mode reaches a few milliseconds but takes the device away from every other app,
    which is wrong for a background utility.
- **Bluetooth** output adds 100–250 ms whatever the mode, as on macOS.
- **Device changes:** WASAPI invalidates the stream when the default device changes (cpal
  reports a stream error), which the control thread already handles by reopening on the new
  default device; that is also where the "Mute when the output device changes" auto-mute is
  detected.

## Linux

### Key listening: X11 and Wayland

- **X11:** XInput2 raw key events (`XI_RawKeyPress` / `XI_RawKeyRelease` on the root window)
  are listen-only and need no special rights. Any X client can read them, which is a property
  of X11 itself.
- **Wayland:** there is no protocol for reading keys outside your own windows, by design.
  XWayland only sees keys typed into X11 apps. The remaining option is reading the input devices
  directly:
  - **evdev** (`/dev/input/event*`, through `libinput` or plain `read`) works on X11 and
    Wayland alike, and on the console.
  - It needs read access to the device nodes, normally by joining the `input` group:
    `sudo usermod -aG input "$USER"`, then log out and back in. That gives every program the
    user runs the ability to read every keystroke, including passwords typed anywhere, which
    is keylogger-level access. TakTak must make this an explicit opt-in with that warning, never
    ask for it silently, and never ask for root.
  - The XDG desktop portal's GlobalShortcuts interface only delivers registered shortcuts, not
    every key, so it cannot replace evdev; it can still serve the mute hotkey on Wayland.
  - Flatpak and Snap sandboxes hide `/dev/input` unless the package is granted device access
    (`--device=all`), which store reviewers question; a native package (`.deb`, AppImage) is
    the realistic route for Wayland support.
- **Mapping:** evdev and XInput2 report Linux key codes (`KEY_A` …; X11 key codes are those
  plus 8), which map to `Key` by position like the macOS virtual key codes. No keysym or
  layout lookups, so no characters.
- **Per-app rules (future):** X11: `_NET_ACTIVE_WINDOW` on the root window (a `PropertyNotify`
  event, no polling) → `WM_CLASS` of that window (the `id`), `_NET_WM_NAME` as the name.
  Wayland: no generic API; only compositor-specific ones (Sway and Hyprland IPC, a GNOME Shell
  extension). Elsewhere rules stay unsupported.
- **Lock screen:** `org.freedesktop.login1` (systemd-logind) `Lock` / `Unlock` signals and the
  session's `Active` property over D-Bus give the auto-mute on lock and on user switching.

### Audio latency

- **PipeWire** (the default on current distributions): the whole graph runs at one "quantum",
  which dominates latency. PipeWire sets it to the smallest `node.latency` that any active
  client asks for, clamped to `default.clock.min-quantum` (32 frames by default) and
  `default.clock.max-quantum`. `default.clock.quantum` (1024 frames at 48 kHz, about 21 ms)
  applies only while no client asks for less. So TakTak is not clamped up to 1024: its request
  sets the floor, and it lowers the quantum **for every app in the graph** while its stream is
  open.
  - What TakTak asks for: the engine requests a fixed buffer (`BufferSize::Fixed`, 64 frames,
    `BUFFER_FRAMES` in `service.rs`). Through cpal's ALSA backend and `pipewire-alsa` that
    becomes a 64-frame period, hence `node.latency` 64/48000 (1.3 ms). A native PipeWire or
    Pulse client would set the same with `PIPEWIRE_LATENCY="64/48000"` (or `PULSE_LATENCY_MSEC`).
  - Cost: a 64-frame quantum wakes every client in the graph 750 times a second instead of about
    47, which raises system-wide CPU use and the risk of xruns for every app, not just TakTak.
  - Policy for the Linux port: request **128/48000** (about 2.7 ms, below what anyone hears for
    a key click) instead of 64, so the graph is not pushed to the edge for everyone; keep 64 on
    macOS and Windows, where the buffer is TakTak's alone.
  - Settings, for testing and for users: `pw-metadata -n settings 0 clock.force-quantum 128`
    forces a quantum system-wide (`0` resets); `default.clock.quantum` in
    `~/.config/pipewire/pipewire.conf.d/` changes the default used when nobody asks for less.
    `default.clock.min-quantum` only sets the lower clamp: lowering it below 32 does not make
    anything faster, and raising it is how a user stops TakTak (or any app) from forcing a tiny
    quantum.
  A quantum of 64–128 frames (1.3–2.7 ms) is fine on most hardware; smaller ones risk xruns on
  busy systems.
- **PulseAudio** (older systems): similar defaults (25 ms or more); `PULSE_LATENCY_MSEC` lowers
  it per process.
- **ALSA directly** (no sound server): lowest latency, but it takes the device away from other
  apps on most hardware. Not a good default.
- **Bluetooth** output adds 100–250 ms, as elsewhere.

## Summary for the UI

| | Permission step | What to tell the user |
|---|---|---|
| macOS | Input Monitoring (onboarding window) | Already implemented. |
| Windows | none | "TakTak can't hear keys in apps running as administrator." |
| Linux X11 | none | Nothing special. |
| Linux Wayland | opt-in `input` group | What the group allows (every keystroke, for every program), how to join it, and that a log out is needed. |
