//! Native parameter grip counts and restoration of default grip locations.
use super::*;
use crate::scene::block_authoring::{self as authoring, *};
use codec::objects::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Point,
    Linear,
    Polar,
    XY,
    Rotation,
    Flip,
    Alignment,
    Lookup,
    Table,
    Visibility,
}

struct Config {
    kind: Kind,
    id: i32,
    ids: [i32; 4],
    base: Vector3,
    end: Vector3,
}

fn two(data: &DynamicBlockData) -> Option<&BlockTwoPointParameter> {
    Some(match data {
        DynamicBlockData::LinearParameter(p) => &p.parameter,
        DynamicBlockData::PolarParameter(p) => &p.parameter,
        DynamicBlockData::XYParameter(p) => &p.parameter,
        DynamicBlockData::RotationParameter(p) => &p.parameter,
        DynamicBlockData::FlipParameter(p) => &p.parameter,
        DynamicBlockData::AlignmentParameter(p) => &p.parameter,
        _ => return None,
    })
}
fn two_mut(data: &mut DynamicBlockData) -> Option<&mut BlockTwoPointParameter> {
    Some(match data {
        DynamicBlockData::LinearParameter(p) => &mut p.parameter,
        DynamicBlockData::PolarParameter(p) => &mut p.parameter,
        DynamicBlockData::XYParameter(p) => &mut p.parameter,
        DynamicBlockData::RotationParameter(p) => &mut p.parameter,
        DynamicBlockData::FlipParameter(p) => &mut p.parameter,
        DynamicBlockData::AlignmentParameter(p) => &mut p.parameter,
        _ => return None,
    })
}
fn one_mut(data: &mut DynamicBlockData) -> Option<&mut BlockOnePointParameter> {
    Some(match data {
        DynamicBlockData::PointParameter(p) => &mut p.parameter,
        DynamicBlockData::LookupParameter(p) => &mut p.parameter,
        DynamicBlockData::PropertiesTable(p) => &mut p.parameter,
        _ => return None,
    })
}

impl Config {
    fn read(doc: &Document, definition: Handle, handle: Handle) -> Result<Self, &'static str> {
        let graph =
            authoring::graph(doc, definition)?.ok_or("The authoring graph is unavailable.")?;
        if let Some(ObjectType::BlockVisibilityParameter(p)) = doc.objects.get(&handle) {
            if p.owner != graph {
                return Err("Select a parameter in the active block definition.");
            }
            return Ok(Self {
                kind: Kind::Visibility,
                id: p.eval_node_id,
                ids: [p.property_info_count, 0, 0, 0],
                base: p.def_point,
                end: p.def_point,
            });
        }
        let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&handle) else {
            return Err("Select a parameter with configurable native grips.");
        };
        if object.owner != graph {
            return Err("Select a parameter in the active block definition.");
        }
        let kind = match &object.data {
            DynamicBlockData::PointParameter(_) => Kind::Point,
            DynamicBlockData::LinearParameter(_) => Kind::Linear,
            DynamicBlockData::PolarParameter(_) => Kind::Polar,
            DynamicBlockData::XYParameter(_) => Kind::XY,
            DynamicBlockData::RotationParameter(_) => Kind::Rotation,
            DynamicBlockData::FlipParameter(_) => Kind::Flip,
            DynamicBlockData::AlignmentParameter(_) => Kind::Alignment,
            DynamicBlockData::LookupParameter(_) => Kind::Lookup,
            DynamicBlockData::PropertiesTable(_) => Kind::Table,
            _ => return Err("Select a parameter with configurable native grips."),
        };
        if let Some(native) = two(&object.data) {
            let mut ids = native.property_states;
            if let DynamicBlockData::FlipParameter(p) = &object.data {
                ids[0] = p.flags_96;
            }
            return Ok(Self {
                kind,
                id: native.parameter.element.eval.node_id,
                ids,
                base: native.definition_base_point,
                end: native.definition_end_point,
            });
        }
        let native = match &object.data {
            DynamicBlockData::PointParameter(p) => &p.parameter,
            DynamicBlockData::LookupParameter(p) => &p.parameter,
            DynamicBlockData::PropertiesTable(p) => &p.parameter,
            _ => unreachable!(),
        };
        Ok(Self {
            kind,
            id: native.parameter.element.eval.node_id,
            ids: [native.property_count, 0, 0, 0],
            base: native.definition_point,
            end: native.definition_point,
        })
    }
    fn roles(&self, count: usize) -> Option<Vec<usize>> {
        match (self.kind, count) {
            (Kind::Alignment, 0) => None,
            (_, 0) => Some(vec![]),
            (Kind::Linear | Kind::Polar | Kind::XY | Kind::Rotation, 1) => Some(vec![1]),
            (_, 1) => Some(vec![0]),
            (Kind::Linear | Kind::Polar | Kind::XY, 2) => Some(vec![0, 1]),
            (Kind::XY, 4) => Some(vec![0, 1, 2, 3]),
            _ => None,
        }
    }
    fn point(&self, role: usize) -> Vector3 {
        match role {
            0 => self.base,
            1 => self.end,
            2 => Vector3::new(self.end.x, self.base.y, self.base.z),
            _ => Vector3::new(self.base.x, self.end.y, self.base.z),
        }
    }
    fn axes(&self, role: usize) -> [&'static str; 2] {
        if self.kind == Kind::Point
            || self.kind == Kind::Lookup
            || self.kind == Kind::Table
            || self.kind == Kind::Visibility
        {
            return ["UpdatedX", "UpdatedY"];
        }
        match role {
            0 => ["UpdatedBaseX", "UpdatedBaseY"],
            1 => ["UpdatedEndX", "UpdatedEndY"],
            2 => ["UpdatedEndX", "UpdatedBaseY"],
            _ => ["UpdatedBaseX", "UpdatedEndY"],
        }
    }
    fn class(&self) -> (&'static str, &'static str) {
        match self.kind {
            Kind::Point | Kind::XY => ("BLOCKXYGRIP", "AcDbBlockXYGrip"),
            Kind::Linear => ("BLOCKLINEARGRIP", "AcDbBlockLinearGrip"),
            Kind::Polar => ("BLOCKPOLARGRIP", "AcDbBlockPolarGrip"),
            Kind::Rotation => ("BLOCKROTATIONGRIP", "AcDbBlockRotationGrip"),
            Kind::Flip => ("BLOCKFLIPGRIP", "AcDbBlockFlipGrip"),
            Kind::Alignment => ("BLOCKALIGNMENTGRIP", "AcDbBlockAlignmentGrip"),
            Kind::Lookup => ("BLOCKLOOKUPGRIP", "AcDbBlockLookUpGrip"),
            Kind::Table => ("BLOCKPROPERTIESTABLEGRIP", "AcDbBlockGrip"),
            Kind::Visibility => ("BLOCKVISIBILITYGRIP", "AcDbBlockVisibilityGrip"),
        }
    }
}

