use crate::tables::TableEntry;
use crate::types::Handle;

/// Legacy viewport-entity table record (VX_TABLE_RECORD / VPENT_HDR).
///
/// AutoCAD persisted this table from R13 through R2000.  Each record links a
/// viewport entity to the previous viewport-entry record in the chain.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VxTableRecord {
    pub handle: Handle,
    pub name: String,
    pub is_xref_reference: bool,
    pub is_xref_resolved: bool,
    pub is_xref_dependent: bool,
    pub xref_handle: Handle,
    pub is_on: bool,
    pub viewport: Handle,
    pub previous_entry: Handle,
    /// R11 object address. Not present in the R13+ formats supported here.
    pub legacy_viewport_entity_address: u16,
    /// R11 viewport table index. Not present in the R13+ formats supported here.
    pub legacy_viewport_index: i16,
    /// R11 previous-entry index. Not present in the R13+ formats supported here.
    pub legacy_previous_entry_index: i16,
}

impl VxTableRecord {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            handle: Handle::NULL,
            name: name.into(),
            is_xref_reference: false,
            is_xref_resolved: false,
            is_xref_dependent: false,
            xref_handle: Handle::NULL,
            is_on: false,
            viewport: Handle::NULL,
            previous_entry: Handle::NULL,
            legacy_viewport_entity_address: 0,
            legacy_viewport_index: 0,
            legacy_previous_entry_index: 0,
        }
    }
}

impl Default for VxTableRecord {
    fn default() -> Self {
        Self::new("")
    }
}

impl TableEntry for VxTableRecord {
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
}
