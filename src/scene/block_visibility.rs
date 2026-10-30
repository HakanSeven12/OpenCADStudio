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
    BlockElement, BlockEvalExpression, BlockEvalValue, BlockGrip, BlockGripExpression,
    BlockParameterPropertyInfo, BlockRepresentationData, BlockVisibilityParameter,
    BlockVisibilityState, DynamicBlockData, ObjectType,
};
use codec::types::Vector3;
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Document, EntityType, Handle};

use super::Scene;

use super::block_authoring::*;

impl Scene {
    /// BVMODE and BVSHOW picking reveal only state-controlled members of BEDIT.
    pub(crate) fn visibility_revealed(&self, handle: Handle) -> bool {
        if self.document.get_entity(handle).is_some_and(|entity| super::block_construction::is_construction(entity.common())) {
            return self.construction_revealed(handle);
        }
        self.block_visibility_reveal
            && self.block_edit_block.is_some_and(|definition| {
                self.document.get_entity(handle).is_some_and(|entity| {
                    entity.common().owner_handle == definition && entity.common().invisible
                }) && parameter(&self.document, definition)
                    .is_some_and(|parameter| parameter.all_blocks.contains(&handle))
            })
    }

    pub(crate) fn visibility_reveal_handles(&self) -> rustc_hash::FxHashSet<Handle> {
        let mut handles: rustc_hash::FxHashSet<Handle> = self.block_edit_block
            .and_then(|definition| parameter(&self.document, definition))
            .filter(|_| self.block_visibility_reveal)
            .map(|parameter| {
                parameter
                    .all_blocks
                    .iter()
                    .copied()
                    .filter(|handle| self.visibility_revealed(*handle))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(definition) = self.block_edit_block {
            if let Some(record) = self.document.block_records.iter().find(|record| record.handle == definition) {
                handles.extend(record.entity_handles.iter().copied().filter(|handle| self.construction_revealed(*handle)));
            }
        }
        handles
    }

    pub(crate) fn display_entity_invisible(&self, common: &codec::entities::EntityCommon) -> bool {
        if super::block_construction::is_construction(common) { return !self.construction_revealed(common.handle); }
        common.invisible && !self.visibility_revealed(common.handle)
    }

    pub(crate) fn set_block_visibility_reveal(&mut self, reveal: bool) {
        if self.block_visibility_reveal != reveal {
            self.block_visibility_reveal = reveal;
            self.recolor_meshes();
            self.bump_geometry();
        }
    }
}

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

/// The visibility parameter of block definition `definition`.
pub fn parameter(document: &Document, definition: Handle) -> Option<&BlockVisibilityParameter> {
    document.block_visibility_param_for_def(definition)
}

/// Store an edited parameter in the document and its lookup view.
pub(crate) fn store(document: &mut Document, parameter: BlockVisibilityParameter) {
    document
        .block_visibility_params
        .insert(parameter.handle, parameter.clone());
    document.objects.insert(
        parameter.handle,
        ObjectType::BlockVisibilityParameter(parameter),
    );
}

/// Commit a staged state list while retaining the parameter's graph and identity.
pub(crate) fn replace_states(
    document: &mut Document,
    definition: Handle,
    edited: BlockVisibilityParameter,
) {
    debug_assert_eq!(
        parameter(document, definition).map(|p| p.handle),
        Some(edited.handle)
    );
    store(document, edited);
    tag_members(document, definition);
}

/// Why another visibility parameter cannot be placed in this definition.
/// Existing evaluation graphs must be composed, never replaced by a new graph.
pub fn placement_error(document: &Document, definition: Handle) -> Option<&'static str> {
    if parameter(document, definition).is_some() {
        return Some("This block already has a visibility parameter.");
    }
    super::block_authoring::graph(document, definition).err()
}

/// Make block definition `definition` dynamic with a visibility parameter
/// whose grip sits at `location`, one state showing every object. Returns
/// the parameter, or the one the block has already.
pub fn create(document: &mut Document, definition: Handle, location: Vector3) -> Option<Handle> {
    if let Some(existing) = parameter(document, definition) {
        return Some(existing.handle);
    }
    if placement_error(document, definition).is_some()
        || !location.x.is_finite()
        || !location.y.is_finite()
        || !location.z.is_finite()
    {
        return None;
    }
    let (graph, slots) = super::block_authoring::prepare(document, definition).ok()?;
    register_classes(document, CLASSES);
    let [parameter, grip, x, y] = [(); 4].map(|_| document.allocate_handle());

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
            eval_node_id: slots.eval,
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
            property_info_count: slots.eval + 1,
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
                eval: expression(slots.eval + 1, NO_VALUE, BlockEvalValue::None),
                name: "Grip".to_string(),
                major: MAJOR,
                minor: MINOR,
                eed_1071: 0,
            },
            flags_91: slots.eval + 2,
            flags_92: slots.eval + 3,
            location,
            insert_cycling: false,
            insert_cycling_weight: -1,
        }),
    );
    document
        .objects
        .insert(grip, ObjectType::DynamicBlock(grip_object));
    for (handle, node, axis) in [
        (x, slots.eval + 2, "UpdatedX"),
        (y, slots.eval + 3, "UpdatedY"),
    ] {
        let component = dynamic(
            handle,
            graph,
            "BLOCKGRIPLOCATIONCOMPONENT",
            "AcDbBlockGripExpr",
            DynamicBlockData::GripLocationComponent(BlockGripExpression {
                eval: expression(node, 40, BlockEvalValue::Real(0.0)),
                grip_type: slots.eval,
                expression: axis.to_string(),
            }),
        );
        document
            .objects
            .insert(handle, ObjectType::DynamicBlock(component));
    }
    super::block_authoring::append(document, graph, slots, [parameter, grip, x, y]);
    Some(parameter)
}

