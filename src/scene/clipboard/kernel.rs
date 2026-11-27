//! The copy and paste kernel: carrying entities, with the table records,
//! blocks, styles and objects they depend on, from one drawing into another.
//! The commands that drive it are in `app::command_driver::clipboard`.

use document::objects::{Dictionary, ObjectType};
use document::Handle;

use crate::entities::doc_edit::{add_entity_clone_doc_inner, define_block_raw_doc};

/// PASTECLIP kernel: paste a clipboard payload (`entities` + `deps`, as
/// captured by [`copy_to_clipboard_kernel`]) into `doc`, optionally
/// translated, returning the new handles index-aligned with `entities`
/// Recreate missing layer/linetype/text-style/dim-style records with fresh
/// handles from the target document. Shared by `merge_dependencies` (PASTEBLOCK
/// path) and `paste_entities_kernel` — one copy, not a port per caller.
/// Returns the names of the dimension styles it added.
pub(crate) fn merge_table_records(
    doc: &mut document::Document,
    deps: &super::ClipboardDeps,
) -> Vec<String> {
    use document::TableEntry;
    for rec in &deps.layers {
        if !doc.layers.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            let _ = doc.layers.add(r);
        }
    }
    // Shape and text segments name their style by handle: find the target's
    // matching style (a shape file by its font file, it has no name) or
    // recreate it, so a pasted linetype never points into the source drawing.
    let mut segment_styles: rustc_hash::FxHashMap<Handle, Handle> =
        rustc_hash::FxHashMap::default();
    for style in &deps.linetype_styles {
        let existing = doc
            .text_styles
            .iter()
            .find(|s| {
                s.is_shape_file == style.is_shape_file
                    && if style.is_shape_file {
                        s.font_file.eq_ignore_ascii_case(&style.font_file)
                    } else {
                        s.name.eq_ignore_ascii_case(&style.name)
                    }
            })
            .map(|s| s.handle);
        let handle = existing.unwrap_or_else(|| {
            let mut r = style.clone();
            r.set_handle(doc.allocate_handle());
            let handle = r.handle;
            doc.text_styles.add_allow_duplicate(r);
            handle
        });
        segment_styles.insert(style.handle, handle);
    }
    for rec in &deps.linetypes {
        if !doc.line_types.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            for element in &mut r.elements {
                if let Some(complex) = element.complex.as_mut() {
                    if let Some(new_style) = segment_styles.get(&complex.style_handle) {
                        complex.style_handle = *new_style;
                    }
                }
            }
            let _ = doc.line_types.add(r);
        }
    }
    for rec in &deps.text_styles {
        if !doc.text_styles.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            let _ = doc.text_styles.add(r);
        }
    }
    let mut added_dim_styles = Vec::new();
    for rec in &deps.dim_styles {
        if !doc.dim_styles.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            repoint_dim_style(doc, &mut r, &deps.dim_style_refs);
            added_dim_styles.push(r.name.clone());
            let _ = doc.dim_styles.add(r);
        }
    }
    merge_app_ids(doc, deps);
    added_dim_styles
}

/// The arrow blocks a dimension style names.
pub(crate) fn dim_style_blocks(style: &document::tables::DimStyle) -> [Handle; 4] {
    [style.dimblk, style.dimblk1, style.dimblk2, style.dimldrblk]
}

/// The entity, context and override references that must never retain source handles.
pub(crate) fn mleader_handles(m: &mut document::entities::MultiLeader) -> Vec<&mut Option<Handle>> {
    let mut handles = vec![
        &mut m.style_handle,
        &mut m.text_style_handle,
        &mut m.line_type_handle,
        &mut m.arrowhead_handle,
        &mut m.block_content_handle,
        &mut m.context.text_style_handle,
        &mut m.context.block_content_handle,
    ];
    for root in &mut m.context.leader_roots {
        for line in &mut root.lines {
            handles.extend([&mut line.line_type_handle, &mut line.arrowhead_handle]);
        }
    }
    handles.extend(
        m.arrowhead_overrides
            .iter_mut()
            .map(|a| &mut a.arrowhead_handle),
    );
    handles.extend(
        m.block_attributes
            .iter_mut()
            .map(|a| &mut a.attribute_definition_handle),
    );
    handles
}

pub(crate) fn remap_mleader(
    entity: &mut document::EntityType,
    map: &std::collections::HashMap<Handle, Handle>,
) {
    if let document::EntityType::MultiLeader(m) = entity {
        for h in mleader_handles(m) {
            *h = h
                .and_then(|h| map.get(&h).copied())
                .filter(|h| !h.is_null());
        }
    }
}

/// Point a pasted reference's attribute values at this drawing's ATTDEF for
/// the same tag. `map` comes from the block definitions merged with them; a
/// value whose definition the copy could not bring points nowhere rather
/// than at a handle that means something else here.
pub(crate) fn remap_insert_attributes(
    entity: &mut document::EntityType,
    map: &std::collections::HashMap<Handle, Handle>,
) {
    let document::EntityType::Insert(insert) = entity else {
        return;
    };
    for attribute in &mut insert.attributes {
        attribute.attdef_handle = map
            .get(&attribute.attdef_handle)
            .copied()
            .unwrap_or(Handle::NULL);
    }
}

