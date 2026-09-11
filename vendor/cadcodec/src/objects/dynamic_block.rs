//! Semantic data model for dynamic-block parameters, grips, actions and
//! evaluation/history records.
//!
//! The types in this module mirror the inheritance layers used by the DWG
//! object stream. They intentionally contain no reader/writer policy.

use crate::entities::AcisData;
use crate::types::{Color, Handle, Vector2, Vector3};

use super::BlockEvalValue;
use super::BlockVisibilityParameter;

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DynamicBlockObject {
    pub handle: Handle,
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    pub dxf_name: String,
    pub cpp_class_name: String,
    pub data: DynamicBlockData,
}

impl DynamicBlockObject {
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
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DynamicBlockData {
    #[default]
    Unknown,
    Representation(BlockRepresentationData),
    ProxyNode(BlockEvalExpression),
    GripLocationComponent(BlockGripExpression),
    AlignmentGrip(BlockOrientedGrip),
    FlipGrip(BlockFlipGrip),
    LinearGrip(BlockOrientedGrip),
    LookupGrip(BlockGrip),
    PolarGrip(BlockGrip),
    RotationGrip(BlockGrip),
    VisibilityGrip(BlockGrip),
    XYGrip(BlockGrip),
    PropertiesTableGrip(BlockGrip),
    AlignmentParameter(BlockAlignmentParameter),
    BasePointParameter(BlockBasePointParameter),
    FlipParameter(BlockFlipParameter),
    LinearParameter(BlockLinearParameter),
    LookupParameter(BlockLookupParameter),
    PointParameter(BlockPointParameter),
    PolarParameter(BlockPolarParameter),
    RotationParameter(BlockRotationParameter),
    UserParameter(BlockUserParameter),
    VisibilityParameter(BlockVisibilityParameter),
    XYParameter(BlockXYParameter),
    AngularConstraintParameter(BlockAngularConstraintParameter),
    DiametricConstraintParameter(BlockDistanceConstraintParameter),
    RadialConstraintParameter(BlockDistanceConstraintParameter),
    AlignedConstraintParameter(BlockLinearConstraintParameter),
    LinearConstraintParameter(BlockLinearConstraintParameter),
    HorizontalConstraintParameter(BlockLinearConstraintParameter),
    VerticalConstraintParameter(BlockLinearConstraintParameter),
    ParameterDependencyBody(BlockParameterDependencyBody),
    MoveAction(BlockMoveAction),
    FlipAction(BlockFlipAction),
    RotateAction(BlockBasePointAction),
    ScaleAction(BlockBasePointAction),
    ArrayAction(BlockArrayAction),
    LookupAction(BlockLookupAction),
    StretchAction(BlockStretchAction),
    PolarStretchAction(BlockPolarStretchAction),
    PropertiesTable,
    AlignmentParameterEntity,
    BasePointParameterEntity,
    FlipParameterEntity,
    LinearParameterEntity,
    PointParameterEntity,
    RotationParameterEntity,
    VisibilityParameterEntity,
    XYParameterEntity,
    AngularConstraintParameterEntity(BlockAngularConstraintParameterEntity),
    FlipGripEntity,
    LinearGripEntity,
    PolarGripEntity,
    RotationGripEntity,
    VisibilityGripEntity,
    XYGripEntity,
    EvaluationGraph(BlockEvaluationGraph),
    SolidHistory(SolidHistory),
    SolidHistoryNode(SolidHistoryOperation),
}

impl DynamicBlockData {
    pub fn empty_entity_from_dxf_name(name: &str) -> Option<Self> {
        match name.to_ascii_uppercase().as_str() {
            "ALIGNMENTPARAMETERENTITY" => Some(Self::AlignmentParameterEntity),
            "BASEPOINTPARAMETERENTITY" => Some(Self::BasePointParameterEntity),
            "FLIPPARAMETERENTITY" => Some(Self::FlipParameterEntity),
            "LINEARPARAMETERENTITY" => Some(Self::LinearParameterEntity),
            "POINTPARAMETERENTITY" => Some(Self::PointParameterEntity),
            "ROTATIONPARAMETERENTITY" => Some(Self::RotationParameterEntity),
            "VISIBILITYPARAMETERENTITY" => Some(Self::VisibilityParameterEntity),
            "XYPARAMETERENTITY" => Some(Self::XYParameterEntity),
            "FLIPGRIPENTITY" => Some(Self::FlipGripEntity),
            "LINEARGRIPENTITY" => Some(Self::LinearGripEntity),
            "POLARGRIPENTITY" => Some(Self::PolarGripEntity),
            "ROTATIONGRIPENTITY" => Some(Self::RotationGripEntity),
            "VISIBILITYGRIPENTITY" => Some(Self::VisibilityGripEntity),
            "XYGRIPENTITY" => Some(Self::XYGripEntity),
            _ => None,
        }
    }