/// Carry the parameter and its grip through a change of definition coordinates.
/// Expressions read UpdatedX/UpdatedY from the parameter; their cached scalar
/// values are not geometry and must not receive the point transform.
pub fn reframe(
    document: &mut Document,
    definition: Handle,
    transform: &codec::types::Transform,
) -> usize {
    let Some(mut parameter) = parameter(document, definition).cloned() else {
        return 0;
    };
    parameter.def_point = transform.apply(parameter.def_point);
    let graph = parameter.owner;
    let handle = parameter.handle;
    match document.objects.get_mut(&handle) {
        Some(ObjectType::BlockVisibilityParameter(stored)) => {
            stored.def_point = parameter.def_point
        }
        Some(ObjectType::DynamicBlock(object)) => {
            if let DynamicBlockData::VisibilityParameter(stored) = &mut object.data {
                stored.def_point = parameter.def_point;
            }
        }
        _ => {}
    }
    document.block_visibility_params.insert(handle, parameter);
    let mut changed = 1;
    for object in document.objects.values_mut() {
        if let ObjectType::DynamicBlock(object) = object {
            if object.owner == graph {
                if let DynamicBlockData::VisibilityGrip(grip) = &mut object.data {
                    grip.location = transform.apply(grip.location);
                    changed += 1;
                }
            }
        }
    }
    changed
}

/// The names of the states of `parameter`, in list order.
pub fn state_names(document: &Document, parameter: Handle) -> Vec<String> {
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
    document: &mut Document,
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
    document: &mut Document,
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
    document: &mut Document,
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
pub fn delete_state(document: &mut Document, parameter: Handle, index: usize) -> bool {
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
    document: &mut Document,
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
fn owner_definition(document: &Document, parameter: Handle) -> Option<Handle> {
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
pub fn apply_state(document: &mut Document, parameter: Handle, index: usize) {
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
            entity.common_mut().invisible = !visible || super::block_construction::is_construction(entity.common());
        }
    }
}

/// Per-member visibility of state `index`, in definition member order. A
/// member the parameter does not control stays visible in every state: a
/// state only hides objects the parameter governs and it does not name.
pub fn state_visibility(document: &Document, parameter: Handle, index: usize) -> Vec<bool> {
    let Some(ObjectType::BlockVisibilityParameter(parameter)) = document.objects.get(&parameter)
    else {
        return Vec::new();
    };
    let Some(state) = parameter.states.get(index) else {
        return Vec::new();
    };
    let Some(definition) = owner_definition(document, parameter.handle) else {
        return Vec::new();
    };
    members(document, definition)
        .into_iter()
        .map(|handle| {
            !parameter.all_blocks.contains(&handle)
                || state.visible_blocks.contains(&handle)
        })
        .collect()
}

/// The state whose objects are the ones the definition shows now.
pub fn shown_state(document: &Document, parameter: Handle) -> usize {
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
                if document.get_entity(*handle).is_some_and(|entity| super::block_construction::is_construction(entity.common())) { return true; }
                shown == state.visible_blocks.contains(handle)
            })
        })
        .unwrap_or(0)
}

