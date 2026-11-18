//! Owned, mutable boundary representation.
//!
//! # Why this layer exists at all
//!
//! A file-format reader models topology as read-only views over a flat
//! record array, navigated by record index. That shape is right for parsing
//! and for lossless re-emission, and wrong for editing: the views borrow the
//! whole document, indices shift the moment anything is inserted, and there
//! is nowhere to put a face that a boolean has just split in two.
//!
//! So this layer owns its topology. Nodes live in arenas, keys stay stable
//! across edits, and adjacency is stored rather than rediscovered.
//!
//! # Provenance, and why it is not optional
//!
//! Every node carries the [`SourceRef`] it was lifted from, when it came
//! from a file. That single field is what keeps a save from degrading a
//! drawing:
//!
//! - a node that was never touched lowers back to its original record,
//!   byte for byte, carrying attributes and parameter-space curves that this
//!   kernel has no opinion about
//! - only a node the edit actually dirtied is re-emitted from scratch
//!
//! Without it, a boolean on one solid rewrites every solid in the file, and
//! anything the kernel does not model is lost on the way through. With it,
//! the blast radius of an edit is the edit.

pub mod arena;
pub mod audit;
pub mod blend;
mod blend_local;
pub mod boolean;
pub mod bridge;
pub mod surfacing;
pub mod bounds;
pub mod certified_offset;
mod chamfer_circular;
mod chamfer_prismatic;
mod chamfer_profile;
pub mod classify;
pub mod edit;
mod fillet_circular;
mod fillet_prismatic;
mod fillet_rolling;
pub mod geometry;
pub mod heal;
pub mod imprint;
pub mod intersect;
pub mod loft;
pub mod loft_general;
pub mod mass;
pub mod make;
pub mod mesh;
mod march;
mod near;
pub(crate) mod nurbs_builder;
pub mod nurbs_form;
pub mod pcurve;
pub mod place;
pub mod presspull;
pub mod recognize;
pub mod slice;
pub mod ssi;
pub mod shell;
pub mod split;
pub mod sweep;
pub mod sweep_path;
pub mod thicken;
pub mod topology;
pub mod wall_room;

pub use arena::{Arena, Key};
pub use blend::{
    chamfer, chamfer_edges, chamfer_fit, fillet, fillet_edges, fillet_fit, presspull, ChamferError,
    FilletError,
};
pub use boolean::{combine, combine_all, Operation};
pub use bounds::{body_bounds, face_bounds, operation_tolerance, Aabb};
pub use classify::{contains_point, Containment};
pub use geometry::{
    Circle3, Cone, Curve3, Cylinder, Ellipse3, Line3, OffsetSurface, Sphere, Surface, Torus,
};
pub use edit::{
    check, clean, delete_faces, extrude_faces, face_at_point, face_sheet, imprint_curve, move_faces,
    draft_faces, offset_faces, offset_limit, rotation, separate, taper_faces,
};
pub use imprint::{imprint, Imprint, Snag};
pub use intersect::{surfaces as intersect_surfaces, Meeting};
pub use loft::loft;
pub use loft_general::{loft_with_options, LoftError, LoftOptions, LoftSection};
pub use mass::{analytic_mass_properties, mass_properties, MassProperties};
pub use mesh::{body as mesh_body, Mesh};
pub use nurbs_form::{nurbs_curve_form, nurbs_form, quarter_parameter};
pub use recognize::{analytic_curve, analytic_surface};
pub use place::{edge_points, edge_polylines, transform, Placement};
pub use presspull::{
    extrusion_profile_pieces, intersect_planar_regions, planar_face_at_point, planar_face_profile,
    planar_region, presspull_face, presspull_region, subtract_planar_regions, union_planar_regions,
    PlanarFaceProfile, PlanarIntersection, PresspullMode,
};
pub use slice::{slice_by_plane, slice_by_surface, surface_side, PlaneSlice};
pub use shell::{shell, shell_face_at_point, ShellError};
pub use sweep::{
    extrude, extrude_region, extrude_surface, extrude_surface_region, revolve, revolve_region,
    revolve_surface, revolve_surface_region, sweep_along, sweep_along_deformed,
    sweep_along_polyline3d,
};
#[cfg(feature = "offset")]
pub use sweep::{
    extrude_region_tapered, extrude_surface_region_tapered, extrude_surface_tapered,
    extrude_tapered,
};
pub use nurbs_builder::{RationalCurve2, RationalCurve3};
pub use sweep_path::reversed_path_curves;
pub use sweep_path::{
    sweep_corner_refusal, sweep_path, sweep_path_has_corner, sweep_polyline_base, sweep_spatial_polyline, SweepRefusal, sweep_path_start, sweep_path_tangent, sweep_profile_base, sweep_profile_base_from,
    sweep_profile_group_base,
    sweep_profile_placement, SweepOptions, SweepPath,
};
pub use thicken::{thicken, ThickenError};
pub use topology::{
    Body, Coedge, CoedgeKey, CurveKey, Edge, EdgeKey, Face, FaceKey, Flaw, Loop, LoopKey, Lump,
    LumpKey, Shell, ShellKey, SurfaceKey, Vertex, VertexKey,
};

/// A node's origin in the document it was lifted from.
///
/// Opaque here on purpose: this layer knows a node came from record *n* of
/// something, not what a record is. The format layer that lifts a body owns
/// the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceRef(u32);

impl SourceRef {
    /// Wraps a format-layer record index.
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// The wrapped index, for the format layer to resolve.
    pub const fn index(&self) -> u32 {
        self.0
    }
}

/// Whether a node still matches the record it was lifted from.
///
/// Lowering consults this and nothing else: [`Provenance::Clean`] copies the
/// source record through untouched, anything else re-emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Lifted from a source record and unchanged since.
    Clean(SourceRef),
    /// Lifted from a source record, then edited.
    Dirty(SourceRef),
    /// Built by this kernel; there is no source record to fall back on.
    Synthesized,
}

impl Provenance {
    /// The source record, if this node came from one.
    pub const fn source(&self) -> Option<SourceRef> {
        match self {
            Self::Clean(reference) | Self::Dirty(reference) => Some(*reference),
            Self::Synthesized => None,
        }
    }

    /// Whether lowering may reuse the source record verbatim.
    pub const fn is_reusable(&self) -> bool {
        matches!(self, Self::Clean(_))
    }

    /// Marks an edited node, keeping the source reference for diagnostics.
    pub fn soil(&mut self) {
        if let Self::Clean(reference) = *self {
            *self = Self::Dirty(reference);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_node_lowers_from_its_source() {
        let provenance = Provenance::Clean(SourceRef::new(7));
        assert!(provenance.is_reusable());
        assert_eq!(provenance.source().map(|r| r.index()), Some(7));
    }

    #[test]
    fn soiling_blocks_reuse_but_keeps_the_reference() {
        let mut provenance = Provenance::Clean(SourceRef::new(7));
        provenance.soil();
        assert!(!provenance.is_reusable());
        assert_eq!(provenance.source().map(|r| r.index()), Some(7));
    }

    #[test]
    fn soiling_is_idempotent() {
        let mut provenance = Provenance::Clean(SourceRef::new(7));
        provenance.soil();
        let once = provenance;
        provenance.soil();
        assert_eq!(provenance, once);
    }

    #[test]
    fn synthesized_nodes_have_nothing_to_fall_back_on() {
        let provenance = Provenance::Synthesized;
        assert!(!provenance.is_reusable());
        assert_eq!(provenance.source(), None);
    }
}
