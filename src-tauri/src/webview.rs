//! The builder every TakTak window starts from, with media capture switched off.
//!
//! TakTak never touches the microphone or the camera, and its UI has no use for them. On macOS
//! each WebKit view gets a configuration, made before the view exists, that switches off every
//! capture surface: `navigator.mediaDevices` (with it `getUserMedia`, `getDisplayMedia` and
//! `enumerateDevices`), WebRTC (`RTCPeerConnection`) and speech recognition. WebKit would
//! otherwise expose WebRTC and speech recognition, and would turn `navigator.mediaDevices` on
//! by itself if the app ever held camera or microphone permission. WebKit offers these
//! switches only as private `WKPreferences` / `WKWebViewConfiguration` setters; each is called
//! only if this WebKit has it, so a WebKit that drops one changes nothing. `tauri.conf.json`
//! also sends `Permissions-Policy: camera=(), microphone=(), display-capture=()` with every
//! page.
//!
//! What no setting can remove (`docs/platform-notes.md` § Microphone and camera): WebKit
//! preflights microphone and camera permission once per process while it initializes its
//! preferences (to pick the default of the very switch turned off here), and its GPU process
//! preflights the microphone when it starts. Like the audio server's check on every process
//! that plays sound, these are read-only status queries: no prompt, no device opened.

use tauri::{AppHandle, Runtime, WebviewUrl, WebviewWindowBuilder};

/// A window builder for `label`, loading the UI bundle, with media capture off.
pub fn builder<'a, R: Runtime>(
    app: &'a AppHandle<R>,
    label: &str,
) -> WebviewWindowBuilder<'a, R, AppHandle<R>> {
    without_capture(
        app,
        WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into())),
    )
}

#[cfg(target_os = "macos")]
fn without_capture<'a, R: Runtime>(
    app: &AppHandle<R>,
    builder: WebviewWindowBuilder<'a, R, AppHandle<R>>,
) -> WebviewWindowBuilder<'a, R, AppHandle<R>> {
    match macos::configuration(app) {
        Some(configuration) => builder.with_webview_configuration(configuration),
        None => builder,
    }
}

/// WebView2 and WebKitGTK prompt before any capture and TakTak's pages request none; the
/// Permissions-Policy header applies there too.
#[cfg(not(target_os = "macos"))]
fn without_capture<'a, R: Runtime>(
    _app: &AppHandle<R>,
    builder: WebviewWindowBuilder<'a, R, AppHandle<R>>,
) -> WebviewWindowBuilder<'a, R, AppHandle<R>> {
    builder
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::NSObjectProtocol;
    use objc2::{MainThreadMarker, msg_send, sel};
    use objc2_web_kit::WKWebViewConfiguration;
    use std::sync::mpsc;
    use tauri::{AppHandle, Runtime};

    /// A configuration made on the main thread and handed to Tauri, which creates the webview
    /// from it on the main thread.
    struct Configuration(Retained<WKWebViewConfiguration>);
    // SAFETY: the configuration is created on the main thread, not touched by this thread
    // afterwards, and only used by Tauri on the main thread.
    unsafe impl Send for Configuration {}

    /// A fresh configuration with capture off, made on the main thread (immediately when
    /// called there). `None` if the main thread is gone (the app is exiting).
    pub fn configuration<R: Runtime>(
        app: &AppHandle<R>,
    ) -> Option<Retained<WKWebViewConfiguration>> {
        let (tx, rx) = mpsc::channel();
        app.run_on_main_thread(move || {
            if let Some(mtm) = MainThreadMarker::new() {
                let _ = tx.send(Configuration(without_capture(mtm)));
            }
        })
        .ok()?;
        rx.recv().ok().map(|configuration| configuration.0)
    }

    /// Calls the private `BOOL` setter `$setter` with `NO` on `$object`, if it has it.
    macro_rules! disable {
        ($object:expr, $setter:ident) => {
            if $object.respondsToSelector(sel!($setter:)) {
                // SAFETY: the object responds to the selector, a WebKit setter taking a BOOL.
                let () = unsafe { msg_send![&*$object, $setter: false] };
            } else {
                log::debug!("webview: WebKit has no {}", stringify!($setter));
            }
        };
    }

    fn without_capture(mtm: MainThreadMarker) -> Retained<WKWebViewConfiguration> {
        // SAFETY: a plain initializer, on the main thread.
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        disable!(configuration, _setMediaCaptureEnabled);
        // SAFETY: a getter; the preferences object is the configuration's own.
        let preferences = unsafe { configuration.preferences() };
        disable!(preferences, _setMediaDevicesEnabled);
        disable!(preferences, _setMediaStreamEnabled);
        disable!(preferences, _setScreenCaptureEnabled);
        disable!(preferences, _setPeerConnectionEnabled);
        disable!(preferences, _setSpeechRecognitionEnabled);
        configuration
    }
}