/// After the Block Editor: drop objects that were erased, tag new ones and
/// leave the definition showing its first state.
pub fn finish_editing(document: &mut Document, definition: Handle) {
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
    /// alone. A copy shared with other references (after COPY) is copied
    /// again with its current state. Returns the copy's name; a reference
    /// already showing its own copy, or not dynamic, gets `None`.
    pub fn visibility_representation(&mut self, insert: Handle) -> Option<String> {
        let definition = self.document.dynamic_definition_for_insert(insert)?;
        parameter(&self.document, definition)?;
        self.dynamic_representation(insert)
    }

    /// Isolate an evaluated dynamic reference, including copies sharing a representation.
    pub(crate) fn dynamic_representation(&mut self, insert: Handle) -> Option<String> {
        let Some(EntityType::Insert(reference)) = self.document.get_entity(insert) else {
            return None;
        };
        let source = self
            .document
            .block_records
            .get(&reference.block_name)?
            .handle;
        let definition = self.document.dynamic_definition_for_insert(insert)?;
        let direct = source == definition;
        let shared = || {
            self.document.entities().any(|entity| {
                matches!(entity, EntityType::Insert(other)
                    if other.common.handle != insert
                        && other.block_name == reference.block_name)
            })
        };
        if !direct && !shared() {
            return None;
        }
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
        let base = super::render_graph::block_base_point(&self.document, &reference.block_name);
        let originals = members(&self.document, source);
        let copies: Vec<EntityType> = originals
            .iter()
            .filter_map(|handle| self.document.get_entity(*handle).cloned())
            .collect();
        self.define_block_raw(&name, base, copies);
        let record = self.document.block_records.get_mut(&name)?;
        record.flags.anonymous = true;
        let copy = record.handle;
        let copies = members(&self.document, copy);
        self.own_copied_xdicts(originals, copies);
        // The copies keep their element ids, now naming themselves.
        super::block_authoring::retag_members(&mut self.document, copy);
        let mut tag = ExtendedDataRecord::new(REPRESENTATION_TAG);
        tag.add_value(XDataValue::Integer16(1));
        tag.add_value(XDataValue::Handle(definition));
        self.document.set_object_xdata_record(copy, tag);
        self.document.ensure_extension_dictionary(copy);
        if !direct {
            // Already linked to its definition; only its block changes.
            self.repoint_representation(insert, source, copy, &name);
            return Some(name);
        }

        // The reference: AcDbBlockRepresentation / AcDbRepData names the
        // definition it was evaluated from.
        let holder = self.document.ensure_extension_dictionary(insert);
        let existing = match self.document.objects.get(&holder) {
            Some(ObjectType::Dictionary(d)) => d
                .entries
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("AcDbBlockRepresentation"))
                .map(|(_, handle)| *handle),
            _ => None,
        };
        let representation = existing.unwrap_or_else(|| self.document.allocate_handle());
        let data = self.document.allocate_handle();
        if existing.is_none() {
            let mut dictionary = codec::objects::Dictionary::new();
            dictionary.handle = representation;
            dictionary.owner = holder;
            dictionary.reactors.push(holder);
            self.document
                .objects
                .insert(representation, ObjectType::Dictionary(dictionary));
            if let Some(ObjectType::Dictionary(holder)) = self.document.objects.get_mut(&holder) {
                holder.add_entry("AcDbBlockRepresentation", representation);
            }
        }
        if let Some(ObjectType::Dictionary(dictionary)) =
            self.document.objects.get_mut(&representation)
        {
            dictionary
                .entries
                .retain(|(key, _)| !key.eq_ignore_ascii_case("AcDbRepData"));
            dictionary.add_entry("AcDbRepData", data);
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
        self.repoint_representation(insert, source, copy, &name);
        Some(name)
    }

    fn repoint_representation(&mut self, insert: Handle, from: Handle, to: Handle, name: &str) {
        if let Some(EntityType::Insert(reference)) = self.document.get_entity_mut(insert) {
            reference.block_name = name.to_string();
        }
        for record in self.document.block_records.iter_mut() {
            if record.handle == from {
                record.insert_handles.retain(|handle| *handle != insert);
            } else if record.handle == to {
                record.insert_handles.push(insert);
            }
        }
        self.bump_geometry();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{Circle, Insert, Line};

    #[test]
    fn visibility_ribbon_reveal_respects_layers_uncontrolled_flags_and_nested_states() {
        let mut scene = Scene::new();
        scene.define_block_raw(
            "nested",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::UNIT_X,
            ))],
        );
        let nested = scene.document.block_records.get("nested").unwrap().handle;
        let child = members(&scene.document, nested)[0];
        scene
            .document
            .get_entity_mut(child)
            .unwrap()
            .common_mut()
            .invisible = true;
        scene.define_block_raw(
            "editor",
            Vector3::ZERO,
            vec![
                EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::UNIT_X)),
                EntityType::Line(Line::from_points(
                    Vector3::UNIT_Y,
                    Vector3::UNIT_X + Vector3::UNIT_Y,
                )),
                EntityType::Insert(Insert::new("nested", Vector3::ZERO)),
            ],
        );
        let definition = scene.document.block_records.get("editor").unwrap().handle;
        let controlled = members(&scene.document, definition)[0];
        let independent = members(&scene.document, definition)[1];
        scene
            .document
            .get_entity_mut(independent)
            .unwrap()
            .common_mut()
            .invisible = true;
        let parameter = create(&mut scene.document, definition, Vector3::ZERO).unwrap();
        set_visible(&mut scene.document, parameter, 0, &[controlled], false);
        apply_state(&mut scene.document, parameter, 0);
        scene.block_edit_block = Some(definition);
        let before = scene.document.clone();
        scene.set_block_visibility_reveal(true);
        let wires = scene.entity_wires();
        assert!(wires
            .iter()
            .any(|wire| Scene::handle_from_wire_name(&wire.name) == Some(controlled)));
        assert!(!scene.visibility_revealed(independent));
        assert!(!scene.visibility_revealed(child));
        assert_eq!(scene.visibility_reveal_handles().len(), 1);
        assert_eq!(
            scene.select_all_visible(),
            2,
            "only revealed line and the insert can be selected"
        );
        let color = scene.count_color(controlled, [1.0; 4], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(color, [0.5, 0.5, 0.5, 1.0]);
        scene.document.layers.get_mut("0").unwrap().flags.off = true;
        scene.bump_geometry();
        assert_eq!(
            scene.select_all_visible(),
            0,
            "mode never overrides a hidden layer"
        );
        scene.document.layers.get_mut("0").unwrap().flags.off = false;
        scene.set_block_visibility_reveal(false);
        assert_eq!(scene.document, before);
        assert!(!scene.visibility_revealed(controlled));
    }

    /// A member the parameter does not control stays visible in every state;
    /// only the objects a state names are hidden when it does not list them.
    #[test]
    fn a_state_hides_only_the_members_the_parameter_controls() {
        let mut scene = Scene::new();
        scene.define_block_raw(
            "opening",
            Vector3::ZERO,
            vec![
                EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 1.0, 0.0, 0.0)),
                EntityType::Line(Line::from_coords(0.0, 1.0, 0.0, 1.0, 1.0, 0.0)),
                EntityType::Line(Line::from_coords(0.0, 2.0, 0.0, 1.0, 2.0, 0.0)),
            ],
        );
        let definition = scene.document.block_records.get("opening").unwrap().handle;
        let parameter = create(&mut scene.document, definition, Vector3::ZERO).unwrap();
        let hidden = add_state(&mut scene.document, parameter, "WindowHidden", 0).unwrap();
        let frame = members(&scene.document, definition)[1];
        set_visible(&mut scene.document, parameter, hidden, &[frame], false);

        let handles = members(&scene.document, definition);
        let shown = state_visibility(&scene.document, parameter, hidden);
        assert_eq!(
            shown,
            vec![true, false, true],
            "the door and cat are not controlled, so the state leaves them visible"
        );
        assert_eq!(state_visibility(&scene.document, parameter, 0), vec![true; 3]);

        apply_state(&mut scene.document, parameter, hidden);
        let visible = |handle: Handle| {
            !scene
                .document
                .get_entity(handle)
                .expect("member")
                .common()
                .invisible
        };
        assert!(visible(handles[0]), "the door stays visible");
        assert!(!visible(handles[1]), "the window frame is hidden");
        assert!(visible(handles[2]), "the cat stays visible");
    }

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

