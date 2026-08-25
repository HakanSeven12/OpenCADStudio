//! Shared annotative-object detection + annotation-scale resolution.
//!
//! Both the Properties panel (Annotative row / applied scale name) and the
//! tessellation bake (which scales annotative content by the current annotation
//! scale) must agree on *which* entities are annotative — so that logic lives
//! here, once. An entity is annotative if it carries a per-object annotation
//! context, legacy annotative XDATA, or an entity-level annotative flag. Text
//! style state is consulted while creating an object and when an explicit
//! annotation-style update is requested; changing a style alone does not
//! retroactively scale existing text.

use acadrust::entities::{EntityCommon, EntityType};
use acadrust::objects::{
    Dictionary, DimContext, DimSubtype, EmbeddedMTextContext, HatchScaleContext,
    MTextAttributeContext, MTextContext, ObjectContextData, ObjectContextKind, ObjectType,
};
use acadrust::types::{Vector2, Vector3};
use acadrust::{CadDocument, Handle};
use std::borrow::Cow;

/// Resolve a handle to a `Dictionary` object, if it is one.
pub fn as_dict(doc: &CadDocument, handle: Handle) -> Option<&Dictionary> {
    match doc.objects.get(&handle) {
        Some(ObjectType::Dictionary(d)) => Some(d),
        _ => None,
    }
}

/// Resolve the drawing's root named-objects dictionary, creating one if the
/// file has none reachable.
///
/// The canonical pointer is `header.named_objects_dict_handle`, but DWGs
/// written by some programs leave it dangling — pointing at a handle that never
/// loaded, or at a non-dictionary — while the real named-object sub-dictionaries
/// (`ACAD_LAYOUT`, `ACAD_SCALELIST`, …) are instead owned by an unrelated handle
/// that is not a dictionary. When the pointer can't be resolved we adopt any
/// top-level (`owner == NULL`) dictionary as the root; failing that we synthesise
/// a fresh, empty root so that *registering* a new named-object entry (a page
/// setup, an annotation scale, the `CTAB` variable) actually persists instead of
/// silently no-opping against a missing dictionary.
///
/// Idempotent: the resolved or created handle is written back to the header, so
/// later calls return the same dictionary rather than minting another root. On a
/// well-formed drawing this returns the existing root untouched.
pub fn root_named_dict_handle(doc: &mut CadDocument) -> Handle {
    let h = doc.header.named_objects_dict_handle;
    if matches!(doc.objects.get(&h), Some(ObjectType::Dictionary(_))) {
        return h;
    }
    // A top-level dictionary is already present (the standard root shape) — adopt
    // the richest one (matching the DWG writer's own root heuristic) and repair
    // the stale header pointer.
    if let Some(root) = doc
        .objects
        .iter()
        .filter_map(|(k, o)| match o {
            ObjectType::Dictionary(d) if d.owner.is_null() => Some((*k, d.entries.len())),
            _ => None,
        })
        .max_by_key(|&(_, n)| n)
        .map(|(k, _)| k)
    {
        doc.header.named_objects_dict_handle = root;
        return root;
    }
    // Nothing reachable — build a fresh, empty root named-objects dictionary.
    let nh = doc.allocate_handle();
    let mut d = Dictionary::new();
    d.handle = nh;
    d.owner = Handle::NULL;
    doc.objects.insert(nh, ObjectType::Dictionary(d));
    doc.header.named_objects_dict_handle = nh;
    nh
}

/// Set the per-object annotative flag on the entity types that carry one
/// (MTEXT, MULTILEADER, ATTRIB and ATTDEF). Turning it off also strips the per-object annotation
/// context and legacy markers via [`clear_annotation_context`] so the object
/// stops resolving annotative; turning it on leaves the base geometry as the
/// single (implicit, current-scale) representation. TEXT uses a context rather
/// than a native flag; other entity types are not toggled here.
pub fn set_entity_annotative(doc: &mut CadDocument, handle: Handle, want: bool) {
    if let Some(e) = doc.get_entity_mut(handle) {
        match e {
            EntityType::MText(t) => t.is_annotative = want,
            EntityType::MultiLeader(m) => m.enable_annotation_scale = want,
            EntityType::AttributeEntity(attribute) => attribute.flags.annotative = want,
            EntityType::AttributeDefinition(attribute) => attribute.flags.annotative = want,
            _ => {}
        }
    }
    if !want {
        clear_annotation_context(doc, handle);
    }
}

/// Derive the per-scale context payload for an entity from its current
/// placement. Returns the concrete class name and the context kind, or `None`
/// for entity types that do not carry a per-object annotation context.
fn dimension_context_for(doc: &CadDocument, dimension: &acadrust::entities::Dimension) -> Option<DimContext> {
    use acadrust::entities::Dimension;

    let subtype = match dimension {
        Dimension::Aligned(dim) => DimSubtype::Aligned {
            dimline_pt: dim.definition_point,
        },
        Dimension::Linear(dim) => DimSubtype::Aligned {
            dimline_pt: dim.definition_point,
        },
        Dimension::Angular2Ln(dim) => DimSubtype::Angular {
            arc_pt: dim.dimension_arc,
        },
        Dimension::Angular3Pt(dim) => DimSubtype::Angular {
            arc_pt: dim.definition_point,
        },
        Dimension::Diameter(dim) => DimSubtype::Diametric {
            first_arc_pt: dim.angle_vertex,
            def_pt: dim.definition_point,
        },
        Dimension::Radius(dim) => DimSubtype::Radial {
            first_arc_pt: dim.definition_point,
        },
        Dimension::LargeRadial(dim) => DimSubtype::RadialLarge {
            ovr_center: dim.override_center,
            jog_point: dim.jog_point,
        },
        Dimension::Ordinate(dim) => DimSubtype::Ordinate {
            feature_location_pt: dim.feature_location,
            leader_endpt: dim.leader_endpoint,
        },
        Dimension::Arc(_) => return None,
    };
    let base = dimension.base();
    let block = doc
        .block_records
        .iter()
        .find(|record| record.name.eq_ignore_ascii_case(&base.block_name))
        .map(|record| record.handle)
        .unwrap_or(Handle::NULL);
    Some(DimContext {
        def_pt: Vector2::new(base.text_middle_point.x, base.text_middle_point.y),
        is_def_textloc: base.text_user_positioned,
        text_rotation: base.text_rotation,
        block,
        b293: false,
        dimtofl: false,
        dimosxd: false,
        dimatfit: false,
        dimtix: false,
        dimtmove: false,
        override_code: 0,
        has_arrow2: false,
        flip_arrow2: base.flip_arrow2,
        flip_arrow1: base.flip_arrow1,
        subtype,
    })
}

fn mtext_context_for(m: &acadrust::entities::MText) -> MTextContext {
    MTextContext {
        attachment: m.attachment_point as i32,
        x_axis_dir: m
            .dwg_x_direction
            .unwrap_or_else(|| Vector3::new(m.rotation.cos(), m.rotation.sin(), 0.0)),
        insertion: m.insertion_point,
        rect_width: m.rectangle_width,
        rect_height: m.rectangle_height.unwrap_or(0.0),
        extents_width: m.extents_width,
        extents_height: m.extents_height,
        column_type: m.column_data.column_type as i32,
        columns: (m.column_data.column_type != 0).then(|| acadrust::objects::MTextColumns {
            num_heights: m.column_data.column_count,
            width: m.column_data.width,
            gutter: m.column_data.gutter,
            auto_height: m.column_data.auto_height,
            flow_reversed: m.column_data.flow_reversed,
            heights: m.column_data.heights.clone(),
        }),
    }
}

