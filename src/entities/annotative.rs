//! Whether an object is annotative, and where its annotation-scale contexts
//! put it: the queries on the document that need no scene.

use document::Document;
use document::Handle;
use document::EntityType;
use document::Vector3;
use document::entities::EntityCommon;
use document::objects::Dictionary;
use document::objects::MTextContext;
use document::objects::ObjectContextData;
use document::objects::ObjectContextKind;
use document::objects::ObjectType;
use document::entities::Dimension;

/// Resolve a handle to a `Dictionary` object, if it is one.
pub fn as_dict(doc: &Document, handle: Handle) -> Option<&Dictionary> {
    match doc.objects.get(&handle) {
        Some(ObjectType::Dictionary(d)) => Some(d),
        _ => None,
    }
}

pub(crate) fn annotation_scales_in_xdictionary(doc: &Document, xdictionary: Handle) -> Option<Handle> {
    let mgr = as_dict(doc, xdictionary).and_then(|d| d.get("AcDbContextDataManager"))?;
    as_dict(doc, mgr).and_then(|d| d.get("ACDB_ANNOTATIONSCALES"))
}

pub(crate) fn active_context_in(
    doc: &Document,
    collection: Handle,
    scale_handle: Option<Handle>,
) -> Option<&ObjectContextData> {
    let coll = as_dict(doc, collection)?;
    let mut default = None;
    let mut first = None;
    for (_, leaf_h) in &coll.entries {
        let Some(ObjectType::ObjectContextData(leaf)) = doc.objects.get(leaf_h) else {
            continue;
        };
        first.get_or_insert(leaf);
        if leaf.is_default {
            default = Some(leaf);
        }
        if let Some(target) = scale_handle {
            if leaf.scale == target {
                return Some(leaf);
            }
        } else if let Some(ObjectType::Scale(scale)) = doc.objects.get(&leaf.scale) {
            if scale
                .name
                .eq_ignore_ascii_case(&doc.header.current_annotation_scale)
            {
                return Some(leaf);
            }
        }
    }
    default.or(first)
}

pub(crate) fn context_ratio_in(
    doc: &Document,
    collection: Handle,
    scale_handle: Option<Handle>,
) -> Option<f64> {
    let coll = as_dict(doc, collection)?;
    let active = active_context_in(doc, collection, scale_handle)?;
    let native = coll
        .entries
        .iter()
        .find_map(|(_, leaf_h)| match doc.objects.get(leaf_h) {
            Some(ObjectType::ObjectContextData(leaf)) if leaf.is_default => Some(leaf),
            _ => None,
        })?;
    let (Some(ObjectType::Scale(active_scale)), Some(ObjectType::Scale(native_scale))) = (
        doc.objects.get(&active.scale),
        doc.objects.get(&native.scale),
    ) else {
        return None;
    };
    let native_factor = native_scale.inverse_factor();
    if native_factor.abs() <= 1.0e-12 {
        return None;
    }
    let relative = active_scale.inverse_factor() / native_factor;
    (relative.is_finite() && relative > 0.0).then_some(relative)
}

pub(crate) fn mtext_attachment(value: i32) -> document::entities::AttachmentPoint {
    use document::entities::AttachmentPoint;
    match value {
        2 => AttachmentPoint::TopCenter,
        3 => AttachmentPoint::TopRight,
        4 => AttachmentPoint::MiddleLeft,
        5 => AttachmentPoint::MiddleCenter,
        6 => AttachmentPoint::MiddleRight,
        7 => AttachmentPoint::BottomLeft,
        8 => AttachmentPoint::BottomCenter,
        9 => AttachmentPoint::BottomRight,
        _ => AttachmentPoint::TopLeft,
    }
}