#[cfg(test)]
mod placement_fixtures {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    #[ignore = "requires OCS_DYNAMIC_BLOCK_FIXTURES with local DWG fixtures"]
    fn visibility_parameter_placement_fixture_roundtrips() {
        let directory = std::env::var_os("OCS_DYNAMIC_BLOCK_FIXTURES").expect("fixture directory");
        let mut paths = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("dwg"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        assert!(!paths.is_empty(), "no DWG fixtures found");
        for path in paths {
            let original = std::fs::read(&path).unwrap();
            let digest = Sha256::digest(&original);
            let filename = path.file_name().unwrap().to_string_lossy();
            let document = crate::io::load_bytes(&filename, original).expect(&filename);
            let before = document.clone();
            let mut scene = Scene::new();
            scene.document = document;
            let protected = scene
                .document
                .block_records
                .iter()
                .map(|record| record.handle)
                .filter(|definition| {
                    parameter(&scene.document, *definition).is_none()
                        && placement_error(&scene.document, *definition).is_some()
                })
                .collect::<Vec<_>>();
            for definition in &protected {
                assert!(create(&mut scene.document, *definition, Vector3::ZERO).is_none());
            }
            assert_eq!(
                scene.document, before,
                "{filename}: existing graphs remain untouched"
            );
            scene.define_block_raw(
                "PALETTE_VISIBILITY_TEST",
                Vector3::ZERO,
                vec![EntityType::Line(codec::entities::Line::from_points(
                    Vector3::ZERO,
                    Vector3::UNIT_X,
                ))],
            );
            let definition = scene
                .document
                .block_records
                .get("PALETTE_VISIBILITY_TEST")
                .unwrap()
                .handle;
            let location = Vector3::new(3.25, -4.5, 0.0);
            create(&mut scene.document, definition, location).expect(&filename);
            for (handle, object) in &before.objects {
                assert_eq!(
                    scene.document.objects.get(handle),
                    Some(object),
                    "{filename}: preserved object {handle}"
                );
            }
            for extension in ["dwg", "dxf"] {
                let bytes =
                    crate::io::save_to_bytes(&scene.document, extension, scene.document.version)
                        .expect(&filename);
                let read =
                    crate::io::load_bytes(&format!("placed.{extension}"), bytes).expect(&filename);
                let owner = read
                    .block_records
                    .get("PALETTE_VISIBILITY_TEST")
                    .unwrap()
                    .handle;
                assert_eq!(
                    parameter(&read, owner).expect(&filename).def_point,
                    location,
                    "{filename}: {extension}"
                );
            }
            assert_eq!(
                Sha256::digest(std::fs::read(&path).unwrap()),
                digest,
                "{filename}: source checksum"
            );
            eprintln!("{filename}: DWG/DXF placement roundtrips; {} existing graphs protected; source unchanged", protected.len());
        }
    }
}

#[cfg(test)]
mod representation_xdict_tests {
    use super::*;
    use codec::entities::{Insert, Line};
    use codec::objects::{XRecord, XRecordValue};

