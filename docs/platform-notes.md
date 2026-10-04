# Platform notes: Windows and Linux

TakTak ships on macOS first. This page collects what the Windows and Linux ports deal with for
key listening (permissions) and audio latency. Since Milestone 5 `taktak-core` has a key
listener on both (`src-tauri/core/src/input/windows.rs`, `input/linux/`), with the same contract
as macOS: `input::start` runs the hook on a dedicated `taktak-input` thread, the callback gets
one `Down` and one `Up` per physical press (auto-repeat and stray ups filtered by
`PressState`), and dropping the `Listener` stops and joins the thread. Per-app rules are still
kept but unsupported there (`rulesSupported` is false). The macOS side is described in
[`app.md`](app.md) and the overall limits table in [`architecture.md`](architecture.md).

Whatever the platform, the listener only ever reads which physical key went down or up. No
characters, no keyboard layout lookups, nothing logged, stored or sent. The native codes are
mapped by position through pure const tables compiled and tested on every platform:
`keymap_windows.rs` (scan codes), `keymap_linux.rs` (evdev codes; X11 codes are these + 8),
`keymap_macos.rs` (virtual key codes).

**Not yet verified on real hardware.** The Windows and Linux backends were written and
cross-checked (`cargo check`/`clippy --target x86_64-pc-windows-msvc` and
`x86_64-unknown-linux-gnu`) on macOS only. CI or a manual pass on each OS must still confirm:
the hook starts, every key on a full-size keyboard maps to the expected `KeyboardEvent.code`
(including Pause / Num Lock, Print Screen, Right Shift, AltGr, numpad with Num Lock on and off,
Japanese and Korean keys), auto-repeat plays once, stopping is prompt, and on Linux the
X11 → evdev fallback, the `PermissionDenied` path and evdev hotplug.

## Windows

### Key listening

- **API (implemented):** a low-level keyboard hook, `SetWindowsHookExW(WH_KEYBOARD_LL, …)`, on
  the `taktak-input` thread, which runs a `GetMessageW` loop (the hook procedure runs inside it)
  at `THREAD_PRIORITY_TIME_CRITICAL`. `Drop` posts `WM_QUIT` to the thread and joins it. It
  needs no administrator rights and no permission prompt, so `has_permission` and
  `request_permission` return true and the onboarding window skips its permission step
  (`permissionRequired` is false).
- **What it reads:** `KBDLLHOOKSTRUCT.scanCode` plus `LLKHF_EXTENDED` (an `0xE0` prefix), and
  down/up from the message (`WM_KEYDOWN`/`WM_SYSKEYDOWN` vs `WM_KEYUP`/`WM_SYSKEYUP`). Scan codes
  are positions, so the mapping does not depend on the layout. `vkCode` (layout dependent) and
  `ToUnicode`/`ToUnicodeEx` are never used. Quirks handled in `keymap_windows::from_hook`:
  Pause is `0x45` and Num Lock `0x45` + extended (the reverse of the raw scan codes); Right
  Shift arrives with the extended flag; Alt+Print Screen (`0x54`) and Ctrl+Pause (`0xE046`) are
  aliases; scan codes above `0xFF` are system-synthesized (`SCANCODE_SIMULATED`, 0x200): the
  left Ctrl that AltGr generates (`0x21D`) and the shift up/downs inserted around numpad keys
  with Num Lock on (`0x22A`, `0x236`), and are ignored. Scan code 0 (`VK_PACKET` text
  injection) is ignored.
- **Always passes events on:** the hook calls `CallNextHookEx` for every event and never
  blocks or changes one.
- **Timestamps:** `KBDLLHOOKSTRUCT.time` is `GetTickCount` milliseconds, quantized to the
  system tick (about 15.6 ms), too coarse for latency stats, so `event_ns` is the hook's
  receive time (`received_ns`) and the input stage reads 0 on Windows.
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
  wait-free ring and return, never block, allocate or log (well under 1 ms). A removed hook
  cannot be detected from inside the process (`Listener::take_reenabled` is always 0), so
  re-installing it on session unlock and on resume from sleep is the practical guard.
  **Implemented** (Milestone 5), event-driven: the `taktak-input` thread owns a hidden, never
  shown tool window registered with `WTSRegisterSessionNotification(NOTIFY_FOR_THIS_SESSION)`.
  `WM_WTSSESSION_CHANGE` with `WTS_SESSION_UNLOCK`, `WTS_CONSOLE_CONNECT` or
  `WTS_REMOTE_CONNECT`, and the `WM_POWERBROADCAST` resume events (`PBT_APMRESUMEAUTOMATIC`,
  `PBT_APMRESUMESUSPEND`, broadcast to every top-level window) post a message to the thread,
  whose loop then unhooks, forgets held keys and calls `SetWindowsHookExW` again. No polling
  and no app involvement. Known limit: a hook Windows removes at any other time (a stall
  while the session stays unlocked and awake) stays gone until the next unlock, resume or
  restart of the listener (muting and unmuting, or turning sounds off and on, restarts it).
- **Injected keys: ignored** (`keymap_windows::PLAY_INJECTED` is false). Events with
  `LLKHF_INJECTED` come from on-screen keyboards, remote-desktop and automation tools
  (AutoHotkey, macro software) and apps that type for the user. Ignoring them keeps TakTak to
  keys the user physically pressed and cannot echo a tool's synthesized typing. The cost: keys
  typed on the touch or on-screen keyboard, and keys remapped by a tool that swallows the
  physical key and injects another, are silent. Flipping the constant (or making it a setting)
  is the only change needed to play them.
- **Key up without a down** (the key went down on the secure desktop or in an elevated window,
  or before the hook started) is dropped by `PressState`; repeated downs (auto-repeat) play
  once.
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