pub(crate) fn apply_mtext_context(entity: &mut document::entities::MText, context: &MTextContext) {
    entity.attachment_point = mtext_attachment(context.attachment);
    entity.insertion_point = context.insertion;
    entity.rectangle_width = context.rect_width;
    entity.rectangle_height = (context.rect_height > 0.0).then_some(context.rect_height);
    entity.extents_width = context.extents_width;
    entity.extents_height = context.extents_height;
    entity.rotation = context.x_axis_dir.y.atan2(context.x_axis_dir.x);
    entity.dwg_x_direction = Some(context.x_axis_dir);
    if let Some(columns) = &context.columns {
        entity.column_data.column_type = context.column_type as i16;
        entity.column_data.column_count = columns.num_heights;
        entity.column_data.width = columns.width;
        entity.column_data.gutter = columns.gutter;
        entity.column_data.auto_height = columns.auto_height;
        entity.column_data.flow_reversed = columns.flow_reversed;
        entity.column_data.heights.clone_from(&columns.heights);
    } else {
        entity.column_data.column_type = context.column_type as i16;
        entity.column_data.column_count = 0;
        entity.column_data.heights.clear();
    }
}

pub(crate) fn apply_attribute_context(
    insertion_point: &mut Vector3,
    alignment_point: &mut Vector3,
    rotation: &mut f64,
    horizontal_alignment: &mut document::entities::HorizontalAlignment,
    embedded_mtext: &mut Option<Box<document::entities::MText>>,
    context: &document::objects::MTextAttributeContext,
) {
    insertion_point.x = context.insertion.x;
    insertion_point.y = context.insertion.y;
    alignment_point.x = context.alignment.x;
    alignment_point.y = context.alignment.y;
    *rotation = context.rotation;
    *horizontal_alignment =
        document::entities::HorizontalAlignment::from_value(context.horizontal_mode);
    if context.enable_context {
        if let (Some(embedded), Some(mtext)) = (&context.context, embedded_mtext.as_mut()) {
            apply_mtext_context(mtext, &embedded.mtext);
        }
    }
}

/// Place an INSERT's attribute for an annotation scale (the displayed one when
/// `None`): the position of that scale's representation, and the stored height
/// at the ratio to the default representation, as annotative text draws. An
/// ATTRIB is part of its INSERT rather than a document entity, so its scale data
/// is found from its own extension dictionary. `false` leaves it untouched: it
/// has no representations.
pub(crate) fn place_attribute_for_annotation_scale(
    doc: &Document,
    attribute: &mut document::entities::AttributeEntity,
    scale_handle: Option<Handle>,
) -> bool {
    if explicit_annotation_flag(doc, &attribute.common) == Some(false) {
        return false;
    }
    let Some(collection) = attribute
        .common
        .xdictionary_handle
        .and_then(|xdictionary| annotation_scales_in_xdictionary(doc, xdictionary))
    else {
        return false;
    };
    let Some(ObjectContextData {
        kind: ObjectContextKind::MTextAttribute(context),
        ..
    }) = active_context_in(doc, collection, scale_handle)
    else {
        return false;
    };
    apply_attribute_context(
        &mut attribute.insertion_point,
        &mut attribute.alignment_point,
        &mut attribute.rotation,
        &mut attribute.horizontal_alignment,
        &mut attribute.embedded_mtext,
        context,
    );
    let ratio = context_ratio_in(doc, collection, scale_handle).unwrap_or(1.0);
    attribute.height *= ratio;
    if let Some(mtext) = attribute.embedded_mtext.as_mut() {
        mtext.height *= ratio;
        if !context.enable_context || context.context.is_none() {
            mtext.rectangle_width *= ratio;
        }
    }
    true
}

/// Whether an object carries a per-object annotation context with at least one
/// per-scale representation — its extension dictionary holds an
/// `AcDbContextDataManager` whose `ACDB_ANNOTATIONSCALES` collection is
/// non-empty. This catches objects that are annotative by context even when
/// their style is not.
///
/// The non-empty requirement matters: a context manager with an *empty*
/// `ACDB_ANNOTATIONSCALES` is a single-representation marker with no per-scale
/// reps (common in files where objects were flagged annotative but never given
/// a scale). Such an object has nothing to scale *to*, so it must render at its
/// base geometry — treating it as annotative would (mis)scale it by the
/// annotation factor in annotation-scaled viewports, ballooning the text.
pub(crate) fn has_context_manager(doc: &Document, common: &EntityCommon) -> bool {
    let key = |d: &Dictionary, name: &str| {
        d.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, h)| *h)
    };
    let Some(xd) = common.xdictionary_handle.and_then(|h| as_dict(doc, h)) else {
        return false;
    };
    let Some(mgr) = key(xd, "AcDbContextDataManager").and_then(|h| as_dict(doc, h)) else {
        return false;
    };
    key(mgr, "ACDB_ANNOTATIONSCALES")
        .and_then(|h| as_dict(doc, h))
        .map(|coll| !coll.entries.is_empty())
        .unwrap_or(false)
}