/// Merge named styles after blocks exist, keeping target definitions on a name conflict.
pub(crate) fn merge_mleader_styles(
    doc: &mut document::Document,
    deps: &super::ClipboardDeps,
) -> std::collections::HashMap<Handle, Handle> {
    use document::objects::ObjectType;
    let mut map = block_record_map(doc, &deps.blocks);
    for source in &deps.text_styles {
        if let Some(target) = doc.text_styles.get(&source.name) {
            map.insert(source.handle, target.handle);
        }
    }
    for source in &deps.linetypes {
        if let Some(target) = doc.line_types.get(&source.name) {
            map.insert(source.handle, target.handle);
        }
    }
    for def in &deps.blocks {
        let Some(&record) = map.get(&def.source) else {
            continue;
        };
        let members = crate::scene::block_authoring::members(doc, record);
        for source in &def.entities {
            let document::EntityType::AttributeDefinition(a) = source else {
                continue;
            };
            if let Some(target) = members.iter().find(|&&h| matches!(doc.get_entity(h),
                Some(document::EntityType::AttributeDefinition(b)) if a.tag.eq_ignore_ascii_case(&b.tag)))
            {
                map.insert(a.common.handle, *target);
            }
        }
    }
    for source in &deps.mleader_styles {
        let existing = doc.objects.iter().find_map(|(&h, o)| match o {
            ObjectType::MultiLeaderStyle(s) if s.name.eq_ignore_ascii_case(&source.name) => Some(h),
            _ => None,
        });
        let target = existing.unwrap_or_else(|| {
            let mut s = source.clone();
            s.handle = doc.allocate_handle();
            for h in [
                &mut s.text_style_handle,
                &mut s.line_type_handle,
                &mut s.arrowhead_handle,
                &mut s.block_content_handle,
            ] {
                *h = h.and_then(|h| map.get(&h).copied());
            }
            let h = s.handle;
            doc.objects.insert(h, ObjectType::MultiLeaderStyle(s));
            h
        });
        map.insert(source.handle, target);
    }
    if !deps.mleader_styles.is_empty() {
        sync_mleaderstyle_dictionary(doc);
    }
    map
}

/// This drawing's record named like source record `source`, or null.
pub(crate) fn dim_style_ref<T: document::TableEntry>(
    table: &document::tables::Table<T>,
    refs: &rustc_hash::FxHashMap<Handle, String>,
    source: Handle,
) -> Handle {
    refs.get(&source)
        .and_then(|name| table.get(name))
        .map_or(Handle::NULL, |record| record.handle())
}

/// Point a pasted dimension style, which still holds the source drawing's
/// handles, at this drawing's text style, linetypes and arrow blocks of the
/// same names (`refs` names each source handle). A record this drawing lacks
/// is cleared, so the style falls back to the default instead of naming an
/// object that is not there.
pub(crate) fn repoint_dim_style(
    doc: &document::Document,
    style: &mut document::tables::DimStyle,
    refs: &rustc_hash::FxHashMap<Handle, String>,
) {
    style.dimtxsty_handle = dim_style_ref(&doc.text_styles, refs, style.dimtxsty_handle);
    for h in [
        &mut style.dimltex_handle,
        &mut style.dimltex1_handle,
        &mut style.dimltex2_handle,
    ] {
        *h = dim_style_ref(&doc.line_types, refs, *h);
    }
    for h in [
        &mut style.dimblk,
        &mut style.dimblk1,
        &mut style.dimblk2,
        &mut style.dimldrblk,
    ] {
        *h = dim_style_ref(&doc.block_records, refs, *h);
    }
}

/// Point the dimension styles a paste added (`added`) at the arrow blocks
/// the block merge has since recreated: the table merge ran first, while
/// this drawing still lacked them.
pub(crate) fn repoint_added_dim_style_blocks(
    doc: &mut document::Document,
    deps: &super::ClipboardDeps,
    added: &[String],
) {
    for rec in deps.dim_styles.iter().filter(|r| added.contains(&r.name)) {
        let blocks = dim_style_blocks(rec)
            .map(|h| dim_style_ref(&doc.block_records, &deps.dim_style_refs, h));
        if let Some(style) = doc.dim_styles.get_mut(&rec.name) {
            [style.dimblk, style.dimblk1, style.dimblk2, style.dimldrblk] = blocks;
        }
    }
}

/// Register the copied XDATA's applications this drawing lacks.
pub(crate) fn merge_app_ids(doc: &mut document::Document, deps: &super::ClipboardDeps) {
    for rec in &deps.app_ids {
        if !doc.app_ids.contains(&rec.name) {
            let mut r = rec.clone();
            r.handle = doc.allocate_handle();
            let _ = doc.app_ids.add(r);
        }
    }
}

/// The `ACAD_IMAGE_DICT` dictionary under the named-objects root, created
/// when the drawing has none. `None` when the drawing has no root dictionary.
pub(crate) fn ensure_image_dictionary(document: &mut document::Document) -> Option<Handle> {
    use document::objects::{Dictionary, ObjectType};
    let root = document.header.named_objects_dict_handle;
    let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get(&root) else {
        return None;
    };
    if let Some(existing) = root_dictionary.get("ACAD_IMAGE_DICT") {
        if matches!(
            document.objects.get(&existing),
            Some(ObjectType::Dictionary(_))
        ) {
            return Some(existing);
        }
    }
    let handle = document.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = root;
    document
        .objects
        .insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get_mut(&root) {
        root_dictionary.add_entry("ACAD_IMAGE_DICT", handle);
    }
    Some(handle)
}

/// Register image definition `definition` in ACAD_IMAGE_DICT under its file's
/// name (made unique), like an image AutoCAD attached. Returns the dictionary,
/// which is to own the definition; `None` when the drawing has no root
/// dictionary.
pub(crate) fn register_image_definition(
    document: &mut document::Document,
    definition: Handle,
    file_path: &str,
) -> Option<Handle> {
    let stem = std::path::Path::new(file_path)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("IMAGE")
        .to_owned();
    let dictionary = ensure_image_dictionary(document)?;
    if let Some(document::objects::ObjectType::Dictionary(entries)) =
        document.objects.get_mut(&dictionary)
    {
        let mut key = stem.clone();
        let mut suffix = 1;
        while entries.get(&key).is_some() {
            suffix += 1;
            key = format!("{stem}_{suffix}");
        }
        entries.add_entry(key, definition);
    }
    Some(dictionary)
}

