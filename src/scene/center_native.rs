//! Centre marks and centre lines as the drawing file stores them: an
//! anonymous block reference whose lines are driven by an associative action
//! (an AcDbCenterMarkActionBody or AcDbCenterLineActionBody body, a geometry
//! dependency on each source edge, an edge parameter caching that edge and a
//! read/write dependency on the block reference).
//!
//! The scene edits a centre object as one carrier LINE holding its
//! association in xdata. [`export`] turns every carrier of a document about to
//! be saved into the stored form; [`import`] turns the stored form of an
//! opened drawing back into carriers, so objects made by other applications
//! are edited, follow their sources and keep their properties alike.

use codec::entities::{
    CenterLineAssociation, CenterLineSource, CenterLineSourceKind, CenterMarkAssociation,
    CenterMarkSource, CenterMarkSourceKind, EntityType, Insert, Line,
};
use codec::objects::{
    AssocAction, AssocActionBody, AssocActionDependency, AssocActionParam, AssocCurveValue,
    AssocDependency, AssocEdgeActionParam, AssocEvalValue, AssocEvalVariant, AssocGeomDependency,
    AssocPersistentSubentId, AssocSingleDependencyActionParam, AssocSmartCenterActionBody,
    AssocSubcurveKind, AssocValueParam, AssocValueParamVariable, AssociativeData, ObjectType,
};
use codec::types::Vector3;
use codec::{Document, Handle};
use glam::DVec3;

use crate::scene::assoc_array::{associative, block_network, next_anonymous_name};

const MARK_BODY: &str = "AcDbCenterMarkActionBody";
const LINE_BODY: &str = "AcDbCenterLineActionBody";
/// "Not set" for the extension-length parameters: the extension line starts
/// where the source geometry ends.
const UNSET: f64 = 1.0e60;

/// Class records of the stored form, with the values the reference product
/// writes: (DXF name, class name, application, proxy flags, DWG version,
/// maintenance version).
const CLASSES: &[(&str, &str, &str, u16, i32, i32)] = &[
    ("ACDBASSOCNETWORK", "AcDbAssocNetwork", "ObjectDBX Classes", 1025, 27, 45),
    ("ACDBASSOCACTION", "AcDbAssocAction", "ObjectDBX Classes", 1025, 27, 45),
    (MARK_BODY, MARK_BODY, "AcDbSmartCenter", 1153, 33, 377),
    (LINE_BODY, LINE_BODY, "AcDbSmartCenter", 1153, 33, 377),
    (
        "ACDBASSOCEDGEACTIONPARAM",
        "AcDbAssocEdgeActionParam",
        "ObjectDBX Classes",
        1025,
        27,
        50,
    ),
    (
        "ACDBASSOCGEOMDEPENDENCY",
        "AcDbAssocGeomDependency",
        "ObjectDBX Classes",
        1025,
        27,
        50,
    ),
    ("ACDBASSOCDEPENDENCY", "AcDbAssocDependency", "ObjectDBX Classes", 1025, 27, 50),
];

fn register_classes(document: &mut Document) {
    for &(dxf_name, cpp_class_name, application, proxy, dwg_version, maintenance_version) in CLASSES {
        if document.classes.get_by_name(dxf_name).is_some() {
            continue;
        }
        document.classes.add_or_update(codec::classes::DxfClass {
            dxf_name: dxf_name.to_string(),
            cpp_class_name: cpp_class_name.to_string(),
            application_name: application.to_string(),
            proxy_flags: codec::classes::ProxyFlags(proxy),
            instance_count: 0,
            was_zombie: false,
            is_an_entity: false,
            class_number: 0,
            item_class_id: 0x1F3,
            dwg_version,
            maintenance_version,
            unknown1: 0,
            unknown2: 0,
        });
    }
}

fn dvec(point: Vector3) -> DVec3 {
    DVec3::new(point.x, point.y, point.z)
}

fn vector(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

fn tuple(point: DVec3) -> (f64, f64, f64) {
    (point.x, point.y, point.z)
}

fn real(name: &str, value: f64) -> AssocValueParam {
    variant(name, vec![(40, AssocEvalValue::Real(value))])
}

fn text(name: &str, value: &str) -> AssocValueParam {
    variant(name, vec![(1, AssocEvalValue::Text(value.to_string()))])
}

fn variant(name: &str, values: Vec<(i16, AssocEvalValue)>) -> AssocValueParam {
    AssocValueParam {
        name: name.to_string(),
        variables: values
            .into_iter()
            .map(|(code, value)| AssocValueParamVariable {
                value: AssocEvalVariant { code, value },
                handle: Handle::NULL,
            })
            .collect(),
        ..Default::default()
    }
}

fn param<'a>(values: &'a [AssocValueParam], name: &str) -> Option<&'a AssocEvalValue> {
    values
        .iter()
        .find(|value| value.name.eq_ignore_ascii_case(name))
        .and_then(|value| value.variables.first())
        .map(|variable| &variable.value.value)
}

