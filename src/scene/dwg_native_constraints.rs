//! Maps parametric constraints to the drawing format's associative object graph.

use acadrust::objects::{
    Assoc2dConstraintGroup, AssocAction, AssocActionDependency, AssocConstraintNode,
    AssocConstraintNodeData, AssocDependency, AssocDimDependencyBody, AssocEvalValue,
    AssocEvalVariant,
    AssocGeomDependency, AssocNetwork, AssocPersistentSubentId, AssocValueDependency,
    AssocVariable, AssociativeData, AssociativeObject, ObjectType,
};
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, EntityType};
use rustc_hash::FxHashMap;

use super::named_parameters::{DrivingValue, ParameterTable};
use super::parametric_constraints::{
    distance_direction_type, ConstraintKind, NativeConstraintOrigin, ParametricConstraint,
    ParametricConstraintSet, ParametricRef, ParametricScope,
};
use super::Scene;

/// Default sub-dictionary key used for an associative network.
const NETWORK_DICTIONARY_KEY: &str = "ACAD_ASSOCNETWORK";

/// `AcConstraintGroupNode::GroupNodeId::kNullGroupNodeId` — node id 0 is
/// reserved as "no node", so real node ids start at 1.
const FIRST_NODE_ID: i32 = 1;

/// Native implicit-point type values.
mod implicit_point_type {
    pub const START: u8 = 0;
    pub const END: u8 = 1;
    #[allow(dead_code)] // reserved for ParametricRef marker -2, not wired up yet
    pub const MID: u8 = 2;
    pub const CENTER: u8 = 3;
    pub const DEFINE: u8 = 4;
}

/// One entity's constraint-group presence: its geometry node's id, plus
/// whichever `ImplicitPoint` nodes have been created for it so far, keyed
/// by our own `ParametricRef::marker` convention so a second constraint
/// referencing the same point reuses the same node instead of duplicating it.
#[derive(Default)]
struct EntityNodes {
    geometry_node_id: i32,
    points: FxHashMap<i32, i32>,
    segments: FxHashMap<usize, i32>,
    axes: FxHashMap<i32, i32>,
}

/// Builds one scope's `Assoc2dConstraintGroup` graph incrementally,
/// allocating node ids and deduplicating geometry/point nodes per entity.
struct GroupBuilder<'a> {
    document: &'a CadDocument,
    nodes: Vec<AssocConstraintNode>,
    next_node_id: i32,
    entities: FxHashMap<Handle, EntityNodes>,
    /// Real entity handles that ended up with a geometry node — what
    /// `AssocGeomDependency` objects get created for, in first-touch order.
    referenced_entities: Vec<Handle>,
}

impl<'a> GroupBuilder<'a> {
    fn new(document: &'a CadDocument) -> Self {
        Self {
            document,
            nodes: Vec::new(),
            next_node_id: FIRST_NODE_ID,
            entities: FxHashMap::default(),
            referenced_entities: Vec::new(),
        }
    }

    fn alloc_node_id(&mut self) -> i32 {
        let id = self.next_node_id;
        self.next_node_id += 1;
        id
    }

    fn push_node(&mut self, node_id: i32, class_name: &'static str, data: AssocConstraintNodeData) {
        let status = u8::from(matches!(
            &data,
            AssocConstraintNodeData::ImplicitPoint { .. }
                | AssocConstraintNodeData::Point { .. }
                | AssocConstraintNodeData::Line { .. }
                | AssocConstraintNodeData::BoundedLine { .. }
                | AssocConstraintNodeData::Circle { .. }
                | AssocConstraintNodeData::Arc { .. }
                | AssocConstraintNodeData::Ellipse { .. }
                | AssocConstraintNodeData::BoundedEllipse { .. }
                | AssocConstraintNodeData::Spline { .. }
        ));
        self.nodes.push(AssocConstraintNode {
            node_id,
            status,
            connections: Vec::new(),
            class_name: class_name.to_string(),
            registry_flag: false,
            data,
        });
    }

    /// Returns the whole-geometry node for an entity, creating it once.
    fn geometry_node(&mut self, handle: Handle) -> Option<i32> {
        if let Some(existing) = self.entities.get(&handle) {
            if existing.geometry_node_id != 0 {
                return Some(existing.geometry_node_id);
            }
        }
        let entity = self.document.get_entity(handle)?;
        let node_id = self.alloc_node_id();
        match entity {
            EntityType::Point(point) => {
                self.push_node(
                    node_id,
                    "AcConstrainedPoint",
                    AssocConstraintNodeData::Point {
                        geometry_dependency: Handle::NULL,
                        geometry_node_id: node_id,
                        point: Some(point.location),
                    },
                );
            }
            EntityType::Insert(insert) => self.push_node(
                node_id,
                "AcConstrainedPoint",
                AssocConstraintNodeData::Point {
                    geometry_dependency: Handle::NULL,
                    geometry_node_id: node_id,
                    point: Some(insert.insert_point),
                },
            ),
            EntityType::Text(text) => self.push_node(
                node_id,
                "AcConstrainedPoint",
                AssocConstraintNodeData::Point {
                    geometry_dependency: Handle::NULL,
                    geometry_node_id: node_id,
                    point: Some(text.insertion_point),
                },
            ),
            EntityType::MText(text) => self.push_node(
                node_id,
                "AcConstrainedPoint",
                AssocConstraintNodeData::Point {
                    geometry_dependency: Handle::NULL,
                    geometry_node_id: node_id,
                    point: Some(text.insertion_point),
                },
            ),
            EntityType::AttributeDefinition(attribute) => self.push_node(
                node_id,
                "AcConstrainedPoint",
                AssocConstraintNodeData::Point {
                    geometry_dependency: Handle::NULL,
                    geometry_node_id: node_id,
                    point: Some(attribute.insertion_point),
                },
            ),
            EntityType::AttributeEntity(attribute) => self.push_node(
                node_id,
                "AcConstrainedPoint",
                AssocConstraintNodeData::Point {
                    geometry_dependency: Handle::NULL,
                    geometry_node_id: node_id,
                    point: Some(attribute.insertion_point),
                },
            ),
            EntityType::Table(table) => self.push_node(
                node_id,
                "AcConstrainedPoint",
                AssocConstraintNodeData::Point {
                    geometry_dependency: Handle::NULL,
                    geometry_node_id: node_id,
                    point: Some(table.insertion_point),
                },
            ),
            EntityType::Line(l) => {
                let point = l.start;
                let direction = (l.end - l.start).normalize();
                self.push_node(
                    node_id,
                    "AcDbAssocGeomDependency", // placeholder, overwritten below
                    AssocConstraintNodeData::None,
                );
                let last = self.nodes.last_mut().expect("just pushed");
                last.class_name = "AcConstrainedBoundedLine".to_string();
                last.status = 1;
                last.data = AssocConstraintNodeData::BoundedLine {
                    geometry_dependency: Handle::NULL,
                    geometry_node_id: node_id,
                    point,
                    direction,
                    is_ray: false,
                    start_point: l.start,
                    end_point: l.end,
                };
            }
            EntityType::Ray(ray) => {
                self.push_node(
                    node_id,
                    "AcConstrainedBoundedLine",
                    AssocConstraintNodeData::BoundedLine {
                        geometry_dependency: Handle::NULL,
                        geometry_node_id: node_id,
                        point: ray.base_point,
                        direction: ray.direction,
                        is_ray: true,
                        start_point: ray.base_point,
                        end_point: ray.base_point + ray.direction,
                    },
                );
            }
            EntityType::XLine(line) => {
                self.push_node(
                    node_id,
                    "AcConstrainedLine",
                    AssocConstraintNodeData::Line {
                        geometry_dependency: Handle::NULL,
                        geometry_node_id: node_id,
                        point: line.base_point,
                        direction: line.direction,
                    },
                );
            }
            EntityType::Circle(c) => {
                let (axis_x, _axis_y) =
                    crate::scene::view::transform::ocs_axes((c.normal.x, c.normal.y, c.normal.z));
                self.push_node(
                    node_id,
                    "AcConstrainedCircle",
                    AssocConstraintNodeData::Circle {
                        geometry_dependency: Handle::NULL,
                        geometry_node_id: node_id,
                        center: c.center,
                        normal: c.normal,
                        direction: Vector3::new(axis_x.0, axis_x.1, axis_x.2),
                        radius: c.radius,
                        start_parameter: 0.0,
                        end_parameter: std::f64::consts::TAU,
                        reserved: 0.0,
                    },
                );
            }
            EntityType::Arc(a) => {
                let (axis_x, _axis_y) =
                    crate::scene::view::transform::ocs_axes((a.normal.x, a.normal.y, a.normal.z));
                let direction = Vector3::new(axis_x.0, axis_x.1, axis_x.2);
                let start_point = a.center
                    + Vector3::new(axis_x.0, axis_x.1, axis_x.2) * (a.radius * a.start_angle.cos())
                    + Vector3::new(-axis_x.1, axis_x.0, 0.0) * (a.radius * a.start_angle.sin());
                let end_point = a.center
                    + Vector3::new(axis_x.0, axis_x.1, axis_x.2) * (a.radius * a.end_angle.cos())
                    + Vector3::new(-axis_x.1, axis_x.0, 0.0) * (a.radius * a.end_angle.sin());
                self.push_node(
                    node_id,
                    "AcConstrainedArc",
                    AssocConstraintNodeData::Arc {
                        geometry_dependency: Handle::NULL,
                        geometry_node_id: node_id,
                        center: a.center,
                        normal: a.normal,
                        direction,
                        radius: a.radius,
                        start_parameter: a.start_angle,
                        end_parameter: a.end_angle,
                        reserved: 0.0,
                        start_point,
                        end_point,
                    },
                );
            }
            EntityType::Ellipse(e) => {
                let major_length = e.major_axis.length();
                let major = if major_length > f64::EPSILON {
                    e.major_axis / major_length
                } else {
                    Vector3::UNIT_X
                };
                let normal = e.normal.normalize();
                let minor = Vector3::new(
                    normal.y * major.z - normal.z * major.y,
                    normal.z * major.x - normal.x * major.z,
                    normal.x * major.y - normal.y * major.x,
                );
                let point_at = |parameter: f64| {
                    e.center
                        + e.major_axis * parameter.cos()
                        + minor * (major_length * e.minor_axis_ratio * parameter.sin())
                };
                let data = if e.is_full() {
                    AssocConstraintNodeData::Ellipse {
                        geometry_dependency: Handle::NULL,
                        geometry_node_id: node_id,
                        center: e.center,
                        major_axis: e.major_axis,
                        axis_ratio: e.minor_axis_ratio,
                    }
                } else {
                    AssocConstraintNodeData::BoundedEllipse {
                        geometry_dependency: Handle::NULL,
                        geometry_node_id: node_id,
                        center: e.center,
                        major_axis: e.major_axis,
                        axis_ratio: e.minor_axis_ratio,
                        start_point: point_at(e.start_parameter),
                        end_point: point_at(e.end_parameter),
                    }
                };
                let class_name = if e.is_full() {
                    "AcConstrainedEllipse"
                } else {
                    "AcConstrainedBoundedEllipse"
                };
                self.push_node(node_id, class_name, data);
            }
            EntityType::Spline(spline) => {
                let curve = crate::entities::spline::nurbs3(spline)?;
                let weights = curve.weights();
                let rational = spline.flags.rational
                    || weights.iter().any(|weight| (*weight - 1.0).abs() > 1.0e-12);
                let control_points: Vec<_> = curve
                    .control_points()
                    .iter()
                    .map(|point| Vector3::new(point[0], point[1], point[2]))
                    .collect();
                let mut implicit_point_ids = Vec::with_capacity(control_points.len());
                self.push_node(
                    node_id,
                    "AcConstrainedSpline",
                    AssocConstraintNodeData::Spline {
                        geometry_dependency: Handle::NULL,
                        geometry_node_id: node_id,
                        rational,
                        periodic: spline.flags.periodic,
                        degree: curve.degree() as i32,
                        knot_tolerance: spline.knot_tolerance.max(1.0e-10),
                        knot_physical_length: curve.knots().len() as i32,
                        knot_grow_length: 8,
                        knots: curve.knots().to_vec(),
                        weight_physical_length: if rational { weights.len() as i32 } else { 0 },
                        weight_grow_length: 8,
                        weights: if rational {
                            weights.to_vec()
                        } else {
                            Vec::new()
                        },
                        control_point_physical_length: control_points.len() as i32,
                        control_point_grow_length: 8,
                        control_points,
                        implicit_point_ids: Vec::new(),
                    },
                );
                for point_index in 0..curve.control_points().len() {
                    let point_id = self.alloc_node_id();
                    self.push_node(
                        point_id,
                        "AcConstrainedImplicitPoint",
                        AssocConstraintNodeData::ImplicitPoint {
                            geometry_dependency: Handle::NULL,
                            geometry_node_id: point_id,
                            point: None,
                            point_type: implicit_point_type::DEFINE,
                            point_index: point_index as i32,
                            curve_id: node_id,
                        },
                    );
                    implicit_point_ids.push(point_id);
                }
                if let Some(AssocConstraintNode {
                    data:
                        AssocConstraintNodeData::Spline {
                            implicit_point_ids: ids,
                            ..
                        },
                    ..
                }) = self.nodes.iter_mut().find(|node| node.node_id == node_id)
                {
                    *ids = implicit_point_ids.clone();
                }
                let points = &mut self.entities.entry(handle).or_default().points;
                if let Some(first) = implicit_point_ids.first() {
                    points.insert(0, *first);
                }
                if let Some(last) = implicit_point_ids.last() {
                    points.insert(1, *last);
                }
            }
            _ => return None,
        }
        self.entities.entry(handle).or_default().geometry_node_id = node_id;
        if !self.referenced_entities.contains(&handle) {
            self.referenced_entities.push(handle);
        }
        Some(node_id)
    }

    /// Returns a derived line node for a text baseline or ellipse axis. These
    /// are real native curve nodes tied to the source entity dependency, so
    /// the selected direction survives save/reopen instead of collapsing to
    /// the entity insertion/center point.
    fn directional_axis_node(&mut self, reference: ParametricRef) -> Option<i32> {
        let marker = reference.marker?;
        reference.directional_axis()?;
        if let Some(node_id) = self
            .entities
            .get(&reference.entity)
            .and_then(|entity| entity.axes.get(&marker))
        {
            return Some(*node_id);
        }
        let entity = self.document.get_entity(reference.entity)?;
        let [start, end] =
            super::parametric_constraints::directional_axis_endpoints(entity, reference)?;
        let delta = end - start;
        if delta.length_squared() <= 1.0e-24 {
            return None;
        }
        let node_id = self.alloc_node_id();
        self.push_node(
            node_id,
            "AcConstrainedBoundedLine",
            AssocConstraintNodeData::BoundedLine {
                geometry_dependency: Handle::NULL,
                geometry_node_id: node_id,
                point: start,
                direction: delta.normalize(),
                is_ray: false,
                start_point: start,
                end_point: end,
            },
        );
        self.entities
            .entry(reference.entity)
            .or_default()
            .axes
            .insert(marker, node_id);
        if !self.referenced_entities.contains(&reference.entity) {
            self.referenced_entities.push(reference.entity);
        }
        Some(node_id)
    }

    /// Returns the point-node id for a line endpoint or curve center,
    /// creating it the first time the marker is referenced.
    fn point_node(&mut self, handle: Handle, marker: i32) -> Option<i32> {
        if let Some(existing) = self
            .entities
            .get(&handle)
            .and_then(|e| e.points.get(&marker))
        {
            return Some(*existing);
        }
        let segment_midpoint = ParametricRef::point(handle, marker).segment_midpoint_index();
        let segment_center = ParametricRef::point(handle, marker).segment_center_index();
        let polyline = matches!(
            self.document.get_entity(handle),
            Some(EntityType::LwPolyline(_) | EntityType::Polyline2D(_))
        );
        let (curve_id, point_type) = if let Some(segment) = segment_center {
            (
                self.segment_node(handle, segment)?,
                implicit_point_type::CENTER,
            )
        } else if let Some(segment) = segment_midpoint {
            (
                self.segment_node(handle, segment)?,
                implicit_point_type::MID,
            )
        } else if polyline && marker >= 0 {
            let entity = self.document.get_entity(handle)?;
            let points = super::dimension_assoc::source_points(entity);
            let closed = match entity {
                EntityType::LwPolyline(polyline) => polyline.is_closed,
                EntityType::Polyline2D(polyline) => polyline.is_closed(),
                _ => false,
            };
            let vertex = marker as usize;
            if vertex >= points.len() {
                return None;
            }
            let (segment, point_type) = if vertex + 1 < points.len() || closed {
                (vertex, implicit_point_type::START)
            } else {
                (vertex.checked_sub(1)?, implicit_point_type::END)
            };
            (self.segment_node(handle, segment)?, point_type)
        } else {
            let curve_id = self.geometry_node(handle)?;
            if let Some(existing) = self
                .entities
                .get(&handle)
                .and_then(|entity| entity.points.get(&marker))
            {
                return Some(*existing);
            }
            let point_type = match marker {
                0 => implicit_point_type::START,
                1 => implicit_point_type::END,
                -2 => implicit_point_type::MID,
                -3 => implicit_point_type::CENTER,
                _ => return None,
            };
            (curve_id, point_type)
        };
        let node_id = self.alloc_node_id();
        self.push_node(
            node_id,
            "AcConstrainedImplicitPoint",
            AssocConstraintNodeData::ImplicitPoint {
                geometry_dependency: Handle::NULL,
                geometry_node_id: node_id,
                point: None,
                point_type,
                point_index: -1,
                curve_id,
            },
        );
        let mut relations = vec![if point_type == implicit_point_type::CENTER {
            "AcCenterPointConstraint"
        } else {
            "AcPointCurveConstraint"
        }];
        if point_type == implicit_point_type::MID {
            relations.push("AcMidPointConstraint");
        }
        for class_name in relations {
            let relation_id = self.alloc_node_id();
            self.push_node(
                relation_id,
                class_name,
                AssocConstraintNodeData::Geometrical {
                    owner_id: 0,
                    is_implied: true,
                    is_active: true,
                },
            );
            self.connect(relation_id, node_id);
            self.connect(relation_id, curve_id);
        }
        self.entities
            .entry(handle)
            .or_default()
            .points
            .insert(marker, node_id);
        Some(node_id)
    }

