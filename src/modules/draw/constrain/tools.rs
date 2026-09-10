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

pub mod concentric {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "NCONSTRAINT",
            label: "Concentric",
            icon: IconKind::Glyph("◎"),
            event: ModuleEvent::Command("NCONSTRAINT".to_string()),
        }
    }
}

/// A line perpendicular to a circle/arc's tangent at their point of
/// contact — for the Line/Circle-only entity model this is equivalent to
/// "the line passes through the circle's center" (a circle's radius is
/// always normal to its own tangent), so it's built from the same
/// `PointOnLine` primitive `PointOnCurve` already uses for a point-on-line
/// case (`sketch_solve.rs`). Distinct from `Perpendicular`, which only
/// covers line-to-line — AutoCAD's own `GeomConstraintType` enum lists
/// `kNormal` and `kPerpendicular` separately for exactly this reason.
pub mod normal {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "NRCONSTRAINT",
            label: "Normal",
            icon: IconKind::Glyph("⊾"),
            event: ModuleEvent::Command("NRCONSTRAINT".to_string()),
        }
    }
}

pub mod colinear {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "LCONSTRAINT",
            label: "Colinear",
            icon: IconKind::Glyph("L"),
            event: ModuleEvent::Command("LCONSTRAINT".to_string()),
        }
    }
}

pub mod fixed {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "FXCONSTRAINT",
            label: "Fixed",
            icon: IconKind::Glyph("F"),
            event: ModuleEvent::Command("FXCONSTRAINT".to_string()),
        }
    }
}

pub mod symmetric {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "SYCONSTRAINT",
            label: "Symmetric",
            icon: IconKind::Glyph("S"),
            event: ModuleEvent::Command("SYCONSTRAINT".to_string()),
        }
    }
}