fn param_real(values: &[AssocValueParam], name: &str) -> Option<f64> {
    match param(values, name)? {
        AssocEvalValue::Real(value) => Some(*value),
        AssocEvalValue::Long(value) => Some(f64::from(*value)),
        AssocEvalValue::Short(value) => Some(f64::from(*value)),
        AssocEvalValue::Text(value) => value.trim().parse().ok(),
        _ => None,
    }
}

fn param_text(values: &[AssocValueParam], name: &str) -> Option<String> {
    match param(values, name)? {
        AssocEvalValue::Text(value) => Some(value.clone()),
        AssocEvalValue::Real(value) => Some(value.to_string()),
        _ => None,
    }
}

/// A centre mark's cross size or gap as the reference stores it: a factor
/// of the diameter (`0.1x`) or a length.
fn measure_expression(value: f64, relative: bool, diameter: f64) -> String {
    if relative && diameter > 1.0e-12 {
        let factor = value / diameter;
        let rounded = (factor * 1.0e8).round() / 1.0e8;
        format!("{rounded}x")
    } else {
        let rounded = (value * 1.0e8).round() / 1.0e8;
        format!("{rounded}")
    }
}

/// The line work of one stored centre object, in block coordinates, with a
/// flag per line: `true` for the extension (patterned) lines.
struct Picture {
    insert_point: DVec3,
    rotation: f64,
    normal: DVec3,
    lines: Vec<([DVec3; 2], bool, bool)>,
}

/// Mark directions in the order the stored form keeps them: left, right,
/// bottom, top, as indexes into the carrier's `[+x, -x, +y, -y]`.
const STORED_DIRECTIONS: [usize; 4] = [1, 0, 3, 2];

fn mark_picture(association: &CenterMarkAssociation) -> Picture {
    let center = dvec(association.center);
    let x = dvec(association.plane_x).normalize_or(DVec3::X);
    let y = dvec(association.plane_y).normalize_or(DVec3::Y);
    let normal = x.cross(y).normalize_or(DVec3::Z);
    let (ocs_x, _) = crate::scene::view::transform::ocs_axes(tuple(normal));
    let ocs_x = DVec3::new(ocs_x.0, ocs_x.1, ocs_x.2);
    let rotation = normal.cross(ocs_x).dot(x).atan2(ocs_x.dot(x));
    let half = (association.cross_size * 0.5).max(0.0);
    let start = half + association.cross_gap.max(0.0);
    let local = [-DVec3::X, DVec3::X, -DVec3::Y, DVec3::Y];
    let mut lines = vec![
        ([DVec3::new(-half, 0.0, 0.0), DVec3::new(half, 0.0, 0.0)], false, true),
        ([DVec3::new(0.0, -half, 0.0), DVec3::new(0.0, half, 0.0)], false, true),
    ];
    for (slot, index) in STORED_DIRECTIONS.into_iter().enumerate() {
        let end = (association.radius
            + association.extension_length
            + association.length_adjustments[index]
            + association.overshoots[index])
            .max(start);
        let direction = local[slot];
        let near = direction * start;
        let far = direction * end;
        // The negative sides run from the far end in, as stored.
        let line = if slot % 2 == 0 { [far, near] } else { [near, far] };
        lines.push((line, true, association.show_extensions));
    }
    Picture {
        insert_point: center,
        rotation,
        normal,
        lines,
    }
}

fn line_picture(line: &Line) -> Picture {
    let start = dvec(line.start);
    let end = dvec(line.end);
    let middle = (start + end) * 0.5;
    Picture {
        insert_point: middle,
        rotation: 0.0,
        normal: DVec3::Z,
        lines: vec![([start - middle, end - middle], true, true)],
    }
}

/// The source edge a stored dependency reads, as the reference identifies it.
fn subent(source_kind_segment: Option<(usize, usize)>) -> AssocPersistentSubentId {
    match source_kind_segment {
        Some((index, vertices)) => AssocPersistentSubentId {
            class_name: "AcDbAssocEdgePersSubentId".to_string(),
            class_code: 3,
            values: vec![0, index as i32 + 1, ((index + 1) % vertices.max(1)) as i32 + 1],
            ..Default::default()
        },
        None => AssocPersistentSubentId {
            class_name: "AcDbAssocSingleEdgePersSubentId".to_string(),
            class_code: 1,
            ..Default::default()
        },
    }
}

fn vertex_count(document: &Document, handle: Handle) -> usize {
    match document.get_entity(handle) {
        Some(EntityType::LwPolyline(polyline)) => polyline.vertices.len(),
        Some(EntityType::Polyline2D(polyline)) => polyline.vertices.len(),
        _ => 0,
    }
}

