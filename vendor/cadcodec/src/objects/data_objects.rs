//! Structured data/index helper objects.
//!
//! These records do not draw geometry, but their handle graphs are part of the
//! database.  Keeping them typed allows cross-version DWG/DXF writes without
//! falling back to version-locked raw object bytes.

use crate::entities::CellContentGeometry;
use crate::objects::NamedTableCellStyle;
use crate::types::{Handle, Vector3};

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DataObject {
    pub handle: Handle,
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    pub data: DataObjectData,
}

impl DataObject {
    pub fn new(data: DataObjectData) -> Self {
        Self {
            data,
            ..Self::default()
        }
    }

    pub fn dxf_name(&self) -> &'static str {
        self.data.dxf_name()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DataObjectData {
    #[default]
    BreakPointRef,
    BreakData(BreakData),
    CellStyleMap(CellStyleMap),
    AcDsRecord,
    AcDsSchema,
    Dummy,
    IdBuffer(IdBuffer),
    Index(Index),
    LayerIndex(LayerIndex),
    LongTransaction,
    ObjectPointer,
    PartialViewingFilter(PartialViewingFilter),
    TableGeometry(TableGeometry),
}

impl DataObjectData {
    pub fn dxf_name(&self) -> &'static str {
        match self {
            Self::BreakData(_) => "BREAKDATA",
            Self::BreakPointRef => "BREAKPOINTREF",
            Self::CellStyleMap(_) => "CELLSTYLEMAP",
            Self::AcDsRecord => "ACDSRECORD",
            Self::AcDsSchema => "ACDSSCHEMA",
            Self::Dummy => "DUMMY",
            Self::IdBuffer(_) => "IDBUFFER",
            Self::Index(_) => "INDEX",
            Self::LayerIndex(_) => "LAYER_INDEX",
            Self::LongTransaction => "LONG_TRANSACTION",
            Self::ObjectPointer => "OBJECT_PTR",
            Self::PartialViewingFilter(_) => "PARTIAL_VIEWING_FILTER",
            Self::TableGeometry(_) => "TABLEGEOMETRY",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BreakData {
    /// AcDbBreakData version (DXF 70).
    pub version: i16,
    pub dimension_reference: Handle,
    /// Internal soft-pointer slot present after the dimension reference.
    pub reserved_reference: Handle,
    pub point_references: Vec<BreakPointReference>,
}

/// Embedded AcDbBreakPointRef value stored inside an AcDbBreakData object.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BreakPointReference {
    /// Two internal class-version fields not exposed by Autodesk DXF.
    pub version: i16,
    pub reserved: i16,
    /// Break point reference type (DXF 71).
    pub reference_type: i32,
    /// Reference flags (DXF 72).
    pub flags: i16,
    /// Break point identifier (DXF 91).
    pub identifier: i32,
    pub first_point: Vector3,
    pub second_point: Vector3,
    /// Internal trailing version field not exposed by Autodesk DXF.
    pub trailing_version: i16,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IdBuffer {
    /// Undocumented byte stored before the DWG handle vector.
    pub flags: u8,
    pub object_ids: Vec<Handle>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Index {
    pub last_updated_julian_day: i32,
    pub last_updated_milliseconds: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LayerIndexEntry {
    /// Number of layers represented by this index entry in native DWG.
    pub layer_count: i32,
    pub name: String,
    /// Hard-pointer reference to the entry's `IDBUFFER`.
    pub id_buffer: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LayerIndex {
    pub last_updated_julian_day: i32,
    pub last_updated_milliseconds: i32,
    pub entries: Vec<LayerIndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PartialViewingFilter;

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CellStyleMap {
    pub cells: Vec<NamedTableCellStyle>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableGeometryCell {
    pub geometry_data_flag: i32,
    pub width_with_gap: f64,
    pub height_with_gap: f64,
    pub table_geometry: Handle,
    pub geometry: Vec<CellContentGeometry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableGeometry {
    pub rows: i32,
    pub columns: i32,
    pub cells: Vec<TableGeometryCell>,
}
