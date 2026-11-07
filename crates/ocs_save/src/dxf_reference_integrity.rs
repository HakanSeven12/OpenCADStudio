//! Reference-integrity pruning for acknowledged lossy DXF conversions.
//!
//! When a DWG contains unsupported raw objects or entities with no DXF
//! representation (such as `DIMCONTAINER2` stored as `ObjectType::Unknown` with
//! `raw_dxf_codes: None`), exporting to DXF omits those records. Any child
//! objects or entities owned by the omitted records, as well as dictionary,
//! reactor, extension-dictionary, collection, and proxy/XRecord handle slots
//! pointing to them, must be pruned or nulled on the DXF save copy so the
//! emitted DXF handle graph remains structurally valid without hiding broken
//! essential references on surviving native objects.

use std::collections::HashSet;

use document::{
    entities::{EntityCommon, EntityType, ExtendedEntityData, SurfaceData},
    objects::{
        ClassObjectData, DataObjectData, ObjectContextKind, ObjectType, SemanticPropertyValue,
        XRecordValue,
    },
    xdata::XDataValue,
    Document, Handle,
};

fn is_unrestorable_assoc_object(dxf_name: &str) -> bool {
    // Match the pinned codec writer: typed constraint dependencies are supported.
    let normalized: String = dxf_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    normalized.starts_with("ACDBDYNAMICBLOCKPURGEPREVENTER")
}

fn find_root_dict_handle(doc: &Document) -> Handle {
    let header_root = doc.header.named_objects_dict_handle;
    if !header_root.is_null()
        && matches!(
            doc.objects.get(&header_root),
            Some(ObjectType::Dictionary(_))
        )
    {
        return header_root;
    }
    doc.objects
        .iter()
        .find_map(|(handle, obj)| match obj {
            ObjectType::Dictionary(dict) if dict.owner.is_null() => Some(*handle),
            _ => None,
        })
        .unwrap_or(Handle::NULL)
}

fn dxf_skips_entity(entity: &EntityType) -> bool {
    match entity {
        EntityType::Unknown(u) => {
            u.raw_dxf_codes
                .as_ref()
                .is_none_or(|codes| codes.is_empty() || u.dxf_name.is_empty())
                && u.raw_dwg_data.is_none()
                && u.common
                    .graphic_data
                    .as_deref()
                    .unwrap_or_default()
                    .is_empty()
        }
        EntityType::Extended(ext) => matches!(
            &ext.data,
            ExtendedEntityData::Arch(arch)
                if arch
                    .raw_dxf_codes
                    .as_ref()
                    .is_none_or(|codes| codes.is_empty() || arch.dxf_name.is_empty())
                    && ext
                        .common
                        .graphic_data
                        .as_deref()
                        .unwrap_or_default()
                        .is_empty()
        ),
        _ => false,
    }
}

fn dxf_skips_object(object: &ObjectType, doc: &Document) -> bool {
    match object {
        ObjectType::Unknown {
            type_name,
            raw_dxf_codes,
            ..
        } => {
            is_unrestorable_assoc_object(type_name)
                || raw_dxf_codes
                    .as_ref()
                    .is_none_or(|codes| codes.is_empty() || type_name.is_empty())
        }
        ObjectType::Arch(arch) => arch
            .raw_dxf_codes
            .as_ref()
            .is_none_or(|codes| codes.is_empty() || arch.dxf_name.is_empty()),
        ObjectType::DynamicBlock(obj) => is_unrestorable_assoc_object(&obj.dxf_name),
        ObjectType::Associative(obj) => is_unrestorable_assoc_object(&obj.dxf_name),
        ObjectType::DataObject(obj) => matches!(
            &obj.data,
            DataObjectData::BreakData(b) if b.dimension_reference.is_null()
        ),
        ObjectType::ImageDefinitionReactor(reactor) => {
            doc.get_entity(reactor.image_handle).is_none()
                && !doc.objects.contains_key(&reactor.image_handle)
        }
        _ => false,
    }
}

