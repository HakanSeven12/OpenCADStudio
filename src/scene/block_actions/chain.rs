//! The chain that records the order one definition's actions run in.
// The row and the prompt that reach this write land next; the order itself is
// already honoured by evaluation and tested here.
#![allow(dead_code)]
use super::*;

/// The action record inside a dynamic block object, whichever kind it is.
fn action_of(data: &mut DynamicBlockData) -> Option<&mut BlockAction> {
    match data {
        DynamicBlockData::MoveAction(action) => Some(&mut action.action),
        DynamicBlockData::StretchAction(action) => Some(&mut action.action),
        DynamicBlockData::PolarStretchAction(action) => Some(&mut action.action),
        DynamicBlockData::FlipAction(action) => Some(&mut action.action),
        DynamicBlockData::RotateAction(action) => Some(&mut action.action.action),
        DynamicBlockData::ScaleAction(action) => Some(&mut action.action.action),
        DynamicBlockData::ArrayAction(action) => Some(&mut action.action),
        _ => None,
    }
}

/// Name the action whose effect this one works on next. That is the only record
/// of the order the operator chose, because the follow-up sees the member as this
/// action left it; an action that names nothing runs whenever the graph lists it,
/// and `None` hands the order back to the graph.
///
/// The chain is written to a copy of the definition and read back through the order
/// the evaluator builds, so a write that would leave the definition unable to run is
/// refused here rather than at evaluation, and the record is left as it was.
pub(crate) fn set(
    doc: &mut Document,
    definition: Handle,
    action: Handle,
    follow_up: Option<Handle>,
) -> Result<bool, &'static str> {
    let all = actions(doc, definition);
    let (_, head, kind, owner) = all
        .iter()
        .find(|(handle, ..)| *handle == action)
        .ok_or("Select an action belonging to the active block definition.")?;
    let ids = match follow_up {
        None => vec![],
        Some(follow_up) => {
            if follow_up == action {
                return Err("An action cannot chain to itself.");
            }
            let (_, next, _, next_owner) = all
                .iter()
                .find(|(handle, ..)| *handle == follow_up)
                .ok_or("Select a follow-up action belonging to the active block definition.")?;
            if next.element.eval.node_id <= 0 {
                return Err("Select a follow-up action the evaluation graph names.");
            }
            if next_owner != owner {
                return Err("Select a follow-up action of the same parameter.");
            }
            match kind {
                Kind::Array => return Err("An Array action cannot chain a follow-up action yet."),
                Kind::Stretch => {
                    return Err("This Stretch action cannot chain follow-up actions yet.")
                }
                Kind::PolarStretch => {
                    return Err("This Polar Stretch action cannot chain follow-up actions yet.")
                }
                _ => vec![next.element.eval.node_id],
            }
        }
    };
    if head.action_ids == ids {
        return Ok(false);
    }
    let mut staged = doc.clone();
    let Some(ObjectType::DynamicBlock(object)) = staged.objects.get_mut(&action) else {
        unreachable!()
    };
    let Some(inner) = action_of(&mut object.data) else {
        unreachable!()
    };
    inner.action_ids = ids;
    if follow_up.is_some() {
        // A chain the order cannot run is not one the operator can write. Clearing
        // one only ever shortens a sequence, so the order has nothing to refuse.
        chain_order(&actions(&staged, definition))?;
    }
    *doc = staged;
    Ok(true)
}
