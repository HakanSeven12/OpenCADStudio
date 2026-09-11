//! Block record table entry

use super::TableEntry;
use crate::types::{Handle, Vector3};

/// Block record flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockFlags {
    /// Block is anonymous
    pub anonymous: bool,
    /// Block has attributes
    pub has_attributes: bool,
    /// Block is external reference (xref)
    pub is_xref: bool,
    /// Block is xref overlay
    pub is_xref_overlay: bool,
    /// Block is from external reference
    pub is_external: bool,
}

impl BlockFlags {
    /// Create default block flags
    pub fn new() -> Self {
        BlockFlags {
            anonymous: false,
            has_attributes: false,
            is_xref: false,
            is_xref_overlay: false,
            is_external: false,
        }
    }
}

impl Default for BlockFlags {
    fn default() -> Self {
        Self::new()
    }
}

/// A block record table entry
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockRecord {
    /// Unique handle for the block record table entry
    pub handle: Handle,
    /// Handle for the BLOCK entity
    pub block_entity_handle: Handle,
    /// Handle for the ENDBLK entity
    pub block_end_handle: Handle,
    /// Block name
    pub name: String,
    /// Block flags
    pub flags: BlockFlags,
    /// Layout handle (if this block is a layout)
    pub layout: Handle,
    /// Units for block scaling
    pub units: i16,
    /// Explodability flag
    pub explodable: bool,
    /// Can scale uniformly
    pub scale_uniformly: bool,
    /// Handles of entities owned by this block
    pub entity_handles: Vec<Handle>,
    /// XRef file path (empty for normal blocks)
    pub xref_path: String,
    /// Block description (R2000+)
    pub description: String,
    /// Insert count bytes (raw non-zero RL bytes before the zero terminator, R2000+)
    pub insert_count_bytes: Vec<u8>,
    /// Preview data (BMP thumbnail, R2000+)
    pub preview_data: Vec<u8>,
    /// INSERT entity handles that reference this block (R2000+)
    pub insert_handles: Vec<Handle>,
    /// Block insertion base point (read from BLOCK entity in DWG)
    pub base_point: Vector3,
}

impl BlockRecord {
    /// Create a new block record
    pub fn new(name: impl Into<String>) -> Self {
        BlockRecord {
            handle: Handle::NULL,
            block_entity_handle: Handle::NULL,
            block_end_handle: Handle::NULL,
            name: name.into(),
            flags: BlockFlags::new(),
            layout: Handle::NULL,
            units: 0,
            explodable: true,
            scale_uniformly: false,
            entity_handles: Vec::new(),
            xref_path: String::new(),
            description: String::new(),
            insert_count_bytes: Vec::new(),
            preview_data: Vec::new(),
            insert_handles: Vec::new(),
            base_point: Vector3::ZERO,
        }
    }

    /// Create the model space block record
    pub fn model_space() -> Self {
        BlockRecord {
            handle: Handle::NULL,
            block_entity_handle: Handle::NULL,
            block_end_handle: Handle::NULL,
            name: "*Model_Space".to_string(),
            flags: BlockFlags::new(),
            layout: Handle::NULL,
            units: 0,
            explodable: true,
            scale_uniformly: false,
            entity_handles: Vec::new(),
            xref_path: String::new(),
            description: String::new(),
            insert_count_bytes: Vec::new(),
            preview_data: Vec::new(),
            insert_handles: Vec::new(),
            base_point: Vector3::ZERO,
        }
    }

    /// Create the paper space block record
    pub fn paper_space() -> Self {
        BlockRecord {
            handle: Handle::NULL,
            block_entity_handle: Handle::NULL,
            block_end_handle: Handle::NULL,
            name: "*Paper_Space".to_string(),
            flags: BlockFlags::new(),
            layout: Handle::NULL,
            units: 0,
            explodable: true,
            scale_uniformly: false,
            entity_handles: Vec::new(),
            xref_path: String::new(),
            description: String::new(),
            insert_count_bytes: Vec::new(),
            preview_data: Vec::new(),
            insert_handles: Vec::new(),
            base_point: Vector3::ZERO,
        }
    }

    /// Check if this is a model space block
    pub fn is_model_space(&self) -> bool {
        self.name.eq_ignore_ascii_case("*Model_Space")
    }

    /// Check if this is a paper space block
    pub fn is_paper_space(&self) -> bool {
        self.name
            .get(.."*Paper_Space".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("*Paper_Space"))
    }

    /// Check if this is a layout block
    pub fn is_layout(&self) -> bool {
        !self.layout.is_null()
    }

    /// Check if this block is anonymous
    pub fn is_anonymous(&self) -> bool {
        self.flags.anonymous || self.name.starts_with('*')
    }
}

impl TableEntry for BlockRecord {
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
        self.is_model_space() || self.is_paper_space()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_record_creation() {
        let block = BlockRecord::new("MyBlock");
        assert_eq!(block.name, "MyBlock");
        assert!(block.explodable);
    }

    #[test]
    fn space_record_names_are_ascii_case_insensitive() {
        assert!(BlockRecord::new("*MODEL_SPACE").is_model_space());
        assert!(BlockRecord::new("*model_space").is_model_space());
        assert!(BlockRecord::new("*PAPER_SPACE").is_paper_space());
        assert!(BlockRecord::new("*paper_space12").is_paper_space());
        assert!(!BlockRecord::new("*paperwork").is_paper_space());
    }

    #[test]
    fn test_model_space() {
        let block = BlockRecord::model_space();
        assert!(block.is_model_space());
        assert!(block.is_standard());
        assert!(!block.is_paper_space());
    }

    #[test]
    fn test_paper_space() {
        let block = BlockRecord::paper_space();
        assert!(block.is_paper_space());
        assert!(block.is_standard());
        assert!(!block.is_model_space());
    }
}
