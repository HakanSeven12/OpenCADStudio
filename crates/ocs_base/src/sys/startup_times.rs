//! Gated startup-stage timing. Opt in by setting `OCS_STARTUP_LOG` to any
//! value (including empty); without the variable every marker is a no-op and
//! costs one static bool lookup.
//!
//! `mark()` labels one pointer in the boot path (each stage elapsed from the
//! first `mark()` call, which is the process entry in `main`). `update_tick()`
//! and `view_tick()` fire once for the first and Nth message/render pass so
//! the window-open → interactive-content gap in the daemon loop is visible.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

fn start() -> &'static Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("OCS_STARTUP_LOG").is_some())
}

/// Record a named startup stage. Elapsed time is measured from the first
/// marker ever emitted (process entry), so the log doubles as a timeline.
pub fn mark(stage: &'static str) {
    if enabled() {
        eprintln!(
            "[startup] {:>9.1} ms  {}",
            start().elapsed().as_secs_f32() * 1000.0,
            stage
        );
    }
}

/// Update-loop tick: logs the first and every 30th message (the first is the
/// first message after the window opens; the 30th shows the steady state has
/// been reached).
pub fn update_tick() {
    if !enabled() {
        return;
    }
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    if n == 1 {
        mark("first update (message loop alive)");
    } else if n == 30 {
        mark("30th update (steady state)");
    }
}

/// View-loop tick: logs the first and 60th rendered frame.
pub fn view_tick() {
    if !enabled() {
        return;
    }
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    if n == 1 {
        mark("first view (widget tree built)");
    } else if n == 60 {
        mark("60th view (steady rendering)");
    }
}
