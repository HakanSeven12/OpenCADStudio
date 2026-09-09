#![allow(non_snake_case)]
#![recursion_limit = "256"]

pub mod app;
#[cfg(not(target_arch = "wasm32"))]
pub mod cli;
pub mod command;
pub mod config;
pub mod discussions;
pub mod entities;
pub mod i18n;
pub mod io;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp;
pub mod modules;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod network;
pub mod par;
pub mod patreon;
pub mod perf;
pub mod plugin;
pub mod scene;
pub mod snap;
pub mod sys;
pub mod ui;
pub mod videos;
