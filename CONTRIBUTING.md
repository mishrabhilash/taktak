# Contributing to TakTak

Thanks for helping. TakTak is small on purpose: a tray app that clicks when you type, fast,
and with **no way to leak what you type**. Every change has to keep both of those true. This
page covers the setup, the gates CI runs, the code style, the privacy rules, and what a sound
pack needs before it can be bundled.

## Development setup

- **Rust** 1.89 or newer (edition 2024): install with [rustup](https://rustup.rs), then make
  sure `cargo` is on your `PATH` (`export PATH="$HOME/.cargo/bin:$PATH"`).
- **Node** 20.19+ or 22.12+ with npm (CI uses Node 22).
- **Platform prerequisites** for Tauri 2:
  - macOS: the Xcode command line tools (`xcode-select --install`).
  - Windows: Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 10/11).
  - Linux: the packages below.

### Linux packages

Debian and Ubuntu (22.04 or newer); CI installs exactly these:

```sh
sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
  libasound2-dev libxdo-dev libxi-dev libx11-dev patchelf pkg-config
```

### First build

```sh
npm ci                    # UI dependencies, exactly as pinned in package-lock.json
npm run tauri dev         # the app, with hot reload for the UI
npm run dev               # the UI alone in a browser, against an in-memory mock backend
```

`TAKTAK_NO_INPUT=1 npm run tauri dev` runs without the keyboard listener (no Input
Monitoring prompt on macOS). On macOS, `npm run app` builds a release `.app` signed with a
stable identity so the permission survives rebuilds. All commands, the self-test and where
files live: [docs/app.md](docs/app.md). How the pieces fit: [docs/architecture.md](docs/architecture.md).

## Repository layout

| Path | What |
|---|---|
| `src-tauri/` | The Tauri 2 app (Rust): tray, windows, settings, the service around the engine. |
| `src-tauri/core/` | `taktak-core`: key listener, real-time audio engine, pack loader, `taktak-pack` CLI. |
| `src/` | The UI: tray popover, settings and onboarding (Svelte 5 + TypeScript + Vite). |
| `packs/` | The bundled sound packs. Everything here ships inside the app. |
| `tools/pack-maker/` | Records or slices keyboard audio into a pack. |
| `tools/synth-packs/` | Pack loudness measurement (and experimental synthesized packs, not bundled). |
| `tools/pack-sources/` | Scripts that rebuild each bundled pack from its original downloads. |
| `scripts/` | Release, signing, notices and offline-guard scripts (Node, no dependencies). |
| `docs/` | Specs and notes. Start with `architecture.md`, `app.md`, `pack-format.md`. |

## Gates

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs these on macOS, Windows and
Linux. Run them before you open a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check             # svelte-check, fails on warnings
npm test                  # vitest, then the scripts' node --test suites
npm run build
```

CI also runs, once on Linux:

```sh
cargo fetch --locked                         # once: crates for every target, for the offline checks
TAURI_ENV_PLATFORM=linux npm run build       # the UI as it ships (no browser mock)
npm run no-network                           # the offline guard, see Privacy rules
npm run notices -- --check                   # THIRD_PARTY_NOTICES.md is current
```

and the app's headless self-test on every OS (`cargo run -p taktak -- --selftest
--allow-no-audio`). CI runners have no keyboard and usually no audio device. No test may
need either: unit tests must not open audio devices or start the key listener. Code that
needs real hardware belongs behind the self-test (which skips playback with
`--allow-no-audio`) or in a thin layer that tests don't touch (like `pack-maker`'s
`capture.rs`).

**Dependencies changed?** Run `npm run notices` and commit the regenerated
`THIRD_PARTY_NOTICES.md` (it works offline once `cargo fetch` has run). CI fails while the
file is stale. A new crate or npm package also has to pass the offline guard and needs a
reason in the pull request: TakTak keeps its dependency tree small.

## Code style

- **Rust**: `rustfmt` with the repository's `rustfmt.toml`; clippy clean with `-D warnings`.
  Keep `unsafe` to the OS bindings, in small blocks, with a comment on why each one is sound
  (`// SAFETY:` for new code). The real-time audio callback and the key hook
  never allocate, lock, log or block: they talk to other threads through the wait-free rings
  (see docs/architecture.md § Threads).
- **TypeScript/Svelte**: Svelte 5 runes, strict TypeScript, `svelte-check` without warnings.
  The UI talks to the app only through the commands and events in
  [docs/ui-contract.md](docs/ui-contract.md); keep the mock (`src/lib/mock.ts`) in step.
- **Scripts**: plain Node ES modules with no dependencies. Put pure logic in a helper module
  with a `*.test.mjs` next to it (`node --test`).
- **Docs**: when behaviour changes, change the doc that describes it in the same pull request.
  Measured numbers say what was measured, on what, and how.
- **Commits**: small and focused, with a message that says why.

## Privacy rules

TakTak never uses the internet, and it never keeps what you type. These rules are not
negotiable, and CI enforces the mechanical parts.

1. **Never log, store, print or send keys.** Code reads only *which physical key* went down
   or up, never characters or layouts (no `CGEventKeyboardGetUnicodeString`, no keysym or TIS
   lookups). `Key`'s `Debug` output is redacted on purpose: don't add a way around it, don't
   log key names, counts per key, timings or sequences, at any log level, in any build.
2. **Never log or keep the frontmost app.** Per-app rules see the current app's id and name
   and keep only that current value; only the user's own rule list is saved.
