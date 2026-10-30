//! Shared native graph construction for Block Editor authoring objects.
pub(crate) mod graph;
pub(crate) mod edit;
pub(crate) mod metadata;
use codec::objects::{
    BlockEvalExpression, BlockEvalValue, DynamicBlockData, DynamicBlockObject, ObjectType,
};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Document, EntityType, Handle};

pub(super) const MAJOR: i32 = 33;
pub(super) const MINOR: i32 = 329;
pub(super) const NO_VALUE: i16 = -9999;
pub(super) const ELEMENT_TAG: &str = "AcDbBlockRepETag";
pub(super) const REPRESENTATION_TAG: &str = "AcDbBlockRepBTag";
pub(super) const TRUE_NAME: &str = "AcDbDynamicBlockTrueName";
pub(super) const GUID: &str = "AcDbDynamicBlockGUID";

#[derive(Clone, Copy)]
pub(super) struct Slots {
    pub node: i32,
    pub eval: i32,
    pub edge: i32,
}

type PublishedOwners = Vec<(usize, usize, std::sync::Arc<rustc_hash::FxHashSet<Handle>>)>;

/// Documents whose dictionary owners are published: (address, guards, owners).
static PUBLISHED: std::sync::RwLock<PublishedOwners> = std::sync::RwLock::new(Vec::new());

/// The owners of every dictionary in a document, published for as long as
/// this guard borrows it, so nothing can change them meanwhile.
///
/// Resolving a block's extension dictionary scans every object when the block
/// has none (most blocks), and every INSERT placement asks for its block's
/// base point: a drawing with half a million entities spent minutes there
/// (#2023). Batch walks publish the owners once; [`definition_dictionary`]
/// then answers "none" for an unowned block straight away, on any thread.
pub(crate) struct DictionaryOwners<'a> {
    address: usize,
    _document: std::marker::PhantomData<&'a Document>,
}

impl<'a> DictionaryOwners<'a> {
    pub(crate) fn publish(document: &'a Document) -> Self {
        let address = document as *const Document as usize;
        let mut published = PUBLISHED.write().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = published.iter_mut().find(|entry| entry.0 == address) {
            entry.1 += 1;
        } else {
            let owners = document
                .objects
                .values()
                .filter_map(|object| match object {
                    ObjectType::Dictionary(dictionary) => Some(dictionary.owner),
                    _ => None,
                })
                .collect();
            published.push((address, 1, std::sync::Arc::new(owners)));
        }
        Self {
            address,
            _document: std::marker::PhantomData,
        }
    }
}

impl Drop for DictionaryOwners<'_> {
    fn drop(&mut self) {
        let mut published = PUBLISHED.write().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = published.iter().position(|entry| entry.0 == self.address) {
            published[at].1 -= 1;
            if published[at].1 == 0 {
                published.swap_remove(at);
            }
        }
    }
}

/// A block definition's extension dictionary. Skips the whole-document scan
/// [`Document::extension_dictionary_handle`] falls back to when the
/// document's dictionary owners are published and none is `definition`.
pub(crate) fn definition_dictionary(document: &Document, definition: Handle) -> Option<Handle> {
    let address = document as *const Document as usize;
    let unowned = PUBLISHED
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|entry| entry.0 == address)
        .is_some_and(|entry| !entry.2.contains(&definition));
    if unowned {
        return None;
    }
    document.extension_dictionary_handle(definition)
}