/// The cached edge of a centre mark source: centre, normal, reference axis,
/// radius and the arc's angles.
fn arc_curve(document: &Document, source: &CenterMarkSource, center: DVec3, radius: f64) -> Vec<AssocCurveValue> {
    let (normal, start, end) = match document.get_entity(source.handle) {
        Some(EntityType::Arc(arc)) => (dvec(arc.normal), arc.start_angle, arc.end_angle),
        Some(EntityType::Circle(circle)) => (dvec(circle.normal), 0.0, std::f64::consts::TAU),
        Some(EntityType::LwPolyline(polyline)) => (dvec(polyline.normal), 0.0, std::f64::consts::TAU),
        Some(EntityType::Polyline2D(polyline)) => (dvec(polyline.normal), 0.0, std::f64::consts::TAU),
        _ => (DVec3::Z, 0.0, std::f64::consts::TAU),
    };
    let (x, _) = crate::scene::view::transform::ocs_axes(tuple(normal));
    vec![
        AssocCurveValue::Point(vector(center)),
        AssocCurveValue::Point(vector(normal)),
        AssocCurveValue::Point(Vector3::new(x.0, x.1, x.2)),
        AssocCurveValue::Real(radius),
        AssocCurveValue::Real(start),
        AssocCurveValue::Real(end),
        AssocCurveValue::Real(0.0),
    ]
}

/// Where the source line's start lies on the centre line's own direction:
/// 1 on its start side. A polyline segment counts the other way round, as
/// the reference writes it.
fn direction_ending(document: &Document, source: &CenterLineSource, axis: DVec3) -> i32 {
    let Some((start, end)) = crate::scene::centerline::source_segment(document, source) else {
        return 0;
    };
    let forward = (end - start).dot(axis) > 0.0;
    let polyline = !matches!(source.kind, CenterLineSourceKind::Line);
    i32::from(forward != polyline)
}

struct Source {
    handle: Handle,
    segment: Option<(usize, usize)>,
    curve: (i32, AssocSubcurveKind, Vec<AssocCurveValue>),
}

/// Turn every carrier of `document` (a copy about to be saved) into the
/// stored form.
pub(crate) fn export(document: &mut Document) {
    let carriers: Vec<Handle> = document
        .entities()
        .filter_map(|entity| {
            let EntityType::Line(line) = entity else {
                return None;
            };
            let data = &line.common.extended_data;
            (CenterMarkAssociation::read(data).is_some() || CenterLineAssociation::read(data).is_some())
                .then_some(line.common.handle)
        })
        .collect();
    if carriers.is_empty() {
        return;
    }
    register_classes(document);
    for handle in carriers {
        export_one(document, handle);
    }
}

