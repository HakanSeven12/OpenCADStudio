//! macOS: catch a `kAEOpenDocuments` Apple Event sent directly to this
//! already-running process — the *warm* half of #1039 ("Cannot OPEN WITH in
//! FINDER MAC OS").
//!
//! [`src/bin/ocs_launcher.rs`] is the `.app`'s actual `CFBundleExecutable`
//! and stays running for the whole session — Launch Services keeps
//! addressing every open (cold *or* warm) to that one process's Application
//! Serial Number, so it relays each one to this binary over the existing
//! `io::single_instance` port rather than over an Apple Event of its own.
//!
//! That covers every open Finder initiates. What it doesn't cover: a plain
//! `open -a OpenCADStudio-App file.dwg`, or an AppleScript `tell application
//! … to open`, aimed at this binary directly rather than at the launcher —
//! bypassing the launcher (and its relay) entirely. Verified against a real
//! second "Open With" while already running (not assumed): Launch Services
//! can deliver a `kAEOpenDocuments` event straight to an already-running,
//! cleanly-launched instance of this binary, and without a handler here that
//! goes unhandled the same way the original bug described.
//!
//! The standard fix — implement `application:openURLs:` on this app's own
//! `NSApplicationDelegate` — isn't available here: winit already installs
//! its own delegate (`platform_impl::macos::app_state::ApplicationDelegate`)
//! to own window/lifecycle events, `NSApplication` has exactly one delegate
//! slot, and winit's class doesn't implement the open-document method. So
//! this bypasses the delegate protocol and registers directly with the
//! lower-level Apple Event Manager (`AEInstallEventHandler`) instead — the
//! same mechanism AppKit's delegate forwarding is itself built on, and
//! independent of who owns the delegate.
//!
//! Registration timing is counter-intuitive and was pinned down by testing
//! against a real `open -a`, not by reasoning from Apple's docs: registering
//! before anything has touched `NSApplication` (e.g. from `main()`, before
//! `app::run()`) does not stick — winit's own `EventLoop::new()`/`-run`
//! startup sequence re-registers AppKit's own default (no-op) handler for
//! the same event afterwards and silently wins. So [`subscribe`]'s stream
//! registers on its own first poll instead, which iced only reaches once the
//! window/application lifecycle has already finished starting — late enough
//! that nothing overwrites it afterwards. (A *cold*-launch event specifically
//! still isn't caught this way, even registered as the first statement of
//! `main()` — which is exactly why that case needs the separate launcher.)

use std::ffi::{c_void, OsString};
use std::os::raw::c_long;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

// ── Apple Event Manager FFI ─────────────────────────────────────────────
//
// Hand-rolled, matching the existing macOS FFI in `file_association.rs`
// rather than pulling in an objc2 dependency for a handful of C calls. Types
// and constants below are taken from the real SDK headers (verified against
// `AE.framework/Headers/{AppleEvents,AEDataModel}.h`, not from memory) so the
// field widths match this target's actual C ABI.

type OSErr = i16;
type OSType = u32;
type AEEventClass = OSType;
type AEEventID = OSType;
type AEKeyword = OSType;
type DescType = OSType;
/// `SRefCon` is `void *` on this (LP64) target — only ever passed as `0`
/// here, so its exact pointee type doesn't matter.
type SRefCon = *mut c_void;
/// Carbon's `Size` is `typedef long Size;` — 8 bytes on 64-bit macOS.
type Size = c_long;
type Boolean = u8;

/// Layout must match the real C `struct AEDesc` (`descriptorType` then
/// `dataHandle`) exactly, including the compiler-inserted padding before the
/// 8-byte pointer field — `repr(C)` reproduces that automatically here.
#[repr(C)]
struct AEDesc {
    descriptor_type: DescType,
    data_handle: *mut c_void,
}
type AppleEvent = AEDesc;
type AEDescList = AEDesc;

const K_CORE_EVENT_CLASS: AEEventClass = 0x6165_7674; // 'aevt'
const K_AE_OPEN_DOCUMENTS: AEEventID = 0x6F64_6F63; // 'odoc'
const KEY_DIRECT_OBJECT: AEKeyword = 0x2D2D_2D2D; // '----'
const TYPE_AE_LIST: DescType = 0x6C69_7374; // 'list'
/// Coercing a list item to this type is what makes `AEGetNthDesc` hand back
/// the file's location — as a percent-encoded `file://` URL in practice
/// (measured against a real Finder-equivalent open), not the plain path the
/// `typeFileURL` comment in the real `AEDataModel.h` claims.
const TYPE_FILE_URL: DescType = 0x6675_726C; // 'furl'

type AEEventHandlerProcPtr = unsafe extern "C" fn(
    the_apple_event: *const AppleEvent,
    reply: *mut AppleEvent,
    handler_refcon: SRefCon,
) -> OSErr;

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn AEInstallEventHandler(
        the_ae_event_class: AEEventClass,
        the_ae_event_id: AEEventID,
        handler: AEEventHandlerProcPtr,
        handler_refcon: SRefCon,
        is_sys_handler: Boolean,
    ) -> OSErr;

    fn AEGetParamDesc(
        the_apple_event: *const AppleEvent,
        the_ae_keyword: AEKeyword,
        desired_type: DescType,
        result: *mut AEDesc,
    ) -> OSErr;

    fn AECountItems(the_ae_desc_list: *const AEDescList, the_count: *mut c_long) -> OSErr;

    fn AEGetNthDesc(
        the_ae_desc_list: *const AEDescList,
        index: c_long,
        desired_type: DescType,
        the_ae_keyword: *mut AEKeyword,
        result: *mut AEDesc,
    ) -> OSErr;

    fn AEGetDescDataSize(the_ae_desc: *const AEDesc) -> Size;

    fn AEGetDescData(the_ae_desc: *const AEDesc, data_ptr: *mut c_void, maximum_size: Size) -> OSErr;

    fn AEDisposeDesc(the_ae_desc: *mut AEDesc) -> OSErr;
}

