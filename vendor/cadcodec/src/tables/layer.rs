//! Layer table entry

use super::TableEntry;
use crate::types::{Color, Handle, LineWeight, Transparency};

/// Layer flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LayerFlags {
    /// Layer is frozen
    pub frozen: bool,
    /// Layer is locked
    pub locked: bool,
    /// Layer is frozen by default in newly created viewports
    #[cfg_attr(feature = "serde", serde(default))]
    pub frozen_in_new_viewport: bool,
    /// Layer is off (invisible)
    pub off: bool,
    /// Layer is xref-dependent (name contains `|`)
    pub xref_dependent: bool,
}

impl LayerFlags {
    /// Create default layer flags (all false)
    pub fn new() -> Self {
        LayerFlags {
            frozen: false,
            locked: false,
            frozen_in_new_viewport: false,
            off: false,
            xref_dependent: false,
        }
    }

    /// Create flags for a standard layer (layer "0")
    pub fn standard() -> Self {
        Self::new()
    }
}

impl Default for LayerFlags {
    fn default() -> Self {
        Self::new()
    }
}

/// A layer table entry
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Layer {
    /// Unique handle
    pub handle: Handle,
    /// Layer name
    pub name: String,
    /// Layer flags
    pub flags: LayerFlags,
    /// Layer color
    pub color: Color,
    /// Named color identity
    #[cfg_attr(feature = "serde", serde(default))]
    pub color_name: Option<String>,
    /// Color-book identity
    #[cfg_attr(feature = "serde", serde(default))]
    pub book_name: Option<String>,
    /// Line type name
    pub line_type: String,
    /// Line weight
    pub line_weight: LineWeight,
    /// Plot style name
    pub plot_style: String,
    /// Is this layer plottable?
    pub is_plottable: bool,
    /// Layer transparency
    #[cfg_attr(
        feature = "serde",
        serde(default = "crate::types::transparency::opaque")
    )]
    pub transparency: Transparency,
    /// Material handle
    pub material: Handle,
    /// Plot style handle (R2000+)
    pub plotstyle_handle: Handle,
    /// External reference block record handle (for xref-dependent layers)
    pub xref_block_record_handle: Handle,
}

impl Layer {
    /// Create a new layer with default settings
    pub fn new(name: impl Into<String>) -> Self {
        Layer {
            handle: Handle::NULL,
            name: name.into(),
            flags: LayerFlags::new(),
            color: Color::WHITE,
            color_name: None,
            book_name: None,
            line_type: "Continuous".to_string(),
            line_weight: LineWeight::Default,
            plot_style: String::new(),
            is_plottable: true,
            transparency: Transparency::OPAQUE,
            material: Handle::NULL,
            plotstyle_handle: Handle::NULL,
            xref_block_record_handle: Handle::NULL,
        }
    }

    /// Create the standard "0" layer
    pub fn layer_0() -> Self {
        Layer {
            handle: Handle::NULL,
            name: "0".to_string(),
            flags: LayerFlags::standard(),
            color: Color::WHITE,
            color_name: None,
            book_name: None,
            line_type: "Continuous".to_string(),
            line_weight: LineWeight::Default,
            plot_style: String::new(),
            is_plottable: true,
            transparency: Transparency::OPAQUE,
            material: Handle::NULL,
            plotstyle_handle: Handle::NULL,
            xref_block_record_handle: Handle::NULL,
        }
    }

    /// Create a layer with a specific color
    pub fn with_color(name: impl Into<String>, color: Color) -> Self {
        Layer {
            color,
            ..Self::new(name)
        }
    }

    /// Set the layer as frozen
    pub fn freeze(&mut self) {
        self.flags.frozen = true;
    }

    /// Set the layer as thawed
    pub fn thaw(&mut self) {
        self.flags.frozen = false;
    }

    /// Check if the layer is frozen
    pub fn is_frozen(&self) -> bool {
        self.flags.frozen
    }

    /// Set the layer as locked
    pub fn lock(&mut self) {
        self.flags.locked = true;
    }

    /// Set the layer as unlocked
    pub fn unlock(&mut self) {
        self.flags.locked = false;
    }

    /// Check if the layer is locked
    pub fn is_locked(&self) -> bool {
        self.flags.locked
    }

    /// Turn the layer off
    pub fn turn_off(&mut self) {
        self.flags.off = true;
    }

    /// Turn the layer on
    pub fn turn_on(&mut self) {
        self.flags.off = false;
    }

    /// Check if the layer is off
    pub fn is_off(&self) -> bool {
        self.flags.off
    }

    /// Check if the layer is visible (not off and not frozen)
    pub fn is_visible(&self) -> bool {
        !self.flags.off && !self.flags.frozen
    }
}

impl TableEntry for Layer {
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
        self.name == "0"
    }
}