/// Whether a MULTILEADER participates in annotation scaling through its
/// per-object context or entity flag. A later style edit is applied only by an
/// explicit style update, so it cannot retroactively change existing objects.
pub fn mleader_is_annotative(doc: &Document, mleader: &document::entities::MultiLeader) -> bool {
    has_context_manager(doc, &mleader.common) || mleader.enable_annotation_scale
}

pub(crate) fn annotation_flag(data: &document::xdata::ExtendedData) -> Option<bool> {
    data.get_record("AcadAnnotative")?
        .values
        .iter()
        .filter_map(|value| match value {
            document::xdata::XDataValue::Integer16(value) => Some(*value != 0),
            _ => None,
        })
        .next_back()
}

pub(crate) fn definition_annotation_flag(doc: &Document, br: &document::tables::BlockRecord) -> Option<bool> {
    doc.object_xdata(br.handle)
        .and_then(annotation_flag)
        .or_else(|| {
            doc.get_entity(br.block_entity_handle)
                .and_then(|block| annotation_flag(&block.common().extended_data))
        })
}

pub(crate) fn block_annotation_flag(doc: &Document, name: &str) -> Option<bool> {
    let br = doc.block_records.get(name)?;
    definition_annotation_flag(doc, br).or_else(|| {
        // Evaluated anonymous blocks inherit the named definition's flag.
        // Keep "absent" distinct from an explicit off: old contexts alone
        // are still supported when neither definition stores a flag.
        let source = doc
            .object_xdata(br.handle)?
            .get_record("AcDbBlockRepBTag")?
            .values
            .iter()
            .find_map(|value| match value {
                document::xdata::XDataValue::Handle(handle) if !handle.is_null() => Some(*handle),
                _ => None,
            })?;
        doc.block_records
            .iter()
            .find(|record| record.handle == source)
            .and_then(|record| definition_annotation_flag(doc, record))
    })
}

pub(crate) fn insert_annotation_flag(doc: &Document, insert: &document::entities::Insert) -> Option<bool> {
    #[cfg(test)]
    INSERT_FLAG_LOOKUPS.with(|count| count.set(count.get() + 1));
    block_annotation_flag(doc, &insert.block_name).or_else(|| {
        // Some files link the source only through per-reference RepData.
        let source = doc.dynamic_definition_for_insert(insert.common.handle)?;
        // A plain reference's source is its own definition, checked above;
        // only a different one is worth the scan of every block record.
        if doc
            .block_records
            .get(&insert.block_name)
            .is_some_and(|record| record.handle == source)
        {
            return None;
        }
        doc.block_records
            .iter()
            .find(|record| record.handle == source)
            .and_then(|record| definition_annotation_flag(doc, record))
    })
}

/// Explicit flags outrank retained context dictionaries. Attributes without
/// their own flag also stop using obsolete contexts when their INSERT's
/// definition has annotation scaling disabled.
pub(crate) fn explicit_annotation_flag(doc: &Document, common: &EntityCommon) -> Option<bool> {
    annotation_flag(&common.extended_data).or_else(|| {
        if let Some(EntityType::Insert(insert)) = doc.get_entity(common.handle) {
            return insert_annotation_flag(doc, insert);
        }
        if let Some(EntityType::Insert(insert)) = doc.get_entity(common.owner_handle) {
            return annotation_flag(&insert.common.extended_data)
                .or_else(|| insert_annotation_flag(doc, insert))
                .filter(|flag| !flag);
        }
        None
    })
}