    #[test]
    fn visibility_representation_deep_clones_member_xdict() {
        let mut scene = Scene::new();
        scene.define_block_raw(
            "V",
            Vector3::ZERO,
            vec![
                EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::UNIT_X)),
                EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::UNIT_Y)),
            ],
        );
        let definition = scene.document.block_records.get("V").unwrap().handle;
        let originals = members(&scene.document, definition);
        let source = originals[0];
        let root = scene.document.ensure_extension_dictionary(source);
        let record = scene.document.allocate_handle();
        let mut xrecord = XRecord::named("TEST");
        xrecord.handle = record;
        xrecord.owner = root;
        xrecord.reactors.push(root);
        xrecord.add_string(1, "payload");
        xrecord.add_handle(330, root);
        xrecord.add_handle(340, source);
        xrecord.add_handle(350, originals[1]);
        scene
            .document
            .objects
            .insert(record, ObjectType::XRecord(xrecord));
        if let Some(ObjectType::Dictionary(dictionary)) = scene.document.objects.get_mut(&root) {
            dictionary.add_entry("TEST", record);
            dictionary.reactors.push(source);
        }
        let parameter = create(&mut scene.document, definition, Vector3::ZERO).unwrap();
        let other = add_state(&mut scene.document, parameter, "Other", 0).unwrap();
        set_visible(
            &mut scene.document,
            parameter,
            other,
            &[originals[1]],
            false,
        );
        finish_editing(&mut scene.document, definition);
        let insert = scene.add_entity(EntityType::Insert(Insert::new("V", Vector3::ZERO)));
        scene
            .document
            .get_entity_mut(source)
            .unwrap()
            .common_mut()
            .reactors = vec![originals[1], insert];
        let before = scene.document.get_entity(source).unwrap().clone();
        let root_before = scene.document.objects[&root].clone();
        let record_before = scene.document.objects[&record].clone();
        scene.begin_undo_recording();
        let name = scene
            .visibility_representation(insert)
            .expect("anonymous representation");
        assert!(name.starts_with("*U"));
        let copy_definition = scene.document.block_records.get(&name).unwrap().handle;
        let copies = members(&scene.document, copy_definition);
        let copy = scene.document.get_entity(copies[0]).unwrap();
        let copy_root = copy.common().xdictionary_handle.expect("copied dictionary");
        assert_ne!(
            copy_root, root,
            "copy must not share the source extension dictionary"
        );
        assert_eq!(copy.common().reactors, vec![copies[1]]);
        let Some(ObjectType::Dictionary(dictionary)) = scene.document.objects.get(&copy_root)
        else {
            panic!("copied extension dictionary must exist");
        };
        assert_eq!(dictionary.handle, copy_root);
        assert_eq!(dictionary.owner, copies[0]);
        assert_eq!(dictionary.reactors, vec![copies[0]]);
        let copy_record = dictionary
            .entries
            .iter()
            .find(|(key, _)| key == "TEST")
            .unwrap()
            .1;
        assert_ne!(copy_record, record);
        let Some(ObjectType::XRecord(xrecord)) = scene.document.objects.get(&copy_record) else {
            panic!("copied TEST XRECORD must exist");
        };
        assert_eq!(xrecord.handle, copy_record);
        assert_eq!(xrecord.owner, copy_root);
        assert_eq!(xrecord.reactors, vec![copy_root]);
        assert_eq!(xrecord.get_string(1), Some("payload"));
        for (code, handle) in [(330, copy_root), (340, copies[0]), (350, copies[1])] {
            assert!(xrecord
                .entries
                .iter()
                .any(|entry| entry.code == code && entry.value == XRecordValue::Handle(handle)));
        }
        assert_eq!(scene.document.get_entity(source), Some(&before));
        assert_eq!(scene.document.objects.get(&root), Some(&root_before));
        assert_eq!(scene.document.objects.get(&record), Some(&record_before));
        assert!(scene.take_undo_recording().unwrap().is_poisoned());
    }
}
