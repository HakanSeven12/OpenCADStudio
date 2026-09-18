//! Web session memory: which drawings were open (and which was active) the
//! last time a save/open/close touched the tab list.
//!
//! Deliberately NOT a session-restore engine: nothing ever auto-opens from
//! this list. The next boot's crash-recovery flow reads it only to show what
//! the session had and — once restores complete — to put the focus back on
//! the tab the session remembered as active. A small `Vec<String>` + index
//! in localStorage, rewritten in full on every save/open/close, is the whole
//! cost.

use serde::{Deserialize, Serialize};

/// localStorage key, next to `opencadstudio.settings`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub(crate) const SESSION_KEY: &str = "opencadstudio.session";

/// The remembered tab list. `active` indexes `tabs`; construction and
/// parsing both clamp it so a stale index can never point outside the list.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSession {
    pub tabs: Vec<String>,
    pub active: usize,
}

impl WebSession {
    pub fn new(tabs: Vec<String>, active: usize) -> Self {
        Self {
            active: active.min(tabs.len().saturating_sub(1)),
            tabs,
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Forgiving parse: a missing or out-of-range `active` clamps instead of
    /// failing (the list is advisory memory, never load-bearing).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
    fn from_json(json: &str) -> Option<Self> {
        let session: Self = serde_json::from_str(json).ok()?;
        Some(Self::new(session.tabs, session.active))
    }
}

/// Remember `tabs`/`active` for the next boot. Best-effort: unavailable or
/// read-only storage silently skips the write.
#[cfg(target_arch = "wasm32")]
pub fn persist(tabs: &[String], active: usize) {
    if cfg!(test) {
        return;
    }
    let session = WebSession::new(tabs.to_vec(), active);
    if let Some(storage) = web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    {
        let _ = storage.set_item(SESSION_KEY, &session.to_json());
    }
}

/// What the previous session had open, if it ever recorded anything.
#[cfg(target_arch = "wasm32")]
pub fn load() -> Option<WebSession> {
    let json = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(SESSION_KEY).ok().flatten())?;
    WebSession::from_json(&json)
}

// Pure logic only — the localStorage half needs a browser and is exercised
// by hand (and by every web boot's restore flow).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_round_trips_through_json() {
        let session = WebSession::new(
            vec!["plan.dwg".to_string(), "sketch.dxf".to_string()],
            1,
        );
        let restored = WebSession::from_json(&session.to_json()).expect("must parse");
        assert_eq!(restored, session);
        assert_eq!(restored.tabs, ["plan.dwg", "sketch.dxf"]);
        assert_eq!(restored.active, 1);
    }

    #[test]
    fn an_out_of_range_active_index_clamps_instead_of_failing() {
        // Construction clamps…
        assert_eq!(WebSession::new(vec!["a.dwg".to_string()], 9).active, 0);
        // …and so does a parse of stale or hostile JSON, both directions.
        let over = WebSession::from_json(r#"{"tabs":["a.dwg","b.dwg"],"active":99}"#)
            .expect("must parse");
        assert_eq!(over.active, 1);
        let under = WebSession::from_json(r#"{"tabs":["a.dwg"],"active":0}"#).unwrap();
        assert_eq!(under.active, 0);
    }

    #[test]
    fn garbage_is_rejected_and_an_empty_list_is_fine() {
        assert!(WebSession::from_json("").is_none());
        assert!(WebSession::from_json("not json").is_none());
        assert!(WebSession::from_json("{}").is_none()); // missing `tabs` field
        // A session whose every tab closed is still valid memory (it says
        // the crash lost nothing that was visibly open).
        let empty = WebSession::from_json(r#"{"tabs":[],"active":0}"#).unwrap();
        assert!(empty.tabs.is_empty());
        assert_eq!(empty.active, 0);
    }

    #[test]
    fn duplicate_names_are_kept_as_is() {
        // Two tabs may share a display name on the web; the memory list
        // records the list as it was, de-duplication is not its job.
        let session = WebSession::new(
            vec!["a.dwg".to_string(), "a.dwg".to_string()],
            1,
        );
        assert_eq!(
            WebSession::from_json(&session.to_json()).unwrap(),
            session
        );
    }
}
