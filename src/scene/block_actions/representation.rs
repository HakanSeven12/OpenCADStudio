//! Synchronize evaluated members by native element identity, never by position.
use super::*;
use crate::scene::Scene;
use rustc_hash::FxHashMap;

pub(super) fn reconcile(
    scene: &mut Scene,
    def: Handle,
    name: &str,
    evaluated: &Document,
) -> Result<(), &'static str> {
    authoring::tag_members(&mut scene.document, def);
    let rep = scene
        .document
        .block_records
        .get(name)
        .ok_or("Missing representation.")?
        .handle;
    let sources = authoring::members(evaluated, def);
    let old = scene
        .document
        .entities()
        .filter(|e| {
            e.common().owner_handle == rep
                && !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))
        })
        .map(|e| e.common().handle)
        .collect::<Vec<_>>();
    let mut by_id = std::collections::BTreeMap::new();
    for h in &old {
        let e = scene
            .document
            .get_entity(*h)
            .ok_or("Missing representation member.")?;
        let id =
            authoring::element_id(e).ok_or("A representation member has no native element ID.")?;
        if by_id.insert(id, *h).is_some() {
            return Err("The representation has duplicate element IDs.");
        }
    }
    let mut mapping = FxHashMap::default();
    let mut pairs = vec![];
    for &from in &sources {
        let source = evaluated
            .get_entity(from)
            .ok_or("Missing definition member.")?;
        let id =
            authoring::element_id(source).ok_or("A definition member has no native element ID.")?;
        let existing = by_id.get(&id).copied().filter(|h| {
            scene
                .document
                .get_entity(*h)
                .is_some_and(|e| std::mem::discriminant(e) == std::mem::discriminant(source))
        });
        let to = existing.unwrap_or_else(|| scene.document.allocate_handle());
        mapping.insert(from, to);
        pairs.push((from, to, id, existing.is_some()));
    }
    let mut ordered = vec![];
    for (from, to, id, existing) in pairs {
        let mut entity = evaluated
            .get_entity(from)
            .cloned()
            .ok_or("Missing evaluated member.")?;
        let source_xdict = entity.common().xdictionary_handle;
        if existing {
            let previous = scene
                .document
                .get_entity(to)
                .cloned()
                .ok_or("Missing representation member.")?;
            let current = previous.common();
            match (&mut entity, &previous) {
                (EntityType::Insert(insert), EntityType::Insert(old)) => {
                    let mut used = std::collections::BTreeSet::new();
                    for attribute in &mut insert.attributes {
                        let retained = old.attributes.iter().find(|a| {
                            a.tag.eq_ignore_ascii_case(&attribute.tag)
                                && !used.contains(&a.common.handle)
                        });
                        attribute.common.handle = retained
                            .map(|a| a.common.handle)
                            .unwrap_or_else(|| scene.document.allocate_handle());
                        attribute.common.owner_handle = to;
                        attribute.common.xdictionary_handle =
                            retained.and_then(|a| a.common.xdictionary_handle);
                        attribute.common.raw_record = None;
                        used.insert(attribute.common.handle);
                    }
                    insert.seqend_handle = if insert.attributes.is_empty() {
                        None
                    } else {
                        old.seqend_handle
                            .or_else(|| Some(scene.document.allocate_handle()))
                    };
                    for attribute in &old.attributes {
                        if !used.contains(&attribute.common.handle) {
                            scene.document.remove_entity_arc(attribute.common.handle);
                        }
                    }
                }
                (EntityType::Polyline3D(poly), EntityType::Polyline3D(old)) => {
                    for (i, v) in poly.vertices.iter_mut().enumerate() {
                        v.handle = old
                            .vertices
                            .get(i)
                            .map(|v| v.handle)
                            .unwrap_or_else(|| scene.document.allocate_handle());
                    }
                }
                _ => {}
            }
            entity.common_mut().xdictionary_handle = current.xdictionary_handle;
            entity.common_mut().reactors = current.reactors.clone();
            if crate::scene::block_visibility::parameter(&scene.document, def)
                .is_some_and(|p| p.all_blocks.contains(&from))
            {
                entity.common_mut().invisible = current.invisible;
            }
        } else {
            Scene::reset_clone_subhandles(&mut scene.document, &mut entity);
            if let EntityType::Insert(insert) = &mut entity {
                for attribute in &mut insert.attributes {
                    attribute.common.owner_handle = to;
                }
                if insert.seqend_handle.is_some() || !insert.attributes.is_empty() {
                    insert.seqend_handle = Some(scene.document.allocate_handle());
                }
            }
            entity.common_mut().xdictionary_handle = None;
            entity.common_mut().reactors.clear();
        }
        entity.common_mut().handle = to;
        entity.common_mut().owner_handle = rep;
        if crate::scene::block_construction::is_construction(entity.common()) { entity.common_mut().invisible = true; }
        entity.common_mut().raw_record = None;
        if existing {
            *scene.document.get_entity_mut(to).unwrap() = entity;
        } else {
            scene
                .document
                .add_entity(entity)
                .map_err(|_| "Could not add a representation member.")?;
            if let Some(root) = source_xdict {
                let cloned = Scene::clone_xdict_tree(
                    &mut scene.document,
                    from,
                    root,
                    to,
                    &mapping,
                    &FxHashMap::default(),
                    &mut vec![],
                )
                .ok_or("Could not copy the new member's extension dictionary.")?;
                scene
                    .document
                    .get_entity_mut(to)
                    .unwrap()
                    .common_mut()
                    .xdictionary_handle = Some(cloned);
            }
        }
        let attributes = match scene.document.get_entity(to) {
            Some(EntityType::Insert(i)) => i.attributes.clone(),
            _ => vec![],
        };
        for attribute in attributes {
            if let Some(stored) = scene.document.get_entity_mut(attribute.common.handle) {
                *stored = EntityType::AttributeEntity(attribute);
            }
        }
        authoring::set_entity_tag(&mut scene.document, to, authoring::element_tag(id, to));
        ordered.push(to);
    }
    for h in old {
        if !ordered.contains(&h) {
            let attrs = match scene.document.get_entity(h) {
                Some(EntityType::Insert(i)) => i
                    .attributes
                    .iter()
                    .map(|a| a.common.handle)
                    .collect::<Vec<_>>(),
                _ => vec![],
            };
            for attr in attrs {
                scene.document.remove_entity_arc(attr);
            }
            scene.document.remove_entity_arc(h);
        }
    }
    scene
        .document
        .block_records
        .get_mut(name)
        .unwrap()
        .entity_handles = ordered;
    // Keep the anonymous block's native base point in step with its definition.
    let source_name = scene
        .document
        .block_records
        .iter()
        .find(|b| b.handle == def)
        .unwrap()
        .name
        .clone();
    let base = crate::scene::render_graph::block_base_point(&scene.document, &source_name);
    let marker = scene
        .document
        .block_records
        .get(name)
        .unwrap()
        .block_entity_handle;
    if let Some(EntityType::Block(block)) = scene.document.get_entity_mut(marker) {
        block.base_point = base;
    }
    scene
        .document
        .block_records
        .get_mut(name)
        .unwrap()
        .base_point = base;
    Ok(())
}

