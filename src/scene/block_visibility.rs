//! Visibility states of a dynamic block, authored in the Block Editor and
//! stored the way the format keeps them: a BLOCKVISIBILITYPARAMETER in the
//! block's `ACAD_ENHANCEDBLOCK` evaluation graph (with its grip and the
//! grip's two location expressions), `AcDbBlockRepETag` tags on the
//! definition and on each of its objects, and, behind every reference that
//! shows a state other than the definition's, an evaluated `*U` copy tagged
//! `AcDbBlockRepBTag` and linked back by `AcDbBlockRepresentationData`.
//!
//! The definition itself shows the first state, so a reference to it needs
//! no copy until another state is picked for it.

use codec::objects::{
    BlockElement, BlockEvalExpression, BlockEvalValue, BlockEvaluationEdge, BlockEvaluationGraph,
    BlockEvaluationNode, BlockGrip, BlockGripExpression, BlockParameterPropertyInfo,
    BlockRepresentationData, BlockVisibilityParameter, BlockVisibilityState, DynamicBlockData,
    DynamicBlockObject, ObjectType,
};
use codec::types::Vector3;
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{CadDocument, EntityType, Handle};

use super::Scene;

const ELEMENT_TAG: &str = "AcDbBlockRepETag";
const REPRESENTATION_TAG: &str = "AcDbBlockRepBTag";
const TRUE_NAME: &str = "AcDbDynamicBlockTrueName";
const GUID: &str = "AcDbDynamicBlockGUID";

/// Object version the parameter, grip and expressions carry.
const MAJOR: i32 = 33;
const MINOR: i32 = 329;
/// Value code of an expression that holds no value.
const NO_VALUE: i16 = -9999;

/// (DXF name, class, DWG maintenance version) of the classes used here.
const CLASSES: &[(&str, &str, i32)] = &[
    ("ACAD_EVALUATION_GRAPH", "AcDbEvalGraph", 55),
    (
        "BLOCKVISIBILITYPARAMETER",
        "AcDbBlockVisibilityParameter",
        55,
    ),
    ("BLOCKVISIBILITYGRIP", "AcDbBlockVisibilityGrip", 55),
    ("BLOCKGRIPLOCATIONCOMPONENT", "AcDbBlockGripExpr", 20),
    (
        "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION",
        "AcDbDynamicBlockPurgePreventer",
        61,
    ),
    (
        "ACDB_BLOCKREPRESENTATION_DATA",
        "AcDbBlockRepresentationData",
        58,
    ),
];

