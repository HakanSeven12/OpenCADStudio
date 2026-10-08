/// Stable, language-neutral entity capability schema.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityScope {
    Canvas,
    Internal,
    Opaque,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAccess {
    ReadWrite,
    ReadOnly,
    Unmapped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCoverage {
    /// Name shown in the document model, or source name when unmapped.
    pub name: String,
    /// Field in the opencadcodec snapshot; `kind` is synthesized from the variant.
    pub source_path: String,
    pub type_id: String,
    pub optional: bool,
    pub is_sequence: bool,
    /// Whether the serialized host snapshot carries this property.
    pub snapshot_readable: bool,
    /// `unmapped` means no normalized Python property or setter; the raw
    /// snapshot can still expose its serialized value.
    pub model_access: ModelAccess,
    /// Write validation applied by the v7 transaction path. Currently
    /// `transaction_geometry` for selected primitive geometry, otherwise
    /// `type_conversion_only` for writable properties or `none`.
    pub validation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityKindCoverage {
    pub kind: String,
    pub scope: EntityScope,
    pub properties: Vec<PropertyCoverage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityCoverageCatalog {
    pub schema_version: u32,
    pub entity_kinds: Vec<EntityKindCoverage>,
}
