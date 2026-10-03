//! Which app is in front, and the apps the user can pick for per-app rules
//! (`docs/ui-contract.md` § Per-app rules, § Auto-mute, § Privacy (Milestone 4)).
//!
//! macOS, through objc2:
//! - [`observe`]: event-driven observers, registered on the main thread and removed on quit
//!   ([`stop_observing`]). `NSWorkspace` notifications for the frontmost app
//!   (`didActivateApplication`, plus one [`frontmost`] read at start) and the user
//!   session (`sessionDidResignActive` / `sessionDidBecomeActive`, fast user switching), and the
//!   distributed `com.apple.screenIsLocked` / `com.apple.screenIsUnlocked` notifications. TakTak's
//!   own activations are ignored, so the frontmost app stays the one the user came from. Nothing
//!   polls.
//! - [`running_apps`], [`icons`]: the running regular apps, and app icons as 32 × 32 PNG
//!   `data:` URLs, kept in a small in-memory cache ([`IconCache`], never persisted) for listed,
//!   chosen and running-list apps only.
//! - [`choose_app`]: the native picker for an `.app` bundle; reads only its `Info.plist`
//!   (identifier, names) and icon.
//!
//! Elsewhere these are stubs: per-app rules are unsupported, and nothing is observed.
//!
//! Privacy: the frontmost app is handed to the [`Handler`] and nowhere else. Nothing here logs
//! it, keeps a history of it or writes it anywhere.

use crate::state::{AppRef, OWN_APP_ID};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// What the observers report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemEvent {
    /// Another app came to the front (`None`: one without a bundle identifier).
    Frontmost(Option<AppRef>),
    /// The screen locked (`true`) or unlocked.
    ScreenLocked(bool),
    /// This user session became active (`true`) or inactive (fast user switching).
    SessionActive(bool),
}

/// Receives [`SystemEvent`]s, on the main thread; it must return quickly.
pub type Handler = Arc<dyn Fn(SystemEvent) + Send + Sync>;

/// The most ids `get_app_icons` looks up per call.
pub const MAX_ICON_IDS: usize = 200;
/// Commands on a platform without per-app rules.
pub const UNSUPPORTED: &str = "Per-app rules aren’t available on this system yet.";
/// `choose_app`: the chosen bundle has no `CFBundleIdentifier`.
pub const NO_BUNDLE_ID: &str = "That app has no bundle identifier, so TakTak can’t tell it apart.";
/// `choose_app`: a picker is already open.
pub const PICKER_OPEN: &str = "The app chooser is already open.";

/// The frontmost app as TakTak names it: needs a bundle identifier; the name falls back to it.
pub fn app_ref(id: Option<String>, name: Option<String>) -> Option<AppRef> {
    let id = id.map(|id| id.trim().to_owned()).filter(|id| !id.is_empty())?;
    let name = name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
    Some(AppRef { name: name.unwrap_or_else(|| id.clone()), id })
}

/// One running app, as the platform reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Running {
    pub id: Option<String>,
    pub name: Option<String>,
    /// A regular app: it has a Dock icon (activation policy "regular").
    pub regular: bool,
    /// TakTak itself.
    pub own: bool,
}

/// The `(id, name)` pairs `list_running_apps` offers: regular apps with a bundle identifier,
/// not TakTak, one per id (the first), sorted by name ignoring case, then by id.
pub fn pickable(apps: Vec<Running>) -> Vec<AppRef> {
    let mut seen = HashSet::new();
    let mut picked: Vec<AppRef> = apps
        .into_iter()
        .filter(|app| app.regular && !app.own)
        .filter_map(|app| app_ref(app.id, app.name))
        .filter(|app| app.id != OWN_APP_ID && seen.insert(app.id.clone()))
        .collect();
    picked.sort_by_cached_key(|app| (app.name.to_lowercase(), app.id.clone()));
    picked
}

