//! Application ID table entry

use super::TableEntry;
use crate::types::Handle;

/// An application ID table entry (for extended data)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AppId {
    /// Unique handle
    pub handle: Handle,
    /// Application name
    pub name: String,
}

impl AppId {
    /// Create a new application ID
    pub fn new(name: impl Into<String>) -> Self {
        AppId {
            handle: Handle::NULL,
            name: name.into(),
        }
    }

    /// Create the standard "ACAD" application ID
    pub fn acad() -> Self {
        Self::new("ACAD")
    }
}

impl TableEntry for AppId {
    fn handle(&self) -> Handle {
        self.handle
    }

    fn set_handle(&mut self, handle: Handle) {
        self.handle = handle;
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, name: String) {
        self.name = name;
    }

    fn is_standard(&self) -> bool {
        self.name == "ACAD"
    }
}
