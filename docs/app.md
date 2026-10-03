# Running and building the TakTak app

The app is the Tauri 2 shell in `src-tauri` (Rust) around `taktak-core`, with the tray popover,
settings and onboarding UI in `src/` (Svelte 5 + TypeScript + Vite). How the two talk is
specified in [`ui-contract.md`](ui-contract.md); threads and real-time rules are in
[`architecture.md`](architecture.md). Windows and Linux permission and latency notes are in
[`platform-notes.md`](platform-notes.md).

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
| `npm run dev` | The UI alone in a normal browser at <http://localhost:1420>, answered by the in-memory mock (`src/lib/mock.ts`). `?window=tray` shows the popover, `?window=onboarding` the welcome window. `?scenario=denied,fault,invalid,empty` simulates problems, in any combination; Milestone 4 adds `firstrun`, `relaunch`, `rules`, `switching`, `locked`, `outputchange` and `unsupported` (see the end of [`ui-contract.md`](ui-contract.md)). |
| `TAKTAK_NO_INPUT=1 npm run tauri dev` | Runs without the keyboard listener, so macOS never asks for Input Monitoring. Sounds then stay off: permission is `unknown`, `playing` is false, and the output stream only opens while a preview plays. Preview still works. |
| `TAKTAK_LOG=debug npm run tauri dev` | More detail on stderr (`error`, `warn`, `info` (the default), `debug`, `trace`, `off`). Other crates only log warnings and above. Nothing is ever written to a log file, and no key identity is ever logged. Neither is the frontmost app. |
| `npm run app` | A release `.app` signed with a stable identity, so Input Monitoring survives rebuilds. See [Development signing](#development-signing). |

Debug builds read the bundled packs from the repository's `packs/` when the resource folder
has none, so `cargo run -p taktak` works too. That build loads the UI from the dev server,
though, so start `npm run dev` first.

## Gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check     # svelte-check, fails on warnings
npm test          # vitest, then npm run test:scripts (node --test: scripts/signing.mjs helpers)
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
- `Contents/Resources/packs/<id>/…`: the 9 bundled packs, copied from `packs/` (`bundle.resources`).
- `Contents/Resources/icon.icns`. The tray icons are compiled into the binary.
- `Info.plist` with:
  - `CFBundleIdentifier` `tech.taktak.app`;
  - `CFBundleName` and `CFBundleDisplayName` `TakTak`;
  - `LSUIElement` true, merged from `src-tauri/Info.plist`, so there is no Dock icon or app menu from the first frame. At startup the app also sets the Accessory activation policy.

A release bundle is about 22 MB: a 10 MB binary plus 14 MB of packs.

Signing: `signingIdentity` is `null`, so builds carry only the linker's ad-hoc signature, and
its code identity changes with every build. macOS ties Input Monitoring to that identity, so a
rebuilt app has to be granted the permission again. For local builds, `npm run app` signs with
a stable identity instead (next section). Distribution needs a Developer ID signature and
notarization. `hardenedRuntime` is already on, and no entitlements are needed.

### Development signing

`npm run app` (`scripts/app.mjs`; any `tauri build` options go after `--`) builds
`target/release/bundle/macos/TakTak.app` and signs it with `codesign`:

- with the identity **TakTak Development**, when `security find-identity -v -p codesigning`
  lists it. Every build then has the same code identity, and macOS keeps the Input Monitoring
  grant across rebuilds;
- ad hoc otherwise, with a hint on how to create the certificate. The permission is then lost on
  every rebuild, as with a plain `tauri build`.

`TAKTAK_SIGNING_IDENTITY="Apple Development: …"` picks another identity from that list, for
example an Apple Development certificate you already have (a SHA-1 hash from that list works
too). The script signs by the certificate's SHA-1 hash, so two certificates with the same name
do not make `codesign` fail as ambiguous: it uses the first one listed and asks you to delete
the duplicate in Keychain Access, so later builds keep the same identity. `CARGO_TARGET_DIR` is
honoured; a relative one is taken relative to the repository root (the script hands cargo the
absolute path, since `tauri build` runs cargo from `src-tauri`). Signing runs after the build, without a secure timestamp, so it works offline and with a
self-signed certificate. The script then checks the signature (`codesign --verify --strict`).

Creating the **TakTak Development** certificate, once per Mac:

1. Open Keychain Access. In the menu choose Keychain Access → Certificate Assistant → Create a
   Certificate….
2. Name: `TakTak Development`. Identity Type: Self-Signed Root. Certificate Type: Code Signing.
   Click Create, then Done. It goes into the login keychain.
3. Find the certificate in the login keychain, open it, expand Trust and set "Code Signing" to
   Always Trust. Close the window and enter your password. Without this step
   `find-identity -v` does not list it, and the script says so.
4. Check: `security find-identity -v -p codesigning` lists `"TakTak Development"`, once. If it
   is listed twice with different hashes (the steps were run twice), delete one of the two in
   Keychain Access (login keychain, My Certificates).

The first build signed this way has a new code identity, so macOS still remembers an Input
Monitoring entry for the old ad-hoc signature, which may look enabled but no longer applies.
Reset it once, then grant the permission again when TakTak asks:

```sh
tccutil reset ListenEvent tech.taktak.app
```

After that, rebuilds keep the permission. The certificate is for your own builds only: other
Macs do not trust it, and Gatekeeper still treats the app as unidentified.

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
- checks per-app rule evaluation (each mode with a listed, an unlisted and an unknown frontmost app; nothing blocked where rules are unsupported; `add_rule_app`'s checks);
- checks the auto-mute state machine (lock, inactive session, an armed output change that outlasts a lock until an unmute, no auto-mute while muted by hand or with the setting off, device changes by name);
- checks the onboarding decision;
- checks that settings are saved, reloaded and recovered from a corrupt file, and that a Milestone 3 file migrates (new fields defaulted, onboarding counted as done, a damaged rule list keeps its good entries);
- drives the real service through pack switches, rejected ids, level clamping, mute and enable, a per-app rule closing the gate, and a hot-reloaded user pack that is selected, then broken on disk, then deleted;
- checks that the service keeps the output closed while nothing can play, that a preview (at volume 0) opens it, and that it closes again when the preview is stopped and when the clip ends. Without an output device it checks that the preview is refused with a message;
- starts a service whose settings select a pack TakTak no longer bundles (`deep-thock`) and checks that it moves to the default pack (Buckling Spring) silently, without `activePackError`, and saves that;
- starts a second service while the user pack is broken and checks that it names the pack as broken rather than missing;
- checks that settings are saved on shutdown.

It prints one line per check and exits 0 only if nothing failed. It writes only to a temporary
folder, which it removes. A release run takes about 2 s and debug about 8 s, because decoding
is slow unoptimized.

## Running it

- **Starting:** launch `TakTak.app`. It appears only as a keycap icon in the menu bar, with the tooltip "TakTak". On the first launch, and whenever Input Monitoring is missing, the welcome window opens too (see below).
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
- **Sound output:** the output stream and the keyboard listener run only while a key press can make a sound (sounds on, not muted, not auto-muted, not in an app a per-app rule has silenced for 5 s or longer, Input Monitoring granted) or a preview plays. Otherwise they are closed and `coreaudiod` can let the device sleep. The playing pack stays decoded, so turning sounds on or unmuting takes about 0.1 s. If macOS moves the output to another device (headphones plugged in, AirPods connected), TakTak reopens on that device 0.5 s later. That way the device's own rate and the 64-frame buffer apply, and Settings names the right device.
- **macOS keyboard permission:** TakTak never shows the macOS permission prompt by itself. The startup log says `Input Monitoring: granted` or `Input Monitoring: not granted` once. The onboarding window (next item) explains the permission first; its "Open Input Monitoring Settings" button, like "Grant Input Monitoring" in the popover and Settings, asks macOS to list TakTak (`CGRequestListenEventAccess`, which is `IOHIDRequestAccess` for listening; macOS shows its own prompt the first time only) and opens Privacy & Security → Input Monitoring. TakTak checks again every 2 s and starts listening by itself once the permission is granted, without a restart when macOS allows it. On macOS 11 and 12 the same pane is System Preferences → Security & Privacy → Privacy → Input Monitoring, and its lock must be clicked before the list can change; the welcome window and the error text name both.
- **Welcome window** ("Welcome to TakTak", `onboarding`): opens at startup once the first permission check (and the first listener start) is done, when the onboarding was never closed (a fresh install) or Input Monitoring is missing. It does not open again once the permission is granted and the window was closed once (closing it, or its Done/Later button, sets `onboardingDone`; quitting does not). An existing Milestone 3 settings file counts as done. When macOS reports the permission as granted but the listener still cannot start (`relaunchSuggested`), the window leads with Quit & Reopen and the troubleshooting for a stale entry (remove TakTak from the list with − and add it again, or `tccutil reset ListenEvent tech.taktak.app`). "Show welcome guide" in Settings and "Needs Input Monitoring…" in the tray menu open it again. Where there is no permission step and no key listener (Windows and Linux for now, or `TAKTAK_NO_INPUT=1`), the window says key sounds are not available instead of "You're all set", and the popover and Settings status read `Key sounds unavailable` with an info notice and no permission button.
- **Quit & Reopen** (`relaunch`): quits normally, and the new TakTak waits up to 5 s for the old one to exit instead of handing over to it. From a bundle it is started through LaunchServices (`open -n TakTak.app --args --relaunch`), so macOS attributes Input Monitoring to TakTak itself. If the welcome window was open, the new instance opens it again (`--onboarding`). On Windows and Linux the running instance holds `running.lock` in the app data folder for the same purpose (the single-instance plugin lets go before the process ends).
- **Per-app rules** (macOS, Settings → Apps): "Everywhere", "Only in these apps" or "Never in these apps" (modes `everywhere`, `only`, `never`), by bundle identifier. The frontmost app is tracked by `NSWorkspace` notifications (no polling); TakTak's own windows don't count, so Settings talks about the app you came from, and typing in TakTak's windows follows that app's rule. A blocked app silences keys at once; the output stream and listener close only after 5 s in it (one wake-up), so ⌘Tab through it doesn't reopen the device, and leaving it reopens at once. "Add app" lists the running apps with their icons (rendered once per app, kept in a bounded in-memory cache of at most 256 icons that never takes in the frontmost app's icon as such, so it is no record of visited apps); "Choose app…" opens a file picker for `.app` bundles and reads their `Info.plist`. The rule list (ids and names) is the only thing persisted; the frontmost app is never logged, stored or kept as a history. Limits: Spotlight, Raycast and Alfred panels don't become the frontmost app, so typing there follows the previous app's rule; password fields are always silent (Secure Input). Windows and Linux: the rules are kept and editable but have no effect yet (`rulesSupported` is false).
- **Auto-mute:** always silent while the screen is locked or the user session is inactive (fast user switching): `com.apple.screenIsLocked`/`screenIsUnlocked` (distributed notifications, delivered immediately even while TakTak is inactive) and `NSWorkspace` session notifications. Optional "Mute when the output device changes" (Settings → General, off by default): when the default output device becomes a different device (by name) while sounds are on and not muted by hand, TakTak mutes itself until you unmute (tray Mute, the hotkey or the switch in the UI), turn sounds on, or turn the setting off. The device is checked when the output reopens after a reroute or fault (within about 0.5 s) and, while the output is closed, every 5 s. Auto-mute never changes the manual mute or Sounds On, and closes the output at once.
- **"Edit rules…"** in the popover opens Settings at the Apps section. `open_settings` takes no arguments, so the popover leaves a note `{id, at}` in `localStorage` under `taktak.openSection`; Settings reads and deletes it when it loads, or picks it up from the `storage` event when it is already open. Notes older than 10 s are ignored. Only the section id is stored.
- **Tray status:** while sounds are on and not muted by hand but still silent, the menu starts with the reason (`Muted — screen locked`, `Muted — output device changed`, `Needs Input Monitoring…`, `No sound output`, `Silent in <app>`) and the tooltip says it too. The Mute check is also on during an output-change auto-mute.

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

**Startup:** 12 packs scanned in 26 ms, and Deep Thock loaded in 19 ms. (Measured when TakTak
still bundled the three synthesized packs and Deep Thock was the default. The default is now
Buckling Spring, 9 packs ship, and Buckling Spring's own load at 48 kHz is about 25 ms; see
the load-time table in [architecture.md](architecture.md).)

**Milestone 4 smoke run** (release bundle built into a separate target directory, `TAKTAK_NO_INPUT=1 TAKTAK_LOG=debug`, a throwaway `HOME`, about 12 s, then SIGTERM):

| Run | CPU time at 4 s → 12 s | RSS | Threads | Log |
|---|---|---|---|---|
| Returning user (`onboardingDone` true), no window | 0.25 s → 0.25 s (0 % over the 8 s idle stretch) | 82 MB | 26 | observers started, 12 packs in 36 ms, Deep Thock in 21 ms, no other warnings, observers removed at exit, exit 0 |
| Fresh install, welcome window open | 0.47 s → 0.47 s (0 %) | 108 MB | 29 | as above, plus `state requested by the onboarding window`; quitting with it open left `onboardingDone` unset, exit 0 |

(Also with the earlier 12-pack set and Deep Thock as the default pack.)

The only warning was `second launches cannot reach TakTak: path must be shorter than SUN_LEN`: the throwaway `HOME` made the `instance.sock` path longer than the 104 bytes a Unix socket path may have. A normal home directory is far below that. Idle threads: main, NSEventThread, 14 tokio workers, `taktak-events`, `taktak-settings`, `taktak-loader` and `taktak-control`, all blocked. Under `TAKTAK_NO_INPUT=1` there is no listener and no permission check, so the `Input Monitoring:` line does not appear.

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
- **Unsigned builds:** each rebuild loses Input Monitoring (see Signing), unless built with `npm run app` and a **TakTak Development** certificate.
- **Per-app rules:** Spotlight, Raycast and Alfred panels report the previous app as frontmost. A rule block shorter than 5 s keeps the output open.
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
  - the reopen on a new default device (plugging in headphones);
  - Milestone 4 in the real app: the welcome window's buttons and its live switch to the success state, Quit & Reopen, the app picker, real screen lock, fast user switching and output-change auto-mutes, and the tray status item.

  What has been checked:
  - the self-test;
  - Milestone 4's AppKit code in a throwaway probe on macOS 26: the observers register and are removed (nothing arrives after removal), the frontmost app is read, activations posted into NSWorkspace's center reach the handler (TakTak's own are ignored), session notifications arrive, the running apps are listed in about 8 ms, and icons render as 32 × 32 RGBA PNGs (about 6 ms each in a debug build, then cached);
  - the startup log;
  - Milestone 4 smoke runs of the release bundle (see Measured): the observers start and stop, the welcome window opens on a fresh install and loads the UI, idle CPU stays at 0, and SIGTERM exits 0 without marking the onboarding done;
  - the second-instance handover (`open -n`) and the reopen event (`open` on a running instance);
  - the settings window loading the bundled UI and calling `get_state` over IPC (seen with `TAKTAK_LOG=debug`);
  - both views running without errors under `freezePrototype` in a browser.
