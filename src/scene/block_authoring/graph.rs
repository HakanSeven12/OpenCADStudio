//! Maintain native adjacency lists while adding/replacing action dependencies.
use codec::objects::{BlockEvaluationEdge, BlockEvaluationGraph, DynamicBlockData, ObjectType};
use codec::{Document, Handle};

pub(crate) fn valid_lists(g: &BlockEvaluationGraph, node: i32) -> bool {
    let Some(n) = g.nodes.iter().find(|n| n.id == node) else {
        return false;
    };
    for (head, tail, previous_slot, next_slot, outgoing) in
        [(0, 1, 0, 1, false), (2, 3, 2, 3, true)]
    {
        let mut current = n.node_data[head];
        let mut previous = -1;
        let mut seen = std::collections::BTreeSet::new();
        while current >= 0 {
            if !seen.insert(current) {
                return false;
            }
            let Some(edge) = g.edges.iter().find(|e| e.id == current) else {
                return false;
            };
            if (if outgoing {
                edge.source_node
            } else {
                edge.destination_node
            }) != node
                || edge.outgoing_edges[previous_slot] != previous
            {
                return false;
            }
            previous = current;
            current = edge.outgoing_edges[next_slot];
        }
        if previous != n.node_data[tail]
            || seen.len()
                != g.edges
                    .iter()
                    .filter(|e| {
                        (if outgoing {
                            e.source_node
                        } else {
                            e.destination_node
                        }) == node
                    })
                    .count()
        {
            return false;
        }
    }
    true
}