/// An app bundle's display name: the first non-empty of `candidates` (the localized
/// `CFBundleDisplayName`, then `CFBundleName`), else `file_name` without `.app`.
pub fn display_name(candidates: &[Option<String>], file_name: &str) -> String {
    candidates
        .iter()
        .flatten()
        .map(|name| name.trim())
        .find(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| file_name.strip_suffix(".app").unwrap_or(file_name).to_owned())
}

/// A PNG as a `data:` URL.
pub fn data_url(png: &[u8]) -> String {
    use base64::Engine;
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

/// The ids `get_app_icons` looks up: each once, at most [`MAX_ICON_IDS`].
pub fn icon_ids(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter().filter(|id| seen.insert(id.clone())).take(MAX_ICON_IDS).collect()
}

/// The most icons [`IconCache`] keeps; the least recently used goes first.
pub const ICON_CACHE_SIZE: usize = 256;

/// App icons by bundle id for this session (`None`: the app has no icon). In memory only, at
/// most [`ICON_CACHE_SIZE`], least recently used evicted first. Only apps the user listed, chose
/// or was shown in the running-apps list go in, never the frontmost app as such
/// ([`icon_lookups`]), so it never adds up to a record of the apps the user visited.
#[derive(Debug, Default)]
pub struct IconCache {
    icons: HashMap<String, (Option<String>, u64)>,
    /// Bumped on every use; an entry's value is when it was last used.
    clock: u64,
}

impl IconCache {
    pub fn get(&mut self, id: &str) -> Option<Option<String>> {
        self.clock += 1;
        let now = self.clock;
        self.icons.get_mut(id).map(|(icon, used)| {
            *used = now;
            icon.clone()
        })
    }

    pub fn insert(&mut self, id: &str, icon: Option<String>) {
        self.clock += 1;
        if !self.icons.contains_key(id) && self.icons.len() >= ICON_CACHE_SIZE {
            let oldest =
                self.icons.iter().min_by_key(|(_, (_, used))| *used).map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                self.icons.remove(&oldest);
            }
        }
        self.icons.insert(id.to_owned(), (icon, self.clock));
    }

    pub fn len(&self) -> usize {
        self.icons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }
}

/// `get_app_icons`' ids split into those whose icons may be cached (apps on the rule list) and
/// those looked up once and not kept (anything else, such as the frontmost app, whose icon the
/// UI asks for on every change: caching those would collect the apps the user visited).
pub fn icon_lookups<'a>(
    ids: Vec<String>,
    listed: impl IntoIterator<Item = &'a str>,
) -> (Vec<String>, Vec<String>) {
    let listed: HashSet<&str> = listed.into_iter().collect();
    ids.into_iter().partition(|id| listed.contains(id.as_str()))
}

#[cfg(target_os = "macos")]
pub use mac::{choose_app, frontmost, icons, observe, running_apps, stop_observing};

#[cfg(not(target_os = "macos"))]
pub use stub::{choose_app, frontmost, icons, observe, running_apps, stop_observing};