/// Resolve a graph without touching it. Reject malformed/opaque relationships
/// before opening a history transaction or allocating any new native objects.
pub(super) fn graph(
    document: &Document,
    definition: Handle,
) -> Result<Option<Handle>, &'static str> {
    if !document
        .block_records
        .iter()
        .any(|record| record.handle == definition)
    {
        return Err("The block definition is no longer available.");
    }
    let Some(dictionary) = definition_dictionary(document, definition) else {
        return Ok(None);
    };
    let Some(ObjectType::Dictionary(record)) = document.objects.get(&dictionary) else {
        return Err("The block extension dictionary is unavailable.");
    };
    let Some(handle) = record.get("ACAD_ENHANCEDBLOCK") else {
        return Ok(None);
    };
    let Some(ObjectType::DynamicBlock(object)) = document.objects.get(&handle) else {
        return Err("The block authoring graph cannot be edited.");
    };
    let DynamicBlockData::EvaluationGraph(graph) = &object.data else {
        return Err("The block authoring graph cannot be edited.");
    };
    if object.owner != dictionary {
        return Err("The block authoring graph has an invalid owner.");
    }
    let nodes: rustc_hash::FxHashSet<_> = graph.nodes.iter().map(|node| node.id).collect();
    let edges: rustc_hash::FxHashSet<_> = graph.edges.iter().map(|edge| edge.id).collect();
    if nodes.len() != graph.nodes.len()
        || edges.len() != graph.edges.len()
        || graph
            .nodes
            .iter()
            .any(|node| node.id < 0 || document.objects.get(&node.expression).is_none())
        || graph.edges.iter().any(|edge| {
            edge.id < 0
                || !nodes.contains(&edge.source_node)
                || !nodes.contains(&edge.destination_node)
        })
    {
        return Err("The block authoring graph has invalid node relationships.");
    }
    slots(Some(graph))?;
    Ok(Some(handle))
}

fn slots(graph: Option<&codec::objects::BlockEvaluationGraph>) -> Result<Slots, &'static str> {
    let node = graph
        .and_then(|graph| graph.nodes.iter().map(|node| node.id).max())
        .unwrap_or(-1)
        .checked_add(1);
    let eval = graph
        .map(|graph| {
            graph.first_node_id.max(graph.first_node_id_copy).max(
                graph
                    .nodes
                    .iter()
                    .map(|node| node.next_id)
                    .max()
                    .unwrap_or(0),
            )
        })
        .unwrap_or(0)
        .checked_add(1);
    let edge = graph
        .and_then(|graph| graph.edges.iter().map(|edge| edge.id).max())
        .unwrap_or(-1)
        .checked_add(1);
    match (node, eval, edge) {
        (Some(node), Some(eval), Some(edge))
            if node.checked_add(3).is_some()
                && eval.checked_add(3).is_some()
                && edge.checked_add(2).is_some() =>
        {
            Ok(Slots { node, eval, edge })
        }
        _ => Err("The block authoring graph has no free node IDs."),
    }
}

/// Prepare an existing graph for appending, or create the native graph and
/// definition metadata once. Existing nodes, edges, GUIDs and relationships stay.
pub(super) fn prepare(
    document: &mut Document,
    definition: Handle,
) -> Result<(Handle, Slots), &'static str> {
    if let Some(handle) = graph(document, definition)? {
        let ObjectType::DynamicBlock(object) = &document.objects[&handle] else {
            unreachable!()
        };
        let DynamicBlockData::EvaluationGraph(graph) = &object.data else {
            unreachable!()
        };
        return Ok((handle, slots(Some(graph))?));
    }
    register_classes(
        document,
        &[
            ("ACAD_EVALUATION_GRAPH", "AcDbEvalGraph", 55),
            (
                "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION",
                "AcDbDynamicBlockPurgePreventer",
                61,
            ),
        ],
    );
    for app in [ELEMENT_TAG, REPRESENTATION_TAG, TRUE_NAME, GUID] {
        ensure_app_id(document, app);
    }
    let name = document
        .block_records
        .iter()
        .find(|record| record.handle == definition)
        .unwrap()
        .name
        .clone();
    let dictionary = document.ensure_extension_dictionary(definition);
    let handle = document.allocate_handle();
    let preventer = document.allocate_handle();
    let mut object = dynamic(
        handle,
        dictionary,
        "ACAD_EVALUATION_GRAPH",
        "AcDbEvalGraph",
        DynamicBlockData::EvaluationGraph(Default::default()),
    );
    object.reactors.push(dictionary);
    document
        .objects
        .insert(handle, ObjectType::DynamicBlock(object));
    let mut object = dynamic(
        preventer,
        dictionary,
        "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION",
        "AcDbDynamicBlockPurgePreventer",
        DynamicBlockData::Representation(codec::objects::BlockRepresentationData {
            flags: 1,
            block: definition,
        }),
    );
    object.reactors.push(dictionary);
    document
        .objects
        .insert(preventer, ObjectType::DynamicBlock(object));
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
        dictionary.add_entry("ACAD_ENHANCEDBLOCK", handle);
        dictionary.add_entry("AcDbDynamicBlockRoundTripPurgePreventer", preventer);
    }
    let mut name_record = ExtendedDataRecord::new(TRUE_NAME);
    name_record.add_value(XDataValue::String(name));
    document.set_object_xdata_record(definition, name_record);
    let mut guid = ExtendedDataRecord::new(GUID);
    guid.add_value(XDataValue::String(new_guid()));
    document.set_object_xdata_record(definition, guid);
    tag_members(document, definition);
    Ok((handle, slots(None)?))
}