fn attribute_context_for(
    insertion: Vector3,
    alignment: Vector3,
    rotation: f64,
    horizontal_mode: i16,
    embedded: Option<&acadrust::entities::MText>,
    scale: Handle,
) -> MTextAttributeContext {
    MTextAttributeContext {
        horizontal_mode,
        rotation,
        insertion: Vector2::new(insertion.x, insertion.y),
        alignment: Vector2::new(alignment.x, alignment.y),
        enable_context: embedded.is_some(),
        context: embedded.map(|mtext| EmbeddedMTextContext {
            owner_handle: Handle::NULL,
            reactors: Vec::new(),
            xdictionary_handle: None,
            has_binary_data: false,
            class_version: 3,
            is_default: false,
            scale,
            mtext: mtext_context_for(mtext),
        }),
    }
}

fn context_kind_for(
    doc: &CadDocument,
    entity: &EntityType,
    scale: Handle,
) -> Option<(&'static str, ObjectContextKind)> {
    match entity {
        EntityType::Insert(ins) => Some((
            "ACDB_BLKREFOBJECTCONTEXTDATA_CLASS",
            ObjectContextKind::BlkRef {
                rotation: ins.rotation,
                insertion: ins.insert_point,
                scale_factor: Vector3::new(ins.x_scale(), ins.y_scale(), ins.z_scale()),
            },
        )),
        EntityType::Text(t) => Some((
            "ACDB_TEXTOBJECTCONTEXTDATA_CLASS",
            ObjectContextKind::Text {
                horizontal_mode: t.horizontal_alignment as i16,
                rotation: t.rotation,
                insertion: Vector2::new(t.insertion_point.x, t.insertion_point.y),
                alignment: t
                    .alignment_point
                    .map(|p| Vector2::new(p.x, p.y))
                    .unwrap_or(Vector2::new(0.0, 0.0)),
            },
        )),
        EntityType::MText(m) => Some((
            "ACDB_MTEXTOBJECTCONTEXTDATA_CLASS",
            ObjectContextKind::MText(mtext_context_for(m)),
        )),
        EntityType::Dimension(dimension) => {
            let context = dimension_context_for(doc, dimension)?;
            Some((context.subtype.class_name(), ObjectContextKind::Dim(context)))
        }
        EntityType::MultiLeader(mleader) => Some((
            "ACDB_MLEADEROBJECTCONTEXTDATA_CLASS",
            ObjectContextKind::MLeader(mleader.context.clone()),
        )),
        EntityType::AttributeEntity(attribute) => Some((
            "ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS",
            ObjectContextKind::MTextAttribute(attribute_context_for(
                attribute.insertion_point,
                attribute.alignment_point,
                attribute.rotation,
                attribute.horizontal_alignment.to_value(),
                attribute.embedded_mtext.as_deref(),
                scale,
            )),
        )),
        EntityType::AttributeDefinition(attribute) => Some((
            "ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS",
            ObjectContextKind::MTextAttribute(attribute_context_for(
                attribute.insertion_point,
                attribute.alignment_point,
                attribute.rotation,
                attribute.horizontal_alignment.to_value(),
                attribute.embedded_mtext.as_deref(),
                scale,
            )),
        )),
        EntityType::Leader(leader) => Some((
            "ACDB_LEADEROBJECTCONTEXTDATA_CLASS",
            ObjectContextKind::Leader(acadrust::objects::LeaderContext {
                points: leader.vertices.clone(),
                x_direction: leader.horizontal_direction,
                annotation_enabled: !leader.annotation_handle.is_null(),
                insertion_offset: Vector3::ZERO,
                endpoint_projection: leader.annotation_offset,
            }),
        )),
        EntityType::Tolerance(tolerance) => Some((
            "ACDB_FCFOBJECTCONTEXTDATA_CLASS",
            ObjectContextKind::Fcf {
                location: tolerance.insertion_point,
                horizontal_direction: tolerance.direction,
            },
        )),
        EntityType::Hatch(hatch) => Some((
            "ACDB_HATCHSCALECONTEXTDATA_CLASS",
            ObjectContextKind::HatchScale(HatchScaleContext {
                pattern_lines: hatch.pattern.lines.clone(),
                pattern_scale: hatch.pattern_scale,
                pattern_base: Vector3::ZERO,
                loop_types: hatch
                    .paths
                    .iter()
                    .map(|path| path.flags.bits() as i32)
                    .collect(),
                supports_context: true,
            }),
        )),
        _ => None,
    }
}

pub fn supports_annotation_context(entity: &EntityType) -> bool {
    match entity {
        EntityType::Insert(_)
        | EntityType::Text(_)
        | EntityType::MText(_)
        | EntityType::MultiLeader(_)
        | EntityType::AttributeEntity(_)
        | EntityType::AttributeDefinition(_)
        | EntityType::Leader(_)
        | EntityType::Tolerance(_)
        | EntityType::Hatch(_) => true,
        EntityType::Dimension(dimension) => {
            !matches!(dimension, acadrust::entities::Dimension::Arc(_))
        }
        _ => false,
    }
}

fn register_context_class(doc: &mut CadDocument, dxf_name: &str) {
    doc.register_object_context_class(dxf_name);
    if doc.classes.get_by_name(dxf_name).is_some() {
        return;
    }
    let cpp_name = match dxf_name {
        "ACDB_MLEADEROBJECTCONTEXTDATA_CLASS" => "AcDbMLeaderObjectContextData",
        "ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS" => "AcDbMTextAttributeObjectContextData",
        "ACDB_LEADEROBJECTCONTEXTDATA_CLASS" => "AcDbLeaderObjectContextData",
        "ACDB_FCFOBJECTCONTEXTDATA_CLASS" => "AcDbFcfObjectContextData",
        _ => return,
    };
    use acadrust::classes::{DxfClass, ProxyFlags};
    let proxy_flags = ProxyFlags(
        ProxyFlags::ERASE_ALLOWED.0
            | ProxyFlags::CLONING_ALLOWED.0
            | ProxyFlags::DISABLES_PROXY_WARNING_DIALOG.0,
    );
    doc.classes.add_or_update(DxfClass {
        dxf_name: dxf_name.to_string(),
        cpp_class_name: cpp_name.to_string(),
        application_name: "ObjectDBX Classes".to_string(),
        proxy_flags,
        instance_count: 0,
        was_zombie: false,
        is_an_entity: false,
        class_number: 0,
        item_class_id: 0x1F3,
        dwg_version: 0,
        maintenance_version: 0,
        unknown1: 0,
        unknown2: 0,
    });
}

/// Give an entity a per-object annotation context for `scale_handle`,
/// synthesizing the extension-dictionary chain it hangs from when absent:
///
/// ```text
/// entity xdict → "AcDbContextDataManager" → "ACDB_ANNOTATIONSCALES" → "*An" → leaf
/// ```
///
/// The leaf is an [`ObjectContextData`] whose placement is copied from the
/// entity's current geometry and whose `340` handle references `scale_handle`
/// (an `AcDbScale` in `ACAD_SCALELIST`). Idempotent: a leaf for that scale is
/// not duplicated. Exactly one leaf per object is marked `is_default` (the
/// first one created — the native representation). Returns `false` for entity
/// kinds that carry no per-object context (their annotative-ness is style-driven).
pub fn create_annotation_context(
    doc: &mut CadDocument,
    entity_handle: Handle,
    scale_handle: Handle,
) -> bool {
    let Some((class_name, kind)) = doc
        .get_entity(entity_handle)
        .and_then(|entity| context_kind_for(doc, entity, scale_handle))
    else {
        return false;
    };
    // The writer emits a 500+ class number only for registered classes.
    register_context_class(doc, class_name);

    // Extension dictionary (hard-owns its entries; 280 = 1). Create it if the
    // entity has none, and point the entity at it.
    let xdict_h = match doc
        .get_entity(entity_handle)
        .and_then(|e| e.common().xdictionary_handle)
    {
        Some(h) if as_dict(doc, h).is_some() => h,
        _ => {
            let h = doc.allocate_handle();
            let mut d = Dictionary::new();
            d.handle = h;
            d.owner = entity_handle;
            d.hard_owner = true;
            doc.objects.insert(h, ObjectType::Dictionary(d));
            if let Some(e) = doc.get_entity_mut(entity_handle) {
                e.common_mut().xdictionary_handle = Some(h);
            }
            h
        }
    };

    let mgr_h = get_or_create_child_dict(doc, xdict_h, "AcDbContextDataManager");
    let coll_h = get_or_create_child_dict(doc, mgr_h, "ACDB_ANNOTATIONSCALES");

    // Idempotent: if a leaf already applies to this scale, keep it.
    let existing = as_dict(doc, coll_h)
        .map(|d| {
            d.entries.iter().any(|(_, lh)| {
                matches!(
                    doc.objects.get(lh),
                    Some(ObjectType::ObjectContextData(c)) if c.scale == scale_handle
                )
            })
        })
        .unwrap_or(false);
    if existing {
        return true;
    }

    // The first representation created is the default (native) one.
    let is_default = as_dict(doc, coll_h).map(|d| d.entries.is_empty()).unwrap_or(true);
    let n = as_dict(doc, coll_h).map(|d| d.entries.len()).unwrap_or(0) + 1;
    let key = format!("*A{n}");

    let leaf_h = doc.allocate_handle();
    let leaf = ObjectContextData {
        handle: leaf_h,
        owner_handle: coll_h,
        reactors: vec![coll_h],
        xdictionary_handle: None,
        class_version: 3,
        is_default,
        scale: scale_handle,
        kind,
    };
    doc.objects
        .insert(leaf_h, ObjectType::ObjectContextData(leaf));
    if let Some(ObjectType::Dictionary(coll)) = doc.objects.get_mut(&coll_h) {
        coll.add_entry(key, leaf_h);
    }
    true
}