fn state_name(doc: &Document, def: Handle, insert: Handle) -> Option<(String, usize)> {
    let parameter = crate::scene::block_visibility::parameter(doc, def)?;
    let EntityType::Insert(reference) = doc.get_entity(insert)? else {
        return None;
    };
    let rep = doc.block_records.get(&reference.block_name)?;
    let by_id = rep
        .entity_handles
        .iter()
        .filter_map(|h| {
            let e = doc.get_entity(*h)?;
            Some((authoring::element_id(e)?, !e.common().invisible))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    parameter.states.iter().enumerate().find_map(|(i, state)| {
        let matches = parameter.all_blocks.iter().all(|h| {
            doc.get_entity(*h)
                .and_then(authoring::element_id)
                .and_then(|id| by_id.get(&id))
                .is_some_and(|shown| *shown == state.visible_blocks.contains(h))
        });
        matches.then(|| (state.name.clone(), i))
    })
}

pub(super) fn visibility_state(
    doc: &Document,
    before: &Document,
    def: Handle,
    insert: Handle,
) -> Option<usize> {
    let current = crate::scene::block_visibility::parameter(doc, def)?;
    let (name, index) = state_name(before, def, insert).or_else(|| state_name(doc, def, insert))?;
    Some(
        current
            .states
            .iter()
            .position(|s| s.name == name)
            .unwrap_or(index.min(current.states.len().saturating_sub(1))),
    )
}

pub(super) fn apply_visibility(
    doc: &mut Document,
    def: Handle,
    insert: Handle,
    state: usize,
) -> Result<(), &'static str> {
    let parameter = crate::scene::block_visibility::parameter(doc, def)
        .ok_or("Missing visibility parameter.")?;
    let state = parameter
        .states
        .get(state)
        .ok_or("Missing visibility state.")?;
    let flags = parameter
        .all_blocks
        .iter()
        .filter_map(|h| {
            Some((
                authoring::element_id(doc.get_entity(*h)?)?,
                !state.visible_blocks.contains(h),
            ))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let EntityType::Insert(reference) = doc.get_entity(insert).ok_or("Missing reference.")? else {
        return Err("Missing reference.");
    };
    let members = doc
        .block_records
        .get(&reference.block_name)
        .ok_or("Missing representation.")?
        .entity_handles
        .clone();
    for h in members {
        let id = doc.get_entity(h).and_then(authoring::element_id);
        if let Some(hidden) = id.and_then(|id| flags.get(&id)) {
            let entity = doc.get_entity_mut(h).unwrap();
            entity.common_mut().invisible = *hidden || crate::scene::block_construction::is_construction(entity.common());
        }
    }
    Ok(())
}