/// Preflight the larger subgraph needed by parameters with state expressions.
pub(super) fn capacity(
    document: &Document,
    definition: Handle,
    nodes: i32,
    edges: i32,
) -> Result<(), &'static str> {
    let graph = graph(document, definition)?.and_then(|handle| match &document.objects[&handle] {
        ObjectType::DynamicBlock(object) => match &object.data {
            DynamicBlockData::EvaluationGraph(graph) => Some(graph),
            _ => None,
        },
        _ => None,
    });
    let start = slots(graph)?;
    if start.node.checked_add(nodes - 1).is_none()
        || start.eval.checked_add(nodes - 1).is_none()
        || start.edge.checked_add(edges - 1).is_none()
    {
        return Err("The block authoring graph has no free node IDs.");
    }
    Ok(())
}

/// A parameter, its single grip and two position expressions. Supports one-point
/// parameters and two-point parameters configured with one end grip.
/// Graph node IDs, expression IDs and edge IDs are separate namespaces.
pub(super) fn append(
    document: &mut Document,
    handle: Handle,
    slots: Slots,
    expressions: [Handle; 4],
) {
    append_components(document, handle, slots, &expressions, &[2, 3]);
}

/// Basepoint parameters and action objects can be standalone expressions.
pub(super) fn append_single(
    document: &mut Document,
    handle: Handle,
    slots: Slots,
    expression: Handle,
) {
    let ObjectType::DynamicBlock(object) = document.objects.get_mut(&handle).unwrap() else {
        unreachable!()
    };
    let DynamicBlockData::EvaluationGraph(graph) = &mut object.data else {
        unreachable!()
    };
    graph.nodes.push(codec::objects::BlockEvaluationNode {
        id: slots.node,
        next_id: slots.eval,
        expression,
        edge_flags: 32,
        node_data: [-1; 4],
        active_cycles: None,
    });
    graph.first_node_id = slots.eval;
    graph.first_node_id_copy = slots.eval;
}

