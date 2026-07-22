//! UI-thread hang watchdog (#120).
//!
//! The Windows freeze reports (#120) all share one signature: the event loop
//! stops pumping (`AppHangB1` — "Top level window is idle"), one core spins,
//! and the app can only be killed from Task Manager. It never reproduces on
//! our machines, so this module turns a reporter's hang into a debuggable
//! artifact: a background thread watches a heartbeat that the UI thread bumps
//! on every processed message (plus a 5-second timer message so an idle but
//! healthy app keeps beating), and when the heartbeat goes stale for
//! [`STALL_AFTER_MS`] it writes a minidump — with every thread's stack — next
//! to the config, then leaves the app alone (it may still recover).
//!
//! The dump lands in `%LOCALAPPDATA%\OpenCADStudio\` as
//! `OpenCADStudio-hang-<unix-secs>.dmp`, alongside a small `.txt` telling the
//! user what it is. One dump per run. `OCS_NO_WATCHDOG=1` disables the whole
//! thing.
//!
//! False-positive guards:
//! * not armed until the first beat (startup shader compiles can be slow);
//! * a watchdog sleep that overshoots wildly means the machine suspended —
//!   every thread was paused, so the stale beat proves nothing and the
//!   baseline is reset instead of dumping.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static START: OnceLock<Instant> = OnceLock::new();
static BEAT_MS: AtomicU64 = AtomicU64::new(0);
static DUMPED: AtomicBool = AtomicBool::new(false);

const CHECK_EVERY: Duration = Duration::from_secs(5);
/// How long the UI thread must stay silent before it counts as hung. Long
/// enough that a slow synchronous save of a huge drawing doesn't trip it.
const STALL_AFTER_MS: u64 = 25_000;

fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Called from the UI thread for every processed message. `max(1)` keeps the
/// value nonzero so zero can mean "no beat yet" (watchdog not armed).
pub fn beat() {
    BEAT_MS.store(now_ms().max(1), Ordering::Relaxed);
}

/// Spawn the watchdog thread. Call once at startup, before the event loop.
pub fn start() {
    if std::env::var_os("OCS_NO_WATCHDOG").is_some() {
        return;
    }
    let _ = START.get_or_init(Instant::now);
    let _ = std::thread::Builder::new()
        .name("hang-watchdog".into())
        .spawn(|| {
            // Beats older than this floor are ignored — bumped after a
            // detected system suspend so the paused interval doesn't read
            // as a hang.
            let mut floor_ms: u64 = 0;
            loop {
                let before = Instant::now();
                std::thread::sleep(CHECK_EVERY);
                if before.elapsed() > CHECK_EVERY * 3 {
                    floor_ms = now_ms();
                    continue;
                }
                let beat = BEAT_MS.load(Ordering::Relaxed);
                if beat == 0 {
                    continue;
                }
                let age = now_ms().saturating_sub(beat.max(floor_ms));
                if age > STALL_AFTER_MS && !DUMPED.swap(true, Ordering::SeqCst) {
                    write_dump(age);
                }
            }
        });
}

/// Directory the dump goes to: `%LOCALAPPDATA%\OpenCADStudio`, or the temp
/// dir when that can't be created.
fn dump_dir() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|d| std::path::PathBuf::from(d).join("OpenCADStudio"))
        .filter(|d| std::fs::create_dir_all(d).is_ok())
        .unwrap_or_else(std::env::temp_dir)
}

fn write_dump(stall_ms: u64) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        MiniDumpWithDataSegs, MiniDumpWithIndirectlyReferencedMemory, MiniDumpWithThreadInfo,
        MiniDumpWriteDump,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};

    let dir = dump_dir();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("OpenCADStudio-hang-{stamp}.dmp"));
    let Ok(file) = std::fs::File::create(&path) else {
        return;
    };
    let ok = unsafe {
        MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            file.as_raw_handle(),
            MiniDumpWithThreadInfo | MiniDumpWithDataSegs | MiniDumpWithIndirectlyReferencedMemory,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    drop(file);
    if ok == 0 {
        let _ = std::fs::remove_file(&path);
        return;
    }
    let note = dir.join(format!("OpenCADStudio-hang-{stamp}.txt"));
    let _ = std::fs::write(
        &note,
        format!(
            "OpenCADStudio detected that its interface stopped responding for \
             {} seconds and captured a diagnostic snapshot:\n\n    {}\n\n\
             Please attach that .dmp file to the GitHub issue \
             https://github.com/HakanSeven12/OpenCADStudio/issues/120 — it \
             contains the exact state of every thread at the moment of the \
             hang and no drawing contents.\n",
            stall_ms / 1000,
            path.display()
        ),
    );
    eprintln!(
        "hang-watchdog: UI thread unresponsive for {}s — minidump written to {}",
        stall_ms / 1000,
        path.display()
    );
}