fn object_owner(object: &ObjectType) -> Handle {
    match object {
        ObjectType::Dictionary(v) => v.owner,
        ObjectType::Layout(v) => v.owner,
        ObjectType::XRecord(v) => v.owner,
        ObjectType::Group(v) => v.owner,
        ObjectType::MLineStyle(v) => v.owner,
        ObjectType::ImageDefinition(v) => v.owner,
        ObjectType::UnderlayDefinition(v) => v.owner_handle,
        ObjectType::PlotSettings(v) => v.owner,
        ObjectType::MultiLeaderStyle(v) => v.owner_handle,
        ObjectType::TableStyle(v) => v.owner_handle,
        ObjectType::TableContent(v) | ObjectType::TableTemplate(v) => v.common.owner_handle,
        ObjectType::Scale(v) => v.owner_handle,
        ObjectType::ObjectContextData(v) => v.owner_handle,
        ObjectType::SortEntitiesTable(v) => v.owner_handle,
        ObjectType::DictionaryVariable(v) => v.owner_handle,
        ObjectType::VisualStyle(v) => v.owner,
        ObjectType::Material(v) => v.owner,
        ObjectType::ImageDefinitionReactor(v) => v.owner,
        ObjectType::GeoData(v) => v.owner,
        ObjectType::SpatialFilter(v) => v.owner,
        ObjectType::RasterVariables(v) => v.owner,
        ObjectType::BookColor(v) => v.owner,
        ObjectType::PlaceHolder(v) => v.owner,
        ObjectType::DictionaryWithDefault(v) => v.owner,
        ObjectType::WipeoutVariables(v) => v.owner,
        ObjectType::BlockVisibilityParameter(v) => v.owner,
        ObjectType::DynamicBlock(v) => v.owner,
        ObjectType::Associative(v) => v.owner,
        ObjectType::ClassObject(v) => v.owner,
        ObjectType::DataObject(v) => v.owner,
        ObjectType::Field(v) => v.owner,
        ObjectType::FieldList(v) => v.owner,
        ObjectType::RegisteredClass(v) => v.owner,
        ObjectType::DgnLineStyle(v) => v.owner,
        ObjectType::ProxyObject(v) => v.owner,
        ObjectType::Arch(v) => v.owner,
        ObjectType::Unknown { owner, .. } => *owner,
    }
}

