// Trim / Extend — ribbon definitions + full command implementations.
//
// TRIM  (TR): Click the segment you want to remove. The command finds all
//             intersections of that entity with every other entity and trims
//             out the clicked interval. Stays active — click more segments,
//             press Enter to finish.
//
// EXTEND (EX): Click near one end of an entity.  The command extends that
//              endpoint to the nearest intersecting boundary. Stays active.

use std::f64::consts::TAU;

// The plane geometry these commands run on lives in opencadkernel; `geom` adapts
// its call shapes to the loose scalars and f32 render vertices used here.
use super::geom;
use super::geom::{
    arc_parameter as arc_t, arc_points as arc_pts, ellipse_closest_parameter,
    ellipse_points as ellipse_pts, lerp as lerp2, normalize_angle as norm,
};

use crate::modules::draw::fence::{crossing_box_preview, FencePick};

use document::entities::{
    Arc as ArcEnt, Circle as CircleEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline,
    LwVertex, Ray as RayEnt, Spline as SplineEnt, XLine as XLineEnt,
};
use document::types::Vector3;
use document::{EntityType, Handle};
use glam::DVec3;
use ocs_geom::geom2d::nurbs::clamped_uniform_knots;
use ocs_geom::geom2d::{
    intersect as kernel_intersect, trim_spans as kernel_trim_spans, Arc as KernelArc, BulgeArc,
    Circle as KernelCircle, Curve, Ellipse as KernelEllipse, EllipseArc as KernelEllipseArc,
    Extent as KernelExtent, Line as KernelLine, NurbsCurve, Ray as KernelRay,
    Tolerance as KernelTolerance, XLine as KernelXLine,
};

use crate::entities::curve::{
    entity_curve_xy, entity_with_lwpolyline_world_xy, entity_with_lwpolyline_world_xy_owned,
};

use crate::command::{CadCommand, CmdResult};
use crate::modules::draw::modify::spline_ops::{
    spline_cut, spline_nearest_t, spline_pts_wire, spline_range, spline_to_nurbs, t_to_rel,
};
use crate::modules::IconKind;
use crate::scene::model::wire_model::WireModel;
use crate::t;

use super::entity_index::{modify_aabb, ModifyEntityIndex};

mod hatch_region;

// ── Dropdown constants ─────────────────────────────────────────────────────

pub const DROPDOWN_ID: &str = "trim_extend";
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../../../assets/icons/trim.svg"));

pub const DROPDOWN_ITEMS: &[(&str, &str, IconKind)] = &[
    (
        "TRIM",
        "Trim",
        IconKind::Svg(include_bytes!("../../../../../assets/icons/trim.svg")),
    ),
    (
        "EXTEND",
        "Extend",
        IconKind::Svg(include_bytes!("../../../../../assets/icons/extend.svg")),
    ),
];

// ══════════════════════════════════════════════════════════════════════════
// Geometry helpers
// ══════════════════════════════════════════════════════════════════════════

// ── Boundary geometry ─────────────────────────────────────────────────────

/// Virtual extent used to represent infinite ends of Ray / XLine.
const TRIM_EXTENT: f64 = 1_000_000.0;

/// Sampling density for the plan-view point lists the fence and preview
/// passes walk. The renderer's own figure, so a preview cut lands where the
/// drawn geometry is rather than a chord away from it.
const SAMPLE_SEGMENTS_PER_RADIAN: f64 = ocs_geom::geom2d::DEFAULT_SEGMENTS_PER_RADIAN;

mod extend_cmd;
mod extrim_cmd;
mod fence;
mod geo;
mod hatch;
mod mode;
mod ops;
mod preview;
#[cfg(test)]
mod tests;
mod trim_cmd;

pub(crate) use self::extrim_cmd::*;
pub(crate) use self::fence::*;
pub(crate) use self::geo::*;
pub(crate) use self::hatch::*;
pub(crate) use self::mode::*;
pub(crate) use self::ops::*;
pub(crate) use self::preview::*;

pub use self::extend_cmd::ExtendCommand;
pub use self::extrim_cmd::ExtrimCommand;
pub use self::geo::is_trim_boundary_entity;
pub use self::mode::{quick_mode, set_quick_mode};
pub use self::trim_cmd::TrimCommand;
