//! Deterministic Move-to-parameter propagation. Reference caches retain
//! effective frames; recover autonomous input before replaying the graph.
use super::instance::ParameterFrame;
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const CYCLE_ERROR: &str = "The block action chain contains a cycle.";

/// Which points of a target parameter a Move action carries: the whole
/// parameter, or one end of a Linear parameter whose grip it selects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Part {
    base: bool,
    end: bool,
}

const WHOLE: Part = Part {
    base: true,
    end: true,
};

impl Part {
    fn union(self, other: Part) -> Part {
        Part {
            base: self.base || other.base,
            end: self.end || other.end,
        }
    }
}

/// Per-end displacement received from incoming Move actions.
#[derive(Clone, Copy, Default)]
struct Offset {
    base: DVec3,
    end: DVec3,
}

fn frame_index(
    doc: &Document,
    definition: Handle,
    parameters: &[Parameter],
) -> Result<BTreeMap<Handle, (Handle, i32, Part)>, &'static str> {
    let owner = authoring::graph(doc, definition).ok().flatten();
    let mut index = BTreeMap::new();
    for p in parameters.iter().filter(|p| p.is_move_target()) {
        let mut members = vec![(p.handle, WHOLE)];
        if p.point || p.flip {
            members.push((p.grip, WHOLE));
        } else if p.linear {
            // A Linear grip in a Move selection carries that end only.
            for grip in super::super::block_grips::config::locations(doc, definition, p.handle)
                .unwrap_or_default()
            {
                let part = match grip.role {
                    0 => Part {
                        base: true,
                        end: false,
                    },
                    1 => Part {
                        base: false,
                        end: true,
                    },
                    _ => continue,
                };
                members.push((grip.handle, part));
            }
        }
        for (h, part) in members {
            if h != Handle::NULL && doc.object_owner(h) == owner {
                if index
                    .get(&h)
                    .is_some_and(|previous: &(Handle, i32, Part)| {
                        (previous.0, previous.1) != (p.handle, p.id)
                    })
                {
                    return Err("An action has an unsupported parameter binding.");
                }
                index.insert(h, (p.handle, p.id, part));
            }
        }
    }
    Ok(index)
}

pub(super) fn frame_target(
    doc: &Document,
    definition: Handle,
    source_id: i32,
    handle: Handle,
) -> Option<Handle> {
    let parameters = all_parameters(doc, definition);
    let index = frame_index(doc, definition, &parameters).ok()?;
    index.get(&handle).and_then(|(parameter, id, _)| {
        (*id != source_id || *parameter == handle).then_some(*parameter)
    })
}

/// Points have one movable coordinate; two-point parameters transport both.
fn translated(p: &Parameter, frame: ParameterFrame, offset: Offset, sign: f64) -> ParameterFrame {
    ParameterFrame {
        base: if p.point {
            p.base
        } else {
            frame.base + offset.base * sign
        },
        end: frame.end + offset.end * sign,
    }
}