fn collect_omitted_dxf_handles(doc: &Document) -> (HashSet<Handle>, HashSet<Handle>) {
    let root_dict_handle = find_root_dict_handle(doc);
    let mut non_xref_block_handles = HashSet::new();
    let mut emitted_entity_handles = HashSet::new();
    let mut loft_handles = HashSet::new();

    for br in doc.block_records.iter() {
        if br.flags.is_xref || br.flags.is_xref_overlay {
            continue;
        }
        non_xref_block_handles.insert(br.handle);
        if !br.block_entity_handle.is_null() {
            emitted_entity_handles.insert(br.block_entity_handle);
        }
        if !br.block_end_handle.is_null() {
            emitted_entity_handles.insert(br.block_end_handle);
        }
        for &eh in &br.entity_handles {
            let Some(entity) = doc.get_entity(eh) else {
                continue;
            };
            if dxf_skips_entity(entity) {
                continue;
            }
            emitted_entity_handles.insert(eh);
            if let EntityType::Insert(ins) = entity {
                for attr in &ins.attributes {
                    if !attr.common.handle.is_null() {
                        emitted_entity_handles.insert(attr.common.handle);
                    }
                }
            }
            if let EntityType::Surface(surf) = entity {
                if let SurfaceData::Lofted {
                    cross_sections,
                    guide_curves,
                    path_curve,
                    ..
                } = &surf.surface_data
                {
                    for &lh in cross_sections
                        .iter()
                        .chain(guide_curves)
                        .chain(path_curve.iter())
                    {
                        if !lh.is_null() {
                            loft_handles.insert(lh);
                        }
                    }
                }
            }
        }
    }

    for entity in doc.entities() {
        if matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_))
            && non_xref_block_handles.contains(&entity.common().owner_handle)
        {
            emitted_entity_handles.insert(entity.common().handle);
        }
    }

    let mut omitted_handles = HashSet::new();
    for (handle, obj) in &doc.objects {
        if *handle != root_dict_handle && dxf_skips_object(obj, doc) {
            omitted_handles.insert(*handle);
        }
    }
    for entity in doc.entities() {
        let handle = entity.common().handle;
        if !handle.is_null() && !emitted_entity_handles.contains(&handle) {
            omitted_handles.insert(handle);
        }
    }

    if omitted_handles.is_empty() {
        return (omitted_handles, loft_handles);
    }

    loop {
        let mut changed = false;
        for (handle, obj) in &doc.objects {
            if *handle == root_dict_handle || omitted_handles.contains(handle) {
                continue;
            }
            let owner = object_owner(obj);
            let should_omit = (!owner.is_null() && omitted_handles.contains(&owner))
                || match obj {
                    ObjectType::Dictionary(dict) => {
                        dict.owner != root_dict_handle
                            && !dict.owner.is_null()
                            && !dict.entries.is_empty()
                            && dict
                                .entries
                                .iter()
                                .all(|(_, target)| omitted_handles.contains(target))
                    }
                    ObjectType::DictionaryWithDefault(dict) => {
                        dict.owner != root_dict_handle
                            && !dict.owner.is_null()
                            && !dict.entries.is_empty()
                            && dict
                                .entries
                                .iter()
                                .all(|(_, target)| omitted_handles.contains(target))
                    }
                    ObjectType::ImageDefinitionReactor(reactor) => {
                        omitted_handles.contains(&reactor.image_handle)
                    }
                    ObjectType::DataObject(data) => matches!(
                        &data.data,
                        DataObjectData::BreakData(b)
                            if b.dimension_reference.is_null()
                                || omitted_handles.contains(&b.dimension_reference)
                    ),
                    _ => false,
                };
            if should_omit {
                omitted_handles.insert(*handle);
                changed = true;
            }
        }
        for entity in doc.entities() {
            let handle = entity.common().handle;
            if omitted_handles.contains(&handle) {
                continue;
            }
            let owner = entity.common().owner_handle;
            if !owner.is_null() && omitted_handles.contains(&owner) {
                omitted_handles.insert(handle);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    (omitted_handles, loft_handles)
}

/// Returns `true` if `doc` contains any objects or entities that will be
/// omitted on DXF export and require reference-integrity pruning.
pub fn has_omitted_dxf_records(doc: &Document) -> bool {
    !collect_omitted_dxf_handles(doc).0.is_empty()
}

fn clean_raw_dxf_handle_codes(
    codes: &mut [(i32, String)],
    omitted_handles: &HashSet<Handle>,
) -> bool {
    let mut modified = false;
    for (code, value) in codes.iter_mut() {
        if matches!(*code, 330..=369 | 390..=399 | 1005) {
            if let Ok(raw) = u64::from_str_radix(value.trim(), 16) {
                if raw != 0 && omitted_handles.contains(&Handle::new(raw)) {
                    *value = "0".to_string();
                    modified = true;
                }
            }
        }
    }
    modified
}

fn sanitize_extended_data(
    xdata: &mut document::xdata::ExtendedData,
    omitted_handles: &HashSet<Handle>,
) {
    let mut records = xdata.records().to_vec();
    let mut changed = false;
    for record in &mut records {
        for value in &mut record.values {
            if let XDataValue::Handle(h) = value {
                if omitted_handles.contains(h) {
                    *h = Handle::NULL;
                    changed = true;
                }
            }
        }
    }
    if changed {
        xdata.clear();
        for record in records {
            xdata.add_record(record);
        }
    }
}

fn clean_entity_common(common: &mut EntityCommon, omitted_handles: &HashSet<Handle>) {
    common.reactors.retain(|h| !omitted_handles.contains(h));
    if common
        .xdictionary_handle
        .is_some_and(|h| omitted_handles.contains(&h))
    {
        common.xdictionary_handle = None;
    }
    sanitize_extended_data(&mut common.extended_data, omitted_handles);
    if common
        .color_book_handle
        .is_some_and(|h| omitted_handles.contains(&h))
    {
        common.color_book_handle = None;
    }
    if common
        .full_visual_style_handle
        .is_some_and(|h| omitted_handles.contains(&h))
    {
        common.full_visual_style_handle = None;
    }
    if common
        .face_visual_style_handle
        .is_some_and(|h| omitted_handles.contains(&h))
    {
        common.face_visual_style_handle = None;
    }
    if common
        .edge_visual_style_handle
        .is_some_and(|h| omitted_handles.contains(&h))
    {
        common.edge_visual_style_handle = None;
    }
    if common
        .material_handle
        .is_some_and(|h| omitted_handles.contains(&h))
    {
        common.material_handle = None;
    }
    if common
        .plotstyle_handle
        .is_some_and(|h| omitted_handles.contains(&h))
    {
        common.plotstyle_handle = None;
    }
}

/// Prunes records omitted by DXF export (and any child objects/entities
/// transitively owned by them) from a save copy of `doc`, and cleans
/// non-essential references to those omitted handles so the resulting DXF
/// handle graph has no dangling references or orphaned objects.
///
/// Essential references inside surviving native graphs (such as dynamic block
/// evaluation expressions or representation block pointers pointing to
/// non-existent handles) are intentionally left untouched so structural
/// verification continues to reject damaged native graphs.
pub fn prune_omitted_dxf_records(doc: &mut Document) -> bool {
    let (omitted_handles, loft_handles) = collect_omitted_dxf_handles(doc);
    if omitted_handles.is_empty() {
        return false;
    }

    for handle in &omitted_handles {
        doc.objects.remove(handle);
        doc.object_xdata.remove(handle);
        doc.context_scales.remove(handle);
        doc.block_visibility_params.remove(handle);
        if !loft_handles.contains(handle) {
            let _ = doc.remove_entity_arc(*handle);
        }
    }

    for xdata in doc.object_xdata.values_mut() {
        sanitize_extended_data(xdata, &omitted_handles);
    }

    for br in doc.block_records.iter_mut() {
        br.entity_handles.retain(|h| !omitted_handles.contains(h));
        br.insert_handles.retain(|h| !omitted_handles.contains(h));
    }

    for layer in doc.layers.iter_mut() {
        if omitted_handles.contains(&layer.material) {
            layer.material = Handle::NULL;
        }
        if omitted_handles.contains(&layer.plotstyle_handle) {
            layer.plotstyle_handle = Handle::NULL;
        }
    }

    let surviving_entity_handles: Vec<Handle> = doc.entities().map(|e| e.common().handle).collect();
    for handle in surviving_entity_handles {
        let Some(entity) = doc.get_entity_mut(handle) else {
            continue;
        };
        clean_entity_common(entity.common_mut(), &omitted_handles);
        match entity {
            EntityType::Insert(ins) => {
                for attr in &mut ins.attributes {
                    clean_entity_common(&mut attr.common, &omitted_handles);
                }
            }
            EntityType::Leader(leader) => {
                if omitted_handles.contains(&leader.annotation_handle) {
                    leader.annotation_handle = Handle::NULL;
                }
            }
            EntityType::Hatch(hatch) => {
                for path in &mut hatch.paths {
                    path.boundary_handles
                        .retain(|h| !omitted_handles.contains(h));
                }
            }
            EntityType::Viewport(vp) => {
                vp.frozen_layers.retain(|h| !omitted_handles.contains(h));
                if omitted_handles.contains(&vp.clip_boundary_handle) {
                    vp.clip_boundary_handle = Handle::NULL;
                }
                if omitted_handles.contains(&vp.visual_style_handle) {
                    vp.visual_style_handle = Handle::NULL;
                }
                if omitted_handles.contains(&vp.sun_handle) {
                    vp.sun_handle = Handle::NULL;
                }
            }
            EntityType::Extended(ext) => match &mut ext.data {
                ExtendedEntityData::Proxy(proxy) => {
                    for reference in &mut proxy.object_ids {
                        if omitted_handles.contains(&reference.handle) {
                            reference.handle = Handle::NULL;
                        }
                    }
                }
                ExtendedEntityData::RegisteredClass(reg) => {
                    for prop in &mut reg.properties {
                        if let SemanticPropertyValue::Handle(h) = &mut prop.value {
                            if omitted_handles.contains(h) {
                                *h = Handle::NULL;
                            }
                        }
                    }
                    for reference in &mut reg.object_ids {
                        if omitted_handles.contains(&reference.handle) {
                            reference.handle = Handle::NULL;
                        }
                    }
                }
                _ => {}
            },
            EntityType::Unknown(unknown) => {
                if let Some(codes) = &mut unknown.raw_dxf_codes {
                    clean_raw_dxf_handle_codes(codes, &omitted_handles);
                }
            }
            _ => {}
        }
    }

    for object in doc.objects.values_mut() {
        match object {
            ObjectType::Dictionary(dict) => {
                dict.reactors.retain(|h| !omitted_handles.contains(h));
                if dict
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    dict.xdictionary_handle = None;
                }
                let removed_keys: Vec<String> = dict
                    .entries
                    .iter()
                    .filter(|(_, target)| omitted_handles.contains(target))
                    .map(|(key, _)| key.clone())
                    .collect();
                if !removed_keys.is_empty() {
                    dict.entries
                        .retain(|(_, target)| !omitted_handles.contains(target));
                    dict.hard_owner_entries.retain(|key| {
                        !removed_keys
                            .iter()
                            .any(|removed| removed.eq_ignore_ascii_case(key))
                    });
                }
            }
            ObjectType::DictionaryWithDefault(dict) => {
                dict.entries
                    .retain(|(_, target)| !omitted_handles.contains(target));
                if omitted_handles.contains(&dict.default_handle) {
                    dict.default_handle = Handle::NULL;
                }
            }
            ObjectType::XRecord(xrecord) => {
                let mut modified = false;
                let before_reactors = xrecord.reactors.len();
                xrecord.reactors.retain(|h| !omitted_handles.contains(h));
                if xrecord.reactors.len() != before_reactors {
                    modified = true;
                }
                if xrecord
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    xrecord.xdictionary_handle = None;
                    modified = true;
                }
                for entry in &mut xrecord.entries {
                    if let XRecordValue::Handle(h) = &mut entry.value {
                        if omitted_handles.contains(h) {
                            *h = Handle::NULL;
                            modified = true;
                        }
                    }
                }
                for reference in &mut xrecord.object_references {
                    if omitted_handles.contains(&reference.handle) {
                        reference.handle = Handle::NULL;
                        modified = true;
                    }
                }
                if modified {
                    xrecord.raw_dwg_data = None;
                    xrecord.raw_dwg_version = None;
                }
            }
            ObjectType::Group(group) => {
                group.entities.retain(|h| !omitted_handles.contains(h));
            }
            ObjectType::SortEntitiesTable(table) => {
                let to_remove: Vec<Handle> = table
                    .entries()
                    .filter_map(|entry| {
                        omitted_handles
                            .contains(&entry.entity_handle)
                            .then_some(entry.entity_handle)
                    })
                    .collect();
                if !to_remove.is_empty() {
                    for eh in to_remove {
                        table.remove_entry(eh);
                    }
                    table.raw_dxf_codes = None;
                    table.raw_dxf_version = None;
                }
            }
            ObjectType::RegisteredClass(reg) => {
                let mut modified = false;
                let before_reactors = reg.reactors.len();
                reg.reactors.retain(|h| !omitted_handles.contains(h));
                if reg.reactors.len() != before_reactors {
                    modified = true;
                }
                if reg
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    reg.xdictionary_handle = None;
                    modified = true;
                }
                for prop in &mut reg.properties {
                    if let SemanticPropertyValue::Handle(h) = &mut prop.value {
                        if omitted_handles.contains(h) {
                            *h = Handle::NULL;
                            modified = true;
                        }
                    }
                }
                for reference in &mut reg.object_ids {
                    if omitted_handles.contains(&reference.handle) {
                        reference.handle = Handle::NULL;
                        modified = true;
                    }
                }
                if modified {
                    reg.raw_dwg_data = None;
                    reg.raw_dwg_version = None;
                }
            }
            ObjectType::ProxyObject(proxy) => {
                proxy.reactors.retain(|h| !omitted_handles.contains(h));
                if proxy
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    proxy.xdictionary_handle = None;
                }
                for reference in &mut proxy.object_ids {
                    if omitted_handles.contains(&reference.handle) {
                        reference.handle = Handle::NULL;
                    }
                }
                if !proxy.dxf_data.is_empty() {
                    clean_raw_dxf_handle_codes(&mut proxy.dxf_data, &omitted_handles);
                }
            }
            ObjectType::DataObject(data) => {
                data.reactors.retain(|h| !omitted_handles.contains(h));
                if data
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    data.xdictionary_handle = None;
                }
                match &mut data.data {
                    DataObjectData::IdBuffer(buf) => {
                        buf.object_ids.retain(|h| !omitted_handles.contains(h));
                    }
                    DataObjectData::LayerIndex(idx) => {
                        idx.entries
                            .retain(|e| !omitted_handles.contains(&e.id_buffer));
                    }
                    DataObjectData::TableGeometry(geom) => {
                        for cell in &mut geom.cells {
                            if omitted_handles.contains(&cell.table_geometry) {
                                cell.table_geometry = Handle::NULL;
                            }
                        }
                    }
                    _ => {}
                }
            }
            ObjectType::ClassObject(cls) => {
                cls.reactors.retain(|h| !omitted_handles.contains(h));
                if cls
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    cls.xdictionary_handle = None;
                }
                match &mut cls.data {
                    ClassObjectData::ObjectDataDictionary(dict) => {
                        dict.entries.retain(|(_, h)| !omitted_handles.contains(h));
                    }
                    ClassObjectData::SpatialIndex(idx) => {
                        idx.indexed_objects.retain(|h| !omitted_handles.contains(h));
                    }
                    ClassObjectData::PartialViewingIndex(idx) => {
                        idx.entries.retain(|e| !omitted_handles.contains(&e.object));
                    }
                    ClassObjectData::SectionManager(mgr) => {
                        mgr.sections.retain(|h| !omitted_handles.contains(h));
                    }
                    ClassObjectData::SectionSettings(settings) => {
                        for item in &mut settings.types {
                            item.sources.retain(|h| !omitted_handles.contains(h));
                            if omitted_handles.contains(&item.destination_block) {
                                item.destination_block = Handle::NULL;
                            }
                        }
                    }
                    ClassObjectData::LightList(list) => {
                        list.lights.retain(|e| !omitted_handles.contains(&e.handle));
                    }
                    ClassObjectData::ContextDataManager(mgr) => {
                        if omitted_handles.contains(&mgr.object_context) {
                            mgr.object_context = Handle::NULL;
                        }
                        for sub in &mut mgr.sub_managers {
                            if omitted_handles.contains(&sub.handle) {
                                sub.handle = Handle::NULL;
                            }
                            sub.entries.retain(|e| !omitted_handles.contains(&e.item));
                        }
                    }
                    ClassObjectData::ViewRepModelSpaceViewSelectionSet(sel) => {
                        sel.entities.retain(|h| !omitted_handles.contains(h));
                    }
                    ClassObjectData::DataTable(table) => {
                        for col in &mut table.columns {
                            for row in &mut col.rows {
                                if omitted_handles.contains(&row.handle) {
                                    row.handle = Handle::NULL;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            ObjectType::Layout(layout) => {
                layout.reactors.retain(|h| !omitted_handles.contains(h));
                if layout
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    layout.xdictionary_handle = None;
                }
                layout.viewports.retain(|h| !omitted_handles.contains(h));
                if omitted_handles.contains(&layout.viewport) {
                    layout.viewport = Handle::NULL;
                }
                if omitted_handles.contains(&layout.plot_view_handle) {
                    layout.plot_view_handle = Handle::NULL;
                }
                if omitted_handles.contains(&layout.visual_style_handle) {
                    layout.visual_style_handle = Handle::NULL;
                }
            }
            ObjectType::PlotSettings(settings) => {
                settings.reactors.retain(|h| !omitted_handles.contains(h));
                if settings
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    settings.xdictionary_handle = None;
                }
                if omitted_handles.contains(&settings.plot_view_handle) {
                    settings.plot_view_handle = Handle::NULL;
                }
                if omitted_handles.contains(&settings.visual_style_handle) {
                    settings.visual_style_handle = Handle::NULL;
                }
            }
            ObjectType::ImageDefinition(imagedef) => {
                imagedef.reactors.retain(|h| !omitted_handles.contains(h));
            }
            ObjectType::UnderlayDefinition(def) => {
                def.reactors.retain(|h| !omitted_handles.contains(h));
            }
            ObjectType::ObjectContextData(ctx) => {
                ctx.reactors.retain(|h| !omitted_handles.contains(h));
                if ctx
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    ctx.xdictionary_handle = None;
                }
                if let ObjectContextKind::Dim(dim) = &mut ctx.kind {
                    if omitted_handles.contains(&dim.block) {
                        dim.block = Handle::NULL;
                    }
                }
            }
            ObjectType::VisualStyle(vs) => {
                vs.reactors.retain(|h| !omitted_handles.contains(h));
                if vs
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    vs.xdictionary_handle = None;
                }
            }
            ObjectType::Material(mat) => {
                mat.reactors.retain(|h| !omitted_handles.contains(h));
                if mat
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    mat.xdictionary_handle = None;
                }
            }
            ObjectType::GeoData(geo) => {
                geo.reactors.retain(|h| !omitted_handles.contains(h));
                if geo
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    geo.xdictionary_handle = None;
                }
            }
            ObjectType::DynamicBlock(dyn_obj) => {
                dyn_obj.reactors.retain(|h| !omitted_handles.contains(h));
                if dyn_obj
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    dyn_obj.xdictionary_handle = None;
                }
            }
            ObjectType::Associative(assoc) => {
                assoc.reactors.retain(|h| !omitted_handles.contains(h));
                if assoc
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    assoc.xdictionary_handle = None;
                }
            }
            ObjectType::Field(field) => {
                field.child_fields.retain(|h| !omitted_handles.contains(h));
                field
                    .referenced_objects
                    .retain(|h| !omitted_handles.contains(h));
            }
            ObjectType::FieldList(list) => {
                list.fields.retain(|h| !omitted_handles.contains(h));
            }
            ObjectType::DgnLineStyle(dgn) => {
                dgn.reactors.retain(|h| !omitted_handles.contains(h));
                if dgn
                    .xdictionary_handle
                    .is_some_and(|h| omitted_handles.contains(&h))
                {
                    dgn.xdictionary_handle = None;
                }
            }
            ObjectType::TableContent(table) | ObjectType::TableTemplate(table) => {
                clean_entity_common(&mut table.common, &omitted_handles);
            }
            ObjectType::Arch(arch) => {
                if let Some(codes) = &mut arch.raw_dxf_codes {
                    clean_raw_dxf_handle_codes(codes, &omitted_handles);
                }
            }
            ObjectType::Unknown { raw_dxf_codes, .. } => {
                if let Some(codes) = raw_dxf_codes {
                    clean_raw_dxf_handle_codes(codes, &omitted_handles);
                }
            }
            ObjectType::BlockVisibilityParameter(_)
            | ObjectType::MLineStyle(_)
            | ObjectType::MultiLeaderStyle(_)
            | ObjectType::TableStyle(_)
            | ObjectType::Scale(_)
            | ObjectType::DictionaryVariable(_)
            | ObjectType::ImageDefinitionReactor(_)
            | ObjectType::SpatialFilter(_)
            | ObjectType::RasterVariables(_)
            | ObjectType::BookColor(_)
            | ObjectType::PlaceHolder(_)
            | ObjectType::WipeoutVariables(_) => {}
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_constraint_records_are_not_classified_as_omitted() {
        for name in [
            "ACDBASSOCDEPENDENCY",
            "ACDBASSOCVALUEDEPENDENCY",
            "ACDBASSOCGEOMDEPENDENCY",
            "ACDBASSOCDIMDEPENDENCYBODY",
        ] {
            assert!(!is_unrestorable_assoc_object(name), "{name}");
        }
        for name in [
            "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION",
            "AcDbDynamicBlockPurgePreventer",
        ] {
            assert!(is_unrestorable_assoc_object(name), "{name}");
        }
    }

    #[test]
    fn image_reactors_retain_their_surviving_entity_owner() {
        let mut doc = Document::new();
        let image = doc
            .add_entity(EntityType::RasterImage(
                document::entities::RasterImage::new(
                    "synthetic.png",
                    document::types::Vector3::ZERO,
                    8.,
                    8.,
                ),
            ))
            .unwrap();
        let handle = doc.allocate_handle();
        let mut reactor = document::objects::ImageDefinitionReactor::new(image);
        reactor.handle = handle;
        reactor.owner = image;
        doc.objects
            .insert(handle, ObjectType::ImageDefinitionReactor(reactor));
        let before = doc.clone();
        assert!(!has_omitted_dxf_records(&doc));
        assert!(!prune_omitted_dxf_records(&mut doc));
        assert_eq!(doc, before);
    }

    #[test]
    fn removing_a_default_does_not_select_an_unrelated_dictionary_entry() {
        let mut doc = Document::new();
        let root = find_root_dict_handle(&doc);
        let dictionary = doc.allocate_handle();
        let removed = doc.allocate_handle();
        let retained = doc.allocate_handle();
        let mut placeholder = document::objects::PlaceHolder::default();
        placeholder.handle = retained;
        placeholder.owner = dictionary;
        doc.objects
            .insert(retained, ObjectType::PlaceHolder(placeholder));
        doc.objects.insert(
            removed,
            ObjectType::Unknown {
                type_name: "SYNTHETIC_RAW_OBJECT".into(),
                handle: removed,
                owner: dictionary,
                raw_dxf_codes: None,
                raw_dwg_data: Some(vec![0]),
                raw_dwg_handle_bits: 0,
                raw_dwg_version: Some(document::DxfVersion::AC1032),
            },
        );
        let mut value = document::objects::DictionaryWithDefault::default();
        value.handle = dictionary;
        value.owner = root;
        value.entries = vec![("removed".into(), removed), ("retained".into(), retained)];
        value.default_handle = removed;
        doc.objects
            .insert(dictionary, ObjectType::DictionaryWithDefault(value));
        assert!(prune_omitted_dxf_records(&mut doc));
        let ObjectType::DictionaryWithDefault(value) = &doc.objects[&dictionary] else {
            panic!()
        };
        assert_eq!(value.default_handle, Handle::NULL);
        assert_eq!(value.entries, vec![("retained".into(), retained)]);
    }
}