3. **No network code and no networking dependencies.** No HTTP clients, TLS, WebSockets,
   DNS, sockets (`std::net`), update checkers, telemetry, crash reporters or analytics, in the
   app or the tools. In the UI: no `fetch`, `XMLHttpRequest`, `WebSocket`, `EventSource` or
   `sendBeacon`, and no remote fonts, images or scripts. The CSP's `connect-src` stays limited
   to Tauri's IPC. The app promises it to every user, in so many words ("TakTak is fully
   offline — it never uses the internet.", `src/lib/offline.ts`, shown in the welcome window,
   Settings → About and the tray popover): any change that would make that untrue is out.
4. **No files about the user.** Settings only; logs go to stderr, never to a file.
5. **Never touch the microphone or the camera.** Audio is output only: no input devices or
   streams and no input-scope CoreAudio queries in the app; no microphone, camera or speech
   recognition usage descriptions or entitlements; no capture APIs in the UI (`mediaDevices`,
   `getUserMedia`, `MediaRecorder`, `SpeechRecognition`, WebRTC); every window is built with
   `src-tauri/src/webview.rs`, which switches WebKit's capture features off. (The pack-maker
   tool records from a microphone when *you* run it; it is never bundled.) See
   [docs/platform-notes.md](docs/platform-notes.md#macos-microphone-and-camera) for the
   system-made checks macOS still logs.

`npm run no-network` ([scripts/no-network.mjs](scripts/no-network.mjs)) enforces rules 3 and
5. It fails on:

- a denylisted networking crate (`reqwest`, `hyper`, `ureq`, `isahc`, `surf`, `curl`,
  `openssl-sys`, `rustls`, `tungstenite`, `h2`, `quinn`, Tauri's http/updater/websocket
  plugins and more; the list is in [scripts/no-network-lib.mjs](scripts/no-network-lib.mjs))
  anywhere in the dependency tree of any workspace crate, for any desktop target;
- the same crates in `Cargo.lock`, except a short allow-list with the reason for each (today:
  `reqwest`, `hyper` and `hyper-util`, which Tauri uses only for its Android and iOS dev-server
  proxy and which never reach a desktop build, as the tree check proves);
- `std::net` in our Rust code, network calls in `src/`, networking npm packages;
- a `connect-src` beyond Tauri's IPC, or an updater/HTTP plugin in `tauri.conf.json`;
- any `http(s)://` URL in the shipped UI bundle other than XML namespace names, Svelte's error
  documentation links and docs/license links.
- a microphone/camera/speech usage description or entitlement in `src-tauri`'s plists,
  entitlements or `tauri.conf.json`, a `Permissions-Policy` that no longer disables camera and
  microphone, or a capture or audio input API in `src/` or the app's Rust code.

Don't extend an allow-list to make a change pass. If you believe a dependency is needed, open
an issue first and explain why it cannot reach the network.

Developer tools may use the network when *you* run them: `npm ci`, `cargo fetch`, and
`tools/pack-sources/*/fetch.sh` (which downloads the original recordings to rebuild a bundled
pack). None of that is part of the app.

## Sound packs

Personal packs need nothing from this page: put them in your user packs folder (see the
[README](README.md#add-your-own-pack)). A pack proposed for **bundling** in `packs/` ships in
an app that anyone may redistribute or sell, so it must meet the
[bundled-pack rules](docs/pack-format.md#bundled-packs), which
`src-tauri/core/tests/bundled_packs.rs` and the loudness test enforce:

- **License**: one of `CC0-1.0`, `CC-BY-3.0`, `CC-BY-4.0`, `MIT`, `0BSD`, `Unlicense`,
  `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`. Never `LicenseRef-Personal`, and never
  NonCommercial, NoDerivatives or ShareAlike licenses. CC BY packs need an `attribution` line,
  and a `LICENSE.txt` in the pack folder when the license wants its notice to travel with
  copies (MIT, BSD, ISC, Apache-2.0, CC BY).
- **Provenance**: a `SOURCES.md` in the pack folder recording, for **every** audio file, where
  it came from (URL, or how it was recorded), the author, proof of the license, its SHA-256 as
  shipped (on the same line as its pack-relative path) and the processing applied. Add a build
  script under `tools/pack-sources/<id>/` so the pack can be rebuilt from the original
  downloads, and a credit section in [CREDITS.md](CREDITS.md).
- **Names**: the folder name equals the pack `id`. No third-party trademarks in `id` or `name`
  (IBM, Model M, Cherry, MX, Gateron, Kailh, Keychron, Razer, Logitech, Corsair, Topre, HHKB,
  Kenney…); the hardware may be named factually in `description`.
- **Loudness**: typing loudness within 0.5 dB of TakTak's reference (−25.8 LK), set with the
  pack's `volume`, unless headroom limits it (then `SOURCES.md` states the shortfall). Check
  with `cargo run -p synth-packs --release -- loudness --check packs/<id>`.
- **Validity**: `cargo run -p taktak-core --bin taktak-pack -- validate --strict packs/<id>`
  passes, and the pack is a folder (not a zip) of at most 6 MB.
- **Only sounds you may share**: your own recordings, or recordings under one of the licenses
  above. `pack-maker` refuses loopback devices, so it can't capture other apps' audio; please
  don't work around that.

The walkthrough is in [docs/adding-a-pack.md](docs/adding-a-pack.md).

## Reporting bugs

Include your OS and version, the TakTak version, the output device (built-in, USB,
Bluetooth) and what you expected. Logs: run TakTak from a terminal with `TAKTAK_LOG=debug`;
by design they never contain keys or app names, so they are safe to paste. Never include
anything you typed while reproducing the bug.

## License

By contributing, you agree that your code is released under the [MIT License](LICENSE), and
that a sound pack you contribute is released under the license stated in its `pack.json`.
