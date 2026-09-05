//! Stable entity identity (plan §2 decision #7).

use serde::{Deserialize, Serialize};

/// A stable u64 handle to a first-level document entity, decoupled from any
/// internal engine/kernel pointer. Value-compatible with `acadrust::Handle` —
/// kernel keys never cross the API boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub u64);

impl ObjectId {
    /// The null / absent handle (`acadrust::Handle::NULL` value `0`).
    pub const NULL: ObjectId = ObjectId(0);

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub const fn is_null(self) -> bool {
        self.0 == 0
    }

    /// Build from a raw u64 (e.g. an `acadrust::Handle`'s value).
    pub const fn from_u64(v: u64) -> Self {
        Self(v)
    }

    /// Build from an `acadrust::Handle` (host/ipc feature).
    #[cfg(any(feature = "host", feature = "ipc", test))]
    pub fn from_handle(h: acadrust::Handle) -> Self {
        Self(h.value())
    }

    /// Convert to an `acadrust::Handle` (host/ipc feature).
    #[cfg(any(feature = "host", feature = "ipc", test))]
    pub fn to_handle(self) -> acadrust::Handle {
        acadrust::Handle::new(self.0)
    }
}
