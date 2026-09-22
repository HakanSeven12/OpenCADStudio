//! Ribbon button definitions for the Constraints group.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub mod horizontal {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("GCHORIZONTAL")
    }
}

pub mod vertical {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("VCONSTRAINT")
    }
}

pub mod parallel {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("PCONSTRAINT")
    }
}

pub mod perpendicular {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command_as("QCONSTRAINT", "GCPERPENDICULAR")
    }
}

pub mod equal {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("ECONSTRAINT")
    }
}

pub mod tangent {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("TCONSTRAINT")
    }
}

pub mod concentric {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("GCCONCENTRIC")
    }
}

/// A line perpendicular to a circle/arc's tangent at their point of
/// contact — for the Line/Circle-only entity model this is equivalent to
/// "the line passes through the circle's center" (a circle's radius is
/// always normal to its own tangent), so it's built from the same
/// `PointOnLine` primitive `PointOnCurve` already uses for a point-on-line
/// case (`parametric_solve.rs`). Distinct from `Perpendicular`, which only
/// covers line-to-line.
pub mod normal {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("NRCONSTRAINT")
    }
}

pub mod colinear {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("LCONSTRAINT")
    }
}

pub mod fixed {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("FXCONSTRAINT")
    }
}

pub mod symmetric {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("SYCONSTRAINT")
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "GCHORIZONTAL",
        "VCONSTRAINT",
        "PCONSTRAINT",
        "QCONSTRAINT",
        "GCPERPENDICULAR",
        "ECONSTRAINT",
        "GCEQUAL",
        "TCONSTRAINT",
        "GCCONCENTRIC",
        "NRCONSTRAINT",
        "LCONSTRAINT",
        "FXCONSTRAINT",
        "GCFIX",
        "SYCONSTRAINT",
    ]
});
