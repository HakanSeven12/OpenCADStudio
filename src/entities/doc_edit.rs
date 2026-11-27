//! Edits on the document that need no scene: writing a block definition,
//! keeping a definition's record flags true to its body, and giving a copied
//! entity fresh sub-handles.

use document::{EntityType, Handle};

/// Document-level verbatim block-definition recreate: the `&mut Document`
/// half of `Scene::define_block_raw` (no-op when the block exists). The
/// trailing `bump_geometry` stays caller-side.
pub(crate) fn define_block_raw_doc(
    doc: &mut document::Document,
    name: &str,
    base_point: document::types::Vector3,
    entities: &[document::EntityType],
) {
    if name.is_empty() || doc.block_records.get(name).is_some() {
        return;
    }
    let next = doc.next_handle();
    let br_handle = Handle::new(next);
    let block_handle = Handle::new(next + 1);
    let mut block_record = document::tables::BlockRecord::new(name);
    block_record.flags.anonymous =
        name.starts_with('*') && !(block_record.is_model_space() || block_record.is_paper_space());
    block_record.handle = br_handle;
    block_record.block_entity_handle = block_handle;
    if doc.block_records.add(block_record).is_err() {
        return;
    }
    let mut block = document::entities::Block::new(name, base_point);
    block.common.handle = block_handle;
    block.common.owner_handle = br_handle;
    let _ = doc.add_entity(document::EntityType::Block(block));
    for mut entity in entities.iter().cloned() {
        add_entity_clone_doc_inner(doc, &mut entity, br_handle);
        let _ = doc.add_entity(entity);
    }
    // The terminator is allocated after the body so handle order matches
    // record order inside the definition.
    let end_handle = doc.allocate_handle();
    let mut block_end = document::entities::BlockEnd::new();
    block_end.common.handle = end_handle;
    block_end.common.owner_handle = br_handle;
    let _ = doc.add_entity(document::EntityType::BlockEnd(block_end));
    if let Some(record) = doc.block_records.get_mut(name) {
        record.block_end_handle = end_handle;
    }
    sync_attribute_flag(doc, name);
}

/// Shared sub-handle reset + owner stamp for block-definition members.
pub(crate) fn add_entity_clone_doc_inner(
    doc: &mut document::Document,
    entity: &mut document::EntityType,
    owner: Handle,
) {
    reset_clone_subhandles(doc, entity);
    entity.common_mut().handle = Handle::NULL;
    entity.common_mut().owner_handle = owner;
    // An extension dictionary is exclusively owned; the clone must not claim
    // the source's (cross-drawing it would dangle into the source document),
    // or file validation reports "Extension dictionary <H> Cannot access".
    // Paste reattaches a recreated subtree afterwards (finalize_paste).
    entity.common_mut().xdictionary_handle = None;
}

/// Say on the record whether its own body declares attribute definitions.
/// A DXF file writes the definition's "has attribute definitions" marker
/// from the record flag alone, so the record has to state what its body
/// actually holds: read back from the body every time a body is written —
/// by the block editor, by a definition replace, by a pasted definition's
/// twin — rather than left to whichever command set it first.
pub(crate) fn sync_attribute_flag(document: &mut document::Document, name: &str) {
    let holds_definitions = document
        .entities_in_block(name)
        .any(|entity| matches!(entity, EntityType::AttributeDefinition(_)));
    if let Some(record) = document.block_records.get_mut(name) {
        record.flags.has_attributes = holds_definitions;
    }
}

/// Give a freshly-cloned entity brand-new handles for every *inline*
/// sub-entity that stores one (INSERT attributes and SEQEND, 3D-polyline
/// vertices, polyface vertices / faces / SEQEND, polygon-mesh vertices).
/// `document.add_entity` only assigns the top-level handle, and the DWG
/// writer reuses every non-null stored sub-handle, so without this a copy
/// writes its source's sub-handles again — duplicate handles that corrupt
/// the saved DWG (other CAD apps attach the copies to the source's
/// vertices, or drop them). Vertices that don't store a handle
/// (LwPolyline / heavy 2D polyline) get one from the writer, so they need
/// no fix-up here. Shared with the clipboard paste path. (#129)
pub(crate) fn reset_clone_subhandles(doc: &mut document::Document, entity: &mut EntityType) {
    match entity {
        EntityType::Table(table) => {
            // A table exclusively owns its anonymous picture block. The
            // codec also resolves by name, so detach both references.
            // A table drawn from its cells (no handle, the name kept)
            // keeps the name as that mark: the scene gives the copy a
            // block of its own and goes on drawing it from its cells.
            let from_cells =
                table.block_record_handle.is_none() && !table.block_name.is_empty();
            table.block_record_handle = None;
            if !from_cells {
                table.block_name.clear();
            }
        }
        EntityType::Insert(ins) => {
            for att in ins.attributes.iter_mut() {
                att.common.handle = doc.allocate_handle();
                att.common.xdictionary_handle = None;
                // The text of a multiline attribute is an object of its
                // own inside the reference, so the copy needs a fresh
                // handle for it and its new attribute as owner.
                let owner = att.common.handle;
                if let Some(model) = &mut att.embedded_mtext {
                    model.common.handle = doc.allocate_handle();
                    model.common.owner_handle = owner;
                }
            }
            if ins.seqend_handle.is_some() {
                ins.seqend_handle = Some(doc.allocate_handle());
            }
        }
        EntityType::Polyline3D(p) => {
            for v in p.vertices.iter_mut() {
                v.handle = doc.allocate_handle();
            }
        }
        EntityType::PolyfaceMesh(m) => {
            for v in m.vertices.iter_mut() {
                v.common.handle = doc.allocate_handle();
            }
            for f in m.faces.iter_mut() {
                f.common.handle = doc.allocate_handle();
            }
            if m.seqend_handle.is_some() {
                m.seqend_handle = Some(doc.allocate_handle());
            }
        }
        EntityType::PolygonMesh(m) => {
            for v in m.vertices.iter_mut() {
                v.common.handle = doc.allocate_handle();
            }
        }
        _ => {}
    }
}
