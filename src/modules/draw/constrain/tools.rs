//! Ribbon button definitions for the Constraints group.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub mod horizontal {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "HCONSTRAINT",
            label: "Horizontal",
            icon: IconKind::Glyph("—"),
            event: ModuleEvent::Command("HCONSTRAINT".to_string()),
        }
    }
}

pub mod vertical {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "VCONSTRAINT",
            label: "Vertical",
            icon: IconKind::Glyph("│"),
            event: ModuleEvent::Command("VCONSTRAINT".to_string()),
        }
    }
}

pub mod parallel {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "PCONSTRAINT",
            label: "Parallel",
            icon: IconKind::Glyph("∥"),
            event: ModuleEvent::Command("PCONSTRAINT".to_string()),
        }
    }
}

pub mod perpendicular {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "QCONSTRAINT",
            label: "Perpendicular",
            icon: IconKind::Glyph("⊥"),
            event: ModuleEvent::Command("QCONSTRAINT".to_string()),
        }
    }
}

pub mod equal {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "ECONSTRAINT",
            label: "Equal",
            icon: IconKind::Glyph("="),
            event: ModuleEvent::Command("ECONSTRAINT".to_string()),
        }
    }
}

pub mod tangent {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "TCONSTRAINT",
            label: "Tangent",
            icon: IconKind::Glyph("⌒"),
            event: ModuleEvent::Command("TCONSTRAINT".to_string()),
        }
    }
}