// The kernel has allocated the final entity handles. Register reactors in the
// Scene shell so paste's existing undo scope captures objects and entity links.
pub(crate) fn ensure_pasted_image_reactors(
    scene: &mut crate::scene::Scene,
    handles: &[Handle],
    deps: &super::ClipboardDeps,
    first_new_handle: u64,
) {
    let mut images = handles.to_vec();
    for block in &deps.blocks {
        if let Some(record) = scene
            .document
            .block_records
            .get(&block.name)
            .filter(|record| record.handle.value() >= first_new_handle)
        {
            // Only newly imported members; existing definitions belong to the
            // target drawing and COPY of an INSERT keeps their images intact.
            images.extend(crate::scene::block_authoring::members(
                &scene.document,
                record.handle,
            ));
        }
    }
    for handle in images {
        let Some(document::EntityType::RasterImage(image)) = scene.document.get_entity(handle)
        else {
            continue;
        };
        let mut entity = document::EntityType::RasterImage(image.clone());
        scene.ensure_image_definition(&mut entity);
        *scene.document.get_entity_mut(handle).expect("pasted image") = entity;
    }
}

/// The Scene half of a paste, for every path that runs
/// [`paste_entities_kernel`]: interactive PASTECLIP/PASTEORIG and the
/// cross-document control copy. The kernel adds entities to the document; this
/// registers the objects they need, seeds the caches the renderer reads, bumps
/// what changed, and re-creates the groups and constraints that travelled with
/// them. Shared so that a wiring step added for one caller cannot be missing
/// from another.
pub(crate) fn finish_paste_in_scene(
    scene: &mut crate::scene::Scene,
    deps: &super::ClipboardDeps,
    by_index: &[Handle],
    handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
    block_records_before: usize,
    first_new_handle: u64,
) {
    ensure_pasted_image_reactors(scene, by_index, deps, first_new_handle);
    // Batched equivalent of `add_entity`'s per-add bump (same pattern as
    // `Scene::add_entities`): fresh top-level solids tessellate once here
    // instead of once per entity. Block-definition merges (and pasted
    // Block/BlockEnd sentinels) need the full geometry rebuild, matching
    // the `bump_geometry` tails the Scene helpers used to run. INSERTs of
    // blocks this drawing already defines add no definition, so they take
    // the per-entity bump like any other pasted entity.
    let mut extra_block_handles: Vec<Handle> = Vec::new();
    let mut changes = Vec::new();
    if scene.document.block_records.len() != block_records_before {
        for record in scene.document.block_records.iter() {
            if record.handle.value() >= first_new_handle {
                extra_block_handles.push(record.handle);
                for &member in &record.entity_handles {
                    changes.push((member, crate::scene::ChangeKind::Added));
                }
            }
        }
    }
    let mut has_block_sentinel = false;
    for &handle in by_index {
        if handle.is_null() {
            continue;
        }
        // `add_entity` seeds fill and raster models; the document-level
        // add does not, so a pasted hatch without an association
        // to rebuild it was never drawn. (#1807)
        if matches!(
            scene.document.get_entity(handle),
            Some(
                document::EntityType::Hatch(_)
                    | document::EntityType::Solid(_)
                    | document::EntityType::RasterImage(_)
                    | document::EntityType::Ole2Frame(_)
                    | document::EntityType::Underlay(_)
            )
        ) {
            scene.reseed_derived_caches(handle);
        }
        if let Some(entity) = scene.document.get_entity(handle) {
            if matches!(
                entity,
                document::EntityType::Block(_) | document::EntityType::BlockEnd(_)
            ) {
                has_block_sentinel = true;
                let owner = entity.common().owner_handle;
                if !owner.is_null() {
                    extra_block_handles.push(owner);
                }
            } else if let Some(edited) = scene.block_edit_block {
                if entity.common().owner_handle == edited {
                    extra_block_handles.push(edited);
                }
            }
        }
        changes.push((handle, crate::scene::ChangeKind::Added));
    }
    if has_block_sentinel || !extra_block_handles.is_empty() {
        scene.bump_entities_restyled_with_block_deps(&changes, &extra_block_handles, &[]);
    } else if !changes.is_empty() {
        scene.bump_entities(&changes);
    }
    // The wires above predate the freshly attached clip object
    // graphs; refresh the pasted entities so XCLIP-filtered inserts
    // render clipped immediately.
    if !deps.ext_objects.is_empty() {
        let changes: Vec<_> = by_index
            .iter()
            .copied()
            .filter(|handle| !handle.is_null())
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        scene.bump_entities(&changes);
    }
    // Source handles stored in the clipboard map one-to-one to the freshly
    // pasted handles (see `handle_map`); refresh each pasted LEADER's
    // displayed annotation context now that the relink ran in the kernel.
    for &handle in by_index.iter().filter(|h| !h.is_null()) {
        if matches!(
            scene.document.get_entity(handle),
            Some(document::EntityType::Leader(_))
        ) {
            let _ = scene.sync_displayed_annotation_context(handle);
        }
    }
    // Recreate any group whose whole membership was copied, so a pasted
    // group stays grouped — cross-drawing too, since the groups were
    // snapshotted into the clipboard at copy time. `by_index` is aligned
    // with the clipboard, so the kernel's `handle_map` maps each
    // clipboard clone to its new handle. Same shared `recreate_groups`
    // the in-drawing COPY path uses. (#440)
    if !deps.groups.is_empty() {
        scene.recreate_groups(deps.groups.clone(), handle_map);
    }
    // Constraints wholly within the pasted selection follow it.
    scene.duplicate_parametric_constraints_for(handle_map);
    // Incremental: fresh top-level solids already tessellated above, and
    // existing document solids are still cached — so only newly
    // introduced block-definition solids need building. The full rebuild
    // would clear and re-tessellate the entire document (every solid in the
    // drawing) on each paste, which is what made a large paste stall.
    scene.populate_missing_meshes_from_document();
}

