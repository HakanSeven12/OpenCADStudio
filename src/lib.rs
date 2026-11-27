#![allow(non_snake_case)]
#![recursion_limit = "512"]

// Match the application allocator in opt-in release audit tests.
#[cfg(all(test, feature = "performance-audit", not(target_arch = "wasm32")))]
#[global_allocator]
static AUDIT_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod app;

pub use ocs_base::{perf_record, perf_record_diagnostic, t, tf, tr};

/// The Quad display-option message, re-exported at the crate root so every
/// surface reaches it as `crate::QuadOptionMsg` (see `app/mod.rs`).
pub(crate) use app::QuadOptionMsg;

/// Tests that build a headless iced renderer take this first: two made in
/// parallel crash the process, wherever in the crate they run.
#[cfg(test)]
pub(crate) fn headless_ui_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs a test that lays out or renders a large view (the whole ribbon,
/// say): one at a time under `headless_ui_lock`, on a thread with the stack
/// such a view needs.
#[cfg(test)]
pub(crate) fn with_headless_ui(test: impl FnOnce() + Send + 'static) {
    let _guard = headless_ui_lock();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(test)
        .expect("spawn UI test")
        .join()
        .unwrap_or_else(|error| std::panic::resume_unwind(error));
}
pub mod calc;
pub mod catalog;
#[cfg(not(target_arch = "wasm32"))]
pub mod cli;
pub mod command;
pub use ocs_base::config;
pub mod discussions;
pub mod display;
pub mod entities;
pub(crate) mod entity_creation;
pub mod filter;
#[cfg(not(target_arch = "wasm32"))]
pub mod gpu_backend;
pub use ocs_base::i18n;
pub use ocs_base::icon_assets;
pub(crate) mod ifc_plan;
pub(crate) mod input;
pub mod io;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp_intent;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp_ops;
pub mod media;
pub mod modules;
#[cfg(not(target_arch = "wasm32"))]
pub use ocs_base::network;
pub use ocs_base::par;
#[cfg(not(target_arch = "wasm32"))]
pub mod parametric;
pub use ocs_base::paths;
pub mod patreon;
pub mod perf {
    pub use ocs_base::perf::*;
    #[cfg(all(test, feature = "performance-audit"))]
    pub(crate) mod audit;
}
pub mod plugin;
pub mod plugin_catalog;
#[cfg(not(target_arch = "wasm32"))]
pub mod rest;
pub mod scene;
pub use ocs_base::schema_validation;
pub mod snap;
pub mod sys {
    pub use ocs_base::sys::*;
    #[cfg(not(target_arch = "wasm32"))]
    pub mod crash_log;
}
pub use ocs_base::sysinfo;
pub mod text;
pub mod ui;
pub mod videos;
pub use ocs_base::wildcard;
pub use ocs_base::xml_depth;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod isolation_tests {
    /// This crate's tests reach `ocs_base` with its test isolation on
    /// (`test-isolation`, set by the dev-dependency): their settings go to a
    /// scratch folder, never the user's own.
    #[test]
    fn tests_never_touch_the_real_config_folder() {
        let dir = crate::config::config_dir().expect("a config folder");
        assert!(dir.starts_with(std::env::temp_dir()), "{dir:?}");
    }
}