/// Whether an object participates in annotation scaling. Explicit object and
/// effective block-definition flags outrank contexts; text styles do not count.
pub fn object_is_annotative(doc: &Document, common: &EntityCommon) -> bool {
    let xd = &common.extended_data;
    if let Some(flag) = explicit_annotation_flag(doc, common) {
        return flag;
    }
    // Per-object annotation context (works regardless of style).
    has_context_manager(doc, common)
        || xd.get_record("AcAnnoPO").is_some()
        || xd.get_record("AcAnnotativeData").is_some()
}

#[cfg(test)]
thread_local! {
    /// How many times this thread asked for the annotation flag of a block
    /// reference's definition (`insert_annotation_flag`).
    pub(crate) static INSERT_FLAG_LOOKUPS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

pub fn scale_handle_by_name(doc: &Document, name: &str) -> Option<Handle> {
    doc.objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::Scale(scale)
                if !scale.is_temporary && scale.name.eq_ignore_ascii_case(name) =>
            {
                Some(*handle)
            }
            _ => None,
        })
}

/// The annotation scales an object currently carries a per-object context for,
/// as `(scale name, scale handle)` pairs (one per representation). Empty when
/// the object has no per-object context chain.
pub fn object_scale_memberships(doc: &Document, entity: Handle) -> Vec<(String, Handle)> {
    let mut out = Vec::new();
    let Some(coll_h) = annotation_scales_dict(doc, entity) else {
        return out;
    };
    if let Some(coll) = as_dict(doc, coll_h) {
        for (_, lh) in &coll.entries {
            if let Some(ObjectType::ObjectContextData(leaf)) = doc.objects.get(lh) {
                if let Some(ObjectType::Scale(s)) = doc.objects.get(&leaf.scale) {
                    out.push((s.name.clone(), leaf.scale));
                }
            }
        }
    }
    out
}

/// Resolve an entity's `ACDB_ANNOTATIONSCALES` collection dictionary handle, if
/// its context chain exists.
pub(crate) fn annotation_scales_dict(doc: &Document, entity: Handle) -> Option<Handle> {
    let xd = doc
        .get_entity(entity)
        .and_then(|e| e.common().xdictionary_handle)?;
    annotation_scales_in_xdictionary(doc, xd)
}

/// Whether an entity participates in annotation scaling.
pub fn is_annotative(doc: &Document, entity: &EntityType) -> bool {
    if object_is_annotative(doc, entity.common()) {
        return true;
    }
    // Annotative via the entity's own flag.
    // Text styles can be made annotative without converting existing text;
    // those objects must keep their stored height until explicitly updated.
    match entity {
        EntityType::Text(_) => false,
        EntityType::MText(t) => t.is_annotative,
        EntityType::AttributeEntity(attribute) => attribute.flags.annotative,
        EntityType::AttributeDefinition(attribute) => attribute.flags.annotative,
        EntityType::MultiLeader(ml) => mleader_is_annotative(doc, ml),
        _ => false,
    }
}

/// Resolve style metrics used by saved dimension geometry.
/// The size multiplier of an annotative dimension's picture: the model units
/// per paper unit of the scale it is drawn at: the current annotation scale
/// when the dimension has it or has no scales, else the first of its scales. The saved picture
/// is what a reader shows for that scale, so it can't be drawn at 1:1.
pub(crate) fn annotative_picture_scale(dim: &Dimension, doc: &Document) -> f64 {
    let memberships =
        crate::entities::annotative::object_scale_memberships(doc, dim.base().common.handle);
    let current = &doc.header.current_annotation_scale;
    let chosen = memberships
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(current))
        .or_else(|| memberships.first());
    let factor = |handle: &Handle| match doc.objects.get(handle) {
        // The scale's own ratio, whatever the drawing units: annotation
        // sizes are in drawing units (#2354).
        Some(document::objects::ObjectType::Scale(scale)) => Some(scale.inverse_factor()),
        _ => None,
    };
    // Without scales of its own, a dimension annotative by its style draws at
    // the current one.
    let handle = chosen
        .map(|(_, handle)| *handle)
        .or_else(|| crate::entities::annotative::scale_handle_by_name(doc, current));
    handle
        .and_then(|handle| factor(&handle))
        .filter(|f| f.is_finite() && *f > 0.0)
        .unwrap_or(1.0)
}