    pub fn entity_dxf_name(&self) -> Option<&'static str> {
        match self {
            Self::AlignmentParameterEntity => Some("ALIGNMENTPARAMETERENTITY"),
            Self::BasePointParameterEntity => Some("BASEPOINTPARAMETERENTITY"),
            Self::FlipParameterEntity => Some("FLIPPARAMETERENTITY"),
            Self::LinearParameterEntity => Some("LINEARPARAMETERENTITY"),
            Self::PointParameterEntity => Some("POINTPARAMETERENTITY"),
            Self::RotationParameterEntity => Some("ROTATIONPARAMETERENTITY"),
            Self::VisibilityParameterEntity => Some("VISIBILITYPARAMETERENTITY"),
            Self::XYParameterEntity => Some("XYPARAMETERENTITY"),
            Self::AngularConstraintParameterEntity(_) => {
                Some("BLOCKANGULARCONSTRAINTPARAMETERENTITY")
            }
            Self::FlipGripEntity => Some("FLIPGRIPENTITY"),
            Self::LinearGripEntity => Some("LINEARGRIPENTITY"),
            Self::PolarGripEntity => Some("POLARGRIPENTITY"),
            Self::RotationGripEntity => Some("ROTATIONGRIPENTITY"),
            Self::VisibilityGripEntity => Some("VISIBILITYGRIPENTITY"),
            Self::XYGripEntity => Some("XYGRIPENTITY"),
            _ => None,
        }
    }

