use codec::entities::{Block, BlockEnd, EntityCommon};
use codec::objects::ObjectType;
use codec::tables::BlockRecord;
use codec::{Document, EntityType, Handle};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;

mod constraints;

/// The copy is built in the last saved document, so the source definition and
/// its references remain at their checkpoint even when the editor is dirty.
pub(super) struct DefinitionCopy {
    pub document: Document,
    pub handles: FxHashMap<Handle, Handle>,
    pub record: Handle,
}

fn remap_common(common: &mut EntityCommon, handles: &FxHashMap<Handle, Handle>) {
    let map = |h: Handle| handles.get(&h).copied().unwrap_or(h);
    common.handle = map(common.handle);
    common.owner_handle = map(common.owner_handle);
    common.xdictionary_handle = common.xdictionary_handle.map(map);
    common.reactors = common
        .reactors
        .iter()
        .filter_map(|h| handles.get(h).copied())
        .collect();
    let mut data = codec::xdata::ExtendedData::new();
    for mut record in common.extended_data.records().iter().cloned() {
        for value in &mut record.values {
            if let codec::xdata::XDataValue::Handle(handle) = value {
                *handle = map(*handle);
            }
        }
        data.add_record(record);
    }
    common.extended_data = data;
    common.raw_record = None;
}

fn cloneable_object(object: &ObjectType) -> bool {
    if let ObjectType::Associative(object) = object {
        return constraints::cloneable(object);
    }
    if let ObjectType::DynamicBlock(object) = object {
        return !matches!(object.data, codec::objects::DynamicBlockData::Unknown);
    }
    matches!(
        object,
        ObjectType::Dictionary(_)
            | ObjectType::DictionaryWithDefault(_)
            | ObjectType::DictionaryVariable(_)
            | ObjectType::SpatialFilter(_)
            | ObjectType::ObjectContextData(_)
            | ObjectType::SortEntitiesTable(_)
            | ObjectType::Field(_)
            | ObjectType::FieldList(_)
            | ObjectType::PlaceHolder(_)
            | ObjectType::ImageDefinitionReactor(_)
            | ObjectType::BlockVisibilityParameter(_)
            | ObjectType::XRecord(codec::objects::XRecord {
                entries_complete: true,
                ..
            })
    )
}

