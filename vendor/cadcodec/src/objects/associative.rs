//! Semantic model for associative-network objects.
//!
//! AutoCAD stores most associative objects as short inheritance chains.  The
//! shared structs below mirror those chains so DWG and DXF use one lossless
//! representation instead of class-specific byte blobs.

use crate::entities::{AcisData, Silhouette, Wire};
use crate::types::{DxfVersion, Handle, Vector2, Vector3};

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssociativeObject {
    pub handle: Handle,
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    pub dxf_name: String,
    pub cpp_class_name: String,
    pub data: AssociativeData,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub source_version: Option<DxfVersion>,
}

impl AssociativeObject {
    pub fn new(dxf_name: impl Into<String>, cpp_class_name: impl Into<String>) -> Self {
        Self {
            dxf_name: dxf_name.into(),
            cpp_class_name: cpp_class_name.into(),
            ..Self::default()
        }
    }

    pub(crate) fn visit_handles_mut(&mut self, visit: &mut impl FnMut(&mut Handle)) {
        visit(&mut self.owner);
        for handle in &mut self.reactors {
            visit(handle);
        }
        if let Some(handle) = self.xdictionary_handle.as_mut() {
            visit(handle);
        }
        self.data.visit_handles_mut(visit);
    }

    /// Whether any owner/reactor/payload reference points at `target`.
    ///
    /// This read-side query deliberately reuses the exhaustive handle visitor,
    /// so new associative payload variants cannot silently disappear from
    /// document relationship lookups.
    pub fn references_handle(&self, target: Handle) -> bool {
        self.owner == target
            || self.reactors.contains(&target)
            || self.xdictionary_handle == Some(target)
            || self.data.references_handle(target)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AssociativeData {
    #[default]
    Unknown,
    Dependency(AssocDependency),
    ValueDependency(AssocValueDependency),
    GeomDependency(AssocGeomDependency),
    SurfaceActionBody(AssocSurfaceActionBody),
    Action(AssocAction),
    Network(AssocNetwork),
    AnnotationActionBody(AssocAnnotationActionBody),
    PersSubentManager(AssocPersSubentManager),
    EdgeActionParam(AssocEdgeActionParam),
    ConstraintGroup(Assoc2dConstraintGroup),
    Variable(AssocVariable),
    ActionParam(AssocActionParam),
    CompoundActionParam(AssocCompoundActionParam),
    OsnapPointRefActionParam(AssocOsnapPointRefActionParam),
    PointRefActionParam(AssocCompoundActionParam),
    ObjectActionParam(AssocSingleDependencyActionParam),
    PathActionParam(AssocPathActionParam),
    DimDependencyBody(AssocDimDependencyBody),
    FaceActionParam(AssocFaceActionParam),
    VertexActionParam(AssocVertexActionParam),
    AsmBodyActionParam(AssocAsmBodyActionParam),
    ArrayParameters(AssocArrayParameters),
    ArrayActionBody(AssocArrayActionBody),
    ArrayModifyActionBody(AssocArrayModifyActionBody),
    DimensionAssociation(AssocDimensionAssociation),
    PersSubentManagerStatic(PersSubentManager),
    ViewRepActionBody(AssocViewRepActionBody),
    ViewObjectActionParam(AssocViewObjectActionParam),
    ViewRepHatchManager(AssocViewRepHatchManager),
    ViewRepHatchActionParam(AssocViewRepHatchActionParam),
    ViewLabelActionParam(AssocViewLabelActionParam),
}

impl AssociativeData {
    fn references_handle(&self, target: Handle) -> bool {
        match self {
            Self::Unknown
            | Self::ActionParam(_)
            | Self::DimDependencyBody(_)
            | Self::PersSubentManager(_)
            | Self::PersSubentManagerStatic(_) => false,
            Self::Dependency(value) => dependency_references(value, target),
            Self::ValueDependency(value) => {
                dependency_references(&value.dependency, target)
                    || eval_references(&value.value, target)
            }
            Self::GeomDependency(value) => dependency_references(&value.dependency, target),
            Self::SurfaceActionBody(value) => {
                parameter_body_references(&value.parameter_body, target)
                    || value.surface_body.dependency == target
            }
            Self::Action(value) => action_references(value, target),
            Self::Network(value) => {
                action_references(&value.action, target)
                    || value
                        .actions
                        .iter()
                        .any(|dependency| dependency.dependency == target)
                    || value.owned_actions.contains(&target)
            }
            Self::AnnotationActionBody(value) => {
                parameter_body_references(&value.annotation.parameter_body, target)
                    || value.annotation.dependency == target
                    || value.entity == target
                    || value
                        .actions
                        .iter()
                        .any(|dependency| dependency.dependency == target)
                    || value.read_node == target
                    || value.dimension_node == target
                    || value.dependency == target
            }
            Self::EdgeActionParam(value) => {
                single_dependency_references(&value.single_dependency, target)
                    || value.parameter == target
            }
            Self::ConstraintGroup(value) => {
                action_references(&value.action, target)
                    || value.dependency == target
                    || value.actions.contains(&target)
                    || value.nodes.iter().any(|node| match &node.data {
                        AssocConstraintNodeData::Angle {
                            value_dependency,
                            dimension_dependency,
                            ..
                        }
                        | AssocConstraintNodeData::Distance {
                            value_dependency,
                            dimension_dependency,
                            ..
                        }
                        | AssocConstraintNodeData::RadiusDiameter {
                            value_dependency,
                            dimension_dependency,
                            ..
                        } => *value_dependency == target || *dimension_dependency == target,
                        AssocConstraintNodeData::ImplicitPoint {
                            geometry_dependency,
                            ..
                        }
                        | AssocConstraintNodeData::Point {
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
                        } => *geometry_dependency == target,
                        _ => false,
                    })
            }
            Self::Variable(value) => {
                action_references(&value.action, target) || eval_references(&value.value, target)
            }
            Self::CompoundActionParam(value)
            | Self::PointRefActionParam(value)
            | Self::PathActionParam(AssocPathActionParam {
                compound: value, ..
            }) => compound_references(value, target),
            Self::OsnapPointRefActionParam(value) => compound_references(&value.compound, target),
            Self::ObjectActionParam(value)
            | Self::FaceActionParam(AssocFaceActionParam {
                single_dependency: value,
                ..
            })
            | Self::VertexActionParam(AssocVertexActionParam {
                single_dependency: value,
                ..
            }) => single_dependency_references(value, target),
            Self::AsmBodyActionParam(value) => {
                single_dependency_references(&value.single_dependency, target)
                    || value.history == target
            }
            Self::ArrayParameters(value) => value.items.iter().any(|item| {
                item.first_handle == Some(target) || item.second_handle == Some(target)
            }),
            Self::ArrayActionBody(value) => {
                parameter_body_references(&value.parameter_body, target)
            }
            Self::ArrayModifyActionBody(value) => {
                parameter_body_references(&value.body.parameter_body, target)
            }
            Self::DimensionAssociation(value) => {
                value.dimension == target
                    || value.references.iter().flatten().any(|reference| {
                        reference.xrefs.contains(&target)
                            || reference.intersection_objects.contains(&target)
                    })
            }
            Self::ViewRepActionBody(value) => value.view_rep == target,
            Self::ViewObjectActionParam(value) => {
                single_dependency_references(&value.single_dependency, target)
            }
            Self::ViewRepHatchManager(value) => {
                compound_references(&value.compound, target)
                    || value.items.iter().any(|item| item.parameter == target)
            }
            Self::ViewRepHatchActionParam(value) => {
                single_dependency_references(&value.single_dependency, target)
            }
            Self::ViewLabelActionParam(value) => {
                single_dependency_references(&value.single_dependency, target)
            }
        }
    }

    pub(crate) fn visit_handles_mut(&mut self, visit: &mut impl FnMut(&mut Handle)) {
        match self {
            Self::Unknown
            | Self::ActionParam(_)
            | Self::DimDependencyBody(_)
            | Self::PersSubentManager(_)
            | Self::PersSubentManagerStatic(_) => {}
            Self::Dependency(value) => visit_dependency(value, visit),
            Self::ValueDependency(value) => {
                visit_dependency(&mut value.dependency, visit);
                visit_eval(&mut value.value, visit);
            }
            Self::GeomDependency(value) => {
                visit_dependency(&mut value.dependency, visit);
            }
            Self::SurfaceActionBody(value) => {
                visit_parameter_body(&mut value.parameter_body, visit);
                visit(&mut value.surface_body.dependency);
            }
            Self::Action(value) => visit_action(value, visit),
            Self::Network(value) => {
                visit_action(&mut value.action, visit);
                for dependency in &mut value.actions {
                    visit(&mut dependency.dependency);
                }
                for handle in &mut value.owned_actions {
                    visit(handle);
                }
            }
            Self::AnnotationActionBody(value) => {
                visit_parameter_body(&mut value.annotation.parameter_body, visit);
                visit(&mut value.annotation.dependency);
                visit(&mut value.entity);
                for dependency in &mut value.actions {
                    visit(&mut dependency.dependency);
                }
                visit(&mut value.read_node);
                visit(&mut value.dimension_node);
                visit(&mut value.dependency);
            }
            Self::EdgeActionParam(value) => {
                visit_single_dependency(&mut value.single_dependency, visit);
                visit(&mut value.parameter);
            }
            Self::ConstraintGroup(value) => {
                visit_action(&mut value.action, visit);
                visit(&mut value.dependency);
                for handle in &mut value.actions {
                    visit(handle);
                }
                for node in &mut value.nodes {
                    match &mut node.data {
                        AssocConstraintNodeData::Angle {
                            value_dependency,
                            dimension_dependency,
                            ..
                        }
                        | AssocConstraintNodeData::Distance {
                            value_dependency,
                            dimension_dependency,
                            ..
                        }
                        | AssocConstraintNodeData::RadiusDiameter {
                            value_dependency,
                            dimension_dependency,
                            ..
                        } => {
                            visit(value_dependency);
                            visit(dimension_dependency);
                        }
                        AssocConstraintNodeData::ImplicitPoint {
                            geometry_dependency,
                            ..
                        }
                        | AssocConstraintNodeData::Point {
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
                        } => visit(geometry_dependency),
                        _ => {}
                    }
                }
            }
            Self::Variable(value) => {
                visit_action(&mut value.action, visit);
                visit_eval(&mut value.value, visit);
            }
            Self::CompoundActionParam(value)
            | Self::PointRefActionParam(value)
            | Self::PathActionParam(AssocPathActionParam {
                compound: value, ..
            }) => visit_compound(value, visit),
            Self::OsnapPointRefActionParam(value) => {
                visit_compound(&mut value.compound, visit);
            }
            Self::ObjectActionParam(value)
            | Self::FaceActionParam(AssocFaceActionParam {
                single_dependency: value,
                ..
            })
            | Self::VertexActionParam(AssocVertexActionParam {
                single_dependency: value,
                ..
            })
            | Self::AsmBodyActionParam(AssocAsmBodyActionParam {
                single_dependency: value,
                ..
            }) => visit_single_dependency(value, visit),
            Self::ArrayParameters(value) => {
                for item in &mut value.items {
                    if let Some(handle) = item.first_handle.as_mut() {
                        visit(handle);
                    }
                    if let Some(handle) = item.second_handle.as_mut() {
                        visit(handle);
                    }
                }
            }
            Self::ArrayActionBody(value) => {
                visit_parameter_body(&mut value.parameter_body, visit);
            }
            Self::ArrayModifyActionBody(value) => {
                visit_parameter_body(&mut value.body.parameter_body, visit);
            }
            Self::DimensionAssociation(value) => {
                visit(&mut value.dimension);
                for reference in value.references.iter_mut().flatten() {
                    for handle in &mut reference.xrefs {
                        visit(handle);
                    }
                    for handle in &mut reference.intersection_objects {
                        visit(handle);
                    }
                }
            }
            Self::ViewRepActionBody(value) => visit(&mut value.view_rep),
            Self::ViewObjectActionParam(value) => {
                visit_single_dependency(&mut value.single_dependency, visit);
            }
            Self::ViewRepHatchManager(value) => {
                visit_compound(&mut value.compound, visit);
                for item in &mut value.items {
                    visit(&mut item.parameter);
                }
            }
            Self::ViewRepHatchActionParam(value) => {
                visit_single_dependency(&mut value.single_dependency, visit);
            }
            Self::ViewLabelActionParam(value) => {
                visit_single_dependency(&mut value.single_dependency, visit);
            }
        }
        if let Self::AsmBodyActionParam(value) = self {
            visit(&mut value.history);
        }
    }
}

fn eval_references(value: &AssocEvalVariant, target: Handle) -> bool {
    matches!(value.value, AssocEvalValue::Handle(handle) if handle == target)
}

fn value_param_references(value: &AssocValueParam, target: Handle) -> bool {
    value.controlled_object_dependency == target
        || value
            .variables
            .iter()
            .any(|variable| variable.handle == target || eval_references(&variable.value, target))
}

fn dependency_references(value: &AssocDependency, target: Handle) -> bool {
    value.dependent_on == target
        || value.read_dependency == target
        || value.node == target
        || value.dependency_body == target
}

fn action_references(value: &AssocAction, target: Handle) -> bool {
    value.owning_network == target
        || value.action_body == target
        || value
            .dependencies
            .iter()
            .any(|dependency| dependency.dependency == target)
        || value.owned_parameters.contains(&target)
        || value
            .values
            .iter()
            .any(|parameter| value_param_references(parameter, target))
}

fn parameter_body_references(value: &AssocParamBasedActionBody, target: Handle) -> bool {
    value.dependencies.contains(&target)
        || value.dependency == target
        || value
            .values
            .iter()
            .any(|parameter| value_param_references(parameter, target))
}

fn single_dependency_references(value: &AssocSingleDependencyActionParam, target: Handle) -> bool {
    value.dependency == target
}

fn compound_references(value: &AssocCompoundActionParam, target: Handle) -> bool {
    value.parameters.contains(&target)
        || value.child_parameter.as_ref().is_some_and(|child| {
            child.parameter == target
                || child.secondary_parameter == target
                || child.tertiary_parameter == target
        })
}

fn visit_eval(value: &mut AssocEvalVariant, visit: &mut impl FnMut(&mut Handle)) {
    if let AssocEvalValue::Handle(handle) = &mut value.value {
        visit(handle);
    }
}

fn visit_value_param(value: &mut AssocValueParam, visit: &mut impl FnMut(&mut Handle)) {
    for variable in &mut value.variables {
        visit_eval(&mut variable.value, visit);
        visit(&mut variable.handle);
    }
    visit(&mut value.controlled_object_dependency);
}

fn visit_dependency(value: &mut AssocDependency, visit: &mut impl FnMut(&mut Handle)) {
    visit(&mut value.dependent_on);
    visit(&mut value.read_dependency);
    visit(&mut value.node);
    visit(&mut value.dependency_body);
}

fn visit_action(value: &mut AssocAction, visit: &mut impl FnMut(&mut Handle)) {
    visit(&mut value.owning_network);
    visit(&mut value.action_body);
    for dependency in &mut value.dependencies {
        visit(&mut dependency.dependency);
    }
    for handle in &mut value.owned_parameters {
        visit(handle);
    }
    for parameter in &mut value.values {
        visit_value_param(parameter, visit);
    }
}

fn visit_parameter_body(
    value: &mut AssocParamBasedActionBody,
    visit: &mut impl FnMut(&mut Handle),
) {
    for handle in &mut value.dependencies {
        visit(handle);
    }
    for parameter in &mut value.values {
        visit_value_param(parameter, visit);
    }
    visit(&mut value.dependency);
}

fn visit_single_dependency(
    value: &mut AssocSingleDependencyActionParam,
    visit: &mut impl FnMut(&mut Handle),
) {
    visit(&mut value.dependency);
}

fn visit_compound(value: &mut AssocCompoundActionParam, visit: &mut impl FnMut(&mut Handle)) {
    for handle in &mut value.parameters {
        visit(handle);
    }
    if let Some(child) = value.child_parameter.as_mut() {
        visit(&mut child.parameter);
        visit(&mut child.secondary_parameter);
        visit(&mut child.tertiary_parameter);
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocDependency {
    pub class_version: i16,
    pub status: i32,
    pub is_read_dependency: bool,
    pub is_write_dependency: bool,
    pub is_attached_to_object: bool,
    pub is_delegating_to_owning_action: bool,
    pub order: i32,
    pub dependent_on: Handle,
    pub name: Option<String>,
    pub read_dependency: Handle,
    pub node: Handle,
    pub dependency_body: Handle,
    pub dependency_body_id: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocValueDependency {
    pub dependency: AssocDependency,
    pub class_version: i32,
    pub name: String,
    pub value: AssocEvalVariant,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocPersistentSubentId {
    pub class_name: String,
    pub dependent_on_compound_object: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocGeomDependency {
    pub dependency: AssocDependency,
    pub class_version: i16,
    pub enabled: bool,
    pub persistent_subent: AssocPersistentSubentId,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AssocEvalValue {
    #[default]
    None,
    Real(f64),
    Long(i32),
    Short(i16),
    Byte(u8),
    Text(String),
    Handle(Handle),
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocEvalVariant {
    /// DXF/resbuf type code describing `value`.
    pub code: i16,
    pub value: AssocEvalValue,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocValueParamVariable {
    pub value: AssocEvalVariant,
    pub handle: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocValueParam {
    pub class_version: i32,
    pub name: String,
    pub unit_type: i32,
    pub variables: Vec<AssocValueParamVariable>,
    pub controlled_object_dependency: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocActionDependency {
    pub is_owned: bool,
    pub dependency: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocAction {
    pub class_version: i16,
    pub geometry_status: i32,
    pub owning_network: Handle,
    pub action_body: Handle,
    pub action_index: i32,
    pub max_dependency_index: i32,
    pub dependencies: Vec<AssocActionDependency>,
    pub owned_parameters: Vec<Handle>,
    pub values: Vec<AssocValueParam>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocNetwork {
    pub action: AssocAction,
    pub network_version: i16,
    pub network_action_index: i32,
    pub actions: Vec<AssocActionDependency>,
    pub owned_actions: Vec<Handle>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocActionParam {
    pub is_r2013: i16,
    pub version: i32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocActionBody {
    pub version: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocParamBasedActionBody {
    pub version: i32,
    pub minor: i32,
    pub dependencies: Vec<Handle>,
    pub marker: i32,
    pub values: Vec<AssocValueParam>,
    pub empty_value_marker: i32,
    pub dependency: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocSurfaceBody {
    pub version: i32,
    pub dependency: Handle,
    pub is_semi_associative: bool,
    pub marker: i32,
    pub is_semi_override: bool,
    pub grip_status: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AssocSurfaceActionKind {
    #[default]
    Plane,
    Extend,
    Extruded,
    Lofted,
    Network,
    Offset,
    Revolved,
    Trim,
    Blend,
    Patch,
    Fillet,
    Swept,
    EdgeChamfer,
    EdgeFillet,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocSurfaceActionBody {
    pub kind: AssocSurfaceActionKind,
    pub action_body: AssocActionBody,
    pub parameter_body: AssocParamBasedActionBody,
    pub surface_body: AssocSurfaceBody,
    pub path_status: i32,
    pub class_version: i32,
    pub option: u8,
    pub flags: [bool; 5],
    pub status: i16,
    pub secondary_status: i16,
    pub distance: f64,
    pub first_point: Vector2,
    pub second_point: Vector2,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocAnnotationBase {
    pub action_body: AssocActionBody,
    pub parameter_body: AssocParamBasedActionBody,
    pub version: i16,
    pub dependency: Handle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AssocAnnotationKind {
    #[default]
    RestoreEntityState,
    MLeader,
    AlignedDimension,
    ThreePointAngularDimension,
    OrdinateDimension,
    RotatedDimension,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocAnnotationDependency {
    pub dependency_id: i32,
    pub dependency: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocAnnotationActionBody {
    pub kind: AssocAnnotationKind,
    pub annotation: AssocAnnotationBase,
    pub action_body: AssocActionBody,
    pub class_version: i32,
    pub entity: Handle,
    pub actions: Vec<AssocAnnotationDependency>,
    pub read_node: Handle,
    pub dimension_node: Handle,
    pub dependency: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocPersSubentManager {
    pub class_version: i32,
    pub markers: [i32; 3],
    pub steps: Vec<i32>,
    pub subent_count: i32,
    /// Fixed semantic tail currently documented as 34 integer slots.
    pub subent_data: Vec<i32>,
    pub final_flag: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocSingleDependencyActionParam {
    pub action_param: AssocActionParam,
    pub dependency_class_version: i32,
    pub dependency: Handle,
    pub class_version: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocViewRepActionBody {
    pub action_body: AssocActionBody,
    pub class_version: i16,
    pub view_rep: Handle,
    pub view_type: i32,
    pub rotation: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AssocViewObjectActionParamKind {
    #[default]
    ViewBorder,
    ViewRep,
    ViewSymbol,
    ViewStyle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocViewObjectActionParam {
    pub kind: AssocViewObjectActionParamKind,
    pub single_dependency: AssocSingleDependencyActionParam,
    pub class_version: i16,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocViewRepHatchManagerItem {
    pub first_id: i64,
    pub second_id: i64,
    pub status: i32,
    pub parameter: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocViewRepHatchManager {
    pub compound: AssocCompoundActionParam,
    pub class_version: i16,
    pub items: Vec<AssocViewRepHatchManagerItem>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocViewRepHatchActionParam {
    pub single_dependency: AssocSingleDependencyActionParam,
    pub class_version: i16,
    pub normal: Vector3,
    pub hatch_index: i32,
    pub flags: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocViewLabelActionParam {
    pub single_dependency: AssocSingleDependencyActionParam,
    pub class_version: i16,
    pub label_version: i16,
    pub offset: Vector2,
    pub flag: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AssocSubcurveKind {
    #[default]
    None,
    Arc,
    Ellipse,
    Line,
    LineSegment3d,
    Nurb3d,
    Curve3d,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocEdgeActionParam {
    pub single_dependency: AssocSingleDependencyActionParam,
    pub parameter: Handle,
    pub has_action: bool,
    pub action_type: i32,
    pub subcurve_kind: AssocSubcurveKind,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocConstraintNode {
    pub node_id: i32,
    pub status: u8,
    pub connections: Vec<i32>,
    pub class_name: String,
    pub registry_flag: bool,
    pub data: AssocConstraintNodeData,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AssocConstraintNodeData {
    #[default]
    None,
    Geometrical {
        owner_id: i32,
        is_implied: bool,
        is_active: bool,
    },
    Angle {
        owner_id: i32,
        is_implied: bool,
        is_active: bool,
        value_dependency: Handle,
        dimension_dependency: Handle,
        sector_type: u8,
    },
    Parallel {
        owner_id: i32,
        is_implied: bool,
        is_active: bool,
        datum_line_index: Option<i32>,
    },
    Distance {
        owner_id: i32,
        is_implied: bool,
        is_active: bool,
        value_dependency: Handle,
        dimension_dependency: Handle,
        direction_type: u8,
        distance: Option<Vector3>,
    },
    RadiusDiameter {
        owner_id: i32,
        is_implied: bool,
        is_active: bool,
        value_dependency: Handle,
        dimension_dependency: Handle,
        mode: u8,
    },
    ImplicitPoint {
        geometry_dependency: Handle,
        geometry_node_id: i32,
        point: Option<Vector3>,
        point_type: u8,
        point_index: i32,
        curve_id: i32,
    },
    Point {
        geometry_dependency: Handle,
        geometry_node_id: i32,
        point: Option<Vector3>,
    },
    Line {
        geometry_dependency: Handle,
        geometry_node_id: i32,
        point: Vector3,
        direction: Vector3,
    },
    BoundedLine {
        geometry_dependency: Handle,
        geometry_node_id: i32,
        point: Vector3,
        direction: Vector3,
        is_ray: bool,
        start_point: Vector3,
        end_point: Vector3,
    },
    Circle {
        geometry_dependency: Handle,
        geometry_node_id: i32,
        center: Vector3,
        normal: Vector3,
        direction: Vector3,
        radius: f64,
        start_parameter: f64,
        end_parameter: f64,
        reserved: f64,
    },
    Arc {
        geometry_dependency: Handle,
        geometry_node_id: i32,
        center: Vector3,
        normal: Vector3,
        direction: Vector3,
        radius: f64,
        start_parameter: f64,
        end_parameter: f64,
        reserved: f64,
        start_point: Vector3,
        end_point: Vector3,
    },
    Ellipse {
        owner_id: i32,
        is_implied: bool,
        is_active: bool,
        center: Vector3,
        short_axis: Vector3,
        axis_ratio: f64,
    },
    BoundedEllipse {
        owner_id: i32,
        is_implied: bool,
        is_active: bool,
        center: Vector3,
        short_axis: Vector3,
        axis_ratio: f64,
        start_point: Vector3,
        end_point: Vector3,
    },
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Assoc2dConstraintGroup {
    pub action: AssocAction,
    pub version: i32,
    pub flag: bool,
    pub work_plane: [Vector3; 3],
    pub dependency: Handle,
    pub actions: Vec<Handle>,
    pub nodes: Vec<AssocConstraintNode>,
}

impl Default for Assoc2dConstraintGroup {
    fn default() -> Self {
        Self {
            action: AssocAction::default(),
            version: 0,
            flag: false,
            work_plane: [Vector3::ZERO; 3],
            dependency: Handle::NULL,
            actions: Vec::new(),
            nodes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocVariable {
    pub action: AssocAction,
    pub class_version: i32,
    pub name: String,
    pub expression: String,
    pub evaluator: String,
    pub description: String,
    pub value: AssocEvalVariant,
    pub has_cached_value: bool,
    pub cached_value: String,
    pub flag: bool,
    pub reserved: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocCompoundActionParam {
    pub action_param: AssocActionParam,
    pub class_version: i16,
    pub status: i16,
    pub parameters: Vec<Handle>,
    pub child_parameter: Option<AssocChildParameter>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocChildParameter {
    pub status: i16,
    pub id: i32,
    pub parameter: Handle,
    pub secondary_parameter: Handle,
    pub marker: i32,
    pub tertiary_parameter: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocOsnapPointRefActionParam {
    pub compound: AssocCompoundActionParam,
    pub status: i16,
    pub osnap_mode: u8,
    pub parameter: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocPathActionParam {
    pub compound: AssocCompoundActionParam,
    pub version: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocDimDependencyBody {
    pub dependency_body_version: i16,
    pub base_version: i16,
    pub name: String,
    pub class_version: i16,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocFaceActionParam {
    pub single_dependency: AssocSingleDependencyActionParam,
    pub index: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocVertexActionParam {
    pub single_dependency: AssocSingleDependencyActionParam,
    pub point: Vector3,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocAsmBodyActionParam {
    pub single_dependency: AssocSingleDependencyActionParam,
    pub acis_data: AcisData,
    pub point_of_reference: Vector3,
    pub wires: Vec<Wire>,
    pub silhouettes: Vec<Silhouette>,
    pub history: Handle,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocArrayItem {
    pub class_version: i32,
    pub location: [i32; 3],
    pub flags: i32,
    pub uses_default_transform: bool,
    pub x_direction: Vector3,
    pub transform: [f64; 16],
    pub relative_transform: Option<[f64; 16]>,
    pub first_handle: Option<Handle>,
    pub second_handle: Option<Handle>,
}

impl Default for AssocArrayItem {
    fn default() -> Self {
        Self {
            class_version: 0,
            location: [0; 3],
            flags: 0,
            uses_default_transform: false,
            x_direction: Vector3::ZERO,
            transform: [0.0; 16],
            relative_transform: None,
            first_handle: None,
            second_handle: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocArrayParameters {
    pub version: i32,
    pub class_name: String,
    pub items: Vec<AssocArrayItem>,
    pub item_count: i32,
    pub row_count: i32,
    pub level_count: i32,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocArrayActionBody {
    pub action_body: AssocActionBody,
    pub parameter_body: AssocParamBasedActionBody,
    pub version: i32,
    pub parameter_block: String,
    pub transform: [f64; 16],
}

impl Default for AssocArrayActionBody {
    fn default() -> Self {
        Self {
            action_body: AssocActionBody::default(),
            parameter_body: AssocParamBasedActionBody::default(),
            version: 0,
            parameter_block: String::new(),
            transform: [0.0; 16],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocArrayModifyActionBody {
    pub body: AssocArrayActionBody,
    pub status: i16,
    pub item_locations: Vec<[i32; 3]>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocDimensionReference {
    pub class_name: String,
    pub osnap_type: u8,
    pub xrefs: Vec<Handle>,
    pub main_subent_type: i32,
    pub main_gs_marker: i32,
    pub xref_paths: Vec<String>,
    pub osnap_distance: f64,
    pub osnap_point: Vector3,
    pub intersection_objects: Vec<Handle>,
    pub intersection_subent_type: i32,
    pub intersection_gs_marker: i32,
    pub intersection_xref_paths: Vec<String>,
    pub has_last_point_reference: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AssocDimensionAssociation {
    pub associativity: i32,
    pub trans_space: bool,
    pub rotated_type: u8,
    pub dimension: Handle,
    /// Four associativity slots. Each active slot contains one or more chained
    /// `AcDbOsnapPointRef` records; the on-disk continuation bit links records
    /// within the same slot.
    pub references: [Vec<AssocDimensionReference>; 4],
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PersSubentManager {
    pub class_version: i32,
    pub marker_zero: i32,
    pub marker_two: i32,
    pub associative_step_count: i32,
    pub associative_subent_count: i32,
    pub steps: Vec<i32>,
    pub subents: Vec<i32>,
}

pub fn associative_canonical_name(name: &str) -> String {
    let upper = name.to_ascii_uppercase();
    let canonical = if let Some(rest) = upper.strip_prefix("ACDBASSOC") {
        format!("ASSOC{rest}")
    } else {
        upper
    };
    match canonical.as_str() {
        // Seen in Autodesk/LibreDWG class tables without the second `D`.
        "ASSOCALIGNEDIMACTIONBODY" => "ASSOCALIGNEDDIMACTIONBODY".to_string(),
        "ACDBPERSSUBENTMANAGER" => "PERSUBENTMGR".to_string(),
        "ACDBDIMASSOC" => "DIMASSOC".to_string(),
        _ => canonical,
    }
}

pub fn is_associative_object_name(name: &str) -> bool {
    matches!(
        associative_canonical_name(name).as_str(),
        "ASSOCDEPENDENCY"
            | "ASSOCPLANESURFACEACTIONBODY"
            | "ASSOCEXTENDSURFACEACTIONBODY"
            | "ASSOCEXTRUDEDSURFACEACTIONBODY"
            | "ASSOCLOFTEDSURFACEACTIONBODY"
            | "ASSOCNETWORKSURFACEACTIONBODY"
            | "ASSOCOFFSETSURFACEACTIONBODY"
            | "ASSOCREVOLVEDSURFACEACTIONBODY"
            | "ASSOCTRIMSURFACEACTIONBODY"
            | "ASSOCBLENDSURFACEACTIONBODY"
            | "ASSOCPATCHSURFACEACTIONBODY"
            | "ASSOCFILLETSURFACEACTIONBODY"
            | "ASSOCACTION"
            | "ASSOCVALUEDEPENDENCY"
            | "ASSOCGEOMDEPENDENCY"
            | "ASSOCNETWORK"
            | "ASSOCSWEPTSURFACEACTIONBODY"
            | "ASSOCEDGECHAMFERACTIONBODY"
            | "ASSOCEDGEFILLETACTIONBODY"
            | "ASSOCRESTOREENTITYSTATEACTIONBODY"
            | "ASSOCMLEADERACTIONBODY"
            | "ASSOCALIGNEDDIMACTIONBODY"
            | "ASSOC3POINTANGULARDIMACTIONBODY"
            | "ASSOCORDINATEDIMACTIONBODY"
            | "ASSOCROTATEDDIMACTIONBODY"
            | "ASSOCPERSSUBENTMANAGER"
            | "ASSOCEDGEACTIONPARAM"
            | "ASSOC2DCONSTRAINTGROUP"
            | "ASSOCVARIABLE"
            | "ASSOCACTIONPARAM"
            | "ASSOCCOMPOUNDACTIONPARAM"
            | "ASSOCOSNAPPOINTREFACTIONPARAM"
            | "ASSOCPOINTREFACTIONPARAM"
            | "ASSOCOBJECTACTIONPARAM"
            | "ASSOCPATHACTIONPARAM"
            | "ASSOCDIMDEPENDENCYBODY"
            | "ASSOCFACEACTIONPARAM"
            | "ASSOCVERTEXACTIONPARAM"
            | "ASSOCASMBODYACTIONPARAM"
            | "ASSOCARRAYMODIFYPARAMETERS"
            | "ASSOCARRAYPATHPARAMETERS"
            | "ASSOCARRAYPOLARPARAMETERS"
            | "ASSOCARRAYRECTANGULARPARAMETERS"
            | "ASSOCARRAYACTIONBODY"
            | "ASSOCARRAYMODIFYACTIONBODY"
            | "DIMASSOC"
            | "PERSUBENTMGR"
            | "ASSOCVIEWREPACTIONBODY"
            | "ASSOCVIEWBORDERACTIONPARAM"
            | "ASSOCVIEWREPHATCHMANAGER"
            | "ASSOCVIEWREPACTIONPARAM"
            | "ASSOCVIEWREPHATCHACTIONPARAM"
            | "ASSOCVIEWSYMBOLACTIONPARAM"
            | "ASSOCVIEWSTYLEACTIONPARAM"
            | "ASSOCVIEWLABELACTIONPARAM"
    )
}

pub fn associative_cpp_class_name(name: &str) -> Option<&'static str> {
    Some(match associative_canonical_name(name).as_str() {
        "ASSOCDEPENDENCY" => "AcDbAssocDependency",
        "ASSOCPLANESURFACEACTIONBODY" => "AcDbAssocPlaneSurfaceActionBody",
        "ASSOCEXTENDSURFACEACTIONBODY" => "AcDbAssocExtendSurfaceActionBody",
        "ASSOCEXTRUDEDSURFACEACTIONBODY" => "AcDbAssocExtrudedSurfaceActionBody",
        "ASSOCLOFTEDSURFACEACTIONBODY" => "AcDbAssocLoftedSurfaceActionBody",
        "ASSOCNETWORKSURFACEACTIONBODY" => "AcDbAssocNetworkSurfaceActionBody",
        "ASSOCOFFSETSURFACEACTIONBODY" => "AcDbAssocOffsetSurfaceActionBody",
        "ASSOCREVOLVEDSURFACEACTIONBODY" => "AcDbAssocRevolvedSurfaceActionBody",
        "ASSOCTRIMSURFACEACTIONBODY" => "AcDbAssocTrimSurfaceActionBody",
        "ASSOCBLENDSURFACEACTIONBODY" => "AcDbAssocBlendSurfaceActionBody",
        "ASSOCPATCHSURFACEACTIONBODY" => "AcDbAssocPatchSurfaceActionBody",
        "ASSOCFILLETSURFACEACTIONBODY" => "AcDbAssocFilletSurfaceActionBody",
        "ASSOCACTION" => "AcDbAssocAction",
        "ASSOCVALUEDEPENDENCY" => "AcDbAssocValueDependency",
        "ASSOCGEOMDEPENDENCY" => "AcDbAssocGeomDependency",
        "ASSOCNETWORK" => "AcDbAssocNetwork",
        "ASSOCSWEPTSURFACEACTIONBODY" => "AcDbAssocSweptSurfaceActionBody",
        "ASSOCEDGECHAMFERACTIONBODY" => "AcDbAssocEdgeChamferActionBody",
        "ASSOCEDGEFILLETACTIONBODY" => "AcDbAssocEdgeFilletActionBody",
        "ASSOCRESTOREENTITYSTATEACTIONBODY" => "AcDbAssocRestoreEntityStateActionBody",
        "ASSOCMLEADERACTIONBODY" => "AcDbAssocMLeaderActionBody",
        "ASSOCALIGNEDDIMACTIONBODY" => "AcDbAssocAlignedDimActionBody",
        "ASSOC3POINTANGULARDIMACTIONBODY" => "AcDbAssoc3PointAngularDimActionBody",
        "ASSOCORDINATEDIMACTIONBODY" => "AcDbAssocOrdinateDimActionBody",
        "ASSOCROTATEDDIMACTIONBODY" => "AcDbAssocRotatedDimActionBody",
        "ASSOCPERSSUBENTMANAGER" => "AcDbAssocPersSubentManager",
        "ASSOCEDGEACTIONPARAM" => "AcDbAssocEdgeActionParam",
        "ASSOC2DCONSTRAINTGROUP" => "AcDbAssoc2dConstraintGroup",
        "ASSOCVARIABLE" => "AcDbAssocVariable",
        "ASSOCACTIONPARAM" => "AcDbAssocActionParam",
        "ASSOCCOMPOUNDACTIONPARAM" => "AcDbAssocCompoundActionParam",
        "ASSOCOSNAPPOINTREFACTIONPARAM" => "AcDbAssocOsnapPointRefActionParam",
        "ASSOCPOINTREFACTIONPARAM" => "AcDbAssocPointRefActionParam",
        "ASSOCOBJECTACTIONPARAM" => "AcDbAssocObjectActionParam",
        "ASSOCPATHACTIONPARAM" => "AcDbAssocPathActionParam",
        "ASSOCDIMDEPENDENCYBODY" => "AcDbAssocDimDependencyBody",
        "ASSOCFACEACTIONPARAM" => "AcDbAssocFaceActionParam",
        "ASSOCVERTEXACTIONPARAM" => "AcDbAssocVertexActionParam",
        "ASSOCASMBODYACTIONPARAM" => "AcDbAssocAsmbodyActionParam",
        "ASSOCARRAYMODIFYPARAMETERS" => "AcDbAssocArrayModifyParameters",
        "ASSOCARRAYPATHPARAMETERS" => "AcDbAssocArrayPathParameters",
        "ASSOCARRAYPOLARPARAMETERS" => "AcDbAssocArrayPolarParameters",
        "ASSOCARRAYRECTANGULARPARAMETERS" => "AcDbAssocArrayRectangularParameters",
        "ASSOCARRAYACTIONBODY" => "AcDbAssocArrayActionBody",
        "ASSOCARRAYMODIFYACTIONBODY" => "AcDbAssocArrayModifyActionBody",
        "DIMASSOC" => "AcDbDimAssoc",
        "PERSUBENTMGR" => "AcDbPersSubentManager",
        "ASSOCVIEWREPACTIONBODY" => "AcDbAssocViewRepActionBody",
        "ASSOCVIEWBORDERACTIONPARAM" => "AcDbAssocViewBorderActionParam",
        "ASSOCVIEWREPHATCHMANAGER" => "AcDbAssocViewRepHatchManager",
        "ASSOCVIEWREPACTIONPARAM" => "AcDbAssocViewRepActionParam",
        "ASSOCVIEWREPHATCHACTIONPARAM" => "AcDbAssocViewRepHatchActionParam",
        "ASSOCVIEWSYMBOLACTIONPARAM" => "AcDbAssocViewSymbolActionParam",
        "ASSOCVIEWSTYLEACTIONPARAM" => "AcDbAssocViewStyleActionParam",
        "ASSOCVIEWLABELACTIONPARAM" => "AcDbAssocViewLabelActionParam",
        _ => return None,
    })
}
