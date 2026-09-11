//! Maps sketch constraints to the drawing format's native associative object graph.
//! The application record remains authoritative when both representations exist.

use acadrust::objects::{
    Assoc2dConstraintGroup, AssocAction, AssocConstraintNode, AssocConstraintNodeData,
    AssocDependency, AssocEvalValue, AssocEvalVariant, AssocGeomDependency, AssocNetwork,
    AssocPersistentSubentId, AssocValueDependency, AssocVariable, AssociativeData,
    AssociativeObject, ObjectType,
};
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, EntityType};
use rustc_hash::FxHashMap;

use super::named_parameters::{DrivingValue, ParameterTable};
use super::sketch_constraints::{ConstraintKind, SketchConstraint, SketchConstraintSet, SketchRef};
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
    #[allow(dead_code)] // reserved for SketchRef marker -2, not wired up yet
    pub const MID: u8 = 2;
    pub const CENTER: u8 = 3;
}

/// One entity's constraint-group presence: its geometry node's id, plus
/// whichever `ImplicitPoint` nodes have been created for it so far, keyed
/// by our own `SketchRef::marker` convention so a second constraint
/// referencing the same point reuses the same node instead of duplicating it.
#[derive(Default)]
struct EntityNodes {
    geometry_node_id: i32,
    points: FxHashMap<i32, i32>,
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
        self.nodes.push(AssocConstraintNode {
            node_id,
            status: 1,
            connections: Vec::new(),
            class_name: class_name.to_string(),
            registry_flag: true,
            data,
        });
    }

    /// The geometry node id for `entity`'s whole shape, creating a `Line`
    /// (well: `BoundedLine`, §4 — nothing we draw is an infinite
    /// construction line), `Circle`, or `Arc` node the first time this
    /// entity is referenced. Returns `None` for an entity kind this pass
    /// doesn't support as constraint geometry — currently including
    /// `Ellipse` (constraint-parity Phase 5): the solver
    /// (`sketch_solve.rs`) and this app's own XRecord format both support
    /// ellipse-referencing constraints already, but `acadrust`'s
    /// `AssocConstraintNodeData::Ellipse`/`BoundedEllipse` variants have a
    /// different field shape from `Circle`/`Arc`'s here (`owner_id`/
    /// `is_implied`/`is_active` instead of `geometry_dependency`/
    /// `geometry_node_id`, i.e. they don't obviously look like the same
    /// "geometry dependency for an entity reference" role) — without
    /// confirming that against a real externally written file, guessing would
    /// risk writing a wrong-but-plausible-looking object graph, which is
    /// worse than the honest "not persisted to DWG yet" gap this already
    /// degrades to (same contract as `ArcLength`, which has no native
    /// representation at all).
    fn geometry_node(&mut self, handle: Handle) -> Option<i32> {
        if let Some(existing) = self.entities.get(&handle) {
            if existing.geometry_node_id != 0 {
                return Some(existing.geometry_node_id);
            }
        }
        let entity = self.document.get_entity(handle)?;
        let node_id = self.alloc_node_id();
        match entity {
            EntityType::Line(l) => {
                let point = l.start;
                let direction = (l.end - l.start).normalize();
                self.push_node(
                    node_id,
                    "AcDbAssocGeomDependency", // placeholder, overwritten below
                    AssocConstraintNodeData::None,
                );
                let last = self.nodes.last_mut().expect("just pushed");
                last.class_name = "ACCONSTRAINEDBOUNDEDLINE".to_string();
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
            EntityType::Circle(c) => {
                let (axis_x, _axis_y) =
                    crate::scene::view::transform::ocs_axes((c.normal.x, c.normal.y, c.normal.z));
                self.push_node(
                    node_id,
                    "ACCONSTRAINEDCIRCLE",
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
                    "ACCONSTRAINEDARC",
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
            _ => return None,
        }
        self.entities.entry(handle).or_default().geometry_node_id = node_id;
        if !self.referenced_entities.contains(&handle) {
            self.referenced_entities.push(handle);
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
        let curve_id = self.geometry_node(handle)?;
        let point_type = match marker {
            0 => implicit_point_type::START,
            1 => implicit_point_type::END,
            -3 => implicit_point_type::CENTER,
            _ => return None,
        };
        let node_id = self.alloc_node_id();
        self.push_node(
            node_id,
            "ACCONSTRAINEDIMPLICITPOINT",
            AssocConstraintNodeData::ImplicitPoint {
                geometry_dependency: Handle::NULL,
                geometry_node_id: node_id,
                point: None,
                point_type,
                point_index: -1,
                curve_id,
            },
        );
        self.entities
            .entry(handle)
            .or_default()
            .points
            .insert(marker, node_id);
        Some(node_id)
    }

    /// The node id `SketchRef` resolves to: a point node for a marked
    /// reference, the whole geometry node otherwise.
    fn ref_node(&mut self, r: SketchRef) -> Option<i32> {
        match r.marker {
            Some(marker) => self.point_node(r.entity, marker),
            None => self.geometry_node(r.entity),
        }
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
        ConstraintKind::Coincident => Some("ACPOINTCOINCIDENCECONSTRAINT"),
        ConstraintKind::Horizontal => Some("ACHORIZONTALCONSTRAINT"),
        ConstraintKind::Vertical => Some("ACVERTICALCONSTRAINT"),
        ConstraintKind::Perpendicular => Some("ACPERPENDICULARCONSTRAINT"),
        ConstraintKind::Tangent => Some("ACTANGENTCONSTRAINT"),
        // the native format's own `GeomConstraintType` enum (native SDK's
        // `AcGeomConstraint.h`) lists `kNormal` alongside `kPerpendicular`
        // as a distinct, real geometric-constraint kind.
        ConstraintKind::Normal => Some("ACNORMALCONSTRAINT"),
        ConstraintKind::Concentric => Some("ACCONCENTRICCONSTRAINT"),
        ConstraintKind::CenterPoint => Some("ACCENTERPOINTCONSTRAINT"),
        ConstraintKind::Colinear => Some("ACCOLINEARCONSTRAINT"),
        ConstraintKind::Fixed => Some("ACFIXEDCONSTRAINT"),
        ConstraintKind::Midpoint => Some("ACMIDPOINTCONSTRAINT"),
        ConstraintKind::PointOnCurve => Some("ACPOINTCURVECONSTRAINT"),
        ConstraintKind::Symmetric => Some("ACSYMMETRICCONSTRAINT"),
        // Also a plain `Geometrical`-shaped node in the native format's own class list
        // (`is_plain_geometrical_constraint`) — unlike `Equal`, its class
        // name never branches on entity type, so it belongs here rather
        // than in the caller's per-entity-type dispatch.
        ConstraintKind::EqualDistance => Some("ACEQUALDISTANCECONSTRAINT"),
        // `Equal` needs the resolved entity types, so the caller handles it.
        ConstraintKind::Equal
        | ConstraintKind::Parallel
        | ConstraintKind::Distance
        | ConstraintKind::Angle
        | ConstraintKind::Radius
        | ConstraintKind::Diameter
        | ConstraintKind::DistanceX
        | ConstraintKind::DistanceY => None,
        // No native DWG representation exists at all: neither
        // `AcExplicitConstr.h` (Distance/Angle/RadiusDiameter — no
        // ArcLength-shaped class) nor `acadrust`'s `AssocConstraintNodeData`
        // has anything for it. `constraint_node` below falls through to
        // its own `_ => None` for this kind, same "can't persist to DWG,
        // XRecord-only" gap the dependency-chain DXF omission already
        // documents.
        ConstraintKind::ArcLength => None,
    }
}

/// `Diameter`/`DistanceX`/`DistanceY` reuse `Radius`'/`Distance`'s own DWG
/// class — native implementations represents them as the same
/// `AcRadiusDiameterConstraint`/`AcDistanceConstraint` object with a
/// different `RadiusDiameterConstrType`/`DirectionType` mode byte (set in
/// `constraint_node` below), not a distinct native class.
const fn dimensional_class_name(kind: ConstraintKind) -> &'static str {
    match kind {
        ConstraintKind::Distance | ConstraintKind::DistanceX | ConstraintKind::DistanceY => {
            "ACDISTANCECONSTRAINT"
        }
        ConstraintKind::Angle => "ACANGLECONSTRAINT",
        ConstraintKind::Radius | ConstraintKind::Diameter => "ACRADIUSDIAMETERCONSTRAINT",
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
/// (mirrors `sketch_solve::build_constraint`'s own "skip, don't error"
/// contract for an unbuildable constraint).
fn constraint_node(
    builder: &mut GroupBuilder,
    document: &CadDocument,
    constraint: &SketchConstraint,
    needs_value_dependency: &mut Vec<(i32, Option<DrivingValue>)>,
) -> Option<i32> {
    let refs: &[SketchRef] = &constraint.refs;
    if let Some(class_name) = geometrical_class_name(constraint.kind) {
        let target_refs: Vec<i32> = refs.iter().filter_map(|r| builder.ref_node(*r)).collect();
        if target_refs.len() != refs.len() {
            return None;
        }
        let owner_id = *target_refs.first()?;
        let node_id = builder.alloc_node_id();
        builder.push_node(
            node_id,
            class_name,
            AssocConstraintNodeData::Geometrical {
                owner_id,
                is_implied: false,
                is_active: true,
            },
        );
        for target in &target_refs {
            builder.connect(node_id, *target);
        }
        return Some(node_id);
    }
    match constraint.kind {
        ConstraintKind::Parallel => {
            let [a, b] = refs else { return None };
            let (na, nb) = (builder.ref_node(*a)?, builder.ref_node(*b)?);
            let node_id = builder.alloc_node_id();
            builder.push_node(
                node_id,
                "ACPARALLELCONSTRAINT",
                AssocConstraintNodeData::Parallel {
                    owner_id: na,
                    is_implied: false,
                    is_active: true,
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
            let both_lines = matches!(document.get_entity(a.entity), Some(EntityType::Line(_)))
                && matches!(document.get_entity(b.entity), Some(EntityType::Line(_)));
            let class_name = if both_lines {
                "ACEQUALLENGTHCONSTRAINT"
            } else {
                "ACEQUALRADIUSCONSTRAINT"
            };
            let node_id = builder.alloc_node_id();
            builder.push_node(
                node_id,
                class_name,
                AssocConstraintNodeData::Geometrical {
                    owner_id: na,
                    is_implied: false,
                    is_active: true,
                },
            );
            builder.connect(node_id, na);
            builder.connect(node_id, nb);
            Some(node_id)
        }
        ConstraintKind::Distance
        | ConstraintKind::DistanceX
        | ConstraintKind::DistanceY
        | ConstraintKind::Angle
        | ConstraintKind::Radius
        | ConstraintKind::Diameter => {
            let target_refs: Vec<i32> = match constraint.kind {
                ConstraintKind::Radius | ConstraintKind::Diameter => {
                    vec![builder.ref_node(*refs.first()?)?]
                }
                _ => {
                    let [a, b] = refs else { return None };
                    vec![builder.ref_node(*a)?, builder.ref_node(*b)?]
                }
            };
            let owner_id = *target_refs.first()?;
            let node_id = builder.alloc_node_id();
            let data = match constraint.kind {
                // Plain two-point distance: undirected (`kNotDirected`).
                ConstraintKind::Distance => AssocConstraintNodeData::Distance {
                    owner_id,
                    is_implied: false,
                    is_active: true,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    direction_type: 0,
                    distance: None,
                },
                // X-only/Y-only: `kFixedDirection` with the fixed unit
                // vector the distance is measured along — the native format's own
                // representation of DistanceX/DistanceY, not a distinct
                // constraint class.
                ConstraintKind::DistanceX => AssocConstraintNodeData::Distance {
                    owner_id,
                    is_implied: false,
                    is_active: true,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    direction_type: 1,
                    distance: Some(Vector3::new(1.0, 0.0, 0.0)),
                },
                ConstraintKind::DistanceY => AssocConstraintNodeData::Distance {
                    owner_id,
                    is_implied: false,
                    is_active: true,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    direction_type: 1,
                    distance: Some(Vector3::new(0.0, 1.0, 0.0)),
                },
                ConstraintKind::Angle => AssocConstraintNodeData::Angle {
                    owner_id,
                    is_implied: false,
                    is_active: true,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    sector_type: 0,
                },
                ConstraintKind::Radius => AssocConstraintNodeData::RadiusDiameter {
                    owner_id,
                    is_implied: false,
                    is_active: true,
                    value_dependency: Handle::NULL,
                    dimension_dependency: Handle::NULL,
                    mode: 0,
                },
                // `kCircleDiameter` — same class as Radius (`mode: 0` /
                // `kCircleRadius`), different mode byte.
                ConstraintKind::Diameter => AssocConstraintNodeData::RadiusDiameter {
                    owner_id,
                    is_implied: false,
                    is_active: true,
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
    /// One `AssocVariable` handle per distinct named parameter actually
    /// referenced in this scope, created once and shared by every
    /// constraint that references the same name (mirrors how the native format's own
    /// variables work — one object, many dependents).
    variables: FxHashMap<String, Handle>,
}

impl Allocator<'_> {
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

    fn geom_dependency(&mut self, group_handle: Handle, entity: Handle) -> Handle {
        self.insert_associative(
            group_handle,
            "ACDBASSOCGEOMDEPENDENCY",
            "AcDbAssocGeomDependency",
            AssociativeData::GeomDependency(AssocGeomDependency {
                dependency: AssocDependency {
                    class_version: 1,
                    is_read_dependency: true,
                    is_write_dependency: true,
                    is_attached_to_object: true,
                    dependent_on: entity,
                    ..Default::default()
                },
                class_version: 1,
                enabled: true,
                // Point and curve addressing lives on constraint-node data.
                persistent_subent: AssocPersistentSubentId::default(),
            }),
        )
    }

    /// Returns the `AssocVariable` handle for `name`, creating it once per scope.
    fn variable(&mut self, group_handle: Handle, name: &str, formula: &str) -> Handle {
        if let Some(&handle) = self.variables.get(name) {
            return handle;
        }
        let handle = self.insert_associative(
            group_handle,
            "ACDBASSOCVARIABLE",
            "AcDbAssocVariable",
            AssociativeData::Variable(AssocVariable {
                action: AssocAction::default(),
                class_version: 1,
                name: name.to_string(),
                expression: formula.to_string(),
                // Empty selects the format's default expression evaluator.
                evaluator: String::new(),
                description: String::new(),
                value: AssocEvalVariant::default(),
                has_cached_value: false,
                cached_value: String::new(),
                flag: false,
                reserved: 0,
            }),
        );
        self.variables.insert(name.to_string(), handle);
        handle
    }

    /// Uses a variable dependency for named values and a null dependency for
    /// literal values.
    fn value_dependency(
        &mut self,
        group_handle: Handle,
        variable_handle: Option<Handle>,
        resolved: f64,
    ) -> Handle {
        let value = AssocEvalVariant {
            code: 40,
            value: AssocEvalValue::Real(resolved),
        };
        self.insert_associative(
            group_handle,
            "ACDBASSOCVALUEDEPENDENCY",
            "AcDbAssocValueDependency",
            AssociativeData::ValueDependency(AssocValueDependency {
                dependency: AssocDependency {
                    class_version: 1,
                    is_read_dependency: true,
                    is_write_dependency: false,
                    dependent_on: variable_handle.unwrap_or(Handle::NULL),
                    ..Default::default()
                },
                class_version: 1,
                name: String::new(),
                value,
            }),
        )
    }
}

/// Sets the dependency on whole-geometry nodes. Implicit points retain a
/// null dependency and refer to their owning geometry node by `curve_id`.
fn set_geometry_dependency(data: &mut AssocConstraintNodeData, handle: Handle) {
    match data {
        AssocConstraintNodeData::BoundedLine {
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

/// The DWG/DXF CLASSES-table entries this module's five `AssociativeObject`
/// kinds need before they can be written with a real class number.
/// Confirmed by reading the actual writer: `write_associative_object`
/// resolves the class number via
/// `document.classes.get_by_name(&object.dxf_name)`
/// (`io/dwg/dwg_stream_writers/object_writer/associative.rs:700-709`), and
/// falls back to a generic type code 500 for every unregistered class alike
/// if it misses — which also means the CLASSES section written to the file
/// never declares them, so a reload can't dispatch the type-500 objects back
/// to `read_associative_object` either (`dwg_document_builder.rs`'s object
/// dispatcher keys off exactly that class-number -> name mapping). A fresh
/// `CadDocument::initialize_defaults` only registers classes every drawing
/// needs, not ones specific to this feature, so this module registers its
/// own — the same pattern `CadDocument::register_object_context_class`
/// already uses for a different object family.
const ASSOC_CLASSES: &[(&str, &str)] = &[
    ("ASSOC2DCONSTRAINTGROUP", "AcDbAssoc2dConstraintGroup"),
    ("ASSOCNETWORK", "AcDbAssocNetwork"),
    ("ASSOCVARIABLE", "AcDbAssocVariable"),
    ("ASSOCGEOMDEPENDENCY", "AcDbAssocGeomDependency"),
    ("ASSOCVALUEDEPENDENCY", "AcDbAssocValueDependency"),
];

fn ensure_associative_classes_registered(document: &mut CadDocument) {
    for (dxf_name, cpp_class_name) in ASSOC_CLASSES {
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
            dwg_version: 0,
            maintenance_version: 0,
            unknown1: 0,
            unknown2: 0,
        });
    }
}

/// Ensures `owner`'s extension dictionary exists, returning its handle.
/// `acadrust::CadDocument` only exposes creating one through
/// [`CadDocument::ensure_xrecord`] — the dictionary side map is private to
/// that crate. A throwaway XRecord is therefore created
/// and immediately removed purely to force that wiring through the one
/// public entry point that gets it right for both DWG and DXF.
fn ensure_extension_dictionary(document: &mut CadDocument, owner: Handle) -> Handle {
    if let Some(handle) = document.extension_dictionary_handle(owner) {
        return handle;
    }
    const BOOTSTRAP_KEY: &str = "OCS_DWG_NATIVE_BOOTSTRAP";
    document.ensure_xrecord(owner, BOOTSTRAP_KEY);
    let dictionary_handle = document
        .extension_dictionary_handle(owner)
        .expect("ensure_xrecord just created the extension dictionary");
    remove_dictionary_entry(document, dictionary_handle, BOOTSTRAP_KEY);
    dictionary_handle
}

/// Removes a named entry from `dictionary_handle`, and recursively removes
/// the object it pointed to — used both for the bootstrap XRecord above and
/// for clearing a stale `ACAD_ASSOCNETWORK` entry before a resave rebuilds
/// it from scratch.
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

/// Whether `owner`'s scope already carries a materialized native graph —
/// from an earlier save with the setting on, or from a file that came from
/// real compatible applications. Drives the "sync-if-present, create-only-if-
/// enabled" rule in [`Scene::materialize_dwg_native_constraints_for_save`]:
/// a scope that already has this graph keeps getting it rebuilt on every
/// save regardless of the current setting, so turning the setting off never
/// leaves a stale, out-of-sync graph behind — only a scope that doesn't have
/// one yet is gated by the setting.
fn has_native_network(document: &CadDocument, owner: Handle) -> bool {
    let Some(dictionary_handle) = document.extension_dictionary_handle(owner) else {
        return false;
    };
    let Some(ObjectType::Dictionary(dictionary)) = document.objects.get(&dictionary_handle) else {
        return false;
    };
    dictionary.get(NETWORK_DICTIONARY_KEY).is_some()
}

/// One scope's worth of
/// [`Scene::materialize_dwg_native_constraints_for_save`] — see that
/// method's doc comment for the save-time contract this implements.
fn materialize_scope(
    document: &mut CadDocument,
    owner: Handle,
    set: &SketchConstraintSet,
    parameters: &ParameterTable,
) {
    let dictionary_handle = ensure_extension_dictionary(document, owner);
    remove_dictionary_entry(document, dictionary_handle, NETWORK_DICTIONARY_KEY);

    // Pass 1 (read-only): constraint-node shapes.
    let mut needs_value_dependency = Vec::new();
    let (mut nodes, entities) = {
        let mut builder = GroupBuilder::new(document);
        for constraint in &set.constraints {
            if !constraint.enabled {
                continue;
            }
            constraint_node(
                &mut builder,
                document,
                constraint,
                &mut needs_value_dependency,
            );
        }
        let GroupBuilder {
            nodes, entities, ..
        } = builder;
        (nodes, entities)
    };
    if nodes.is_empty() {
        return;
    }

    // Root pseudo-node (`nodes[0]`): confirmed mandatory by reading the DWG
    // writer — `Assoc2dConstraintGroup`'s write path only emits node data at
    // all when `nodes.first()` is `Some`, so an empty `Vec` would skip node
    // serialization entirely. This synthetic anchor is what makes every
    // other node reach the file, not decoration.
    let all_ids: Vec<i32> = nodes.iter().map(|n| n.node_id).collect();
    nodes.insert(
        0,
        AssocConstraintNode {
            node_id: 0,
            status: 1,
            connections: all_ids,
            class_name: String::new(),
            registry_flag: false,
            data: AssocConstraintNodeData::None,
        },
    );

    // Pass 2 (mutable): allocate handles for everything the node shapes
    // above still reference as `Handle::NULL`. The group and network
    // reference each other, so both handles are reserved up front.
    let group_handle = document.allocate_handle();
    let network_handle = document.allocate_handle();
    let mut allocator = Allocator {
        document,
        variables: FxHashMap::default(),
    };

    for (entity_handle, entity_nodes) in &entities {
        if entity_nodes.geometry_node_id == 0 {
            continue;
        }
        let dep_handle = allocator.geom_dependency(group_handle, *entity_handle);
        if let Some(node) = nodes
            .iter_mut()
            .find(|n| n.node_id == entity_nodes.geometry_node_id)
        {
            set_geometry_dependency(&mut node.data, dep_handle);
        }
    }

    for (node_id, driving) in needs_value_dependency {
        let Some(driving) = driving else { continue };
        let Ok(resolved) = driving.resolve(parameters) else {
            continue;
        };
        let variable_handle = match &driving {
            DrivingValue::Named(name) => {
                let formula = parameters
                    .get(name)
                    .map(|p| p.source.clone())
                    .unwrap_or_default();
                Some(allocator.variable(group_handle, name, &formula))
            }
            DrivingValue::Literal(_) => None,
        };
        let dep_handle = allocator.value_dependency(group_handle, variable_handle, resolved);
        if let Some(node) = nodes.iter_mut().find(|n| n.node_id == node_id) {
            set_value_dependency(&mut node.data, dep_handle);
        }
    }

    let group = Assoc2dConstraintGroup {
        action: AssocAction {
            owning_network: network_handle,
            ..Default::default()
        },
        version: 1,
        flag: true,
        // Only world-XY entities reach native graph materialization.
        work_plane: [Vector3::ZERO, Vector3::UNIT_X, Vector3::UNIT_Y],
        dependency: Handle::NULL,
        actions: Vec::new(),
        nodes,
    };
    allocator.insert_associative_at(
        group_handle,
        network_handle,
        "ASSOC2DCONSTRAINTGROUP",
        "AcDbAssoc2dConstraintGroup",
        AssociativeData::ConstraintGroup(group),
    );

    let network = AssocNetwork {
        action: AssocAction::default(),
        network_version: 1,
        network_action_index: 0,
        actions: Vec::new(),
        owned_actions: vec![group_handle],
    };
    allocator.insert_associative_at(
        network_handle,
        dictionary_handle,
        "ASSOCNETWORK",
        "AcDbAssocNetwork",
        AssociativeData::Network(network),
    );

    set_dictionary_entry(
        allocator.document,
        dictionary_handle,
        NETWORK_DICTIONARY_KEY,
        network_handle,
    );
}

impl Scene {
    /// Rebuilds every sketch scope's native `AssocNetwork`/
    /// `Assoc2dConstraintGroup` graph from `self.sketch_constraints`,
    /// replacing whatever this module wrote on a previous save.
    ///
    /// `enabled` is the app's `write_dwg_native_constraints` Options toggle
    /// (off by default — this graph adds file weight every save, whether or
    /// not compatible applications interop is wanted). It only gates *creating* the
    /// graph in a scope that doesn't have one yet: a scope that already
    /// carries one — from an earlier save with the setting on, or from a
    /// file that came from native implementations — keeps getting it rebuilt in sync
    /// regardless of the current setting ([`has_native_network`]), so
    /// toggling this off never leaves a stale graph sitting in the file.
    pub(crate) fn materialize_dwg_native_constraints_for_save(&mut self, enabled: bool) {
        let mut classes_registered = false;
        for index in 0..self.sketch_constraints.len() {
            let set = self.sketch_constraints[index].clone();
            let owner = set.scope.owner_handle(&self.document);
            if owner.is_null() {
                continue;
            }
            if !enabled && !has_native_network(&self.document, owner) {
                continue;
            }
            if !classes_registered {
                ensure_associative_classes_registered(&mut self.document);
                classes_registered = true;
            }
            materialize_scope(&mut self.document, owner, &set, &self.named_parameters);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::sketch_constraints::SketchScope;
    use super::*;
    use acadrust::entities::{Arc, Circle, Line};

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

    /// Digs a scope's materialized native graph back out of `document`:
    /// `owner`'s extension dictionary -> `ACAD_ASSOCNETWORK` entry ->
    /// its one owned action, which (for this module) is always the
    /// `Assoc2dConstraintGroup` itself. Panics with a descriptive message on
    /// any broken link, since every test using this expects the graph to be
    /// present and well-formed.
    fn native_group(document: &CadDocument, owner: Handle) -> &Assoc2dConstraintGroup {
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
        let group_handle = *network
            .owned_actions
            .first()
            .expect("network should own the constraint group");
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
        for ext in ["dxf", "dwg"] {
            let mut scene = Scene::new();
            let a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
            let b = line_entity(&mut scene, (10.0, 0.0), (10.0, 5.0));
            scene
                .sketch_constraint_set_mut(SketchScope::ModelSpace)
                .add(ConstraintKind::Horizontal, vec![SketchRef::whole(a)], None);
            scene
                .sketch_constraint_set_mut(SketchScope::ModelSpace)
                .add(ConstraintKind::Vertical, vec![SketchRef::whole(b)], None);

            scene.materialize_dwg_native_constraints_for_save(true);
            let owner = scene.document.header.model_space_block_handle;
            // Sanity before the round trip even happens: root + 2 geometry
            // nodes + 2 constraint nodes.
            let before = native_group(&scene.document, owner);
            assert_eq!(
                before.nodes.len(),
                5,
                "expected root + 2 geometry + 2 constraint nodes, got {:#?}",
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
                class_names.contains(&"ACHORIZONTALCONSTRAINT"),
                "{ext}: missing horizontal constraint node, got {class_names:?}"
            );
            assert!(
                class_names.contains(&"ACVERTICALCONSTRAINT"),
                "{ext}: missing vertical constraint node, got {class_names:?}"
            );
            assert_eq!(
                class_names
                    .iter()
                    .filter(|&&n| n == "ACCONSTRAINEDBOUNDEDLINE")
                    .count(),
                2,
                "{ext}: expected both lines' geometry nodes to survive, got {class_names:?}"
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
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(
                ConstraintKind::Distance,
                vec![SketchRef::point(a, 1), SketchRef::point(b, 0)],
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

    /// DWG is the format this module's dependency chain (`AssocGeomDependency`
    /// / `AssocValueDependency` / `AssocVariable`) actually survives a save
    /// through — see [`a_dxf_save_keeps_only_the_constraint_group_shell`] for
    /// the DXF side of this same scene and why it's different, not a bug here.
    #[test]
    fn a_dwg_save_keeps_the_full_dependency_chain_including_the_named_variable() {
        let (mut scene, owner) = distance_with_named_parameter_scene();
        scene.materialize_dwg_native_constraints_for_save(true);
        assert!(
            has_variable_named(&scene.document, "gap"),
            "expected an AssocVariable BEFORE serialization"
        );

        let bytes = crate::io::save_to_bytes(&scene.document, "dwg", scene.document.version)
            .unwrap_or_else(|e| panic!("save to dwg: {e}"));
        let reloaded = crate::io::load_bytes("dwg_native_named_param.dwg", bytes)
            .unwrap_or_else(|e| panic!("reload dwg: {e}"));

        let group = native_group(&reloaded, owner);
        assert!(
            group
                .nodes
                .iter()
                .any(|n| n.class_name == "ACDISTANCECONSTRAINT"),
            "expected an ACDISTANCECONSTRAINT node, got {:?}",
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
    }

    /// Confirmed by reading the actual DXF writer
    /// (`io/dxf/writer/section_writer.rs`'s `is_unrestorable_assoc_object`,
    /// called from its `ObjectType::Associative` write arm): `acadrust`'s DXF
    /// path deliberately does not write `AssocDependency`/
    /// `AssocValueDependency`/`AssocGeomDependency`/`AssocVariable` objects at
    /// all ("Unrestorable associative-framework objects are not written") —
    /// DWG has no equivalent filter. So a DXF save keeps the
    /// `Assoc2dConstraintGroup`/`AssocNetwork` shell (and every constraint
    /// node's own embedded geometry — real coordinates, not lost), but each
    /// node's `geometry_dependency`/`value_dependency` handle points at
    /// nothing once reloaded, and no `AssocVariable` survives at all. This is
    /// a real, upstream `acadrust` limitation, not something this module can
    /// work around without patching that dependency — out of scope here (see
    /// this project's stated preference, `named_parameters_persist.rs`, to
    /// pick a different approach rather than patch `acadrust`). DWG is
    /// therefore the only format this feature round-trips fully through.
    #[test]
    fn a_dxf_save_keeps_only_the_constraint_group_shell() {
        let (mut scene, owner) = distance_with_named_parameter_scene();
        scene.materialize_dwg_native_constraints_for_save(true);

        let bytes = crate::io::save_to_bytes(&scene.document, "dxf", scene.document.version)
            .unwrap_or_else(|e| panic!("save to dxf: {e}"));
        let reloaded = crate::io::load_bytes("dwg_native_named_param.dxf", bytes)
            .unwrap_or_else(|e| panic!("reload dxf: {e}"));

        let group = native_group(&reloaded, owner);
        assert!(
            group
                .nodes
                .iter()
                .any(|n| n.class_name == "ACDISTANCECONSTRAINT"),
            "expected the ACDISTANCECONSTRAINT node's shell to survive, got {:?}",
            group
                .nodes
                .iter()
                .map(|n| &n.class_name)
                .collect::<Vec<_>>()
        );
        assert!(
            !has_variable_named(&reloaded, "gap"),
            "acadrust's DXF writer should not carry AssocVariable; update the native materializer if this changes"
        );
    }

    #[test]
    fn resaving_replaces_rather_than_accumulates_native_objects() {
        let mut scene = Scene::new();
        let a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(ConstraintKind::Horizontal, vec![SketchRef::whole(a)], None);

        scene.materialize_dwg_native_constraints_for_save(true);
        let first_count = scene.document.objects.len();
        scene.materialize_dwg_native_constraints_for_save(true);
        let second_count = scene.document.objects.len();
        assert_eq!(
            first_count, second_count,
            "a resave with the same constraints must not accumulate new objects"
        );
    }

    #[test]
    fn an_empty_constraint_set_leaves_no_native_network() {
        let mut scene = Scene::new();
        let _ = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
        scene.materialize_dwg_native_constraints_for_save(true);

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
        has_native_network(
            &scene.document,
            scene.document.header.model_space_block_handle,
        )
    }

    #[test]
    fn the_setting_off_skips_a_scope_with_no_existing_native_graph() {
        let mut scene = Scene::new();
        let a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(ConstraintKind::Horizontal, vec![SketchRef::whole(a)], None);

        scene.materialize_dwg_native_constraints_for_save(false);
        assert!(
            !model_space_has_native_network(&scene),
            "a scope with no prior native graph should stay untouched while the setting is off"
        );
    }

    /// The "sync-if-present, create-only-if-enabled" rule from
    /// `Scene::materialize_dwg_native_constraints_for_save`'s doc comment:
    /// once a scope has the native graph (here, from an earlier save with
    /// the setting on), turning the setting off must not freeze that graph
    /// out of date — it should keep tracking the current constraints.
    #[test]
    fn the_setting_off_still_resyncs_a_scope_that_already_has_a_native_graph() {
        let mut scene = Scene::new();
        let a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(ConstraintKind::Horizontal, vec![SketchRef::whole(a)], None);
        scene.materialize_dwg_native_constraints_for_save(true);
        assert!(
            model_space_has_native_network(&scene),
            "sanity: the graph should exist after the first, enabled save"
        );

        let b = line_entity(&mut scene, (0.0, 0.0), (0.0, 10.0));
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(ConstraintKind::Vertical, vec![SketchRef::whole(b)], None);
        scene.materialize_dwg_native_constraints_for_save(false);

        let owner = scene.document.header.model_space_block_handle;
        let group = native_group(&scene.document, owner);
        assert!(
            group.nodes.iter().any(|n| n.class_name == "ACVERTICALCONSTRAINT"),
            "the second, disabled-setting save should still pick up the new Vertical constraint, got {:?}",
            group.nodes.iter().map(|n| &n.class_name).collect::<Vec<_>>()
        );
    }

    /// The other half of the same rule: disabling the setting after a scope
    /// already has the graph must not make that graph disappear either —
    /// only remove it by clearing the scope's constraints (existing
    /// `an_empty_constraint_set_leaves_no_native_network` behavior), not by
    /// toggling the setting off.
    #[test]
    fn the_setting_off_does_not_remove_an_existing_native_graph() {
        let mut scene = Scene::new();
        let a = line_entity(&mut scene, (0.0, 0.0), (10.0, 0.0));
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(ConstraintKind::Horizontal, vec![SketchRef::whole(a)], None);
        scene.materialize_dwg_native_constraints_for_save(true);
        scene.materialize_dwg_native_constraints_for_save(false);
        assert!(
            model_space_has_native_network(&scene),
            "an existing native graph must survive a disabled-setting resave, not be silently dropped"
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

            let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
            set.add(
                ConstraintKind::Concentric,
                vec![SketchRef::center(circle_a), SketchRef::center(circle_b)],
                None,
            );
            set.add(
                ConstraintKind::Colinear,
                vec![SketchRef::whole(line_a), SketchRef::whole(line_b)],
                None,
            );
            set.add(ConstraintKind::Fixed, vec![SketchRef::whole(line_a)], None);
            set.add(
                ConstraintKind::CenterPoint,
                vec![SketchRef::point(marker, 0), SketchRef::center(circle_a)],
                None,
            );
            set.add(
                ConstraintKind::Midpoint,
                vec![SketchRef::point(marker, 1), SketchRef::whole(line_a)],
                None,
            );
            set.add(
                ConstraintKind::PointOnCurve,
                vec![SketchRef::point(marker, 0), SketchRef::whole(circle_a)],
                None,
            );
            set.add(
                ConstraintKind::EqualDistance,
                vec![
                    SketchRef::point(line_b, 0),
                    SketchRef::point(line_b, 1),
                    SketchRef::point(line_a, 0),
                    SketchRef::point(line_a, 1),
                ],
                None,
            );
            set.add(
                ConstraintKind::Symmetric,
                vec![
                    SketchRef::center(circle_a),
                    SketchRef::center(circle_b),
                    SketchRef::whole(axis),
                ],
                None,
            );
            set.add(
                ConstraintKind::Normal,
                vec![SketchRef::whole(circle_a), SketchRef::whole(line_a)],
                None,
            );

            scene.materialize_dwg_native_constraints_for_save(true);
            let owner = scene.document.header.model_space_block_handle;

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded = crate::io::load_bytes(&format!("new_kinds.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded, owner);
            let class_names: Vec<&str> =
                group.nodes.iter().map(|n| n.class_name.as_str()).collect();

            for expected in [
                "ACCONCENTRICCONSTRAINT",
                "ACCOLINEARCONSTRAINT",
                "ACFIXEDCONSTRAINT",
                "ACCENTERPOINTCONSTRAINT",
                "ACMIDPOINTCONSTRAINT",
                "ACPOINTCURVECONSTRAINT",
                "ACEQUALDISTANCECONSTRAINT",
                "ACSYMMETRICCONSTRAINT",
                "ACNORMALCONSTRAINT",
            ] {
                assert!(
                    class_names.contains(&expected),
                    "{ext}: missing {expected} node, got {class_names:?}"
                );
            }
        }
    }

    /// Constraint-parity Phase 1: an Arc registers in the solver as its own
    /// center/radius (`sketch_solve.rs`'s `EntityGeom::Circle` reuse), and
    /// this module already had `ACCONSTRAINEDARC` geometry-dependency
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

            let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
            set.add(
                ConstraintKind::Concentric,
                vec![SketchRef::center(arc), SketchRef::center(circle)],
                None,
            );
            set.add(
                ConstraintKind::Radius,
                vec![SketchRef::whole(arc)],
                Some(crate::scene::named_parameters::DrivingValue::Literal(9.0)),
            );

            scene.materialize_dwg_native_constraints_for_save(true);
            let owner = scene.document.header.model_space_block_handle;

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded = crate::io::load_bytes(&format!("arc_ref.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded, owner);
            let class_names: Vec<&str> =
                group.nodes.iter().map(|n| n.class_name.as_str()).collect();
            assert!(
                class_names.contains(&"ACCONCENTRICCONSTRAINT"),
                "{ext}: missing ACCONCENTRICCONSTRAINT, got {class_names:?}"
            );
            assert!(
                class_names.contains(&"ACRADIUSDIAMETERCONSTRAINT"),
                "{ext}: missing ACRADIUSDIAMETERCONSTRAINT, got {class_names:?}"
            );
            assert!(
                group
                    .nodes
                    .iter()
                    .any(|n| n.class_name == "ACCONSTRAINEDARC"),
                "{ext}: the arc should have its own geometry node, got {class_names:?}"
            );
        }
    }

    /// Constraint-parity Phase 4b: a Coincident constraint referencing an
    /// arc's actual *endpoint* (marker 0, not its center via `-3` or its
    /// whole curve) round-trips correctly too — `SketchRef`'s marker is
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
                .sketch_constraint_set_mut(SketchScope::ModelSpace)
                .add(
                    ConstraintKind::Coincident,
                    vec![SketchRef::point(arc, 0), SketchRef::point(line, 0)],
                    None,
                );

            scene.materialize_dwg_native_constraints_for_save(true);
            let owner = scene.document.header.model_space_block_handle;

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded = crate::io::load_bytes(&format!("arc_endpoint_ref.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded, owner);
            let class_names: Vec<&str> =
                group.nodes.iter().map(|n| n.class_name.as_str()).collect();
            assert!(
                class_names.contains(&"ACPOINTCOINCIDENCECONSTRAINT"),
                "{ext}: missing ACPOINTCOINCIDENCECONSTRAINT, got {class_names:?}"
            );
            assert!(
                group
                    .nodes
                    .iter()
                    .any(|n| n.class_name == "ACCONSTRAINEDARC"),
                "{ext}: the arc should still have its own geometry node, got {class_names:?}"
            );
        }
    }

    /// Constraint-parity Phase 5: an ellipse isn't a supported geometry
    /// node type in `geometry_node` yet (see its own doc comment for why),
    /// so a Concentric constraint referencing one degrades to "not in the
    /// native graph" — the same "skip, don't error" contract every other
    /// unbuildable native-constraint case gets — while the other
    /// constraint in the same scope (Fixed, on a plain circle) still
    /// persists normally, and the ellipse constraint itself still survives
    /// in this app's own XRecord format.
    #[test]
    fn a_constraint_referencing_an_ellipse_is_absent_from_the_native_graph_but_not_from_xrecord() {
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

            let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
            set.add(
                ConstraintKind::Concentric,
                vec![SketchRef::center(ellipse), SketchRef::center(circle)],
                None,
            );
            set.add(ConstraintKind::Fixed, vec![SketchRef::whole(circle)], None);

            scene.materialize_sketch_constraints_for_save();
            scene.materialize_dwg_native_constraints_for_save(true);
            let owner = scene.document.header.model_space_block_handle;

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded_doc = crate::io::load_bytes(&format!("ellipse_ref.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded_doc, owner);
            let class_names: Vec<&str> =
                group.nodes.iter().map(|n| n.class_name.as_str()).collect();
            assert!(
                class_names.contains(&"ACFIXEDCONSTRAINT"),
                "{ext}: the other constraint should still persist, got {class_names:?}"
            );
            assert!(
                !class_names.contains(&"ACCONCENTRICCONSTRAINT"),
                "{ext}: the ellipse-referencing Concentric constraint has no supported geometry node and must not appear, got {class_names:?}"
            );

            let mut reloaded_scene = Scene::new();
            reloaded_scene.document = reloaded_doc;
            reloaded_scene.load_sketch_constraints_from_document();
            let restored = reloaded_scene
                .sketch_constraint_set(SketchScope::ModelSpace)
                .unwrap_or_else(|| {
                    panic!("{ext}: no ModelSpace constraint set survived the round trip")
                });
            let kinds: Vec<ConstraintKind> = restored.constraints.iter().map(|c| c.kind).collect();
            assert!(
                kinds.contains(&ConstraintKind::Concentric),
                "{ext}: the ellipse constraint should still round-trip via XRecord, got {kinds:?}"
            );
        }
    }

    /// Constraint-parity Phase 4a: `ArcLength` has no native SDK class at
    /// all (confirmed against `AcExplicitConstr.h`) — this asserts the
    /// degradation is exactly as documented: the native DWG/DXF graph
    /// gets every *other* constraint on the arc (Radius, here) but no
    /// ArcLength-shaped node, while the app's own constraint set (what
    /// XRecord persistence actually serializes, per `sketch_persist.rs`'s
    /// fully-generic encode/decode) still carries it untouched.
    #[test]
    fn arc_length_has_no_native_class_but_survives_in_the_apps_own_constraint_set() {
        for ext in ["dxf", "dwg"] {
            let mut scene = Scene::new();
            let arc = arc_entity(&mut scene, (0.0, 0.0), 4.0);

            let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
            set.add(
                ConstraintKind::Radius,
                vec![SketchRef::whole(arc)],
                Some(DrivingValue::Literal(4.0)),
            );
            set.add(
                ConstraintKind::ArcLength,
                vec![SketchRef::whole(arc)],
                Some(DrivingValue::Literal(6.0)),
            );

            // Both materializers, "called alongside, not instead of" each
            // other (`materialize_dwg_native_constraints_for_save`'s own
            // doc comment) — the native graph and this app's own XRecord
            // format are independent, additive persistence paths.
            scene.materialize_sketch_constraints_for_save();
            scene.materialize_dwg_native_constraints_for_save(true);
            let owner = scene.document.header.model_space_block_handle;

            let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
                .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
            let reloaded_doc = crate::io::load_bytes(&format!("arc_length.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext}: {e}"));

            let group = native_group(&reloaded_doc, owner);
            let class_names: Vec<&str> =
                group.nodes.iter().map(|n| n.class_name.as_str()).collect();
            assert!(
                class_names.contains(&"ACRADIUSDIAMETERCONSTRAINT"),
                "{ext}: missing ACRADIUSDIAMETERCONSTRAINT, got {class_names:?}"
            );
            assert!(
                !class_names.iter().any(|n| n.contains("ARCLENGTH")),
                "{ext}: ArcLength has no native DWG class and must not appear here, got {class_names:?}"
            );

            // Still present in this app's own (XRecord-backed) constraint
            // set — reload into a fresh `Scene` and read it back the same
            // way `sketch_persist.rs`'s own round-trip tests do.
            let mut reloaded_scene = Scene::new();
            reloaded_scene.document = reloaded_doc;
            reloaded_scene.load_sketch_constraints_from_document();
            let restored = reloaded_scene
                .sketch_constraint_set(SketchScope::ModelSpace)
                .unwrap_or_else(|| {
                    panic!("{ext}: no ModelSpace constraint set survived the round trip")
                });
            let kinds: Vec<ConstraintKind> = restored.constraints.iter().map(|c| c.kind).collect();
            assert!(
                kinds.contains(&ConstraintKind::ArcLength),
                "{ext}: ArcLength should still round-trip via XRecord, got {kinds:?}"
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

            let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
            set.add(
                ConstraintKind::Diameter,
                vec![SketchRef::whole(circle)],
                Some(DrivingValue::Literal(16.0)),
            );
            set.add(
                ConstraintKind::DistanceX,
                vec![SketchRef::point(line, 0), SketchRef::point(line, 1)],
                Some(DrivingValue::Literal(10.0)),
            );
            set.add(
                ConstraintKind::DistanceY,
                vec![SketchRef::point(line, 0), SketchRef::point(line, 1)],
                Some(DrivingValue::Literal(3.0)),
            );

            scene.materialize_dwg_native_constraints_for_save(true);
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
}
