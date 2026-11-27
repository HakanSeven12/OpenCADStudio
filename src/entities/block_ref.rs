//! Where a block reference puts its block: the transform of each reference
//! and array cell, the blocks an entity draws through, and the definition
//! queries those need.

use document::{Document, EntityType, Handle};
use document::types::Color;
use crate::entities::insert::BlockScalePolicy;
use document::Vector3;
use document::entities::Insert;
use document::types::Matrix3;
use document::types::Matrix4;
use document::types::Transform;

pub(crate) fn layer_aci(document: &Document, layer: &str) -> u8 {
    document
        .layers
        .get(layer)
        .and_then(|layer| match &layer.color {
            Color::Index(index) => Some(*index),
            _ => None,
        })
        .unwrap_or(0)
}

pub fn block_base_point(document: &Document, block_name: &str) -> Vector3 {
    block_record_by_name(document, block_name)
        .map(|record| {
            if let Some((_, parameter)) = crate::entities::insert::block_base_point_parameter(document, record.handle)
            {
                return parameter.point;
            }
            document
                .get_entity(record.block_entity_handle)
                .and_then(|entity| match entity {
                    EntityType::Block(block) => Some(block.base_point),
                    _ => None,
                })
                .unwrap_or(record.base_point)
        })
        .unwrap_or(Vector3::ZERO)
}

/// An annotative dimension with a single scale representation has a single
/// picture, its own block: edits drop it and moves carry it like any other
/// dimension's. With several, each scale's block is a separate picture those
/// edits leave behind, so it is drawn from its definition instead.
pub(crate) fn single_scale_picture(document: &Document, handle: Handle, block: &str) -> bool {
    crate::entities::annotative::object_scale_memberships(document, handle).len() == 1
        && matches!(
            document.get_entity(handle),
            Some(EntityType::Dimension(stored))
                if stored.base().block_name.trim().eq_ignore_ascii_case(block)
        )
}

/// An unedited dimension whose definition points measure something other than
/// the measurement it stores (group 42) cannot be rebuilt: a dimension copied
/// into a block can keep some points in world coordinates, so one stored as 65
/// rebuilds as 6570. The source application draws the scale's picture, and so
/// do we. Saving refreshes the measurement of every dimension it re-bakes, so
/// a picture an edit left behind is never chosen this way.
pub(crate) fn rebuild_disagrees_with_picture(document: &Document, handle: Handle) -> bool {
    let Some(EntityType::Dimension(stored)) = document.get_entity(handle) else {
        return false;
    };
    let measured = stored.stored_measurement();
    let kept = stored.base().actual_measurement;
    !stored.base().block_name.trim().is_empty()
        && (measured - kept).abs() > 1e-6 * measured.abs().max(kept.abs()).max(1.0)
}

/// An annotative dimension's picture drawn at another size than its scale
/// gives: text far from DIMTXT times the scale. Earlier versions of this
/// program saved such pictures at 1:1, and they came back oversized on every
/// open; the dimension is rebuilt instead.
pub(crate) fn picture_off_scale(
    document: &Document,
    dimension: &document::entities::Dimension,
    record: &document::tables::BlockRecord,
) -> bool {
    let name = dimension.base().style_name.trim();
    let Some(style) = document.dim_styles.iter().find(|style| {
        style.name.eq_ignore_ascii_case(name)
            || (name.is_empty() && style.name.eq_ignore_ascii_case("Standard"))
    }) else {
        return false;
    };
    let style_annotative = style.annotative;
    let style = crate::entities::dimension::resolved_dimension_style(style, dimension, document);
    // A fixed DIMSCALE, or a non-annotative one (DIMSCALE 0 there follows
    // the viewport), sizes the picture, not the annotation scale.
    if !(style.annotative || style_annotative) || style.dimscale > 1e-6 {
        return false;
    }
    let expected = style.dimtxt
        * crate::entities::annotative::annotative_picture_scale(dimension, document);
    let drawn = record.entity_handles.iter().find_map(|handle| match document.get_entity(*handle) {
        Some(EntityType::Text(text)) => Some(text.height),
        Some(EntityType::MText(text)) => Some(text.height),
        _ => None,
    });
    match drawn {
        Some(drawn) if expected > 0.0 && drawn > 0.0 => !(0.8..=1.25).contains(&(drawn / expected)),
        _ => false,
    }
}

/// The block reference that places a dimension's picture block: at the
/// dimension's insertion point, with its insertion rotation and scale. The
/// point is stored in the dimension's OCS; the picture's sub-entities are in
/// world coordinates, so only the point goes through the OCS. A zero scale
/// component (an unread field) counts as 1 rather than collapsing the picture.
pub(crate) fn dimension_picture_insert(
    base: &document::entities::dimension::DimensionBase,
    block_name: String,
) -> Insert {
    let scale = |s: f64| if s.abs() < 1e-12 { 1.0 } else { s };
    let point = Matrix3::arbitrary_axis(base.normal) * base.insertion_point;
    Insert::new(block_name, point)
        .with_scale(
            scale(base.insertion_scale.x),
            scale(base.insertion_scale.y),
            scale(base.insertion_scale.z),
        )
        .with_rotation(base.insertion_rotation)
}

