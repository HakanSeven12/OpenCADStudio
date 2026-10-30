//! Runtime grip eligibility for supported native action bindings.
use super::*;

fn connections_match(connections: &[BlockConnection], id: i32, names: &[&str]) -> bool {
    connections.len() == names.len()
        && connections
            .iter()
            .zip(names)
            .all(|(connection, name)| connection.code == id && connection.name == *name)
}

/// The same binding contract is used by grip activation and action evaluation.
/// None denotes a displacement family, whose key-point rules are separate.
pub(super) fn scalar_mask(parameter: &Parameter, data: &DynamicBlockData) -> Option<u8> {
    let id = parameter.id;
    let base_names = &["UpdatedBaseX", "UpdatedBaseY"];
    let (valid, roles) = match data {
        DynamicBlockData::ScaleAction(action) => (
            connections_match(&action.connections, id, &["Scale", "XScale", "YScale"])
                && connections_match(&action.action.connections, id, base_names),
            0b0011,
        ),
        DynamicBlockData::ArrayAction(action) => (
            connections_match(
                &action.connections,
                id,
                &["Base", "End", "UpdatedBase", "UpdatedEnd"],
            ),
            if parameter.xy { 0b1111 } else { 0b0011 },
        ),
        DynamicBlockData::RotateAction(action) => (
            connections_match(&action.connections, id, &["AngleDelta"])
                && connections_match(&action.action.connections, id, base_names),
            0b0010,
        ),
        DynamicBlockData::FlipAction(action) => (
            connections_match(
                &action.connections,
                id,
                &["Flip", "UpdatedFlip", "UpdatedBase", "UpdatedEnd"],
            ),
            0b0001,
        ),
        _ => return None,
    };
    Some(if valid { roles } else { 0 })
}

fn displacement(p: &Parameter, connections: &[BlockConnection]) -> u8 {
    if connections.len() != 2 || connections.iter().any(|connection| connection.code != p.id) {
        return 0;
    }
    match (
        p.point,
        p.xy,
        connections[0].name.as_str(),
        connections[1].name.as_str(),
    ) {
        (true, _, "XDelta", "YDelta") => 1,
        // XY corners share endpoint coordinates. A displacement input is
        // associated with every configured role that can change either input.
        (false, true, "BaseXDelta", "BaseYDelta") => 0b1101,
        (false, true, "EndXDelta", "EndYDelta") => 0b1110,
        (false, true, "EndXDelta", "BaseYDelta") => 0b0111,
        (false, true, "BaseXDelta", "EndYDelta") => 0b1011,
        (false, _, "BaseXDelta", "BaseYDelta") => 1,
        (false, _, "EndXDelta", "EndYDelta") => 2,
        _ => 0,
    }
}

pub(crate) fn mask(doc: &Document, definition: Handle, parameter: &Parameter) -> u8 {
    actions(doc, definition)
        .into_iter()
        .filter(|(_, _, kind, node)| *node == parameter.id && compatible(parameter, *kind))
        .fold(0, |mask, (handle, _, _, _)| {
            let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&handle) else {
                return mask;
            };
            mask | match &object.data {
                DynamicBlockData::MoveAction(action) => {
                    displacement(parameter, &action.connections)
                }
                DynamicBlockData::StretchAction(action) => {
                    displacement(parameter, &action.connections)
                }
                DynamicBlockData::PolarStretchAction(action) => {
                    match stretch::polar_key_point(action) {
                        Ok(XyKeyPoint::Base) => 1,
                        Ok(XyKeyPoint::End) => 2,
                        _ => 0,
                    }
                }
                _ => scalar_mask(parameter, &object.data).unwrap_or(0),
            }
        })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod scalar_tests;