pub(super) fn copy_definition(
    source: &Document,
    mut target: Document,
    source_name: &str,
    name: &str,
) -> Result<DefinitionCopy, String> {
    let root = source
        .block_records
        .get(source_name)
        .ok_or_else(|| format!("block \"{source_name}\" does not exist"))?;
    // Shared symbol records can be created while editing. Retain them when
    // restoring the source definition to its checkpoint.
    target.header = source.header.clone();
    target.layers = source.layers.clone();
    target.line_types = source.line_types.clone();
    target.text_styles = source.text_styles.clone();
    target.dim_styles = source.dim_styles.clone();
    target.app_ids = source.app_ids.clone();
    for class in source.classes.iter() {
        if !target.classes.contains(&class.dxf_name) {
            target.classes.add_or_update(class.clone());
        }
    }
    let mut records = vec![(root.clone(), name.to_string())];
    let mut names = FxHashSet::default();
    names.insert(source_name.to_ascii_uppercase());
    let mut index = 0;
    let mut anonymous = 0;
    while index < records.len() {
        for entity in source.entities_in_block(&records[index].0.name) {
            if let EntityType::Insert(insert) = entity {
                if insert.block_name.eq_ignore_ascii_case(name) {
                    return Err(crate::scene::block_service::circular_dependency_error(
                        name,
                        &insert.block_name,
                    ));
                }
                if target.block_records.get(&insert.block_name).is_none()
                    && names.insert(insert.block_name.to_ascii_uppercase())
                {
                    let record = source
                        .block_records
                        .get(&insert.block_name)
                        .ok_or_else(|| format!("block \"{}\" does not exist", insert.block_name))?;
                    records.push((record.clone(), record.name.clone()));
                }
            }
            if let EntityType::Dimension(dimension) = entity {
                let picture = &dimension.base().block_name;
                if picture.is_empty() || !names.insert(picture.to_ascii_uppercase()) {
                    continue;
                }
                let Some(record) = source.block_records.get(picture) else {
                    return Err(format!("Dimension picture block \"{picture}\" is missing."));
                };
                let new_name = loop {
                    let candidate = format!("*D{anonymous}");
                    anonymous += 1;
                    if target.block_records.get(&candidate).is_none()
                        && !records
                            .iter()
                            .any(|(_, name)| name.eq_ignore_ascii_case(&candidate))
                    {
                        break candidate;
                    }
                };
                records.push((record.clone(), new_name));
            }
        }
        index += 1;
    }

    let mut entities = Vec::new();
    let mut identities = FxHashSet::default();
    for (record, _) in &records {
        identities.insert(record.handle);
        for marker in [record.block_entity_handle, record.block_end_handle] {
            if let Some(entity) = source.get_entity(marker) {
                entities.push(entity.clone());
            }
        }
        for entity in source.entities_in_block(&record.name) {
            if matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)) {
                continue;
            }
            if matches!(entity, EntityType::Unknown(_)) {
                return Err("Save Block As cannot copy opaque entities yet.".to_string());
            }
            entities.push(entity.clone());
        }
    }
    for entity in &entities {
        identities.insert(entity.common().handle);
        match entity {
            EntityType::Insert(insert) => {
                identities.extend(insert.attributes.iter().map(|a| a.common.handle));
                identities.extend(
                    insert
                        .attributes
                        .iter()
                        .filter_map(|a| a.embedded_mtext.as_ref().map(|text| text.common.handle)),
                );
                identities.extend(insert.seqend_handle);
            }
            EntityType::AttributeDefinition(attribute) => {
                identities.extend(
                    attribute
                        .embedded_mtext
                        .as_ref()
                        .map(|text| text.common.handle),
                );
            }
            EntityType::AttributeEntity(attribute) => {
                identities.extend(
                    attribute
                        .embedded_mtext
                        .as_ref()
                        .map(|text| text.common.handle),
                );
            }
            EntityType::Polyline3D(polyline) => {
                identities.extend(polyline.vertices.iter().map(|v| v.handle))
            }
            _ => {}
        }
    }
    identities.remove(&Handle::NULL);
    // Ownership, rather than arbitrary references, determines what must be
    // independent. Nested INSERT definitions and shared styles stay shared.
    let mut objects = Vec::new();
    loop {
        let added: Vec<_> = source
            .objects
            .iter()
            .filter(|(handle, _)| {
                !identities.contains(handle)
                    && source
                        .object_owner(**handle)
                        .is_some_and(|owner| identities.contains(&owner))
            })
            .map(|(handle, object)| (*handle, object.clone()))
            .collect();
        if added.is_empty() {
            break;
        }
        for (handle, object) in added {
            if !cloneable_object(&object) {
                return Err(
                    "Save Block As cannot copy this block's native object relationships yet."
                        .to_string(),
                );
            }
            identities.insert(handle);
            objects.push((handle, object));
        }
    }
    for owner in &identities {
        if let Some(dictionary) = source.extension_dictionary_handle(*owner) {
            if !identities.contains(&dictionary) {
                return Err(
                    "Save Block As requires an independently owned extension dictionary."
                        .to_string(),
                );
            }
        }
    }
    for (_, object) in &objects {
        if let ObjectType::Dictionary(dictionary) = object {
            if dictionary.entries.iter().any(|(key, handle)| {
                !handle.is_null()
                    && (dictionary.hard_owner || dictionary.is_entry_hard_owner(key))
                    && !identities.contains(handle)
            }) {
                return Err(
                    "Save Block As cannot duplicate a dictionary's foreign owned entries."
                        .to_string(),
                );
            }
        }
    }
    identities.remove(&Handle::NULL);
    let floor = source
        .next_handle()
        .max(source.header.handle_seed)
        .max(target.next_handle());
    if target.next_handle() < floor {
        target.header.handle_seed = floor - 1;
        target.allocate_handle();
    }
    let mut identities: Vec<_> = identities.into_iter().collect();
    identities.sort_by_key(|h| h.value());
    let handles: FxHashMap<_, _> = identities
        .into_iter()
        .map(|h| (h, target.allocate_handle()))
        .collect();
    let remap: HashMap<_, _> = handles.iter().map(|(a, b)| (*a, *b)).collect();
    let block_names: HashMap<_, _> = records
        .iter()
        .map(|(r, n)| (r.name.to_ascii_uppercase(), n.clone()))
        .collect();
    for (record, new_name) in &records {
        let mut copied: BlockRecord = record.clone();
        copied.name = new_name.clone();
        copied.handle = handles[&record.handle];
        copied.block_entity_handle = handles
            .get(&record.block_entity_handle)
            .copied()
            .unwrap_or_else(|| target.allocate_handle());
        copied.block_end_handle = handles
            .get(&record.block_end_handle)
            .copied()
            .unwrap_or_else(|| target.allocate_handle());
        copied.entity_handles.clear();
        copied.insert_handles.clear();
        copied.insert_count_bytes.clear();
        copied.preview_data.clear();
        copied.flags.anonymous = new_name.starts_with('*');
        copied.layout = Handle::NULL;
        target
            .block_records
            .add(copied.clone())
            .map_err(|e| e.to_string())?;
        if source.get_entity(record.block_entity_handle).is_none() {
            let mut marker = Block::new(new_name, record.base_point);
            marker.common.handle = copied.block_entity_handle;
            marker.common.owner_handle = copied.handle;
            target
                .add_entity(EntityType::Block(marker))
                .map_err(|e| e.to_string())?;
        }
        if source.get_entity(record.block_end_handle).is_none() {
            let mut marker = BlockEnd::new();
            marker.common.handle = copied.block_end_handle;
            marker.common.owner_handle = copied.handle;
            target
                .add_entity(EntityType::BlockEnd(marker))
                .map_err(|e| e.to_string())?;
        }
    }
    for mut entity in entities {
        remap_common(entity.common_mut(), &handles);
        match &mut entity {
            EntityType::Block(block) => {
                block.name = block_names[&block.name.to_ascii_uppercase()].clone();
            }
            EntityType::Insert(insert) => {
                insert.seqend_handle = insert.seqend_handle.map(|h| handles[&h]);
                let copied_definition = block_names
                    .get(&insert.block_name.to_ascii_uppercase())
                    .is_some_and(|name| name.eq_ignore_ascii_case(&insert.block_name));
                for attribute in &mut insert.attributes {
                    remap_common(&mut attribute.common, &handles);
                    if attribute.common.handle.is_null() {
                        attribute.common.handle = target.allocate_handle();
                    }
                    attribute.common.owner_handle = insert.common.handle;
                    if copied_definition {
                        attribute.attdef_handle = handles
                            .get(&attribute.attdef_handle)
                            .copied()
                            .unwrap_or(attribute.attdef_handle);
                    }
                    if let Some(text) = &mut attribute.embedded_mtext {
                        remap_common(&mut text.common, &handles);
                        if text.common.handle.is_null() {
                            text.common.handle = target.allocate_handle();
                        }
                    }
                }
            }
            EntityType::AttributeDefinition(attribute) => {
                if let Some(text) = &mut attribute.embedded_mtext {
                    remap_common(&mut text.common, &handles);
                    if text.common.handle.is_null() {
                        text.common.handle = target.allocate_handle();
                    }
                }
            }
            EntityType::AttributeEntity(attribute) => {
                attribute.attdef_handle = handles
                    .get(&attribute.attdef_handle)
                    .copied()
                    .unwrap_or(attribute.attdef_handle);
                if let Some(text) = &mut attribute.embedded_mtext {
                    remap_common(&mut text.common, &handles);
                    if text.common.handle.is_null() {
                        text.common.handle = target.allocate_handle();
                    }
                }
            }
            EntityType::Polyline3D(polyline) => {
                for vertex in &mut polyline.vertices {
                    vertex.handle = handles
                        .get(&vertex.handle)
                        .copied()
                        .unwrap_or_else(|| target.allocate_handle());
                }
            }
            EntityType::Dimension(dimension) => {
                if let Some(name) =
                    block_names.get(&dimension.base().block_name.to_ascii_uppercase())
                {
                    dimension.base_mut().block_name = name.clone();
                }
            }
            EntityType::Leader(leader) => {
                if let Some(handle) = handles.get(&leader.annotation_handle) {
                    leader.annotation_handle = *handle;
                }
            }
            EntityType::RasterImage(image) => {
                image.definition_reactor_handle = image
                    .definition_reactor_handle
                    .map(|h| handles.get(&h).copied().unwrap_or(h));
            }
            EntityType::Wipeout(image) => {
                image.definition_reactor_handle = image
                    .definition_reactor_handle
                    .map(|h| handles.get(&h).copied().unwrap_or(h));
            }
            _ => {}
        }
        target.add_entity(entity).map_err(|e| e.to_string())?;
    }
    crate::scene::remap_hatch_associations(&mut target, &handles);
    for (old, mut object) in objects {
        let new = handles[&old];
        match &mut object {
            ObjectType::Associative(object) => {
                constraints::remap(object, new, &handles)?;
            }
            ObjectType::DynamicBlock(object) => {
                object.handle = new;
                object.visit_handles_mut(&mut |handle| {
                    if let Some(new) = handles.get(handle) {
                        *handle = *new;
                    }
                });
                if let codec::objects::DynamicBlockData::VisibilityParameter(parameter) =
                    &object.data
                {
                    target
                        .block_visibility_params
                        .insert(new, parameter.clone());
                }
            }
            ObjectType::BlockVisibilityParameter(parameter) => {
                parameter.handle = new;
                parameter.owner = handles
                    .get(&parameter.owner)
                    .copied()
                    .unwrap_or(parameter.owner);
                if let codec::objects::BlockEvalValue::Handle(handle) = &mut parameter.eval_value {
                    *handle = handles.get(handle).copied().unwrap_or(*handle);
                }
                for handle in
                    parameter
                        .all_blocks
                        .iter_mut()
                        .chain(parameter.states.iter_mut().flat_map(|state| {
                            state
                                .visible_blocks
                                .iter_mut()
                                .chain(state.visible_params.iter_mut())
                        }))
                {
                    *handle = handles.get(handle).copied().unwrap_or(*handle);
                }
                target
                    .block_visibility_params
                    .insert(new, parameter.clone());
            }
            ObjectType::SortEntitiesTable(table) => {
                let source_block = table.block_owner_handle;
                let mut ordered: Vec<_> = source
                    .entities()
                    .filter(|e| {
                        e.common().owner_handle == source_block
                            && !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))
                    })
                    .map(|e| {
                        (
                            table.effective_sort_handle(e.common().handle),
                            e.common().handle,
                        )
                    })
                    .collect();
                ordered.sort_by_key(|(sort, entity)| (sort.value(), entity.value()));
                table.handle = new;
                table.owner_handle = handles[&table.owner_handle];
                table.block_owner_handle = handles[&source_block];
                table.clear();
                table.raw_dxf_codes = None;
                table.raw_dxf_version = None;
                for (_, entity) in ordered {
                    table.add_entry(handles[&entity], target.allocate_handle());
                }
            }
            ObjectType::Field(field) => {
                field.handle = new;
                field.owner = handles[&field.owner];
                for h in field
                    .child_fields
                    .iter_mut()
                    .chain(field.referenced_objects.iter_mut())
                {
                    if let Some(new) = handles.get(h) {
                        *h = *new;
                    }
                }
                for value in std::iter::once(&mut field.value)
                    .chain(field.child_values.iter_mut().map(|c| &mut c.value))
                {
                    if let Some(handle) = value.handle_value.as_mut() {
                        if let Some(new) = handles.get(handle) {
                            *handle = *new;
                        }
                    }
                }
            }
            ObjectType::FieldList(fields) => {
                fields.handle = new;
                fields.owner = handles[&fields.owner];
                for h in &mut fields.fields {
                    if let Some(new) = handles.get(h) {
                        *h = *new;
                    }
                }
            }
            ObjectType::PlaceHolder(placeholder) => {
                placeholder.handle = new;
                placeholder.owner = handles[&placeholder.owner];
            }
            ObjectType::ImageDefinitionReactor(reactor) => {
                reactor.handle = new;
                reactor.owner = handles[&reactor.owner];
                if let Some(new) = handles.get(&reactor.image_handle) {
                    reactor.image_handle = *new;
                }
            }
            _ => super::command_driver::remap_object(&mut object, new, &remap),
        }
        if let ObjectType::XRecord(record) = &mut object {
            for reference in &mut record.object_references {
                if let Some(new) = handles.get(&reference.handle) {
                    reference.handle = *new;
                }
            }
            record.reactors = record
                .reactors
                .iter()
                .filter_map(|h| handles.get(h).copied())
                .collect();
            record.xdictionary_handle = record
                .xdictionary_handle
                .map(|h| handles.get(&h).copied().unwrap_or(h));
            record.raw_dwg_data = None;
            record.raw_dwg_version = None;
        }
        target.objects.insert(new, object);
    }
    for (_, name) in &records {
        let has_attributes = target
            .entities_in_block(name)
            .any(|entity| matches!(entity, EntityType::AttributeDefinition(_)));
        target
            .block_records
            .get_mut(name)
            .unwrap()
            .flags
            .has_attributes = has_attributes;
        for entity in target.entities_in_block(name) {
            if let EntityType::Insert(insert) = entity {
                if crate::scene::block_service::would_create_cycle(
                    &target,
                    name,
                    &insert.block_name,
                ) {
                    return Err(crate::scene::block_service::circular_dependency_error(
                        name,
                        &insert.block_name,
                    ));
                }
            }
        }
    }
    for (old, new) in &handles {
        if let Some(data) = source.object_xdata(*old) {
            for mut record in data.records().iter().cloned() {
                for value in &mut record.values {
                    if let codec::xdata::XDataValue::Handle(handle) = value {
                        *handle = handles.get(handle).copied().unwrap_or(*handle);
                    }
                }
                target.set_object_xdata_record(*new, record);
            }
        }
        if let Some(definition) = source.block_representations.get(old) {
            target.block_representations.insert(
                *new,
                handles.get(definition).copied().unwrap_or(*definition),
            );
        }
        if let Some(field) = source.fields.get(old) {
            let mut field = field.clone();
            field.handle = *new;
            field.owner = handles.get(&field.owner).copied().unwrap_or(field.owner);
            for object in &mut field.objects {
                if let Some(new) = handles.get(object) {
                    *object = *new;
                }
            }
            target.fields.insert(*new, field);
        }
        if let Some(scale) = source.context_scales.get(old) {
            target.context_scales.insert(*new, *scale);
        }
    }
    for (record, name) in &records {
        let definition = handles[&record.handle];
        crate::scene::block_authoring::refresh_copied_identity(&mut target, definition, name);
    }
    crate::scene::Scene::register_copied_constraint_networks(&mut target, &handles)?;
    Ok(DefinitionCopy {
        record: handles[&root.handle],
        document: target,
        handles,
    })
}