fn export_one(document: &mut Document, handle: Handle) {
    let Some(EntityType::Line(line)) = document.get_entity(handle).cloned() else {
        return;
    };
    let mark = CenterMarkAssociation::read(&line.common.extended_data);
    let center_line = CenterLineAssociation::read(&line.common.extended_data);
    let owner = line.common.owner_handle;
    let picture = match &mark {
        Some(association) => mark_picture(association),
        None => line_picture(&line),
    };
    let sources: Vec<Source> = match (&mark, &center_line) {
        (Some(association), _) => vec![Source {
            handle: association.source.handle,
            segment: (association.source.segment_index >= 0).then(|| {
                (
                    association.source.segment_index as usize,
                    vertex_count(document, association.source.handle),
                )
            }),
            curve: (
                11,
                AssocSubcurveKind::Arc,
                arc_curve(document, &association.source, dvec(association.center), association.radius),
            ),
        }],
        (None, Some(association)) => [&association.first, &association.second]
            .into_iter()
            .map(|source| {
                let (start, end) = crate::scene::centerline::source_segment(document, source)
                    .unwrap_or((dvec(source.pick_point), dvec(source.pick_point)));
                Source {
                    handle: source.handle,
                    segment: (source.segment_index >= 0).then(|| {
                        (source.segment_index as usize, vertex_count(document, source.handle))
                    }),
                    curve: (
                        23,
                        AssocSubcurveKind::LineSegment3d,
                        vec![
                            AssocCurveValue::Point(vector(start)),
                            AssocCurveValue::Point(vector(end - start)),
                        ],
                    ),
                }
            })
            .collect(),
        _ => return,
    };
    let associated = mark
        .as_ref()
        .map(|association| association.associated)
        .or(center_line.as_ref().map(|association| association.associated))
        .unwrap_or(false)
        && sources
            .iter()
            .all(|source| document.get_entity(source.handle).is_some());

    // The picture block.
    let name = next_anonymous_name(document);
    let mut record = codec::tables::BlockRecord::new(&name);
    record.flags.anonymous = true;
    record.explodable = true;
    let record_handle = document.allocate_handle();
    let block_handle = document.allocate_handle();
    let end_handle = document.allocate_handle();
    record.handle = record_handle;
    record.block_entity_handle = block_handle;
    record.block_end_handle = end_handle;
    record.insert_handles = vec![handle];
    if document.block_records.add(record).is_err() {
        return;
    }
    let mut block = codec::entities::Block::new(&name, Vector3::ZERO);
    block.common.handle = block_handle;
    block.common.owner_handle = record_handle;
    block.common.layer = line.common.layer.clone();
    let _ = document.add_entity(EntityType::Block(block));
    let mut block_end = codec::entities::BlockEnd::new();
    block_end.common.handle = end_handle;
    block_end.common.owner_handle = record_handle;
    block_end.common.layer = line.common.layer.clone();
    let _ = document.add_entity(EntityType::BlockEnd(block_end));
    for (points, patterned, visible) in &picture.lines {
        let mut part = Line::from_points(vector(points[0]), vector(points[1]));
        part.common.owner_handle = record_handle;
        part.common.layer = line.common.layer.clone();
        part.common.color = codec::types::Color::ByBlock;
        part.common.transparency = codec::types::Transparency::ByBlock;
        part.common.linetype_scale = line.common.linetype_scale;
        part.common.line_weight = line.common.line_weight;
        part.common.linetype = if *patterned {
            line.common.linetype.clone()
        } else {
            "Continuous".to_string()
        };
        part.common.invisible = !visible;
        let _ = document.add_entity(EntityType::Line(part));
    }

    // The block reference takes the carrier's place and handle.
    let insert_ocs =
        crate::scene::view::transform::wcs_point_to_ocs(tuple(picture.insert_point), tuple(picture.normal));
    let mut insert = Insert::new(name, Vector3::new(insert_ocs.0, insert_ocs.1, insert_ocs.2));
    insert.rotation = picture.rotation;
    insert.normal = vector(picture.normal);
    insert.common = line.common.clone();
    insert
        .common
        .extended_data
        .remove_record(codec::entities::CENTERMARK_XDATA_APPLICATION);
    insert
        .common
        .extended_data
        .remove_record(codec::entities::CENTERLINE_XDATA_APPLICATION);

    // The action.
    let network = block_network(document, owner);
    let action = document.allocate_handle();
    let body = document.allocate_handle();
    let geometry: Vec<Handle> = if associated {
        sources.iter().map(|_| document.allocate_handle()).collect()
    } else {
        Vec::new()
    };
    let edges: Vec<Handle> = sources.iter().map(|_| document.allocate_handle()).collect();
    let insert_dependency = document.allocate_handle();
    insert.common.reactors.push(insert_dependency);
    if let Some(entity) = document.get_entity_mut(handle) {
        *entity = EntityType::Insert(insert);
    }
    for (index, dependency) in geometry.iter().enumerate() {
        let source = &sources[index];
        // Dependencies on the same object are chained to each other.
        let previous = (index > 0 && sources[index - 1].handle == source.handle)
            .then(|| geometry[index - 1])
            .unwrap_or(Handle::NULL);
        let next = (index + 1 < sources.len() && sources[index + 1].handle == source.handle)
            .then(|| geometry[index + 1])
            .unwrap_or(Handle::NULL);
        document.objects.insert(
            *dependency,
            associative(
                *dependency,
                action,
                "ACDBASSOCGEOMDEPENDENCY",
                "AcDbAssocGeomDependency",
                AssociativeData::GeomDependency(AssocGeomDependency {
                    dependency: AssocDependency {
                        class_version: 2,
                        is_read_dependency: true,
                        is_attached_to_object: true,
                        is_delegating_to_owning_action: true,
                        dependent_on: source.handle,
                        read_dependency: previous,
                        node: next,
                        dependency_body_id: index as i32 + 1,
                        ..Default::default()
                    },
                    enabled: true,
                    persistent_subent: subent(source.segment),
                    ..Default::default()
                }),
            ),
        );
        if let Some(entity) = document.get_entity_mut(source.handle) {
            entity.common_mut().reactors.push(*dependency);
        }
    }
    for (index, edge) in edges.iter().enumerate() {
        let (action_type, subcurve_kind, curve) = if associated {
            sources[index].curve.clone()
        } else {
            (0, AssocSubcurveKind::None, Vec::new())
        };
        document.objects.insert(
            *edge,
            associative(
                *edge,
                action,
                "ACDBASSOCEDGEACTIONPARAM",
                "AcDbAssocEdgeActionParam",
                AssociativeData::EdgeActionParam(AssocEdgeActionParam {
                    single_dependency: AssocSingleDependencyActionParam {
                        action_param: AssocActionParam {
                            is_r2013: 1,
                            version: 0,
                            name: "InputEdge".to_string(),
                        },
                        dependency: geometry.get(index).copied().unwrap_or(Handle::NULL),
                        ..Default::default()
                    },
                    has_action: associated,
                    action_type,
                    subcurve_kind,
                    curve,
                    ..Default::default()
                }),
            ),
        );
    }
    document.objects.insert(
        insert_dependency,
        associative(
            insert_dependency,
            action,
            "ACDBASSOCDEPENDENCY",
            "AcDbAssocDependency",
            AssociativeData::Dependency(AssocDependency {
                class_version: 2,
                is_read_dependency: true,
                is_write_dependency: true,
                is_attached_to_object: true,
                is_delegating_to_owning_action: true,
                dependent_on: handle,
                dependency_body_id: sources.len() as i32 + 1,
                ..Default::default()
            }),
        ),
    );
    let linetype = line.common.linetype.clone();
    let weight = f64::from(line.common.line_weight.value());
    let mut values = match (&mark, &center_line) {
        (Some(association), _) => {
            let mut values = Vec::new();
            for (name, index) in [
                ("HorizontalStartExtension", 1),
                ("HorizontalEndExtension", 0),
                ("VerticalStartExtension", 3),
                ("VerticalEndExtension", 2),
            ] {
                let adjustment = association.length_adjustments[index];
                values.push(real(
                    name,
                    if adjustment.abs() <= 1.0e-12 {
                        UNSET
                    } else {
                        association.radius + adjustment
                    },
                ));
            }
            for (name, index) in [
                ("HorizontalStartOvershoot", 1),
                ("HorizontalEndOvershoot", 0),
                ("VerticalStartOvershoot", 3),
                ("VerticalEndOvershoot", 2),
            ] {
                values.push(real(
                    name,
                    association.extension_length + association.overshoots[index],
                ));
            }
            let diameter = association.radius * 2.0;
            values.push(text(
                "CrossHairLengthExpression",
                &measure_expression(association.cross_size, association.cross_size_relative, diameter),
            ));
            values.push(text(
                "CrossHairGapExpression",
                &measure_expression(association.cross_gap, association.cross_gap_relative, diameter),
            ));
            values.push(variant(
                "CenterDepId",
                vec![(330, AssocEvalValue::Handle(insert_dependency))],
            ));
            values.push(real("CachedCircleRadius", association.radius));
            values
        }
        (None, Some(association)) => {
            let start = dvec(line.start);
            let end = dvec(line.end);
            let axis = (end - start).normalize_or(DVec3::X);
            let length = (end - start).length()
                - association.start_extension
                - association.end_extension
                - association.start_length_adjustment
                - association.end_length_adjustment;
            let ext_length = |adjustment: f64| {
                if adjustment.abs() <= 1.0e-12 {
                    UNSET
                } else {
                    length * 0.5 + adjustment
                }
            };
            vec![
                real("StartExtLength", ext_length(association.start_length_adjustment)),
                real("EndExtLength", ext_length(association.end_length_adjustment)),
                real("StartOvershoot", association.start_extension),
                real("EndOvershoot", association.end_extension),
                variant(
                    "EdgeRefDirectionEnding",
                    vec![
                        (
                            90,
                            AssocEvalValue::Long(direction_ending(document, &association.first, axis)),
                        ),
                        (
                            90,
                            AssocEvalValue::Long(direction_ending(document, &association.second, axis)),
                        ),
                    ],
                ),
                variant(
                    "CenterDepId",
                    vec![(330, AssocEvalValue::Handle(insert_dependency))],
                ),
                real("CachedCenterLineLength", length.max(0.0)),
                real("CachedCenterLineDirectionX", axis.x),
                real("CachedCenterLineDirectionY", axis.y),
                real("CachedCenterLineDirectionZ", axis.z),
            ]
        }
        _ => return,
    };
    values.push(text("SubentLineType", &linetype));
    values.push(real("SubentLineWeight", weight));
    values.push(real("SubentLinetypeScale", line.common.linetype_scale));
    values.push(real("CachedAssociateStatus", if associated { 1.0 } else { 0.0 }));
    if !associated {
        values.push(real("IsExplicitlyDeassociatived", 1.0));
    }
    let (body_name, max_index) = if mark.is_some() {
        (MARK_BODY, 2)
    } else {
        (LINE_BODY, 3)
    };
    document.objects.insert(
        body,
        associative(
            body,
            action,
            body_name,
            body_name,
            AssociativeData::SmartCenterActionBody(AssocSmartCenterActionBody {
                action_body: AssocActionBody { version: 2 },
                ..Default::default()
            }),
        ),
    );
    let action_index = match document.objects.get_mut(&network) {
        Some(ObjectType::Associative(object)) => match &mut object.data {
            AssociativeData::Network(network) => {
                network.network_action_index += 1;
                network.actions.push(AssocActionDependency {
                    is_owned: true,
                    dependency: action,
                });
                network.network_action_index
            }
            _ => 1,
        },
        _ => 1,
    };
    let mut dependencies: Vec<AssocActionDependency> = geometry
        .iter()
        .map(|dependency| AssocActionDependency {
            is_owned: true,
            dependency: *dependency,
        })
        .collect();
    dependencies.push(AssocActionDependency {
        is_owned: true,
        dependency: insert_dependency,
    });
    document.objects.insert(
        action,
        associative(
            action,
            network,
            "ACDBASSOCACTION",
            "AcDbAssocAction",
            AssociativeData::Action(AssocAction {
                class_version: 2,
                owning_network: network,
                action_body: body,
                action_index,
                max_dependency_index: max_index,
                dependencies,
                owned_parameters: edges,
                values: std::mem::take(&mut values),
                ..Default::default()
            }),
        ),
    );
}

