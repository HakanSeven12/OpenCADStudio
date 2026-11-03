//! Deterministic Move-to-Point parameter propagation. Reference caches retain
//! effective endpoints; recover autonomous input before replaying the graph.
use super::instance::ParameterFrame;
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const CYCLE_ERROR: &str = "The block action chain contains a cycle.";

fn point_index(
    doc: &Document,
    definition: Handle,
    parameters: &[Parameter],
) -> Result<BTreeMap<Handle, (Handle, i32)>, &'static str> {
    let owner = authoring::graph(doc, definition).ok().flatten();
    let mut index = BTreeMap::new();
    for p in parameters.iter().filter(|p| p.point) {
        for h in [p.handle, p.grip] {
            if h != Handle::NULL && doc.object_owner(h) == owner {
                if index
                    .get(&h)
                    .is_some_and(|previous| *previous != (p.handle, p.id))
                {
                    return Err("An action has an unsupported parameter binding.");
                }
                index.insert(h, (p.handle, p.id));
            }
        }
    }
    Ok(index)
}

pub(super) fn point_target(
    doc: &Document,
    definition: Handle,
    source_id: i32,
    handle: Handle,
) -> Option<Handle> {
    let parameters = all_parameters(doc, definition);
    let index = point_index(doc, definition, &parameters).ok()?;
    index.get(&handle).and_then(|(parameter, id)| {
        (*id != source_id || *parameter == handle).then_some(*parameter)
    })
}

/// Points have one movable coordinate; two-point parameters transport both.
fn translated(p: &Parameter, frame: ParameterFrame, delta: DVec3) -> ParameterFrame {
    ParameterFrame {
        base: if p.point { p.base } else { frame.base + delta },
        end: frame.end + delta,
    }
}

struct Link<'a> {
    target: Handle,
    action: &'a BlockMoveAction,
}
struct Node<'a> {
    parameter: Parameter,
    chained: bool,
    links: Vec<Link<'a>>,
}

pub(super) struct Graph<'a> {
    nodes: BTreeMap<Handle, Node<'a>>,
    order: Vec<Handle>,
}

impl<'a> Graph<'a> {
    pub(super) fn new(doc: &'a Document, definition: Handle) -> Result<Self, &'static str> {
        let parameters = all_parameters(doc, definition);
        let index = point_index(doc, definition, &parameters)?;
        let mut graph = Self {
            nodes: BTreeMap::new(),
            order: Vec::new(),
        };
        let mut edges = BTreeSet::new();
        for (handle, action, kind, source_id) in actions(doc, definition) {
            if kind != Kind::Move {
                continue;
            }
            let targets = action
                .dependencies
                .iter()
                .filter_map(|handle| {
                    index.get(handle).and_then(|(parameter, id)| {
                        (*id != source_id || *parameter == *handle).then_some(*parameter)
                    })
                })
                .collect::<BTreeSet<_>>();
            if targets.is_empty() {
                continue;
            }
            let source = parameters
                .iter()
                .find(|p| p.id == source_id)
                .ok_or("An action has an unsupported parameter binding.")?;
            let ObjectType::DynamicBlock(object) = &doc.objects[&handle] else {
                unreachable!()
            };
            let DynamicBlockData::MoveAction(action) = &object.data else {
                unreachable!()
            };
            for p in std::iter::once(source).chain(
                targets
                    .iter()
                    .map(|target| parameters.iter().find(|p| p.handle == *target).unwrap()),
            ) {
                let chained = match &doc.objects[&p.handle] {
                    ObjectType::DynamicBlock(object) => matches!(&object.data,
                        DynamicBlockData::PointParameter(p) if p.parameter.parameter.chain_actions),
                    _ => false,
                };
                graph.nodes.entry(p.handle).or_insert_with(|| Node {
                    parameter: p.clone(),
                    chained,
                    links: Vec::new(),
                });
            }
            for target in targets {
                edges.insert((source.handle, target));
                graph
                    .nodes
                    .get_mut(&source.handle)
                    .unwrap()
                    .links
                    .push(Link { target, action });
            }
        }
        let mut indegree = graph
            .nodes
            .keys()
            .map(|h| (*h, 0usize))
            .collect::<BTreeMap<_, _>>();
        let mut adjacent: BTreeMap<Handle, Vec<Handle>> = BTreeMap::new();
        for (source, target) in edges {
            *indegree.get_mut(&target).unwrap() += 1;
            adjacent.entry(source).or_default().push(target);
        }
        let mut ready = indegree
            .iter()
            .filter_map(|(h, n)| (*n == 0).then_some(*h))
            .collect::<BTreeSet<_>>();
        while let Some(handle) = ready.pop_first() {
            graph.order.push(handle);
            for target in adjacent.get(&handle).into_iter().flatten() {
                let count = indegree.get_mut(target).unwrap();
                *count -= 1;
                if *count == 0 {
                    ready.insert(*target);
                }
            }
        }
        if graph.order.len() != graph.nodes.len() {
            return Err(CYCLE_ERROR);
        }
        Ok(graph)
    }

    pub(super) fn parameter(&self, handle: Handle) -> &Parameter {
        &self.nodes[&handle].parameter
    }
    pub(super) fn points(&self) -> impl Iterator<Item = &Parameter> {
        self.nodes
            .values()
            .map(|n| &n.parameter)
            .filter(|p| p.point)
    }

    pub(super) fn autonomous(
        &self,
        previous: &Document,
        values: Option<&BTreeMap<Handle, DVec3>>,
        previous_bases: Option<&BTreeMap<Handle, DVec3>>,
    ) -> Result<BTreeMap<Handle, ParameterFrame>, &'static str> {
        let mut incoming = BTreeMap::<Handle, DVec3>::new();
        let mut autonomous = BTreeMap::new();
        for handle in &self.order {
            let node = &self.nodes[handle];
            let p = &node.parameter;
            let effective = ParameterFrame {
                base: previous_bases
                    .and_then(|b| b.get(handle))
                    .copied()
                    .unwrap_or(p.base),
                end: values
                    .and_then(|v| v.get(handle))
                    .copied()
                    .unwrap_or_else(|| current(previous, p)),
            };
            let raw = translated(
                p,
                effective,
                -incoming.get(handle).copied().unwrap_or(DVec3::ZERO),
            );
            autonomous.insert(*handle, raw);
            let active = if p.point && !node.chained {
                raw
            } else {
                effective
            };
            Self::propagate(node, active, &mut incoming)?;
        }
        Ok(autonomous)
    }

