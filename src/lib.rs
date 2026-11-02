#![allow(non_snake_case)]
#![recursion_limit = "512"]

pub mod app;

/// Tests that build a headless iced renderer take this first: two made in
/// parallel crash the process, wherever in the crate they run.
#[cfg(test)]
pub(crate) fn headless_ui_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
#[cfg(not(target_arch = "wasm32"))]
pub mod cli;
pub mod command;
pub mod config;
pub mod discussions;
pub mod entities;
pub(crate) mod entity_creation;
#[cfg(not(target_arch = "wasm32"))]
pub mod gpu_backend;
pub mod i18n;
pub(crate) mod input;
pub mod io;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp_intent;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp_ops;
pub mod modules;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod network;
pub mod par;
#[cfg(not(target_arch = "wasm32"))]
pub mod parametric;
pub mod patreon;
pub mod perf;
pub mod plugin;
#[cfg(not(target_arch = "wasm32"))]
pub mod rest;
pub mod scene;
pub(crate) mod schema_validation;
pub mod snap;
pub mod sys;
pub mod sysinfo;
pub mod ui;
pub mod videos;