pub fn insert_transform(document: &Document, insert: &Insert) -> Transform {
    insert_transform_with_base(insert, block_base_point(document, &insert.block_name))
}

/// [`insert_transform`] for a caller that already resolved the block's base
/// point (the `-INSERT` command builds the INSERT before the drawing is in
/// scope), so the base-point convention lives in exactly one place:
/// world = insert_point + Rot(rotation) * Scale * (P - base).
pub fn insert_transform_with_base(insert: &Insert, base: Vector3) -> Transform {
    Transform::from_translation(Vector3::new(-base.x, -base.y, -base.z))
        .then(&insert.get_transform())
}

pub fn insert_transform_with_policy(
    document: &Document,
    insert: &Insert,
    annotation_scale: f32,
    scale_policy: BlockScalePolicy,
) -> Transform {
    let mut transform = insert_transform(document, insert);
    if scale_policy == BlockScalePolicy::FromInsert
        && (annotation_scale - 1.0).abs() > 1.0e-6
        && insert
            .common
            .extended_data
            .get_record("AcAnnotativeData")
            .is_some()
    {
        let point = insert.insert_point;
        let scale = Transform::from_translation(Vector3::new(-point.x, -point.y, -point.z))
            .then(&Transform::from_scale(annotation_scale as f64))
            .then(&Transform::from_translation(Vector3::new(
                point.x, point.y, point.z,
            )));
        transform = transform.then(&scale);
    }
    transform
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum BlockRole {
    Insert,
    DimensionPicture,
    TablePicture,
    TableCell,
    MultiLeaderContent,
    LineStyleSymbol,
    ArrowHead,
}

#[derive(Clone)]
pub struct BlockUse {
    pub block: Handle,
    pub insert: Insert,
    pub role: BlockRole,
    pub scale_policy: BlockScalePolicy,
    /// Whether this edge belongs to the active render representation. Inactive
    /// edges remain dependencies so cache invalidation and purge stay complete.
    pub active: bool,
    pub replaces_host_wire: bool,
    pub suppress_root_points: bool,
}

pub(crate) fn block_record_by_name<'a>(
    document: &'a Document,
    name: &str,
) -> Option<&'a document::tables::BlockRecord> {
    document.block_records.get(name).or_else(|| {
        document
            .block_records
            .iter()
            .find(|record| record.name.eq_ignore_ascii_case(name))
    })
}

pub(crate) fn block_use(
    document: &Document,
    mut insert: Insert,
    role: BlockRole,
    scale_policy: BlockScalePolicy,
    active: bool,
    replaces_host_wire: bool,
    suppress_root_points: bool,
) -> BlockUse {
    let block = block_record_by_name(document, &insert.block_name).map_or(Handle::NULL, |record| {
        insert.block_name.clone_from(&record.name);
        record.handle
    });
    BlockUse {
        block,
        insert,
        role,
        scale_policy,
        active,
        replaces_host_wire,
        suppress_root_points,
    }
}

