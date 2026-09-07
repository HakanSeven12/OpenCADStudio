//! macOS-only launcher shim, packaged as the .app's `CFBundleExecutable`
//! (`packaging/build_macos_signed.sh`) instead of the real GUI binary
//! (#1039 — "Cannot OPEN WITH in FINDER MAC OS").
//!
//! ## Why this exists
//!
//! Finder never delivers an opened file as a command-line argument — a
//! double-click, "Open With", and even a plain `open -a OpenCADStudio
//! file.dwg` (no `--args`) all resolve it, then hand the running/launching
//! process the path as a `kAEOpenDocuments` Apple Event instead. The real
//! binary only ever reads `std::env::args()` (see `src/cli.rs`), so none of
//! those reached it — Finder just showed "cannot open files in this format".
//!
//! The standard, documented fix is an `NSApplicationDelegate` implementing
//! `application:openURLs:`. That is not available inside the real binary:
//! winit already installs its own delegate to own window/lifecycle events
//! (`platform_impl::macos::app_state::ApplicationDelegate`), `NSApplication`
//! has exactly one delegate slot, and winit's class doesn't implement the
//! open-document method. Several other approaches were tried and measured,
//! not assumed, against real Finder-equivalent opens:
//!
//!   * Registering a raw `AEInstallEventHandler` callback (bypassing the
//!     delegate protocol) from inside the real binary, at every point
//!     tried — before `app::run()`, on `iced::Subscription`'s first poll,
//!     even as the literal first statement of `main()` — reliably caught
//!     every Apple Event sent to an *already-running, cleanly-launched*
//!     process, but never a cold-launch event. `io::macos_open_events` still
//!     carries this: it handles a *second* file opened while this process
//!     is already running.
//!   * A raw `AEInstallEventHandler` in a separate, minimal, non-AppKit
//!     process (no winit/iced, just a manual `CFRunLoopRunInMode` pump)
//!     still missed the cold-launch event, ruling out "the real binary's
//!     heavy dependency graph loading before `main()` wins the race".
//!   * A from-scratch `NSApplication` with our own delegate (this file) does
//!     catch the cold-launch event reliably. But relaying by replacing this
//!     process's image (`exec`, same PID) turned out to have its own
//!     failure mode: Launch Services tracks "the running instance of this
//!     bundle" by the Application Serial Number it handed out at launch —
//!     tied to *this* process, not to the real binary. `exec` doesn't change
//!     that ASN, but a later document open sent to it after the `exec`
//!     silently never arrived anywhere (measured via a real second "Open
//!     With", not assumed) — and a plain `spawn`'d child in place of `exec`
//!     didn't help either: `lsappinfo` showed Launch Services still
//!     addressing *this* launcher's ASN for the next open, now pointed at a
//!     process that had already exited.
//!
//! So this process has to stay alive for the app's session — it is, as far
//! as Launch Services is concerned, "the app". Every open it receives (cold
//! or warm) is relayed to the real GUI binary over the exact channel a
//! second CLI invocation already uses (`io::single_instance`): hand off to
//! an already-running editor if one answers, otherwise launch a fresh one
//! with the files on argv. `NSApplicationActivationPolicyAccessory` keeps it
//! out of the Dock and Cmd-Tab — it owns no window and has nothing to show.
//!
//! Kept intentionally free of every other dependency in this crate (iced,
//! wgpu, cadkernel, …) beyond what relaying needs, so its own startup is as
//! close to instant as possible — plausibly part of why the isolated-process
//! approach above catches the cold-launch event when the full binary didn't.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNotification, NSObject, NSObjectProtocol, NSURL,
};

use OpenCADStudio::io::single_instance;

/// Name of the real GUI binary inside `Contents/MacOS/`, sibling to this
/// launcher. Must match the packaging script's bundle assembly step.
const REAL_BINARY_NAME: &str = "OpenCADStudio-App";