    /// The node id `ParametricRef` resolves to: a point node for a marked
    /// reference, the whole geometry node otherwise.
    fn ref_node(&mut self, r: ParametricRef) -> Option<i32> {
        if r.directional_axis().is_some() {
            return self.directional_axis_node(r);
        }
        if r.segment_midpoint_index().is_some() {
            return self.point_node(r.entity, r.marker?);
        }
        if r.segment_center_index().is_some() {
            return self.point_node(r.entity, r.marker?);
        }
        if let Some(segment) = r.segment_index() {
            return self.segment_node(r.entity, segment);
        }
        if r.marker == Some(0)
            && matches!(
                self.document.get_entity(r.entity),
                Some(
                    EntityType::Point(_)
                        | EntityType::Insert(_)
                        | EntityType::Text(_)
                        | EntityType::MText(_)
                        | EntityType::AttributeDefinition(_)
                        | EntityType::AttributeEntity(_)
                        | EntityType::Table(_)
                )
            )
        {
            return self.geometry_node(r.entity);
        }
        match r.marker {
            Some(marker) => self.point_node(r.entity, marker),
            None => self.geometry_node(r.entity),
        }
    }

    fn segment_node(&mut self, handle: Handle, index: usize) -> Option<i32> {
        if let Some(node_id) = self
            .entities
            .get(&handle)
            .and_then(|entity| entity.segments.get(&index))
        {
            return Some(*node_id);
        }
        let entity = self.document.get_entity(handle)?;
        let points = super::dimension_assoc::source_points(entity);
        let (closed, bulge) = match entity {
            EntityType::LwPolyline(polyline) => {
                (polyline.is_closed, polyline.vertices.get(index)?.bulge)
            }
            EntityType::Polyline2D(polyline) => {
                (polyline.is_closed(), polyline.vertices.get(index)?.bulge)
            }
            _ => return None,
        };
        let start = *points.get(index)?;
        let end = if index + 1 < points.len() {
            points[index + 1]
        } else if closed {
            *points.first()?
        } else {
            return None;
        };
        let node_id = self.alloc_node_id();
        if bulge.abs() <= 1e-9 {
            self.push_node(
                node_id,
                "AcConstrainedBoundedLine",
                AssocConstraintNodeData::BoundedLine {
                    geometry_dependency: Handle::NULL,
                    geometry_node_id: node_id,
                    point: start,
                    direction: (end - start).normalize(),
                    is_ray: false,
                    start_point: start,
                    end_point: end,
                },
            );
        } else {
            let arc = cadkernel::geom2d::BulgeArc::from_bulge(
                [start.x, start.y],
                [end.x, end.y],
                bulge,
            )?;
            self.push_node(
                node_id,
                "AcConstrainedArc",
                AssocConstraintNodeData::Arc {
                    geometry_dependency: Handle::NULL,
                    geometry_node_id: node_id,
                    center: Vector3::new(arc.center[0], arc.center[1], start.z),
                    normal: Vector3::UNIT_Z,
                    direction: Vector3::UNIT_X,
                    radius: arc.radius,
                    start_parameter: arc.start_angle,
                    end_parameter: arc.start_angle + arc.sweep,
                    reserved: 0.0,
                    start_point: start,
                    end_point: end,
                },
            );
        }
        self.entities
            .entry(handle)
            .or_default()
            .segments
            .insert(index, node_id);
        if !self.referenced_entities.contains(&handle) {
            self.referenced_entities.push(handle);
        }
        Some(node_id)
    }

    fn datum_line(&mut self, direction: Vector3) -> i32 {
        let node_id = self.alloc_node_id();
        self.push_node(
            node_id,
            "AcConstrainedDatumLine",
            AssocConstraintNodeData::Line {
                geometry_dependency: Handle::NULL,
                geometry_node_id: node_id,
                point: Vector3::ZERO,
                direction: if direction.length_squared() > 1.0e-24 {
                    direction.normalize()
                } else {
                    Vector3::UNIT_X
                },
            },
        );
        node_id
    }

    /// Records an undirected edge on both nodes.
    fn connect(&mut self, a: i32, b: i32) {
        if let Some(node) = self.nodes.iter_mut().find(|n| n.node_id == a) {
            node.connections.push(b);
        }
        if let Some(node) = self.nodes.iter_mut().find(|n| n.node_id == b) {
            node.connections.push(a);
        }
    }
}

/// Class name for a plain `Geometrical`-shaped constraint.
fn geometrical_class_name(kind: ConstraintKind) -> Option<&'static str> {
    match kind {
        ConstraintKind::Coincident => Some("AcPointCoincidenceConstraint"),
        ConstraintKind::Perpendicular => Some("AcPerpendicularConstraint"),
        ConstraintKind::Tangent => Some("AcTangentConstraint"),
        // the native format's own `GeomConstraintType` enum (native SDK's
        // `AcGeomConstraint.h`) lists `kNormal` alongside `kPerpendicular`
        // as a distinct, real geometric-constraint kind.
        ConstraintKind::Normal => Some("AcNormalConstraint"),
        ConstraintKind::Concentric => Some("AcConcentricConstraint"),
        ConstraintKind::CenterPoint => Some("AcCenterPointConstraint"),
        ConstraintKind::Colinear => Some("AcColinearConstraint"),
        ConstraintKind::Fixed => Some("AcFixedConstraint"),
        ConstraintKind::Midpoint => Some("AcMidPointConstraint"),
        ConstraintKind::PointOnCurve => Some("AcPointCurveConstraint"),
        ConstraintKind::Symmetric => Some("AcSymmetricConstraint"),
        // Also a plain `Geometrical`-shaped node in the native format's own class list
        // (`is_plain_geometrical_constraint`) — unlike `Equal`, its class
        // name never branches on entity type, so it belongs here rather
        // than in the caller's per-entity-type dispatch.
        ConstraintKind::EqualDistance => Some("AcEqualDistanceConstraint"),
        // `Equal` needs the resolved entity types, so the caller handles it.
        ConstraintKind::Equal
        | ConstraintKind::Smooth
        | ConstraintKind::Horizontal
        | ConstraintKind::Vertical
        | ConstraintKind::Parallel
        | ConstraintKind::Distance
        | ConstraintKind::Angle
        | ConstraintKind::Radius
        | ConstraintKind::Diameter
        | ConstraintKind::DistanceX
        | ConstraintKind::DistanceY
        | ConstraintKind::DistanceDirected
        | ConstraintKind::Angle3Point
        | ConstraintKind::RigidSet => None,
    }
}

/// `Diameter`/`DistanceX`/`DistanceY` reuse `Radius`'/`Distance`'s own DWG
/// class — native implementations represents them as the same
/// `AcRadiusDiameterConstraint`/`AcDistanceConstraint` object with a
/// different `RadiusDiameterConstrType`/`DirectionType` mode byte (set in
/// `constraint_node` below), not a distinct native class.
const fn dimensional_class_name(kind: ConstraintKind) -> &'static str {
    match kind {
        ConstraintKind::Distance
        | ConstraintKind::DistanceX
        | ConstraintKind::DistanceY
        | ConstraintKind::DistanceDirected => "AcDistanceConstraint",
        ConstraintKind::Angle => "AcAngleConstraint",
        ConstraintKind::Angle3Point => "Ac3PointAngleConstraint",
        ConstraintKind::Radius | ConstraintKind::Diameter => "AcRadiusDiameterConstraint",
        _ => "",
    }
}

/// Builds one `AssocConstraintNode` for `constraint`, returning its node id
/// so the caller can track which ones still need a `value_dependency`
/// patched in once handles can be allocated (pass 2, `Allocator` — building
/// node *shape* only needs read access to resolve `Equal`'s line-vs-circle
/// split, `AssocGeomDependency`/`AssocValueDependency` handles need mutable
/// access to the same document, so this has to be two passes). `None` if
/// this constraint's refs don't resolve to nodes this pass supports
/// (mirrors `parametric_solve::build_constraint`'s own "skip, don't error"
/// contract for an unbuildable constraint).
fn constraint_node(
    builder: &mut GroupBuilder,
    document: &CadDocument,
    constraint: &ParametricConstraint,
    needs_value_dependency: &mut Vec<(i32, Option<DrivingValue>)>,
) -> Option<i32> {
    let refs: &[ParametricRef] = &constraint.refs;
    if constraint.kind == ConstraintKind::Smooth {
        let (first, second, second_curve_ref) = match refs {
            [first, second] => (*first, *second, ParametricRef::whole(second.entity)),
            [first, second, second_curve] if second.entity == second_curve.entity => {
                (*first, *second, *second_curve)
            }
            _ => return None,
        };
        let endpoint_parameter = |reference: ParametricRef| {
            let entity = document.get_entity(reference.entity)?;
            let EntityType::Spline(spline) = entity else {
                return Some(None);
            };
            if spline.flags.closed || spline.flags.periodic {
                return None;
            }
            let curve = crate::entities::spline::nurbs3(spline)?;
            let value = match reference.marker {
                Some(0) => curve.knots().get(curve.degree()).copied(),
                Some(1) => curve
                    .knots()
                    .get(curve.knots().len().checked_sub(curve.degree() + 1)?)
                    .copied(),
                _ => None,
            }?;
            Some(Some(value))
        };
        let first_parameter = endpoint_parameter(first)?;
        let second_parameter = endpoint_parameter(second)?;
        if first_parameter.is_none() && second_parameter.is_none() {
            return None;
        }
        let first_curve = builder.geometry_node(first.entity)?;
        let second_curve = builder.ref_node(second_curve_ref)?;
        let first_point = builder.ref_node(first)?;
        let second_point = builder.ref_node(second)?;

        let coincidence = builder.alloc_node_id();
        builder.push_node(
            coincidence,
            "AcPointCoincidenceConstraint",
            AssocConstraintNodeData::Geometrical {
                owner_id: 0,
                is_implied: false,
                is_active: constraint.enabled,
            },
        );
        builder.connect(coincidence, first_point);
        builder.connect(coincidence, second_point);

        let add_help_pair = |builder: &mut GroupBuilder, constraint_class| {
            let mut add_help = |value: Option<f64>| {
                value.map(|value| {
                    let node_id = builder.alloc_node_id();
                    builder.push_node(
                        node_id,
                        "AcHelpParameter",
                        AssocConstraintNodeData::HelpParameter {
                            value,
                            reserved: false,
                        },
                    );
                    node_id
                })
            };
            let first_help = add_help(first_parameter);
            let second_help = add_help(second_parameter);
            let child = builder.alloc_node_id();
            builder.push_node(
                child,
                constraint_class,
                AssocConstraintNodeData::Geometrical {
                    owner_id: 0,
                    is_implied: false,
                    is_active: constraint.enabled,
                },
            );
            for target in [
                Some(first_curve),
                Some(second_curve),
                first_help,
                second_help,
            ]
            .into_iter()
            .flatten()
            {
                builder.connect(child, target);
            }
            if let Some(first_help) = first_help {
                builder.connect(first_help, first_curve);
            }
            if let Some(second_help) = second_help {
                builder.connect(second_help, second_curve);
            }
            child
        };
        let tangent = add_help_pair(builder, "AcTangentConstraint");
        let equal_curvature = add_help_pair(builder, "AcEqualCurvatureConstraint");
        let node_id = builder.alloc_node_id();
        builder.push_node(
            node_id,
            "AcG2SmoothConstraint",
            AssocConstraintNodeData::Composite {
                owner_id: 0,
                is_implied: false,
                is_active: constraint.enabled,
                owned_constraint_ids: vec![coincidence, tangent, equal_curvature],
            },
        );
        for child_id in [coincidence, tangent, equal_curvature] {
            if let Some(AssocConstraintNode {
                data: AssocConstraintNodeData::Geometrical { owner_id, .. },
                ..
            }) = builder
                .nodes
                .iter_mut()
                .find(|node| node.node_id == child_id)
            {
                *owner_id = node_id;
            }
        }
        return Some(node_id);
    }
    if let Some(class_name) = geometrical_class_name(constraint.kind) {
        let target_refs: Vec<i32> = refs.iter().filter_map(|r| builder.ref_node(*r)).collect();
        if target_refs.len() != refs.len() {
            return None;
        }
        let node_id = builder.alloc_node_id();
        builder.push_node(
            node_id,
            class_name,
            AssocConstraintNodeData::Geometrical {
                owner_id: 0,
                is_implied: false,
                is_active: constraint.enabled,
            },
        );
        for target in &target_refs {
            builder.connect(node_id, *target);
        }
        return Some(node_id);
    }
    match constraint.kind {
        ConstraintKind::Horizontal | ConstraintKind::Vertical => {
            if !matches!(refs.len(), 1 | 2) {
                return None;
            }
            let targets: Vec<_> = refs
                .iter()
                .map(|reference| builder.ref_node(*reference))
                .collect::<Option<_>>()?;
            let horizontal = constraint.kind == ConstraintKind::Horizontal;
            let fallback = if horizontal {
                Vector3::UNIT_X
            } else {
                Vector3::UNIT_Y
            };
            let datum = builder.datum_line(constraint.axis_direction.unwrap_or(fallback));
            let node_id = builder.alloc_node_id();
            builder.push_node(
                node_id,
                if horizontal {
                    "AcHorizontalConstraint"
                } else {
                    "AcVerticalConstraint"
                },
                AssocConstraintNodeData::Parallel {
                    owner_id: 0,
                    is_implied: false,
                    is_active: constraint.enabled,
                    datum_line_index: Some(datum),
                },
            );
            for target in targets {
                builder.connect(node_id, target);
            }
            Some(node_id)
        }
        ConstraintKind::Parallel => {
            let [a, b] = refs else { return None };
            let (na, nb) = (builder.ref_node(*a)?, builder.ref_node(*b)?);
            let node_id = builder.alloc_node_id();
            builder.push_node(
                node_id,
                "AcParallelConstraint",
                AssocConstraintNodeData::Parallel {
                    owner_id: 0,
                    is_implied: false,
                    is_active: constraint.enabled,
                    datum_line_index: None,
                },
            );
            builder.connect(node_id, na);
            builder.connect(node_id, nb);
            Some(node_id)
        }
        ConstraintKind::Equal => {
            let [a, b] = refs else { return None };
            let (na, nb) = (builder.ref_node(*a)?, builder.ref_node(*b)?);
            let is_straight = |reference: &ParametricRef| {
                matches!(
                    document.get_entity(reference.entity),
                    Some(EntityType::Line(_))
                ) || (reference.segment_index().is_some()
                    && matches!(
                        document.get_entity(reference.entity),
                        Some(EntityType::LwPolyline(_) | EntityType::Polyline2D(_))
                    ))
            };
            let both_lines = is_straight(a) && is_straight(b);
            let class_name = if both_lines {
                "AcEqualLengthConstraint"
            } else {
                "AcEqualRadiusConstraint"
            };
            let node_id = builder.alloc_node_id();
            builder.push_node(
                node_id,
                class_name,
                AssocConstraintNodeData::Geometrical {
                    owner_id: 0,
                    is_implied: false,
                    is_active: constraint.enabled,
                },
            );
            builder.connect(node_id, na);
            builder.connect(node_id, nb);
            Some(node_id)
        }
        ConstraintKind::Distance
        | ConstraintKind::DistanceX
        | ConstraintKind::DistanceY
        | ConstraintKind::DistanceDirected
        | ConstraintKind::Angle
        | ConstraintKind::Angle3Point
        | ConstraintKind::Radius
        | ConstraintKind::Diameter => {
            let target_refs: Vec<i32> = match constraint.kind {
                ConstraintKind::Radius | ConstraintKind::Diameter => {
                    vec![builder.ref_node(*refs.first()?)?]
                }
                ConstraintKind::Angle3Point => {
                    let [a, vertex, b] = refs else { return None };
                    vec![
                        builder.ref_node(*a)?,
                        builder.ref_node(*vertex)?,
                        builder.ref_node(*b)?,
                    ]
                }
                ConstraintKind::DistanceDirected => {
                    if !matches!(refs.len(), 2 | 3) {
                        return None;
                    }
                    refs.iter()
                        .map(|reference| builder.ref_node(*reference))
                        .collect::<Option<_>>()?
                }
                _ => {
                    let [a, b] = refs else { return None };
                    vec![builder.ref_node(*a)?, builder.ref_node(*b)?]
                }
            };
            let owner_id = 0;
            let node_id = builder.alloc_node_id();
            let data = match constraint.kind {
                // Plain two-point distance: undirected (`kNotDirected`).
                ConstraintKind::Distance => AssocConstraintNodeData::Distance {
                    owner_id,
                    is_implied: false,
                    is_active: constraint.enabled,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    direction_type: distance_direction_type::UNDIRECTED,
                    distance: None,
                },
                // X-only/Y-only: `kFixedDirection` with the fixed unit
                // vector the distance is measured along — the native format's own
                // representation of DistanceX/DistanceY, not a distinct
                // constraint class.
                ConstraintKind::DistanceX => AssocConstraintNodeData::Distance {
                    owner_id,
                    is_implied: false,
                    is_active: constraint.enabled,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    direction_type: distance_direction_type::FIXED,
                    distance: Some(Vector3::new(1.0, 0.0, 0.0)),
                },
                ConstraintKind::DistanceY => AssocConstraintNodeData::Distance {
                    owner_id,
                    is_implied: false,
                    is_active: constraint.enabled,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    direction_type: distance_direction_type::FIXED,
                    distance: Some(Vector3::new(0.0, 1.0, 0.0)),
                },
                ConstraintKind::DistanceDirected => AssocConstraintNodeData::Distance {
                    owner_id,
                    is_implied: false,
                    is_active: constraint.enabled,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    direction_type: constraint.distance_direction_type,
                    distance: constraint.distance_direction,
                },
                ConstraintKind::Angle => AssocConstraintNodeData::Angle {
                    owner_id,
                    is_implied: false,
                    is_active: constraint.enabled,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    sector_type: constraint.angle_sector,
                },
                ConstraintKind::Angle3Point => AssocConstraintNodeData::Angle {
                    owner_id,
                    is_implied: false,
                    is_active: constraint.enabled,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    sector_type: constraint.angle_sector,
                },
                ConstraintKind::Radius => AssocConstraintNodeData::RadiusDiameter {
                    owner_id,
                    is_implied: false,
                    is_active: constraint.enabled,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    mode: 0,
                },
                // `kCircleDiameter` — same class as Radius (`mode: 0` /
                // `kCircleRadius`), different mode byte.
                ConstraintKind::Diameter => AssocConstraintNodeData::RadiusDiameter {
                    owner_id,
                    is_implied: false,
                    is_active: constraint.enabled,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    mode: 1,
                },
                _ => unreachable!(),
            };
            builder.push_node(node_id, dimensional_class_name(constraint.kind), data);
            for target in &target_refs {
                builder.connect(node_id, *target);
            }
            needs_value_dependency.push((node_id, constraint.driving_param.clone()));
            Some(node_id)
        }
        _ => None,
    }
}