/// Every direct block edge owned by an entity. One host may have multiple
/// edges, such as block-valued table cells. Representation flags let renderers
/// choose the active picture while dependency consumers keep the full graph.
pub(crate) fn entity_owned_block_uses(
    document: &Document,
    entity: &EntityType,
    annotation_scale: f32,
) -> Vec<BlockUse> {
    match entity {
        EntityType::Insert(insert) => vec![block_use(
            document,
            insert.clone(),
            BlockRole::Insert,
            BlockScalePolicy::FromInsert,
            true,
            true,
            false,
        )],
        EntityType::Dimension(dimension) => {
            let name = dimension.base().block_name.trim();
            if let Some(record) = block_record_by_name(document, name)
                .filter(|record| !record.entity_handles.is_empty())
            {
                let base = dimension.base();
                let mut insert = dimension_picture_insert(base, record.name.clone());
                insert.common = base.common.clone();
                let picture = (!crate::entities::annotative::is_annotative(document, entity)
                    || single_scale_picture(document, base.common.handle, &record.name)
                    || rebuild_disagrees_with_picture(document, base.common.handle))
                    && !picture_off_scale(document, dimension, record);
                vec![block_use(
                    document,
                    insert,
                    BlockRole::DimensionPicture,
                    BlockScalePolicy::FromInsert,
                    picture,
                    picture,
                    true,
                )]
            } else {
                Vec::new()
            }
        }
        EntityType::Table(table) => {
            let picture_record = table.block_record_handle.and_then(|handle| {
                document
                    .block_records
                    .iter()
                    .find(|record| record.handle == handle)
                    .filter(|record| !record.entity_handles.is_empty())
            });
            let mut uses = Vec::new();
            if let Some(record) = picture_record {
                let mut insert = Insert::new(record.name.clone(), table.insertion_point);
                insert.rotation = table
                    .horizontal_direction
                    .y
                    .atan2(table.horizontal_direction.x);
                insert.common = table.common.clone();
                uses.push(block_use(
                    document,
                    insert,
                    BlockRole::TablePicture,
                    BlockScalePolicy::FromInsert,
                    true,
                    true,
                    false,
                ));
            }
            let cells_active = picture_record.is_none();
            uses.extend(
                crate::entities::table::block_cell_inserts(table, document, annotation_scale)
                    .into_iter()
                    .map(|insert| {
                        block_use(
                            document,
                            insert,
                            BlockRole::TableCell,
                            BlockScalePolicy::Applied,
                            cells_active,
                            false,
                            false,
                        )
                    }),
            );
            uses
        }
        EntityType::MultiLeader(multileader) => {
            crate::entities::multileader::block_content_insert(document, multileader)
                .map(|insert| {
                    vec![block_use(
                        document,
                        insert,
                        BlockRole::MultiLeaderContent,
                        BlockScalePolicy::Applied,
                        true,
                        false,
                        false,
                    )]
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

pub fn entity_render_block_uses(
    document: &Document,
    entity: &EntityType,
    annotation_scale: f32,
) -> Vec<BlockUse> {
    entity_owned_block_uses(document, entity, annotation_scale)
}

pub(crate) const MAX_MINSERT_INSTANCES: usize = 20_000;

pub fn array_offsets(insert: &Insert) -> Vec<[f64; 3]> {
    if !insert.is_minsert() {
        return vec![[0.0; 3]];
    }
    let instance_count = insert.instance_count();
    if instance_count > MAX_MINSERT_INSTANCES {
        eprintln!(
            "MINSERT '{}' requests {} instances ({}x{}), exceeding the {} budget; rendering a single instance instead",
            insert.block_name,
            instance_count,
            insert.row_count,
            insert.column_count,
            MAX_MINSERT_INSTANCES
        );
        return vec![[0.0; 3]];
    }
    let mut offsets = Vec::with_capacity(instance_count);
    for row in 0..insert.row_count {
        for column in 0..insert.column_count {
            offsets.push([
                column as f64 * insert.column_spacing,
                row as f64 * insert.row_spacing,
                0.0,
            ]);
        }
    }
    offsets
}

pub fn insert_instance_transform(
    document: &Document,
    insert: &Insert,
    offset: [f64; 3],
    annotation_scale: f32,
    scale_policy: BlockScalePolicy,
) -> Transform {
    let transform = insert_transform_with_policy(document, insert, annotation_scale, scale_policy);
    if offset == [0.0; 3] {
        transform
    } else {
        let orientation = Transform::from_matrix(Matrix4::rotation_z(insert.rotation)).then(
            &Transform::from_matrix(Matrix4::from_matrix3(Matrix3::arbitrary_axis(
                insert.normal,
            ))),
        );
        let world_offset =
            orientation.apply_rotation(Vector3::new(offset[0], offset[1], offset[2]));
        transform.then(&Transform::from_translation(world_offset))
    }
}

pub fn insert_instance_translation_delta(
    document: &Document,
    insert: &Insert,
    offset: [f64; 3],
    annotation_scale: f32,
    scale_policy: BlockScalePolicy,
) -> Vector3 {
    let base =
        insert_instance_transform(document, insert, [0.0; 3], annotation_scale, scale_policy)
            .apply(Vector3::ZERO);
    let instance =
        insert_instance_transform(document, insert, offset, annotation_scale, scale_policy)
            .apply(Vector3::ZERO);
    instance - base
}

pub fn block_contains_hatch(
    document: &Document,
    block_name: &str,
    memo: &mut std::collections::HashMap<String, bool>,
) -> bool {
    block_contains_kind(document, block_name, memo, &|entity| {
        matches!(entity, EntityType::Hatch(_))
    })
}

/// Whether the block, or a block it nests, holds an entity `is_kind` accepts.
pub(crate) fn block_contains_kind(
    document: &Document,
    block_name: &str,
    memo: &mut std::collections::HashMap<String, bool>,
    is_kind: &dyn Fn(&EntityType) -> bool,
) -> bool {
    let key = block_name.to_ascii_lowercase();
    if let Some(&contains) = memo.get(&key) {
        return contains;
    }
    memo.insert(key.clone(), false);
    let contains = block_record_by_name(document, block_name).is_some_and(|record| {
        record
            .entity_handles
            .iter()
            .any(|&handle| match document.get_entity(handle) {
                Some(entity) if is_kind(entity) => true,
                Some(entity) => entity_render_block_uses(document, entity, 1.0)
                    .into_iter()
                    .filter(|block_use| block_use.active)
                    .any(|block_use| {
                        block_contains_kind(document, &block_use.insert.block_name, memo, is_kind)
                    }),
                _ => false,
            })
    });
    memo.insert(key, contains);
    contains
}
