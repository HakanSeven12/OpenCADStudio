use document::entities::{
    Dimension, DimensionAligned, DimensionAngular2Ln, DimensionAngular3Pt, DimensionArc,
    DimensionBase, DimensionDiameter, DimensionLargeRadial, DimensionLinear, DimensionOrdinate,
    DimensionRadius,
};
use glam::{DVec3, Vec3};

use crate::entities::common::{
    center_grip, edit_angle_prop as edit_angle, edit_prop as edit, lineweight_label,
    lineweight_options, parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable};
use crate::entities::transform::EntityTransform;
use crate::scene::convert::tess_util::aci_to_rgba;
use crate::scene::convert::tessellate::{
    add_polyline, add_segment, append_arrow, arrow_from_block_with_deferred_hatch, normalized_or,
    ArrowKind, DimGeom,
};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::{SnapHint, TangentGeom, WireModel};
use crate::t;
use document::entities::{MText, Text};
use document::tables::DimStyle;
use document::types::{Color as AcadColor, Vector3};
use document::{Document, EntityType, Handle};

pub(crate) fn dimension_text_override(base: &DimensionBase) -> Option<&str> {
    base.user_text
        .as_deref()
        .or_else(|| (!base.text.is_empty()).then_some(base.text.as_str()))
}

pub(crate) fn set_dimension_text_override(base: &mut DimensionBase, text: Option<String>) {
    base.text = text.clone().unwrap_or_default();
    base.user_text = text;
}

/// Recompute the fields a script cannot set: the base definition point and
/// the stored measurement, exactly as grip edits do.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn normalize_scripted_dimension(dim: &mut Dimension) {
    let definition_point = dimension_definition_point(dim);
    dim.base_mut().definition_point = definition_point;
    dim.base_mut().actual_measurement = dim.stored_measurement();
}

fn dimension_definition_point(dim: &Dimension) -> document::types::Vector3 {
    match dim {
        Dimension::Aligned(d) => d.definition_point,
        Dimension::Linear(d) => d.definition_point,
        Dimension::Radius(d) => d.definition_point,
        Dimension::Diameter(d) => d.definition_point,
        Dimension::Angular2Ln(d) => d.definition_point,
        Dimension::Angular3Pt(d) => d.definition_point,
        Dimension::Ordinate(d) => d.definition_point,
        Dimension::Arc(d) => d.definition_point,
        Dimension::LargeRadial(d) => d.definition_point,
    }
}

mod baked;
mod format;
mod geometry;
mod grips;
mod ocs;
mod props;
mod style;
mod style_props;
mod tess;
mod text_layout;

pub(crate) use self::baked::*;
pub(crate) use self::format::*;
pub(crate) use self::geometry::*;
pub(crate) use self::ocs::*;
pub(crate) use self::props::*;
pub(crate) use self::style::*;
pub(crate) use self::style_props::*;
pub use self::style_props::style_sections;
pub use self::tess::DimensionTess;
pub(crate) use self::text_layout::*;

#[cfg(test)]
mod angular_unit_tests;
#[cfg(test)]
mod arch_format_tests;
#[cfg(test)]
mod arrow_tip_grips_tests;
#[cfg(test)]
mod dim_color_override_tests;
#[cfg(test)]
mod dimtad_tests;
#[cfg(test)]
mod dimtmove_leader_tests;
#[cfg(test)]
mod layout_parity_tests;
#[cfg(test)]
mod limits_format_tests;
#[cfg(test)]
mod linear_transform_tests;
#[cfg(test)]
mod text_width_factor_tests;

/// The dimension an entity is, if it is one. The variant holds it boxed, so
/// a pattern cannot reach into it; match on this instead.
pub(crate) fn as_dimension(
    entity: &document::EntityType,
) -> Option<&document::entities::Dimension> {
    match entity {
        document::EntityType::Dimension(dimension) => Some(dimension),
        _ => None,
    }
}

/// [`as_dimension`], mutably.
pub(crate) fn as_dimension_mut(
    entity: &mut document::EntityType,
) -> Option<&mut document::entities::Dimension> {
    match entity {
        document::EntityType::Dimension(dimension) => Some(dimension),
        _ => None,
    }
}