/// Source image definition → this drawing's definition of the same file (one
/// definition per file, as AutoCAD keeps it), recreated and registered in
/// ACAD_IMAGE_DICT when the drawing has none. Pasting within the drawing
/// finds the very same object.
pub(crate) fn merge_image_definitions(
    doc: &mut document::Document,
    deps: &super::ClipboardDeps,
) -> rustc_hash::FxHashMap<Handle, Handle> {
    use document::objects::ObjectType;
    let mut map = rustc_hash::FxHashMap::default();
    for definition in &deps.image_definitions {
        let same_file = |d: &document::objects::ImageDefinition| {
            d.file_name.eq_ignore_ascii_case(&definition.file_name)
        };
        let existing = match doc.objects.get(&definition.handle) {
            Some(ObjectType::ImageDefinition(d)) if same_file(d) => Some(definition.handle),
            _ => doc
                .objects
                .iter()
                .find_map(|(handle, object)| match object {
                    ObjectType::ImageDefinition(d) if same_file(d) => Some(*handle),
                    _ => None,
                }),
        };
        let handle = existing.unwrap_or_else(|| {
            let handle = doc.allocate_handle();
            let mut copy = definition.clone();
            copy.handle = handle;
            // The source images' reactors; pasted images drop theirs.
            copy.reactors.clear();
            copy.owner = register_image_definition(doc, handle, &definition.file_name)
                .unwrap_or(Handle::NULL);
            doc.objects
                .insert(handle, ObjectType::ImageDefinition(copy));
            handle
        });
        map.insert(definition.handle, handle);
    }
    map
}

/// Point a pasted raster image at this drawing's image definition. Its
/// reactor link is dropped, as binding an xref does: the reactor belongs to
/// the source image, also when pasting within the same drawing.
pub(crate) fn remap_image_definition(
    entity: &mut document::EntityType,
    definitions: &rustc_hash::FxHashMap<Handle, Handle>,
) {
    let document::EntityType::RasterImage(image) = entity else {
        return;
    };
    if let Some(definition) = image.definition_handle {
        if let Some(&target) = definitions.get(&definition) {
            image.definition_handle = Some(target);
        }
    }
    image.definition_reactor_handle = None;
}

/// The block records a table's block cells show.
pub(crate) fn table_cell_blocks(
    table: &document::entities::Table,
) -> impl Iterator<Item = Handle> + '_ {
    table
        .rows
        .iter()
        .flat_map(|row| &row.cells)
        .flat_map(|cell| &cell.contents)
        .filter_map(|content| content.block_handle)
}

/// Point a pasted table's block cells at this drawing's records of the same
/// blocks. `records` maps source records to target records.
pub(crate) fn remap_table_cell_blocks(
    table: &mut document::entities::Table,
    records: &std::collections::HashMap<Handle, Handle>,
) {
    let contents = table
        .rows
        .iter_mut()
        .flat_map(|row| &mut row.cells)
        .flat_map(|cell| &mut cell.contents);
    for content in contents {
        if let Some(block) = content.block_handle.as_mut() {
            if let Some(&target) = records.get(block) {
                *block = target;
            }
        }
    }
}

/// Source APPID handle → this drawing's APPID of the same name, for the
/// copied XDATA. Run after [`merge_app_ids`].
pub(crate) fn eed_app_map(
    doc: &document::Document,
    deps: &super::ClipboardDeps,
) -> rustc_hash::FxHashMap<u64, u64> {
    deps.app_ids
        .iter()
        .filter_map(|a| Some((a.handle.value(), doc.app_ids.get(&a.name)?.handle.value())))
        .collect()
}

/// Re-point a copied entity's verbatim EED at this drawing's APPIDs. A block
/// whose application the map does not know names no APPID here, and a DWG
/// carrying one is rejected outright by the reference program, so it goes.
pub(crate) fn remap_eed_apps(
    entity: &mut document::EntityType,
    apps: &rustc_hash::FxHashMap<u64, u64>,
) {
    let remap = |xdata: &mut document::xdata::ExtendedData| {
        xdata
            .raw_dwg_eed
            .retain_mut(|(app, _)| match apps.get(app) {
                Some(&target) => {
                    *app = target;
                    true
                }
                None => false,
            });
    };
    remap(&mut entity.common_mut().extended_data);
    if let document::EntityType::Insert(insert) = entity {
        for attribute in &mut insert.attributes {
            remap(&mut attribute.common.extended_data);
        }
    }
}

/// Merge the clipboard's block definitions and multileader styles, and point
/// the multileaders inside newly added blocks at this drawing's records.
/// Returns how many blocks were added and the map the pasted multileaders are
/// remapped with ([`remap_mleader`]).
pub(crate) fn merge_blocks_and_mleader_styles(
    doc: &mut document::Document,
    deps: &super::ClipboardDeps,
    apps: &rustc_hash::FxHashMap<u64, u64>,
    images: &rustc_hash::FxHashMap<Handle, Handle>,
) -> (usize, std::collections::HashMap<Handle, Handle>) {
    let new_blocks: Vec<_> = deps
        .blocks
        .iter()
        .filter(|d| !doc.block_records.contains(&d.name))
        .collect();
    let created = merge_block_defs(doc, &deps.blocks, apps, images);
    let records = merge_mleader_styles(doc, deps);
    for def in new_blocks {
        if let Some(record) = doc.block_records.get(&def.name).map(|r| r.handle) {
            for member in crate::scene::block_authoring::members(doc, record) {
                if let Some(entity) = doc.get_entity_mut(member) {
                    remap_mleader(entity, &records);
                    remap_insert_attributes(entity, &records);
                }
            }
        }
    }
    (created, records)
}

/// Recreate block definitions the pasted INSERTs reference but the target
/// document lacks. Shared by `merge_clipboard_blocks` and
/// `paste_entities_kernel`. Returns how many definitions were created, so
/// render-cache callers can bump once instead of per definition.
/// `images` maps source image definitions to this drawing's (see
/// [`merge_image_definitions`]), for raster images inside the blocks.
pub(crate) fn merge_block_defs(
    doc: &mut document::Document,
    blocks: &[super::BlockDef],
    apps: &rustc_hash::FxHashMap<u64, u64>,
    images: &rustc_hash::FxHashMap<Handle, Handle>,
) -> usize {
    let mut created = Vec::new();
    for def in blocks {
        let before = doc.block_records.len();
        let mut entities = def.entities.clone();
        for entity in &mut entities {
            remap_eed_apps(entity, apps);
            remap_image_definition(entity, images);
        }
        define_block_raw_doc(doc, &def.name, def.base_point, &entities);
        if doc.block_records.len() > before {
            created.push(def);
        }
    }
    // Graphs and table cells name other records (an evaluated copy names its
    // definition), so they are re-pointed once every record exists.
    let records = block_record_map(doc, blocks);
    for def in &created {
        if let Some(record) = doc.block_records.get(&def.name).map(|r| r.handle) {
            for member in crate::scene::block_authoring::members(doc, record) {
                if let Some(document::EntityType::Table(table)) = doc.get_entity_mut(member) {
                    remap_table_cell_blocks(table, &records);
                }
            }
        }
        import_block_def_graph(doc, def, &records);
    }
    created.len()
}

