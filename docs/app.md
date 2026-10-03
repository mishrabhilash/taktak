# Running and building the TakTak app

The app is the Tauri 2 shell in `src-tauri` (Rust) around `taktak-core`, with the tray popover
and settings UI in `src/` (Svelte 5 + TypeScript + Vite). How the two talk is specified in
[`ui-contract.md`](ui-contract.md); threads and real-time rules are in
[`architecture.md`](architecture.md).

## Prerequisites

- Rust 1.89 or newer (edition 2024, let-chains, `File::try_lock`): `export PATH="$HOME/.cargo/bin:$PATH"`.
- Node 20 or newer with npm. Then run `npm install` once at the repository root. The Tauri CLI
  is the `@tauri-apps/cli` dev dependency, so `npx tauri …` and `npm run tauri …` both work.
- macOS: the Xcode command line tools. Windows: WebView2 and the MSVC build tools. Linux:
  WebKitGTK 4.1 and the other Tauri 2 prerequisites.

## Develop

| Command | What it does |
|---|---|
| `npm run tauri dev` | Starts Vite on <http://localhost:1420> (`beforeDevCommand`), then builds and runs the debug app against it. UI edits hot-reload; Rust edits rebuild and restart the app. Port 1420 is strict, so stop any other `npm run dev` first. |
| `npm run dev` | The UI alone in a normal browser at <http://localhost:1420>, answered by the in-memory mock (`src/lib/mock.ts`). `?window=tray` shows the popover. `?scenario=denied,fault,invalid,empty` simulates problems, in any combination. |
| `TAKTAK_NO_INPUT=1 npm run tauri dev` | Runs without the keyboard listener, so macOS never asks for Input Monitoring. Sounds then stay off: permission is `unknown`, `playing` is false, and the output stream only opens while a preview plays. Preview still works. |
| `TAKTAK_LOG=debug npm run tauri dev` | More detail on stderr (`error`, `warn`, `info` (the default), `debug`, `trace`, `off`). Other crates only log warnings and above. Nothing is ever written to a log file, and no key identity is ever logged. |

Debug builds read the bundled packs from the repository's `packs/` when the resource folder
has none, so `cargo run -p taktak` works too. That build loads the UI from the dev server,
though, so start `npm run dev` first.

## Gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check     # svelte-check, fails on warnings
npm test          # vitest
npm run build
```

## Build

```sh
npm run release -- --bundles app        # release to distribute: target/release/bundle/macos/TakTak.app
npm run release                         # every target in tauri.conf.json for this OS (.app + .dmg on macOS)
npx tauri build --bundles app           # local release build (keeps your paths in the binary, see below)
npx tauri build --debug --bundles app   # debug:   target/debug/bundle/macos/TakTak.app
```

Build anything you distribute with `npm run release` (`scripts/release.mjs`; any
`tauri build` options go after `--`). A plain `tauri build` compiles absolute source paths
into the binary: panic locations of every dependency name
`<home>/.cargo/registry/src/…`, about 450 of them, so the binary carries the builder's home
directory and account name. The script runs `tauri build` with rustc's
`--remap-path-prefix` for the checkout (`/taktak`), `CARGO_HOME` (`/cargo`), `RUSTUP_HOME`
(`/rustup`) and the home directory (`/home`), added to any `RUSTFLAGS` you set. Afterwards it
checks that the built binary no longer contains the home directory path, and exits non-zero
if it does. Changing the flags rebuilds everything once. To check a binary on its own:
`npm run release -- --check [<binary>…]` (by default `target/release/taktak`; the bundle's
`Contents/MacOS/taktak` is a copy of it). Cargo's `trim-paths` profile setting would make
this the default, but it is not stable yet (Rust 1.99).

`tauri build` runs `npm run build` with `TAURI_ENV_PLATFORM` set. The shipped `dist/` then
holds no mock code or mock data, and the app embeds it and serves it under the CSP from
`tauri.conf.json`.

Don't distribute a plain `cargo build --release`. It lacks the `tauri/custom-protocol`
feature, so its windows try to load the dev server.

The macOS bundle contains:

- `Contents/MacOS/taktak`.
- `Contents/Resources/packs/<id>/…`: the 12 bundled packs, copied from `packs/` (`bundle.resources`).
- `Contents/Resources/icon.icns`. The tray icons are compiled into the binary.
- `Info.plist` with:
  - `CFBundleIdentifier` `tech.taktak.app`;
  - `CFBundleName` and `CFBundleDisplayName` `TakTak`;
  - `LSUIElement` true, merged from `src-tauri/Info.plist`, so there is no Dock icon or app menu from the first frame. At startup the app also sets the Accessory activation policy.

A release bundle is about 22 MB: a 10 MB binary plus 14 MB of packs.

Signing: `signingIdentity` is `null`, so builds carry only the linker's ad-hoc signature, and
its code identity changes with every build. macOS ties Input Monitoring to that identity, so a
rebuilt app has to be granted the permission again. Distribution needs a Developer ID
signature and notarization. `hardenedRuntime` is already on, and no entitlements are needed.

## Self-test

```sh
target/release/bundle/macos/TakTak.app/Contents/MacOS/taktak --selftest
cargo run -p taktak -- --selftest                  # same, debug, repository packs
taktak --selftest --allow-no-audio                 # CI without an output device
taktak --selftest --packs <dir>                    # another bundled-packs folder
```

The self-test is headless: no windows, no tray and no keyboard listener. It:

- scans and loads every bundled pack;
- opens the default output and swaps every pack into the engine, then plays and stops a preview;
- checks the input gate;
- checks that settings are saved, reloaded and recovered from a corrupt file;
- drives the real service through pack switches, rejected ids, level clamping, mute and enable, and a hot-reloaded user pack that is selected, then broken on disk, then deleted;
- checks that the service keeps the output closed while nothing can play, that a preview (at volume 0) opens it, and that it closes again when the preview is stopped and when the clip ends. Without an output device it checks that the preview is refused with a message;
- starts a second service while the user pack is broken and checks that it names the pack as broken rather than missing;
- checks that settings are saved on shutdown.

It prints one line per check and exits 0 only if nothing failed. It writes only to a temporary
folder, which it removes. A release run takes about 2 s and debug about 8 s, because decoding
is slow unoptimized.

## Running it

- **Starting:** launch `TakTak.app`. It appears only as a keycap icon in the menu bar, with the tooltip "TakTak".
- **Tray icon:**
  - left click toggles the popover;
  - right click opens the menu: Sounds On, Pack ▸, Mute (with the hotkey), Settings…, Quit TakTak.
- **Opening it again:** the running app opens or focuses the settings window. This is the way back in when the menu-bar icon is hidden, behind the notch for example.
  - Finder, Spotlight, Launchpad or a plain `open TakTak.app` start no second process on macOS. The running app gets a reopen event (`RunEvent::Reopen`) instead.
  - A second process (`open -n TakTak.app`, or running the binary again) hands over and exits 0 before it creates anything.
  - On macOS the handover uses `src-tauri/src/instance.rs`. It holds an exclusive `flock` on `instance.lock`, and listens on the socket `instance.sock` (mode 0600). Both are in `~/Library/Application Support/tech.taktak.app/`. Each user account gets its own instance, and other accounts can neither block nor impersonate it. Nothing is passed along: the connection alone is the message.
  - On Windows and Linux, `tauri-plugin-single-instance` does the same per user session. (On macOS that plugin uses a fixed socket in the shared `/tmp`, which is why TakTak doesn't use it there.)
  - Checked on macOS with a wrapper bundle around the debug binary: `open` on a running instance logged "opened again" in the same process, and `open -n` logged the handover and exited.
- **Quitting:** use Quit in the menu, the popover or the `quit` command. SIGTERM and SIGINT (`kill <pid>`, Ctrl+C under `tauri dev`) take the same path. All of these save pending settings, stop the audio and remove `instance.sock` before exiting 0, waiting at most 2 s. A second signal exits at once. Only SIGKILL skips this, which can lose up to 300 ms of debounced setting changes. The next launch then replaces the leftover socket. The lock always goes away with the process.
- **Sound output:** the output stream and the keyboard listener run only while a key press can make a sound (sounds on, not muted, Input Monitoring granted) or a preview plays. Otherwise they are closed and `coreaudiod` can let the device sleep. The playing pack stays decoded, so turning sounds on or unmuting takes about 0.1 s. If macOS moves the output to another device (headphones plugged in, AirPods connected), TakTak reopens on that device 0.5 s later. That way the device's own rate and the 64-frame buffer apply, and Settings names the right device.
- **macOS keyboard permission:** the first normal launch asks for Input Monitoring, once per launch at most. TakTak checks again every 2 s and starts listening by itself once the permission is granted. "Grant Input Monitoring" in the popover or the settings window opens the right pane of System Settings.

## Where things live

| What | macOS | Windows | Linux |
|---|---|---|---|
| Settings (`settings.json`, atomic write, debounced 300 ms) | `~/Library/Application Support/tech.taktak.app/` | `%APPDATA%\tech.taktak.app\` | `~/.config/tech.taktak.app/` |
| User packs (watched; hot reload) | `~/Library/Application Support/tech.taktak.app/packs/` | `%APPDATA%\tech.taktak.app\packs\` | `~/.local/share/tech.taktak.app/packs/` |
| Launch at login | `~/Library/LaunchAgents/TakTak.plist` | Run registry key | `~/.config/autostart/` |
| Logs | stderr only | stderr only | stderr only |

A missing or corrupt `settings.json` falls back to the defaults, and a corrupt file is moved
aside. At startup the OS's actual launch-at-login state wins over `launchAtLogin` in the
file. Turning launch at login on in a dev build registers the dev binary.

## Measured (release bundle, MacBook Pro, macOS 26, built-in speakers 44.1 kHz, 64-frame buffer)

Measured with `TAKTAK_NO_INPUT=1`, so there is no listener, and no typing. CPU is the CPU
time used over the window, divided by its length. The two table rows were measured while the
output stream was still always open. TakTak now closes it whenever nothing can play, which is
always the case under `TAKTAK_NO_INPUT=1`. The figures with the stream open still apply
whenever sounds are on.

| State | TakTak CPU | TakTak memory (footprint / RSS) | WebKit helpers (footprint) |
|---|---|---|---|
| No window, stream open, 30 s | 0.3 % | 13 MB / 80 MB | none running |
| Settings window open, stream open, 30 s | 0.3 % | 21 MB / 107 MB | WebContent 42 MB, GPU 16 MB, Networking 5 MB, all at 0 % CPU |

With the stream closed (debug build, no window, `top` over 16 s), TakTak used 0.0 % CPU, and
`coreaudiod` 0.1 % in the one sample nothing else disturbed. A Chrome helper and another
TakTak build with the old always-open stream then started using the speakers, so that figure
still needs a clean rerun.

**Startup:** 12 packs scanned in 26 ms, and Deep Thock loaded in 19 ms.

**CPU:** while the stream is open, about half of TakTak's CPU time goes to the real-time CoreAudio IO thread, which mixes silence 689 times a second. The control thread sleeps except for its 4 Hz engine poll, or a 0.2 Hz look at the default device while the stream is closed.

**Memory:**
- With the settings window open, all four processes together have a footprint of about 84 MB. The RSS figures add up to more, because each counts the shared system frameworks again.
- Closing the settings window destroys its webview.
- The tray popover is destroyed once it has stayed hidden for 60 s, and 1 s after Settings opens, so its WebContent and GPU processes don't stay resident for the whole session. Before this change, one click kept about 45 MB of helpers alive until quit. The steady state after one open and close still needs measuring again. The Networking helper (about 5 MB) may stay.

**System-wide audio cost:** `coreaudiod` uses 5–9 % of one core while any output stream is open on the built-in speakers, against 0 % with none.
- TakTak opens its stream only while a key press can make a sound or a preview plays (see Sound output above).
- A silent test stream costs the same at 64, 128, 256 or 512 frames and at the device default, so a bigger buffer would not help.
- See Known limitations.

## Known limitations

- **Output stream while sounds are on:** the stream stays open whenever a key press could make a sound, even with no typing for hours. That costs about 0.3 % CPU in TakTak plus 5–9 % in `coreaudiod`. It is closed while sounds are off, muted or not permitted. Suspending it after a stretch without keystrokes, at the price of one slow first click (30 ms or more), is a design decision that hasn't been made yet.
- **Windows are never freed (upstream):** each settings window that is closed and each popover that is destroyed leaves its native `NSWindow` behind. Its webview and WebContent process do go away. A review measured about 0.5 MB per Settings open and close, growing linearly, with the closed windows still in `NSApp.windows`. TakTak holds no handle to them, and `close()` and `destroy()` both leak, so the cause is in tao 0.37 / wry 0.57. To report upstream with this repro: open and close a `WebviewWindow` N times, then count `TaoWindow` instances with `heap <pid>`. Destroying the popover after 60 s hidden adds at most one such leak per minute of use, against about 45 MB kept for the whole session before.
- **Unsigned builds:** each rebuild loses Input Monitoring (see Signing).
- **Hotkeys on macOS:**
  - Carbon hotkey registration does not report shortcuts that other apps already use, so most conflicts can't be detected.
  - In the recorder, pressing the current mute shortcut toggles mute instead of being recorded, because the global hotkey gets the keys first.
- **Preview:** `preview_pack` resolves once the clip plays and rejects with a message when it can't play. The button still shows "playing" for a fixed 2.2 s, because the contract has no "preview finished" event.
- **Windows and Linux:** `taktak-core` has no key listener there yet. Permission shows `unknown` and no sounds play, though preview works.
- **Not yet click-tested in the real app:**
  - the tray menu items and the popover's position;
  - the popover's 250 ms reopen guard and Escape to hide;
  - the popover handing the keyboard back to the previous app (macOS hides TakTak when the popover hides), its destruction after 60 s hidden, and its recreation on the next click;
  - the hotkey press itself;
  - live typing with Input Monitoring granted, and the output opening when sounds are turned on or unmuted;
  - the reopen on a new default device (plugging in headphones).

  What has been checked:
  - the self-test;
  - the startup log;
  - the second-instance handover (`open -n`) and the reopen event (`open` on a running instance);
  - the settings window loading the bundled UI and calling `get_state` over IPC (seen with `TAKTAK_LOG=debug`);
  - both views running without errors under `freezePrototype` in a browser.