/// Everything a scope's native graph needs beyond the group's own object:
/// the `AssocGeomDependency` and `AssocValueDependency`/`AssocVariable`
/// objects it references by handle, and the network/dictionary wiring.
struct Allocator<'a> {
    document: &'a mut CadDocument,
    /// One shared variable per distinct parameter in this scope.
    variables: FxHashMap<String, Handle>,
    variable_actions: Vec<Handle>,
}

impl Allocator<'_> {
    fn numeric_eval(value: f64) -> AssocEvalVariant {
        if value.fract() == 0.0 && value >= i32::MIN as f64 && value <= i32::MAX as f64 {
            AssocEvalVariant {
                code: 90,
                value: AssocEvalValue::Long(value as i32),
            }
        } else {
            AssocEvalVariant {
                code: 40,
                value: AssocEvalValue::Real(value),
            }
        }
    }

    fn insert_associative(
        &mut self,
        owner: Handle,
        dxf_name: &str,
        cpp_class_name: &str,
        data: AssociativeData,
    ) -> Handle {
        let handle = self.document.allocate_handle();
        self.insert_associative_at(handle, owner, dxf_name, cpp_class_name, data);
        handle
    }

    /// Like [`Allocator::insert_associative`], but for a handle already
    /// allocated up front — needed for the group/network pair, which
    /// reference each other (`AssocAction::owning_network` /
    /// `AssocNetwork::owned_actions`) and so must both have real handles
    /// before either object is actually built.
    fn insert_associative_at(
        &mut self,
        handle: Handle,
        owner: Handle,
        dxf_name: &str,
        cpp_class_name: &str,
        data: AssociativeData,
    ) {
        let object = AssociativeObject {
            handle,
            owner,
            dxf_name: dxf_name.to_string(),
            cpp_class_name: cpp_class_name.to_string(),
            data,
            ..Default::default()
        };
        self.document
            .objects
            .insert(handle, ObjectType::Associative(object));
    }

    fn geom_dependency(
        &mut self,
        group_handle: Handle,
        entity: Handle,
        dependency_id: i32,
    ) -> Handle {
        self.insert_associative(
            group_handle,
            "ACDBASSOCGEOMDEPENDENCY",
            "AcDbAssocGeomDependency",
            AssociativeData::GeomDependency(AssocGeomDependency {
                dependency: AssocDependency {
                    class_version: 2,
                    is_read_dependency: true,
                    is_write_dependency: true,
                    is_attached_to_object: true,
                    is_delegating_to_owning_action: true,
                    order: -10_000,
                    dependent_on: entity,
                    dependency_body_id: dependency_id,
                    ..Default::default()
                },
                class_version: 0,
                enabled: true,
                // Point and curve addressing lives on constraint-node data.
                persistent_subent: AssocPersistentSubentId::default(),
            }),
        )
    }

    /// Returns the `AssocVariable` handle for `name`, creating it once per scope.
    fn variable(
        &mut self,
        network_handle: Handle,
        name: &str,
        formula: &str,
        resolved: f64,
        description: &str,
    ) -> Handle {
        if let Some(&handle) = self.variables.get(name) {
            if let Some(ObjectType::Associative(object)) = self.document.objects.get_mut(&handle) {
                object.owner = network_handle;
                if let AssociativeData::Variable(variable) = &mut object.data {
                    variable.action.owning_network = network_handle;
                    variable.expression = formula.to_string();
                    variable.value = Self::numeric_eval(resolved);
                }
            }
            return handle;
        }
        let handle = self.insert_associative(
            network_handle,
            "ACDBASSOCVARIABLE",
            "AcDbAssocVariable",
            AssociativeData::Variable(AssocVariable {
                action: AssocAction {
                    class_version: 2,
                    owning_network: network_handle,
                    action_index: self.variable_actions.len() as i32 + 1,
                    ..Default::default()
                },
                class_version: 2,
                name: name.to_string(),
                expression: formula.to_string(),
                evaluator: "AcDbCalc:1.0".to_string(),
                description: description.to_string(),
                value: Self::numeric_eval(resolved),
                has_cached_value: false,
                cached_value: String::new(),
                flag: false,
                reserved: 0,
            }),
        );
        self.variables.insert(name.to_string(), handle);
        self.variable_actions.push(handle);
        handle
    }

    fn value_dependency(
        &mut self,
        group_handle: Handle,
        variable_handle: Handle,
        resolved: f64,
        dependency_id: i32,
    ) -> Handle {
        let value = Self::numeric_eval(resolved);
        let handle = self.insert_associative(
            group_handle,
            "ACDBASSOCVALUEDEPENDENCY",
            "AcDbAssocValueDependency",
            AssociativeData::ValueDependency(AssocValueDependency {
                dependency: AssocDependency {
                    class_version: 2,
                    is_read_dependency: true,
                    is_write_dependency: false,
                    is_attached_to_object: true,
                    is_delegating_to_owning_action: true,
                    dependent_on: variable_handle,
                    dependency_body_id: dependency_id,
                    ..Default::default()
                },
                class_version: 0,
                name: String::new(),
                value,
            }),
        );
        if let Some(ObjectType::Associative(variable)) =
            self.document.objects.get_mut(&variable_handle)
        {
            variable.reactors.push(handle);
        }
        handle
    }

    /// The `AcDbAssocDependency` + `AcDbAssocDimDependencyBody` pair that
    /// ties a dimensional constraint to the dynamic dimension showing it.
    fn dimension_dependency(
        &mut self,
        group_handle: Handle,
        dimension: Handle,
        name: &str,
        dependency_id: i32,
    ) -> Handle {
        let handle = self.document.allocate_handle();
        let body = self.insert_associative(
            handle,
            "ACDBASSOCDIMDEPENDENCYBODY",
            "AcDbAssocDimDependencyBody",
            AssociativeData::DimDependencyBody(AssocDimDependencyBody {
                dependency_body_version: 1,
                base_version: 1,
                name: name.to_string(),
                class_version: 1,
            }),
        );
        self.insert_associative_at(
            handle,
            group_handle,
            "ACDBASSOCDEPENDENCY",
            "AcDbAssocDependency",
            AssociativeData::Dependency(AssocDependency {
                class_version: 2,
                is_read_dependency: true,
                is_write_dependency: true,
                is_attached_to_object: true,
                is_delegating_to_owning_action: true,
                dependent_on: dimension,
                dependency_body: body,
                dependency_body_id: dependency_id,
                ..Default::default()
            }),
        );
        // Earlier materializations' dependencies are gone by now; drop
        // their reactor entries before adding this one.
        let live: Vec<Handle> = self
            .document
            .get_entity(dimension)
            .map(|entity| {
                entity
                    .common()
                    .reactors
                    .iter()
                    .copied()
                    .filter(|reactor| self.document.objects.contains_key(reactor))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(entity) = self.document.get_entity_mut(dimension) {
            let reactors = &mut entity.common_mut().reactors;
            *reactors = live;
            reactors.push(handle);
        }
        handle
    }
}

/// Sets the dependency on whole-geometry nodes. Implicit points retain a
/// null dependency and refer to their owning geometry node by `curve_id`.
fn set_geometry_dependency(data: &mut AssocConstraintNodeData, handle: Handle) {
    match data {
        AssocConstraintNodeData::Point {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::Line {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::BoundedLine {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::Circle {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::Arc {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::Ellipse {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::BoundedEllipse {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::Spline {
            geometry_dependency,
            ..
        } => {
            *geometry_dependency = handle;
        }
        _ => {}
    }
}

/// Patches a dimensional-constraint node's `value_dependency` field
/// (`Distance`/`Angle`/`RadiusDiameter` — the three kinds
/// [`constraint_node`] defers to `needs_value_dependency`).
fn set_value_dependency(data: &mut AssocConstraintNodeData, handle: Handle) {
    match data {
        AssocConstraintNodeData::Distance {
            value_dependency, ..
        }
        | AssocConstraintNodeData::Angle {
            value_dependency, ..
        }
        | AssocConstraintNodeData::RadiusDiameter {
            value_dependency, ..
        } => {
            *value_dependency = handle;
        }
        _ => {}
    }
}

/// Patches a dimensional-constraint node's `dimension_dependency` field.
fn set_dimension_dependency(data: &mut AssocConstraintNodeData, handle: Handle) {
    match data {
        AssocConstraintNodeData::Distance {
            dimension_dependency,
            ..
        }
        | AssocConstraintNodeData::Angle {
            dimension_dependency,
            ..
        }
        | AssocConstraintNodeData::RadiusDiameter {
            dimension_dependency,
            ..
        } => {
            *dimension_dependency = handle;
        }
        _ => {}
    }
}

/// Associative classes that must have real class numbers in DWG/DXF.
const ASSOC_CLASSES: &[(&str, &str, i32)] = &[
    (
        "ACDBASSOC2DCONSTRAINTGROUP",
        "AcDbAssoc2dConstraintGroup",
        45,
    ),
    ("ACDBASSOCNETWORK", "AcDbAssocNetwork", 45),
    ("ACDBASSOCVARIABLE", "AcDbAssocVariable", 45),
    ("ACDBASSOCGEOMDEPENDENCY", "AcDbAssocGeomDependency", 29),
    ("ACDBASSOCVALUEDEPENDENCY", "AcDbAssocValueDependency", 29),
    ("ACDBASSOCDEPENDENCY", "AcDbAssocDependency", 29),
    ("ACDBASSOCDIMDEPENDENCYBODY", "AcDbAssocDimDependencyBody", 45),
];

fn ensure_associative_classes_registered(document: &mut CadDocument) {
    for (dxf_name, cpp_class_name, maintenance_version) in ASSOC_CLASSES {
        if document.classes.get_by_name(dxf_name).is_some() {
            continue;
        }
        document.classes.add_or_update(acadrust::classes::DxfClass {
            dxf_name: dxf_name.to_string(),
            cpp_class_name: cpp_class_name.to_string(),
            application_name: "ObjectDBX Classes".to_string(),
            proxy_flags: acadrust::classes::ProxyFlags(
                acadrust::classes::ProxyFlags::ERASE_ALLOWED.0
                    | acadrust::classes::ProxyFlags::CLONING_ALLOWED.0
                    | acadrust::classes::ProxyFlags::DISABLES_PROXY_WARNING_DIALOG.0,
            ),
            instance_count: 0,
            was_zombie: false,
            is_an_entity: false,
            class_number: 0,
            // 499 = "object" (non-entity) — every class here is one.
            item_class_id: 0x1F3,
            dwg_version: 27,
            maintenance_version: *maintenance_version,
            unknown1: 0,
            unknown2: 0,
        });
    }
}

fn ensure_extension_dictionary(document: &mut CadDocument, owner: Handle) -> Handle {
    document.ensure_extension_dictionary(owner)
}

/// Removes a dictionary entry and its owned object tree.
fn remove_dictionary_entry(document: &mut CadDocument, dictionary_handle: Handle, key: &str) {
    let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary_handle)
    else {
        return;
    };
    let Some(index) = dictionary
        .entries
        .iter()
        .position(|(name, _)| name.eq_ignore_ascii_case(key))
    else {
        return;
    };
    let (_, target) = dictionary.entries.remove(index);
    remove_owned_recursive(document, target);
}

fn set_dictionary_entry(
    document: &mut CadDocument,
    dictionary_handle: Handle,
    key: &str,
    target: Handle,
) {
    let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary_handle)
    else {
        return;
    };
    if let Some((_, handle)) = dictionary
        .entries
        .iter_mut()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
    {
        *handle = target;
    } else {
        dictionary.add_entry(key, target);
    }
}

fn ensure_global_network_dictionary(document: &mut CadDocument) -> Handle {
    let root_handle = document.header.named_objects_dict_handle;
    if let Some(ObjectType::Dictionary(root)) = document.objects.get(&root_handle) {
        if let Some(handle) = root.get(NETWORK_DICTIONARY_KEY) {
            if matches!(
                document.objects.get(&handle),
                Some(ObjectType::Dictionary(_))
            ) {
                return handle;
            }
        }
    }

    remove_dictionary_entry(document, root_handle, NETWORK_DICTIONARY_KEY);
    let dictionary_handle = document.allocate_handle();
    let mut dictionary = acadrust::objects::Dictionary::new();
    dictionary.handle = dictionary_handle;
    dictionary.owner = root_handle;
    dictionary.reactors.push(root_handle);
    document
        .objects
        .insert(dictionary_handle, ObjectType::Dictionary(dictionary));
    set_dictionary_entry(
        document,
        root_handle,
        NETWORK_DICTIONARY_KEY,
        dictionary_handle,
    );
    dictionary_handle
}

/// Removes `root` and every object transitively owned by it so rebuilding the
/// graph cannot accumulate orphaned dependency or variable objects.
fn remove_owned_recursive(document: &mut CadDocument, root: Handle) {
    if root.is_null() {
        return;
    }
    let children: Vec<Handle> = document
        .objects
        .keys()
        .copied()
        .filter(|&h| document.object_owner(h) == Some(root))
        .collect();
    for child in children {
        remove_owned_recursive(document, child);
    }
    document.objects.remove(&root);
}

fn native_group_handles(document: &CadDocument, owner: Handle) -> Vec<Handle> {
    let Some(network_handle) = native_scope_network_handle(document, owner) else {
        return Vec::new();
    };
    let mut handles = Vec::new();
    if let Some(ObjectType::Associative(AssociativeObject {
        data: AssociativeData::Network(network),
        ..
    })) = document.objects.get(&network_handle)
    {
        handles.extend(network.actions.iter().filter_map(|action| {
            matches!(
                document.objects.get(&action.dependency),
                Some(ObjectType::Associative(AssociativeObject {
                    data: AssociativeData::ConstraintGroup(_),
                    ..
                }))
            )
            .then_some(action.dependency)
        }));
    }
    for (handle, object) in &document.objects {
        let ObjectType::Associative(object) = object else {
            continue;
        };
        let AssociativeData::ConstraintGroup(group) = &object.data else {
            continue;
        };
        if (object.owner == network_handle || group.action.owning_network == network_handle)
            && !handles.contains(handle)
        {
            handles.push(*handle);
        }
    }
    handles
}

fn native_scope_network_handle(document: &CadDocument, owner: Handle) -> Option<Handle> {
    let dictionary = document.extension_dictionary_handle(owner)?;
    let ObjectType::Dictionary(dictionary) = document.objects.get(&dictionary)? else {
        return None;
    };
    let mut network_handle = dictionary.get(NETWORK_DICTIONARY_KEY)?;
    while let Some(ObjectType::Dictionary(dictionary)) = document.objects.get(&network_handle) {
        network_handle = dictionary.get(NETWORK_DICTIONARY_KEY)?;
    }
    matches!(
        document.objects.get(&network_handle),
        Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Network(_),
            ..
        }))
    )
    .then_some(network_handle)
}

fn geometry_dependency(data: &AssocConstraintNodeData) -> Option<Handle> {
    match data {
        AssocConstraintNodeData::Point {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::Line {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::BoundedLine {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::Circle {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::Arc {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::Ellipse {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::BoundedEllipse {
            geometry_dependency,
            ..
        }
        | AssocConstraintNodeData::Spline {
            geometry_dependency,
            ..
        } if !geometry_dependency.is_null() => Some(*geometry_dependency),
        _ => None,
    }
}

fn dependency_entity(
    document: &CadDocument,
    dependency: Handle,
    geometry: &AssocConstraintNodeData,
) -> Option<Handle> {
    let ObjectType::Associative(object) = document.objects.get(&dependency)? else {
        return None;
    };
    let AssociativeData::GeomDependency(dependency) = &object.data else {
        return None;
    };
    let entity = dependency.dependency.dependent_on;
    let supported = match (document.get_entity(entity), geometry) {
        (
            Some(
                EntityType::Point(_)
                | EntityType::Insert(_)
                | EntityType::Text(_)
                | EntityType::MText(_)
                | EntityType::AttributeDefinition(_)
                | EntityType::AttributeEntity(_)
                | EntityType::Table(_),
            ),
            AssocConstraintNodeData::Point { .. },
        ) => true,
        (
            Some(EntityType::Text(_) | EntityType::MText(_) | EntityType::Ellipse(_)),
            AssocConstraintNodeData::BoundedLine { .. },
        ) => true,
        (Some(EntityType::Line(_)), AssocConstraintNodeData::BoundedLine { is_ray, .. }) => !is_ray,
        (
            Some(EntityType::LwPolyline(_) | EntityType::Polyline2D(_)),
            AssocConstraintNodeData::BoundedLine { .. } | AssocConstraintNodeData::Arc { .. },
        ) => true,
        (Some(EntityType::Ray(_)), AssocConstraintNodeData::BoundedLine { is_ray, .. }) => *is_ray,
        (Some(EntityType::XLine(_)), AssocConstraintNodeData::Line { .. }) => true,
        (Some(EntityType::Circle(_)), AssocConstraintNodeData::Circle { .. }) => true,
        (Some(EntityType::Arc(_)), AssocConstraintNodeData::Arc { .. }) => true,
        (
            Some(EntityType::Ellipse(_)),
            AssocConstraintNodeData::Ellipse { .. }
            | AssocConstraintNodeData::BoundedEllipse { .. },
        ) => true,
        (Some(EntityType::Spline(_)), AssocConstraintNodeData::Spline { .. }) => true,
        _ => false,
    };
    supported.then_some(entity)
}

fn group_requires_preservation(document: &CadDocument, group: &Assoc2dConstraintGroup) -> bool {
    group.nodes.iter().any(|node| {
        if matches!(node.data, AssocConstraintNodeData::RigidSet { .. }) {
            return true;
        }
        if let Some(dependency) = geometry_dependency(&node.data) {
            if dependency_entity(document, dependency, &node.data).is_none() {
                return true;
            }
        }
        let class_name = node.class_name.to_ascii_uppercase();
        class_name.ends_with("CONSTRAINT")
            && constraint_kind(node).is_none()
            && !matches!(
                class_name.as_str(),
                "ACEQUALCURVATURECONSTRAINT" | "ACEQUALHELPPARAMETERCONSTRAINT"
            )
    })
}

fn work_plane_vector(work_plane: &[Vector3; 3], local: Vector3) -> Option<Vector3> {
    let [origin, axis_x, axis_y] = *work_plane;
    let plane = cadkernel::space::Plane::from_axes(
        [origin.x, origin.y, origin.z],
        [axis_x.x, axis_x.y, axis_x.z],
        [axis_y.x, axis_y.y, axis_y.z],
    );
    let mut world = cadkernel::space::Vec3::from(plane.vector_at([local.x, local.y]));
    if local.z != 0.0 {
        world = world + cadkernel::space::Vec3::from(plane.normal()?) * local.z;
    }
    let world = world.normalize()?;
    Some(Vector3::new(world.x, world.y, world.z))
}

fn polyline_segment_reference(
    document: &CadDocument,
    entity: Handle,
    data: &AssocConstraintNodeData,
    work_plane: &[Vector3; 3],
) -> Option<ParametricRef> {
    let (start_point, end_point) = match data {
        AssocConstraintNodeData::BoundedLine {
            start_point,
            end_point,
            ..
        }
        | AssocConstraintNodeData::Arc {
            start_point,
            end_point,
            ..
        } => (*start_point, *end_point),
        _ => return None,
    };
    let [origin, axis_x, axis_y] = *work_plane;
    let normal = Vector3::new(
        axis_x.y * axis_y.z - axis_x.z * axis_y.y,
        axis_x.z * axis_y.x - axis_x.x * axis_y.z,
        axis_x.x * axis_y.y - axis_x.y * axis_y.x,
    );
    let to_world = |point: Vector3| origin + axis_x * point.x + axis_y * point.y + normal * point.z;
    let start_point = to_world(start_point);
    let end_point = to_world(end_point);
    let entity_value = document.get_entity(entity)?;
    let closed = match entity_value {
        EntityType::LwPolyline(polyline) => polyline.is_closed,
        EntityType::Polyline2D(polyline) => polyline.is_closed(),
        _ => return None,
    };
    let points = super::dimension_assoc::source_points(entity_value);
    let segment_count = points.len().saturating_sub(usize::from(!closed));
    (0..segment_count)
        .min_by(|first, second| {
            let score = |index: usize| {
                let a = points[index];
                let b = points[(index + 1) % points.len()];
                let forward = (a - start_point).length_squared() + (b - end_point).length_squared();
                let reverse = (b - start_point).length_squared() + (a - end_point).length_squared();
                forward.min(reverse)
            };
            score(*first).total_cmp(&score(*second))
        })
        .map(|index| ParametricRef::segment(entity, index))
}

fn directional_axis_reference(
    document: &CadDocument,
    entity: Handle,
    data: &AssocConstraintNodeData,
    work_plane: &[Vector3; 3],
) -> Option<ParametricRef> {
    let AssocConstraintNodeData::BoundedLine {
        start_point,
        end_point,
        ..
    } = data
    else {
        return None;
    };
    match document.get_entity(entity)? {
        EntityType::Text(_) | EntityType::MText(_) => {
            Some(ParametricRef::text_baseline(entity))
        }
        EntityType::Ellipse(ellipse) => {
            let [origin, axis_x, axis_y] = *work_plane;
            let normal = Vector3::new(
                axis_x.y * axis_y.z - axis_x.z * axis_y.y,
                axis_x.z * axis_y.x - axis_x.x * axis_y.z,
                axis_x.x * axis_y.y - axis_x.y * axis_y.x,
            );
            let to_world = |point: Vector3| {
                origin + axis_x * point.x + axis_y * point.y + normal * point.z
            };
            let direction = (to_world(*end_point) - to_world(*start_point)).normalize();
            let major = ellipse.major_axis.normalize();
            let ellipse_normal = ellipse.normal.normalize();
            let minor = Vector3::new(
                ellipse_normal.y * major.z - ellipse_normal.z * major.y,
                ellipse_normal.z * major.x - ellipse_normal.x * major.z,
                ellipse_normal.x * major.y - ellipse_normal.y * major.x,
            );
            if direction.dot(&minor).abs() > direction.dot(&major).abs() {
                Some(ParametricRef::ellipse_minor_axis(entity))
            } else {
                Some(ParametricRef::ellipse_major_axis(entity))
            }
        }
        _ => None,
    }
}

fn numeric_value(value: &AssocEvalVariant) -> Option<f64> {
    match value.value {
        AssocEvalValue::Real(value) => Some(value),
        AssocEvalValue::Long(value) => Some(value as f64),
        AssocEvalValue::Short(value) => Some(value as f64),
        AssocEvalValue::Byte(value) => Some(value as f64),
        _ => None,
    }
}

fn driving_value(
    document: &CadDocument,
    dependency: Handle,
    parameters: &mut ParameterTable,
) -> Option<DrivingValue> {
    let ObjectType::Associative(object) = document.objects.get(&dependency)? else {
        return None;
    };
    let AssociativeData::ValueDependency(value_dependency) = &object.data else {
        return None;
    };
    let literal = numeric_value(&value_dependency.value);
    let ObjectType::Associative(variable) = document
        .objects
        .get(&value_dependency.dependency.dependent_on)?
    else {
        return literal.map(DrivingValue::Literal);
    };
    let AssociativeData::Variable(variable) = &variable.data else {
        return literal.map(DrivingValue::Literal);
    };
    let source = if variable.expression.trim().is_empty() {
        numeric_value(&variable.value)?.to_string()
    } else {
        variable.expression.clone()
    };
    if parameters.set(&variable.name, &source).is_ok() {
        parameters.set_description(&variable.name, &variable.description);
        Some(DrivingValue::Named(variable.name.clone()))
    } else {
        literal
            .or_else(|| numeric_value(&variable.value))
            .map(DrivingValue::Literal)
    }
}

/// The dynamic dimension an `AcDbAssocDependency` ties to a dimensional
/// constraint node.
fn dependency_dimension(document: &CadDocument, dependency: Handle) -> Option<Handle> {
    let ObjectType::Associative(object) = document.objects.get(&dependency)? else {
        return None;
    };
    let AssociativeData::Dependency(dependency) = &object.data else {
        return None;
    };
    let target = dependency.dependent_on;
    matches!(document.get_entity(target)?, EntityType::Dimension(_)).then_some(target)
}

fn owning_composite(data: &AssocConstraintNodeData) -> Option<i32> {
    match data {
        AssocConstraintNodeData::Geometrical { owner_id, .. }
        | AssocConstraintNodeData::Composite { owner_id, .. }
        | AssocConstraintNodeData::Angle { owner_id, .. }
        | AssocConstraintNodeData::Parallel { owner_id, .. }
        | AssocConstraintNodeData::Distance { owner_id, .. }
        | AssocConstraintNodeData::RadiusDiameter { owner_id, .. } => Some(*owner_id),
        _ => None,
    }
}

fn constraint_is_active(data: &AssocConstraintNodeData) -> bool {
    match data {
        AssocConstraintNodeData::Geometrical { is_active, .. }
        | AssocConstraintNodeData::Composite { is_active, .. }
        | AssocConstraintNodeData::Angle { is_active, .. }
        | AssocConstraintNodeData::Parallel { is_active, .. }
        | AssocConstraintNodeData::Distance { is_active, .. }
        | AssocConstraintNodeData::RadiusDiameter { is_active, .. } => *is_active,
        _ => true,
    }
}

fn constraint_is_implied(data: &AssocConstraintNodeData) -> bool {
    match data {
        AssocConstraintNodeData::Geometrical { is_implied, .. }
        | AssocConstraintNodeData::Composite { is_implied, .. }
        | AssocConstraintNodeData::Angle { is_implied, .. }
        | AssocConstraintNodeData::Parallel { is_implied, .. }
        | AssocConstraintNodeData::Distance { is_implied, .. }
        | AssocConstraintNodeData::RadiusDiameter { is_implied, .. } => *is_implied,
        _ => false,
    }
}

fn set_constraint_active(data: &mut AssocConstraintNodeData, active: bool) {
    match data {
        AssocConstraintNodeData::Geometrical { is_active, .. }
        | AssocConstraintNodeData::Composite { is_active, .. }
        | AssocConstraintNodeData::Angle { is_active, .. }
        | AssocConstraintNodeData::Parallel { is_active, .. }
        | AssocConstraintNodeData::Distance { is_active, .. }
        | AssocConstraintNodeData::RadiusDiameter { is_active, .. } => *is_active = active,
        _ => {}
    }
}

fn clear_removed_geometry_owner(data: &mut AssocConstraintNodeData, removed: &[i32]) {
    let owner = match data {
        AssocConstraintNodeData::ImplicitPoint {
            geometry_node_id, ..
        }
        | AssocConstraintNodeData::Point {
            geometry_node_id, ..
        }
        | AssocConstraintNodeData::RigidSet {
            geometry_node_id, ..
        }
        | AssocConstraintNodeData::Line {
            geometry_node_id, ..
        }
        | AssocConstraintNodeData::BoundedLine {
            geometry_node_id, ..
        }
        | AssocConstraintNodeData::Circle {
            geometry_node_id, ..
        }
        | AssocConstraintNodeData::Arc {
            geometry_node_id, ..
        }
        | AssocConstraintNodeData::Ellipse {
            geometry_node_id, ..
        }
        | AssocConstraintNodeData::BoundedEllipse {
            geometry_node_id, ..
        }
        | AssocConstraintNodeData::Spline {
            geometry_node_id, ..
        } => geometry_node_id,
        _ => return,
    };
    if removed.contains(owner) {
        *owner = 0;
    }
}

fn constraint_kind(node: &AssocConstraintNode) -> Option<ConstraintKind> {
    Some(match node.class_name.to_ascii_uppercase().as_str() {
        "ACCONSTRAINEDRIGIDSET" => ConstraintKind::RigidSet,
        "ACPOINTCOINCIDENCECONSTRAINT" => ConstraintKind::Coincident,
        "ACHORIZONTALCONSTRAINT" => ConstraintKind::Horizontal,
        "ACVERTICALCONSTRAINT" => ConstraintKind::Vertical,
        "ACPARALLELCONSTRAINT" => ConstraintKind::Parallel,
        "ACPERPENDICULARCONSTRAINT" => ConstraintKind::Perpendicular,
        "ACEQUALLENGTHCONSTRAINT" | "ACEQUALRADIUSCONSTRAINT" => ConstraintKind::Equal,
        "ACANGLECONSTRAINT" => ConstraintKind::Angle,
        "AC3POINTANGLECONSTRAINT" => ConstraintKind::Angle3Point,
        "ACTANGENTCONSTRAINT" => ConstraintKind::Tangent,
        "ACG2SMOOTHCONSTRAINT" => ConstraintKind::Smooth,
        "ACCONCENTRICCONSTRAINT" => ConstraintKind::Concentric,
        "ACCENTERPOINTCONSTRAINT" => ConstraintKind::CenterPoint,
        "ACCOLINEARCONSTRAINT" => ConstraintKind::Colinear,
        "ACMIDPOINTCONSTRAINT" => ConstraintKind::Midpoint,
        "ACFIXEDCONSTRAINT" => ConstraintKind::Fixed,
        "ACPOINTCURVECONSTRAINT" => ConstraintKind::PointOnCurve,
        "ACEQUALDISTANCECONSTRAINT" => ConstraintKind::EqualDistance,
        "ACSYMMETRICCONSTRAINT" => ConstraintKind::Symmetric,
        "ACNORMALCONSTRAINT" => ConstraintKind::Normal,
        "ACRADIUSDIAMETERCONSTRAINT" => {
            let AssocConstraintNodeData::RadiusDiameter { mode, .. } = node.data else {
                return None;
            };
            if mode == 1 {
                ConstraintKind::Diameter
            } else {
                ConstraintKind::Radius
            }
        }
        "ACDISTANCECONSTRAINT" => {
            let AssocConstraintNodeData::Distance {
                direction_type,
                distance,
                ..
            } = &node.data
            else {
                return None;
            };
            if *direction_type == distance_direction_type::UNDIRECTED {
                ConstraintKind::Distance
            } else if *direction_type == distance_direction_type::FIXED
                && distance.is_some_and(|direction| {
                    let length = direction.x.hypot(direction.y);
                    length > f64::EPSILON
                        && (direction.x / length - 1.0).abs() < 1.0e-9
                        && (direction.y / length).abs() < 1.0e-9
                })
            {
                ConstraintKind::DistanceX
            } else if *direction_type == distance_direction_type::FIXED
                && distance.is_some_and(|direction| {
                    let length = direction.x.hypot(direction.y);
                    length > f64::EPSILON
                        && (direction.x / length).abs() < 1.0e-9
                        && (direction.y / length - 1.0).abs() < 1.0e-9
                })
            {
                ConstraintKind::DistanceY
            } else {
                ConstraintKind::DistanceDirected
            }
        }
        _ => return None,
    })
}

fn rigid_set_references(
    document: &CadDocument,
    geometry_ids: &[i32],
    refs: &FxHashMap<i32, ParametricRef>,
) -> (Vec<ParametricRef>, Vec<(ParametricRef, Vector3)>) {
    let mut entities = Vec::new();
    let mut points = Vec::new();
    let mut add_point = |reference: ParametricRef, point: Vector3| {
        if !points.iter().any(|(existing, _)| *existing == reference) {
            points.push((reference, point));
        }
    };
    for geometry_id in geometry_ids {
        let Some(reference) = refs.get(geometry_id).copied() else {
            continue;
        };
        if !entities.iter().any(|existing| *existing == reference) {
            entities.push(reference);
        }
        let Some(entity) = document.get_entity(reference.entity) else {
            continue;
        };
        if let Some(marker) = reference.marker {
            if let Some(point) = super::parametric_constraints::resolve_point(entity, marker) {
                add_point(reference, point);
            }
            continue;
        }
        for (marker, point) in super::dimension_assoc::source_points(entity)
            .into_iter()
            .enumerate()
        {
            add_point(ParametricRef::point(reference.entity, marker as i32), point);
        }
        match entity {
            EntityType::Circle(circle) => {
                add_point(ParametricRef::center(reference.entity), circle.center_wcs())
            }
            EntityType::Arc(arc) => {
                add_point(ParametricRef::center(reference.entity), arc.center_wcs())
            }
            _ => {}
        }
    }
    (entities, points)
}

pub(super) fn native_constraint_set(
    document: &CadDocument,
    owner: Handle,
    parameters: &mut ParameterTable,
) -> Option<ParametricConstraintSet> {
    let groups: Vec<_> = native_group_handles(document, owner)
        .into_iter()
        .filter_map(|handle| match document.objects.get(&handle) {
            Some(ObjectType::Associative(AssociativeObject {
                data: AssociativeData::ConstraintGroup(group),
                ..
            })) => Some((handle, group, group_requires_preservation(document, group))),
            _ => None,
        })
        .collect();
    if groups.is_empty() {
        return None;
    }
    let scope = if owner == document.header.model_space_block_handle {
        ParametricScope::ModelSpace
    } else {
        ParametricScope::Block(owner)
    };
    let mut set = ParametricConstraintSet::new(scope);
    for (group_handle, group, retained) in groups {
        if retained {
            set.retained_standard_groups.push(group_handle);
        }
        let mut refs = FxHashMap::default();
        for node in &group.nodes {
            let Some(dependency) = geometry_dependency(&node.data) else {
                continue;
            };
            if let Some(entity) = dependency_entity(document, dependency, &node.data) {
                let reference =
                    polyline_segment_reference(document, entity, &node.data, &group.work_plane)
                        .or_else(|| {
                            directional_axis_reference(
                                document,
                                entity,
                                &node.data,
                                &group.work_plane,
                            )
                        })
                        .unwrap_or_else(|| {
                            if matches!(node.data, AssocConstraintNodeData::Point { .. }) {
                                ParametricRef::point(entity, 0)
                            } else {
                                ParametricRef::whole(entity)
                            }
                        });
                refs.insert(node.node_id, reference);
            }
        }
        for node in &group.nodes {
            let AssocConstraintNodeData::ImplicitPoint {
                point_type,
                point_index,
                curve_id,
                ..
            } = node.data
            else {
                continue;
            };
            let Some(curve) = refs.get(&curve_id).copied() else {
                continue;
            };
            let reference = if let Some(segment) = curve.segment_index() {
                let Some(entity) = document.get_entity(curve.entity) else {
                    continue;
                };
                let point_count = super::dimension_assoc::source_points(entity).len();
                match point_type {
                    implicit_point_type::START => {
                        ParametricRef::point(curve.entity, segment as i32)
                    }
                    implicit_point_type::END if point_count > 0 => {
                        ParametricRef::point(curve.entity, ((segment + 1) % point_count) as i32)
                    }
                    implicit_point_type::MID => {
                        ParametricRef::segment_midpoint(curve.entity, segment)
                    }
                    implicit_point_type::CENTER => {
                        ParametricRef::segment_center(curve.entity, segment)
                    }
                    _ => continue,
                }
            } else {
                let marker = match point_type {
                    implicit_point_type::START => 0,
                    implicit_point_type::END => 1,
                    implicit_point_type::MID => -2,
                    implicit_point_type::CENTER => -3,
                    implicit_point_type::DEFINE => {
                        let Some(point_count) = group.nodes.iter().find_map(|node| {
                            (node.node_id == curve_id)
                                .then_some(&node.data)
                                .and_then(|data| {
                                    let AssocConstraintNodeData::Spline { control_points, .. } =
                                        data
                                    else {
                                        return None;
                                    };
                                    Some(control_points.len())
                                })
                        }) else {
                            continue;
                        };
                        if point_index == 0 {
                            0
                        } else if usize::try_from(point_index).ok() == point_count.checked_sub(1) {
                            1
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };
                ParametricRef::point(curve.entity, marker)
            };
            refs.insert(node.node_id, reference);
        }

        for node in &group.nodes {
            let Some(kind) = constraint_kind(node) else {
                continue;
            };
            if owning_composite(&node.data).is_some_and(|owner| owner != 0) {
                continue;
            }
            if constraint_is_implied(&node.data) {
                continue;
            }
            let enabled = constraint_is_active(&node.data);
            let (targets, rigid_points) = match &node.data {
                AssocConstraintNodeData::RigidSet { geometry_ids, .. } => {
                    rigid_set_references(document, geometry_ids, &refs)
                }
                AssocConstraintNodeData::Composite {
                    owned_constraint_ids,
                    ..
                } if kind == ConstraintKind::Smooth => {
                    let mut endpoints: Vec<ParametricRef> = owned_constraint_ids
                        .iter()
                        .filter_map(|id| group.nodes.iter().find(|child| child.node_id == *id))
                        .find(|child| {
                            child
                                .class_name
                                .eq_ignore_ascii_case("AcPointCoincidenceConstraint")
                        })
                        .map(|child| {
                            child
                                .connections
                                .iter()
                                .filter_map(|id| refs.get(id).copied())
                                .collect()
                        })
                        .unwrap_or_default();
                    if let Some(target) = endpoints.get(1).copied() {
                        let segment = owned_constraint_ids
                            .iter()
                            .filter_map(|id| group.nodes.iter().find(|child| child.node_id == *id))
                            .filter(|child| {
                                child.class_name.eq_ignore_ascii_case("AcTangentConstraint")
                            })
                            .flat_map(|child| child.connections.iter())
                            .filter_map(|id| refs.get(id).copied())
                            .find(|reference| {
                                reference.entity == target.entity
                                    && reference.segment_index().is_some()
                            });
                        if let Some(segment) = segment {
                            endpoints.push(segment);
                        }
                    }
                    (endpoints, Vec::new())
                }
                _ => (
                    node.connections
                        .iter()
                        .filter_map(|id| refs.get(id).copied())
                        .collect(),
                    Vec::new(),
                ),
            };
            if matches!(
                kind,
                ConstraintKind::PointOnCurve | ConstraintKind::CenterPoint
            ) && targets.len() == 2
                && targets[0].entity == targets[1].entity
            {
                continue;
            }
            let parameter_table = if matches!(scope, ParametricScope::Block(_)) {
                &mut set.local_parameters
            } else {
                &mut *parameters
            };
            let driving = match &node.data {
                AssocConstraintNodeData::Angle {
                    value_dependency, ..
                }
                | AssocConstraintNodeData::Distance {
                    value_dependency, ..
                }
                | AssocConstraintNodeData::RadiusDiameter {
                    value_dependency, ..
                } => driving_value(document, *value_dependency, parameter_table),
                _ => None,
            };
            let valid_target_count = match kind {
                ConstraintKind::Horizontal | ConstraintKind::Vertical => {
                    matches!(targets.len(), 1 | 2)
                }
                ConstraintKind::Fixed | ConstraintKind::Radius | ConstraintKind::Diameter => {
                    targets.len() == 1
                }
                ConstraintKind::Symmetric | ConstraintKind::Angle3Point => targets.len() == 3,
                ConstraintKind::DistanceDirected => matches!(targets.len(), 2 | 3),
                ConstraintKind::EqualDistance => targets.len() == 4,
                ConstraintKind::RigidSet => !targets.is_empty() && rigid_points.len() >= 2,
                ConstraintKind::Smooth => matches!(targets.len(), 2 | 3),
                _ => targets.len() == 2,
            };
            if !valid_target_count {
                continue;
            }
            if matches!(
                kind,
                ConstraintKind::Distance
                    | ConstraintKind::DistanceX
                    | ConstraintKind::DistanceY
                    | ConstraintKind::DistanceDirected
                    | ConstraintKind::Angle
                    | ConstraintKind::Angle3Point
                    | ConstraintKind::Radius
                    | ConstraintKind::Diameter
            ) && driving.is_none()
            {
                continue;
            }
            let dimension = match &node.data {
                AssocConstraintNodeData::Angle {
                    dimension_dependency,
                    ..
                }
                | AssocConstraintNodeData::Distance {
                    dimension_dependency,
                    ..
                }
                | AssocConstraintNodeData::RadiusDiameter {
                    dimension_dependency,
                    ..
                } => dependency_dimension(document, *dimension_dependency),
                _ => None,
            };
            let id = set.add(kind, targets, driving);
            if let Some(dimension) = dimension {
                set.dimensions.insert(id, dimension);
            }
            if let Some(constraint) = set.constraints.last_mut() {
                constraint.enabled = enabled;
                constraint.rigid_points = rigid_points;
                if matches!(kind, ConstraintKind::Horizontal | ConstraintKind::Vertical) {
                    let datum_id = match &node.data {
                        AssocConstraintNodeData::Parallel {
                            datum_line_index, ..
                        } => *datum_line_index,
                        _ => None,
                    };
                    let local_direction = datum_id.and_then(|datum_id| {
                        group.nodes.iter().find_map(|datum| {
                            (datum.node_id == datum_id).then_some(&datum.data).and_then(|data| {
                                match data {
                                    AssocConstraintNodeData::Line { direction, .. } => {
                                        Some(*direction)
                                    }
                                    _ => None,
                                }
                            })
                        })
                    });
                    if let Some(world) =
                        local_direction.and_then(|local| work_plane_vector(&group.work_plane, local))
                    {
                        constraint.axis_direction = Some(world);
                    }
                }
                if let AssocConstraintNodeData::Distance {
                    direction_type,
                    distance,
                    ..
                } = &node.data
                {
                    constraint.distance_direction_type = *direction_type;
                    constraint.distance_direction = *distance;
                }
                if let AssocConstraintNodeData::Angle { sector_type, .. } = &node.data {
                    constraint.angle_sector = *sector_type;
                }
                if retained {
                    constraint.native_origin = Some(NativeConstraintOrigin {
                        group: group_handle,
                        node: node.node_id,
                    });
                }
            }
        }
    }
    (!set.constraints.is_empty() || !set.retained_standard_groups.is_empty()).then_some(set)
}

fn sync_retained_group_constraints(document: &mut CadDocument, set: &ParametricConstraintSet) {
    let retained: FxHashMap<(Handle, i32), bool> = set
        .constraints
        .iter()
        .filter_map(|constraint| {
            constraint
                .native_origin
                .map(|origin| ((origin.group, origin.node), constraint.enabled))
        })
        .collect();
    for &group_handle in &set.retained_standard_groups {
        let Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::ConstraintGroup(group),
            ..
        })) = document.objects.get_mut(&group_handle)
        else {
            continue;
        };
        let mut removed = Vec::new();
        for node in &mut group.nodes {
            if constraint_kind(node).is_none() || constraint_is_implied(&node.data) {
                continue;
            }
            match retained.get(&(group_handle, node.node_id)) {
                Some(active) => set_constraint_active(&mut node.data, *active),
                None => removed.push(node.node_id),
            }
        }
        if removed.is_empty() {
            continue;
        }
        group.nodes.retain(|node| !removed.contains(&node.node_id));
        for node in &mut group.nodes {
            node.connections.retain(|id| !removed.contains(id));
            clear_removed_geometry_owner(&mut node.data, &removed);
        }
    }
}

fn detach_preserved_scope(
    document: &mut CadDocument,
    owner: Handle,
) -> (FxHashMap<String, Handle>, Vec<Handle>) {
    let Some(network_handle) = native_scope_network_handle(document, owner) else {
        return (FxHashMap::default(), Vec::new());
    };
    let preserved_groups: Vec<_> = native_group_handles(document, owner)
        .into_iter()
        .filter(|handle| {
            matches!(
                document.objects.get(handle),
                Some(ObjectType::Associative(AssociativeObject {
                    data: AssociativeData::ConstraintGroup(group),
                    ..
                })) if group_requires_preservation(document, group)
            )
        })
        .collect();
    if preserved_groups.is_empty() {
        return (FxHashMap::default(), Vec::new());
    }

    let mut variable_handles = Vec::new();
    for group_handle in &preserved_groups {
        let Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::ConstraintGroup(group),
            ..
        })) = document.objects.get(group_handle)
        else {
            continue;
        };
        for dependency in &group.actions {
            let Some(ObjectType::Associative(AssociativeObject {
                data: AssociativeData::ValueDependency(value),
                ..
            })) = document.objects.get(dependency)
            else {
                continue;
            };
            let handle = value.dependency.dependent_on;
            if !handle.is_null() && !variable_handles.contains(&handle) {
                variable_handles.push(handle);
            }
        }
    }

    let dictionary = document
        .extension_dictionary_handle(owner)
        .unwrap_or(Handle::NULL);
    for handle in &preserved_groups {
        if let Some(ObjectType::Associative(object)) = document.objects.get_mut(handle) {
            object.owner = dictionary;
        }
    }
    let mut variables = FxHashMap::default();
    for handle in variable_handles {
        let Some(ObjectType::Associative(object)) = document.objects.get_mut(&handle) else {
            continue;
        };
        object.owner = dictionary;
        if let AssociativeData::Variable(variable) = &object.data {
            variables.insert(variable.name.clone(), handle);
        }
    }

    debug_assert!(document.objects.contains_key(&network_handle));
    (variables, preserved_groups)
}

/// Materializes one scope into the drawing's associative object graph.
fn materialize_scope(
    document: &mut CadDocument,
    owner: Handle,
    set: &ParametricConstraintSet,
    parameters: &ParameterTable,
    root_network_handle: Handle,
    action_index: i32,
    existing_variables: FxHashMap<String, Handle>,
    preserved_groups: Vec<Handle>,
    materialize_all_parameters: bool,
) -> Option<Handle> {
    let dictionary_handle = ensure_extension_dictionary(document, owner);
    remove_dictionary_entry(document, dictionary_handle, NETWORK_DICTIONARY_KEY);

    // Pass 1 (read-only): constraint-node shapes.
    let mut needs_value_dependency = Vec::new();
    let mut dimension_links = Vec::new();
    let (mut nodes, entities, referenced_entities) = {
        let mut builder = GroupBuilder::new(document);
        for constraint in &set.constraints {
            let node_id = constraint_node(
                &mut builder,
                document,
                constraint,
                &mut needs_value_dependency,
            );
            if let Some((node_id, dimension)) = node_id.zip(set.dimensions.get(&constraint.id)) {
                if document.get_entity(*dimension).is_some() {
                    dimension_links.push((node_id, *dimension));
                }
            }
        }
        let GroupBuilder {
            nodes,
            entities,
            referenced_entities,
            ..
        } = builder;
        (nodes, entities, referenced_entities)
    };
    if nodes.is_empty()
        && parameters.is_empty()
        && existing_variables.is_empty()
        && preserved_groups.is_empty()
    {
        return None;
    }

    if !nodes.is_empty() {
        nodes.insert(
            0,
            AssocConstraintNode {
                node_id: 0,
                status: 0,
                connections: Vec::new(),
                class_name: String::new(),
                registry_flag: false,
                data: AssocConstraintNodeData::None,
            },
        );
    }

    // Pass 2 (mutable): allocate handles for everything the node shapes
    // above still reference as `Handle::NULL`. The group and network
    // reference each other, so both handles are reserved up front.
    let group_handle = (!nodes.is_empty()).then(|| document.allocate_handle());
    let network_handle = document.allocate_handle();
    let mut allocator = Allocator {
        document,
        variable_actions: existing_variables.values().copied().collect(),
        variables: existing_variables,
    };
    allocator.variable_actions.sort();
    for (index, handle) in allocator.variable_actions.iter().copied().enumerate() {
        if let Some(ObjectType::Associative(object)) = allocator.document.objects.get_mut(&handle) {
            object.owner = network_handle;
            if let AssociativeData::Variable(variable) = &mut object.data {
                variable.action.owning_network = network_handle;
                variable.action.action_index = index as i32 + 1;
                if let Some(parameter) = parameters.get(&variable.name) {
                    variable.expression = parameter.source.clone();
                    variable.description = parameters.description(&parameter.name).to_string();
                    if let Ok(resolved) = parameters.resolve(&parameter.name) {
                        variable.value = Allocator::numeric_eval(resolved);
                    }
                }
            }
        }
    }

    if materialize_all_parameters {
        for parameter in parameters.iter() {
            let resolved = parameters.resolve(&parameter.name).unwrap_or(0.0);
            allocator.variable(
                network_handle,
                &parameter.name,
                &parameter.source,
                resolved,
                parameters.description(&parameter.name),
            );
        }
    }

    let mut geometry_dependencies = Vec::with_capacity(referenced_entities.len());
    for (index, entity_handle) in referenced_entities.iter().enumerate() {
        let Some(entity_nodes) = entities.get(entity_handle) else {
            continue;
        };
        if entity_nodes.geometry_node_id == 0 {
            if entity_nodes.segments.is_empty() && entity_nodes.axes.is_empty() {
                continue;
            }
        }
        let dep_handle = allocator.geom_dependency(group_handle?, *entity_handle, index as i32 + 1);
        geometry_dependencies.push(dep_handle);
        for node_id in std::iter::once(entity_nodes.geometry_node_id)
            .filter(|node_id| *node_id != 0)
            .chain(entity_nodes.segments.values().copied())
            .chain(entity_nodes.axes.values().copied())
        {
            if let Some(node) = nodes.iter_mut().find(|node| node.node_id == node_id) {
                set_geometry_dependency(&mut node.data, dep_handle);
            }
        }
    }

    let mut group_dependencies = geometry_dependencies;
    let mut node_names: FxHashMap<i32, String> = FxHashMap::default();
    for (node_id, driving) in needs_value_dependency {
        let Some(driving) = driving else { continue };
        let Ok(resolved) = driving.resolve(parameters) else {
            continue;
        };
        let (name, formula) = match &driving {
            DrivingValue::Named(name) => {
                let formula = parameters
                    .get(name)
                    .map(|p| p.source.clone())
                    .unwrap_or_default();
                (name.clone(), formula)
            }
            DrivingValue::Literal(_) => (format!("d{node_id}"), resolved.to_string()),
        };
        node_names.insert(node_id, name.clone());
        let variable_handle = allocator.variable(
            network_handle,
            &name,
            &formula,
            resolved,
            parameters.description(&name),
        );
        let dependency_id = group_dependencies.len() as i32 + 1;
        let dep_handle =
            allocator.value_dependency(group_handle?, variable_handle, resolved, dependency_id);
        group_dependencies.push(dep_handle);
        if let Some(node) = nodes.iter_mut().find(|n| n.node_id == node_id) {
            set_value_dependency(&mut node.data, dep_handle);
        }
    }
    for (node_id, dimension) in dimension_links {
        let Some(name) = node_names.get(&node_id) else {
            continue;
        };
        let dependency_id = group_dependencies.len() as i32 + 1;
        let dep_handle =
            allocator.dimension_dependency(group_handle?, dimension, name, dependency_id);
        group_dependencies.push(dep_handle);
        if let Some(node) = nodes.iter_mut().find(|n| n.node_id == node_id) {
            set_dimension_dependency(&mut node.data, dep_handle);
        }
    }

    if let Some(group_handle) = group_handle {
        let group = Assoc2dConstraintGroup {
            action: AssocAction {
                class_version: 2,
                owning_network: network_handle,
                action_index: allocator.variable_actions.len() as i32 + 1,
                max_dependency_index: group_dependencies.len() as i32 + 1,
                ..Default::default()
            },
            version: 2,
            flag: false,
            work_plane: [Vector3::ZERO, Vector3::UNIT_X, Vector3::UNIT_Y],
            dependency: Handle::NULL,
            actions: group_dependencies,
            nodes,
        };
        allocator.insert_associative_at(
            group_handle,
            network_handle,
            "ASSOC2DCONSTRAINTGROUP",
            "AcDbAssoc2dConstraintGroup",
            AssociativeData::ConstraintGroup(group),
        );
    }

    for (offset, handle) in preserved_groups.iter().copied().enumerate() {
        if let Some(ObjectType::Associative(object)) = allocator.document.objects.get_mut(&handle) {
            object.owner = network_handle;
            if let AssociativeData::ConstraintGroup(group) = &mut object.data {
                group.action.owning_network = network_handle;
                group.action.action_index = allocator.variable_actions.len() as i32
                    + i32::from(group_handle.is_some())
                    + offset as i32
                    + 1;
            }
        }
    }

    let network = AssocNetwork {
        action: AssocAction {
            class_version: 2,
            owning_network: root_network_handle,
            action_index,
            ..Default::default()
        },
        network_version: 0,
        network_action_index: allocator.variable_actions.len() as i32
            + i32::from(group_handle.is_some())
            + preserved_groups.len() as i32,
        actions: allocator
            .variable_actions
            .iter()
            .copied()
            .chain(group_handle)
            .chain(preserved_groups)
            .map(|dependency| AssocActionDependency {
                is_owned: true,
                dependency,
            })
            .collect(),
        owned_actions: Vec::new(),
    };
    allocator.insert_associative_at(
        network_handle,
        dictionary_handle,
        "ASSOCNETWORK",
        "AcDbAssocNetwork",
        AssociativeData::Network(network),
    );
    if let Some(ObjectType::Associative(network)) =
        allocator.document.objects.get_mut(&network_handle)
    {
        network.reactors.push(dictionary_handle);
    }

    set_dictionary_entry(
        allocator.document,
        dictionary_handle,
        NETWORK_DICTIONARY_KEY,
        network_handle,
    );
    Some(network_handle)
}

impl Scene {
    pub(crate) fn load_named_parameters_from_document(&mut self) {
        self.named_parameters = ParameterTable::new();
        let mut variables: Vec<_> = self
            .document
            .objects
            .values()
            .filter_map(|object| match object {
                ObjectType::Associative(AssociativeObject {
                    handle,
                    data: AssociativeData::Variable(variable),
                    ..
                }) => Some((*handle, variable)),
                _ => None,
            })
            .collect();
        variables.sort_by_key(|(handle, _)| handle.value());
        let model_network = native_scope_network_handle(
            &self.document,
            self.document.header.model_space_block_handle,
        );
        for (_, variable) in variables {
            if model_network.is_none_or(|network| variable.action.owning_network != network) {
                continue;
            }
            let source = if variable.expression.trim().is_empty() {
                numeric_value(&variable.value).map(|value| value.to_string())
            } else {
                Some(variable.expression.clone())
            };
            if let Some(source) = source {
                let _ = self.named_parameters.set(&variable.name, &source);
            }
        }
    }

    pub(crate) fn load_parametric_constraints_from_document(&mut self) {
        self.parametric_constraints.clear();
        // Dynamic dimensions read from the file get their screen size on the
        // first tick after the view is known.
        self.dynamic_dimension_camera_gen = None;
        let owners: Vec<_> = self
            .document
            .block_records
            .iter()
            .map(|record| record.handle)
            .collect();
        for owner in owners {
            if let Some(set) =
                native_constraint_set(&self.document, owner, &mut self.named_parameters)
            {
                self.parametric_constraints.push(set);
            }
        }
        // Whole-set replace (file open / reload): covers the DWG-import
        // `enabled` / `axis_direction` / `distance_direction` / `angle_sector`
        // writes, which happen on the local set during decode.
        self.bump_constraints_epoch();
    }

    /// Makes the standard associative graph match the live command model.
    pub(crate) fn sync_native_parametric_graph(&mut self) {
        let mut owners: Vec<_> = self
            .document
            .block_records
            .iter()
            .map(|record| record.handle)
            .collect();
        for set in &self.parametric_constraints {
            let owner = set.scope.owner_handle(&self.document);
            if !owner.is_null() && !owners.contains(&owner) {
                owners.push(owner);
            }
        }
        let model_owner = self.document.header.model_space_block_handle;
        if !owners.contains(&model_owner) {
            owners.push(model_owner);
        }

        let mut scopes = Vec::new();
        for owner in owners {
            let existing = native_scope_network_handle(&self.document, owner).is_some();
            let runtime = self
                .parametric_constraints
                .iter()
                .find(|set| set.scope.owner_handle(&self.document) == owner)
                .cloned();
            let set = match runtime {
                Some(set) => Some(set),
                None if existing => {
                    native_constraint_set(&self.document, owner, &mut self.named_parameters)
                }
                None => None,
            };
            if set.is_none()
                && !existing
                && (owner != model_owner || self.named_parameters.is_empty())
            {
                continue;
            }
            if let Some(set) = &set {
                sync_retained_group_constraints(&mut self.document, set);
            }
            let (variables, groups) = detach_preserved_scope(&mut self.document, owner);
            let mut set = set.unwrap_or_else(|| {
                if owner == model_owner {
                    ParametricConstraintSet::new(ParametricScope::ModelSpace)
                } else {
                    ParametricConstraintSet::new(ParametricScope::Block(owner))
                }
            });
            set.constraints.retain(|constraint| {
                constraint
                    .native_origin
                    .is_none_or(|origin| !groups.contains(&origin.group))
            });
            for constraint in &mut set.constraints {
                constraint.native_origin = None;
            }
            scopes.push((owner, set, variables, groups));
        }
        if scopes.is_empty() {
            return;
        }

        ensure_associative_classes_registered(&mut self.document);
        let global_dictionary = ensure_global_network_dictionary(&mut self.document);
        remove_dictionary_entry(
            &mut self.document,
            global_dictionary,
            NETWORK_DICTIONARY_KEY,
        );
        let root_network_handle = self.document.allocate_handle();
        let mut child_networks = Vec::new();
        for (index, (owner, set, variables, groups)) in scopes.into_iter().enumerate() {
            let parameters = if set.local_parameters.is_empty() {
                &self.named_parameters
            } else {
                &set.local_parameters
            };
            if let Some(handle) = materialize_scope(
                &mut self.document,
                owner,
                &set,
                parameters,
                root_network_handle,
                index as i32 + 1,
                variables,
                groups,
                owner == model_owner,
            ) {
                child_networks.push(handle);
            }
        }
        if child_networks.is_empty() {
            return;
        }

        let root_network = AssociativeObject {
            handle: root_network_handle,
            owner: global_dictionary,
            reactors: vec![global_dictionary],
            dxf_name: "ASSOCNETWORK".to_string(),
            cpp_class_name: "AcDbAssocNetwork".to_string(),
            data: AssociativeData::Network(AssocNetwork {
                action: AssocAction {
                    class_version: 2,
                    ..Default::default()
                },
                network_version: 0,
                network_action_index: child_networks.len() as i32,
                actions: child_networks
                    .into_iter()
                    .map(|dependency| AssocActionDependency {
                        is_owned: false,
                        dependency,
                    })
                    .collect(),
                owned_actions: Vec::new(),
            }),
            ..Default::default()
        };
        self.document
            .objects
            .insert(root_network_handle, ObjectType::Associative(root_network));
        set_dictionary_entry(
            &mut self.document,
            global_dictionary,
            NETWORK_DICTIONARY_KEY,
            root_network_handle,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::parametric_constraints::{
        angle_sector, distance_direction_type, ParametricScope,
    };
    use super::*;
    use crate::scene::ChangeKind;
    use acadrust::entities::{Arc, Circle, Insert, Line, LwPolyline, Ray, Spline, XLine};
    use acadrust::tables::BlockRecord;
    use acadrust::types::Vector2;

    fn line_entity(scene: &mut Scene, start: (f64, f64), end: (f64, f64)) -> Handle {
        scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(start.0, start.1, 0.0),
            Vector3::new(end.0, end.1, 0.0),
        )))
    }

    fn circle_entity(scene: &mut Scene, center: (f64, f64), radius: f64) -> Handle {
        scene.add_entity(EntityType::Circle(Circle::from_center_radius(
            Vector3::new(center.0, center.1, 0.0),
            radius,
        )))
    }

    fn arc_entity(scene: &mut Scene, center: (f64, f64), radius: f64) -> Handle {
        scene.add_entity(EntityType::Arc(Arc::from_center_radius_angles(
            Vector3::new(center.0, center.1, 0.0),
            radius,
            0.0,
            std::f64::consts::PI,
        )))
    }

    fn native_group_handle(document: &CadDocument, owner: Handle) -> Handle {
        let dict = document
            .extension_dictionary_handle(owner)
            .expect("extension dictionary should exist");
        let Some(ObjectType::Dictionary(dictionary)) = document.objects.get(&dict) else {
            panic!("expected owner's extension dictionary object to exist");
        };
        let network_handle = dictionary
            .get(NETWORK_DICTIONARY_KEY)
            .expect("ACAD_ASSOCNETWORK entry should exist");
        let Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Network(network),
            ..
        })) = document.objects.get(&network_handle)
        else {
            panic!("expected an AssocNetwork object at the ACAD_ASSOCNETWORK handle");
        };
        network
            .actions
            .iter()
            .filter(|dependency| dependency.is_owned)
            .map(|dependency| dependency.dependency)
            .find(|handle| {
                matches!(
                    document.objects.get(handle),
                    Some(ObjectType::Associative(AssociativeObject {
                        data: AssociativeData::ConstraintGroup(_),
                        ..
                    }))
                )
            })
            .expect("network should own the constraint group")
    }

    fn native_group(document: &CadDocument, owner: Handle) -> &Assoc2dConstraintGroup {
        let group_handle = native_group_handle(document, owner);
        let Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::ConstraintGroup(group),
            ..
        })) = document.objects.get(&group_handle)
        else {
            panic!(
                "expected an Assoc2dConstraintGroup object at the network's owned action handle"
            );
        };
        group
    }

    #[test]
    fn horizontal_and_vertical_constraints_round_trip_through_dwg_and_dxf() {
        for ext in ["dwg", "dxf"] {
            let mut scene = Scene::new();
            let a = line_entity(&mut scene, (0.0, 0.0), (6.0, 8.0));
            let b = line_entity(&mut scene, (10.0, 0.0), (10.0, 5.0));
            let horizontal_direction = Vector3::new(0.6, 0.8, 0.0);
            scene
                .parametric_constraint_set_mut(ParametricScope::ModelSpace)
                .add_axis_constraint(
                    ConstraintKind::Horizontal,
                    vec![ParametricRef::whole(a)],
                    horizontal_direction,
                );
            scene
                .parametric_constraint_set_mut(ParametricScope::ModelSpace)
                .add(
                    ConstraintKind::Vertical,
                    vec![ParametricRef::whole(b)],
                    None,
                );

            scene.sync_native_parametric_graph();
            let owner = scene.document.header.model_space_block_handle;
            // Root + two geometry nodes + two datums + two constraints.
            let before = native_group(&scene.document, owner);
            assert_eq!(
                before.nodes.len(),
                7,
                "expected root + 2 geometry + 2 datums + 2 constraints, got {:#?}",
                before.nodes
            );

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded = crate::io::load_bytes(&format!("dwg_native_poc.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded, owner);
            let class_names: Vec<&str> =
                group.nodes.iter().map(|n| n.class_name.as_str()).collect();
            assert!(
                class_names.contains(&"AcHorizontalConstraint"),
                "{ext}: missing horizontal constraint node, got {class_names:?}"
            );
            assert!(
                class_names.contains(&"AcVerticalConstraint"),
                "{ext}: missing vertical constraint node, got {class_names:?}"
            );
            assert_eq!(
                class_names
                    .iter()
                    .filter(|&&n| n == "AcConstrainedBoundedLine")
                    .count(),
                2,
                "{ext}: expected both lines' geometry nodes to survive, got {class_names:?}"
            );
            let datums: Vec<i32> = group
                .nodes
                .iter()
                .filter_map(|node| match node.data {
                    AssocConstraintNodeData::Parallel {
                        datum_line_index: Some(datum),
                        ..
                    } if matches!(
                        node.class_name.as_str(),
                        "AcHorizontalConstraint" | "AcVerticalConstraint"
                    ) =>
                    {
                        Some(datum)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(datums.len(), 2, "{ext}: both constraints need a datum");
            assert!(datums.iter().all(|datum| group.nodes.iter().any(|node| {
                node.node_id == *datum && node.class_name == "AcConstrainedDatumLine"
            })));

            let mut restored = Scene::new();
            restored.document = reloaded;
            restored.load_parametric_constraints_from_document();
            let horizontal = restored
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .unwrap()
                .constraints
                .iter()
                .find(|constraint| constraint.kind == ConstraintKind::Horizontal)
                .unwrap();
            assert!(
                (horizontal.axis_direction.unwrap() - horizontal_direction).length() < 1.0e-12
            );
        }
    }

    #[test]
    fn ray_and_construction_line_geometry_round_trip_through_the_standard_graph() {
        for ext in ["dwg", "dxf"] {
            let mut scene = Scene::new();
            let ray = scene.add_entity(EntityType::Ray(Ray::new(
                Vector3::ZERO,
                Vector3::new(1.0, 1.0, 0.0),
            )));
            let xline = scene.add_entity(EntityType::XLine(XLine::new(
                Vector3::new(4.0, 0.0, 0.0),
                Vector3::UNIT_Y,
            )));
            let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
            set.add(
                ConstraintKind::Horizontal,
                vec![ParametricRef::whole(ray)],
                None,
            );
            set.add(
                ConstraintKind::Parallel,
                vec![ParametricRef::whole(ray), ParametricRef::whole(xline)],
                None,
            );
            scene.sync_native_parametric_graph();

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|error| panic!("save {ext}: {error}"));
            let mut restored = Scene::new();
            restored.document = crate::io::load_bytes(&format!("line_kinds.{ext}"), bytes)
                .unwrap_or_else(|error| panic!("reload {ext}: {error}"));

            let owner = restored.document.header.model_space_block_handle;
            let group = native_group(&restored.document, owner);
            assert!(group.nodes.iter().any(|node| {
                matches!(
                    node.data,
                    AssocConstraintNodeData::BoundedLine { is_ray: true, .. }
                )
            }));
            assert!(group.nodes.iter().any(|node| {
                node.class_name == "AcConstrainedLine"
                    && matches!(node.data, AssocConstraintNodeData::Line { .. })
            }));

            restored.load_parametric_constraints_from_document();
            let set = restored
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .expect("constraints should import");
            assert!(set
                .constraints
                .iter()
                .any(|constraint| constraint.kind == ConstraintKind::Horizontal));
            assert!(set
                .constraints
                .iter()
                .any(|constraint| constraint.kind == ConstraintKind::Parallel));
        }
    }

    #[test]
    fn implicit_midpoint_round_trips_as_standard_implied_relations() {
        for ext in ["dwg", "dxf"] {
            let mut scene = Scene::new();
            let carrier = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
            let attached = line_entity(&mut scene, (5.0, 0.0), (5.0, 4.0));
            scene
                .parametric_constraint_set_mut(ParametricScope::ModelSpace)
                .add(
                    ConstraintKind::Coincident,
                    vec![
                        ParametricRef::point(carrier, -2),
                        ParametricRef::point(attached, 0),
                    ],
                    None,
                );
            scene.sync_native_parametric_graph();

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|error| panic!("save {ext}: {error}"));
            let mut restored = Scene::new();
            restored.document = crate::io::load_bytes(&format!("midpoint.{ext}"), bytes)
                .unwrap_or_else(|error| panic!("reload {ext}: {error}"));
            let owner = restored.document.header.model_space_block_handle;
            let group = native_group(&restored.document, owner);

            assert!(group.nodes.iter().any(|node| matches!(
                node.data,
                AssocConstraintNodeData::ImplicitPoint {
                    point_type: implicit_point_type::MID,
                    ..
                }
            )));
            for class_name in ["AcPointCurveConstraint", "AcMidPointConstraint"] {
                assert!(group.nodes.iter().any(|node| {
                    node.class_name == class_name
                        && matches!(
                            node.data,
                            AssocConstraintNodeData::Geometrical {
                                is_implied: true,
                                is_active: true,
                                ..
                            }
                        )
                }));
            }

            restored.load_parametric_constraints_from_document();
            let set = restored
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .expect("coincidence should import");
            assert_eq!(set.constraints.len(), 1);
            assert_eq!(set.constraints[0].kind, ConstraintKind::Coincident);
            assert!(set.constraints[0]
                .refs
                .iter()
                .any(|reference| reference.entity == carrier && reference.marker == Some(-2)));
        }
    }

    #[test]
    fn spline_smoothness_round_trips_as_a_standard_composite_graph() {
        for ext in ["dwg", "dxf"] {
            let mut scene = Scene::new();
            let first = scene.add_entity(EntityType::Spline(Spline::from_control_points(
                3,
                vec![
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(2.0, 0.0, 0.0),
                    Vector3::new(4.0, 2.0, 0.0),
                    Vector3::new(6.0, 2.0, 0.0),
                ],
            )));
            let second = line_entity(&mut scene, (6.0, 2.0), (12.0, 2.0));
            scene
                .parametric_constraint_set_mut(ParametricScope::ModelSpace)
                .add(
                    ConstraintKind::Smooth,
                    vec![
                        ParametricRef::point(first, 1),
                        ParametricRef::point(second, 0),
                    ],
                    None,
                );
            scene.sync_native_parametric_graph();

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|error| panic!("save {ext}: {error}"));
            let mut restored = Scene::new();
            restored.document = crate::io::load_bytes(&format!("smooth.{ext}"), bytes)
                .unwrap_or_else(|error| panic!("reload {ext}: {error}"));
            let owner = restored.document.header.model_space_block_handle;
            let group = native_group(&restored.document, owner);

            let spline_nodes: Vec<_> = group
                .nodes
                .iter()
                .filter_map(|node| match &node.data {
                    AssocConstraintNodeData::Spline {
                        implicit_point_ids, ..
                    } => Some(implicit_point_ids),
                    _ => None,
                })
                .collect();
            assert_eq!(spline_nodes.len(), 1, "{ext}: spline graph node is missing");
            assert!(spline_nodes.iter().all(|points| points.len() == 4));
            assert_eq!(
                group
                    .nodes
                    .iter()
                    .filter(|node| matches!(
                        node.data,
                        AssocConstraintNodeData::HelpParameter { .. }
                    ))
                    .count(),
                2,
                "{ext}: the spline side needs one parameter for each child"
            );
            let composite = group
                .nodes
                .iter()
                .find(|node| node.class_name == "AcG2SmoothConstraint")
                .expect("smooth composite should exist");
            let AssocConstraintNodeData::Composite {
                owned_constraint_ids,
                ..
            } = &composite.data
            else {
                panic!("{ext}: smooth node should carry composite data");
            };
            assert_eq!(owned_constraint_ids.len(), 3);
            assert!(owned_constraint_ids.iter().all(|child_id| {
                group.nodes.iter().any(|child| {
                    child.node_id == *child_id
                        && owning_composite(&child.data) == Some(composite.node_id)
                })
            }));

            restored.load_parametric_constraints_from_document();
            let constraints = &restored
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .expect("smooth constraint should import")
                .constraints;
            assert_eq!(
                constraints.len(),
                1,
                "{ext}: composite children stay internal"
            );
            assert_eq!(constraints[0].kind, ConstraintKind::Smooth);
            assert_eq!(
                constraints[0].refs,
                vec![
                    ParametricRef::point(first, 1),
                    ParametricRef::point(second, 0)
                ]
            );
        }
    }

    #[test]
    fn native_only_constraints_and_parameters_are_imported() {
        let mut scene = Scene::new();
        let a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
        let b = line_entity(&mut scene, (0.0, 5.0), (10.0, 5.0));
        scene.named_parameters.set("gap", "5").unwrap();
        let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(a)],
            None,
        );
        set.add(
            ConstraintKind::Parallel,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );
        set.add(
            ConstraintKind::DistanceY,
            vec![ParametricRef::point(a, 0), ParametricRef::point(b, 0)],
            Some(DrivingValue::Named("gap".to_string())),
        );

        scene.sync_native_parametric_graph();
        let bytes = crate::io::save_to_bytes(&scene.document, "dwg", scene.document.version)
            .expect("save native-only graph");
        let mut restored = Scene::new();
        restored.document = crate::io::load_bytes("native_only.dwg", bytes).unwrap();
        restored.load_named_parameters_from_document();
        restored.load_parametric_constraints_from_document();

        let set = restored
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .expect("native group should become an editable constraint set");
        let kinds: Vec<_> = set
            .constraints
            .iter()
            .map(|constraint| constraint.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                ConstraintKind::Horizontal,
                ConstraintKind::Parallel,
                ConstraintKind::DistanceY,
            ]
        );
        assert_eq!(restored.named_parameters.resolve("gap"), Ok(5.0));
        assert_eq!(
            set.constraints[2].driving_param,
            Some(DrivingValue::Named("gap".to_string()))
        );
    }

    #[test]
    fn equal_parameter_names_remain_independent_across_block_scopes() {
        let mut scene = Scene::new();
        let mut block_handles = Vec::new();
        let mut line_handles = Vec::new();
        for (index, length) in [5.0, 9.0].into_iter().enumerate() {
            let mut block = BlockRecord::new(format!("fixture_{index}"));
            block.handle = scene.document.allocate_handle();
            let block_handle = block.handle;
            scene.document.block_records.add(block).unwrap();

            let mut line = Line::from_points(Vector3::ZERO, Vector3::new(length, 0.0, 0.0));
            line.common.owner_handle = block_handle;
            let line_handle = scene.document.add_entity(EntityType::Line(line)).unwrap();
            let set = scene.parametric_constraint_set_mut(ParametricScope::Block(block_handle));
            set.local_parameters
                .set("width", &length.to_string())
                .unwrap();
            set.add(
                ConstraintKind::DistanceX,
                vec![
                    ParametricRef::point(line_handle, 0),
                    ParametricRef::point(line_handle, 1),
                ],
                Some(DrivingValue::Named("width".to_string())),
            );
            block_handles.push(block_handle);
            line_handles.push(line_handle);
        }

        scene.sync_native_parametric_graph();
        let bytes = crate::io::save_to_bytes(&scene.document, "dwg", scene.document.version)
            .expect("save block parameter scopes");
        let mut restored = Scene::new();
        restored.document = crate::io::load_bytes("block_parameters.dwg", bytes).unwrap();
        restored.load_named_parameters_from_document();
        restored.load_parametric_constraints_from_document();

        assert!(restored.named_parameters.is_empty());
        for (index, block_handle) in block_handles.into_iter().enumerate() {
            let set = restored
                .parametric_constraint_set(ParametricScope::Block(block_handle))
                .expect("block constraint scope should survive");
            assert_eq!(set.local_parameters.resolve("width"), Ok([5.0, 9.0][index]));
            assert_eq!(
                set.constraints[0].driving_param,
                Some(DrivingValue::Named("width".to_string()))
            );
        }

        let changes: Vec<_> = line_handles
            .into_iter()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        restored.refresh_parametric_constraints(&changes);
    }

    #[test]
    fn standalone_named_parameters_round_trip_as_standard_variables() {
        for ext in ["dwg", "dxf"] {
            let mut scene = Scene::new();
            let xrecords_before = scene
                .document
                .objects
                .values()
                .filter(|object| matches!(object, ObjectType::XRecord(_)))
                .count();
            scene.named_parameters.set("width", "12").unwrap();
            scene.named_parameters.set("height", "width / 2").unwrap();
            scene.sync_native_parametric_graph();

            assert_eq!(
                scene
                    .document
                    .objects
                    .values()
                    .filter(|object| matches!(object, ObjectType::XRecord(_)))
                    .count(),
                xrecords_before,
                "standard parameters must not create application records"
            );
            assert!(has_variable_named(&scene.document, "width"));
            assert!(has_variable_named(&scene.document, "height"));

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let mut restored = Scene::new();
            restored.document = crate::io::load_bytes(&format!("parameters.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));
            restored.load_named_parameters_from_document();

            assert_eq!(restored.named_parameters.resolve("width"), Ok(12.0));
            assert_eq!(restored.named_parameters.resolve("height"), Ok(6.0));
        }
    }

    #[test]
    fn command_style_add_and_remove_sync_the_standard_graph_immediately() {
        let mut scene = Scene::new();
        let line = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
        let id = scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Horizontal,
                vec![ParametricRef::whole(line)],
                None,
            );
        scene.bump_entities(&[(line, ChangeKind::Modified)]);

        let owner = scene.document.header.model_space_block_handle;
        assert!(native_group(&scene.document, owner)
            .nodes
            .iter()
            .any(|node| node.class_name == "AcHorizontalConstraint"));

        assert!(scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .remove(id));
        scene.bump_entities(&[(line, ChangeKind::Modified)]);
        assert!(native_scope_network_handle(&scene.document, owner).is_none());
    }

    #[test]
    fn inactive_constraints_round_trip_without_becoming_active() {
        for ext in ["dwg", "dxf"] {
            let mut scene = Scene::new();
            let line = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
            let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
            set.add(
                ConstraintKind::Horizontal,
                vec![ParametricRef::whole(line)],
                None,
            );
            set.constraints[0].enabled = false;
            scene.bump_entities(&[(line, ChangeKind::Modified)]);

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let mut restored = Scene::new();
            restored.document = crate::io::load_bytes(&format!("inactive.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));
            restored.load_parametric_constraints_from_document();

            let constraint = &restored
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .expect("constraint set should survive")
                .constraints[0];
            assert_eq!(constraint.kind, ConstraintKind::Horizontal);
            assert!(!constraint.enabled);
        }
    }

    #[test]
    fn point_and_two_point_horizontal_constraints_round_trip_natively() {
        let mut scene = Scene::new();
        let mut block = BlockRecord::new("fixture");
        block.handle = scene.document.allocate_handle();
        scene.document.block_records.add(block).unwrap();
        let point = scene.add_entity(EntityType::Point(acadrust::entities::Point::at(
            Vector3::new(1.0, 2.0, 0.0),
        )));
        let insert = scene.add_entity(EntityType::Insert(Insert::new(
            "fixture",
            Vector3::new(3.0, 4.0, 0.0),
        )));
        let line = line_entity(&mut scene, (5.0, 7.0), (10.0, 9.0));
        let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Horizontal,
            vec![
                ParametricRef::point(point, 0),
                ParametricRef::point(insert, 0),
            ],
            None,
        );
        set.add(
            ConstraintKind::Midpoint,
            vec![ParametricRef::point(point, 0), ParametricRef::whole(line)],
            None,
        );

        scene.sync_native_parametric_graph();
        let bytes = crate::io::save_to_bytes(&scene.document, "dwg", scene.document.version)
            .expect("save point graph");
        let mut restored = Scene::new();
        restored.document = crate::io::load_bytes("point_constraints.dwg", bytes).unwrap();
        restored.load_parametric_constraints_from_document();

        let constraints = &restored
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .expect("native point constraints should be imported")
            .constraints;
        assert_eq!(constraints.len(), 2);
        assert_eq!(constraints[0].kind, ConstraintKind::Horizontal);
        assert_eq!(constraints[0].refs.len(), 2);
        assert_eq!(constraints[1].kind, ConstraintKind::Midpoint);
    }

    #[test]
    fn polyline_segment_constraints_round_trip_through_the_native_graph() {
        let mut scene = Scene::new();
        let polyline = scene.add_entity(EntityType::LwPolyline(LwPolyline::from_points(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(5.0, 2.0),
            Vector2::new(10.0, 7.0),
        ])));
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Horizontal,
                vec![ParametricRef::segment(polyline, 1)],
                None,
            );

        scene.sync_native_parametric_graph();
        let bytes = crate::io::save_to_bytes(&scene.document, "dwg", scene.document.version)
            .expect("save polyline segment graph");
        let mut restored = Scene::new();
        restored.document = crate::io::load_bytes("polyline_segment.dwg", bytes).unwrap();
        restored.load_parametric_constraints_from_document();

        let set = restored
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .expect("native segment constraint should be imported");
        assert_eq!(set.constraints.len(), 1);
        assert_eq!(set.constraints[0].kind, ConstraintKind::Horizontal);
        assert_eq!(
            set.constraints[0].refs,
            vec![ParametricRef::segment(polyline, 1)]
        );
    }

    #[test]
    fn bulged_polyline_point_on_curve_round_trips_with_its_segment() {
        for ext in ["dwg", "dxf"] {
            let mut source = LwPolyline::new();
            source.add_point(Vector2::new(0.0, 0.0));
            source.add_point_with_bulge(Vector2::new(5.0, 0.0), 0.5);
            source.add_point(Vector2::new(10.0, 5.0));
            let mut scene = Scene::new();
            let polyline = scene.add_entity(EntityType::LwPolyline(source));
            let point = line_entity(&mut scene, (8.0, 2.0), (9.0, 2.0));
            scene
                .parametric_constraint_set_mut(ParametricScope::ModelSpace)
                .add(
                    ConstraintKind::PointOnCurve,
                    vec![
                        ParametricRef::point(point, 0),
                        ParametricRef::segment(polyline, 1),
                    ],
                    None,
                );

            scene.sync_native_parametric_graph();
            let group = native_group(
                &scene.document,
                scene.document.header.model_space_block_handle,
            );
            assert!(group
                .nodes
                .iter()
                .any(|node| node.class_name == "AcConstrainedArc"));

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|error| panic!("save {ext}: {error}"));
            let mut restored = Scene::new();
            restored.document = crate::io::load_bytes(&format!("bulged_segment.{ext}"), bytes)
                .unwrap_or_else(|error| panic!("reload {ext}: {error}"));
            restored.load_parametric_constraints_from_document();

            let set = restored
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .expect("native point-on-curve constraint should be imported");
            assert_eq!(set.constraints.len(), 1, "{ext}");
            assert_eq!(set.constraints[0].kind, ConstraintKind::PointOnCurve);
            assert_eq!(
                set.constraints[0].refs,
                vec![
                    ParametricRef::point(point, 0),
                    ParametricRef::segment(polyline, 1),
                ],
                "{ext}"
            );
        }
    }

    fn distance_with_named_parameter_scene() -> (Scene, Handle) {
        let mut scene = Scene::new();
        let a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
        let b = line_entity(&mut scene, (20.0, 0.0), (30.0, 0.0));
        scene
            .named_parameters
            .set("gap", "5")
            .expect("defining the parameter should succeed");
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Distance,
                vec![ParametricRef::point(a, 1), ParametricRef::point(b, 0)],
                Some(DrivingValue::Named("gap".to_string())),
            );
        let owner = scene.document.header.model_space_block_handle;
        (scene, owner)
    }

    fn has_variable_named(document: &CadDocument, name: &str) -> bool {
        document.objects.values().any(|obj| {
            matches!(
                obj,
                ObjectType::Associative(AssociativeObject { data: AssociativeData::Variable(v), .. })
                    if v.name == name
            )
        })
    }

    #[test]
    fn a_dwg_save_keeps_the_full_dependency_chain_including_the_named_variable() {
        let (mut scene, owner) = distance_with_named_parameter_scene();
        scene.sync_native_parametric_graph();
        assert!(
            has_variable_named(&scene.document, "gap"),
            "expected an AssocVariable BEFORE serialization"
        );

        let bytes = crate::io::save_to_bytes(&scene.document, "dwg", scene.document.version)
            .unwrap_or_else(|e| panic!("save to dwg: {e}"));
        let reloaded = crate::io::load_bytes("dwg_native_named_param.dwg", bytes)
            .unwrap_or_else(|e| panic!("reload dwg: {e}"));

        let group_handle = native_group_handle(&reloaded, owner);
        let group = native_group(&reloaded, owner);
        assert!(
            group
                .nodes
                .iter()
                .any(|n| n.class_name == "AcDistanceConstraint"),
            "expected a distance constraint node, got {:?}",
            group
                .nodes
                .iter()
                .map(|n| &n.class_name)
                .collect::<Vec<_>>()
        );
        assert!(
            has_variable_named(&reloaded, "gap"),
            "expected the AssocVariable \"gap\" to survive a DWG round trip"
        );

        assert_eq!(group.action.class_version, 2);
        assert_eq!(group.action.action_index, 2);
        assert_eq!(group.action.max_dependency_index, 4);
        assert_eq!(group.actions.len(), 3);

        let value_dependency = group
            .nodes
            .iter()
            .find_map(|node| match &node.data {
                AssocConstraintNodeData::Distance {
                    value_dependency, ..
                } => Some(*value_dependency),
                _ => None,
            })
            .expect("distance node should reference its value dependency");
        let Some(ObjectType::Associative(AssociativeObject {
            owner: value_owner,
            data: AssociativeData::ValueDependency(value),
            ..
        })) = reloaded.objects.get(&value_dependency)
        else {
            panic!("value dependency should exist");
        };
        assert_eq!(*value_owner, group_handle);
        assert_eq!(value.dependency.dependency_body_id, 3);
        assert!(value.dependency.is_read_dependency);
        assert!(!value.dependency.is_write_dependency);

        let variable_handle = value.dependency.dependent_on;
        let Some(ObjectType::Associative(AssociativeObject {
            owner: network_handle,
            reactors,
            data: AssociativeData::Variable(variable),
            ..
        })) = reloaded.objects.get(&variable_handle)
        else {
            panic!("named variable should exist");
        };
        assert_eq!(group.action.owning_network, *network_handle);
        assert_eq!(variable.action.owning_network, *network_handle);
        assert_eq!(variable.action.action_index, 1);
        assert!(reactors.contains(&value_dependency));

        let Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Network(network),
            ..
        })) = reloaded.objects.get(network_handle)
        else {
            panic!("scope network should exist");
        };
        assert_eq!(network.network_action_index, 2);
        assert_eq!(network.actions.len(), 2);
        assert_eq!(network.actions[0].dependency, variable_handle);
        assert_eq!(network.actions[1].dependency, group_handle);
        assert!(network.actions.iter().all(|action| action.is_owned));
    }

    #[test]
    fn a_dxf_save_keeps_the_full_constraint_dependency_chain() {
        let (mut scene, owner) = distance_with_named_parameter_scene();
        scene.sync_native_parametric_graph();

        let bytes = crate::io::save_to_bytes(&scene.document, "dxf", scene.document.version)
            .unwrap_or_else(|e| panic!("save to dxf: {e}"));
        let reloaded = crate::io::load_bytes("dwg_native_named_param.dxf", bytes)
            .unwrap_or_else(|e| panic!("reload dxf: {e}"));

        let group = native_group(&reloaded, owner);
        assert!(
            group
                .nodes
                .iter()
                .any(|n| n.class_name == "AcDistanceConstraint"),
            "expected the distance constraint node to survive, got {:?}",
            group
                .nodes
                .iter()
                .map(|n| &n.class_name)
                .collect::<Vec<_>>()
        );
        assert!(
            has_variable_named(&reloaded, "gap"),
            "expected the named variable to survive a DXF round trip"
        );
    }

    #[test]
    fn resaving_replaces_rather_than_accumulates_native_objects() {
        let mut scene = Scene::new();
        let a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Horizontal,
                vec![ParametricRef::whole(a)],
                None,
            );

        scene.sync_native_parametric_graph();
        let first_count = scene.document.objects.len();
        scene.sync_native_parametric_graph();
        let second_count = scene.document.objects.len();
        assert_eq!(
            first_count, second_count,
            "a resave with the same constraints must not accumulate new objects"
        );
    }

    #[test]
    fn a_standard_rigid_set_and_its_constraints_remain_editable() {
        let mut scene = Scene::new();
        let a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Horizontal,
                vec![ParametricRef::whole(a)],
                None,
            );
        scene.sync_native_parametric_graph();

        let owner = scene.document.header.model_space_block_handle;
        let group_handle = native_group_handle(&scene.document, owner);
        let Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::ConstraintGroup(group),
            ..
        })) = scene.document.objects.get_mut(&group_handle)
        else {
            panic!("expected a native constraint group");
        };
        let geometry_id = group
            .nodes
            .iter()
            .find(|node| node.class_name == "AcConstrainedBoundedLine")
            .map(|node| node.node_id)
            .expect("line geometry should exist");
        for node in &mut group.nodes {
            if node.node_id == geometry_id {
                if let AssocConstraintNodeData::BoundedLine {
                    geometry_node_id, ..
                } = &mut node.data
                {
                    *geometry_node_id = 99;
                }
            }
        }
        group.nodes.push(AssocConstraintNode {
            node_id: 99,
            class_name: "AcConstrainedRigidSet".to_string(),
            data: AssocConstraintNodeData::RigidSet {
                geometry_dependency: Handle::NULL,
                geometry_node_id: 0,
                reserved: false,
                transform: [
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                ],
                geometry_ids: vec![geometry_id],
            },
            ..Default::default()
        });

        scene.load_parametric_constraints_from_document();
        let imported = scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .expect("standard group should be imported");
        assert!(imported
            .constraints
            .iter()
            .any(|constraint| constraint.kind == ConstraintKind::Horizontal));
        assert!(imported
            .constraints
            .iter()
            .any(|constraint| constraint.kind == ConstraintKind::RigidSet));

        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Vertical,
                vec![ParametricRef::whole(a)],
                None,
            );
        scene.sync_native_parametric_graph();

        let groups: Vec<_> = native_group_handles(&scene.document, owner)
            .into_iter()
            .filter_map(|handle| match scene.document.objects.get(&handle) {
                Some(ObjectType::Associative(AssociativeObject {
                    data: AssociativeData::ConstraintGroup(group),
                    ..
                })) => Some(group),
                _ => None,
            })
            .collect();
        assert!(groups.iter().any(|group| group
            .nodes
            .iter()
            .any(|node| matches!(node.data, AssocConstraintNodeData::RigidSet { .. }))));
        assert!(groups.iter().any(|group| group
            .nodes
            .iter()
            .any(|node| node.class_name == "AcVerticalConstraint")));

        let horizontal = scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .constraints
            .iter_mut()
            .find(|constraint| constraint.kind == ConstraintKind::Horizontal)
            .expect("imported horizontal constraint");
        horizontal.enabled = false;
        scene.sync_native_parametric_graph();
        let Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::ConstraintGroup(group),
            ..
        })) = scene.document.objects.get(&group_handle)
        else {
            panic!("retained group should exist");
        };
        assert!(group.nodes.iter().any(|node| {
            node.class_name == "AcHorizontalConstraint" && !constraint_is_active(&node.data)
        }));
    }

    #[test]
    fn an_empty_constraint_set_leaves_no_native_network() {
        let mut scene = Scene::new();
        let _ = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        scene.sync_native_parametric_graph();

        let owner = scene.document.header.model_space_block_handle;
        let dict = scene.document.extension_dictionary_handle(owner);
        if let Some(dict) = dict {
            if let Some(ObjectType::Dictionary(dictionary)) = scene.document.objects.get(&dict) {
                assert!(
                    dictionary.get(NETWORK_DICTIONARY_KEY).is_none(),
                    "an unconstrained scope should not materialize a native network"
                );
            }
        }
    }

    fn model_space_has_native_network(scene: &Scene) -> bool {
        native_scope_network_handle(
            &scene.document,
            scene.document.header.model_space_block_handle,
        )
        .is_some()
    }

    #[test]
    fn sync_creates_the_standard_graph_for_a_new_constraint() {
        let mut scene = Scene::new();
        let a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Horizontal,
                vec![ParametricRef::whole(a)],
                None,
            );

        scene.sync_native_parametric_graph();
        assert!(
            model_space_has_native_network(&scene),
            "a new constraint should immediately create its standard graph"
        );
    }

    /// These `ConstraintKind`s all reuse the same generic
    /// `Geometrical`-shaped node path `geometrical_class_name` drives (see
    /// its own doc comment) — one representative mix of ref shapes (2 whole
    /// entities, an asymmetric point+whole pair, and a 3-ref case) is
    /// enough to confirm that path handles them all, without one dedicated
    /// test per kind.
    #[test]
    fn new_constraint_kinds_round_trip_with_their_own_dwg_native_class_names() {
        for ext in ["dxf", "dwg"] {
            let mut scene = Scene::new();
            let circle_a = circle_entity(&mut scene, (0.0, 0.0), 3.0);
            let circle_b = circle_entity(&mut scene, (5.0, 5.0), 1.0);
            let line_a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
            let line_b = line_entity(&mut scene, (3.0, 4.0), (7.0, 6.0));
            let marker = line_entity(&mut scene, (20.0, 20.0), (21.0, 21.0));
            let axis = line_entity(&mut scene, (0.0, 0.0), (0.0, 10.0));

            let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
            set.add(
                ConstraintKind::Concentric,
                vec![
                    ParametricRef::center(circle_a),
                    ParametricRef::center(circle_b),
                ],
                None,
            );
            set.add(
                ConstraintKind::Colinear,
                vec![ParametricRef::whole(line_a), ParametricRef::whole(line_b)],
                None,
            );
            set.add(
                ConstraintKind::Fixed,
                vec![ParametricRef::whole(line_a)],
                None,
            );
            set.add(
                ConstraintKind::CenterPoint,
                vec![
                    ParametricRef::point(marker, 0),
                    ParametricRef::center(circle_a),
                ],
                None,
            );
            set.add(
                ConstraintKind::Midpoint,
                vec![
                    ParametricRef::point(marker, 1),
                    ParametricRef::whole(line_a),
                ],
                None,
            );
            set.add(
                ConstraintKind::PointOnCurve,
                vec![
                    ParametricRef::point(marker, 0),
                    ParametricRef::whole(circle_a),
                ],
                None,
            );
            set.add(
                ConstraintKind::EqualDistance,
                vec![
                    ParametricRef::point(line_b, 0),
                    ParametricRef::point(line_b, 1),
                    ParametricRef::point(line_a, 0),
                    ParametricRef::point(line_a, 1),
                ],
                None,
            );
            set.add(
                ConstraintKind::Symmetric,
                vec![
                    ParametricRef::center(circle_a),
                    ParametricRef::center(circle_b),
                    ParametricRef::whole(axis),
                ],
                None,
            );
            set.add(
                ConstraintKind::Normal,
                vec![ParametricRef::whole(circle_a), ParametricRef::whole(line_a)],
                None,
            );

            scene.sync_native_parametric_graph();
            let owner = scene.document.header.model_space_block_handle;

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded = crate::io::load_bytes(&format!("new_kinds.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded, owner);
            let class_names: Vec<&str> =
                group.nodes.iter().map(|n| n.class_name.as_str()).collect();

            for expected in [
                "AcConcentricConstraint",
                "AcColinearConstraint",
                "AcFixedConstraint",
                "AcCenterPointConstraint",
                "AcMidPointConstraint",
                "AcPointCurveConstraint",
                "AcEqualDistanceConstraint",
                "AcSymmetricConstraint",
                "AcNormalConstraint",
            ] {
                assert!(
                    class_names.contains(&expected),
                    "{ext}: missing {expected} node, got {class_names:?}"
                );
            }
        }
    }

    /// Constraint-parity Phase 1: an Arc registers in the solver as its own
    /// center/radius (`parametric_solve.rs`'s `EntityGeom::Circle` reuse), and
    /// this module already had constrained-arc geometry-dependency
    /// support in place before that. This confirms the two sides actually
    /// meet: a Concentric/Tangent/Radius constraint referencing an arc
    /// still gets its expected class names AND its geometry node is the
    /// arc-specific one, through a real DWG/DXF byte round trip — not just
    /// the in-memory graph this test module builds directly.
    #[test]
    fn constraints_referencing_an_arc_round_trip_with_the_arc_geometry_node() {
        for ext in ["dxf", "dwg"] {
            let mut scene = Scene::new();
            let arc = arc_entity(&mut scene, (0.0, 0.0), 4.0);
            let circle = circle_entity(&mut scene, (10.0, -6.0), 2.0);

            let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
            set.add(
                ConstraintKind::Concentric,
                vec![ParametricRef::center(arc), ParametricRef::center(circle)],
                None,
            );
            set.add(
                ConstraintKind::Radius,
                vec![ParametricRef::whole(arc)],
                Some(crate::scene::named_parameters::DrivingValue::Literal(9.0)),
            );

            scene.sync_native_parametric_graph();
            let owner = scene.document.header.model_space_block_handle;

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded = crate::io::load_bytes(&format!("arc_ref.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded, owner);
            let class_names: Vec<&str> =
                group.nodes.iter().map(|n| n.class_name.as_str()).collect();
            assert!(
                class_names.contains(&"AcConcentricConstraint"),
                "{ext}: missing concentric constraint, got {class_names:?}"
            );
            assert!(
                class_names.contains(&"AcRadiusDiameterConstraint"),
                "{ext}: missing radius/diameter constraint, got {class_names:?}"
            );
            assert!(
                group
                    .nodes
                    .iter()
                    .any(|n| n.class_name == "AcConstrainedArc"),
                "{ext}: the arc should have its own geometry node, got {class_names:?}"
            );
        }
    }

    /// Constraint-parity Phase 4b: a Coincident constraint referencing an
    /// arc's actual *endpoint* (marker 0, not its center via `-3` or its
    /// whole curve) round-trips correctly too — `ParametricRef`'s marker is
    /// serialized generically regardless of what entity type or point it
    /// addresses, so this needs no dedicated DWG-side wiring beyond what
    /// already existed; this test is here to actually confirm that rather
    /// than assume it.
    #[test]
    fn a_coincident_constraint_on_an_arcs_endpoint_round_trips() {
        for ext in ["dxf", "dwg"] {
            let mut scene = Scene::new();
            let arc = arc_entity(&mut scene, (0.0, 0.0), 4.0);
            let line = line_entity(&mut scene, (20.0, 20.0), (21.0, 20.0));

            scene
                .parametric_constraint_set_mut(ParametricScope::ModelSpace)
                .add(
                    ConstraintKind::Coincident,
                    vec![ParametricRef::point(arc, 0), ParametricRef::point(line, 0)],
                    None,
                );

            scene.sync_native_parametric_graph();
            let owner = scene.document.header.model_space_block_handle;

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded = crate::io::load_bytes(&format!("arc_endpoint_ref.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded, owner);
            let class_names: Vec<&str> =
                group.nodes.iter().map(|n| n.class_name.as_str()).collect();
            assert!(
                class_names.contains(&"AcPointCoincidenceConstraint"),
                "{ext}: missing point-coincidence constraint, got {class_names:?}"
            );
            assert!(
                group
                    .nodes
                    .iter()
                    .any(|n| n.class_name == "AcConstrainedArc"),
                "{ext}: the arc should still have its own geometry node, got {class_names:?}"
            );
        }
    }

    #[test]
    fn an_ellipse_constraint_round_trips_in_the_native_graph() {
        for ext in ["dxf", "dwg"] {
            let mut scene = Scene::new();
            let ellipse = scene.add_entity(EntityType::Ellipse(
                acadrust::entities::Ellipse::from_center_axes(
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(4.0, 0.0, 0.0),
                    0.5,
                ),
            ));
            let circle = circle_entity(&mut scene, (10.0, 10.0), 2.0);

            let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
            set.add(
                ConstraintKind::Concentric,
                vec![
                    ParametricRef::center(ellipse),
                    ParametricRef::center(circle),
                ],
                None,
            );
            set.add(
                ConstraintKind::Fixed,
                vec![ParametricRef::whole(circle)],
                None,
            );

            scene.sync_native_parametric_graph();
            let owner = scene.document.header.model_space_block_handle;

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded_doc = crate::io::load_bytes(&format!("ellipse_ref.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded_doc, owner);
            let class_names: Vec<&str> =
                group.nodes.iter().map(|n| n.class_name.as_str()).collect();
            assert!(
                class_names.contains(&"AcFixedConstraint"),
                "{ext}: the other constraint should still persist, got {class_names:?}"
            );
            assert!(
                class_names.contains(&"AcConcentricConstraint"),
                "{ext}: missing ellipse-referencing constraint, got {class_names:?}"
            );
            assert!(
                class_names.contains(&"AcConstrainedEllipse"),
                "{ext}: missing ellipse geometry node, got {class_names:?}"
            );

            let mut reloaded_scene = Scene::new();
            reloaded_scene.document = reloaded_doc;
            reloaded_scene.load_parametric_constraints_from_document();
            let restored = reloaded_scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .unwrap_or_else(|| {
                    panic!("{ext}: no ModelSpace constraint set survived the round trip")
                });
            let kinds: Vec<ConstraintKind> = restored.constraints.iter().map(|c| c.kind).collect();
            assert!(
                kinds.contains(&ConstraintKind::Concentric),
                "{ext}: the ellipse constraint should round-trip through the associative graph, got {kinds:?}"
            );
        }
    }

    /// Constraint-parity Phase 2: `Diameter`/`DistanceX`/`DistanceY` reuse
    /// `Radius`'/`Distance`'s own DWG class with a different
    /// `RadiusDiameterConstrType`/`DirectionType` mode byte, matching real
    /// the native format's representation (this module's own research, folded into
    /// `dimensional_class_name`'s doc comment). Confirms that byte actually
    /// survives a real DWG/DXF write+read, not just the in-memory node this
    /// module built.
    #[test]
    fn diameter_and_directed_distance_constraints_round_trip_with_their_mode_byte() {
        for ext in ["dxf", "dwg"] {
            let mut scene = Scene::new();
            let circle = circle_entity(&mut scene, (0.0, 0.0), 3.0);
            let line = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));

            let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
            set.add(
                ConstraintKind::Diameter,
                vec![ParametricRef::whole(circle)],
                Some(DrivingValue::Literal(16.0)),
            );
            set.add(
                ConstraintKind::DistanceX,
                vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
                Some(DrivingValue::Literal(10.0)),
            );
            set.add(
                ConstraintKind::DistanceY,
                vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
                Some(DrivingValue::Literal(3.0)),
            );

            scene.sync_native_parametric_graph();
            let owner = scene.document.header.model_space_block_handle;

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded = crate::io::load_bytes(&format!("diameter_directed.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded, owner);
            let radius_diameter_nodes: Vec<_> = group
                .nodes
                .iter()
                .filter_map(|n| match &n.data {
                    AssocConstraintNodeData::RadiusDiameter { mode, .. } => Some(*mode),
                    _ => None,
                })
                .collect();
            assert_eq!(radius_diameter_nodes, vec![1], "{ext}: Diameter should round-trip as mode=1 (kCircleDiameter), got {radius_diameter_nodes:?}");

            let distance_nodes: Vec<_> = group
                .nodes
                .iter()
                .filter_map(|n| match &n.data {
                    AssocConstraintNodeData::Distance {
                        direction_type,
                        distance,
                        ..
                    } => Some((*direction_type, *distance)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                distance_nodes.len(),
                2,
                "{ext}: expected DistanceX and DistanceY nodes, got {distance_nodes:?}"
            );
            assert!(
                distance_nodes.contains(&(1, Some(Vector3::new(1.0, 0.0, 0.0)))),
                "{ext}: DistanceX should round-trip as direction_type=1 with a (1,0,0) direction, got {distance_nodes:?}"
            );
            assert!(
                distance_nodes.contains(&(1, Some(Vector3::new(0.0, 1.0, 0.0)))),
                "{ext}: DistanceY should round-trip as direction_type=1 with a (0,1,0) direction, got {distance_nodes:?}"
            );
        }
    }

    #[test]
    fn line_relative_distance_and_angle_sectors_round_trip() {
        for ext in ["dxf", "dwg"] {
            let mut scene = Scene::new();
            let measured = line_entity(&mut scene, (0.0, 0.0), (3.0, 4.0));
            let direction = line_entity(&mut scene, (-2.0, -2.0), (2.0, 2.0));

            let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
            let distance_id = set.add(
                ConstraintKind::DistanceDirected,
                vec![
                    ParametricRef::point(measured, 0),
                    ParametricRef::point(measured, 1),
                    ParametricRef::whole(direction),
                ],
                Some(DrivingValue::Literal(7.0)),
            );
            let distance = set
                .constraints
                .iter_mut()
                .find(|constraint| constraint.id == distance_id)
                .unwrap();
            distance.distance_direction_type = distance_direction_type::PARALLEL_TO_LINE;
            distance.distance_direction = Some(Vector3::new(
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
                0.0,
            ));

            let angle_id = set.add(
                ConstraintKind::Angle,
                vec![
                    ParametricRef::whole(measured),
                    ParametricRef::whole(direction),
                ],
                Some(DrivingValue::Literal(30.0)),
            );
            set.constraints
                .iter_mut()
                .find(|constraint| constraint.id == angle_id)
                .unwrap()
                .angle_sector = angle_sector::ANTIPARALLEL_COUNTERCLOCKWISE;

            let angle3_id = set.add(
                ConstraintKind::Angle3Point,
                vec![
                    ParametricRef::point(measured, 0),
                    ParametricRef::point(measured, 1),
                    ParametricRef::point(direction, 1),
                ],
                Some(DrivingValue::Literal(45.0)),
            );
            set.constraints
                .iter_mut()
                .find(|constraint| constraint.id == angle3_id)
                .unwrap()
                .angle_sector = angle_sector::ANTIPARALLEL_CLOCKWISE;

            scene.sync_native_parametric_graph();
            let owner = scene.document.header.model_space_block_handle;
            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|error| panic!("save to {ext}: {error}"));
            let document = crate::io::load_bytes(&format!("directed.{ext}"), bytes)
                .unwrap_or_else(|error| panic!("reload {ext}: {error}"));

            let group = native_group(&document, owner);
            assert!(group.nodes.iter().any(|node| matches!(
                node.data,
                AssocConstraintNodeData::Distance {
                    direction_type: distance_direction_type::PARALLEL_TO_LINE,
                    ..
                }
            )));
            let mut sectors: Vec<_> = group
                .nodes
                .iter()
                .filter_map(|node| match node.data {
                    AssocConstraintNodeData::Angle { sector_type, .. } => Some(sector_type),
                    _ => None,
                })
                .collect();
            sectors.sort_unstable();
            assert_eq!(
                sectors,
                vec![
                    angle_sector::ANTIPARALLEL_CLOCKWISE,
                    angle_sector::ANTIPARALLEL_COUNTERCLOCKWISE,
                ]
            );

            let mut restored = Scene::new();
            restored.document = document;
            restored.load_named_parameters_from_document();
            restored.load_parametric_constraints_from_document();
            let restored = restored
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .unwrap();
            let distance = restored
                .constraints
                .iter()
                .find(|constraint| constraint.kind == ConstraintKind::DistanceDirected)
                .unwrap();
            assert_eq!(
                distance.distance_direction_type,
                distance_direction_type::PARALLEL_TO_LINE
            );
            assert_eq!(distance.refs.len(), 3);
            let mut restored_sectors: Vec<_> = restored
                .constraints
                .iter()
                .filter(|constraint| {
                    matches!(
                        constraint.kind,
                        ConstraintKind::Angle | ConstraintKind::Angle3Point
                    )
                })
                .map(|constraint| constraint.angle_sector)
                .collect();
            restored_sectors.sort_unstable();
            assert_eq!(restored_sectors, sectors);
        }
    }

}
