#![allow(non_snake_case)]

pub mod app;
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod cli;
pub mod command;
pub mod entities;
pub mod io;
pub mod modules;
pub mod patreon;
pub mod discussions;
pub mod videos;
pub mod plugin;
pub mod perf;
pub mod scene;
pub mod snap;
pub mod ui;
pub mod par;
pub mod sys;