/// How long to wait, after `applicationDidFinishLaunching:`, for an
/// `application:openURLs:` callback before concluding this particular launch
/// came with no documents (Dock icon, `open OpenCADStudio.app` with no
/// file). `application:openURLs:` arrives as part of the same startup
/// sequence when it's coming at all — essentially immediately, not after a
/// meaningful delay — so this window is slack, not a user-visible wait.
const NO_DOCUMENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(400);

/// Set once `application:openURLs:` has handled a launch's documents, so the
/// delayed "no documents" fallback in `applicationDidFinishLaunching:` knows
/// to stay out of the way instead of *also* launching a bare instance.
static DOCS_HANDLED: AtomicBool = AtomicBool::new(false);

fn real_binary_path() -> PathBuf {
    let exe = std::env::current_exe().expect("could not resolve the launcher's own path");
    exe.parent()
        .expect("launcher executable has no parent directory")
        .join(REAL_BINARY_NAME)
}

/// Deliver `files` to a running editor if one answers on the single-instance
/// port, otherwise launch a fresh one with `files` on argv — the same
/// channel a second `OpenCADStudio file.dwg` from a terminal already uses.
/// This process (the launcher) is never itself a candidate to serve that
/// port: `try_connect_existing` only ever connects, never binds, so it can't
/// steal the port out from under the real binary that's either already
/// running or about to claim it on its own.
fn deliver_or_launch(files: &[String]) {
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    let delivered = single_instance::try_connect_existing()
        .map(|stream| single_instance::handoff(stream, &paths))
        .unwrap_or(false);
    if !delivered {
        if let Err(err) = std::process::Command::new(real_binary_path()).args(files).spawn() {
            eprintln!("OpenCADStudio launcher: failed to launch the real binary: {err}");
        }
    }
    reassert_accessory_policy();
}

/// AppKit activates this process — promoting it from
/// NSApplicationActivationPolicyAccessory (no Dock icon, set at startup) to
/// a normal foreground app with its own Dock icon — as a side effect of
/// handling an open-document Apple Event, even though nothing here ever
/// shows a window. Measured via `lsappinfo`, not assumed: the launcher's own
/// entry flips from type="UIElement" to type="Foreground" the moment it
/// handles a *second* Apple Event (a fresh cold launch's own first event
/// doesn't trigger this — the policy is presumably still settling at that
/// point). Set it back immediately after relaying, so a warm "Open With"
/// doesn't leave a second, contentless Dock icon sitting next to the real
/// editor's.
fn reassert_accessory_policy() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm)
            .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

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

        /// Fires when this already-running process is reactivated with no
        /// visible windows to show — clicking the Dock icon, or Finder
        /// launching the bundle again while Launch Services still considers
        /// it running (this launcher's own ASN never dies between opens; see
        /// the module doc). Without this, quitting the real GUI window while
        /// this relay stays alive left the app in a state where reopening it
        /// did nothing: no fresh `applicationDidFinishLaunching:` (that only
        /// fires once, at this process's own cold start) and no
        /// `application:openURLs:` (no document was named).
        #[method(applicationShouldHandleReopen:hasVisibleWindows:)]
        fn should_handle_reopen(&self, _sender: &NSApplication, has_visible_windows: bool) -> bool {
            if !has_visible_windows {
                deliver_or_launch(&[]);
            }
            true
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc();
        unsafe { msg_send_id![this, init] }
    }
}

fn main() {
    let mtm = MainThreadMarker::new().expect("the launcher must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // No window, no Dock icon, no menu bar — this process only relays.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let delegate = Delegate::new(mtm);
    let proto: &ProtocolObject<dyn NSApplicationDelegate> = ProtocolObject::from_ref(&*delegate);
    app.setDelegate(Some(proto));

    // Stays running for the app's session — see the module doc for why:
    // Launch Services keeps addressing future document-opens to this
    // process's Application Serial Number, not to whatever it spawns.
    // SAFETY: called once, on the main thread, immediately after `setDelegate`.
    unsafe { app.run() };
}