    pub(super) fn contains(&self, handle: Handle) -> bool {
        self.nodes.contains_key(&handle)
    }

    pub(super) fn resolve(
        &self,
        previous: &Document,
        staged: &mut Document,
        selected: &BTreeSet<Handle>,
        autonomous: &BTreeMap<Handle, ParameterFrame>,
        values: Option<&BTreeMap<Handle, DVec3>>,
        bases: Option<&BTreeMap<Handle, DVec3>>,
    ) -> Result<BTreeMap<Handle, ParameterFrame>, &'static str> {
        let mut incoming = BTreeMap::<Handle, DVec3>::new();
        let mut final_points = BTreeMap::new();
        for handle in &self.order {
            let node = &self.nodes[handle];
            let p = &node.parameter;
            let offset = incoming.get(handle).copied().unwrap_or(DVec3::ZERO);
            let mut raw = if selected.contains(handle) {
                translated(
                    p,
                    ParameterFrame {
                        base: bases.and_then(|b| b.get(handle)).copied().unwrap_or(p.base),
                        end: current(staged, p),
                    },
                    -offset,
                )
            } else {
                autonomous
                    .get(handle)
                    .copied()
                    .unwrap_or_else(|| ParameterFrame {
                        base: bases.and_then(|b| b.get(handle)).copied().unwrap_or(p.base),
                        end: values
                            .and_then(|v| v.get(handle))
                            .copied()
                            .unwrap_or_else(|| current(previous, p)),
                    })
            };
            // Non-Point nodes are currently input sources only. Their base is
            // supplied by the current evaluation frame, independently of the
            // endpoint recovered from the previous evaluation.
            if !p.point {
                raw.base = bases.and_then(|b| b.get(handle)).copied().unwrap_or(p.base);
            }
            let effective = translated(p, raw, offset);
            let active = if p.point && !node.chained {
                raw
            } else {
                effective
            };
            if !active.base.is_finite()
                || !active.end.is_finite()
                || !effective.base.is_finite()
                || !effective.end.is_finite()
            {
                return Err("The parameter value produces non-finite geometry.");
            }
            if p.point {
                set_value(staged, p, Value::Point(active.end))?;
                final_points.insert(*handle, effective);
            }
            Self::propagate(node, active, &mut incoming)?;
        }
        Ok(final_points)
    }

    fn propagate(
        node: &Node<'_>,
        frame: ParameterFrame,
        incoming: &mut BTreeMap<Handle, DVec3>,
    ) -> Result<(), &'static str> {
        let p = &node.parameter;
        for link in &node.links {
            let delta = action_move_delta(p, frame.base, frame.end, &link.action.connections)
                .ok_or("This Move action uses unsupported parameter connections.")?;
            let delta = options::displacement(&link.action.offsets, delta)?;
            *incoming.entry(link.target).or_default() += delta;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
