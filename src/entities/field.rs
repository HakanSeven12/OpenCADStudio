//! Host-side glue for dynamic text field evaluation.
//!
//! The field language (DIESEL + AcVar), the field structure/linkage, and all
//! date math live in the reader library ([`acadrust::fields`]). OCS only
//! supplies the **environment** — the current clock, OS login and environment
//! variables — through [`FieldContext`]. A DWG library must stay deterministic
//! and platform-neutral, so the one genuinely system-specific bit (reading the
//! clock / OS user) lives here in the app instead.

use acadrust::fields::FieldContext;
use acadrust::types::Handle;
use acadrust::CadDocument;

/// OCS's environment provider for field evaluation.
struct OcsFieldContext;

impl FieldContext for OcsFieldContext {
    fn now_julian(&self) -> f64 {
        // Unix epoch is Julian Day 2440587.5.
        epoch_secs() as f64 / 86_400.0 + 2_440_587.5
    }

    fn login(&self) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            for var in ["USER", "LOGNAME", "USERNAME"] {
                if let Ok(v) = std::env::var(var) {
                    if !v.is_empty() {
                        return Some(v);
                    }
                }
            }
        }
        None
    }

    fn getenv(&self, name: &str) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            return std::env::var(name).ok().filter(|v| !v.is_empty());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = name;
            None
        }
    }
}

/// Re-evaluate the field hosted by entity `host` (usually an MTEXT), or `None`
/// to keep the cached text. Thin wrapper over the library engine.
pub fn resolve(document: &CadDocument, host: Handle) -> Option<String> {
    acadrust::fields::resolve(document, host, &OcsFieldContext)
}

pub fn resolve_handle(document: &CadDocument, field: Handle, host: Handle) -> Option<String> {
    acadrust::fields::resolve_handle(document, field, host, &OcsFieldContext)
}

/// Seconds since the Unix epoch. Native uses the system clock; wasm uses the JS
/// `Date` clock. This platform-specific read stays in the app, not the library.
fn epoch_secs() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as i64
    }
}