/// Append a parameter and grip, followed by its location/state expressions.
/// `components` gives the expression offsets in native dependency order.
pub(super) fn append_components(
    document: &mut Document,
    handle: Handle,
    slots: Slots,
    expressions: &[Handle],
    components: &[usize],
) {
    use codec::objects::{BlockEvaluationEdge, BlockEvaluationNode};
    let ObjectType::DynamicBlock(object) = document.objects.get_mut(&handle).unwrap() else {
        unreachable!()
    };
    let DynamicBlockData::EvaluationGraph(graph) = &mut object.data else {
        unreachable!()
    };
    let n = slots.node;
    let e = slots.edge;
    let mut links = vec![[-1; 4]; expressions.len()];
    links[0] = [e, e, e + 1, e + components.len() as i32];
    links[1] = [-1, -1, e, e];
    for (index, component) in components.iter().enumerate() {
        let edge = e + 1 + index as i32;
        links[*component] = [edge, edge, -1, -1];
    }
    for (offset, expression) in expressions.iter().copied().enumerate() {
        graph.nodes.push(BlockEvaluationNode {
            id: n + offset as i32,
            edge_flags: 32,
            next_id: slots.eval + offset as i32,
            expression,
            node_data: links[offset],
            active_cycles: None,
        });
    }
    let dependencies = std::iter::once((0, n + 1, n, [-1; 5])).chain(
        components.iter().enumerate().map(|(index, component)| {
            let offset = index as i32 + 1;
            let previous = if index == 0 { -1 } else { e + offset - 1 };
            let next = if index + 1 == components.len() {
                -1
            } else {
                e + offset + 1
            };
            (
                offset,
                n,
                n + *component as i32,
                [-1, -1, previous, next, -1],
            )
        }),
    );
    for (offset, source_node, destination_node, outgoing_edges) in dependencies {
        graph.edges.push(BlockEvaluationEdge {
            id: e + offset,
            next_id: 0,
            incoming_edge: 1,
            source_node,
            destination_node,
            outgoing_edges,
        });
    }
    graph.first_node_id = slots.eval + (expressions.len() as i32 - 1);
    graph.first_node_id_copy = graph.first_node_id;
}

pub(super) fn register_classes(document: &mut Document, classes: &[(&str, &str, i32)]) {
    for &(dxf_name, cpp_class_name, maintenance_version) in classes {
        if let Some(existing) = document.classes.get_mut_by_name(dxf_name) {
            existing.proxy_flags = codec::classes::ProxyFlags(1153);
            existing.dwg_version = 25;
            existing.maintenance_version = maintenance_version;
            continue;
        }
        document.classes.add_or_update(codec::classes::DxfClass {
            dxf_name: dxf_name.to_string(),
            cpp_class_name: cpp_class_name.to_string(),
            application_name: "ObjectDBX Classes".to_string(),
            proxy_flags: codec::classes::ProxyFlags(1153),
            instance_count: 0,
            was_zombie: false,
            is_an_entity: false,
            class_number: 0,
            item_class_id: 0x1F3,
            dwg_version: 25,
            maintenance_version,
            unknown1: 0,
            unknown2: 0,
        });
    }
}

pub(super) fn ensure_app_id(document: &mut Document, name: &str) {
    if !document.app_ids.contains(name) {
        let mut app = codec::tables::AppId::new(name);
        app.handle = document.allocate_handle();
        let _ = document.app_ids.add(app);
    }
}

pub(super) fn expression(
    node_id: i32,
    value_code: i16,
    value: BlockEvalValue,
) -> BlockEvalExpression {
    BlockEvalExpression {
        parent_id: BlockEvalExpression::NO_PARENT,
        major: MAJOR,
        minor: MINOR,
        value_code,
        value,
        node_id,
    }
}

pub(super) fn dynamic(
    handle: Handle,
    owner: Handle,
    dxf_name: &str,
    cpp_class_name: &str,
    data: DynamicBlockData,
) -> DynamicBlockObject {
    let mut object = DynamicBlockObject::new(dxf_name, cpp_class_name);
    object.handle = handle;
    object.owner = owner;
    object.data = data;
    object
}

