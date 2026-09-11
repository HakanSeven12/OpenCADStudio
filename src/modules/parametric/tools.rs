//! Ribbon button definitions for the Constraints group.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub mod horizontal {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "HCONSTRAINT",
            label: "Horizontal",
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/horizontal.svg"
            )),
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
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/vertical.svg"
            )),
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
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/parallel.svg"
            )),
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
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/perpendicular.svg"
            )),
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
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/equal.svg"
            )),
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
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/tangent.svg"
            )),
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
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/concentric.svg"
            )),
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
/// covers line-to-line.
pub mod normal {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "NRCONSTRAINT",
            label: "Normal",
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/normal.svg"
            )),
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
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/colinear.svg"
            )),
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
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/fixed.svg"
            )),
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
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/symmetric.svg"
            )),
            event: ModuleEvent::Command("SYCONSTRAINT".to_string()),
        }
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "HCONSTRAINT",
        "VCONSTRAINT",
        "PCONSTRAINT",
        "QCONSTRAINT",
        "ECONSTRAINT",
        "TCONSTRAINT",
        "NCONSTRAINT",
        "NRCONSTRAINT",
        "LCONSTRAINT",
        "FXCONSTRAINT",
        "SYCONSTRAINT",
    ]
});
