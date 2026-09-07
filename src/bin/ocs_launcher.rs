//! Receives macOS document-open callbacks before the GUI runtime starts.
//!
//! The app bundle uses this helper as its `CFBundleExecutable`. It forwards
//! Finder URLs to a running editor or starts the sibling GUI binary, then
//! stays alive so later opens keep reaching the same AppKit delegate.

#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::ProtocolObject;
#[cfg(target_os = "macos")]
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
#[cfg(target_os = "macos")]
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNotification, NSObject, NSObjectProtocol, NSURL,
};

#[cfg(target_os = "macos")]
use OpenCADStudio::io::single_instance;

/// Name of the real GUI binary inside `Contents/MacOS/`, sibling to this
/// launcher. Must match the packaging script's bundle assembly step.
#[cfg(target_os = "macos")]
const REAL_BINARY_NAME: &str = "OpenCADStudio-App";

/// How long to wait, after `applicationDidFinishLaunching:`, for an
/// `application:openURLs:` callback before concluding this particular launch
/// came with no documents (Dock icon, `open OpenCADStudio.app` with no
/// file). `application:openURLs:` arrives as part of the same startup
/// sequence when it's coming at all — essentially immediately, not after a
/// meaningful delay — so this window is slack, not a user-visible wait.
#[cfg(target_os = "macos")]
const NO_DOCUMENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(400);

/// Set once `application:openURLs:` has handled a launch's documents, so the
/// delayed "no documents" fallback in `applicationDidFinishLaunching:` knows
/// to stay out of the way instead of *also* launching a bare instance.
#[cfg(target_os = "macos")]
static DOCS_HANDLED: AtomicBool = AtomicBool::new(false);

/// The first spawned GUI owns the launcher lifetime.
#[cfg(target_os = "macos")]
static GUI_STARTED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
fn real_binary_path() -> PathBuf {
    let exe = std::env::current_exe().expect("could not resolve the launcher's own path");
    exe.parent()
        .expect("launcher executable has no parent directory")
        .join(REAL_BINARY_NAME)
}

/// Forward to a running editor or start the bundled GUI.
#[cfg(target_os = "macos")]
fn deliver_or_launch(files: &[String]) {
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    if let Some(stream) = single_instance::try_connect_existing() {
        if single_instance::handoff(stream, &paths) {
            reassert_accessory_policy();
            return;
        }
    }
    match std::process::Command::new(real_binary_path())
        .args(files)
        .spawn()
    {
        Ok(mut child) if !GUI_STARTED.swap(true, Ordering::SeqCst) => {
            std::thread::spawn(move || {
                let _ = child.wait();
                std::process::exit(0);
            });
        }
        Ok(_) => {}
        Err(err) => {
            eprintln!("OpenCADStudio launcher: failed to launch the GUI: {err}");
        }
    }
    reassert_accessory_policy();
}

/// Keep the windowless relay out of the Dock after an open event activates it.
#[cfg(target_os = "macos")]
fn reassert_accessory_policy() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm)
            .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

#[cfg(target_os = "macos")]
declare_class!(
    struct Delegate;

    unsafe impl ClassType for Delegate {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "OCSLauncherDelegate";
    }

    impl DeclaredClass for Delegate {}

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[method(applicationDidFinishLaunching:)]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            std::thread::spawn(|| {
                std::thread::sleep(NO_DOCUMENTS_TIMEOUT);
                if !DOCS_HANDLED.swap(true, Ordering::SeqCst) {
                    deliver_or_launch(&[]);
                }
            });
        }

        #[method(application:openURLs:)]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            DOCS_HANDLED.store(true, Ordering::SeqCst);
            let files: Vec<String> = urls
                .iter()
                .filter_map(|url| unsafe { url.path() })
                .map(|path| path.to_string())
                .collect();
            deliver_or_launch(&files);
        }

        /// Start or activate the GUI when the bundle is reopened without files.
        #[method(applicationShouldHandleReopen:hasVisibleWindows:)]
        fn should_handle_reopen(&self, _sender: &NSApplication, has_visible_windows: bool) -> bool {
            if !has_visible_windows {
                deliver_or_launch(&[]);
            }
            true
        }
    }
);

#[cfg(target_os = "macos")]
impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc();
        unsafe { msg_send_id![this, init] }
    }
}

#[cfg(target_os = "macos")]
fn main() {
    let mtm = MainThreadMarker::new().expect("the launcher must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // No window, no Dock icon, no menu bar — this process only relays.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let delegate = Delegate::new(mtm);
    let proto: &ProtocolObject<dyn NSApplicationDelegate> = ProtocolObject::from_ref(&*delegate);
    app.setDelegate(Some(proto));

    // SAFETY: called once, on the main thread, immediately after `setDelegate`.
    unsafe { app.run() };
}

#[cfg(not(target_os = "macos"))]
fn main() {}