/// `definition`'s object graph, for a copy of the definition to carry. A
/// draw-order table or associative network beside a dynamic block's graph
/// holds objects the export cannot move, and one of them used to drop the
/// whole graph, parameters and actions included; leave those two behind
/// instead.
pub(crate) fn export_graph(
    document: &Document,
    definition: Handle,
) -> Option<codec::BlockGraph> {
    if let Some(graph) = document.export_block_graph(definition) {
        return Some(graph);
    }
    let root = document.extension_dictionary_handle(definition)?;
    let Some(ObjectType::Dictionary(dictionary)) = document.objects.get(&root) else {
        return None;
    };
    if !dictionary.entries.iter().any(|(key, _)| key == "ACAD_ENHANCEDBLOCK") {
        return None;
    }
    let left = |key: &str| matches!(key, "ACAD_SORTENTS" | "ACAD_ASSOCNETWORK");
    let dropped: Vec<Handle> = dictionary
        .entries
        .iter()
        .filter(|(key, _)| left(key))
        .map(|(_, handle)| *handle)
        .collect();
    // Other graph objects may still name them (reactors), so drop the
    // objects themselves, not just their entries.
    let gone: Vec<Handle> = document
        .objects
        .keys()
        .copied()
        .filter(|handle| dropped.iter().any(|top| document.owner_chain_reaches(*handle, *top)))
        .collect();
    let mut document = document.clone();
    for handle in gone {
        document.objects.remove(&handle);
    }
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&root) {
        dictionary.entries.retain(|(key, _)| !left(key));
    }
    document.export_block_graph(definition)
}