    pub fn entity_cpp_name(&self) -> Option<&'static str> {
        match self {
            Self::AlignmentParameterEntity => Some("AcDbBlockAlignmentParameterEntity"),
            Self::BasePointParameterEntity => Some("AcDbBlockBasepointParameterEntity"),
            Self::FlipParameterEntity => Some("AcDbBlockFlipParameterEntity"),
            Self::LinearParameterEntity => Some("AcDbBlockLinearParameterEntity"),
            Self::PointParameterEntity => Some("AcDbBlockPointParameterEntity"),
            Self::RotationParameterEntity => Some("AcDbBlockRotationParameterEntity"),
            Self::VisibilityParameterEntity => Some("AcDbBlockVisibilityParameterEntity"),
            Self::XYParameterEntity => Some("AcDbBlockXYParameterEntity"),
            Self::AngularConstraintParameterEntity(_) => {
                Some("AcDbBlockAngularConstraintParameterEntity")
            }
            Self::FlipGripEntity => Some("AcDbBlockFlipGripEntity"),
            Self::LinearGripEntity => Some("AcDbBlockLinearGripEntity"),
            Self::PolarGripEntity => Some("AcDbBlockPolarGripEntity"),
            Self::RotationGripEntity => Some("AcDbBlockRotationGripEntity"),
            Self::VisibilityGripEntity => Some("AcDbBlockVisibilityGripEntity"),
            Self::XYGripEntity => Some("AcDbBlockXYGripEntity"),
            _ => None,
        }
    }

    pub(crate) fn visit_handles_mut(&mut self, visit: &mut impl FnMut(&mut Handle)) {
        match self {
            Self::Unknown
            | Self::PropertiesTable
            | Self::AlignmentParameterEntity
            | Self::BasePointParameterEntity
            | Self::FlipParameterEntity
            | Self::LinearParameterEntity
            | Self::PointParameterEntity
            | Self::RotationParameterEntity
            | Self::VisibilityParameterEntity
            | Self::XYParameterEntity
            | Self::FlipGripEntity
            | Self::LinearGripEntity
            | Self::PolarGripEntity
            | Self::RotationGripEntity
            | Self::VisibilityGripEntity
            | Self::XYGripEntity
            | Self::ParameterDependencyBody(_) => {}
            Self::Representation(value) => visit(&mut value.block),
            Self::ProxyNode(value) => visit_block_eval(value, visit),
            Self::GripLocationComponent(value) => {
                visit_block_eval(&mut value.eval, visit);
            }
            Self::AlignmentGrip(value) | Self::LinearGrip(value) => {
                visit_block_grip(&mut value.grip, visit);
            }
            Self::FlipGrip(value) => {
                visit_block_grip(&mut value.grip, visit);
            }
            Self::LookupGrip(value)
            | Self::PolarGrip(value)
            | Self::RotationGrip(value)
            | Self::VisibilityGrip(value)
            | Self::XYGrip(value)
            | Self::PropertiesTableGrip(value) => {
                visit_block_grip(value, visit);
            }
            Self::AlignmentParameter(value) => {
                visit_two_point_parameter(&mut value.parameter, visit);
            }
            Self::BasePointParameter(value) => {
                visit_one_point_parameter(&mut value.parameter, visit);
            }
            Self::FlipParameter(value) => {
                visit_two_point_parameter(&mut value.parameter, visit);
            }
            Self::LinearParameter(value) => {
                visit_two_point_parameter(&mut value.parameter, visit);
            }
            Self::LookupParameter(value) => {
                visit_one_point_parameter(&mut value.parameter, visit);
            }
            Self::PointParameter(value) => {
                visit_one_point_parameter(&mut value.parameter, visit);
            }
            Self::PolarParameter(value) => {
                visit_two_point_parameter(&mut value.parameter, visit);
            }
            Self::RotationParameter(value) => {
                visit_two_point_parameter(&mut value.parameter, visit);
            }
            Self::UserParameter(value) => {
                visit_one_point_parameter(&mut value.parameter, visit);
                visit(&mut value.associated_variable);
                visit_block_value(&mut value.value, visit);
            }
            Self::VisibilityParameter(value) => {
                value.visit_handles_mut(visit);
            }
            Self::XYParameter(value) => {
                visit_two_point_parameter(&mut value.parameter, visit);
            }
            Self::AngularConstraintParameter(value) => {
                visit_constraint(&mut value.constraint, visit);
            }
            Self::DiametricConstraintParameter(value) | Self::RadialConstraintParameter(value) => {
                visit_constraint(&mut value.constraint, visit);
            }
            Self::AlignedConstraintParameter(value)
            | Self::LinearConstraintParameter(value)
            | Self::HorizontalConstraintParameter(value)
            | Self::VerticalConstraintParameter(value) => {
                visit_constraint(&mut value.constraint, visit);
            }
            Self::MoveAction(value) => {
                visit_block_action(&mut value.action, visit);
            }
            Self::FlipAction(value) => {
                visit_block_action(&mut value.action, visit);
            }
            Self::RotateAction(value) | Self::ScaleAction(value) => {
                visit_action_with_base_point(&mut value.action, visit);
            }
            Self::ArrayAction(value) => {
                visit_block_action(&mut value.action, visit);
            }
            Self::LookupAction(value) => {
                visit_block_action(&mut value.action, visit);
            }
            Self::StretchAction(value) => {
                visit_block_action(&mut value.action, visit);
                for item in &mut value.handles {
                    visit(&mut item.handle);
                }
            }
            Self::PolarStretchAction(value) => {
                visit_block_action(&mut value.action, visit);
                for handle in &mut value.handles {
                    visit(handle);
                }
            }
            Self::AngularConstraintParameterEntity(value) => {
                visit_constraint(&mut value.constraint, visit);
            }
            Self::EvaluationGraph(value) => {
                for node in &mut value.nodes {
                    visit(&mut node.expression);
                }
            }
            Self::SolidHistory(value) => visit(&mut value.owner),
            Self::SolidHistoryNode(value) => {
                visit_solid_history_operation(value, visit);
            }
        }
    }
}