/// One stored centre object found in an opened drawing.
struct Stored {
    insert: Handle,
    action: Handle,
    mark: bool,
}

fn stored_objects(document: &Document) -> Vec<Stored> {
    let mut found = Vec::new();
    for entity in document.entities() {
        let EntityType::Insert(insert) = entity else {
            continue;
        };
        for reactor in &insert.common.reactors {
            let Some(ObjectType::Associative(dependency)) = document.objects.get(reactor) else {
                continue;
            };
            if !matches!(dependency.data, AssociativeData::Dependency(_)) {
                continue;
            }
            let Some(ObjectType::Associative(action)) = document.objects.get(&dependency.owner) else {
                continue;
            };
            let AssociativeData::Action(data) = &action.data else {
                continue;
            };
            let Some(ObjectType::Associative(body)) = document.objects.get(&data.action_body) else {
                continue;
            };
            if !matches!(body.data, AssociativeData::SmartCenterActionBody(_)) {
                continue;
            }
            found.push(Stored {
                insert: insert.common.handle,
                action: dependency.owner,
                mark: body.dxf_name.eq_ignore_ascii_case(MARK_BODY)
                    || body.cpp_class_name.eq_ignore_ascii_case(MARK_BODY),
            });
            break;
        }
    }
    found
}

/// The source of each geometry dependency of `action`, in dependency order,
/// with the segment its persistent id names on a polyline.
fn action_sources(document: &Document, action: &AssocAction) -> Vec<(Handle, i32)> {
    let mut sources: Vec<(i32, Handle, i32)> = action
        .dependencies
        .iter()
        .filter_map(|link| match document.objects.get(&link.dependency) {
            Some(ObjectType::Associative(object)) => match &object.data {
                AssociativeData::GeomDependency(geometry) => {
                    let segment = if geometry.persistent_subent.class_code == 3
                        || geometry
                            .persistent_subent
                            .resolved_class_name()
                            .eq_ignore_ascii_case("AcDbAssocEdgePersSubentId")
                    {
                        geometry
                            .persistent_subent
                            .values
                            .get(1)
                            .map_or(-1, |vertex| vertex - 1)
                    } else {
                        -1
                    };
                    Some((
                        geometry.dependency.dependency_body_id,
                        geometry.dependency.dependent_on,
                        segment,
                    ))
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    sources.sort_by_key(|(id, ..)| *id);
    sources
        .into_iter()
        .map(|(_, handle, segment)| (handle, segment))
        .collect()
}

/// World placement of a block reference: insertion point and the block's x
/// and y axes (scaled).
fn insert_frame(insert: &Insert) -> (DVec3, DVec3, DVec3) {
    let normal = dvec(insert.normal).normalize_or(DVec3::Z);
    let origin = crate::scene::view::transform::ocs_point_to_wcs(
        (insert.insert_point.x, insert.insert_point.y, insert.insert_point.z),
        tuple(normal),
    );
    let (ocs_x, ocs_y) = crate::scene::view::transform::ocs_axes(tuple(normal));
    let ocs_x = DVec3::new(ocs_x.0, ocs_x.1, ocs_x.2);
    let ocs_y = DVec3::new(ocs_y.0, ocs_y.1, ocs_y.2);
    let (sin, cos) = insert.rotation.sin_cos();
    let x = ocs_x * cos + ocs_y * sin;
    let y = ocs_y * cos - ocs_x * sin;
    (
        DVec3::new(origin.0, origin.1, origin.2),
        x * insert.x_scale(),
        y * insert.y_scale(),
    )
}

/// The block's lines of a block reference, in world coordinates, with their
/// invisibility.
fn block_lines(document: &Document, insert: &Insert) -> Vec<([DVec3; 2], bool)> {
    let Some(record) = document.block_records.get(&insert.block_name) else {
        return Vec::new();
    };
    let (origin, x, y) = insert_frame(insert);
    let z = x.cross(y).normalize_or(DVec3::Z) * insert.z_scale();
    record
        .entity_handles
        .iter()
        .filter_map(|handle| match document.get_entity(*handle) {
            Some(EntityType::Line(line)) => {
                let place = |point: Vector3| origin + x * point.x + y * point.y + z * point.z;
                Some(([place(line.start), place(line.end)], line.common.invisible))
            }
            _ => None,
        })
        .collect()
}

fn mark_source_kind(entity: Option<&EntityType>) -> Option<CenterMarkSourceKind> {
    match entity? {
        EntityType::Circle(_) => Some(CenterMarkSourceKind::Circle),
        EntityType::Arc(_) => Some(CenterMarkSourceKind::Arc),
        EntityType::LwPolyline(_) => Some(CenterMarkSourceKind::LwPolylineArcSegment),
        EntityType::Polyline2D(_) => Some(CenterMarkSourceKind::Polyline2DArcSegment),
        _ => None,
    }
}

fn line_source_kind(entity: Option<&EntityType>) -> Option<CenterLineSourceKind> {
    match entity? {
        EntityType::Line(_) => Some(CenterLineSourceKind::Line),
        EntityType::LwPolyline(_) => Some(CenterLineSourceKind::LwPolylineSegment),
        EntityType::Polyline2D(_) => Some(CenterLineSourceKind::Polyline2DSegment),
        _ => None,
    }
}

/// Parse a stored cross size or gap: the length and whether it is a factor
/// of the diameter.
fn parse_measure(expression: Option<String>, diameter: f64, fallback: f64) -> (f64, bool) {
    let Some(expression) = expression else {
        return (diameter * fallback, true);
    };
    let text = expression.trim();
    if let Some(factor) = text
        .strip_suffix(['x', 'X'])
        .and_then(|value| value.trim().parse::<f64>().ok())
    {
        return (diameter * factor.max(0.0), true);
    }
    match text.parse::<f64>() {
        Ok(value) => (value.max(0.0), false),
        Err(_) => (diameter * fallback, true),
    }
}

/// Turn every stored centre object of an opened drawing into a carrier.
pub(crate) fn import(document: &mut Document) {
    let stored = stored_objects(document);
    for object in stored {
        import_one(document, object);
    }
}

fn import_one(document: &mut Document, stored: Stored) {
    let Some(EntityType::Insert(insert)) = document.get_entity(stored.insert).cloned() else {
        return;
    };
    let Some(ObjectType::Associative(object)) = document.objects.get(&stored.action) else {
        return;
    };
    let AssociativeData::Action(action) = &object.data else {
        return;
    };
    let action = action.clone();
    let values = &action.values;
    let sources = action_sources(document, &action);
    let associated = !sources.is_empty()
        && param_real(values, "CachedAssociateStatus").unwrap_or(1.0) > 0.5
        && sources
            .iter()
            .all(|(handle, _)| document.get_entity(*handle).is_some());
    let lines = block_lines(document, &insert);
    let (origin, x, y) = insert_frame(&insert);
    let scale = x.length().max(1.0e-12);
    let x_axis = x.normalize_or(DVec3::X);
    let y_axis = y.normalize_or(DVec3::Y);

    let mut carrier = if stored.mark {
        let radius = param_real(values, "CachedCircleRadius").unwrap_or(0.0) * scale;
        let (source_handle, segment) = sources.first().copied().unwrap_or((Handle::NULL, -1));
        let source_entity = document.get_entity(source_handle);
        let kind = mark_source_kind(source_entity).unwrap_or(CenterMarkSourceKind::Circle);
        let pick = origin + x_axis * radius;
        let diameter = radius * 2.0;
        let (cross_size, size_relative) =
            parse_measure(param_text(values, "CrossHairLengthExpression"), diameter, 0.1);
        let (cross_gap, gap_relative) =
            parse_measure(param_text(values, "CrossHairGapExpression"), diameter, 0.05);
        let overshoot = |name: &str| param_real(values, name).unwrap_or(0.0) * scale;
        let extension_length = overshoot("HorizontalEndOvershoot");
        let mut overshoots = [0.0; 4];
        let mut length_adjustments = [0.0; 4];
        for (slot, (extension, over)) in [
            ("HorizontalEndExtension", "HorizontalEndOvershoot"),
            ("HorizontalStartExtension", "HorizontalStartOvershoot"),
            ("VerticalEndExtension", "VerticalEndOvershoot"),
            ("VerticalStartExtension", "VerticalStartOvershoot"),
        ]
        .into_iter()
        .enumerate()
        {
            overshoots[slot] = overshoot(over) - extension_length;
            if let Some(length) = param_real(values, extension).filter(|value| *value < UNSET * 0.5) {
                length_adjustments[slot] = length * scale - radius;
            }
        }
        let show_extensions = lines.len() < 6 || lines[2..].iter().any(|(_, invisible)| !invisible);
        let association = CenterMarkAssociation {
            source: CenterMarkSource {
                handle: source_handle,
                kind,
                segment_index: segment,
                pick_point: vector(pick),
            },
            plane_origin: vector(origin),
            plane_x: vector(x_axis),
            plane_y: vector(y_axis),
            center: vector(origin),
            radius,
            cross_size,
            cross_gap,
            cross_size_relative: size_relative,
            cross_gap_relative: gap_relative,
            extension_length,
            length_adjustments,
            overshoots,
            show_extensions,
            associated,
        };
        let mut line = Line::from_points(vector(origin), vector(origin));
        crate::scene::centermark::update_carrier(&mut line, &association);
        line
    } else {
        let Some(([start, end], _)) = lines.first().copied() else {
            return;
        };
        let middle = (start + end) * 0.5;
        let make_source = |(handle, segment): (Handle, i32)| {
            let kind = line_source_kind(document.get_entity(handle))?;
            let mut source = CenterLineSource {
                handle,
                kind,
                segment_index: segment,
                pick_point: vector(middle),
            };
            // A point on the half of the source the centre line runs along,
            // so the same sector is bisected again.
            if let Some((a, b)) = crate::scene::centerline::source_segment(document, &source) {
                let direction = b - a;
                let t = ((middle - a).dot(direction) / direction.length_squared().max(1.0e-24))
                    .clamp(0.0, 1.0);
                source.pick_point = vector(a + direction * t);
            }
            Some(source)
        };
        let first = sources.first().copied().and_then(make_source);
        let second = sources.get(1).copied().and_then(make_source);
        let fallback = |handle: Handle| CenterLineSource {
            handle,
            kind: CenterLineSourceKind::Line,
            segment_index: -1,
            pick_point: vector(middle),
        };
        let association = CenterLineAssociation {
            first: first.unwrap_or_else(|| fallback(Handle::NULL)),
            second: second.unwrap_or_else(|| fallback(Handle::NULL)),
            plane_origin: Vector3::ZERO,
            plane_x: Vector3::new(1.0, 0.0, 0.0),
            plane_y: Vector3::new(0.0, 1.0, 0.0),
            start_extension: param_real(values, "StartOvershoot").unwrap_or(0.0) * scale,
            end_extension: param_real(values, "EndOvershoot").unwrap_or(0.0) * scale,
            start_length_adjustment: 0.0,
            end_length_adjustment: 0.0,
            associated: associated && sources.len() == 2,
        };
        let mut line = Line::from_points(vector(start), vector(end));
        association.write(&mut line.common.extended_data);
        line
    };

    // The carrier takes the block reference's place, handle and appearance.
    let extended_data = std::mem::take(&mut carrier.common.extended_data);
    carrier.common = insert.common.clone();
    let action_dependencies: Vec<Handle> = action
        .dependencies
        .iter()
        .map(|link| link.dependency)
        .collect();
    carrier
        .common
        .reactors
        .retain(|reactor| !action_dependencies.contains(reactor));
    for record in extended_data.records() {
        carrier.common.extended_data.upsert_record(record.clone());
    }
    if let Some(entity) = document.get_entity_mut(stored.insert) {
        *entity = EntityType::Line(carrier);
    }
    for application in [
        codec::entities::CENTERMARK_XDATA_APPLICATION,
        codec::entities::CENTERLINE_XDATA_APPLICATION,
    ] {
        if !document.app_ids.contains(application) {
            let mut app = codec::tables::AppId::new(application);
            app.handle = document.allocate_handle();
            let _ = document.app_ids.add(app);
        }
    }

    // The stored form goes: the action's objects, the source reactors, the
    // network's entry and the picture block.
    for (source, _) in &sources {
        if let Some(entity) = document.get_entity_mut(*source) {
            entity
                .common_mut()
                .reactors
                .retain(|reactor| !action_dependencies.contains(reactor));
        }
    }
    for handle in action
        .dependencies
        .iter()
        .map(|link| link.dependency)
        .chain(action.owned_parameters.iter().copied())
        .chain([action.action_body, stored.action])
    {
        document.objects.remove(&handle);
    }
    if let Some(ObjectType::Associative(network)) = document.objects.get_mut(&action.owning_network) {
        if let AssociativeData::Network(network) = &mut network.data {
            network
                .actions
                .retain(|link| link.dependency != stored.action);
        }
    }
    if let Some(record) = document.block_records.get(&insert.block_name).cloned() {
        let still_used = document.entities().any(|entity| {
            matches!(entity, EntityType::Insert(other) if other.block_name == insert.block_name)
        });
        if !still_used {
            for handle in record
                .entity_handles
                .iter()
                .copied()
                .chain([record.block_entity_handle, record.block_end_handle])
            {
                document.remove_entity(handle);
            }
            document.block_records.remove(&insert.block_name);
        }
    }
}
