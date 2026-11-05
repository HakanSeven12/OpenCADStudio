//! The chain that records the order one definition's actions run in.
use super::*;

/// Follow-ups in retained list order, spelled as shown in the Actions tab.
/// A handle outside the definition's actions has no row.
pub(crate) fn labels(doc: &Document, definition: Handle, action: Handle) -> Option<Vec<String>> {
    let all = actions(doc, definition);
    let (_, head, _, owner) = all.iter().find(|(handle, ..)| *handle == action)?;
    let mut seen = std::collections::BTreeSet::new();
    Some(
        head.action_ids
            .iter()
            .filter(|id| **id > 0 && seen.insert(**id))
            .filter_map(|id| {
                all.iter()
                    .find(|(_, other, _, next_owner)| {
                        next_owner == owner && &other.element.eval.node_id == id
                    })
                    .map(|(_, other, _, _)| other.element.name.clone())
            })
            .collect(),
    )
}

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

/// Walk the chain from one action. A chain that arrives back where the walk started
/// has no first action, so the order it names is one nothing can run.
fn loops(doc: &Document, definition: Handle, action: Handle) -> bool {
    let all = actions(doc, definition);
    let by_id: std::collections::BTreeMap<_, _> = all
        .iter()
        .filter(|(_, record, ..)| record.element.eval.node_id > 0)
        .map(|record| (record.1.element.eval.node_id, record))
        .collect();
    let mut visited = std::collections::BTreeSet::from([action]);
    let mut pending = Vec::from_iter(all.iter().find(|(handle, ..)| *handle == action));
    while let Some((_, current, kind, owner)) = pending.pop() {
        if matches!(kind, Kind::Array | Kind::Stretch | Kind::PolarStretch) {
            continue;
        }
        for id in &current.action_ids {
            let Some(&next) = by_id.get(id) else { continue };
            if next.3 != *owner {
                continue;
            }
            if next.0 == action {
                return true;
            }
            if visited.insert(next.0) {
                pending.push(next);
            }
        }
    }
    false
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
        chain_order(&staged, definition, &actions(&staged, definition))?;
        if loops(&staged, definition, action) {
            return Err("An action chain cannot loop back to the action that started it.");
        }
    }
    *doc = staged;
    Ok(true)
}

/// Name the follow-up the way the Actions tab shows it. An empty name hands the
/// order back to the evaluation graph, which is what clearing the row means.
pub(crate) fn set_by_name(
    doc: &mut Document,
    definition: Handle,
    action: Handle,
    name: &str,
) -> Result<bool, &'static str> {
    let name = name.trim();
    let follow_up = if name.is_empty() {
        None
    } else {
        let mut named = actions(doc, definition)
            .into_iter()
            .filter(|(_, other, _, _)| other.element.name.eq_ignore_ascii_case(name))
            .map(|(handle, ..)| handle);
        let Some(handle) = named.next() else {
            return Err("Select a follow-up action belonging to the active block definition.");
        };
        if named.next().is_some() {
            return Err("Two actions in this block share that name.");
        }
        Some(handle)
    };
    set(doc, definition, action, follow_up)
}