#[cfg(target_os = "macos")]
mod mac {
    use super::{Handler, IconCache, NO_BUNDLE_ID, PICKER_OPEN, SystemEvent};
    use crate::state::{AppInfo, AppRef, OWN_APP_ID};
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBitmapImageFileType, NSBitmapImageRep,
        NSCompositingOperation, NSDeviceRGBColorSpace, NSGraphicsContext, NSImage,
        NSImageInterpolation, NSModalResponse, NSModalResponseOK, NSOpenPanel,
        NSRunningApplication, NSWorkspace, NSWorkspaceApplicationKey,
        NSWorkspaceDidActivateApplicationNotification,
        NSWorkspaceSessionDidBecomeActiveNotification,
        NSWorkspaceSessionDidResignActiveNotification,
    };
    use objc2_core_foundation::{
        CFDictionary, CFNotificationCenter, CFNotificationName, CFNotificationSuspensionBehavior,
        CFRetained, CFString,
    };
    use objc2_foundation::{
        NSArray, NSBundle, NSDictionary, NSFileManager, NSNotification, NSNotificationCenter,
        NSNotificationName, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
    };
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

    /// Icons are rendered at this many points, at 2× (32 × 32 px).
    const ICON_POINTS: f64 = 16.0;
    const ICON_PIXELS: isize = 32;
    const SCREEN_LOCKED: &str = "com.apple.screenIsLocked";
    const SCREEN_UNLOCKED: &str = "com.apple.screenIsUnlocked";

    /// The registered observers; main thread only.
    struct Observers {
        workspace: Retained<NSNotificationCenter>,
        tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
        distributed: Option<CFRetained<CFNotificationCenter>>,
    }

    thread_local! {
        static OBSERVERS: RefCell<Option<Observers>> = const { RefCell::new(None) };
    }

    /// Where the distributed-notification callbacks (plain C functions) send their events.
    static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);
    /// The observer's identity in the distributed center: any address unique to TakTak.
    static DISTRIBUTED_OBSERVER: u8 = 0;
    static ICONS: LazyLock<Mutex<IconCache>> = LazyLock::new(Mutex::default);
    static PICKER_SHOWN: AtomicBool = AtomicBool::new(false);

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn own_pid() -> libc::pid_t {
        // SAFETY: getpid has no preconditions and cannot fail.
        unsafe { libc::getpid() }
    }

    fn string(s: Option<Retained<NSString>>) -> Option<String> {
        s.map(|s| s.to_string())
    }

    /// Whether `app` is this TakTak (or another copy of it).
    fn is_own(app: &NSRunningApplication) -> bool {
        app.processIdentifier() == own_pid()
            || *app == *NSRunningApplication::currentApplication()
            || string(app.bundleIdentifier()).is_some_and(|id| id == OWN_APP_ID)
    }

    fn app_ref_of(app: &NSRunningApplication) -> Option<AppRef> {
        super::app_ref(string(app.bundleIdentifier()), string(app.localizedName()))
    }

    /// The app a workspace notification is about.
    fn notified_app(
        notification: NonNull<NSNotification>,
    ) -> Option<Retained<NSRunningApplication>> {
        // SAFETY: the notification center passes a valid notification for the block's duration.
        let notification = unsafe { notification.as_ref() };
        // SAFETY: an AppKit constant string.
        let key: &NSString = unsafe { NSWorkspaceApplicationKey };
        notification.userInfo()?.objectForKey(key)?.downcast::<NSRunningApplication>().ok()
    }

    fn add_observer(
        center: &NSNotificationCenter,
        name: &NSNotificationName,
        on_post: impl Fn(NonNull<NSNotification>) + 'static,
    ) -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
        let block = RcBlock::new(on_post);
        // SAFETY: no object filter, no queue (workspace notifications are posted on the main
        // thread, so the block runs there); the block only reads the notification and calls
        // the handler, which is Send + Sync.
        unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) }
    }

    fn dispatch(event: SystemEvent) {
        let handler = lock(&HANDLER).clone();
        if let Some(handler) = handler {
            handler(event);
        }
    }

    unsafe extern "C-unwind" fn on_screen_locked(
        _center: *mut CFNotificationCenter,
        _observer: *mut c_void,
        _name: *const CFNotificationName,
        _object: *const c_void,
        _info: *const CFDictionary,
    ) {
        dispatch(SystemEvent::ScreenLocked(true));
    }

    unsafe extern "C-unwind" fn on_screen_unlocked(
        _center: *mut CFNotificationCenter,
        _observer: *mut c_void,
        _name: *const CFNotificationName,
        _object: *const c_void,
        _info: *const CFDictionary,
    ) {
        dispatch(SystemEvent::ScreenLocked(false));
    }

    fn distributed_observer() -> *const c_void {
        std::ptr::from_ref(&DISTRIBUTED_OBSERVER).cast()
    }

    /// The app in front now, unless it is TakTak (or has no bundle identifier). Main thread.
    pub fn frontmost() -> Option<AppRef> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        if is_own(&app) { None } else { app_ref_of(&app) }
    }

    /// Registers the observers; [`SystemEvent`]s go to `handler` from then on. Main thread only
    /// (returns `false` elsewhere). Returns whether per-app rules work.
    pub fn observe(handler: Handler) -> bool {
        if MainThreadMarker::new().is_none() {
            log::warn!("per-app rules are off: the observers must start on the main thread");
            return false;
        }
        if OBSERVERS.with_borrow(Option::is_some) {
            return true;
        }
        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();
        let mut tokens = Vec::new();
        let on_activate = handler.clone();
        // SAFETY (all three): AppKit constant strings.
        let activated = unsafe { NSWorkspaceDidActivateApplicationNotification };
        tokens.push(add_observer(&center, activated, move |notification| {
            if let Some(app) = notified_app(notification).filter(|app| !is_own(app)) {
                on_activate(SystemEvent::Frontmost(app_ref_of(&app)));
            }
        }));
        for (name, active) in [
            (unsafe { NSWorkspaceSessionDidResignActiveNotification }, false),
            (unsafe { NSWorkspaceSessionDidBecomeActiveNotification }, true),
        ] {
            let handler = handler.clone();
            tokens.push(add_observer(&center, name, move |_| {
                handler(SystemEvent::SessionActive(active));
            }));
        }

        *lock(&HANDLER) = Some(handler);
        // The distributed center through CoreFoundation, so delivery is immediate even while
        // TakTak is not the active app (AppKit suspends distributed notifications for inactive
        // apps unless the observer asks for immediate delivery).
        let distributed = CFNotificationCenter::distributed_center();
        if let Some(center) = &distributed {
            type Callback = unsafe extern "C-unwind" fn(
                *mut CFNotificationCenter,
                *mut c_void,
                *const CFNotificationName,
                *const c_void,
                *const CFDictionary,
            );
            let callbacks: [(&str, Callback); 2] =
                [(SCREEN_LOCKED, on_screen_locked), (SCREEN_UNLOCKED, on_screen_unlocked)];
            for (name, callback) in callbacks {
                let name = CFString::from_static_str(name);
                // SAFETY: the observer is a static address, the callbacks match the signature,
                // and the registration is removed in `stop_observing`.
                unsafe {
                    center.add_observer(
                        distributed_observer(),
                        Some(callback),
                        Some(&name),
                        std::ptr::null(),
                        CFNotificationSuspensionBehavior::DeliverImmediately,
                    );
                }
            }
        } else {
            log::warn!("cannot watch the screen lock: no distributed notification center");
        }
        OBSERVERS.set(Some(Observers { workspace: center, tokens, distributed }));
        log::debug!("watching the frontmost app, the screen lock and the session");
        true
    }

    /// Removes the observers (on quit). Main thread only.
    pub fn stop_observing() {
        let Some(observers) = OBSERVERS.take() else { return };
        for token in &observers.tokens {
            let token: &AnyObject = AsRef::<AnyObject>::as_ref(&**token);
            // SAFETY: a token `addObserverForName:…` returned for this center.
            unsafe { observers.workspace.removeObserver(token) };
        }
        if let Some(center) = &observers.distributed {
            // SAFETY: the observer registered in `observe`.
            unsafe { center.remove_every_observer(distributed_observer()) };
        }
        *lock(&HANDLER) = None;
        log::debug!("stopped watching the frontmost app, the screen lock and the session");
    }

    /// `image` drawn at 16 pt @2x, as a PNG `data:` URL.
    fn png_data_url(image: &NSImage) -> Option<String> {
        // SAFETY: no planes (AppKit allocates them), 8-bit RGBA, AppKit's color space constant.
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                ICON_PIXELS,
                ICON_PIXELS,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }?;
        let size = NSSize::new(ICON_POINTS, ICON_POINTS);
        rep.setSize(size);
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        context.setImageInterpolation(NSImageInterpolation::High);
        image.drawInRect_fromRect_operation_fraction(
            NSRect::new(NSPoint::new(0.0, 0.0), size),
            NSRect::ZERO,
            NSCompositingOperation::SourceOver,
            1.0,
        );
        context.flushGraphics();
        NSGraphicsContext::restoreGraphicsState_class();
        // SAFETY: an empty property dictionary is valid for PNG.
        let png = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        }?;
        Some(super::data_url(&png.to_vec()))
    }

    /// `id`'s cached icon, or `render`'s, which is then cached if `keep`.
    fn cached_icon(
        id: &str,
        keep: bool,
        render: impl FnOnce() -> Option<Retained<NSImage>>,
    ) -> Option<String> {
        if let Some(icon) = lock(&ICONS).get(id) {
            return icon;
        }
        let icon = render().and_then(|image| png_data_url(&image));
        if keep {
            lock(&ICONS).insert(id, icon.clone());
        }
        icon
    }

    /// The running regular apps, without TakTak, one per id, sorted by name; without icons
    /// (see [`icons`]), so this is quick. Main thread only.
    pub fn running_apps() -> Vec<AppRef> {
        let running = NSWorkspace::sharedWorkspace()
            .runningApplications()
            .to_vec()
            .into_iter()
            .map(|app| super::Running {
                id: string(app.bundleIdentifier()),
                name: string(app.localizedName()),
                regular: app.activationPolicy() == NSApplicationActivationPolicy::Regular,
                own: is_own(&app),
            })
            .collect();
        super::pickable(running)
    }

    /// An icon for each of `ids` (`None`: macOS cannot find the app, or it has no icon): a
    /// running app's own icon, else the icon of the app LaunchServices finds for the id. Cached
    /// if `keep` (see [`super::icon_lookups`]), except apps not found (they may be installed
    /// later); an icon already cached is reused either way. Main thread only; call it with a
    /// few ids at a time, rendering takes a few milliseconds per icon.
    pub fn icons(ids: &[String], keep: bool) -> HashMap<String, Option<String>> {
        let workspace = NSWorkspace::sharedWorkspace();
        ids.iter()
            .map(|id| {
                let cached = lock(&ICONS).get(id);
                let icon = cached.unwrap_or_else(|| {
                    let bundle_id = NSString::from_str(id);
                    let running =
                        NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id)
                            .to_vec()
                            .into_iter()
                            .find_map(|app| app.icon());
                    if let Some(image) = running {
                        return cached_icon(id, keep, || Some(image));
                    }
                    let path = workspace
                        .URLForApplicationWithBundleIdentifier(&bundle_id)
                        .and_then(|url| url.path())?;
                    cached_icon(id, keep, || Some(workspace.iconForFile(&path)))
                });
                (id.clone(), icon)
            })
            .collect()
    }

    /// The `AppInfo` of the app bundle at `url`, from its `Info.plist`.
    fn app_info_at(url: &NSURL) -> Result<AppInfo, String> {
        let bundle = NSBundle::bundleWithURL(url).ok_or(NO_BUNDLE_ID)?;
        let id = string(bundle.bundleIdentifier())
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty())
            .ok_or(NO_BUNDLE_ID)?;
        let info_string = |key: &str| {
            bundle
                .objectForInfoDictionaryKey(&NSString::from_str(key))
                .and_then(|value| value.downcast::<NSString>().ok())
                .map(|value| value.to_string())
        };
        let path = url.path().unwrap_or_else(|| NSString::from_str(""));
        let file_name = NSFileManager::defaultManager().displayNameAtPath(&path).to_string();
        let name = super::display_name(
            &[info_string("CFBundleDisplayName"), info_string("CFBundleName")],
            &file_name,
        );
        let workspace = NSWorkspace::sharedWorkspace();
        let icon = cached_icon(&id, true, || Some(workspace.iconForFile(&path)));
        Ok(AppInfo { id, name, icon_data_url: icon })
    }

    /// Opens the picker for an app bundle (starting in /Applications) and calls `done` with the
    /// chosen app, `None` when cancelled, or a message for the user. Returns at once; `done`
    /// runs on the main thread when the picker closes. Main thread only.
    pub fn choose_app(done: Box<dyn FnOnce(Result<Option<AppInfo>, String>) + Send>) {
        let Some(mtm) = MainThreadMarker::new() else {
            done(Err("The app chooser could not be opened.".to_owned()));
            return;
        };
        if PICKER_SHOWN.swap(true, Ordering::SeqCst) {
            done(Err(PICKER_OPEN.to_owned()));
            return;
        }
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);
        panel.setResolvesAliases(true);
        let bundles =
            NSArray::from_retained_slice(&[NSString::from_str("com.apple.application-bundle")]);
        // `allowedContentTypes` needs UniformTypeIdentifiers; the UTI string works the same.
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&bundles));
        panel.setDirectoryURL(NSURL::from_directory_path("/Applications").as_deref());
        panel.setPrompt(Some(&NSString::from_str("Choose")));
        panel.setMessage(Some(&NSString::from_str("Choose an app for TakTak’s per-app rules.")));
        // An accessory app is not activated by showing a window; bring the picker forward.
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);

        let done = Cell::new(Some(done));
        let chosen = panel.clone();
        let handler = RcBlock::new(move |response: NSModalResponse| {
            PICKER_SHOWN.store(false, Ordering::SeqCst);
            let result = if response == NSModalResponseOK {
                chosen.URL().map(|url| app_info_at(&url)).transpose()
            } else {
                Ok(None)
            };
            if let Some(done) = done.take() {
                done(result);
            }
        });
        panel.beginWithCompletionHandler(&handler);
    }
}