pub(crate) fn members(document: &Document, definition: Handle) -> Vec<Handle> {
    document
        .block_records
        .iter()
        .find(|record| record.handle == definition)
        .map(|record| {
            record
                .entity_handles
                .iter()
                .copied()
                .filter(|handle| {
                    !matches!(
                        document.get_entity(*handle),
                        None | Some(EntityType::Block(_) | EntityType::BlockEnd(_))
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn element_tag(id: i32, handle: Handle) -> ExtendedDataRecord {
    let mut tag = ExtendedDataRecord::new(ELEMENT_TAG);
    tag.add_value(XDataValue::Integer16(1));
    tag.add_value(XDataValue::Integer32(id));
    tag.add_value(XDataValue::Handle(handle));
    tag
}

pub(super) fn element_id(entity: &EntityType) -> Option<i32> {
    entity
        .common()
        .extended_data
        .get_record(ELEMENT_TAG)?
        .values
        .iter()
        .find_map(|value| match value {
            XDataValue::Integer32(id) => Some(*id),
            _ => None,
        })
}

pub(super) fn set_entity_tag(
    document: &mut Document,
    handle: Handle,
    record: ExtendedDataRecord,
) {
    let app = document
        .app_ids
        .get(&record.application_name)
        .map(|app| app.handle.value());
    if let Some(entity) = document.get_entity_mut(handle) {
        let xdata = &mut entity.common_mut().extended_data;
        // The fresh record, not bytes kept from a read, is what gets written.
        if let Some(app) = app {
            xdata.raw_dwg_eed.retain(|(owner, _)| *owner != app);
        }
        xdata.upsert_record(record);
    }
}

/// Point each tagged member's element tag back at the member itself, after
/// the definition was copied onto new handles.
pub(crate) fn retag_members(document: &mut Document, definition: Handle) {
    for handle in members(document, definition) {
        if let Some(id) = document.get_entity(handle).and_then(element_id) {
            let tagged = document.get_entity(handle).is_some_and(|entity| {
                element_id(entity) == Some(id)
                    && entity
                        .common()
                        .extended_data
                        .get_record(ELEMENT_TAG)
                        .is_some_and(|record| {
                            record
                                .values
                                .iter()
                                .any(|v| matches!(v,XDataValue::Handle(h) if *h==handle))
                        })
            });
            if !tagged {
                set_entity_tag(document, handle, element_tag(id, handle));
            }
        }
    }
}

/// A copied definition owns its identity even when its evaluation IDs are retained.
pub(crate) fn refresh_copied_identity(document: &mut Document, definition: Handle, name: &str) {
    if !matches!(graph(document, definition), Ok(Some(_))) {
        return;
    }
    let mut true_name = ExtendedDataRecord::new(TRUE_NAME);
    true_name.add_value(XDataValue::String(name.to_owned()));
    document.set_object_xdata_record(definition, true_name);
    let mut guid = ExtendedDataRecord::new(GUID);
    guid.add_value(XDataValue::String(new_guid()));
    document.set_object_xdata_record(definition, guid);
    retag_members(document, definition);
}

/// Give every object of the definition its element tag, numbering the new
/// ones after the highest id in use, and record the next id on the block.
pub(super) fn tag_members(document: &mut Document, definition: Handle) {
    let members = members(document, definition);
    let stored_next = document
        .object_xdata(definition)
        .and_then(|data| data.get_record(ELEMENT_TAG))
        .and_then(|record| {
            record.values.iter().find_map(|v| match v {
                XDataValue::Integer32(n) => Some(*n),
                _ => None,
            })
        })
        .unwrap_or(0);
    let mut next = members
        .iter()
        .filter_map(|handle| document.get_entity(*handle).and_then(element_id))
        .max()
        .map_or(0, |id| id.saturating_add(1))
        .max(stored_next);
    let mut checked = std::collections::BTreeSet::new();
    let needs_ids = members.iter().any(|h| {
        document.get_entity(*h).is_none_or(|e| {
            element_id(e).is_none_or(|id| {
                !checked.insert(id)
                    || !e
                        .common()
                        .extended_data
                        .get_record(ELEMENT_TAG)
                        .is_some_and(|r| {
                            r.values
                                .iter()
                                .any(|v| matches!(v,XDataValue::Handle(link) if link==h))
                        })
            })
        })
    });
    if needs_ids {
        // Undo can restore an older definition counter while evaluated copies
        // still contain newer IDs. Never recycle any of those live identities.
        let representations = document
            .block_records
            .iter()
            .filter(|b| {
                document
                    .object_xdata(b.handle)
                    .and_then(|d| d.get_record(REPRESENTATION_TAG))
                    .is_some_and(|r| {
                        r.values
                            .iter()
                            .any(|v| matches!(v,XDataValue::Handle(h) if *h==definition))
                    })
            })
            .map(|b| b.handle)
            .collect::<std::collections::BTreeSet<_>>();
        if let Some(high) = document
            .entities()
            .filter(|e| representations.contains(&e.common().owner_handle))
            .filter_map(element_id)
            .max()
        {
            next = next.max(high.saturating_add(1));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for handle in members {
        let id = match document.get_entity(handle).and_then(element_id) {
            Some(id)
                if id >= 0
                    && document
                        .get_entity(handle)
                        .unwrap()
                        .common()
                        .extended_data
                        .get_record(ELEMENT_TAG)
                        .is_some_and(|r| {
                            r.values
                                .iter()
                                .any(|v| matches!(v,XDataValue::Handle(h) if *h==handle))
                        })
                    && seen.insert(id) =>
            {
                id
            }
            _ => {
                let id = next;
                next = next.saturating_add(1);
                seen.insert(id);
                id
            }
        };
        let tagged = document.get_entity(handle).is_some_and(|entity| {
            element_id(entity) == Some(id)
                && entity
                    .common()
                    .extended_data
                    .get_record(ELEMENT_TAG)
                    .is_some_and(|record| {
                        record
                            .values
                            .iter()
                            .any(|v| matches!(v,XDataValue::Handle(h) if *h==handle))
                    })
        });
        if !tagged {
            set_entity_tag(document, handle, element_tag(id, handle));
        }
    }
    let mut tag = ExtendedDataRecord::new(ELEMENT_TAG);
    tag.add_value(XDataValue::Integer16(1));
    tag.add_value(XDataValue::Integer32(next));
    if stored_next != next
        || document
            .object_xdata(definition)
            .and_then(|d| d.get_record(ELEMENT_TAG))
            .is_none()
    {
        document.set_object_xdata_record(definition, tag);
    }
}

pub(super) fn new_guid() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        bytes = seed.to_le_bytes();
    }
    let hex: String = bytes.iter().map(|byte| format!("{byte:02X}")).collect();
    format!(
        "{{{}-{}-{}-{}-{}}}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}