/// True when `common` belongs to an annotative object whose per-object scale
/// contexts are *all* bound to scales other than the current one — an
/// off-scale representation that must stay hidden. A drawing may materialise
/// each annotation scale as its own object (e.g. a block insert per scale on a
/// "0 @ <scale>" layer); only the current scale's representation should draw.
///
/// False for non-annotative objects (no per-object context — the vast
/// majority) and for objects whose contexts include the current scale. Gated
/// on an extension dictionary so non-annotative entities skip the lookup.
pub fn annotative_offscale_for(
    doc: &CadDocument,
    common: &EntityCommon,
    scale_handle: Option<Handle>,
    all_visible: bool,
) -> bool {
    if all_visible {
        return false;
    }
    if !common
        .xdictionary_handle
        .map(|h| !h.is_null())
        .unwrap_or(false)
    {
        return false;
    }
    let scales = object_scale_memberships(doc, common.handle);
    if scales.is_empty() {
        return false;
    }
    match scale_handle {
        Some(handle) => !scales.iter().any(|(_, member)| *member == handle),
        None => !scales.iter().any(|(name, _)| {
            name.eq_ignore_ascii_case(&doc.header.current_annotation_scale)
        }),
    }
}
pub(crate) fn annotation_scale_handles_for_entity(
    doc: &CadDocument,
    entity_handle: Handle,
) -> Vec<Handle> {
    object_scale_memberships(doc, entity_handle)
        .into_iter()
        .map(|(_, scale_handle)| scale_handle)
        .collect()
}
pub fn scale_handle_by_name(doc: &CadDocument, name: &str) -> Option<Handle> {
    doc.objects.iter().find_map(|(handle, object)| match object {
        ObjectType::Scale(scale)
            if !scale.is_temporary && scale.name.eq_ignore_ascii_case(name) =>
        {
            Some(*handle)
        }
        _ => None,
    })
}

pub fn ensure_scale_object(
    doc: &mut CadDocument,
    source: &acadrust::objects::Scale,
) -> Handle {
    if let Some(handle) = scale_handle_by_name(doc, &source.name) {
        return handle;
    }
    let root = root_named_dict_handle(doc);
    let scale_dictionary = as_dict(doc, root)
        .and_then(|dictionary| dictionary.get("ACAD_SCALELIST"))
        .filter(|handle| matches!(doc.objects.get(handle), Some(ObjectType::Dictionary(_))))
        .unwrap_or_else(|| {
            let handle = doc.allocate_handle();
            let mut dictionary = Dictionary::new();
            dictionary.handle = handle;
            dictionary.owner = root;
            doc.objects
                .insert(handle, ObjectType::Dictionary(dictionary));
            if let Some(ObjectType::Dictionary(root_dictionary)) = doc.objects.get_mut(&root) {
                root_dictionary.add_entry("ACAD_SCALELIST", handle);
            }
            handle
        });
    let handle = doc.allocate_handle();
    let mut scale = source.clone();
    scale.handle = handle;
    scale.owner_handle = scale_dictionary;
    scale.is_temporary = false;
    doc.objects.insert(handle, ObjectType::Scale(scale));
    if let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&scale_dictionary) {
        dictionary.add_entry(source.name.clone(), handle);
    }
    handle
}

