//! Shared performance trace for terminal output and the in-app `PERF` panel.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const MAX_LINES: usize = 500;

static UI_ENABLED: AtomicBool = AtomicBool::new(false);
static ENV_ENABLED: OnceLock<bool> = OnceLock::new();
static LINES: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
#[cfg(not(target_arch = "wasm32"))]
static STDERR_TX: OnceLock<std::sync::mpsc::SyncSender<String>> = OnceLock::new();

fn lines() -> &'static Mutex<VecDeque<String>> {
    LINES.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_LINES)))
}

/// True when tracing was requested with `PERF=1` or the in-app panel is open.
pub fn enabled() -> bool {
    UI_ENABLED.load(Ordering::Relaxed)
        || *ENV_ENABLED.get_or_init(|| std::env::var_os("PERF").is_some())
}

/// Elapsed diagnostic time; a disabled timer never reads the clock.
pub fn elapsed_ms(start: Option<iced::time::Instant>) -> f64 {
    start.map_or(0.0, |start| start.elapsed().as_secs_f64() * 1000.0)
}

/// Enable or disable collection driven by the in-app `PERF` command.
pub fn set_ui_enabled(enabled: bool) {
    UI_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Write one performance line to stderr and retain it for the in-app panel.
pub fn record(args: fmt::Arguments<'_>) {
    if !enabled() {
        return;
    }
    let line = args.to_string();
    // Terminal writes can block the UI/render thread when its pipe is slow.
    // PERF is diagnostic and must not create the hitch it is measuring.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let tx = STDERR_TX.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::sync_channel::<String>(1024);
            let _ = std::thread::Builder::new()
                .name("ocs-perf-log".to_string())
                .spawn(move || {
                    while let Ok(line) = rx.recv() {
                        eprintln!("{line}");
                    }
                });
            tx
        });
        let _ = tx.try_send(line.clone());
    }
    #[cfg(target_arch = "wasm32")]
    eprintln!("{line}");
    let mut entries = lines().lock().unwrap_or_else(|e| e.into_inner());
    if entries.len() == MAX_LINES {
        entries.pop_front();
    }
    entries.push_back(line);
}

/// Plain-text snapshot used by the panel and its Copy button.
pub fn snapshot_text() -> String {
    let entries = lines().lock().unwrap_or_else(|e| e.into_inner());
    entries
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A short tail for the live HUD. The full 500-line history remains available
/// to Copy, while rendering a large monolithic text widget on every mouse event
/// is avoided.
pub fn snapshot_tail_text(max_lines: usize) -> String {
    let entries = lines().lock().unwrap_or_else(|e| e.into_inner());
    let skip = entries.len().saturating_sub(max_lines);
    entries
        .iter()
        .skip(skip)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn clear() {
    lines().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[macro_export]
macro_rules! perf_record {
    ($($arg:tt)*) => {
        $crate::perf::record(format_args!($($arg)*))
    };
}