/// Source record → this drawing's record of the same name, per definition.
pub(crate) fn block_record_map(
    doc: &document::Document,
    blocks: &[super::BlockDef],
) -> std::collections::HashMap<Handle, Handle> {
    blocks
        .iter()
        .filter_map(|def| Some((def.source, doc.block_records.get(&def.name)?.handle)))
        .collect()
}

/// Carry `def`'s object graph (a dynamic block's parameters and states) onto
/// the record just defined from it here. `records` maps the source records
/// the graph may name.
pub(crate) fn import_block_def_graph(
    doc: &mut document::Document,
    def: &super::BlockDef,
    records: &std::collections::HashMap<Handle, Handle>,
) {
    let Some(graph) = &def.graph else {
        return;
    };
    let Some(record) = doc.block_records.get(&def.name).map(|record| record.handle) else {
        return;
    };
    let members = crate::scene::block_authoring::members(doc, record);
    if members.len() != def.entities.len() {
        return;
    }
    let mut map = records.clone();
    map.extend(def.entities.iter().map(|e| e.common().handle).zip(members));
    doc.import_block_graph(graph, record, &map);
    crate::scene::block_authoring::retag_members(doc, record);
}

/// (`NULL` where an add failed) plus the source-handle → pasted-handle map.
///
/// Document-level work lives here; Scene/render concerns stay caller-side in
/// `finalize_paste` (tessellation bumps, `populate_missing_meshes`,
/// `recreate_groups`, `duplicate_parametric_constraints_for`,
/// `sync_displayed_annotation_context`, undo/dirty/selection/echo/panels).
/// See the per-helper report in `finalize_paste`.
/// Where pasted entities go, the way `Scene::add_entity` routes a new one.
#[derive(Clone, Copy)]
pub(crate) enum PasteSpace<'a> {
    Model,
    /// A layout's paper space, when no viewport of it is active.
    Layout(&'a str),
    /// The block definition being edited (BEDIT).
    Block(Handle),
}

/// Recreate the records, image definitions, block definitions and
/// multileader styles `deps` names that `doc` lacks, with fresh handles;
/// existing names are kept. Shared by the paste and DESIGN CENTER.
/// Returns the APPID, image definition and record maps the pasted
/// entities are remapped with.
pub(crate) fn merge_named_content(
    doc: &mut document::Document,
    deps: &super::ClipboardDeps,
) -> (
    rustc_hash::FxHashMap<u64, u64>,
    rustc_hash::FxHashMap<Handle, Handle>,
    std::collections::HashMap<Handle, Handle>,
) {
    // Table records first; the dimension styles they add are re-pointed at
    // their arrow blocks once the block merge has recreated those.
    let added_dim_styles = merge_table_records(doc, deps);
    let apps = eed_app_map(doc, deps);
    let image_definitions = merge_image_definitions(doc, deps);
    let (_, records) = merge_blocks_and_mleader_styles(doc, deps, &apps, &image_definitions);
    repoint_added_dim_style_blocks(doc, deps, &added_dim_styles);
    (apps, image_definitions, records)
}

