//! Open CAD Studio's foundations: localisation, configuration, platform
//! glue and small helpers that know nothing about CAD.

pub mod config;
pub mod i18n;
pub mod icon_assets;
#[cfg(not(target_arch = "wasm32"))]
pub mod network;
pub mod par;
pub mod paths;
pub mod perf;
pub mod schema_validation;
pub mod sys;
pub mod sysinfo;
pub mod wildcard;
pub mod xml_depth;
