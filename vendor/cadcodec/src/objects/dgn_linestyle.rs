//! Typed DGN line-style objects (`AcDbLS*`).
//!
//! A DWG linetype imported from DGN normally has no ordinary dash array. Its
//! real pattern is an `AcDbLSDefinition` pointing at a tree of compound,
//! stroke, point, symbol, and internal components.

use crate::types::Handle;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLineStyleObject {
    pub handle: Handle,
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    pub data: DgnLineStyleData,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DgnLineStyleData {
    Definition {
        description: String,
        version: i32,
        style_number: i32,
        component_uid: [u8; 16],
        is_continuous: bool,
        unit_definition: f64,
        unit_scale: f64,
        units_type: i32,
        is_element: bool,
        is_physical: bool,
        is_scale_independent: bool,
        is_snappable: bool,
        root_component: Handle,
        properties: Vec<super::SemanticProperty>,
    },
    Component {
        kind: DgnLsComponentType,
        description: String,
        version: i32,
        component_uid: [u8; 16],
        scale: f64,
        property_flags: u8,
        component: DgnLsComponentData,
        properties: Vec<super::SemanticProperty>,
    },
    Registered {
        dxf_name: String,
        properties: Vec<super::SemanticProperty>,
        payload: super::ProxyPayload,
        object_ids: Vec<super::ProxyObjectReference>,
    },
}

impl DgnLineStyleObject {
    pub fn dxf_name(&self) -> &str {
        match &self.data {
            DgnLineStyleData::Definition { .. } => "LSDEFINITION",
            DgnLineStyleData::Component {
                kind: DgnLsComponentType::Symbol,
                ..
            } => "LSSYMBOLCOMPONENT",
            DgnLineStyleData::Component {
                kind: DgnLsComponentType::Compound,
                ..
            } => "LSCOMPOUNDCOMPONENT",
            DgnLineStyleData::Component {
                kind: DgnLsComponentType::Stroke,
                ..
            } => "LSSTROKEPATTERNCOMPONENT",
            DgnLineStyleData::Component {
                kind: DgnLsComponentType::Point,
                ..
            } => "LSPOINTCOMPONENT",
            DgnLineStyleData::Component {
                kind: DgnLsComponentType::Internal,
                ..
            } => "LSINTERNALCOMPONENT",
            DgnLineStyleData::Registered { dxf_name, .. } => dxf_name,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DgnLsComponentType {
    Symbol,
    Compound,
    Stroke,
    Point,
    Internal,
}

impl DgnLsComponentType {
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            1 => Some(Self::Symbol),
            2 => Some(Self::Compound),
            3 => Some(Self::Stroke),
            4 => Some(Self::Point),
            6 => Some(Self::Internal),
            _ => None,
        }
    }

    pub fn code(self) -> i32 {
        match self {
            Self::Symbol => 1,
            Self::Compound => 2,
            Self::Stroke => 3,
            Self::Point => 4,
            Self::Internal => 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DgnLsComponentData {
    Symbol(DgnLsSymbolComponent),
    Compound(DgnLsCompoundComponent),
    Stroke(DgnLsStrokePattern),
    Point(DgnLsPointComponent),
    Internal(DgnLsInternalComponent),
}

impl DgnLsComponentData {
    pub fn references(&self) -> Vec<Handle> {
        match self {
            Self::Symbol(value) => vec![value.block],
            Self::Compound(value) => value.entries.iter().map(|entry| entry.component).collect(),
            Self::Stroke(_) | Self::Internal(_) => Vec::new(),
            Self::Point(value) => {
                let mut result = Vec::with_capacity(value.symbols.len() + 1);
                result.push(value.stroke_component);
                result.extend(value.symbols.iter().map(|symbol| symbol.symbol_component));
                result
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLsSymbolComponent {
    pub stored_unit_scale: f64,
    pub unit_scale: f64,
    pub has_unit_scale: bool,
    pub is_3d: bool,
    pub block: Handle,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLsCompoundComponent {
    pub entries: Vec<DgnLsCompoundEntry>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLsCompoundEntry {
    pub component: Handle,
    pub offset: f64,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLsStrokePattern {
    pub has_iteration_limit: bool,
    pub is_single_segment: bool,
    pub iteration_limit: i32,
    pub auto_phase: f64,
    pub phase: f64,
    pub phase_mode: DgnLsPhaseMode,
    pub strokes: Vec<DgnLsStroke>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DgnLsPhaseMode {
    Distance,
    Fraction,
    Centered,
    Reserved,
}

impl DgnLsPhaseMode {
    pub fn from_code(code: u8) -> Self {
        match code & 3 {
            0 => Self::Distance,
            1 => Self::Fraction,
            2 => Self::Centered,
            _ => Self::Reserved,
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Self::Distance => 0,
            Self::Fraction => 1,
            Self::Centered => 2,
            Self::Reserved => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLsStroke {
    pub is_dash: bool,
    pub bypass_corner: bool,
    pub can_be_scaled: bool,
    pub invert_at_origin: bool,
    pub invert_at_end: bool,
    pub length: f64,
    pub start_width: f64,
    pub end_width: f64,
    pub width_mode: i32,
    pub cap_mode: i32,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLsPointComponent {
    pub stroke_component: Handle,
    pub symbols: Vec<DgnLsSymbolReference>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLsSymbolReference {
    pub symbol_component: Handle,
    pub partial_strokes: bool,
    pub clip_partial: bool,
    pub allow_stretch: bool,
    pub partial_projected: bool,
    pub use_symbol_color: bool,
    pub use_symbol_lineweight: bool,
    pub justify: i32,
    pub rotation_type: i32,
    pub vertex_mask: i32,
    pub x_offset: f64,
    pub y_offset: f64,
    pub angle: f64,
    pub stroke_number: i32,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLsInternalComponent {
    pub pattern: DgnLsStrokePattern,
    pub internal_version: i32,
    pub hardware_style: i32,
    pub is_hardware_style: bool,
    pub line_code: i32,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLsDefinition {
    pub handle: Handle,
    pub name: String,
    pub root_component: Handle,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DgnLsComponent {
    pub handle: Handle,
    pub component_type: DgnLsComponentType,
    pub description: String,
    pub refs: Vec<Handle>,
    pub scale: f64,
}

impl DgnLsComponent {
    pub fn symbol_block(&self) -> Option<Handle> {
        if self.component_type == DgnLsComponentType::Symbol {
            self.refs.first().copied()
        } else {
            None
        }
    }
}
