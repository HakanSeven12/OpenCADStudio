// Boolean operations on 3D solids (Design group). The actual CSG runs in
// `App::solid_boolean` (src/app/model_ops.rs) using the kernel on the
// session-cached B-reps; this module just names the operations.

/// Which boolean a Design-group tool performs on the two selected solids.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

impl BoolOp {
    pub fn from_id(id: &str) -> Option<BoolOp> {
        Some(match id {
            "UNION" => BoolOp::Union,
            "SUBTRACT" => BoolOp::Subtract,
            "INTERSECT" => BoolOp::Intersect,
            _ => return None,
        })
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["UNION", "SUBTRACT", "INTERSECT"]
});