pub(crate) fn choices(doc: &Document, definition: Handle, parameter: Handle) -> Vec<usize> {
    let Ok(config) = Config::read(doc, definition, parameter) else {
        return vec![];
    };
    [0, 1, 2, 4]
        .into_iter()
        .filter(|count| config.roles(*count).is_some())
        .collect()
}

fn graph_mut(doc: &mut Document, graph: Handle) -> &mut BlockEvaluationGraph {
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&graph).unwrap() else {
        unreachable!()
    };
    let DynamicBlockData::EvaluationGraph(graph) = &mut object.data else {
        unreachable!()
    };
    graph
}
fn link(
    doc: &mut Document,
    graph: Handle,
    source: i32,
    destination: i32,
) -> Result<(), &'static str> {
    let graph = graph_mut(doc, graph);
    let source = graph
        .nodes
        .iter()
        .find(|n| n.next_id == source)
        .ok_or("The grip source node is unavailable.")?
        .id;
    let destination = graph
        .nodes
        .iter()
        .find(|n| n.next_id == destination)
        .ok_or("The grip destination node is unavailable.")?
        .id;
    authoring::graph::link(graph, source, destination)
}
fn component(
    doc: &mut Document,
    definition: Handle,
    graph: Handle,
    id: i32,
    axis: &str,
) -> Result<i32, &'static str> {
    let existing = doc
        .objects
        .values()
        .filter_map(|object| match object {
            ObjectType::DynamicBlock(object) if object.owner == graph => match &object.data {
                DynamicBlockData::GripLocationComponent(c)
                    if c.grip_type == id && c.expression == axis =>
                {
                    Some(c.eval.node_id)
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    if existing.len() > 1 {
        return Err("The parameter has ambiguous grip expressions.");
    }
    if let Some(id) = existing.first() {
        return Ok(*id);
    }
    let (_, slots) = authoring::prepare(doc, definition)?;
    let handle = doc.allocate_handle();
    let (code, value) = if axis == "UpdatedFlip" {
        (70, BlockEvalValue::Short(0))
    } else {
        (40, BlockEvalValue::Real(0.0))
    };
    doc.objects.insert(
        handle,
        ObjectType::DynamicBlock(dynamic(
            handle,
            graph,
            "BLOCKGRIPLOCATIONCOMPONENT",
            "AcDbBlockGripExpr",
            DynamicBlockData::GripLocationComponent(BlockGripExpression {
                eval: expression(slots.eval, code, value),
                grip_type: id,
                expression: axis.into(),
            }),
        )),
    );
    authoring::append_single(doc, graph, slots, handle);
    link(doc, graph, id, slots.eval)?;
    Ok(slots.eval)
}

/// None restores positions; Some changes the grip count. Commit only complete graphs.
pub(crate) fn set(
    doc: &mut Document,
    definition: Handle,
    parameter: Handle,
    count: Option<usize>,
) -> Result<bool, &'static str> {
    let config = Config::read(doc, definition, parameter)?;
    if [config.base, config.end]
        .iter()
        .any(|p| ![p.x, p.y, p.z].iter().all(|v| v.is_finite()))
    {
        return Err("The parameter has a non-finite default grip location.");
    }
    let positive = config
        .ids
        .iter()
        .copied()
        .filter(|id| *id > 0)
        .collect::<std::collections::BTreeSet<_>>();
    if positive.len() != config.ids.iter().filter(|id| **id > 0).count() {
        return Err("The parameter has ambiguous grip roles.");
    }
    let graph = authoring::graph(doc, definition)?.unwrap();
    let mut old = [None; 4];
    for (role, id) in config.ids.iter().enumerate().filter(|(_, id)| **id > 0) {
        let candidates = doc
            .objects
            .iter()
            .filter_map(|(handle, object)| match object {
                ObjectType::DynamicBlock(object) if object.owner == graph => {
                    super::grip(&object.data)
                        .filter(|grip| grip.element.eval.node_id == *id)
                        .map(|_| *handle)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return Err("The parameter has an unavailable or ambiguous grip.");
        }
        old[role] = Some(candidates[0]);
    }
    let roles = if let Some(count) = count {
        config
            .roles(count)
            .ok_or("This grip count is unavailable for the selected parameter.")?
    } else {
        old.iter()
            .enumerate()
            .filter_map(|(role, handle)| handle.map(|_| role))
            .collect()
    };
    let mut staged = doc.clone();
    let repaired = if config.kind == Kind::Table {
        crate::scene::block_properties_table::repair_grip_graph(&mut staged, definition, parameter)?
    } else {
        false
    };
    if count.is_some()
        && roles.iter().all(|role| old[*role].is_some())
        && old.iter().flatten().count() == roles.len()
    {
        if repaired {
            *doc = staged;
        }
        return Ok(repaired);
    }
    if count.is_none() {
        let mut changed = repaired;
        for role in roles {
            let ObjectType::DynamicBlock(object) =
                staged.objects.get_mut(&old[role].unwrap()).unwrap()
            else {
                unreachable!()
            };
            let grip = super::grip_mut(&mut object.data).unwrap();
            let at = config.point(role);
            changed |= grip.location != at;
            grip.location = at;
        }
        if changed {
            *doc = staged;
        }
        return Ok(changed);
    }
    // Validate adjacency before removing nodes; unrelated action dependencies survive.
    {
        let graph = graph_mut(&mut staged, graph);
        if graph
            .nodes
            .iter()
            .any(|node| !authoring::graph::valid_lists(graph, node.id))
        {
            return Err("The authoring graph has invalid adjacency links.");
        }
    }
    let parameter_node = graph_mut(&mut staged, graph)
        .nodes
        .iter()
        .find(|n| n.expression == parameter)
        .ok_or("The parameter node is unavailable.")?
        .id;
    let mut removed = Vec::new();
    for (_, handle) in old
        .iter()
        .enumerate()
        .filter(|(role, handle)| handle.is_some() && !roles.contains(role))
    {
        let handle = handle.unwrap();
        let g = graph_mut(&mut staged, graph);
        let node = g
            .nodes
            .iter()
            .find(|n| n.expression == handle)
            .ok_or("The grip node is unavailable.")?
            .id;
        if g.edges.iter().any(|edge| {
            (edge.source_node == node && edge.destination_node != parameter_node)
                || (edge.destination_node == node && edge.source_node != parameter_node)
        }) {
            return Err("This grip has additional dependencies that cannot be removed.");
        }
        let edges = g
            .edges
            .iter()
            .filter(|e| e.source_node == node || e.destination_node == node)
            .map(|e| e.id)
            .collect::<Vec<_>>();
        for edge in edges {
            authoring::graph::remove(g, edge);
        }
        g.nodes.retain(|n| n.id != node);
        staged.objects.remove(&handle);
        removed.push(handle);
    }
    let mut ids = [0; 4];
    let (class, cpp) = config.class();
    let classes = [
        (class, cpp, 55),
        ("BLOCKGRIPLOCATIONCOMPONENT", "AcDbBlockGripExpr", 20),
    ];
    let missing = classes
        .into_iter()
        .filter(|(name, ..)| staged.classes.get_by_name(name).is_none())
        .collect::<Vec<_>>();
    if roles.iter().any(|role| old[*role].is_none()) {
        authoring::register_classes(&mut staged, &missing);
    }
    for role in roles {
        if let Some(handle) = old[role] {
            let ObjectType::DynamicBlock(object) = &staged.objects[&handle] else {
                unreachable!()
            };
            ids[role] = super::grip(&object.data).unwrap().element.eval.node_id;
            continue;
        }
        let axes = config.axes(role);
        let x = component(&mut staged, definition, graph, config.id, axes[0])?;
        let y = component(&mut staged, definition, graph, config.id, axes[1])?;
        let flip = if config.kind == Kind::Flip {
            component(&mut staged, definition, graph, config.id, "UpdatedFlip")?
        } else {
            0
        };
        let (_, slots) = authoring::prepare(&mut staged, definition)?;
        let handle = staged.allocate_handle();
        let native = BlockGrip {
            element: BlockElement {
                eval: expression(slots.eval, NO_VALUE, BlockEvalValue::None),
                name: ["Base Grip", "End Grip", "X Grip", "Y Grip"][role].into(),
                major: MAJOR,
                minor: MINOR,
                ..Default::default()
            },
            flags_91: x,
            flags_92: y,
            location: config.point(role),
            insert_cycling: !matches!(
                config.kind,
                Kind::Rotation | Kind::Flip | Kind::Lookup | Kind::Table | Kind::Visibility
            ),
            insert_cycling_weight: -1,
        };
        let data = match config.kind {
            Kind::Point | Kind::XY => DynamicBlockData::XYGrip(native),
            Kind::Linear | Kind::Alignment => {
                let oriented = BlockOrientedGrip {
                    grip: native,
                    orientation: if config.kind == Kind::Linear && role == 0 {
                        config.base - config.end
                    } else {
                        config.end - config.base
                    },
                };
                if config.kind == Kind::Linear {
                    DynamicBlockData::LinearGrip(oriented)
                } else {
                    DynamicBlockData::AlignmentGrip(oriented)
                }
            }
            Kind::Polar => DynamicBlockData::PolarGrip(native),
            Kind::Rotation => DynamicBlockData::RotationGrip(native),
            Kind::Flip => DynamicBlockData::FlipGrip(BlockFlipGrip {
                grip: native,
                combined_state: flip,
                orientation: config.end - config.base,
            }),
            Kind::Lookup => DynamicBlockData::LookupGrip(native),
            Kind::Table => DynamicBlockData::PropertiesTableGrip(native),
            Kind::Visibility => DynamicBlockData::VisibilityGrip(native),
        };
        staged.objects.insert(
            handle,
            ObjectType::DynamicBlock(dynamic(handle, graph, class, cpp, data)),
        );
        authoring::append_single(&mut staged, graph, slots, handle);
        link(&mut staged, graph, slots.eval, config.id)?;
        ids[role] = slots.eval;
    }
    if config.kind == Kind::Visibility {
        let ObjectType::BlockVisibilityParameter(native) = &staged.objects[&parameter] else {
            unreachable!()
        };
        let mut native = native.clone();
        native.property_info_count = ids[0];
        crate::scene::block_visibility::store(&mut staged, native);
    } else {
        let ObjectType::DynamicBlock(object) = staged.objects.get_mut(&parameter).unwrap() else {
            unreachable!()
        };
        if let Some(native) = two_mut(&mut object.data) {
            native.property_states = ids;
            if matches!(
                config.kind,
                Kind::Linear | Kind::Polar | Kind::XY | Kind::Rotation
            ) {
                for (index, property) in native.properties.iter_mut().enumerate() {
                    if property
                        .connections
                        .iter()
                        .any(|c| !matches!(c.name.as_str(), "DisplacementX" | "DisplacementY"))
                    {
                        return Err("This parameter has additional native grip connections.");
                    }
                    property.connections.clear();
                    for (role, id) in ids.iter().enumerate().filter(|(_, id)| **id > 0) {
                        for (axis, name) in config.axes(role).iter().enumerate() {
                            let index_for_axis = match *name {
                                "UpdatedBaseX" => 0,
                                "UpdatedBaseY" => 1,
                                "UpdatedEndX" => 2,
                                _ => 3,
                            };
                            if index_for_axis == index {
                                property.connections.push(BlockConnection {
                                    code: *id,
                                    name: if axis == 0 {
                                        "DisplacementX"
                                    } else {
                                        "DisplacementY"
                                    }
                                    .into(),
                                });
                            }
                        }
                    }
                }
            }
        }
        if let DynamicBlockData::FlipParameter(p) = &mut object.data {
            p.flags_96 = ids[0];
        }
        if let Some(native) = one_mut(&mut object.data) {
            native.property_count = ids[0];
            if config.kind == Kind::Point {
                for (axis, property) in native.properties.iter_mut().enumerate() {
                    property.connections = if ids[0] > 0 {
                        vec![BlockConnection {
                            code: ids[0],
                            name: if axis == 0 {
                                "DisplacementX"
                            } else {
                                "DisplacementY"
                            }
                            .into(),
                        }]
                    } else {
                        vec![]
                    };
                }
            }
        }
    }
    let visibility = staged
        .block_visibility_params
        .values()
        .filter(|p| {
            p.states.iter().any(|state| {
                state
                    .visible_params
                    .iter()
                    .any(|handle| removed.contains(handle))
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    for mut native in visibility {
        for state in &mut native.states {
            state
                .visible_params
                .retain(|handle| !removed.contains(handle));
        }
        crate::scene::block_visibility::store(&mut staged, native);
    }
    authoring::graph(&staged, definition)?;
    let g = graph_mut(&mut staged, graph);
    if g.nodes
        .iter()
        .any(|node| !authoring::graph::valid_lists(g, node.id))
    {
        return Err("The grip update produced invalid adjacency links.");
    }
    *doc = staged;
    Ok(true)
}

pub(crate) fn snapshot(
    doc: &Document,
    definition: Handle,
) -> Result<Vec<(Handle, ObjectType)>, &'static str> {
    let graph = authoring::graph(doc, definition)?.ok_or("The authoring graph is unavailable.")?;
    Ok(doc
        .objects
        .iter()
        .filter(|(handle, object)| {
            **handle == graph
                || match object {
                    ObjectType::DynamicBlock(object) => object.owner == graph,
                    ObjectType::BlockVisibilityParameter(parameter) => parameter.owner == graph,
                    _ => false,
                }
        })
        .map(|(handle, object)| (*handle, object.clone()))
        .collect())
}
pub(crate) fn count(doc: &Document, definition: Handle, parameter: Handle) -> Option<usize> {
    let config = Config::read(doc, definition, parameter).ok()?;
    Some(config.ids.iter().filter(|id| **id > 0).count())
}

pub(crate) struct NativeGrip {
    pub role: usize,
    pub handle: Handle,
    pub point: Vector3,
    pub key_point: Vector3,
    pub direction: Option<Vector3>,
}

pub(crate) fn locations(
    doc: &Document,
    definition: Handle,
    parameter: Handle,
) -> Result<Vec<NativeGrip>, &'static str> {
    let config = Config::read(doc, definition, parameter)?;
    let graph = authoring::graph(doc, definition)?.unwrap();
    let mut result = Vec::new();
    for (role, id) in config.ids.iter().enumerate().filter(|(_, id)| **id > 0) {
        let candidates = doc
            .objects
            .iter()
            .filter_map(|(handle, object)| {
                let ObjectType::DynamicBlock(object) = object else {
                    return None;
                };
                if object.owner != graph {
                    return None;
                }
                let native = super::grip(&object.data)?;
                if native.element.eval.node_id != *id {
                    return None;
                }
                let direction = match &object.data {
                    DynamicBlockData::LinearGrip(g) | DynamicBlockData::AlignmentGrip(g) => {
                        Some(g.orientation)
                    }
                    DynamicBlockData::FlipGrip(g) => {
                        Some(Vector3::new(-g.orientation.y, g.orientation.x, 0.))
                    }
                    _ => None,
                };
                Some(NativeGrip {
                    role,
                    handle: *handle,
                    point: native.location,
                    key_point: config.point(role),
                    direction,
                })
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return Err("The parameter has an unavailable or ambiguous grip.");
        }
        result.extend(candidates);
    }
    if result
        .iter()
        .map(|g| g.handle)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != result.len()
    {
        return Err("The parameter has ambiguous grip roles.");
    }
    Ok(result)
}

pub(crate) fn set_location(
    doc: &mut Document,
    definition: Handle,
    parameter: Handle,
    role: usize,
    point: Vector3,
) -> Result<bool, &'static str> {
    let config = Config::read(doc, definition, parameter)?;
    let grip = locations(doc, definition, parameter)?
        .into_iter()
        .find(|grip| grip.role == role)
        .ok_or("The selected native grip is unavailable.")?;
    if ![point.x, point.y, point.z]
        .iter()
        .all(|value| value.is_finite())
        || (point.z - config.point(role).z).abs() > 1e-8
    {
        return Err("Specify a finite grip location in the parameter plane.");
    }
    if grip.point == point {
        return Ok(false);
    }
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&grip.handle).unwrap() else {
        unreachable!()
    };
    super::grip_mut(&mut object.data).unwrap().location = point;
    Ok(true)
}

/// Keep native display glyphs separate from definition geometry and labels.
pub(crate) fn apply_graphic(
    doc: &Document,
    definition: Handle,
    parameter: Handle,
    graphic: &mut crate::scene::block_distance_parameter::Graphic,
) -> Option<Vec<NativeGrip>> {
    let config = Config::read(doc, definition, parameter).ok()?;
    let grips = locations(doc, definition, parameter).ok()?;
    graphic.grips.clear();
    graphic.round_grips.clear();
    graphic.lookup_grips.clear();
    for grip in &grips {
        match config.kind {
            Kind::Rotation => graphic.round_grips.push(grip.point),
            Kind::Lookup | Kind::Table | Kind::Visibility => graphic.lookup_grips.push(grip.point),
            _ => graphic.grips.push((grip.point, grip.direction.is_some())),
        }
    }
    Some(grips)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{
        block_actions as actions, block_distance_parameter as distance,
        block_orientation_parameter as orientation, Scene,
    };
    use codec::{DwgReader, DwgWriter, DxfReader, DxfWriter, EntityType};
    use std::io::Cursor;

    fn fixture(family: usize) -> (Document, Handle, Handle) {
        let mut scene = Scene::new();
        scene.define_block_raw(
            "GRIPS",
            Vector3::ZERO,
            vec![EntityType::Line(codec::entities::Line::from_points(
                Vector3::UNIT_X,
                Vector3::new(2., 0., 0.),
            ))],
        );
        let definition = scene.document.block_records.get("GRIPS").unwrap().handle;
        let doc = &mut scene.document;
        let base = Vector3::ZERO;
        let end = Vector3::new(10., 10., 0.);
        let label = Vector3::new(5., 12., 0.);
        let parameter = match family {
            0 => crate::scene::block_point_parameter::create(doc, definition, base, label).unwrap(),
            1..=3 => distance::create(
                doc,
                definition,
                [
                    distance::Kind::Linear,
                    distance::Kind::Polar,
                    distance::Kind::XY,
                ][family - 1],
                base,
                end,
                label,
            )
            .unwrap(),
            4..=6 => orientation::create(
                doc,
                definition,
                [
                    orientation::Kind::Rotation,
                    orientation::Kind::Flip,
                    orientation::Kind::Alignment,
                ][family - 4],
                base,
                end,
                if family == 4 { end } else { label },
            )
            .unwrap(),
            7 => crate::scene::block_lookup::create(doc, definition, base).unwrap(),
            8 => {
                distance::create(doc, definition, distance::Kind::Linear, base, end, label)
                    .unwrap();
                let columns = crate::scene::block_properties_table::columns(doc, definition);
                crate::scene::block_properties_table::apply(
                    doc,
                    definition,
                    &columns,
                    &[vec![20.]],
                    false,
                )
                .unwrap()
            }
            _ => crate::scene::block_visibility::create(doc, definition, base).unwrap(),
        };
        if family <= 5 {
            let target = doc.block_records.get("GRIPS").unwrap().entity_handles[0];
            actions::create(
                doc,
                definition,
                if family == 4 {
                    actions::Kind::Rotate
                } else if family == 5 {
                    actions::Kind::Flip
                } else {
                    actions::Kind::Move
                },
                parameter,
                &[target],
                glam::DVec3::ZERO,
            )
            .unwrap();
        }
        (scene.document, definition, parameter)
    }
    fn check_graph(doc: &Document, definition: Handle) {
        let handle = authoring::graph(doc, definition).unwrap().unwrap();
        let ObjectType::DynamicBlock(object) = &doc.objects[&handle] else {
            panic!()
        };
        let DynamicBlockData::EvaluationGraph(graph) = &object.data else {
            panic!()
        };
        assert!(graph
            .nodes
            .iter()
            .all(|node| authoring::graph::valid_lists(graph, node.id)));
    }
    fn native_grips(doc: &Document, definition: Handle, parameter: Handle) -> Vec<Handle> {
        let config = Config::read(doc, definition, parameter).unwrap();
        let mut handles = doc
            .objects
            .iter()
            .filter_map(|(h, object)| match object {
                ObjectType::DynamicBlock(object) => super::super::grip(&object.data)
                    .filter(|g| config.ids.contains(&g.element.eval.node_id))
                    .map(|_| *h),
                _ => None,
            })
            .collect::<Vec<_>>();
        handles.sort();
        handles
    }
    #[test]
    fn block_grip_locations_follow_key_point_edits_without_losing_offsets() {
        use crate::scene::block_authoring::edit::{self, Part};
        for family in 0..10 {
            let (mut doc, definition, parameter) = fixture(family);
            let maximum = *choices(&doc, definition, parameter).last().unwrap();
            set(&mut doc, definition, parameter, Some(maximum)).unwrap();
            for grip in locations(&doc, definition, parameter).unwrap() {
                set_location(
                    &mut doc,
                    definition,
                    parameter,
                    grip.role,
                    grip.point + Vector3::new(15., 7., 0.),
                )
                .unwrap();
            }
            assert!(edit::items(&doc, definition)
                .iter()
                .any(|item| item.handle == parameter));
            let before = locations(&doc, definition, parameter).unwrap();
            let part = if (1..=6).contains(&family) {
                Part::Base
            } else {
                Part::Point
            };
            edit::move_part(
                &mut doc,
                definition,
                parameter,
                part,
                Vector3::new(-2., -3., 0.),
            )
            .unwrap();
            for (old, new) in before
                .iter()
                .zip(locations(&doc, definition, parameter).unwrap())
            {
                assert_eq!(old.handle, new.handle);
                assert!((new.point - new.key_point - (old.point - old.key_point)).length() < 1e-8);
            }
            if family == 1 {
                let grips = locations(&doc, definition, parameter).unwrap();
                let base = grips
                    .iter()
                    .find(|grip| grip.role == 0)
                    .unwrap()
                    .direction
                    .unwrap();
                let end = grips
                    .iter()
                    .find(|grip| grip.role == 1)
                    .unwrap()
                    .direction
                    .unwrap();
                assert!((base + end).length() < 1e-8);
            }
            check_graph(&doc, definition);
        }
    }

    #[test]
    fn block_grip_alignment_restores_its_required_grip_and_direction() {
        let (mut doc, definition, parameter) = fixture(6);
        let native = locations(&doc, definition, parameter).unwrap().remove(0);
        let graph_handle = authoring::graph(&doc, definition).unwrap().unwrap();
        let graph = graph_mut(&mut doc, graph_handle);
        let node = graph
            .nodes
            .iter()
            .find(|node| node.expression == native.handle)
            .unwrap()
            .id;
        let edges = graph
            .edges
            .iter()
            .filter(|edge| edge.source_node == node || edge.destination_node == node)
            .map(|edge| edge.id)
            .collect::<Vec<_>>();
        for edge in edges {
            authoring::graph::remove(graph, edge);
        }
        graph.nodes.retain(|n| n.id != node);
        doc.objects.remove(&native.handle);
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&parameter).unwrap() else {
            panic!()
        };
        let DynamicBlockData::AlignmentParameter(p) = &mut object.data else {
            panic!()
        };
        p.parameter.property_states[0] = 0;
        assert_eq!(choices(&doc, definition, parameter), vec![1]);
        assert!(set(&mut doc, definition, parameter, Some(0)).is_err());
        assert!(set(&mut doc, definition, parameter, Some(1)).unwrap());
        let restored = locations(&doc, definition, parameter).unwrap().remove(0);
        assert!((restored.direction.unwrap() - native.direction.unwrap()).length() < 1e-8);
        check_graph(&doc, definition);
    }

    #[test]
    fn block_grip_locations_preserve_definition_values_graphs_and_roundtrip() {
        for family in 0..10 {
            let (mut doc, definition, parameter) = fixture(family);
            let maximum = *choices(&doc, definition, parameter).last().unwrap();
            set(&mut doc, definition, parameter, Some(maximum)).unwrap();
            let grips = locations(&doc, definition, parameter).unwrap();
            let handles = grips.iter().map(|grip| grip.handle).collect::<Vec<_>>();
            if family <= 5 {
                let p = actions::all_parameters(&doc, definition)
                    .into_iter()
                    .find(|p| p.handle == parameter)
                    .unwrap();
                let native = locations(&doc, definition, parameter)
                    .unwrap()
                    .into_iter()
                    .find(|g| g.handle == p.grip)
                    .unwrap();
                assert_eq!(
                    actions::grip_position(&p, actions::value(&doc, &p).unwrap()),
                    actions::v(native.point)
                );
            }
            let before = doc.clone();
            for grip in &grips {
                let point = grip.point + Vector3::new(15., 7., 0.);
                assert!(set_location(&mut doc, definition, parameter, grip.role, point).unwrap());
                assert!(!set_location(&mut doc, definition, parameter, grip.role, point).unwrap());
            }
            assert!(before
                .objects
                .iter()
                .filter(|(handle, _)| !handles.contains(handle))
                .all(|(handle, object)| &doc.objects[handle] == object));
            assert_eq!(
                doc.entities().cloned().collect::<Vec<_>>(),
                before.entities().cloned().collect::<Vec<_>>()
            );
            check_graph(&doc, definition);
            if family <= 5 {
                let p = actions::all_parameters(&doc, definition)
                    .into_iter()
                    .find(|p| p.handle == parameter)
                    .unwrap();
                let native = locations(&doc, definition, parameter)
                    .unwrap()
                    .into_iter()
                    .find(|g| g.handle == p.grip)
                    .unwrap();
                assert_eq!(
                    actions::grip_position(&p, actions::value(&doc, &p).unwrap()),
                    actions::v(native.point)
                );
            }
            let changed = doc.clone();
            for point in [
                Vector3::new(f64::INFINITY, 0., 0.),
                Vector3::new(1., 2., 1.),
            ] {
                assert!(
                    set_location(&mut doc, definition, parameter, grips[0].role, point).is_err()
                );
                assert_eq!(doc, changed);
            }
            assert!(set_location(&mut doc, definition, parameter, 99, Vector3::ZERO).is_err());
            assert_eq!(doc, changed);
            for saved in [
                DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
                    .read()
                    .unwrap(),
                DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
                    .unwrap()
                    .read()
                    .unwrap(),
            ] {
                let saved_grips = locations(&saved, definition, parameter).unwrap();
                for (original, saved) in grips.iter().zip(&saved_grips) {
                    assert_eq!(saved.handle, original.handle);
                    assert_eq!(saved.point, original.point + Vector3::new(15., 7., 0.));
                }
                let mut graphic = crate::scene::block_distance_parameter::Graphic::default();
                apply_graphic(&saved, definition, parameter, &mut graphic).unwrap();
                assert_eq!(
                    graphic.grips.len() + graphic.round_grips.len() + graphic.lookup_grips.len(),
                    maximum
                );
            }
            set(&mut doc, definition, parameter, None).unwrap();
            for (old, restored) in grips
                .iter()
                .zip(locations(&doc, definition, parameter).unwrap())
            {
                assert_eq!(old.handle, restored.handle);
                assert_eq!(old.point, restored.point);
            }
        }
    }

    #[test]
    fn block_grip_counts_preserve_action_graphs_and_native_roundtrips() {
        for family in 0..10 {
            let (mut doc, definition, parameter) = fixture(family);
            let actions_before = actions::actions(&doc, definition)
                .into_iter()
                .map(|(h, ..)| (h, doc.objects[&h].clone()))
                .collect::<Vec<_>>();
            let available = choices(&doc, definition, parameter);
            let maximum = *available.last().unwrap();
            if family == 6 {
                assert_eq!(available, vec![1]);
                let before = doc.clone();
                assert!(set(&mut doc, definition, parameter, Some(0)).is_err());
                assert_eq!(doc, before);
            }
            let before = doc.clone();
            assert!(set(&mut doc, definition, parameter, Some(3)).is_err());
            assert_eq!(doc, before);
            assert!(set(&mut doc, Handle::NULL, parameter, Some(0)).is_err());
            assert_eq!(doc, before);
            for desired in [0, maximum, 1, maximum, 0, maximum]
                .into_iter()
                .filter(|count| available.contains(count))
            {
                set(&mut doc, definition, parameter, Some(desired)).unwrap();
                assert_eq!(
                    count(&doc, definition, parameter),
                    Some(desired),
                    "family {family}"
                );
                assert_eq!(native_grips(&doc, definition, parameter).len(), desired);
                check_graph(&doc, definition);
                assert!(actions_before
                    .iter()
                    .all(|(h, object)| &doc.objects[h] == object));
            }
            for saved in [
                DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
                    .read()
                    .unwrap(),
                DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
                    .unwrap()
                    .read()
                    .unwrap(),
            ] {
                assert_eq!(count(&saved, definition, parameter), Some(maximum));
                assert_eq!(
                    native_grips(&saved, definition, parameter),
                    native_grips(&doc, definition, parameter)
                );
                check_graph(&saved, definition);
                assert!(actions_before
                    .iter()
                    .all(|(h, object)| &saved.objects[h] == object));
            }
            if family == 8 {
                set(&mut doc, definition, parameter, Some(0)).unwrap();
                let view = crate::scene::block_properties_table::table(&doc, definition)
                    .unwrap()
                    .unwrap();
                crate::scene::block_properties_table::apply(
                    &mut doc,
                    definition,
                    &view.columns,
                    &view.rows,
                    false,
                )
                .unwrap();
                assert_eq!(count(&doc, definition, parameter), Some(0));
            }
        }
    }

    #[test]
    #[ignore = "requires the local dynamic block DWG fixtures"]
    fn block_grip_counts_native_two_grip_fixture_roundtrips() {
        let root =
            std::env::var("OCS_DYNAMIC_BLOCK_FIXTURES").expect("set OCS_DYNAMIC_BLOCK_FIXTURES");
        let path = std::path::Path::new(&root).join("DWG-2018/BT.dwg");
        let mut doc = DwgReader::from_file(&path).unwrap().read().unwrap();
        let parameter = doc
            .objects
            .iter()
            .find_map(|(handle, object)| match object {
                ObjectType::DynamicBlock(object) => match &object.data {
                    DynamicBlockData::LinearParameter(p)
                        if p.parameter.property_states[0] > 0
                            && p.parameter.property_states[1] > 0 =>
                    {
                        Some((*handle, object.owner))
                    }
                    _ => None,
                },
                _ => None,
            })
            .expect("the native fixture contains a two-grip Linear parameter");
        let definition = doc
            .block_records
            .iter()
            .find(|record| {
                authoring::graph(&doc, record.handle).ok().flatten() == Some(parameter.1)
            })
            .unwrap()
            .handle;
        assert_eq!(count(&doc, definition, parameter.0), Some(2));
        let actions_before = actions::actions(&doc, definition)
            .into_iter()
            .map(|(handle, ..)| (handle, doc.objects[&handle].clone()))
            .collect::<Vec<_>>();
        for desired in [1, 0, 2] {
            set(&mut doc, definition, parameter.0, Some(desired)).unwrap();
            assert_eq!(count(&doc, definition, parameter.0), Some(desired));
            check_graph(&doc, definition);
            assert!(actions_before
                .iter()
                .all(|(handle, object)| &doc.objects[handle] == object));
        }
        for saved in [
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
                .read()
                .unwrap(),
            DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
                .unwrap()
                .read()
                .unwrap(),
        ] {
            assert_eq!(count(&saved, definition, parameter.0), Some(2));
            check_graph(&saved, definition);
        }
    }

    #[test]
    fn block_grip_counts_reject_malformed_and_shared_dependencies_atomically() {
        let (doc, definition, parameter) = fixture(1);
        let mut invalid = doc.clone();
        let ObjectType::DynamicBlock(object) = invalid.objects.get_mut(&parameter).unwrap() else {
            panic!()
        };
        let DynamicBlockData::LinearParameter(native) = &mut object.data else {
            panic!()
        };
        native.parameter.property_states[0] = native.parameter.property_states[1];
        let before = invalid.clone();
        assert!(set(&mut invalid, definition, parameter, Some(0)).is_err());
        assert_eq!(invalid, before);
        let mut invalid = doc.clone();
        let ObjectType::DynamicBlock(object) = invalid.objects.get_mut(&parameter).unwrap() else {
            panic!()
        };
        let DynamicBlockData::LinearParameter(native) = &mut object.data else {
            panic!()
        };
        native.parameter.definition_end_point.x = f64::INFINITY;
        let before = invalid.clone();
        assert!(set(&mut invalid, definition, parameter, None).is_err());
        assert_eq!(invalid, before);
        let mut linked = doc.clone();
        let grip = native_grips(&linked, definition, parameter)[0];
        let action = actions::actions(&linked, definition)[0].0;
        let graph_handle = authoring::graph(&linked, definition).unwrap().unwrap();
        let graph = graph_mut(&mut linked, graph_handle);
        let a = graph
            .nodes
            .iter()
            .find(|n| n.expression == grip)
            .unwrap()
            .id;
        let b = graph
            .nodes
            .iter()
            .find(|n| n.expression == action)
            .unwrap()
            .id;
        authoring::graph::link(graph, a, b).unwrap();
        let before = linked.clone();
        assert!(set(&mut linked, definition, parameter, Some(0)).is_err());
        assert_eq!(linked, before);
    }

    #[test]
    fn block_grip_reposition_retains_handles_bindings_and_cycle_metadata() {
        for family in 0..10 {
            let (mut doc, definition, parameter) = fixture(family);
            let retained = native_grips(&doc, definition, parameter)[0];
            let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&retained).unwrap() else {
                panic!()
            };
            let grip = super::super::grip_mut(&mut object.data).unwrap();
            grip.location = Vector3::new(35., 42., 0.);
            grip.insert_cycling = false;
            grip.insert_cycling_weight = 17;
            let before = doc.objects[&retained].clone();
            let maximum = *choices(&doc, definition, parameter).last().unwrap();
            set(&mut doc, definition, parameter, Some(maximum)).unwrap();
            assert_eq!(doc.objects[&retained], before);
            assert!(set(&mut doc, definition, parameter, None).unwrap());
            let ObjectType::DynamicBlock(object) = &doc.objects[&retained] else {
                panic!()
            };
            let grip = super::super::grip(&object.data).unwrap();
            assert!(!grip.insert_cycling);
            assert_eq!(grip.insert_cycling_weight, 17);
            let config = Config::read(&doc, definition, parameter).unwrap();
            let role = config
                .ids
                .iter()
                .position(|id| *id == grip.element.eval.node_id)
                .unwrap();
            assert_eq!(grip.location, config.point(role));
            assert!(!set(&mut doc, definition, parameter, None).unwrap());
            check_graph(&doc, definition);
        }
    }
}