/// Register the classes with the flags and versions the reference writes
/// for them; a reader takes a class registered otherwise for a proxy.
fn register_classes(document: &mut CadDocument) {
    for &(dxf_name, cpp_class_name, maintenance_version) in CLASSES {
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

fn ensure_app_id(document: &mut CadDocument, name: &str) {
    if !document.app_ids.contains(name) {
        let mut app = codec::tables::AppId::new(name);
        app.handle = document.allocate_handle();
        let _ = document.app_ids.add(app);
    }
}

fn expression(node_id: i32, value_code: i16, value: BlockEvalValue) -> BlockEvalExpression {
    BlockEvalExpression {
        parent_id: BlockEvalExpression::NO_PARENT,
        major: MAJOR,
        minor: MINOR,
        value_code,
        value,
        node_id,
    }
}

fn dynamic(
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

/// The visibility parameter of block definition `definition`.
pub fn parameter(document: &CadDocument, definition: Handle) -> Option<&BlockVisibilityParameter> {
    document.block_visibility_param_for_def(definition)
}

/// Store an edited parameter in the document and its lookup view.
fn store(document: &mut CadDocument, parameter: BlockVisibilityParameter) {
    document
        .block_visibility_params
        .insert(parameter.handle, parameter.clone());
    document.objects.insert(
        parameter.handle,
        ObjectType::BlockVisibilityParameter(parameter),
    );
}

/// The objects of a definition, block begin and end left out.
fn members(document: &CadDocument, definition: Handle) -> Vec<Handle> {
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

fn element_tag(id: i32, handle: Handle) -> ExtendedDataRecord {
    let mut tag = ExtendedDataRecord::new(ELEMENT_TAG);
    tag.add_value(XDataValue::Integer16(1));
    tag.add_value(XDataValue::Integer32(id));
    tag.add_value(XDataValue::Handle(handle));
    tag
}

fn element_id(entity: &EntityType) -> Option<i32> {
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

fn set_entity_tag(document: &mut CadDocument, handle: Handle, record: ExtendedDataRecord) {
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

/// Give every object of the definition its element tag, numbering the new
/// ones after the highest id in use, and record the next id on the block.
fn tag_members(document: &mut CadDocument, definition: Handle) {
    let members = members(document, definition);
    let mut next = members
        .iter()
        .filter_map(|handle| document.get_entity(*handle).and_then(element_id))
        .max()
        .map_or(0, |id| id + 1);
    for handle in members {
        let id = match document.get_entity(handle).and_then(element_id) {
            Some(id) => id,
            None => {
                next += 1;
                next - 1
            }
        };
        set_entity_tag(document, handle, element_tag(id, handle));
    }
    let mut tag = ExtendedDataRecord::new(ELEMENT_TAG);
    tag.add_value(XDataValue::Integer16(1));
    tag.add_value(XDataValue::Integer32(next));
    document.set_object_xdata_record(definition, tag);
}

/// A registry-format GUID for a new dynamic block.
fn new_guid() -> String {
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

/// Make block definition `definition` dynamic with a visibility parameter
/// whose grip sits at `location`, one state showing every object. Returns
/// the parameter, or the one the block has already.
pub fn create(document: &mut CadDocument, definition: Handle, location: Vector3) -> Option<Handle> {
    if let Some(existing) = parameter(document, definition) {
        return Some(existing.handle);
    }
    let name = document
        .block_records
        .iter()
        .find(|record| record.handle == definition)?
        .name
        .clone();
    register_classes(document);
    for app in [ELEMENT_TAG, REPRESENTATION_TAG, TRUE_NAME, GUID] {
        ensure_app_id(document, app);
    }
    let dictionary = document.ensure_extension_dictionary(definition);
    let [graph, parameter, grip, x, y, preventer] = [(); 6].map(|_| document.allocate_handle());

    store(
        document,
        BlockVisibilityParameter {
            handle: parameter,
            owner: graph,
            eval_parent_id: BlockEvalExpression::NO_PARENT,
            eval_major: MAJOR,
            eval_minor: MINOR,
            eval_value_code: NO_VALUE,
            eval_value: BlockEvalValue::None,
            eval_node_id: 1,
            element_name: "Visibility State".to_string(),
            element_major: MAJOR,
            element_minor: MINOR,
            element_eed_1071: 0,
            show_properties: true,
            chain_actions: false,
            name: "Visibility1".to_string(),
            description: String::new(),
            def_point: location,
            property_info: [
                BlockParameterPropertyInfo::default(),
                BlockParameterPropertyInfo::default(),
            ],
            property_info_count: 2,
            is_initialized: true,
            unknown_bool: false,
            all_blocks: Vec::new(),
            states: vec![BlockVisibilityState {
                name: "VisibilityState0".to_string(),
                ..Default::default()
            }],
        },
    );
    let grip_object = dynamic(
        grip,
        graph,
        "BLOCKVISIBILITYGRIP",
        "AcDbBlockVisibilityGrip",
        DynamicBlockData::VisibilityGrip(BlockGrip {
            element: BlockElement {
                eval: expression(2, NO_VALUE, BlockEvalValue::None),
                name: "Grip".to_string(),
                major: MAJOR,
                minor: MINOR,
                eed_1071: 0,
            },
            flags_91: 3,
            flags_92: 4,
            location,
            insert_cycling: false,
            insert_cycling_weight: -1,
        }),
    );
    document
        .objects
        .insert(grip, ObjectType::DynamicBlock(grip_object));
    for (handle, node, axis) in [(x, 3, "UpdatedX"), (y, 4, "UpdatedY")] {
        let component = dynamic(
            handle,
            graph,
            "BLOCKGRIPLOCATIONCOMPONENT",
            "AcDbBlockGripExpr",
            DynamicBlockData::GripLocationComponent(BlockGripExpression {
                eval: expression(node, 40, BlockEvalValue::Real(0.0)),
                grip_type: 1,
                expression: axis.to_string(),
            }),
        );
        document
            .objects
            .insert(handle, ObjectType::DynamicBlock(component));
    }
    // The grip drives the parameter, whose position the two expressions
    // read: node 0 the parameter, 1 the grip, 2 and 3 the expressions.
    let node = |id: i32, expression: Handle, node_data: [i32; 4]| BlockEvaluationNode {
        id,
        edge_flags: 32,
        next_id: id + 1,
        expression,
        node_data,
        active_cycles: None,
    };
    let edge =
        |id: i32, source_node: i32, destination_node: i32, outgoing_edges| BlockEvaluationEdge {
            id,
            next_id: 0,
            incoming_edge: 1,
            source_node,
            destination_node,
            outgoing_edges,
        };
    let mut graph_object = dynamic(
        graph,
        dictionary,
        "ACAD_EVALUATION_GRAPH",
        "AcDbEvalGraph",
        DynamicBlockData::EvaluationGraph(BlockEvaluationGraph {
            first_node_id: 4,
            first_node_id_copy: 4,
            nodes: vec![
                node(0, parameter, [0, 0, 1, 2]),
                node(1, grip, [-1, -1, 0, 0]),
                node(2, x, [1, 1, -1, -1]),
                node(3, y, [2, 2, -1, -1]),
            ],
            edges: vec![
                edge(0, 1, 0, [-1; 5]),
                edge(1, 0, 2, [-1, -1, -1, 2, -1]),
                edge(2, 0, 3, [-1, -1, 1, -1, -1]),
            ],
        }),
    );
    graph_object.reactors.push(dictionary);
    document
        .objects
        .insert(graph, ObjectType::DynamicBlock(graph_object));
    let mut preventer_object = dynamic(
        preventer,
        dictionary,
        "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION",
        "AcDbDynamicBlockPurgePreventer",
        DynamicBlockData::Representation(BlockRepresentationData {
            flags: 1,
            block: definition,
        }),
    );
    preventer_object.reactors.push(dictionary);
    document
        .objects
        .insert(preventer, ObjectType::DynamicBlock(preventer_object));
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
        dictionary.add_entry("ACAD_ENHANCEDBLOCK", graph);
        dictionary.add_entry("AcDbDynamicBlockRoundTripPurgePreventer", preventer);
    }

    let mut true_name = ExtendedDataRecord::new(TRUE_NAME);
    true_name.add_value(XDataValue::String(name));
    document.set_object_xdata_record(definition, true_name);
    let mut guid = ExtendedDataRecord::new(GUID);
    guid.add_value(XDataValue::String(new_guid()));
    document.set_object_xdata_record(definition, guid);
    tag_members(document, definition);
    Some(parameter)
}

/// The names of the states of `parameter`, in list order.
pub fn state_names(document: &CadDocument, parameter: Handle) -> Vec<String> {
    match document.objects.get(&parameter) {
        Some(ObjectType::BlockVisibilityParameter(parameter)) => parameter
            .states
            .iter()
            .map(|state| state.name.clone())
            .collect(),
        _ => Vec::new(),
    }
}

fn edit<R>(
    document: &mut CadDocument,
    parameter: Handle,
    change: impl FnOnce(&mut BlockVisibilityParameter) -> R,
) -> Option<R> {
    let Some(ObjectType::BlockVisibilityParameter(current)) = document.objects.get(&parameter)
    else {
        return None;
    };
    let mut edited = current.clone();
    let result = change(&mut edited);
    store(document, edited);
    Some(result)
}

/// Add state `name`, showing what state `like` shows. Returns its index, or
/// `None` when the name is taken.
pub fn add_state(
    document: &mut CadDocument,
    parameter: Handle,
    name: &str,
    like: usize,
) -> Option<usize> {
    edit(document, parameter, |parameter| {
        if parameter
            .states
            .iter()
            .any(|state| state.name.eq_ignore_ascii_case(name))
        {
            return None;
        }
        let visible_blocks = parameter
            .states
            .get(like)
            .map(|state| state.visible_blocks.clone())
            .unwrap_or_default();
        parameter.states.push(BlockVisibilityState {
            name: name.to_string(),
            visible_blocks,
            visible_params: Vec::new(),
        });
        Some(parameter.states.len() - 1)
    })
    .flatten()
}

/// Rename state `index`; false when the new name is taken.
pub fn rename_state(
    document: &mut CadDocument,
    parameter: Handle,
    index: usize,
    name: &str,
) -> bool {
    edit(document, parameter, |parameter| {
        let taken = parameter
            .states
            .iter()
            .enumerate()
            .any(|(other, state)| other != index && state.name.eq_ignore_ascii_case(name));
        match parameter.states.get_mut(index) {
            Some(state) if !taken => {
                state.name = name.to_string();
                true
            }
            _ => false,
        }
    })
    .unwrap_or(false)
}

/// Delete state `index`; the last state stays.
pub fn delete_state(document: &mut CadDocument, parameter: Handle, index: usize) -> bool {
    edit(document, parameter, |parameter| {
        if parameter.states.len() <= 1 || index >= parameter.states.len() {
            return false;
        }
        parameter.states.remove(index);
        true
    })
    .unwrap_or(false)
}

/// Show (`visible`) or hide `handles` in state `index`. An object the
/// parameter did not govern yet is shown in every other state, as it was.
pub fn set_visible(
    document: &mut CadDocument,
    parameter: Handle,
    index: usize,
    handles: &[Handle],
    visible: bool,
) {
    let definition = owner_definition(document, parameter);
    edit(document, parameter, |parameter| {
        for &handle in handles {
            if !parameter.all_blocks.contains(&handle) {
                parameter.all_blocks.push(handle);
                for state in &mut parameter.states {
                    state.visible_blocks.push(handle);
                }
            }
            let Some(state) = parameter.states.get_mut(index) else {
                continue;
            };
            state.visible_blocks.retain(|other| *other != handle);
            if visible {
                state.visible_blocks.push(handle);
            }
        }
    });
    if let Some(definition) = definition {
        tag_members(document, definition);
    }
}

/// The block definition `parameter` belongs to (graph, dictionary, record).
fn owner_definition(document: &CadDocument, parameter: Handle) -> Option<Handle> {
    let graph = match document.objects.get(&parameter)? {
        ObjectType::BlockVisibilityParameter(parameter) => parameter.owner,
        _ => return None,
    };
    let dictionary = match document.objects.get(&graph)? {
        ObjectType::DynamicBlock(graph) => graph.owner,
        _ => return None,
    };
    match document.objects.get(&dictionary)? {
        ObjectType::Dictionary(dictionary) => Some(dictionary.owner),
        _ => None,
    }
}

/// Show the definition's objects as state `index` does.
pub fn apply_state(document: &mut CadDocument, parameter: Handle, index: usize) {
    let Some(ObjectType::BlockVisibilityParameter(current)) = document.objects.get(&parameter)
    else {
        return;
    };
    let Some(state) = current.states.get(index) else {
        return;
    };
    let shown: Vec<(Handle, bool)> = current
        .all_blocks
        .iter()
        .map(|handle| (*handle, state.visible_blocks.contains(handle)))
        .collect();
    for (handle, visible) in shown {
        if let Some(entity) = document.get_entity_mut(handle) {
            entity.common_mut().invisible = !visible;
        }
    }
}

/// The state whose objects are the ones the definition shows now.
pub fn shown_state(document: &CadDocument, parameter: Handle) -> usize {
    let Some(ObjectType::BlockVisibilityParameter(parameter)) = document.objects.get(&parameter)
    else {
        return 0;
    };
    parameter
        .states
        .iter()
        .position(|state| {
            parameter.all_blocks.iter().all(|handle| {
                let shown = document
                    .get_entity(*handle)
                    .is_some_and(|entity| !entity.common().invisible);
                shown == state.visible_blocks.contains(handle)
            })
        })
        .unwrap_or(0)
}

/// After the Block Editor: drop objects that were erased, tag new ones and
/// leave the definition showing its first state.
pub fn finish_editing(document: &mut CadDocument, definition: Handle) {
    let Some(handle) = parameter(document, definition).map(|parameter| parameter.handle) else {
        return;
    };
    let live: Vec<Handle> = members(document, definition);
    edit(document, handle, |parameter| {
        parameter.all_blocks.retain(|handle| live.contains(handle));
        for state in &mut parameter.states {
            state.visible_blocks.retain(|handle| live.contains(handle));
        }
    });
    tag_members(document, definition);
    apply_state(document, handle, 0);
}

impl Scene {
    /// Give a reference to a dynamic definition its own evaluated copy, so a
    /// state picked for it leaves the definition and the other references
    /// alone. Returns the copy's name; a reference already showing a copy,
    /// or not dynamic, keeps its block and gets `None`.
    pub fn visibility_representation(&mut self, insert: Handle) -> Option<String> {
        let Some(EntityType::Insert(reference)) = self.document.get_entity(insert) else {
            return None;
        };
        let definition = self
            .document
            .block_records
            .get(&reference.block_name)?
            .handle;
        parameter(&self.document, definition)?;
        let name = {
            let next = self
                .document
                .block_records
                .iter()
                .filter_map(|record| record.name.strip_prefix("*U")?.parse::<u64>().ok())
                .max()
                .map_or(1, |n| n + 1);
            format!("*U{next}")
        };
        let base = self
            .document
            .block_records
            .iter()
            .find(|record| record.handle == definition)
            .and_then(
                |record| match self.document.get_entity(record.block_entity_handle) {
                    Some(EntityType::Block(block)) => Some(block.base_point),
                    _ => None,
                },
            )
            .unwrap_or(Vector3::ZERO);
        let originals = members(&self.document, definition);
        let copies: Vec<EntityType> = originals
            .iter()
            .filter_map(|handle| self.document.get_entity(*handle).cloned())
            .collect();
        self.define_block_raw(&name, base, copies);
        let record = self.document.block_records.get_mut(&name)?;
        record.flags.anonymous = true;
        let copy = record.handle;
        // The copies keep their element ids, now naming themselves.
        for handle in members(&self.document, copy) {
            if let Some(id) = self.document.get_entity(handle).and_then(element_id) {
                set_entity_tag(&mut self.document, handle, element_tag(id, handle));
            }
        }
        let mut tag = ExtendedDataRecord::new(REPRESENTATION_TAG);
        tag.add_value(XDataValue::Integer16(1));
        tag.add_value(XDataValue::Handle(definition));
        self.document.set_object_xdata_record(copy, tag);
        self.document.ensure_extension_dictionary(copy);

        // The reference: AcDbBlockRepresentation / AcDbRepData names the
        // definition it was evaluated from.
        let holder = self.document.ensure_extension_dictionary(insert);
        let representation = self.document.allocate_handle();
        let data = self.document.allocate_handle();
        let mut dictionary = codec::objects::Dictionary::new();
        dictionary.handle = representation;
        dictionary.owner = holder;
        dictionary.reactors.push(holder);
        dictionary.add_entry("AcDbRepData", data);
        self.document
            .objects
            .insert(representation, ObjectType::Dictionary(dictionary));
        if let Some(ObjectType::Dictionary(holder)) = self.document.objects.get_mut(&holder) {
            holder.add_entry("AcDbBlockRepresentation", representation);
        }
        let mut link = dynamic(
            data,
            representation,
            "ACDB_BLOCKREPRESENTATION_DATA",
            "AcDbBlockRepresentationData",
            DynamicBlockData::Representation(BlockRepresentationData {
                flags: 1,
                block: definition,
            }),
        );
        link.reactors.push(representation);
        self.document
            .objects
            .insert(data, ObjectType::DynamicBlock(link));
        self.document.block_representations.insert(data, definition);
        if let Some(EntityType::Insert(reference)) = self.document.get_entity_mut(insert) {
            reference.block_name = name.clone();
        }
        for record in self.document.block_records.iter_mut() {
            if record.handle == definition {
                record.insert_handles.retain(|handle| *handle != insert);
            } else if record.handle == copy {
                record.insert_handles.push(insert);
            }
        }
        self.bump_geometry();
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{Circle, Insert, Line};

    /// A block given visibility states round-trips them, and a reference
    /// given its own copy still finds the definition and its states.
    #[test]
    fn states_and_a_reference_copy_survive_dwg_and_dxf() {
        let mut scene = Scene::new();
        scene.define_block_raw(
            "door",
            Vector3::ZERO,
            vec![
                EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 1.0, 0.0, 0.0)),
                EntityType::Circle(Circle::from_center_radius(
                    Vector3::new(0.5, 0.5, 0.0),
                    0.25,
                )),
            ],
        );
        let definition = scene.document.block_records.get("door").unwrap().handle;
        let parameter = create(&mut scene.document, definition, Vector3::ZERO).unwrap();
        let open = add_state(&mut scene.document, parameter, "Open", 0).unwrap();
        let circle = members(&scene.document, definition)[1];
        set_visible(&mut scene.document, parameter, open, &[circle], false);
        finish_editing(&mut scene.document, definition);

        let insert = scene.add_entity(EntityType::Insert(Insert::new("door", Vector3::ZERO)));
        let copy = scene.visibility_representation(insert).expect("a copy");
        assert!(copy.starts_with("*U"));

        for ext in ["dwg", "dxf"] {
            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|error| panic!("save {ext}: {error}"));
            let read = crate::io::load_bytes(&format!("door.{ext}"), bytes)
                .unwrap_or_else(|error| panic!("load {ext}: {error}"));
            let insert = read
                .entities()
                .find_map(|entity| match entity {
                    EntityType::Insert(insert) if insert.block_name.starts_with("*U") => {
                        Some(insert.common.handle)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{ext}: reference to the copy"));
            let (found, parameter) = read
                .dynamic_visibility_for_insert(insert)
                .unwrap_or_else(|| panic!("{ext}: no visibility parameter"));
            let record = read
                .block_records
                .iter()
                .find(|r| r.handle == found)
                .unwrap();
            assert_eq!(record.name, "door", "{ext}");
            let names: Vec<&str> = parameter.states.iter().map(|s| s.name.as_str()).collect();
            assert_eq!(names, ["VisibilityState0", "Open"], "{ext}");
            assert_eq!(parameter.states[1].visible_blocks.len(), 0, "{ext}");
        }
    }
}
