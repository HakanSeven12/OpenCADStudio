//! The chain that records the order one definition's actions run in.
use super::*;

/// Schedule the actions blocked by retained loops. Each strongly connected
/// component runs in graph order and releases its successors when complete.
pub(super) fn append_retained_cycles(
    successors: &[Vec<usize>],
    predecessors: &[usize],
    order: &mut Vec<usize>,
) {
    let blocked = |index: usize| predecessors[index] > 0;
    let mut incoming = vec![Vec::new(); successors.len()];
    for (source, edges) in successors
        .iter()
        .enumerate()
        .filter(|(index, _)| blocked(*index))
    {
        for &target in edges {
            if blocked(target) {
                incoming[target].push(source);
            }
        }
    }

    // Two iterative DFS passes find the components without recursing on
    // retained file data. First record finish order on the original edges.
    let mut visited = vec![false; successors.len()];
    let mut finished = Vec::new();
    for root in (0..successors.len()).filter(|index| blocked(*index)) {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        let mut pending = vec![(root, successors[root].iter().copied())];
        while let Some((node, edges)) = pending.last_mut() {
            if let Some(next) = edges.find(|&next| blocked(next) && !visited[next]) {
                visited[next] = true;
                pending.push((next, successors[next].iter().copied()));
            } else {
                finished.push(*node);
                pending.pop();
            }
        }
    }
    // Walking reversed edges in reverse finish order stays in one component.
    let mut component_of = vec![None; successors.len()];
    let mut members = Vec::new();
    for root in finished.into_iter().rev() {
        if component_of[root].is_some() {
            continue;
        }
        let component = members.len();
        let mut group = Vec::new();
        let mut pending = vec![root];
        component_of[root] = Some(component);
        while let Some(node) = pending.pop() {
            group.push(node);
            for &previous in &incoming[node] {
                if component_of[previous].is_none() {
                    component_of[previous] = Some(component);
                    pending.push(previous);
                }
            }
        }
        group.sort_unstable();
        members.push(group);
    }

    // Ignore only edges within a loop. Its exits still enforce precedence
    // and retain their list order when released together.
    let mut exits = vec![Vec::new(); members.len()];
    let mut incoming_groups = vec![0; members.len()];
    for (component, group) in members.iter().enumerate() {
        let mut seen = std::collections::BTreeSet::new();
        for target in group.iter().flat_map(|node| &successors[*node]) {
            if let Some(next) = component_of[*target] {
                if next != component && seen.insert(next) {
                    exits[component].push(next);
                    incoming_groups[next] += 1;
                }
            }
        }
    }
    let mut left: Vec<_> = members.iter().map(Vec::len).collect();
    let mut ready: std::collections::VecDeque<_> = component_of
        .iter()
        .enumerate()
        .filter_map(|(node, group)| {
            group
                .filter(|&group| incoming_groups[group] == 0)
                .map(|_| node)
        })
        .collect();
    while let Some(node) = ready.pop_front() {
        order.push(node);
        let component = component_of[node].expect("scheduled action belongs to a component");
        left[component] -= 1;
        if left[component] == 0 {
            for &next in &exits[component] {
                incoming_groups[next] -= 1;
                if incoming_groups[next] == 0 {
                    ready.extend(members[next].iter().copied());
                }
            }
        }
    }
}

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
    while let Some((_, current, _, owner)) = pending.pop() {
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

#[cfg(test)]
pub(crate) fn set(
    doc: &mut Document,
    definition: Handle,
    action: Handle,
    follow_up: Option<Handle>,
) -> Result<bool, &'static str> {
    set_follow_ups(doc, definition, action, follow_up.as_slice())
}

/// Replace the named follow-ups while keeping retained links to other graph nodes.
fn set_follow_ups(
    doc: &mut Document,
    definition: Handle,
    action: Handle,
    follow_ups: &[Handle],
) -> Result<bool, &'static str> {
    let all = actions(doc, definition);
    let (_, head, _, owner) = all
        .iter()
        .find(|(handle, ..)| *handle == action)
        .ok_or("Select an action belonging to the active block definition.")?;
    let mut requested = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for follow_up in follow_ups {
        if *follow_up == action {
            return Err("An action cannot chain to itself.");
        }
        let (_, next, _, next_owner) = all
            .iter()
            .find(|(handle, ..)| handle == follow_up)
            .ok_or("Select a follow-up action belonging to the active block definition.")?;
        if next.element.eval.node_id <= 0 {
            return Err("Select a follow-up action the evaluation graph names.");
        }
        if next_owner != owner {
            return Err("Select a follow-up action of the same parameter.");
        }
        let id = next.element.eval.node_id;
        if seen.insert(id) {
            requested.push(id);
        }
    }
    let named_ids: std::collections::BTreeSet<_> = all
        .iter()
        .filter(|(_, record, _, candidate_owner)| {
            candidate_owner == owner && record.element.eval.node_id > 0
        })
        .map(|(_, record, _, _)| record.element.eval.node_id)
        .collect();
    seen.clear();
    if head
        .action_ids
        .iter()
        .copied()
        .filter(|id| named_ids.contains(id) && seen.insert(*id))
        .eq(requested.iter().copied())
    {
        return Ok(false);
    }
    let mut replacements = requested.into_iter();
    let mut ids: Vec<_> = head
        .action_ids
        .iter()
        .filter_map(|id| {
            if named_ids.contains(id) {
                replacements.next()
            } else {
                Some(*id)
            }
        })
        .collect();
    ids.extend(replacements);
    let mut staged = doc.clone();
    let Some(ObjectType::DynamicBlock(object)) = staged.objects.get_mut(&action) else {
        unreachable!()
    };
    let Some(inner) = action_of(&mut object.data) else {
        unreachable!()
    };
    inner.action_ids = ids;
    if !follow_ups.is_empty() {
        // A chain the order cannot run is not one the operator can write. Clearing
        // one only ever shortens a sequence, so the order has nothing to refuse.
        chain_order(&staged, definition, &actions(&staged, definition))?;
        if loops(&staged, definition, action) {
            return Err("An action chain cannot loop back to the action that started it.");
        }
    }
    super::flip_graph::preserve_authored_selection(doc, &staged, definition)?;
    *doc = staged;
    Ok(true)
}

/// Accept a single action name or an ordered JSON array of literal names.
/// A blank field or an empty array clears the named follow-ups.
pub(crate) fn set_by_name(
    doc: &mut Document,
    definition: Handle,
    action: Handle,
    name: &str,
) -> Result<bool, &'static str> {
    let name = name.trim();
    let names = if name.is_empty() {
        Vec::new()
    } else {
        serde_json::from_str::<Vec<String>>(name).unwrap_or_else(|_| vec![name.to_owned()])
    };
    let all = actions(doc, definition);
    let follow_ups = names
        .iter()
        .map(|name| {
            let mut named = all
                .iter()
                .filter(|(_, other, _, _)| other.element.name.eq_ignore_ascii_case(name))
                .map(|(handle, ..)| *handle);
            let Some(handle) = named.next() else {
                return Err("Select a follow-up action belonging to the active block definition.");
            };
            if named.next().is_some() {
                return Err("Two actions in this block share that name.");
            }
            Ok(handle)
        })
        .collect::<Result<Vec<_>, _>>()?;
    set_follow_ups(doc, definition, action, &follow_ups)
}
