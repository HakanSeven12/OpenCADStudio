//! Handle remapping for decoded associative constraint graphs.
use document::objects::{
    AssocAction, AssocConstraintNodeData as Node, AssocDependency, AssocEvalValue,
    AssocEvalVariant, AssociativeData as Data, AssociativeObject,
};
use document::Handle;
use rustc_hash::FxHashMap;

pub(super) fn cloneable(object: &AssociativeObject) -> bool {
    matches!(
        object.data,
        Data::Network(_)
            | Data::ConstraintGroup(_)
            | Data::Variable(_)
            | Data::Dependency(_)
            | Data::GeomDependency(_)
            | Data::ValueDependency(_)
            | Data::DimDependencyBody(_)
    )
}

fn eval(value: &mut AssocEvalVariant, visit: &mut impl FnMut(&mut Handle)) {
    if let AssocEvalValue::Handle(handle) = &mut value.value {
        visit(handle);
    }
}

fn dependency(value: &mut AssocDependency, visit: &mut impl FnMut(&mut Handle)) {
    for handle in [
        &mut value.dependent_on,
        &mut value.read_dependency,
        &mut value.node,
        &mut value.dependency_body,
    ] {
        visit(handle);
    }
}

fn action(value: &mut AssocAction, visit: &mut impl FnMut(&mut Handle)) {
    visit(&mut value.owning_network);
    visit(&mut value.action_body);
    for item in &mut value.dependencies {
        visit(&mut item.dependency);
    }
    for handle in &mut value.owned_parameters {
        visit(handle);
    }
    for parameter in &mut value.values {
        visit(&mut parameter.controlled_object_dependency);
        for variable in &mut parameter.variables {
            visit(&mut variable.handle);
            eval(&mut variable.value, visit);
        }
    }
}

pub(super) fn remap(
    object: &mut AssociativeObject,
    new: Handle,
    handles: &FxHashMap<Handle, Handle>,
) -> Result<(), String> {
    let mut missing = None;
    let mut visit = |handle: &mut Handle| {
        if handle.is_null() {
            return;
        }
        if let Some(new) = handles.get(handle) {
            *handle = *new;
        } else {
            missing = Some(*handle);
        }
    };
    object.handle = new;
    visit(&mut object.owner);
    if let Some(handle) = &mut object.xdictionary_handle {
        visit(handle);
    }
    // External reactors observe the source; they must not observe its copy.
    object.reactors = object
        .reactors
        .iter()
        .filter_map(|h| handles.get(h).copied())
        .collect();
    match &mut object.data {
        Data::Network(value) => {
            // The drawing-level parent belongs to the checkpoint document.
            // Register this scope under that parent after the staged clone.
            if !handles.contains_key(&value.action.owning_network) {
                value.action.owning_network = Handle::NULL;
            }
            action(&mut value.action, &mut visit);
            for item in &mut value.actions {
                visit(&mut item.dependency);
            }
            for handle in &mut value.owned_actions {
                visit(handle);
            }
        }
        Data::ConstraintGroup(value) => {
            action(&mut value.action, &mut visit);
            visit(&mut value.dependency);
            for handle in &mut value.actions {
                visit(handle);
            }
            for node in &mut value.nodes {
                match &mut node.data {
                    Node::Angle {
                        value_dependency,
                        dimension_dependency,
                        ..
                    }
                    | Node::Distance {
                        value_dependency,
                        dimension_dependency,
                        ..
                    }
                    | Node::RadiusDiameter {
                        value_dependency,
                        dimension_dependency,
                        ..
                    } => {
                        visit(value_dependency);
                        visit(dimension_dependency);
                    }
                    Node::ImplicitPoint {
                        geometry_dependency,
                        ..
                    }
                    | Node::Point {
                        geometry_dependency,
                        ..
                    }
                    | Node::Line {
                        geometry_dependency,
                        ..
                    }
                    | Node::BoundedLine {
                        geometry_dependency,
                        ..
                    }
                    | Node::Circle {
                        geometry_dependency,
                        ..
                    }
                    | Node::Arc {
                        geometry_dependency,
                        ..
                    }
                    | Node::Ellipse {
                        geometry_dependency,
                        ..
                    }
                    | Node::BoundedEllipse {
                        geometry_dependency,
                        ..
                    }
                    | Node::RigidSet {
                        geometry_dependency,
                        ..
                    }
                    | Node::Spline {
                        geometry_dependency,
                        ..
                    } => visit(geometry_dependency),
                    Node::None
                    | Node::Geometrical { .. }
                    | Node::Composite { .. }
                    | Node::HelpParameter { .. }
                    | Node::Parallel { .. } => {}
                }
            }
        }
        Data::Variable(value) => {
            action(&mut value.action, &mut visit);
            eval(&mut value.value, &mut visit);
            for item in &mut value.dependencies {
                visit(&mut item.dependency);
            }
        }
        Data::Dependency(value) => dependency(value, &mut visit),
        Data::GeomDependency(value) => dependency(&mut value.dependency, &mut visit),
        Data::ValueDependency(value) => {
            dependency(&mut value.dependency, &mut visit);
            eval(&mut value.value, &mut visit);
        }
        Data::DimDependencyBody(_) => {}
        _ => return Err("Save Block As cannot copy this associative object type yet.".into()),
    }
    if let Some(handle) = missing {
        return Err(format!(
            "A block constraint graph refers outside the copied definition ({handle:?})."
        ));
    }
    Ok(())
}