fn visit_block_value(value: &mut BlockEvalValue, visit: &mut impl FnMut(&mut Handle)) {
    if let BlockEvalValue::Handle(handle) = value {
        visit(handle);
    }
}

fn visit_block_eval(value: &mut BlockEvalExpression, visit: &mut impl FnMut(&mut Handle)) {
    visit_block_value(&mut value.value, visit);
}

fn visit_block_element(value: &mut BlockElement, visit: &mut impl FnMut(&mut Handle)) {
    visit_block_eval(&mut value.eval, visit);
}

fn visit_one_point_parameter(
    value: &mut BlockOnePointParameter,
    visit: &mut impl FnMut(&mut Handle),
) {
    visit_block_element(&mut value.parameter.element, visit);
}

fn visit_two_point_parameter(
    value: &mut BlockTwoPointParameter,
    visit: &mut impl FnMut(&mut Handle),
) {
    visit_block_element(&mut value.parameter.element, visit);
}

fn visit_block_grip(value: &mut BlockGrip, visit: &mut impl FnMut(&mut Handle)) {
    visit_block_element(&mut value.element, visit);
}

fn visit_block_action(value: &mut BlockAction, visit: &mut impl FnMut(&mut Handle)) {
    visit_block_element(&mut value.element, visit);
    for handle in &mut value.dependencies {
        visit(handle);
    }
}

fn visit_action_with_base_point(
    value: &mut BlockActionWithBasePoint,
    visit: &mut impl FnMut(&mut Handle),
) {
    visit_block_action(&mut value.action, visit);
}

fn visit_constraint(value: &mut BlockConstraintParameter, visit: &mut impl FnMut(&mut Handle)) {
    visit_two_point_parameter(&mut value.parameter, visit);
    visit(&mut value.dependency);
}

fn visit_solid_history_base(value: &mut SolidHistoryNodeBase, visit: &mut impl FnMut(&mut Handle)) {
    visit_block_eval(&mut value.eval, visit);
    visit(&mut value.material);
}