fn null_desc() -> AEDesc {
    AEDesc {
        descriptor_type: 0,
        data_handle: std::ptr::null_mut(),
    }
}

/// The Apple Event Manager callback. Runs synchronously, inside whichever
/// native run-loop pass just received the event — kept short, and every path
/// just gets handed off to [`send_opened_path`] rather than touched further
/// here.
unsafe extern "C" fn handle_open_documents(
    the_apple_event: *const AppleEvent,
    _reply: *mut AppleEvent,
    _handler_refcon: SRefCon,
) -> OSErr {
    let mut doc_list = null_desc();
    let err =
        unsafe { AEGetParamDesc(the_apple_event, KEY_DIRECT_OBJECT, TYPE_AE_LIST, &mut doc_list) };
    if err != 0 {
        return err;
    }

    let mut count: c_long = 0;
    if unsafe { AECountItems(&doc_list, &mut count) } == 0 {
        for index in 1..=count {
            let mut item = null_desc();
            let mut keyword: AEKeyword = 0;
            let got =
                unsafe { AEGetNthDesc(&doc_list, index, TYPE_FILE_URL, &mut keyword, &mut item) };
            if got == 0 {
                let size = unsafe { AEGetDescDataSize(&item) };
                if size > 0 {
                    let mut buf = vec![0u8; size as usize];
                    let read =
                        unsafe { AEGetDescData(&item, buf.as_mut_ptr() as *mut c_void, size) };
                    if read == 0 {
                        if let Some(path) = path_from_bytes(buf) {
                            send_opened_path(path);
                        }
                    }
                }
                unsafe { AEDisposeDesc(&mut item) };
            }
        }
    }
    unsafe { AEDisposeDesc(&mut doc_list) };
    0 // noErr
}

/// `typeFileURL` data is a percent-encoded `file://` URL (measured, not
/// assumed — see the module doc). `file:///abs/path` has an empty host, so
/// the part after the scheme already starts with the path's own leading `/`.
fn path_from_bytes(bytes: Vec<u8>) -> Option<PathBuf> {
    let s = std::str::from_utf8(&bytes).ok()?;
    let encoded_path = s.strip_prefix("file://")?;
    let decoded = percent_decode(encoded_path.as_bytes());
    if decoded.is_empty() {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(decoded)))
}

/// Minimal `%XX` decoder. Decodes byte-for-byte rather than per-codepoint, so
/// a multi-byte UTF-8 sequence in the original filename (each byte percent-
/// encoded separately) round-trips correctly through `OsString::from_vec`
/// even though the decoded bytes aren't validated as UTF-8 here.
fn percent_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' && i + 2 < input.len() {
            if let (Some(hi), Some(lo)) = (hex_val(input[i + 1]), hex_val(input[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ── Apple Event callback → iced Subscription ────────────────────────────
//
// Same shape as `single_instance`'s "blocking OS primitive → iced stream"
// bridge: a channel filled from an external source (there, a TCP accept loop
// on its own thread; here, the AE callback), drained on a dedicated thread so
// the async wrapper in [`subscribe`] never blocks.

static SENDER: OnceLock<Mutex<Sender<PathBuf>>> = OnceLock::new();

fn send_opened_path(path: PathBuf) {
    if let Some(tx) = SENDER.get() {
        let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(path);
    }
}

/// Files handed to this already-running process via a warm `kAEOpenDocuments`
/// Apple Event. Feeds the same [`crate::app::Message`] path as
/// `single_instance::subscribe` (`Message::OpenExternal`) — see
/// `app::view::subscription`.
pub fn subscribe() -> iced::Subscription<PathBuf> {
    iced::Subscription::run(worker)
}

type PathSender = iced::futures::channel::mpsc::Sender<PathBuf>;

fn worker() -> impl iced::futures::Stream<Item = PathBuf> {
    iced::stream::channel(8, drain_opened_paths)
}

/// Registers the Apple Event handler on this stream's first poll (see the
/// module doc for why here, of all places, rather than at process startup),
/// then forwards every path the handler receives for the rest of the
/// process's life.
async fn drain_opened_paths(out: PathSender) {
    let (tx, rx) = channel();
    if SENDER.set(Mutex::new(tx)).is_err() {
        // `Subscription::run` keys a recipe's identity off `worker`'s function
        // pointer, so iced runs this at most once — this is just a guard
        // against that invariant changing out from under us. Park rather than
        // end the stream, so iced does not re-run the recipe.
        std::future::pending::<()>().await;
        return;
    }
    unsafe {
        AEInstallEventHandler(
            K_CORE_EVENT_CLASS,
            K_AE_OPEN_DOCUMENTS,
            handle_open_documents,
            std::ptr::null_mut(),
            0,
        );
    }
    std::thread::spawn(move || {
        let mut out = out;
        for path in rx {
            let _ = out.try_send(path);
        }
    });
    std::future::pending::<()>().await;
}