pub(crate) fn paste_entities_kernel(
    doc: &mut document::Document,
    entities: &[document::EntityType],
    deps: &super::ClipboardDeps,
    translate: Option<&crate::entities::transform::EntityTransform>,
    space: PasteSpace,
) -> (Vec<Handle>, rustc_hash::FxHashMap<Handle, Handle>) {
    let (apps, image_definitions, records) = merge_named_content(doc, deps);

    let by_index: Vec<Handle> = entities
        .iter()
        .cloned()
        .map(|mut entity| {
            remap_eed_apps(&mut entity, &apps);
            remap_image_definition(&mut entity, &image_definitions);
            remap_mleader(&mut entity, &records);
            remap_insert_attributes(&mut entity, &records);
            if let document::EntityType::Table(table) = &mut entity {
                remap_table_cell_blocks(table, &records);
            }
            let picture_placement = crate::scene::render_graph::dim_picture_placement(&entity);
            if let Some(t) = translate {
                crate::scene::view::dispatch::apply_transform(&mut entity, t);
            }
            crate::scene::render_graph::clear_dim_picture_placement(&mut entity);
            // Same `*D`-block re-point as `finalize_paste` (#290, #161): the
            // baked dimension geometry lives in WCS, so the paste gets its own
            // transformed copy of the snapshotted block.
            if let document::EntityType::Dimension(d) = &entity {
                let bn = d.base().block_name.clone();
                if !bn.trim().is_empty() {
                    if let Some(subs) = deps
                        .dim_blocks
                        .iter()
                        .find(|b| b.name.eq_ignore_ascii_case(&bn))
                        .map(|def| def.entities.clone())
                    {
                        let mut subs = subs;
                        if let Some(placement) = &picture_placement {
                            for sub in &mut subs {
                                crate::scene::view::dispatch::apply_transform(sub, placement);
                            }
                        }
                        let bt = translate.cloned().unwrap_or(
                            crate::entities::transform::EntityTransform::Translate(glam::DVec3::ZERO),
                        );
                        if let Some(new_bn) = define_transformed_block_doc(doc, &subs, &bt) {
                            if let document::EntityType::Dimension(d) = &mut entity {
                                d.base_mut().block_name = new_bn;
                            }
                        }
                    }
                }
            }
            let source = entity.common().handle;
            let is_table = matches!(entity, document::EntityType::Table(_));
            let pasted = add_entity_clone_doc(doc, entity, space);
            // A pasted table gets its own clones of its cell fields, taken
            // from the clipboard (the source may be gone or in another
            // drawing).
            if is_table && !pasted.is_null() {
                crate::scene::table_fields::adopt_table_fields(
                    doc,
                    pasted,
                    source,
                    &deps.table_fields,
                );
            }
            pasted
        })
        .collect();

    // Port of `merge_clipboard_ext_objects` minus the trailing `bump_entities`
    // (render cache — caller-side): recreate each captured xdictionary graph
    // with fresh handles and re-point the pasted entity at its new root.
    let annotation_delta = match translate {
        Some(crate::entities::transform::EntityTransform::Translate(delta)) => *delta,
        _ => glam::DVec3::ZERO,
    };
    for cap in &deps.ext_objects {
        let Some(&new_entity) = by_index.get(cap.entity_index) else {
            continue;
        };
        if new_entity.is_null() {
            continue;
        }
        if let Some(attr_index) = cap.attribute_index {
            let Some(new_attr_handle) = (match doc.get_entity(new_entity) {
                Some(document::EntityType::Insert(ins)) => {
                    ins.attributes.get(attr_index).map(|a| a.common.handle)
                }
                _ => None,
            }) else {
                continue;
            };
            if let Some(new_root) =
                recreate_ext_subtree(doc, cap, Some(new_attr_handle), &records)
            {
                let copied_attr = match doc.get_entity_mut(new_entity) {
                    Some(document::EntityType::Insert(ins)) => {
                        if let Some(att) = ins.attributes.get_mut(attr_index) {
                            att.common.xdictionary_handle = Some(new_root);
                            Some(att.clone())
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some(attribute) = copied_attr {
                    crate::scene::annotative::translate_insert_attribute_contexts(
                        doc,
                        &attribute,
                        annotation_delta,
                    );
                }
            }
            continue;
        }
        if let Some(new_root) = recreate_ext_subtree(doc, cap, Some(new_entity), &records) {
            if let Some(e) = doc.get_entity_mut(new_entity) {
                e.common_mut().xdictionary_handle = Some(new_root);
            }
            crate::scene::annotative::translate_annotation_contexts(
                doc,
                new_entity,
                annotation_delta,
            );
        }
    }

    // Source handles map one-to-one to the freshly pasted handles; reconnect
    // LEADER -> copied annotation through the map (the
    // `sync_displayed_annotation_context` refresh stays caller-side).
    let mut handle_map: rustc_hash::FxHashMap<Handle, Handle> = rustc_hash::FxHashMap::default();
    for (source, &copied) in entities.iter().zip(by_index.iter()) {
        if !copied.is_null() {
            handle_map.insert(source.common().handle, copied);
        }
    }
    // A pasted associative hatch follows the pasted boundary, or drops the
    // association when the boundary was not pasted with it. (#1370)
    crate::scene::remap_hatch_associations(doc, &handle_map);
    // Reactors on the source drawing's objects do not travel with the paste.
    crate::scene::remap_copied_reactors(doc, &handle_map);
    // A pasted clipped viewport is clipped by the pasted boundary.
    crate::scene::remap_copied_viewport_clips(doc, &handle_map, true);
    let leader_links: Vec<(Handle, Handle)> = entities
        .iter()
        .filter_map(|source| {
            let document::EntityType::Leader(leader) = source else {
                return None;
            };
            let copied_leader = handle_map.get(&source.common().handle).copied()?;
            let copied_annotation = handle_map
                .get(&leader.annotation_handle)
                .copied()
                .unwrap_or(Handle::NULL);
            Some((copied_leader, copied_annotation))
        })
        .collect();
    for (leader_handle, annotation_handle) in leader_links {
        if let Some(document::EntityType::Leader(leader)) = doc.get_entity_mut(leader_handle) {
            leader.annotation_handle = annotation_handle;
        }
    }
    (by_index, handle_map)
}

/// Document-level clone-add: the `&mut Document` half of
/// `Scene::add_entity_clone` — fresh inline sub-handles
/// (`Scene::reset_clone_subhandles`), NULL top-level/owner handles, then
/// adds it to `space`. Tessellation/render caching stays caller-side in the
/// Scene shell.
pub(crate) fn add_entity_clone_doc(
    doc: &mut document::Document,
    mut entity: document::EntityType,
    space: PasteSpace,
) -> Handle {
    crate::scene::Scene::reset_clone_subhandles(doc, &mut entity);
    entity.common_mut().handle = Handle::NULL;
    entity.common_mut().owner_handle = Handle::NULL;
    let handle = match space {
        PasteSpace::Model => doc.add_entity(entity),
        PasteSpace::Layout(layout) => doc.add_entity_to_layout(entity, layout),
        PasteSpace::Block(block) => {
            entity.common_mut().owner_handle = block;
            doc.add_entity(entity)
        }
    }
    .unwrap_or(Handle::NULL);
    if !handle.is_null() {
        crate::scene::Scene::adopt_attributes(doc, handle);
    }
    handle
}

/// Document-level fresh `*D<n>` block from `subs`: the `&mut Document`
/// half of `Scene::define_transformed_block` (which has no render-cache tail,
/// so this is a verbatim port). Returns the new block name, or `None` when
/// `subs` is empty.
pub(crate) fn define_transformed_block_doc(
    doc: &mut document::Document,
    subs: &[document::EntityType],
    t: &crate::entities::transform::EntityTransform,
) -> Option<String> {
    if subs.is_empty() {
        return None;
    }
    let mut n = 0u64;
    let new_name = loop {
        let cand = format!("*D{n}");
        if doc.block_records.get(&cand).is_none() {
            break cand;
        }
        n += 1;
    };
    let next = doc.next_handle();
    let br_handle = Handle::new(next);
    let block_handle = Handle::new(next + 1);
    let mut br = document::tables::BlockRecord::new(&new_name);
    br.handle = br_handle;
    br.block_entity_handle = block_handle;
    // See `Scene::define_transformed_block`.
    br.flags.anonymous = true;
    doc.block_records.add(br).ok()?;
    let mut block = document::entities::Block::new(&new_name, document::types::Vector3::ZERO);
    block.common.handle = block_handle;
    block.common.owner_handle = br_handle;
    doc.add_entity(document::EntityType::Block(block)).ok()?;
    for sub in subs {
        let mut sub = sub.clone();
        crate::scene::view::dispatch::apply_transform(&mut sub, t);
        add_entity_clone_doc_inner(doc, &mut sub, br_handle);
        let _ = doc.add_entity(sub);
    }
    let end_handle = doc.allocate_handle();
    let mut block_end = document::entities::BlockEnd::new();
    block_end.common.handle = end_handle;
    block_end.common.owner_handle = br_handle;
    doc.add_entity(document::EntityType::BlockEnd(block_end))
        .ok()?;
    if let Some(record) = doc.block_records.get_mut(&new_name) {
        record.block_end_handle = end_handle;
    }
    Some(new_name)
}

/// COPYCLIP kernel: clone the selected entities out of `doc` and snapshot
/// the table records / block definitions / xdictionary graphs they depend on,
/// so a later paste can recreate them (including cross-drawing). Clipboard
/// storage (`clipboard`, `clipboard_base`, `clipboard_deps`) and the echo stay
/// caller-side in `copy_entities_to_clipboard` / `handle_copy_to_clipboard`.
pub(crate) fn copy_to_clipboard_kernel(
    doc: &document::Document,
    handles: &[Handle],
) -> (Vec<document::EntityType>, super::ClipboardDeps) {
    let entities: Vec<_> = handles
        .iter()
        .filter_map(|&handle| doc.get_entity(handle).cloned())
        .collect();
    let deps = super::ClipboardDeps::capture(doc, &entities);
    (entities, deps)
}

pub(crate) fn recreate_ext_subtree(
    doc: &mut document::Document,
    cap: &super::ClipExtObjects,
    entity_handle: Option<Handle>,
    records: &std::collections::HashMap<Handle, Handle>,
) -> Option<Handle> {
    use std::collections::HashMap;
    let mut remap: HashMap<Handle, Handle> = records.clone();
    if let Some(eh) = entity_handle {
        remap.insert(cap.src_entity_handle, eh);
    }
    for (old, scale) in &cap.annotation_scales {
        let target = crate::scene::annotative::ensure_scale_object(doc, scale);
        remap.insert(*old, target);
    }
    for (old, _) in &cap.objects {
        remap.insert(*old, doc.allocate_handle());
    }
    for (old, obj) in &cap.objects {
        let mut obj = obj.clone();
        let new_h = remap[old];
        remap_object(&mut obj, new_h, &remap);
        // A dynamic reference's link to its definition, indexed.
        if let document::objects::ObjectType::DynamicBlock(object) = &obj {
            if let document::objects::DynamicBlockData::Representation(link) = &object.data {
                if object.dxf_name == "ACDB_BLOCKREPRESENTATION_DATA" {
                    doc.block_representations.insert(new_h, link.block);
                }
            }
        }
        doc.objects.insert(new_h, obj);
    }
    remap.get(&cap.root).copied()
}

/// Replace references to a clipboard entity inside one recreated extension
/// dictionary graph after its final block-owned handle becomes known.
pub(crate) fn remap_ext_subtree_reference(
    doc: &mut document::Document,
    root: Handle,
    source_entity: Handle,
    target_entity: Handle,
) {
    use document::objects::ObjectType;
    use rustc_hash::FxHashSet;
    use std::collections::HashMap;

    let remap = HashMap::from([(source_entity, target_entity)]);
    let mut seen = FxHashSet::default();
    let mut pending = vec![root];
    while let Some(handle) = pending.pop() {
        if handle.is_null() || !seen.insert(handle) {
            continue;
        }
        let children = match doc.objects.get(&handle) {
            Some(ObjectType::Dictionary(dictionary)) => {
                let mut children: Vec<_> =
                    dictionary.entries.iter().map(|(_, child)| *child).collect();
                if let Some(extension) = dictionary.xdictionary_handle {
                    children.push(extension);
                }
                children
            }
            Some(ObjectType::DictionaryWithDefault(dictionary)) => {
                let mut children: Vec<_> =
                    dictionary.entries.iter().map(|(_, child)| *child).collect();
                children.push(dictionary.default_handle);
                children
            }
            _ => Vec::new(),
        };
        pending.extend(children);
        if let Some(mut object) = doc.objects.remove(&handle) {
            remap_object(&mut object, handle, &remap);
            doc.objects.insert(handle, object);
        }
    }
}

/// Rewrite a cloned extension-dictionary object onto fresh handles: set its own
/// handle to `new_handle` and remap its owner and any handle references it holds
/// through `remap` (a handle still in the source space stays unchanged, which is
/// correct for cross-references that point outside the captured subtree).
pub(crate) fn remap_object(
    obj: &mut document::objects::ObjectType,
    new_handle: document::Handle,
    remap: &std::collections::HashMap<document::Handle, document::Handle>,
) {
    use document::objects::ObjectType;
    let map = |h: document::Handle| remap.get(&h).copied().unwrap_or(h);
    match obj {
        ObjectType::Dictionary(d) => {
            d.handle = new_handle;
            d.owner = map(d.owner);
            for (_, h) in d.entries.iter_mut() {
                *h = map(*h);
            }
            if let Some(x) = d.xdictionary_handle.as_mut() {
                *x = map(*x);
            }
            for r in d.reactors.iter_mut() {
                *r = map(*r);
            }
        }
        ObjectType::DictionaryWithDefault(d) => {
            d.handle = new_handle;
            d.owner = map(d.owner);
            for (_, h) in d.entries.iter_mut() {
                *h = map(*h);
            }
            d.default_handle = map(d.default_handle);
        }
        ObjectType::DictionaryVariable(v) => {
            v.handle = new_handle;
            v.owner_handle = map(v.owner_handle);
        }
        ObjectType::SpatialFilter(s) => {
            s.handle = new_handle;
            s.owner = map(s.owner);
        }
        ObjectType::XRecord(x) => {
            x.handle = new_handle;
            x.owner = map(x.owner);
            for r in x.reactors.iter_mut() {
                *r = map(*r);
            }
            if let Some(d) = x.xdictionary_handle.as_mut() {
                *d = map(*d);
            }
            for entry in &mut x.entries {
                if let document::objects::XRecordValue::Handle(handle) = &mut entry.value {
                    *handle = map(*handle);
                }
            }
            // The DWG writer can emit these instead of the entries.
            for reference in &mut x.object_references {
                reference.handle = map(reference.handle);
            }
            // The verbatim DWG record still carries the source handles.
            x.raw_dwg_data = None;
        }
        ObjectType::TableContent(t) => {
            t.common.handle = new_handle;
            t.common.owner_handle = map(t.common.owner_handle);
            for r in t.common.reactors.iter_mut() {
                *r = map(*r);
            }
            if let Some(d) = t.common.xdictionary_handle.as_mut() {
                *d = map(*d);
            }
            remap_table_cell_blocks(t, remap);
        }
        ObjectType::Group(g) => {
            g.handle = new_handle;
            g.owner = map(g.owner);
            for h in g.entities.iter_mut() {
                *h = map(*h);
            }
        }
        ObjectType::ObjectContextData(context) => {
            context.handle = new_handle;
            context.owner_handle = map(context.owner_handle);
            for reactor in &mut context.reactors {
                *reactor = map(*reactor);
            }
            if let Some(dictionary) = &mut context.xdictionary_handle {
                *dictionary = map(*dictionary);
            }
            context.scale = map(context.scale);
            match &mut context.kind {
                document::objects::ObjectContextKind::Dim(dimension) => {
                    dimension.block = map(dimension.block);
                }
                document::objects::ObjectContextKind::HatchView(hatch) => {
                    hatch.view = map(hatch.view);
                }
                document::objects::ObjectContextKind::MTextAttribute(attribute) => {
                    if let Some(embedded) = &mut attribute.context {
                        embedded.owner_handle = map(embedded.owner_handle);
                        for reactor in &mut embedded.reactors {
                            *reactor = map(*reactor);
                        }
                        if let Some(dictionary) = &mut embedded.xdictionary_handle {
                            *dictionary = map(*dictionary);
                        }
                        embedded.scale = map(embedded.scale);
                    }
                }
                document::objects::ObjectContextKind::MLeader(mleader) => {
                    if let Some(handle) = &mut mleader.text_style_handle {
                        *handle = map(*handle);
                    }
                    if let Some(handle) = &mut mleader.block_content_handle {
                        *handle = map(*handle);
                    }
                    if let Some(handle) = &mut mleader.scale_handle {
                        *handle = map(*handle);
                    }
                    for root in &mut mleader.leader_roots {
                        for line in &mut root.lines {
                            if let Some(handle) = &mut line.line_type_handle {
                                *handle = map(*handle);
                            }
                            if let Some(handle) = &mut line.arrowhead_handle {
                                *handle = map(*handle);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        ObjectType::DynamicBlock(object) => {
            object.handle = new_handle;
            object.visit_handles_mut(&mut |h| *h = map(*h));
        }
        // A FIELD under ACAD_FIELD: the paste evaluates and edits its own
        // field tree, so its handle, owner and nested fields move with it.
        ObjectType::Field(f) => {
            f.handle = new_handle;
            f.owner = map(f.owner);
            for child in &mut f.child_fields {
                *child = map(*child);
            }
            for r in &mut f.referenced_objects {
                *r = map(*r);
            }
        }
        // Other leaf object kinds don't appear in an entity xdictionary; if one
        // does, it's inserted with the fresh handle below via the caller's key,
        // but its internal owner is left as-is (best effort).
        _ => {}
    }
}

pub(crate) fn sync_mleaderstyle_dictionary(doc: &mut document::Document) {
    let root_h = crate::scene::annotative::root_named_dict_handle(doc);

    let existing = match doc.objects.get(&root_h) {
        Some(ObjectType::Dictionary(root)) => root
            .entries
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(MLEADERSTYLE_DICT_NAME))
            .map(|(_, handle)| *handle),
        _ => None,
    };

    let dict_h = match existing
        .filter(|handle| matches!(doc.objects.get(handle), Some(ObjectType::Dictionary(_))))
    {
        Some(handle) => handle,
        None => {
            let handle = doc.allocate_handle();

            let mut dict = Dictionary::new();
            dict.handle = handle;
            dict.owner = root_h;

            doc.objects.insert(handle, ObjectType::Dictionary(dict));

            handle
        }
    };

    if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root_h) {
        // An existing entry keeps its place: the order is saved with the
        // drawing, and an untouched Apply must not change it.
        match root
            .entries
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case(MLEADERSTYLE_DICT_NAME))
        {
            Some(entry) => entry.1 = dict_h,
            None => root.add_entry(MLEADERSTYLE_DICT_NAME, dict_h),
        }
    }

    let mut entries: Vec<(String, Handle)> = doc
        .objects
        .iter()
        .filter_map(|(&handle, object)| match object {
            ObjectType::MultiLeaderStyle(style) => Some((style.name.clone(), handle)),
            _ => None,
        })
        .collect();

    entries.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    for (_, handle) in &entries {
        if let Some(ObjectType::MultiLeaderStyle(style)) = doc.objects.get_mut(handle) {
            style.owner_handle = dict_h;
        }
    }

    if let Some(ObjectType::Dictionary(dict)) = doc.objects.get_mut(&dict_h) {
        dict.owner = root_h;
        dict.entries = entries;

        // MLEADERSTYLE uses soft-owner entries (group 350).
        dict.hard_owner = false;
        dict.hard_owner_entries.clear();
    }
}

pub(crate) const MLEADERSTYLE_DICT_NAME: &str = "ACAD_MLEADERSTYLE";