fn visit_solid_history_operation(
    value: &mut SolidHistoryOperation,
    visit: &mut impl FnMut(&mut Handle),
) {
    match value {
        SolidHistoryOperation::Unknown => {}
        SolidHistoryOperation::Box(value) | SolidHistoryOperation::Wedge(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Sphere(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Cylinder(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Cone(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Pyramid(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Torus(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Boolean(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Brep(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Fillet(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Chamfer(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Sweep(value) | SolidHistoryOperation::Extrusion(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Loft(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
        SolidHistoryOperation::Revolve(value) => {
            visit_solid_history_base(&mut value.base, visit);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockEvalExpression {
    pub parent_id: i32,
    pub major: i32,
    pub minor: i32,
    pub value_code: i16,
    pub value: BlockEvalValue,
    pub node_id: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockElement {
    pub eval: BlockEvalExpression,
    pub name: String,
    pub major: i32,
    pub minor: i32,
    pub eed_1071: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockConnection {
    pub code: i32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockParameterProperty {
    pub connections: Vec<BlockConnection>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockParameterValueSet {
    pub description: String,
    pub flags: i32,
    pub minimum: f64,
    pub maximum: f64,
    pub increment: f64,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockParameter {
    pub element: BlockElement,
    pub show_properties: bool,
    pub chain_actions: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockOnePointParameter {
    pub parameter: BlockParameter,
    pub definition_point: Vector3,
    pub properties: [BlockParameterProperty; 2],
    pub property_count: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockTwoPointParameter {
    pub parameter: BlockParameter,
    pub definition_base_point: Vector3,
    pub definition_end_point: Vector3,
    pub properties: [BlockParameterProperty; 4],
    pub property_states: [i32; 4],
    pub parameter_base_location: i16,
    pub updated_base_point: Option<Vector3>,
    pub base_point: Option<Vector3>,
    pub updated_end_point: Option<Vector3>,
    pub end_point: Option<Vector3>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockGrip {
    pub element: BlockElement,
    pub flags_91: i32,
    pub flags_92: i32,
    pub location: Vector3,
    pub insert_cycling: bool,
    pub insert_cycling_weight: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockGripExpression {
    pub eval: BlockEvalExpression,
    pub grip_type: i32,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockOrientedGrip {
    pub grip: BlockGrip,
    pub orientation: Vector3,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockFlipGrip {
    pub grip: BlockGrip,
    pub combined_state: i32,
    pub orientation: Vector3,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockAction {
    pub element: BlockElement,
    pub display_location: Vector3,
    pub dependencies: Vec<Handle>,
    pub action_ids: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockActionOffsets {
    pub offset_x: f64,
    pub offset_y: f64,
    pub angle_offset: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockActionWithBasePoint {
    pub action: BlockAction,
    pub offset: Vector3,
    pub connections: Vec<BlockConnection>,
    pub dependent: bool,
    pub base_point: Vector3,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockConstraintParameter {
    pub parameter: BlockTwoPointParameter,
    pub dependency: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockLinearConstraintParameter {
    pub constraint: BlockConstraintParameter,
    pub expression_name: String,
    pub expression_description: String,
    pub value: f64,
    pub value_set: BlockParameterValueSet,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockRepresentationData {
    pub flags: i16,
    pub block: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockAlignmentParameter {
    pub parameter: BlockTwoPointParameter,
    pub align_perpendicular: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockLinearParameter {
    pub parameter: BlockTwoPointParameter,
    pub distance_name: String,
    pub distance_description: String,
    pub distance: f64,
    pub value_set: BlockParameterValueSet,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockBasePointParameter {
    pub parameter: BlockOnePointParameter,
    pub point: Vector3,
    pub base_point: Vector3,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockFlipParameter {
    pub parameter: BlockTwoPointParameter,
    pub flip_label: String,
    pub flip_label_description: String,
    pub base_state_label: String,
    pub flipped_state_label: String,
    pub definition_label_point: Vector3,
    pub flags_96: i32,
    pub tooltip: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockXYParameter {
    pub parameter: BlockTwoPointParameter,
    pub x_label: String,
    pub x_label_description: String,
    pub y_label: String,
    pub y_label_description: String,
    pub x_value: f64,
    pub y_value: f64,
    pub x_value_set: BlockParameterValueSet,
    pub y_value_set: BlockParameterValueSet,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockRotationParameter {
    pub parameter: BlockTwoPointParameter,
    pub definition_base_angle_point: Vector3,
    pub angle_name: String,
    pub angle_description: String,
    pub angle: f64,
    pub value_set: BlockParameterValueSet,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockPolarParameter {
    pub parameter: BlockTwoPointParameter,
    pub angle_name: String,
    pub angle_description: String,
    pub distance_name: String,
    pub distance_description: String,
    pub offset: f64,
    pub angle_value_set: BlockParameterValueSet,
    pub distance_value_set: BlockParameterValueSet,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockLookupParameter {
    pub parameter: BlockOnePointParameter,
    pub index: i32,
    pub lookup_name: String,
    pub lookup_description: String,
    pub unknown_text: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockPointParameter {
    pub parameter: BlockOnePointParameter,
    pub position_name: String,
    pub position_description: String,
    pub definition_label_point: Vector3,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockUserParameter {
    pub parameter: BlockOnePointParameter,
    pub flags: i16,
    pub associated_variable: Handle,
    pub expression: String,
    pub value_code: i16,
    pub value: BlockEvalValue,
    pub value_type: i16,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockAngularConstraintParameter {
    pub constraint: BlockConstraintParameter,
    pub center_point: Vector3,
    pub end_point: Vector3,
    pub expression_name: String,
    pub expression_description: String,
    pub angle: f64,
    pub orientation_on_both_grips: bool,
    pub value_set: BlockParameterValueSet,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockAngularConstraintParameterEntity {
    pub constraint: BlockConstraintParameter,
    pub center_point: Vector3,
    pub label_point: Vector3,
    pub expression_name: String,
    pub expression_description: String,
    pub angle: f64,
    pub orientation_on_both_grips: bool,
    pub value_set: BlockParameterValueSet,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockDistanceConstraintParameter {
    pub constraint: BlockConstraintParameter,
    pub expression_name: String,
    pub expression_description: String,
    pub distance: f64,
    pub value_set: BlockParameterValueSet,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockParameterDependencyBody {
    pub dependency_body_version: i16,
    pub dimension_base_version: i16,
    pub name: String,
    pub class_version: i16,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockMoveAction {
    pub action: BlockAction,
    pub connections: [BlockConnection; 2],
    pub offsets: BlockActionOffsets,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockFlipAction {
    pub action: BlockAction,
    pub connections: [BlockConnection; 4],
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockBasePointAction {
    pub action: BlockActionWithBasePoint,
    pub connections: Vec<BlockConnection>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockArrayAction {
    pub action: BlockAction,
    pub connections: [BlockConnection; 4],
    pub column_offset: f64,
    pub row_offset: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockLookupRow {
    pub connections: [BlockConnection; 3],
    pub flag_282: bool,
    pub flag_281: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockLookupAction {
    pub action: BlockAction,
    pub row_count: i32,
    pub column_count: i32,
    pub expressions: Vec<String>,
    pub rows: Vec<BlockLookupRow>,
    pub flag_280: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockStretchHandle {
    pub handle: Handle,
    pub indexes: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockStretchCode {
    pub code: i32,
    pub indexes: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockStretchAction {
    pub action: BlockAction,
    pub connections: [BlockConnection; 2],
    pub points: Vec<Vector2>,
    pub handles: Vec<BlockStretchHandle>,
    pub codes: Vec<BlockStretchCode>,
    pub offsets: BlockActionOffsets,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockPolarStretchAction {
    pub action: BlockAction,
    pub connections: [BlockConnection; 6],
    pub points: Vec<Vector2>,
    pub handles: Vec<Handle>,
    pub handle_flags: Vec<i16>,
    pub codes: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockEvaluationNode {
    pub id: i32,
    pub edge_flags: i32,
    pub next_id: i32,
    pub expression: Handle,
    pub node_data: [i32; 4],
    pub active_cycles: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockEvaluationEdge {
    pub id: i32,
    pub next_id: i32,
    pub incoming_edge: i32,
    pub source_node: i32,
    pub destination_node: i32,
    pub outgoing_edges: [i32; 5],
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockEvaluationGraph {
    pub first_node_id: i32,
    pub first_node_id_copy: i32,
    pub nodes: Vec<BlockEvaluationNode>,
    pub edges: Vec<BlockEvaluationEdge>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistory {
    pub major: i32,
    pub minor: i32,
    pub owner: Handle,
    pub history_node_id: i32,
    pub show_history: bool,
    pub record_history: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryNodeBase {
    pub eval: BlockEvalExpression,
    pub major: i32,
    pub minor: i32,
    pub transform: [f64; 16],
    pub color: Color,
    pub step_id: i32,
    pub material: Handle,
}

impl SolidHistoryNodeBase {
    pub fn new(step_id: i32) -> Self {
        Self {
            eval: BlockEvalExpression {
                major: 1,
                node_id: step_id,
                ..BlockEvalExpression::default()
            },
            major: 1,
            transform: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            step_id,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SolidHistoryOperation {
    #[default]
    Unknown,
    Box(SolidHistoryBox),
    Wedge(SolidHistoryBox),
    Sphere(SolidHistorySphere),
    Cylinder(SolidHistoryCylinder),
    Cone(SolidHistoryCone),
    Pyramid(SolidHistoryPyramid),
    Torus(SolidHistoryTorus),
    Boolean(SolidHistoryBoolean),
    Brep(SolidHistoryBrep),
    Fillet(SolidHistoryFillet),
    Chamfer(SolidHistoryChamfer),
    Sweep(SolidHistorySweep),
    Extrusion(SolidHistorySweep),
    Loft(SolidHistoryLoft),
    Revolve(SolidHistoryRevolve),
}

impl SolidHistoryOperation {
    pub fn base(&self) -> Option<&SolidHistoryNodeBase> {
        match self {
            Self::Unknown => None,
            Self::Box(value) | Self::Wedge(value) => Some(&value.base),
            Self::Sphere(value) => Some(&value.base),
            Self::Cylinder(value) => Some(&value.base),
            Self::Cone(value) => Some(&value.base),
            Self::Pyramid(value) => Some(&value.base),
            Self::Torus(value) => Some(&value.base),
            Self::Boolean(value) => Some(&value.base),
            Self::Brep(value) => Some(&value.base),
            Self::Fillet(value) => Some(&value.base),
            Self::Chamfer(value) => Some(&value.base),
            Self::Sweep(value) | Self::Extrusion(value) => Some(&value.base),
            Self::Loft(value) => Some(&value.base),
            Self::Revolve(value) => Some(&value.base),
        }
    }

    pub fn base_mut(&mut self) -> Option<&mut SolidHistoryNodeBase> {
        match self {
            Self::Unknown => None,
            Self::Box(value) | Self::Wedge(value) => Some(&mut value.base),
            Self::Sphere(value) => Some(&mut value.base),
            Self::Cylinder(value) => Some(&mut value.base),
            Self::Cone(value) => Some(&mut value.base),
            Self::Pyramid(value) => Some(&mut value.base),
            Self::Torus(value) => Some(&mut value.base),
            Self::Boolean(value) => Some(&mut value.base),
            Self::Brep(value) => Some(&mut value.base),
            Self::Fillet(value) => Some(&mut value.base),
            Self::Chamfer(value) => Some(&mut value.base),
            Self::Sweep(value) | Self::Extrusion(value) => Some(&mut value.base),
            Self::Loft(value) => Some(&mut value.base),
            Self::Revolve(value) => Some(&mut value.base),
        }
    }

    pub fn class_names(&self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            Self::Unknown => return None,
            Self::Box(_) => ("ACSH_BOX_CLASS", "AcDbShBox"),
            Self::Wedge(_) => ("ACSH_WEDGE_CLASS", "AcDbShWedge"),
            Self::Sphere(_) => ("ACSH_SPHERE_CLASS", "AcDbShSphere"),
            Self::Cylinder(_) => ("ACSH_CYLINDER_CLASS", "AcDbShCylinder"),
            Self::Cone(_) => ("ACSH_CONE_CLASS", "AcDbShCone"),
            Self::Pyramid(_) => ("ACSH_PYRAMID_CLASS", "AcDbShPyramid"),
            Self::Torus(_) => ("ACSH_TORUS_CLASS", "AcDbShTorus"),
            Self::Boolean(_) => ("ACSH_BOOLEAN_CLASS", "AcDbShBoolean"),
            Self::Brep(_) => ("ACSH_BREP_CLASS", "AcDbShBrep"),
            Self::Fillet(_) => ("ACSH_FILLET_CLASS", "AcDbShFillet"),
            Self::Chamfer(_) => ("ACSH_CHAMFER_CLASS", "AcDbShChamfer"),
            Self::Sweep(_) => ("ACSH_SWEEP_CLASS", "AcDbShSweep"),
            Self::Extrusion(_) => ("ACSH_EXTRUSION_CLASS", "AcDbShExtrusion"),
            Self::Loft(_) => ("ACSH_LOFT_CLASS", "AcDbShLoft"),
            Self::Revolve(_) => ("ACSH_REVOLVE_CLASS", "AcDbShRevolve"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryBox {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub length: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistorySphere {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub radius: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryCylinder {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub height: f64,
    pub major_radius: f64,
    pub minor_radius: f64,
    pub x_radius: f64,
}

/// Parametric cone or frustum history.
///
/// The three stored radii are the two base semi-axes and the top major
/// radius. Keeping this separate from cylinder history prevents the top
/// radius from being mistaken for a third copy of the base radius.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryCone {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub height: f64,
    pub base_x_radius: f64,
    pub base_y_radius: f64,
    pub top_radius: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryPyramid {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub height: f64,
    pub sides: i32,
    pub radius: f64,
    pub top_radius: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryTorus {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub major_radius: f64,
    pub minor_radius: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryBoolean {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub operation: u8,
    pub first_operand: i32,
    pub second_operand: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryBrep {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub acis_data: AcisData,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryFillet {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub method: i32,
    pub edges: Vec<i32>,
    pub radii: Vec<f64>,
    pub start_setbacks: Vec<f64>,
    pub end_setbacks: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryChamfer {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub method: i32,
    pub base_distance: f64,
    pub other_distance: f64,
    pub edges: Vec<i32>,
    pub base_face: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistorySweep {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub direction: Vector3,
    pub sweep_entity: Option<crate::entities::EmbeddedEntity>,
    pub path_entity: Option<crate::entities::EmbeddedEntity>,
    pub draft_angle: f64,
    pub start_draft_distance: f64,
    pub end_draft_distance: f64,
    pub scale_factor: f64,
    pub twist_angle: f64,
    pub align_angle: f64,
    pub sweep_entity_transform: [f64; 16],
    pub path_entity_transform: [f64; 16],
    pub align_option: u8,
    pub miter_option: u8,
    pub has_align_start: bool,
    pub bank: bool,
    pub check_intersections: bool,
    pub flags_294_296: [bool; 3],
    pub reference_point: Vector3,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryLoft {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub cross_sections: Vec<crate::entities::EmbeddedEntity>,
    pub guides: Vec<crate::entities::EmbeddedEntity>,
    /// Optional construction settings retained in a versioned extension record.
    /// The native history stream remains unchanged for other consumers.
    #[cfg_attr(feature = "serde", serde(default))]
    pub parameters: Option<SolidHistoryLoftParameters>,
}

/// Parametric loft settings. Angles are radians; magnitudes are nonnegative.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct SolidHistoryLoftParameters {
    pub path_entity: Option<crate::entities::EmbeddedEntity>,
    /// 0 ruled, 1 smooth, 2 first, 3 last, 4 ends, 5 all, 6 draft angles.
    pub normals: i32,
    pub start_draft_angle: f64,
    pub end_draft_angle: f64,
    pub start_magnitude: f64,
    pub end_magnitude: f64,
    /// Continuity and dimensionless bulge at point ends only.
    pub start_continuity: i32,
    pub end_continuity: i32,
    pub start_bulge: f64,
    pub end_bulge: f64,
    pub closed: bool,
    pub periodic: bool,
    pub surface: bool,
    pub align_direction: bool,
    /// Number of embedded members in each joined cross section. Empty means
    /// one independent section per embedded entity.
    pub section_counts: Vec<usize>,
}

impl Default for SolidHistoryLoftParameters {
    fn default() -> Self {
        Self {
            path_entity: None,
            normals: 1,
            start_draft_angle: std::f64::consts::FRAC_PI_2,
            end_draft_angle: std::f64::consts::FRAC_PI_2,
            start_magnitude: 0.0,
            end_magnitude: 0.0,
            start_continuity: 1,
            end_continuity: 1,
            start_bulge: 0.5,
            end_bulge: 0.5,
            closed: false,
            periodic: true,
            surface: false,
            align_direction: true,
            section_counts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidHistoryRevolve {
    pub base: SolidHistoryNodeBase,
    pub operation_major: i32,
    pub operation_minor: i32,
    pub axis_point: Vector3,
    pub direction: Vector3,
    pub revolve_angle: f64,
    pub start_angle: f64,
    pub draft_angle: f64,
    pub field_44: f64,
    pub field_45: f64,
    pub twist_angle: f64,
    pub flag_290: bool,
    pub close_to_axis: bool,
    pub sweep_entity: Option<crate::entities::EmbeddedEntity>,
}