pub(crate) fn remove(g: &mut BlockEvaluationGraph, id: i32) {
    let edge = g.edges.iter().find(|e| e.id == id).unwrap().clone();
    let [pi, ni, po, no, _] = edge.outgoing_edges;
    for (neighbor, field, value) in [(pi, 1, ni), (ni, 0, pi), (po, 3, no), (no, 2, po)] {
        if let Some(e) = g.edges.iter_mut().find(|e| e.id == neighbor) {
            e.outgoing_edges[field] = value;
        }
    }
    for n in &mut g.nodes {
        if n.id == edge.destination_node {
            if n.node_data[0] == id {
                n.node_data[0] = ni;
            }
            if n.node_data[1] == id {
                n.node_data[1] = pi;
            }
        }
        if n.id == edge.source_node {
            if n.node_data[2] == id {
                n.node_data[2] = no;
            }
            if n.node_data[3] == id {
                n.node_data[3] = po;
            }
        }
    }
    g.edges.retain(|e| e.id != id);
}
fn add(g: &mut BlockEvaluationGraph, id: i32, source: i32, destination: i32, inverse: i32) {
    let incoming = g
        .nodes
        .iter()
        .find(|n| n.id == destination)
        .unwrap()
        .node_data[1];
    let outgoing = g.nodes.iter().find(|n| n.id == source).unwrap().node_data[3];
    if let Some(e) = g.edges.iter_mut().find(|e| e.id == incoming) {
        e.outgoing_edges[1] = id;
    }
    if let Some(e) = g.edges.iter_mut().find(|e| e.id == outgoing) {
        e.outgoing_edges[3] = id;
    }
    for n in &mut g.nodes {
        if n.id == destination {
            if n.node_data[0] < 0 {
                n.node_data[0] = id;
            }
            n.node_data[1] = id;
        }
        if n.id == source {
            if n.node_data[2] < 0 {
                n.node_data[2] = id;
            }
            n.node_data[3] = id;
        }
    }
    g.edges.push(BlockEvaluationEdge {
        id,
        next_id: 4,
        incoming_edge: 1,
        source_node: source,
        destination_node: destination,
        outgoing_edges: [incoming, -1, outgoing, -1, inverse],
    });
}
pub(crate) fn bind_ids(
    document: &mut Document,
    graph: Handle,
    action: i32,
    parameters: &[i32],
) -> Result<(), &'static str> {
    let Some(ObjectType::DynamicBlock(object)) = document.objects.get(&graph) else {
        return Err("The authoring graph is unavailable.");
    };
    let DynamicBlockData::EvaluationGraph(original) = &object.data else {
        return Err("The authoring graph is unavailable.");
    };
    let mut g = original.clone();
    if std::iter::once(action)
        .chain(parameters.iter().copied())
        .any(|id| g.nodes.iter().filter(|n| n.next_id == id).count() != 1)
    {
        return Err("An action binding has an invalid or ambiguous expression ID.");
    }
    let action = g
        .nodes
        .iter()
        .find(|n| n.next_id == action)
        .ok_or("The action node is unavailable.")?
        .id;
    let mut bindings = vec![];
    for parameter in parameters {
        let node = g
            .nodes
            .iter()
            .find(|n| n.next_id == *parameter)
            .ok_or("A bound parameter no longer belongs to this block.")?
            .id;
        if node == action {
            return Err("An action cannot bind to itself.");
        }
        if !bindings.contains(&node) {
            bindings.push(node);
        }
    }
    let old = g
        .edges
        .iter()
        .filter(|e| e.source_node == action || e.destination_node == action)
        .collect::<Vec<_>>();
    if std::iter::once(action)
        .chain(bindings.iter().copied())
        .chain(old.iter().flat_map(|e| [e.source_node, e.destination_node]))
        .any(|n| !valid_lists(&g, n))
    {
        return Err("The authoring graph has invalid adjacency links.");
    }
    if old.iter().any(|e| e.next_id != 4) {
        return Err("This action has unsupported graph dependencies.");
    }
    if old.iter().any(|e| {
        !old.iter().any(|inverse| {
            inverse.id == e.outgoing_edges[4]
                && inverse.outgoing_edges[4] == e.id
                && inverse.source_node == e.destination_node
                && inverse.destination_node == e.source_node
        })
    }) {
        return Err("The authoring graph has invalid inverse dependencies.");
    }
    let expected = bindings
        .iter()
        .flat_map(|n| [(action, *n), (*n, action)])
        .collect::<std::collections::BTreeSet<_>>();
    if old.len() == expected.len()
        && old
            .iter()
            .map(|e| (e.source_node, e.destination_node))
            .collect::<std::collections::BTreeSet<_>>()
            == expected
    {
        return Ok(());
    }
    let mut id = g
        .edges
        .iter()
        .map(|e| e.id)
        .max()
        .unwrap_or(-1)
        .checked_add(1)
        .ok_or("The graph has no free edge IDs.")?;
    id.checked_add(bindings.len() as i32 * 2 - 1)
        .ok_or("The graph has no free edge IDs.")?;
    let old = old.iter().map(|e| e.id).collect::<Vec<_>>();
    for edge in old {
        remove(&mut g, edge);
    }
    for node in bindings {
        add(&mut g, id, node, action, id + 1);
        add(&mut g, id + 1, action, node, id);
        id = id.checked_add(2).unwrap_or(i32::MAX);
    }
    let ObjectType::DynamicBlock(object) = document.objects.get_mut(&graph).unwrap() else {
        unreachable!()
    };
    object.data = DynamicBlockData::EvaluationGraph(g);
    Ok(())
}

/// Add a directed native grip dependency without disturbing existing lists.
pub(crate) fn link(
    g: &mut BlockEvaluationGraph,
    source: i32,
    destination: i32,
) -> Result<(), &'static str> {
    if !valid_lists(g, source) || !valid_lists(g, destination) {
        return Err("The authoring graph has invalid adjacency links.");
    }
    if g.edges
        .iter()
        .any(|e| e.source_node == source && e.destination_node == destination)
    {
        return Ok(());
    }
    let id = g
        .edges
        .iter()
        .map(|e| e.id)
        .max()
        .unwrap_or(-1)
        .checked_add(1)
        .ok_or("The graph has no free edge IDs.")?;
    add(g, id, source, destination, -1);
    g.edges.last_mut().unwrap().next_id = 0;
    Ok(())
}