- **Backend choice (implemented, `input/linux/mod.rs`):** `TAKTAK_INPUT=x11` or
  `TAKTAK_INPUT=evdev` forces a backend (no fallback). Otherwise a Wayland session
  (`WAYLAND_DISPLAY` set or `XDG_SESSION_TYPE=wayland`) or no `DISPLAY` uses evdev; an X11
  session uses XInput2 and falls back to evdev if the X server or its XInput 2 extension is
  unusable. A Wayland user who prefers XWayland-only sounds (keys typed into X11 apps) to joining
  the `input` group can set `TAKTAK_INPUT=x11`. Both backends run on `taktak-input`, asleep in
  `poll(2)` on their sources plus an eventfd that `Drop` signals: no busy loop, no timeout,
  prompt stop.
- **X11 (implemented, `input/linux/x11.rs`):** XInput2 raw key events (`XI_RawKeyPress` /
  `XI_RawKeyRelease`) selected on the root window for all master devices, through x11rb's
  pure-Rust connection (no libX11/libxcb at build or run time). They are listen-only and need
  no special rights; any X client can read them, which is a property of X11 itself. Announcing
  XI 2.2 gets the 2.1 rule that raw events ignore keyboard grabs. Only the key code (`detail`)
  is read; no keysyms, XKB state or text are requested. Events flagged `KeyRepeat` are skipped.
  `event_ns` comes from the server timestamp (Xorg and XWayland use `CLOCK_MONOTONIC`
  milliseconds, so it is good to about 1 ms); an implausible age (a remote server) falls back
  to the receive time. If the X server goes away the thread ends silently (the session is
  ending anyway). `has_permission` is always true.
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
- **evdev (implemented, `input/linux/evdev.rs`):**
  - Opens every `/dev/input/event*` read-only and non-blocking, keeps those that report a key
    in the range Esc … F12 (codes 1–88: keyboards, keypads, the keyboard interface of some
    mice; not power buttons, lid switches or plain mice), and never grabs them (`EVIOCGRAB`),
    so the compositor and everyone else still get every event. Only `EV_KEY` code and value
    are read; value 2 (auto-repeat) is ignored.
  - Virtual (uinput) keyboards are included, unlike Windows' injected keys: remappers such as
    keyd, kanata or interception-tools grab the physical keyboard and re-emit through one, so
    ignoring them would silence those users. `ydotool`-style typing is therefore heard.
  - Permission: if device nodes exist but none can be opened (`EACCES`/`EPERM`) and no keyboard
    is readable, `input::start` fails with `InputError::PermissionDenied`, whose message on
    Linux explains the `input` group, the command and the security cost. A missing
    `/dev/input` (sandbox) is `InputError::Platform`. No keyboard at all (nothing plugged in)
    starts anyway and waits for hotplug. `has_permission` (and `request_permission`, which
    cannot prompt) report whether a keyboard is readable, by opening the nodes; it is false
    only in the denied case.
  - Hotplug: inotify on `/dev/input` for `IN_CREATE` and `IN_ATTRIB` (udev sets the node's
    permissions after creating it); a queue overflow rescans the directory. Unplugged devices
    fail their next read (`ENODEV`) and are dropped, together with the held-key state.
  - `SYN_DROPPED` (the kernel buffer for a device overflowed): events up to the next
    `SYN_REPORT` are discarded, held keys are forgotten and the drop is counted for
    `Listener::take_reenabled`.
  - Timestamps: `EVIOCSCLOCKID(CLOCK_MONOTONIC)` makes the kernel stamp events on the monotonic
    clock with microsecond resolution; `event_ns` is derived from it (receive time if a kernel
    refuses the ioctl).
  - Thread priority is the default: raising it needs privileges TakTak does not ask for.
- **Mapping:** evdev and XInput2 report Linux key codes (`KEY_A` …; X11 key codes are those
  plus 8), which map to `Key` by position like the macOS virtual key codes (`keymap_linux.rs`).
  No keysym or layout lookups, so no characters. This assumes the X server uses evdev key codes
  (the evdev and libinput drivers, XWayland: every current setup); the long-obsolete `kbd`
  driver's key codes would map wrongly. `KEY_FN` (464) only arrives through evdev (X11 key
  codes stop at 255), and Lang3/Lang4 (Katakana/Hiragana) and Zenkaku/Hankaku have no `Key`.
- **App side (implemented, Milestone 5):** `permissionRequired` stays macOS-only, but the app
  sets `onboarding.inputGroupNeeded` when `has_permission` reports the devices unreadable, or
  the listener was refused (`InputError::PermissionDenied`: X11 failed and the evdev fallback
  could not open the devices). The welcome window then shows a Linux step: why the devices
  are read directly, `sudo usermod -aG input $USER` with a Copy button, log out and back in,
  and the cost (every program the user runs can read every keystroke). The status reads "Needs
  keyboard access" (not "Key sounds unavailable"), with a notice and a tray item that open the
  window. It opens by itself only on the first launch, so declining the opt-in is not nagged
  about, and TakTak never asks for root. No restart is suggested in that state (it would not
  help).
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
| Windows | none (`has_permission` is always true) | "TakTak can't hear keys in apps running as administrator." Keys from on-screen keyboards and automation tools are silent (injected events are ignored). A listener that fails to start: "Key listener stopped", Quit & Reopen (no macOS wording). |
| Linux X11 | none (XInput2) | Nothing special. A listener that fails to start: as Windows. |
| Linux Wayland | opt-in `input` group (evdev; `InputError::PermissionDenied` until then) | What the group allows (every keystroke, for every program), how to join it, and that a log out is needed. Shown by the app since Milestone 5 (`onboarding.inputGroupNeeded`). |