struct Link<'a> {
    target: Handle,
    part: Part,
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
        let index = frame_index(doc, definition, &parameters)?;
        let mut graph = Self {
            nodes: BTreeMap::new(),
            order: Vec::new(),
        };
        let mut edges = BTreeSet::new();
        for (handle, action, kind, source_id) in actions(doc, definition) {
            if kind != Kind::Move {
                continue;
            }
            let mut targets = BTreeMap::<Handle, Part>::new();
            for handle in &action.dependencies {
                if let Some((parameter, id, part)) = index.get(handle) {
                    if *id != source_id || *parameter == *handle {
                        let entry = targets.entry(*parameter).or_insert(*part);
                        *entry = entry.union(*part);
                    }
                }
            }
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
                    .keys()
                    .map(|target| parameters.iter().find(|p| p.handle == *target).unwrap()),
            ) {
                let chained = match &doc.objects[&p.handle] {
                    ObjectType::DynamicBlock(object) => {
                        matches!(&object.data,
                        DynamicBlockData::PointParameter(p) if p.parameter.parameter.chain_actions)
                            || matches!(&object.data, DynamicBlockData::LinearParameter(p) if p.parameter.parameter.chain_actions)
                            || matches!(&object.data, DynamicBlockData::FlipParameter(p) if p.parameter.parameter.chain_actions)
                    }
                    _ => false,
                };
                graph.nodes.entry(p.handle).or_insert_with(|| Node {
                    parameter: p.clone(),
                    chained,
                    links: Vec::new(),
                });
            }
            for (target, part) in targets {
                edges.insert((source.handle, target));
                graph
                    .nodes
                    .get_mut(&source.handle)
                    .unwrap()
                    .links
                    .push(Link {
                        target,
                        part,
                        action,
                    });
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
    pub(super) fn receivers(&self) -> impl Iterator<Item = &Parameter> {
        self.nodes
            .values()
            .map(|n| &n.parameter)
            .filter(|p| p.is_move_target())
    }

    pub(super) fn autonomous(
        &self,
        previous: &Document,
        values: Option<&BTreeMap<Handle, DVec3>>,
        previous_bases: Option<&BTreeMap<Handle, DVec3>>,
    ) -> Result<BTreeMap<Handle, ParameterFrame>, &'static str> {
        let mut incoming = BTreeMap::<Handle, Offset>::new();
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
                incoming.get(handle).copied().unwrap_or_default(),
                -1.0,
            );
            autonomous.insert(*handle, raw);
            let active = if p.is_move_target() && !node.chained {
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

    pub(super) fn has_unchained_incoming_move(&self, handle: Handle) -> bool {
        self.nodes.get(&handle).is_some_and(|node| !node.chained)
            && self
                .nodes
                .values()
                .any(|node| node.links.iter().any(|link| link.target == handle))
    }

    /// The frame driving an action can differ from its displayed frame when
    /// an incoming parameter is not configured to chain its actions.
    pub(super) fn active_frame(
        &self,
        handle: Handle,
        effective: ParameterFrame,
        autonomous: &BTreeMap<Handle, ParameterFrame>,
    ) -> ParameterFrame {
        match self.nodes.get(&handle) {
            Some(node) if !node.chained => autonomous.get(&handle).copied().unwrap_or(effective),
            _ => effective,
        }
    }

    pub(super) fn resolve(
        &self,
        previous: &Document,
        staged: &mut Document,
        selected: &BTreeSet<Handle>,
        autonomous: &BTreeMap<Handle, ParameterFrame>,
        values: Option<&BTreeMap<Handle, DVec3>>,
        bases: Option<&BTreeMap<Handle, DVec3>>,
        active_frames: &mut BTreeMap<Handle, ParameterFrame>,
    ) -> Result<BTreeMap<Handle, ParameterFrame>, &'static str> {
        let mut incoming = BTreeMap::<Handle, Offset>::new();
        let mut final_frames = BTreeMap::new();
        for handle in &self.order {
            let node = &self.nodes[handle];
            let p = &node.parameter;
            let offset = incoming.get(handle).copied().unwrap_or_default();
            let mut raw = if selected.contains(handle) && !p.flip {
                translated(
                    p,
                    ParameterFrame {
                        base: bases.and_then(|b| b.get(handle)).copied().unwrap_or(p.base),
                        end: current(staged, p),
                    },
                    offset,
                    -1.0,
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
            if !p.point && !incoming.contains_key(handle) {
                raw.base = bases.and_then(|b| b.get(handle)).copied().unwrap_or(p.base);
            }
            let effective = translated(p, raw, offset, 1.0);
            let active = if p.is_move_target() && !node.chained {
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
            active_frames.insert(*handle, active);
            if p.is_move_target() {
                if !p.flip {
                    set_value(staged, p, Value::Point(active.end))?;
                }
                final_frames.insert(*handle, effective);
            }
            Self::propagate(node, active, &mut incoming)?;
        }
        Ok(final_frames)
    }

    fn propagate(
        node: &Node<'_>,
        frame: ParameterFrame,
        incoming: &mut BTreeMap<Handle, Offset>,
    ) -> Result<(), &'static str> {
        let p = &node.parameter;
        for link in &node.links {
            let delta = action_move_delta(p, frame.base, frame.end, &link.action.connections)
                .ok_or("This Move action uses unsupported parameter connections.")?;
            let delta = options::displacement(&link.action.offsets, delta)?;
            let offset = incoming.entry(link.target).or_default();
            if link.part.base {
                offset.base += delta;
            }
            if link.part.end {
                offset.end += delta;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