#[cfg(not(target_os = "macos"))]
mod stub {
    //! No per-app rules on this platform yet (Windows: exe path; Linux: `WM_CLASS`).
    use super::{Handler, UNSUPPORTED};
    use crate::state::{AppInfo, AppRef};
    use std::collections::HashMap;

    pub fn frontmost() -> Option<AppRef> {
        None
    }

    pub fn observe(_handler: Handler) -> bool {
        false
    }

    pub fn stop_observing() {}

    pub fn running_apps() -> Vec<AppRef> {
        Vec::new()
    }

    pub fn icons(ids: &[String], _keep: bool) -> HashMap<String, Option<String>> {
        ids.iter().map(|id| (id.clone(), None)).collect()
    }

    pub fn choose_app(done: Box<dyn FnOnce(Result<Option<AppInfo>, String>) + Send>) {
        done(Err(UNSUPPORTED.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(id: Option<&str>, name: Option<&str>, regular: bool, own: bool) -> Running {
        Running { id: id.map(Into::into), name: name.map(Into::into), regular, own }
    }

    #[test]
    fn frontmost_apps_need_a_bundle_id() {
        assert_eq!(
            app_ref(Some("com.apple.Safari".into()), Some("Safari".into())),
            Some(AppRef { id: "com.apple.Safari".into(), name: "Safari".into() })
        );
        assert_eq!(
            app_ref(Some(" us.zoom.xos ".into()), Some("  ".into())),
            Some(AppRef { id: "us.zoom.xos".into(), name: "us.zoom.xos".into() })
        );
        assert_eq!(app_ref(Some("com.x".into()), None).unwrap().name, "com.x");
        assert_eq!(app_ref(None, Some("Unbundled".into())), None);
        assert_eq!(app_ref(Some("  ".into()), Some("Blank".into())), None);
    }

    #[test]
    fn running_apps_are_regular_bundled_unique_sorted_and_not_taktak() {
        let picked = pickable(vec![
            running(Some("com.tinyspeck.slackmacgap"), Some("Slack"), true, false),
            running(Some("com.apple.Safari"), Some("Safari"), true, false),
            running(Some("com.apple.dock"), Some("Dock"), false, false),
            running(None, Some("Unbundled"), true, false),
            running(Some("tech.taktak.app"), Some("TakTak"), true, false),
            running(Some("dev.taktak.debug"), Some("taktak"), true, true),
            running(Some("com.apple.Safari"), Some("Safari (2)"), true, false),
            running(Some("com.figma.Desktop"), Some("figma"), true, false),
            running(Some("org.mozilla.firefox"), None, true, false),
        ]);
        let names: Vec<(&str, &str)> =
            picked.iter().map(|a| (a.name.as_str(), a.id.as_str())).collect();
        assert_eq!(
            names,
            [
                ("figma", "com.figma.Desktop"),
                ("org.mozilla.firefox", "org.mozilla.firefox"),
                ("Safari", "com.apple.Safari"),
                ("Slack", "com.tinyspeck.slackmacgap"),
            ]
        );
    }

    #[test]
    fn bundle_names_prefer_the_display_name() {
        let s = |v: &str| Some(v.to_owned());
        assert_eq!(
            display_name(&[s("Visual Studio Code"), s("Code")], "Visual Studio Code.app"),
            "Visual Studio Code"
        );
        assert_eq!(display_name(&[None, s("Code")], "x.app"), "Code");
        assert_eq!(display_name(&[s(" "), s("")], "Some App.app"), "Some App");
        assert_eq!(display_name(&[None, None], "Plain"), "Plain");
    }

    #[test]
    fn icons_are_png_data_urls_and_lookups_are_capped() {
        assert_eq!(data_url(&[0x89, b'P', b'N', b'G']), "data:image/png;base64,iVBORw==");
        let ids: Vec<String> = (0..300).map(|i| format!("com.example.app{}", i % 250)).collect();
        let ids = icon_ids(ids);
        assert_eq!(ids.len(), MAX_ICON_IDS);
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), MAX_ICON_IDS, "each once");
        let mut cache = IconCache::default();
        assert_eq!(cache.get("a.b"), None);
        cache.insert("a.b", None);
        assert_eq!(cache.get("a.b"), Some(None), "known to have no icon");
        cache.insert("c.d", Some("data:x".into()));
        assert_eq!(cache.get("c.d"), Some(Some("data:x".into())));
    }

    #[test]
    fn the_icon_cache_is_bounded_and_evicts_the_least_recently_used() {
        let mut cache = IconCache::default();
        for i in 0..ICON_CACHE_SIZE {
            cache.insert(&format!("app{i}"), None);
        }
        assert_eq!(cache.len(), ICON_CACHE_SIZE);
        assert_eq!(cache.get("app0"), Some(None), "app0 used again: app1 is now the oldest");
        cache.insert("app0", Some("data:y".into()));
        assert_eq!(cache.len(), ICON_CACHE_SIZE, "replacing an entry evicts nothing");
        for i in 0..1000 {
            cache.insert(&format!("new{i}"), None);
            assert!(cache.len() <= ICON_CACHE_SIZE);
        }
        assert_eq!(cache.len(), ICON_CACHE_SIZE);
        assert_eq!(cache.get("app1"), None, "evicted");

        let mut cache = IconCache::default();
        for i in 0..ICON_CACHE_SIZE {
            cache.insert(&format!("app{i}"), None);
        }
        cache.get("app0");
        cache.insert("extra", None);
        assert_eq!(cache.get("app0"), Some(None), "recently used, kept");
        assert_eq!(cache.get("app1"), None, "least recently used, evicted");
    }

    #[test]
    fn only_listed_apps_icons_are_cached_by_get_app_icons() {
        let ids = vec!["com.apple.Safari".to_owned(), "com.tinyspeck.slackmacgap".to_owned()];
        let (keep, once) = icon_lookups(ids.clone(), ["com.apple.Safari"]);
        assert_eq!(keep, ["com.apple.Safari"]);
        assert_eq!(once, ["com.tinyspeck.slackmacgap"], "e.g. the frontmost app: not kept");
        let (keep, once) = icon_lookups(ids.clone(), []);
        assert!(keep.is_empty());
        assert_eq!(once, ids);
    }
}