/// The annotation scales an object currently carries a per-object context for,
/// as `(scale name, scale handle)` pairs (one per representation). Empty when
/// the object has no per-object context chain.
pub fn object_scale_memberships(doc: &CadDocument, entity: Handle) -> Vec<(String, Handle)> {
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

/// Remove the per-object context representation that applies to `scale_handle`,
/// keeping the object's other representations. When that was the object's last
/// representation the whole context chain (and the annotative markers) are torn
/// down via [`clear_annotation_context`] so the object becomes non-annotative.
/// Returns `true` if a representation was removed.
pub fn remove_annotation_context_for_scale(
    doc: &mut CadDocument,
    entity: Handle,
    scale_handle: Handle,
) -> bool {
    let Some(coll_h) = annotation_scales_dict(doc, entity) else {
        return false;
    };
    // Find the leaf that applies to this scale.
    let leaf = as_dict(doc, coll_h).and_then(|c| {
        c.entries.iter().find_map(|(_, lh)| {
            matches!(
                doc.objects.get(lh),
                Some(ObjectType::ObjectContextData(o)) if o.scale == scale_handle
            )
            .then_some(*lh)
        })
    });
    let Some(leaf_h) = leaf else {
        return false;
    };
    // If this is the object's only representation, fully de-annotate it (drop the
    // whole chain AND the native flag, like the Yes→No toggle) so it stops
    // resolving annotative; otherwise drop just this leaf.
    let last = as_dict(doc, coll_h).map(|c| c.entries.len() <= 1).unwrap_or(true);
    if last {
        set_entity_annotative(doc, entity, false);
        return true;
    }
    doc.objects.remove(&leaf_h);
    if let Some(ObjectType::Dictionary(coll)) = doc.objects.get_mut(&coll_h) {
        coll.entries.retain(|(_, h)| *h != leaf_h);
    }
    true
}

/// Resolve an entity's `ACDB_ANNOTATIONSCALES` collection dictionary handle, if
/// its context chain exists.
fn annotation_scales_dict(doc: &CadDocument, entity: Handle) -> Option<Handle> {
    let xd = doc.get_entity(entity).and_then(|e| e.common().xdictionary_handle)?;
    let mgr = as_dict(doc, xd).and_then(|d| d.get("AcDbContextDataManager"))?;
    as_dict(doc, mgr).and_then(|d| d.get("ACDB_ANNOTATIONSCALES"))
}

/// Resolve the representation leaf used by the drawing's current annotation
/// scale. Broken scale handles are ignored. When the current named scale is
/// absent, the leaf explicitly marked as the native/default representation is
/// preferred, followed by the first valid leaf.
pub fn active_object_context_for_scale(
    doc: &CadDocument,
    entity: Handle,
    scale_handle: Option<Handle>,
) -> Option<&ObjectContextData> {
    let coll_h = annotation_scales_dict(doc, entity)?;
    let coll = as_dict(doc, coll_h)?;
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

pub fn effective_annotation_scale_for(
    doc: &CadDocument,
    entity: &EntityType,
    fallback: f32,
    scale_handle: Option<Handle>,
) -> f32 {
    let context_annotative = is_annotative(doc, entity);
    let style_annotative = annotation_style_is_annotative(doc, entity);
    // An annotative object is scaled at the scales it has a representation
    // for. Text is the case where the style alone proves nothing: a text style
    // can be turned annotative long after text was drawn in it, and that text
    // keeps its stored height until it is explicitly converted — which is what
    // `is_annotative` already says by ignoring the style for TEXT. Scaling on
    // the style regardless contradicted it, and a drawing whose text sits on a
    // style someone flagged annotative, with no object contexts anywhere, came
    // out at the whole annotation scale too large.
    //
    // A dimension, leader, tolerance or table is different: there the style is
    // the standard signal and still counts on its own.
    let text_like = matches!(
        entity,
        EntityType::Text(_)
            | EntityType::MText(_)
            | EntityType::AttributeEntity(_)
            | EntityType::AttributeDefinition(_)
    );
    if !context_annotative && (text_like || !style_annotative) {
        return 1.0;
    }
    // Annotative TEXT/MTEXT store their paper text height as the base value.
    // `fallback` is the absolute paper-to-model multiplier for the active
    // annotation scale and already includes the drawing's INSUNITS conversion.
    //
    // Example for a metre drawing with 2 mm paper text:
    //   1:1   -> 2 * 0.001 = 0.002 m
    //   1:100 -> 2 * 0.100 = 0.200 m
    //
    // Using active/native here would only produce 1 / 100 and would lose the
    // millimetre-to-metre conversion entirely.
    if matches!(entity, EntityType::Text(_) | EntityType::MText(_)) {
        return fallback;
    }
    if matches!(
        entity,
        EntityType::Dimension(_)
            | EntityType::Leader(_)
            | EntityType::Tolerance(_)
            | EntityType::Table(_)
    ) {
        return fallback;
    }

    // Unlike MTEXT / DIMENSION contexts, an MLEADER context carries its
    // already-scaled text height and overall scale factor. Keep the text
    // height as stored; make `ml.scale_factor * anno_scale` resolve to the
    // active context's scale factor for arrows, doglegs, and fallback text.
    // MLEADER needs the absolute current annotation multiplier here.
    //
    // Its stored context may belong to another annotation scale. The MLEADER
    // tessellator compares this current multiplier with the context's stored
    // scale_factor and applies only the required correction.
    //
    // When the active context already belongs to this scale the correction is 1,
    // preserving AutoCAD per-scale context geometry unchanged.
    if matches!(entity, EntityType::MultiLeader(_)) {
        return fallback;
    }

    let Some(coll_h) = annotation_scales_dict(doc, entity.common().handle) else {
        return fallback;
    };
    let Some(coll) = as_dict(doc, coll_h) else {
        return fallback;
    };

    let active = active_object_context_for_scale(doc, entity.common().handle, scale_handle);
    let native = coll.entries.iter().find_map(|(_, leaf_h)| {
        match doc.objects.get(leaf_h) {
            Some(ObjectType::ObjectContextData(leaf)) if leaf.is_default => Some(leaf),
            _ => None,
        }
    });
    let (Some(active), Some(native)) = (active, native) else {
        return fallback;
    };
    let (Some(ObjectType::Scale(active_scale)), Some(ObjectType::Scale(native_scale))) = (
        doc.objects.get(&active.scale),
        doc.objects.get(&native.scale),
    ) else {
        return fallback;
    };

    let native_factor = native_scale.inverse_factor();
    if native_factor.abs() <= 1.0e-12 {
        return fallback;
    }
    let relative = active_scale.inverse_factor() / native_factor;
    if relative.is_finite() && relative > 0.0 {
        relative as f32
    } else {
        fallback
    }
}

fn text_horizontal(value: i16) -> acadrust::entities::TextHorizontalAlignment {
    use acadrust::entities::TextHorizontalAlignment;
    match value {
        1 => TextHorizontalAlignment::Center,
        2 => TextHorizontalAlignment::Right,
        3 => TextHorizontalAlignment::Aligned,
        4 => TextHorizontalAlignment::Middle,
        5 => TextHorizontalAlignment::Fit,
        _ => TextHorizontalAlignment::Left,
    }
}

fn mtext_attachment(value: i32) -> acadrust::entities::AttachmentPoint {
    use acadrust::entities::AttachmentPoint;
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

fn apply_mtext_context(entity: &mut acadrust::entities::MText, context: &MTextContext) {
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

fn apply_dimension_context(
    dimension: &mut acadrust::entities::Dimension,
    context: &acadrust::objects::DimContext,
    doc: &CadDocument,
) {
    use acadrust::entities::Dimension;
    use acadrust::objects::DimSubtype;

    {
        let base = dimension.base_mut();
        base.text_middle_point.x = context.def_pt.x;
        base.text_middle_point.y = context.def_pt.y;
        base.text_rotation = context.text_rotation;
        base.text_user_positioned = context.is_def_textloc;
        base.flip_arrow1 = context.flip_arrow1;
        base.flip_arrow2 = context.flip_arrow2;
        if let Some(record) = doc.block_records.iter().find(|r| r.handle == context.block) {
            base.block_name.clone_from(&record.name);
        }
    }

    match (&context.subtype, dimension) {
        (DimSubtype::Aligned { dimline_pt }, Dimension::Aligned(dim)) => {
            dim.definition_point = *dimline_pt;
            dim.base.definition_point = *dimline_pt;
        }
        (DimSubtype::Aligned { dimline_pt }, Dimension::Linear(dim)) => {
            dim.definition_point = *dimline_pt;
            dim.base.definition_point = *dimline_pt;
        }
        (DimSubtype::Angular { arc_pt }, Dimension::Angular2Ln(dim)) => {
            dim.dimension_arc = *arc_pt;
            dim.base.definition_point = *arc_pt;
        }
        (DimSubtype::Angular { arc_pt }, Dimension::Angular3Pt(dim)) => {
            dim.definition_point = *arc_pt;
            dim.base.definition_point = *arc_pt;
        }
        (
            DimSubtype::Diametric {
                first_arc_pt,
                def_pt,
            },
            Dimension::Diameter(dim),
        ) => {
            dim.angle_vertex = *first_arc_pt;
            dim.definition_point = *def_pt;
            dim.base.definition_point = *def_pt;
        }
        (DimSubtype::Radial { first_arc_pt }, Dimension::Radius(dim)) => {
            dim.definition_point = *first_arc_pt;
            dim.base.definition_point = *first_arc_pt;
        }
        (
            DimSubtype::RadialLarge {
                ovr_center,
                jog_point,
            },
            Dimension::LargeRadial(dim),
        ) => {
            dim.override_center = *ovr_center;
            dim.jog_point = *jog_point;
        }
        (
            DimSubtype::Ordinate {
                feature_location_pt,
                leader_endpt,
            },
            Dimension::Ordinate(dim),
        ) => {
            dim.feature_location = *feature_location_pt;
            dim.leader_endpoint = *leader_endpt;
        }
        _ => {}
    }
}

fn apply_attribute_context(
    insertion_point: &mut Vector3,
    alignment_point: &mut Vector3,
    rotation: &mut f64,
    horizontal_alignment: &mut acadrust::entities::HorizontalAlignment,
    embedded_mtext: &mut Option<Box<acadrust::entities::MText>>,
    context: &acadrust::objects::MTextAttributeContext,
) {
    insertion_point.x = context.insertion.x;
    insertion_point.y = context.insertion.y;
    alignment_point.x = context.alignment.x;
    alignment_point.y = context.alignment.y;
    *rotation = context.rotation;
    *horizontal_alignment =
        acadrust::entities::HorizontalAlignment::from_value(context.horizontal_mode);
    if context.enable_context {
        if let (Some(embedded), Some(mtext)) = (&context.context, embedded_mtext.as_mut()) {
            apply_mtext_context(mtext, &embedded.mtext);
        }
    }
}

fn apply_hatch_context(hatch: &mut acadrust::entities::Hatch, context: &HatchScaleContext) {
    hatch.pattern.lines.clone_from(&context.pattern_lines);
    hatch.pattern_scale = context.pattern_scale;
    for line in &mut hatch.pattern.lines {
        line.base_point.x += context.pattern_base.x;
        line.base_point.y += context.pattern_base.y;
    }
    for (path, bits) in hatch.paths.iter_mut().zip(&context.loop_types) {
        path.flags = acadrust::entities::BoundaryPathFlags::from_bits(*bits as u32);
    }
}

pub fn entity_for_annotation_context<'a>(
    doc: &'a CadDocument,
    entity: &'a EntityType,
    scale_handle: Option<Handle>,
) -> Cow<'a, EntityType> {
    let Some(context) =
        active_object_context_for_scale(doc, entity.common().handle, scale_handle)
    else {
        return Cow::Borrowed(entity);
    };
    let mut placed = entity.clone();
    match (&context.kind, &mut placed) {
        (
            ObjectContextKind::BlkRef {
                rotation,
                insertion,
                scale_factor,
            },
            EntityType::Insert(insert),
        ) => {
            insert.rotation = *rotation;
            insert.insert_point = *insertion;
            insert.set_x_scale(scale_factor.x);
            insert.set_y_scale(scale_factor.y);
            insert.set_z_scale(scale_factor.z);
        }
        (
            ObjectContextKind::Text {
                horizontal_mode,
                rotation,
                insertion,
                alignment,
            },
            EntityType::Text(text),
        ) => {
            text.horizontal_alignment = text_horizontal(*horizontal_mode);
            text.rotation = *rotation;
            text.insertion_point.x = insertion.x;
            text.insertion_point.y = insertion.y;
            let point = text.alignment_point.get_or_insert(Vector3::ZERO);
            point.x = alignment.x;
            point.y = alignment.y;
        }
        (ObjectContextKind::MText(value), EntityType::MText(mtext)) => {
            apply_mtext_context(mtext, value);
        }
        (ObjectContextKind::Dim(value), EntityType::Dimension(dimension)) => {
            apply_dimension_context(dimension, value, doc);
        }
        (ObjectContextKind::MLeader(value), EntityType::MultiLeader(mleader)) => {
            mleader.context.clone_from(value);
        }
        (
            ObjectContextKind::MTextAttribute(value),
            EntityType::AttributeEntity(attribute),
        ) => {
            apply_attribute_context(
                &mut attribute.insertion_point,
                &mut attribute.alignment_point,
                &mut attribute.rotation,
                &mut attribute.horizontal_alignment,
                &mut attribute.embedded_mtext,
                value,
            );
        }
        (
            ObjectContextKind::MTextAttribute(value),
            EntityType::AttributeDefinition(attribute),
        ) => {
            apply_attribute_context(
                &mut attribute.insertion_point,
                &mut attribute.alignment_point,
                &mut attribute.rotation,
                &mut attribute.horizontal_alignment,
                &mut attribute.embedded_mtext,
                value,
            );
        }
        (ObjectContextKind::Leader(value), EntityType::Leader(leader)) => {
            leader.vertices = value
                .points
                .iter()
                .map(|point| *point + value.insertion_offset)
                .collect();
            leader.horizontal_direction = value.x_direction;
            leader.annotation_offset = value.endpoint_projection;
        }
        (
            ObjectContextKind::Fcf {
                location,
                horizontal_direction,
            },
            EntityType::Tolerance(tolerance),
        ) => {
            tolerance.insertion_point = *location;
            tolerance.direction = *horizontal_direction;
        }
        (ObjectContextKind::HatchScale(value), EntityType::Hatch(hatch)) => {
            apply_hatch_context(hatch, value);
        }
        (ObjectContextKind::HatchView(value), EntityType::Hatch(hatch)) => {
            apply_hatch_context(hatch, &value.hatch);
            hatch.normal = value.view_normal;
            hatch.pattern_angle += value.view_rotation;
        }
        _ => {}
    }
    Cow::Owned(placed)
}

fn sync_mtext_context(context: &mut MTextContext, entity: &acadrust::entities::MText) {
    context.attachment = entity.attachment_point as i32;
    context.x_axis_dir = entity.dwg_x_direction.unwrap_or_else(|| {
        Vector3::new(entity.rotation.cos(), entity.rotation.sin(), 0.0)
    });
    context.insertion = entity.insertion_point;
    context.rect_width = entity.rectangle_width;
    context.rect_height = entity.rectangle_height.unwrap_or(0.0);
    context.extents_width = entity.extents_width;
    context.extents_height = entity.extents_height;
    context.column_type = entity.column_data.column_type as i32;
    if context.column_type == 0 {
        context.columns = None;
    } else {
        let columns = context
            .columns
            .get_or_insert_with(|| acadrust::objects::MTextColumns {
                num_heights: 0,
                width: 0.0,
                gutter: 0.0,
                auto_height: false,
                flow_reversed: false,
                heights: Vec::new(),
            });
        columns.num_heights = entity.column_data.column_count;
        columns.width = entity.column_data.width;
        columns.gutter = entity.column_data.gutter;
        columns.auto_height = entity.column_data.auto_height;
        columns.flow_reversed = entity.column_data.flow_reversed;
        columns.heights.clone_from(&entity.column_data.heights);
    }
}

fn sync_dimension_context(
    context: &mut acadrust::objects::DimContext,
    dimension: &acadrust::entities::Dimension,
    block_handle: Option<Handle>,
) {
    use acadrust::entities::Dimension;
    use acadrust::objects::DimSubtype;

    let base = dimension.base();
    context.def_pt = Vector2::new(base.text_middle_point.x, base.text_middle_point.y);
    context.is_def_textloc = base.text_user_positioned;
    context.text_rotation = base.text_rotation;
    context.flip_arrow1 = base.flip_arrow1;
    context.flip_arrow2 = base.flip_arrow2;
    if let Some(handle) = block_handle {
        context.block = handle;
    }
    match (&mut context.subtype, dimension) {
        (DimSubtype::Aligned { dimline_pt }, Dimension::Aligned(dim)) => {
            *dimline_pt = dim.definition_point;
        }
        (DimSubtype::Aligned { dimline_pt }, Dimension::Linear(dim)) => {
            *dimline_pt = dim.definition_point;
        }
        (DimSubtype::Angular { arc_pt }, Dimension::Angular2Ln(dim)) => {
            *arc_pt = dim.dimension_arc;
        }
        (DimSubtype::Angular { arc_pt }, Dimension::Angular3Pt(dim)) => {
            *arc_pt = dim.definition_point;
        }
        (
            DimSubtype::Diametric {
                first_arc_pt,
                def_pt,
            },
            Dimension::Diameter(dim),
        ) => {
            *first_arc_pt = dim.angle_vertex;
            *def_pt = dim.definition_point;
        }
        (DimSubtype::Radial { first_arc_pt }, Dimension::Radius(dim)) => {
            *first_arc_pt = dim.definition_point;
        }
        (
            DimSubtype::RadialLarge {
                ovr_center,
                jog_point,
            },
            Dimension::LargeRadial(dim),
        ) => {
            *ovr_center = dim.override_center;
            *jog_point = dim.jog_point;
        }
        (
            DimSubtype::Ordinate {
                feature_location_pt,
                leader_endpt,
            },
            Dimension::Ordinate(dim),
        ) => {
            *feature_location_pt = dim.feature_location;
            *leader_endpt = dim.leader_endpoint;
        }
        _ => {}
    }
}

/// Copy an edited entity's placement back into one per-scale leaf so geometry
/// edits remain attached to the representation displayed by the caller.
pub fn sync_annotation_context_from_entity(
    doc: &mut CadDocument,
    entity_handle: Handle,
    scale_handle: Option<Handle>,
) -> bool {
    let Some(leaf_handle) =
        active_object_context_for_scale(doc, entity_handle, scale_handle).map(|leaf| leaf.handle)
    else {
        return false;
    };
    let Some(entity) = doc.get_entity(entity_handle).cloned() else {
        return false;
    };
    let dim_block_handle = match &entity {
        EntityType::Dimension(dimension) => doc
            .block_records
            .iter()
            .find(|record| record.name.eq_ignore_ascii_case(&dimension.base().block_name))
            .map(|record| record.handle),
        _ => None,
    };
    let Some(ObjectType::ObjectContextData(leaf)) = doc.objects.get_mut(&leaf_handle) else {
        return false;
    };
    match (&entity, &mut leaf.kind) {
        (
            EntityType::Insert(insert),
            ObjectContextKind::BlkRef {
                rotation,
                insertion,
                scale_factor,
            },
        ) => {
            *rotation = insert.rotation;
            *insertion = insert.insert_point;
            *scale_factor =
                Vector3::new(insert.x_scale(), insert.y_scale(), insert.z_scale());
        }
        (
            EntityType::Text(text),
            ObjectContextKind::Text {
                horizontal_mode,
                rotation,
                insertion,
                alignment,
            },
        ) => {
            *horizontal_mode = match text.horizontal_alignment {
                acadrust::entities::TextHorizontalAlignment::Left => 0,
                acadrust::entities::TextHorizontalAlignment::Center => 1,
                acadrust::entities::TextHorizontalAlignment::Right => 2,
                acadrust::entities::TextHorizontalAlignment::Aligned => 3,
                acadrust::entities::TextHorizontalAlignment::Middle => 4,
                acadrust::entities::TextHorizontalAlignment::Fit => 5,
            };
            *rotation = text.rotation;
            *insertion = Vector2::new(text.insertion_point.x, text.insertion_point.y);
            let point = text.alignment_point.unwrap_or(Vector3::ZERO);
            *alignment = Vector2::new(point.x, point.y);
        }
        (EntityType::MText(mtext), ObjectContextKind::MText(context)) => {
            sync_mtext_context(context, mtext);
        }
        (EntityType::Dimension(dimension), ObjectContextKind::Dim(context)) => {
            sync_dimension_context(context, dimension, dim_block_handle);
        }
        (EntityType::MultiLeader(mleader), ObjectContextKind::MLeader(context)) => {
            context.clone_from(&mleader.context);
        }
        (
            EntityType::AttributeEntity(attribute),
            ObjectContextKind::MTextAttribute(context),
        ) => {
            context.horizontal_mode = attribute.horizontal_alignment.to_value();
            context.rotation = attribute.rotation;
            context.insertion =
                Vector2::new(attribute.insertion_point.x, attribute.insertion_point.y);
            context.alignment =
                Vector2::new(attribute.alignment_point.x, attribute.alignment_point.y);
            if let (Some(embedded), Some(mtext)) =
                (&mut context.context, &attribute.embedded_mtext)
            {
                sync_mtext_context(&mut embedded.mtext, mtext);
            }
        }
        (
            EntityType::AttributeDefinition(attribute),
            ObjectContextKind::MTextAttribute(context),
        ) => {
            context.horizontal_mode = attribute.horizontal_alignment.to_value();
            context.rotation = attribute.rotation;
            context.insertion =
                Vector2::new(attribute.insertion_point.x, attribute.insertion_point.y);
            context.alignment =
                Vector2::new(attribute.alignment_point.x, attribute.alignment_point.y);
            if let (Some(embedded), Some(mtext)) =
                (&mut context.context, &attribute.embedded_mtext)
            {
                sync_mtext_context(&mut embedded.mtext, mtext);
            }
        }
        (EntityType::Leader(leader), ObjectContextKind::Leader(context)) => {
            context.points = leader
                .vertices
                .iter()
                .map(|point| *point - context.insertion_offset)
                .collect();
            context.x_direction = leader.horizontal_direction;
            context.endpoint_projection = leader.annotation_offset;
        }
        (
            EntityType::Tolerance(tolerance),
            ObjectContextKind::Fcf {
                location,
                horizontal_direction,
            },
        ) => {
            *location = tolerance.insertion_point;
            *horizontal_direction = tolerance.direction;
        }
        (EntityType::Hatch(hatch), ObjectContextKind::HatchScale(context)) => {
            context.pattern_lines.clone_from(&hatch.pattern.lines);
            for line in &mut context.pattern_lines {
                line.base_point.x -= context.pattern_base.x;
                line.base_point.y -= context.pattern_base.y;
            }
            context.pattern_scale = hatch.pattern_scale;
            context.loop_types = hatch
                .paths
                .iter()
                .map(|path| path.flags.bits() as i32)
                .collect();
        }
        (EntityType::Hatch(hatch), ObjectContextKind::HatchView(context)) => {
            context.hatch.pattern_lines.clone_from(&hatch.pattern.lines);
            for line in &mut context.hatch.pattern_lines {
                line.base_point.x -= context.hatch.pattern_base.x;
                line.base_point.y -= context.hatch.pattern_base.y;
            }
            context.hatch.pattern_scale = hatch.pattern_scale;
            context.hatch.loop_types = hatch
                .paths
                .iter()
                .map(|path| path.flags.bits() as i32)
                .collect();
            context.view_normal = hatch.normal;
        }
        _ => {}
    }
    true
}

/// Move every stored scale representation with a pasted entity. The base
/// entity has already moved when this runs; each context leaf still contains
/// its source placement, so it is materialized, translated, and written back
/// without disturbing the transformed base representation.
pub fn translate_annotation_contexts(
    doc: &mut CadDocument,
    entity_handle: Handle,
    delta: glam::DVec3,
) -> bool {
    let Some(base_entity) = doc.get_entity(entity_handle).cloned() else {
        return false;
    };
    let leaves: Vec<_> = annotation_scales_dict(doc, entity_handle)
        .and_then(|collection| as_dict(doc, collection))
        .map(|collection| {
            collection
                .entries
                .iter()
                .filter_map(|(_, leaf_handle)| match doc.objects.get(leaf_handle) {
                    Some(ObjectType::ObjectContextData(leaf)) => {
                        Some((leaf.handle, leaf.scale))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if leaves.is_empty() {
        return false;
    }

    let mut changed = false;
    for (_, scale) in leaves {
        let mut placed = entity_for_annotation_context(doc, &base_entity, Some(scale)).into_owned();
        crate::scene::view::dispatch::apply_transform(
            &mut placed,
            &crate::command::EntityTransform::Translate(delta),
        );

        // The entity translator keeps the compatibility break list in sync,
        // while the complete per-segment list is a separate persisted field.
        if let EntityType::MultiLeader(mleader) = &mut placed {
            let offset = Vector3::new(delta.x, delta.y, delta.z);
            for root in &mut mleader.context.leader_roots {
                for line in &mut root.lines {
                    for info in &mut line.break_infos {
                        for pair in &mut info.break_points {
                            pair.start_point = pair.start_point + offset;
                            pair.end_point = pair.end_point + offset;
                        }
                    }
                }
            }
        }

        // A pasted dimension owns a newly generated graphics block. A source
        // context can still carry the old block handle, so retain the block
        // selected for the transformed base entity before synchronizing it.
        if let (EntityType::Dimension(placed), EntityType::Dimension(base)) =
            (&mut placed, &base_entity)
        {
            placed.base_mut().block_name.clone_from(&base.base().block_name);
        }

        if let Some(entity) = doc.get_entity_mut(entity_handle) {
            *entity = placed;
        }
        changed |= sync_annotation_context_from_entity(doc, entity_handle, Some(scale));
        if let Some(entity) = doc.get_entity_mut(entity_handle) {
            *entity = base_entity.clone();
        }
    }
    changed
}

/// Get the child dictionary stored under `key` in `parent_h`, creating an empty
/// one (owned by `parent_h`) and registering the entry when absent.
fn get_or_create_child_dict(doc: &mut CadDocument, parent_h: Handle, key: &str) -> Handle {
    if let Some(h) = as_dict(doc, parent_h).and_then(|d| d.get(key)) {
        return h;
    }
    let h = doc.allocate_handle();
    let mut d = Dictionary::new();
    d.handle = h;
    d.owner = parent_h;
    doc.objects.insert(h, ObjectType::Dictionary(d));
    if let Some(ObjectType::Dictionary(p)) = doc.objects.get_mut(&parent_h) {
        p.add_entry(key, h);
    }
    h
}

/// Remove an entity's per-object annotation context — the extension-dictionary
/// `AcDbContextDataManager` → `ACDB_ANNOTATIONSCALES` → per-scale leaf subtree —
/// and the legacy annotative XDATA markers, so [`is_annotative`] no longer fires
/// on it. The shared `SCALE` objects in `ACAD_SCALELIST` are document-level and
/// left intact.
pub fn clear_annotation_context(doc: &mut CadDocument, handle: Handle) {
    if let Some(xdict_h) = doc.get_entity(handle).and_then(|e| e.common().xdictionary_handle) {
        // Collect the manager subtree (manager dict, its scales dict, the leaves)
        // before mutating, then drop them.
        let mut remove = Vec::new();
        if let Some(mgr_h) = as_dict(doc, xdict_h).and_then(|d| d.get("AcDbContextDataManager")) {
            remove.push(mgr_h);
            if let Some(scales_h) =
                as_dict(doc, mgr_h).and_then(|d| d.get("ACDB_ANNOTATIONSCALES"))
            {
                remove.push(scales_h);
                if let Some(scales) = as_dict(doc, scales_h) {
                    for (_, leaf) in &scales.entries {
                        remove.push(*leaf);
                    }
                }
            }
        }
        if let Some(ObjectType::Dictionary(xd)) = doc.objects.get_mut(&xdict_h) {
            xd.entries.retain(|(k, _)| k != "AcDbContextDataManager");
        }
        for h in remove {
            doc.objects.remove(&h);
        }
    }
    // Strip the legacy annotative XDATA markers the detection also honours.
    crate::scene::view::dispatch::set_entity_xdata(doc, handle, "AcadAnnotative", None);
    crate::scene::view::dispatch::set_entity_xdata(doc, handle, "AcAnnoPO", None);
    crate::scene::view::dispatch::set_entity_xdata(doc, handle, "AcAnnotativeData", None);
}

/// Does a style name resolve to `name` (or to "Standard" when `name` is blank)?
fn name_matches(style_name: &str, name: &str) -> bool {
    style_name.eq_ignore_ascii_case(name)
        || (name.trim().is_empty() && style_name.eq_ignore_ascii_case("Standard"))
}

/// Whether `name` currently names an annotative text style.
///
/// Creation paths use this to stamp a new TEXT/MTEXT with its own annotation
/// context. Render-time detection deliberately does not use it: an existing
/// non-annotative object may still reference a style later made annotative.
pub fn text_style_is_annotative(doc: &CadDocument, name: &str) -> bool {
    doc.text_styles
        .iter()
        .find(|s| name_matches(&s.name, name))
        .is_some_and(|s| s.annotative)
}

pub fn dim_style_is_annotative(doc: &CadDocument, name: &str) -> bool {
    doc.dim_styles
        .iter()
        .find(|s| name_matches(&s.name, name))
        .is_some_and(|s| s.annotative)
}

fn mleader_style_annotative(doc: &CadDocument, handle: Option<Handle>) -> bool {
    let Some(h) = handle else {
        return false;
    };
    doc.objects.iter().any(|(oh, o)| {
        matches!(o, ObjectType::MultiLeaderStyle(s) if *oh == h && s.is_annotative)
    })
}

fn table_style_annotative(doc: &CadDocument, handle: Option<Handle>) -> bool {
    let Some(handle) = handle else {
        return false;
    };
    doc.objects.iter().any(|(object_handle, object)| {
        matches!(object, ObjectType::TableStyle(style) if *object_handle == handle && style.annotative)
    })
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
fn has_context_manager(doc: &CadDocument, common: &EntityCommon) -> bool {
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
pub fn mleader_is_annotative(
    doc: &CadDocument,
    mleader: &acadrust::entities::MultiLeader,
) -> bool {
    has_context_manager(doc, &mleader.common)
        || mleader.enable_annotation_scale
}

pub fn annotation_style_is_annotative(doc: &CadDocument, entity: &EntityType) -> bool {
    match entity {
        EntityType::Text(text) => text_style_is_annotative(doc, &text.style),
        EntityType::MText(text) => text_style_is_annotative(doc, &text.style),
        EntityType::AttributeEntity(attribute) => {
            text_style_is_annotative(doc, &attribute.text_style)
        }
        EntityType::AttributeDefinition(attribute) => {
            text_style_is_annotative(doc, &attribute.text_style)
        }
        EntityType::Dimension(dimension) => {
            dim_style_is_annotative(doc, &dimension.base().style_name)
        }
        EntityType::Leader(leader) => dim_style_is_annotative(doc, &leader.dimension_style),
        EntityType::Tolerance(tolerance) => {
            dim_style_is_annotative(doc, &tolerance.dimension_style_name)
        }
        EntityType::MultiLeader(leader) => {
            mleader_style_annotative(doc, leader.style_handle)
        }
        EntityType::Table(table) => table_style_annotative(doc, table.table_style_handle),
        _ => false,
    }
}

pub fn apply_mleader_style(
    entity: &mut acadrust::entities::MultiLeader,
    style: &acadrust::objects::MultiLeaderStyle,
) {
    entity.style_handle = Some(style.handle);
    entity.content_type = (style.content_type as i16).into();
    entity.path_type = (style.path_type as i16).into();
    entity.line_color = style.line_color;
    entity.line_type_handle = style.line_type_handle;
    entity.line_weight = style.line_weight;
    entity.enable_landing = style.enable_landing;
    entity.enable_dogleg = style.enable_dogleg;
    entity.dogleg_length = style.landing_distance;
    entity.arrowhead_handle = style.arrowhead_handle;
    entity.arrowhead_size = style.arrowhead_size;
    entity.context.arrowhead_size = style.arrowhead_size;
    entity.context.landing_gap = style.landing_gap;
    entity.text_style_handle = style.text_style_handle;
    entity.text_color = style.text_color;
    entity.text_frame = style.text_frame;
    entity.text_height = style.text_height;
    entity.context.text_height = style.text_height;
    entity.context.text_style_handle = style.text_style_handle;
    entity.context.text_color = style.text_color;
    entity.text_left_attachment = (style.text_left_attachment as i16).into();
    entity.text_right_attachment = (style.text_right_attachment as i16).into();
    entity.text_top_attachment = (style.text_top_attachment as i16).into();
    entity.text_bottom_attachment = (style.text_bottom_attachment as i16).into();
    entity.text_attachment_direction = (style.text_attachment_direction as i16).into();
    entity.text_alignment = (style.text_alignment as i16).into();
    entity.text_angle_type = (style.text_angle_type as i16).into();
    entity.context.text_left_attachment = entity.text_left_attachment;
    entity.context.text_right_attachment = entity.text_right_attachment;
    entity.context.text_top_attachment = entity.text_top_attachment;
    entity.context.text_bottom_attachment = entity.text_bottom_attachment;
    entity.context.text_alignment = entity.text_alignment;
    entity.block_content_handle = style.block_content_handle;
    entity.block_content_color = style.block_content_color;
    entity.block_connection_type = (style.block_content_connection as i16).into();
    entity.block_rotation = style.block_content_rotation;
    entity.block_scale = Vector3::new(
        style.block_content_scale_x,
        style.block_content_scale_y,
        style.block_content_scale_z,
    );
    entity.scale_factor = style.scale_factor;
    entity.context.block_content_handle = style.block_content_handle;
    entity.context.block_content_color = style.block_content_color;
    entity.context.block_connection_type = entity.block_connection_type;
    entity.context.block_rotation = style.block_content_rotation;
    entity.context.block_content_scale = entity.block_scale;
    entity.context.scale_factor = style.scale_factor;
    entity.enable_annotation_scale = style.is_annotative;
    for root in &mut entity.context.leader_roots {
        root.landing_distance = style.landing_distance;
        root.text_attachment_direction = entity.text_attachment_direction;
        for line in &mut root.lines {
            if line.override_flags.is_empty() {
                line.path_type = entity.path_type;
                line.line_color = style.line_color;
                line.line_type_handle = style.line_type_handle;
                line.line_weight = style.line_weight;
                line.arrowhead_handle = style.arrowhead_handle;
                line.arrowhead_size = style.arrowhead_size;
            }
        }
    }
}
fn apply_mleader_style_at_display_scale(
    entity: &mut acadrust::entities::MultiLeader,
    style: &acadrust::objects::MultiLeaderStyle,
    display_scale: f64,
) {
    apply_mleader_style(entity, style);

    let scale = if display_scale.is_finite()
        && display_scale > 1.0e-12
    {
        display_scale
    } else {
        1.0
    };

    // Estos valores del estilo son tamaños de papel.
    // El contexto MLEADER guarda la representación ya
    // escalada para la escala anotativa/modelo activa.
    entity.context.scale_factor = scale;

    entity.context.text_height =
        style.text_height * scale;

    entity.context.arrowhead_size =
        style.arrowhead_size * scale;

    entity.context.landing_gap =
        style.landing_gap * scale;

    for root in &mut entity.context.leader_roots {
        root.landing_distance =
            style.landing_distance * scale;

        root.text_attachment_direction =
            entity.text_attachment_direction;
    }
}

pub fn apply_mleader_style_to_object(
    doc: &mut CadDocument,
    handle: Handle,
    style: &acadrust::objects::MultiLeaderStyle,
) -> bool {
    let Some(EntityType::MultiLeader(original)) =
        doc.get_entity(handle).cloned()
    else {
        return false;
    };

    // Escala visual de la representación base.
    //
    // En una MLEADER anotativa context.scale_factor
    // contiene la escala real de esa representación
    // (por ejemplo 50 para 1:50).
    //
    // En una no anotativa manda el scale_factor del estilo.
    let base_display_scale = if style.is_annotative {
        let scale = original.context.scale_factor;

        if scale.is_finite() && scale > 1.0e-12 {
            scale
        } else {
            1.0
        }
    } else {
        let scale = style.scale_factor;

        if scale.is_finite() && scale > 1.0e-12 {
            scale
        } else {
            1.0
        }
    };

    let mut styled = original.clone();

    apply_mleader_style_at_display_scale(
        &mut styled,
        style,
        base_display_scale,
    );

    if let Some(EntityType::MultiLeader(entity)) =
        doc.get_entity_mut(handle)
    {
        *entity = styled;
    }

    // Actualizar también TODAS las representaciones
    // anotativas almacenadas de la misma MLEADER.
    let leaf_handles: Vec<_> =
        annotation_scales_dict(doc, handle)
            .and_then(|collection| as_dict(doc, collection))
            .map(|collection| {
                collection
                    .entries
                    .iter()
                    .map(|(_, leaf)| *leaf)
                    .collect()
            })
            .unwrap_or_default();

    for leaf_handle in leaf_handles {
        // Primero clonamos los datos necesarios para
        // evitar mantener un borrow mutable sobre doc.
        let context_before =
            match doc.objects.get(&leaf_handle) {
                Some(
                    ObjectType::ObjectContextData(leaf)
                ) => {
                    let ObjectContextKind::MLeader(context) =
                        &leaf.kind
                    else {
                        continue;
                    };

                    context.clone()
                }

                _ => continue,
            };

        let context_scale =
            if context_before.scale_factor.is_finite()
                && context_before.scale_factor > 1.0e-12
            {
                context_before.scale_factor
            } else {
                base_display_scale
            };

        let mut per_scale = original.clone();

        per_scale.context =
            context_before;

        apply_mleader_style_at_display_scale(
            &mut per_scale,
            style,
            context_scale,
        );

        if let Some(
            ObjectType::ObjectContextData(leaf)
        ) = doc.objects.get_mut(&leaf_handle)
        {
            if let ObjectContextKind::MLeader(context) =
                &mut leaf.kind
            {
                context.clone_from(
                    &per_scale.context,
                );
            }
        }
    }

    true
}

pub fn update_entity_from_annotation_style(
    doc: &mut CadDocument,
    handle: Handle,
    current_scale: Option<Handle>,
) -> bool {
    enum StyleUpdate {
        Text { annotative: bool, height: f64 },
        Dimension { annotative: bool },
        MultiLeader(acadrust::objects::MultiLeaderStyle),
        ContextOnly,
    }

    let Some(entity) = doc.get_entity(handle) else {
        return false;
    };
    let update = match entity {
        EntityType::Text(text) => doc.text_styles.get(&text.style).map(|style| {
            StyleUpdate::Text {
                annotative: style.annotative,
                height: style.height,
            }
        }),
        EntityType::MText(text) => doc.text_styles.get(&text.style).map(|style| {
            StyleUpdate::Text {
                annotative: style.annotative,
                height: style.height,
            }
        }),
        EntityType::AttributeEntity(attribute) => doc
            .text_styles
            .get(&attribute.text_style)
            .map(|style| StyleUpdate::Text {
                annotative: style.annotative,
                height: style.height,
            }),
        EntityType::AttributeDefinition(attribute) => doc
            .text_styles
            .get(&attribute.text_style)
            .map(|style| StyleUpdate::Text {
                annotative: style.annotative,
                height: style.height,
            }),
        EntityType::Dimension(dimension) => doc
            .dim_styles
            .get(&dimension.base().style_name)
            .map(|style| StyleUpdate::Dimension {
                annotative: style.annotative,
            }),
        EntityType::Leader(leader) => doc
            .dim_styles
            .get(&leader.dimension_style)
            .map(|style| StyleUpdate::Dimension {
                annotative: style.annotative,
            }),
        EntityType::Tolerance(tolerance) => doc
            .dim_styles
            .get(&tolerance.dimension_style_name)
            .map(|style| StyleUpdate::Dimension {
                annotative: style.annotative,
            }),
        EntityType::MultiLeader(leader) => leader.style_handle.and_then(|style_handle| {
            match doc.objects.get(&style_handle) {
                Some(ObjectType::MultiLeaderStyle(style)) => {
                    Some(StyleUpdate::MultiLeader(style.clone()))
                }
                _ => None,
            }
        }),
        _ if is_annotative(doc, entity) => Some(StyleUpdate::ContextOnly),
        _ => None,
    };
    let Some(update) = update else {
        return false;
    };

    let annotative = match update {
        StyleUpdate::Text { annotative, height } => {
            if height > 0.0 {
                if let Some(entity) = doc.get_entity_mut(handle) {
                    match entity {
                        EntityType::Text(text) => text.height = height,
                        EntityType::MText(text) => text.height = height,
                        EntityType::AttributeEntity(attribute) => attribute.height = height,
                        EntityType::AttributeDefinition(attribute) => attribute.height = height,
                        _ => {}
                    }
                }
            }
            annotative
        }
        StyleUpdate::Dimension { annotative } => annotative,
        StyleUpdate::MultiLeader(style) => {
            apply_mleader_style_to_object(doc, handle, &style);
            style.is_annotative
        }
        StyleUpdate::ContextOnly => return true,
    };

    set_entity_annotative(doc, handle, annotative);
    if annotative {
        if let Some(scale) = current_scale {
            create_annotation_context(doc, handle, scale);
        }
    }
    true
}

/// Whether an entity participates in annotation scaling.
pub fn is_annotative(doc: &CadDocument, entity: &EntityType) -> bool {
    // Per-object annotation context (works regardless of style).
    if has_context_manager(doc, entity.common()) {
        return true;
    }
    // Legacy annotative XDATA markers.
    let xd = &entity.common().extended_data;
    let standard_marker = xd
        .get_record("AcadAnnotative")
        .and_then(|record| {
            record.values.iter().filter_map(|value| match value {
                acadrust::xdata::XDataValue::Integer16(value) => Some(*value),
                _ => None,
            }).last()
        })
        .is_some_and(|value| value != 0);
    if standard_marker
        || xd.get_record("AcAnnoPO").is_some()
        || xd.get_record("AcAnnotativeData").is_some()
    {
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


