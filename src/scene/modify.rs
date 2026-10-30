// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;

/// Orientation fields of a text-like entity, captured so a mirror can keep the
/// glyphs right-reading when MIRRTEXT (`header.mirror_text`) is off.
struct TextOrient {
    rotation: f64,
    oblique: f64,
    x_scale: f64,
}

pub(crate) fn inverse_affine(
    transform: &codec::types::Transform,
) -> Option<codec::types::Transform> {
    use codec::types::{Matrix3, Matrix4, Transform, Vector3};
    let matrix = &transform.matrix.m;
    let linear = Matrix3::from_rows(
        [matrix[0][0], matrix[0][1], matrix[0][2]],
        [matrix[1][0], matrix[1][1], matrix[1][2]],
        [matrix[2][0], matrix[2][1], matrix[2][2]],
    );
    let inverse = linear.inverse()?;
    let translation = Vector3::new(matrix[0][3], matrix[1][3], matrix[2][3]);
    let inverse_translation = inverse * (translation * -1.0);
    Some(Transform::from_matrix(Matrix4 {
        m: [
            [
                inverse.m[0][0],
                inverse.m[0][1],
                inverse.m[0][2],
                inverse_translation.x,
            ],
            [
                inverse.m[1][0],
                inverse.m[1][1],
                inverse.m[1][2],
                inverse_translation.y,
            ],
            [
                inverse.m[2][0],
                inverse.m[2][1],
                inverse.m[2][2],
                inverse_translation.z,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ],
    }))
}

/// Snapshot the orientation of a text / mtext / shape entity, or `None` for any
/// other kind.
fn capture_text_orient(e: &EntityType) -> Option<TextOrient> {
    match e {
        EntityType::Text(t) => Some(TextOrient {
            rotation: t.rotation,
            oblique: t.oblique_angle,
            x_scale: 0.0,
        }),
        EntityType::MText(m) => Some(TextOrient {
            rotation: m.rotation,
            oblique: 0.0,
            x_scale: 0.0,
        }),
        EntityType::Shape(s) => Some(TextOrient {
            rotation: s.rotation,
            oblique: s.oblique_angle,
            x_scale: s.relative_x_scale,
        }),
        _ => None,
    }
}

/// Restores readable text orientation after a mirror.
fn restore_text_orient(e: &mut EntityType, o: &TextOrient) {
    match e {
        EntityType::Text(t) => {
            t.rotation = o.rotation;
            t.oblique_angle = o.oblique;
            use codec::entities::TextHorizontalAlignment as HA;
            t.horizontal_alignment = match t.horizontal_alignment {
                HA::Left => HA::Right,
                HA::Right => HA::Left,
                other => other,
            };
            crate::entities::text::sync_text_alignment_point(t);
        }
        EntityType::MText(m) => {
            m.rotation = o.rotation;
        }
        EntityType::Shape(s) => {
            s.rotation = o.rotation;
            s.oblique_angle = o.oblique;
            s.relative_x_scale = o.x_scale;
        }
        _ => {}
    }
}

/// MIRRTEXT on: a real MIRROR makes text a true glyph mirror. `apply_transform`
/// already reflects the baseline rotation to `2·axis − rot`; the reflected glyph
/// frame is that rotation plus 180° with the backward bit (`0x2`) toggled, which
/// is what AutoCAD writes (upside-down `0x4` untouched). Toggling — not setting —
/// keeps a double mirror an involution. Only single-line TEXT carries group 71;
/// MTEXT / Shape keep the reflected rotation (readable-but-rotated), which is
/// standard-conforming.
fn mirror_true_text_flags(e: &mut EntityType) {
    if let EntityType::Text(t) = e {
        t.generation_flags ^= 0x2;
        t.rotation = (t.rotation + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU);
    }
}

/// Transform one entity of a dimension's `*D` block. Its text reads the
/// right way after a mirror, whatever MIRRTEXT says.
fn transform_dimension_part(entity: &mut EntityType, t: &EntityTransform) {
    let orient = matches!(t, EntityTransform::Mirror { .. })
        .then(|| capture_text_orient(entity))
        .flatten();
    view::dispatch::apply_transform(entity, t);
    if let Some(orient) = orient {
        restore_text_orient(entity, &orient);
    }
}

impl Scene {
    /// Link a leader and its annotation, including the annotation's persistent reactor.
    pub fn link_leader_annotation(&mut self, leader: Handle, annotation: Handle) -> bool {
        let old_annotation = match self.document.get_entity(leader) {
            Some(EntityType::Leader(value)) => value.annotation_handle,
            _ => return false,
        };
        if annotation == leader || self.document.get_entity(annotation).is_none() {
            return false;
        }
        let mut touched = vec![leader, annotation];
        if old_annotation != annotation && self.document.get_entity(old_annotation).is_some() {
            touched.push(old_annotation);
        }
        if self.is_recording_undo() {
            for &handle in &touched {
                let before = self.document.get_entity_arc(handle);
                self.record_undo_before(handle, before);
            }
        }
        if old_annotation != annotation {
            if let Some(entity) = self.document.get_entity_mut(old_annotation) {
                entity
                    .common_mut()
                    .reactors
                    .retain(|&handle| handle != leader);
            }
        }
        if let Some(EntityType::Leader(value)) = self.document.get_entity_mut(leader) {
            value.annotation_handle = annotation;
        }
        let reactors = &mut self
            .document
            .get_entity_mut(annotation)
            .unwrap()
            .common_mut()
            .reactors;
        if !reactors.contains(&leader) {
            reactors.push(leader);
        }
        // The context may have been created before the annotation handle was known.
        self.sync_displayed_annotation_context(leader);
        let changes: Vec<_> = touched
            .into_iter()
            .map(|h| (h, ChangeKind::Modified))
            .collect();
        self.bump_entities(&changes);
        true
    }

    pub(crate) fn sync_displayed_annotation_context(&mut self, handle: Handle) -> bool {
        let scale = self.displayed_annotation_scale_handle();
        crate::scene::annotative::sync_annotation_context_from_entity(
            &mut self.document,
            handle,
            scale,
        )
    }

    /// Invalidate a dimension's baked block inside an edit.
    ///
    /// Only the reference goes: the old picture stays a block of its own,
    /// which an undo (recorded here, or a grip edit's own snapshot) points
    /// back to, and the next save drops it once nothing names it.
    pub fn invalidate_dim_block_recorded(&mut self, handle: Handle) {
        let Some(before) = self.document.get_entity_arc(handle) else {
            return;
        };
        if !matches!(&*before, EntityType::Dimension(d) if !d.base().block_name.is_empty()) {
            return;
        }
        if self.is_recording_undo() {
            self.record_undo_before(handle, Some(before));
        }
        if let Some(EntityType::Dimension(d)) = self.document.get_entity_mut(handle) {
            d.base_mut().block_name.clear();
        }
    }

    // ── Modify (transform / copy) ─────────────────────────────────────────

    pub fn transform_entities(&mut self, handles: &[Handle], t: &EntityTransform) {
        // Never transform objects on a locked layer (defense-in-depth: the pick
        // path already excludes them, but programmatic callers may not).
        let handles: Vec<Handle> = handles
            .iter()
            .copied()
            .filter(|&h| !self.is_layer_locked(h))
            .collect();
        let handles = &handles[..];
        let selection: rustc_hash::FxHashSet<Handle> = handles.iter().copied().collect();
        // The only consumer of these pre-edit images is the parametric solve:
        // they feed its retained sizes and its collapse refusal, and the solve
        // runs only when a constraint scope exists. Without one, skip the
        // clones entirely.
        let has_constraints = self
            .parametric_constraints
            .iter()
            .any(|set| !set.constraints.is_empty());
        let retained_originals: Vec<(Handle, EntityType)> = if has_constraints {
            handles
                .iter()
                .filter_map(|handle| {
                    self.document
                        .get_entity(*handle)
                        .cloned()
                        .map(|entity| (*handle, entity))
                })
                .collect()
        } else {
            Vec::new()
        };
        let transformed_refs: Vec<_> = if has_constraints {
            handles
                .iter()
                .copied()
                .map(parametric_constraints::ParametricRef::whole)
                .collect()
        } else {
            Vec::new()
        };
        let mut driven_refs = Vec::new();
        if has_constraints && !matches!(t, EntityTransform::Translate(_)) {
            let mut seen_driven = rustc_hash::FxHashSet::default();
            for set in &self.parametric_constraints {
                for constraint in &set.constraints {
                    if !constraint.enabled
                        || !matches!(
                            constraint.kind,
                            parametric_constraints::ConstraintKind::Parallel
                                | parametric_constraints::ConstraintKind::Perpendicular
                                | parametric_constraints::ConstraintKind::Colinear
                        )
                    {
                        continue;
                    }
                    let selected_refs = constraint
                        .refs
                        .iter()
                        .filter(|reference| selection.contains(&reference.entity))
                        .count();
                    if selected_refs == 0 || selected_refs == constraint.refs.len() {
                        continue;
                    }
                    for reference in constraint
                        .refs
                        .iter()
                        .copied()
                        .filter(|reference| !selection.contains(&reference.entity))
                    {
                        let anchor = if let Some(index) = reference.segment_index() {
                            parametric_constraints::ParametricRef::point(
                                reference.entity,
                                index as i32,
                            )
                        } else if reference.directional_axis().is_some()
                            && matches!(
                                self.document.get_entity(reference.entity),
                                Some(EntityType::Ellipse(_))
                            )
                        {
                            parametric_constraints::ParametricRef::center(reference.entity)
                        } else {
                            parametric_constraints::ParametricRef::point(reference.entity, 0)
                        };
                        let follows_selected_point = set.constraints.iter().any(|link| {
                            link.enabled
                                && link.kind == parametric_constraints::ConstraintKind::Coincident
                                && link.refs.contains(&anchor)
                                && link
                                    .refs
                                    .iter()
                                    .any(|item| selection.contains(&item.entity))
                        });
                        if !follows_selected_point && seen_driven.insert(anchor) {
                            driven_refs.push(anchor);
                        }
                    }
                }
            }
        }
        // Preserve readable text orientation when MIRRTEXT is disabled.
        let preserve_text_orientation =
            matches!(t, EntityTransform::Mirror { .. }) && !self.document.header.mirror_text;
        // Toggle group-71 flags when glyph mirroring is enabled.
        let mirror_true =
            matches!(t, EntityTransform::Mirror { .. }) && self.document.header.mirror_text;
        let mut text_orient_backup: Vec<(Handle, TextOrient)> = Vec::new();
        if preserve_text_orientation {
            for &h in handles {
                if let Some(entity) = self.document.get_entity(h) {
                    if let Some(o) = capture_text_orient(entity) {
                        text_orient_backup.push((h, o));
                    }
                }
            }
        }
        // A dimension's final geometry is baked into a per-instance `*D`
        // block, and the render draws those sub-entities directly (not the
        // definition points). Transform them with the dimension, or it would
        // stay drawn in place while only its def points move.
        // Turning or mirroring a dimension lays its text out again (it keeps
        // reading upright), which moving the baked picture can't do: those
        // detach the picture, so the dimension draws from its definition and
        // the next save bakes it anew.
        let relayout = matches!(
            t,
            EntityTransform::Rotate { .. }
                | EntityTransform::Mirror { .. }
                | EntityTransform::Affine(_)
        );
        let dim_block_subs: Vec<(Handle, Option<EntityTransform>)> = handles
            .iter()
            .filter(|_| !relayout)
            .filter_map(|&h| match self.document.get_entity(h) {
                Some(entity @ EntityType::Dimension(d)) => {
                    let bn = d.base().block_name.clone();
                    if bn.trim().is_empty() {
                        None
                    } else {
                        Some((bn, render_graph::dim_picture_placement(entity)))
                    }
                }
                _ => None,
            })
            .filter_map(|(bn, placement)| {
                self.document
                    .block_records
                    .iter()
                    .find(|br| br.name.eq_ignore_ascii_case(&bn))
                    .map(|br| {
                        br.entity_handles
                            .iter()
                            .map(|&sh| (sh, placement.clone()))
                            .collect::<Vec<_>>()
                    })
            })
            .flatten()
            .collect();
        let mut refresh_solid_handles = Vec::new();
        for &h in handles {
            // Delta-undo: capture the pre-transform image before mutating.
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(h);
                self.record_undo_before(h, before);
            }
            if let Some(entity) = self.document.get_entity_mut(h) {
                view::dispatch::apply_transform(entity, t);
                render_graph::clear_dim_picture_placement(entity);
                if mirror_true {
                    mirror_true_text_flags(entity);
                }
            }
            // Only the reference goes: the old picture stays a block of its
            // own, which an undo points back to, and is not saved again.
            if relayout {
                if let Some(EntityType::Dimension(d)) = self.document.get_entity_mut(h) {
                    d.base_mut().block_name.clear();
                }
            }
            let rebuilt_history = self.transform_solid_history(h, t);
            if !rebuilt_history
                && self.document.get_entity(h).is_some_and(|entity| {
                    matches!(
                        entity,
                        EntityType::Solid3D(_)
                            | EntityType::Region(_)
                            | EntityType::Body(_)
                            | EntityType::Surface(_)
                            | EntityType::Mesh(_)
                            | EntityType::PolygonMesh(_)
                            | EntityType::PolyfaceMesh(_)
                    )
                })
            {
                refresh_solid_handles.push(h);
            }
            if self.hatches.contains_key(&h) {
                let existing_color = self.hatches[&h].color;
                let new_model = match self.document.get_entity(h) {
                    Some(EntityType::Hatch(dxf)) => Self::hatch_model_from_dxf(dxf, existing_color),
                    // A DXF SOLID renders as a solid-fill hatch; rebuild it from
                    // the moved corners so the fill follows the transform.
                    Some(EntityType::Solid(s)) => Some(Self::solid_hatch_model(s, existing_color)),
                    _ => None,
                };
                if let Some(model) = new_model {
                    self.hatches.insert(h, model);
                }
            }
            let image_bearing = self.document.get_entity(h).is_some_and(|entity| {
                matches!(
                    entity,
                    EntityType::RasterImage(_) | EntityType::Ole2Frame(_) | EntityType::Underlay(_)
                )
            });
            if image_bearing {
                let model = self
                    .document
                    .get_entity(h)
                    .and_then(|entity| self.image_seed_for(entity));
                self.images.remove(&h);
                if let Some(model) = model {
                    self.images.insert(h, model);
                }
            }
        }
        if preserve_text_orientation {
            for (h, o) in text_orient_backup {
                if let Some(entity) = self.document.get_entity_mut(h) {
                    restore_text_orient(entity, &o);
                }
            }
        }
        for &h in handles {
            let others =
                crate::scene::annotative::transform_annotation_contexts(&mut self.document, h, t);
            if self.sync_displayed_annotation_context(h) || others {
                self.poison_undo_recording();
            }
        }
        // Move the baked dimension-block sub-entities too (collected above).
        // These are entities not named in the reported change set, so a delta
        // must capture their before-images here or a dimension move won't undo.
        for (h, placement) in &dim_block_subs {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(*h);
                self.record_undo_before(*h, before);
            }
            if let Some(entity) = self.document.get_entity_mut(*h) {
                if let Some(placement) = placement {
                    view::dispatch::apply_transform(entity, placement);
                }
                transform_dimension_part(entity, t);
            }
        }
        // The moved `*D` contents are a block definition the block cache holds;
        // without a fresh block epoch a reopened drawing's dimensions stay
        // drawn where they were until REGEN. (#1342)
        if !dim_block_subs.is_empty() {
            self.block_epoch =
                super::GEOMETRY_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        // Only the transformed entities changed (a top-level move/rotate/scale/
        // mirror never edits a block definition) — report just those so the
        // resident set re-tessellates only them and every derived cache patches
        // per-handle, keeping the block cache + all other memoized wires.
        let changes: Vec<(Handle, ChangeKind)> =
            handles.iter().map(|&h| (h, ChangeKind::Modified)).collect();
        self.bump_entities_with_parametric_transform_policy(
            &changes,
            &driven_refs,
            &transformed_refs,
            &retained_originals,
        );
        self.refresh_meshes_for_handles(&refresh_solid_handles);
    }

    /// Entities in anonymous dimension blocks that visually belong to
    /// dimensions inside `block_record`. They must follow a block-coordinate
    /// reframe and be included in BEDIT's discard snapshot.
    pub(crate) fn block_definition_dependent_handles(&self, block_record: Handle) -> Vec<Handle> {
        let Some(record) = self
            .document
            .block_records
            .iter()
            .find(|record| record.handle == block_record)
        else {
            return Vec::new();
        };
        let root_handles: HashSet<Handle> = record.entity_handles.iter().copied().collect();
        let mut result = Vec::new();
        let mut seen = HashSet::default();
        for handle in &record.entity_handles {
            let Some(EntityType::Dimension(dimension)) = self.document.get_entity(*handle) else {
                continue;
            };
            let name = dimension.base().block_name.trim();
            if name.is_empty() {
                continue;
            }
            let Some(dependent) = self
                .document
                .block_records
                .iter()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(name))
            else {
                continue;
            };
            for dependent_handle in &dependent.entity_handles {
                if !root_handles.contains(dependent_handle) && seen.insert(*dependent_handle) {
                    result.push(*dependent_handle);
                }
            }
        }
        result
    }

    /// Bake a transient BEDIT UCS into one block definition.
    ///
    /// `local_from_old` maps the currently stored block coordinates into the
    /// newly selected UCS frame. No UCS record is attached to the block: the
    /// content is transformed and remains canonical around identity/zero.
    pub fn reframe_block_definition(
        &mut self,
        block_record: Handle,
        local_from_old: &codec::types::Transform,
    ) -> usize {
        let Some(record) = self
            .document
            .block_records
            .iter()
            .find(|record| record.handle == block_record)
        else {
            return 0;
        };
        let block_name = record.name.clone();
        let root_handles: HashSet<Handle> = record.entity_handles.iter().copied().collect();
        let dependent_handles = self.block_definition_dependent_handles(block_record);
        let mut handles = record.entity_handles.clone();
        handles.extend(dependent_handles.iter().copied());
        let owned: HashSet<Handle> = handles.iter().copied().collect();
        let transform = EntityTransform::Affine(*local_from_old);
        let matrix = &local_from_old.matrix.m;
        let dependent_transform = EntityTransform::Affine(codec::types::Transform::from_matrix(
            codec::types::Matrix4 {
                m: [
                    [matrix[0][0], matrix[0][1], matrix[0][2], 0.0],
                    [matrix[1][0], matrix[1][1], matrix[1][2], 0.0],
                    [matrix[2][0], matrix[2][1], matrix[2][2], 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            },
        ));
        let mut changed = Vec::new();

        for handle in handles {
            let is_structural = self.document.get_entity(handle).is_some_and(|entity| {
                matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_))
            });
            if is_structural {
                continue;
            }
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(handle);
                self.record_undo_before(handle, before);
            }
            if let Some(entity) = self.document.get_entity_mut(handle) {
                // A nested dimension's anonymous `*D` picture is placed by
                // adding the dimension insertion point. That point already
                // receives the affine translation with the root dimension, so
                // its picture receives only the new basis here; applying the
                // translation twice would shift dimensions on origin changes.
                let entity_transform = if root_handles.contains(&handle) {
                    &transform
                } else {
                    &dependent_transform
                };
                view::dispatch::apply_transform(entity, entity_transform);
                changed.push(handle);
            }
        }

        let old_base = crate::scene::render_graph::block_base_point(&self.document, &block_name);
        let basepoint_changes =
            super::block_basepoint::reframe(&mut self.document, block_record, local_from_old);
        // ATTRIBs live inline on each INSERT in that reference's owner space.
        // Conjugate the block-local reframe through the INSERT transform:
        // owner' = M_new · local_from_old · M_old⁻¹ · owner. A native
        // Basepoint follows the frame too, so the two maps can differ.
        // M is the block-local →
        // world map the render path uses (`insert_transform`), because that is
        // the map the attribute positions on the INSERT were placed through;
        // the codec's base-ignoring transform would conjugate them through a
        // displaced frame whenever the definition has a non-zero base point.
        let references: Vec<(Handle, codec::types::Transform)> = self
            .document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Insert(reference)
                    if reference.block_name.eq_ignore_ascii_case(&block_name)
                        && !reference.attributes.is_empty()
                        && !owned.contains(&reference.common.handle) =>
                {
                    let old_insertion =
                        crate::scene::render_graph::insert_transform_with_base(reference, old_base);
                    let insertion =
                        crate::scene::render_graph::insert_transform(&self.document, reference);
                    let inverse = inverse_affine(&old_insertion)?;
                    Some((
                        reference.common.handle,
                        codec::types::Transform::from_matrix(
                            insertion.matrix * local_from_old.matrix * inverse.matrix,
                        ),
                    ))
                }
                _ => None,
            })
            .collect();
        for (handle, attribute_transform) in references {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(handle);
                self.record_undo_before(handle, before);
            }
            if let Some(EntityType::Insert(reference)) = self.document.get_entity_mut(handle) {
                for attribute in &mut reference.attributes {
                    codec::Entity::apply_transform(attribute, &attribute_transform);
                }
                changed.push(handle);
            }
        }

        for handle in changed.iter().copied() {
            let _ = self.sync_displayed_annotation_context(handle);
        }
        let parameter_changes =
            super::block_visibility::reframe(&mut self.document, block_record, local_from_old)
                + super::block_point_parameter::reframe(
                    &mut self.document,
                    block_record,
                    local_from_old,
                );
        let parameter_changes = parameter_changes
            + super::block_distance_parameter::reframe(
                &mut self.document,
                block_record,
                local_from_old,
            );
        let parameter_changes = parameter_changes
            + super::block_orientation_parameter::reframe(
                &mut self.document,
                block_record,
                local_from_old,
            );
        let parameter_changes = parameter_changes
            + basepoint_changes
            + super::block_lookup::reframe(&mut self.document, block_record, local_from_old)
            + super::block_actions::reframe(&mut self.document, block_record, local_from_old);
        if !changed.is_empty() || parameter_changes > 0 {
            self.rebuild_derived_caches();
        }
        changed.len() + parameter_changes
    }

    /// Give a freshly-cloned entity brand-new handles for every *inline*
    /// sub-entity that stores one (INSERT attributes and SEQEND, 3D-polyline
    /// vertices, polyface vertices / faces / SEQEND, polygon-mesh vertices).
    /// `document.add_entity` only assigns the top-level handle, and the DWG
    /// writer reuses every non-null stored sub-handle, so without this a copy
    /// writes its source's sub-handles again — duplicate handles that corrupt
    /// the saved DWG (other CAD apps attach the copies to the source's
    /// vertices, or drop them). Vertices that don't store a handle
    /// (LwPolyline / heavy 2D polyline) get one from the writer, so they need
    /// no fix-up here. Shared with the clipboard paste path. (#129)
    pub(crate) fn reset_clone_subhandles(doc: &mut codec::Document, entity: &mut EntityType) {
        match entity {
            EntityType::Table(table) => {
                // A table exclusively owns its anonymous picture block. The
                // codec also resolves by name, so detach both references.
                table.block_record_handle = None;
                table.block_name.clear();
            }
            EntityType::Insert(ins) => {
                for att in ins.attributes.iter_mut() {
                    att.common.handle = doc.allocate_handle();
                }
                if ins.seqend_handle.is_some() {
                    ins.seqend_handle = Some(doc.allocate_handle());
                }
            }
            EntityType::Polyline3D(p) => {
                for v in p.vertices.iter_mut() {
                    v.handle = doc.allocate_handle();
                }
            }
            EntityType::PolyfaceMesh(m) => {
                for v in m.vertices.iter_mut() {
                    v.common.handle = doc.allocate_handle();
                }
                for f in m.faces.iter_mut() {
                    f.common.handle = doc.allocate_handle();
                }
                if m.seqend_handle.is_some() {
                    m.seqend_handle = Some(doc.allocate_handle());
                }
            }
            EntityType::PolygonMesh(m) => {
                for v in m.vertices.iter_mut() {
                    v.common.handle = doc.allocate_handle();
                }
            }
            _ => {}
        }
    }

    /// Give each block-member copy (and each inline ATTRIB of a copied INSERT)
    /// its own deep copy of the source's extension dictionary tree, owned by
    /// the copy, with reactors and handles inside the tree remapped, as COPY
    /// does. `originals` and `copies` are parallel lists.
    pub(super) fn own_copied_xdicts(&mut self, originals: Vec<Handle>, copies: Vec<Handle>) {
        // Include inline ATTRIBs: define_block_raw gives them new handles too.
        let mut pairs = Vec::new();
        for (from, to) in originals.into_iter().zip(copies) {
            let (Some(source), Some(target)) =
                (self.document.get_entity(from), self.document.get_entity(to))
            else {
                continue;
            };
            pairs.push((source.common().clone(), to, None));
            if let (EntityType::Insert(source), EntityType::Insert(target)) = (source, target) {
                pairs.extend(
                    source
                        .attributes
                        .iter()
                        .zip(&target.attributes)
                        .enumerate()
                        .map(|(k, (a, b))| (a.common.clone(), b.common.handle, Some((to, k)))),
                );
            }
        }
        let mapping: rustc_hash::FxHashMap<_, _> = pairs
            .iter()
            .map(|(common, to, _)| (common.handle, *to))
            .collect();
        let scales = pairs
            .iter()
            .map(|(common, _, attribute)| {
                if attribute.is_some() {
                    Vec::new()
                } else {
                    super::annotative::annotation_scale_handles_for_entity(
                        &self.document,
                        common.handle,
                    )
                }
            })
            .collect::<Vec<_>>();
        let roots = pairs
            .iter()
            .zip(&scales)
            .map(|((common, _, _), scales)| {
                (
                    common.handle,
                    if scales.is_empty() {
                        common.xdictionary_handle
                    } else {
                        None
                    },
                )
            })
            .collect::<Vec<_>>();
        let (cross, owner_of) = self.prepare_xdict_copies(&roots);
        let mut created = Vec::new();
        // Block records and cloned objects require full-snapshot undo, like COPY.
        self.poison_undo_recording();
        for ((common, to, attribute), scales) in pairs.iter().zip(scales) {
            let root = if scales.is_empty() {
                common.xdictionary_handle.and_then(|root| {
                    Self::clone_xdict_tree(
                        &mut self.document,
                        common.handle,
                        root,
                        *to,
                        &cross,
                        &owner_of,
                        &mut created,
                    )
                })
            } else {
                None
            };
            if let Some((parent, k)) = attribute {
                if let Some(EntityType::Insert(insert)) = self.document.get_entity_mut(*parent) {
                    if let Some(attribute) = insert.attributes.get_mut(*k) {
                        attribute.common.xdictionary_handle = root;
                        attribute.common.owner_handle = *parent;
                        attribute.common.reactors = common
                            .reactors
                            .iter()
                            .filter_map(|h| mapping.get(h).copied())
                            .collect();
                    }
                }
            } else if let Some(entity) = self.document.get_entity_mut(*to) {
                entity.common_mut().xdictionary_handle = root;
            }
            for scale in scales {
                super::annotative::create_annotation_context(&mut self.document, *to, scale);
            }
        }
        remap_copied_reactors(&mut self.document, &mapping);
        remap_copied_xdict_cross_refs(&mut self.document, &mapping, &created);
    }

    /// Preallocate COPY's object-tree mapping before cloning any sibling tree.
    pub(super) fn prepare_xdict_copies(
        &mut self,
        roots: &[(Handle, Option<Handle>)],
    ) -> (
        rustc_hash::FxHashMap<Handle, Handle>,
        rustc_hash::FxHashMap<Handle, Handle>,
    ) {
        let mut owner_of = rustc_hash::FxHashMap::default();
        let mut cross = rustc_hash::FxHashMap::default();
        for (src_handle, src_xdict) in roots {
            if let Some(root) = *src_xdict {
                owner_of.insert(root, *src_handle);
            }
        }
        let mut claimed = rustc_hash::FxHashSet::default();
        for (src_handle, src_xdict) in roots {
            let Some(root) = *src_xdict else {
                continue;
            };
            let mut stack = vec![root];
            while let Some(h) = stack.pop() {
                if owner_of.get(&h).is_some_and(|&o| o != *src_handle) {
                    continue;
                }
                if !claimed.insert(h) {
                    continue;
                }
                owner_of.entry(h).or_insert(*src_handle);
                let Some(obj) = self.document.objects.get(&h) else {
                    continue;
                };
                match obj {
                    codec::objects::ObjectType::Dictionary(d) => {
                        for (_, child) in &d.entries {
                            stack.push(*child);
                        }
                        if let Some(xd) = d.xdictionary_handle {
                            stack.push(xd);
                        }
                    }
                    codec::objects::ObjectType::XRecord(x) => {
                        if let Some(xd) = x.xdictionary_handle {
                            stack.push(xd);
                        }
                    }
                    codec::objects::ObjectType::Field(f) => {
                        stack.extend(f.child_fields.iter().copied());
                    }
                    _ => {}
                }
            }
        }
        for &h in owner_of.keys() {
            cross.insert(h, self.document.allocate_handle());
        }
        (cross, owner_of)
    }

    /// Deep-clone the extension-dictionary tree rooted at `src_root` (owned by
    /// the entity `src_owner`) and return the clone's root, wired for
    /// `new_owner`. In-drawing COPY parity: the copy gets its own tree, so
    /// edits on it never touch the source. Dictionary entries, owners,
    /// reactors, nested xdictionary links, and XRecord handle references that
    /// land inside the tree are remapped onto fresh handles; references
    /// leaving the tree are preserved verbatim. A cyclic tree terminates via
    /// the visited set and remaps consistently. Atomic: nothing enters
    /// `document.objects` unless the whole tree clones; a missing or dangling
    /// root returns `None` so the caller leaves the copy without an extension
    /// dictionary — never shared.
    pub(super) fn clone_xdict_tree(
        doc: &mut codec::Document,
        src_owner: Handle,
        src_root: Handle,
        new_owner: Handle,
        // Objects reachable from every copied entity's tree, preallocated to
        // their authoritative clone handles; `owner_of` maps each such object
        // to the copied entity whose tree contains it.
        cross: &rustc_hash::FxHashMap<Handle, Handle>,
        owner_of: &rustc_hash::FxHashMap<Handle, Handle>,
        created: &mut Vec<Handle>,
    ) -> Option<Handle> {
        if src_root.is_null() {
            return None;
        }
        // Walk every object reachable through dictionary entries and
        // xdictionary links, in discovery order (root first). An object owned
        // by a sibling's tree is ref'd, never re-cloned here — the sibling's
        // own pass fills its preallocated handle.
        let mut order: Vec<Handle> = Vec::new();
        let mut index = rustc_hash::FxHashMap::default();
        let mut stack = vec![src_root];
        while let Some(h) = stack.pop() {
            if index.contains_key(&h) {
                continue;
            }
            if owner_of.get(&h).is_some_and(|&o| o != src_owner) {
                continue;
            }
            let Some(obj) = doc.objects.get(&h) else {
                if h == src_root {
                    return None;
                }
                continue;
            };
            index.insert(h, order.len());
            order.push(h);
            match obj {
                codec::objects::ObjectType::Dictionary(d) => {
                    for (_, child) in &d.entries {
                        stack.push(*child);
                    }
                    if let Some(xd) = d.xdictionary_handle {
                        stack.push(xd);
                    }
                }
                codec::objects::ObjectType::XRecord(x) => {
                    if let Some(xd) = x.xdictionary_handle {
                        stack.push(xd);
                    }
                }
                // A text field's nested fields belong to the same tree.
                codec::objects::ObjectType::Field(f) => {
                    stack.extend(f.child_fields.iter().copied());
                }
                _ => {}
            }
        }
        // Clone and remap onto fresh handles; insert only once the whole tree
        // is built so a mid-walk failure leaves the document untouched. An
        // object shared across the copy set lands on its preallocated handle.
        let mut remap = rustc_hash::FxHashMap::default();
        for &h in &order {
            let new_h = cross
                .get(&h)
                .copied()
                .unwrap_or_else(|| doc.allocate_handle());
            remap.insert(h, new_h);
        }
        let rewrite = |handle: Handle| {
            if handle == src_owner {
                new_owner
            } else if let Some(&n) = remap.get(&handle) {
                n
            } else {
                // A sibling tree object ref'd but never walked (e.g. an
                // XRecord handle entry); the sibling's pass clones it.
                *cross.get(&handle).unwrap_or(&handle)
            }
        };
        let mut clones = Vec::with_capacity(order.len());
        let mut representations = Vec::new();
        for &h in &order {
            let new_h = remap[&h];
            let obj = doc.objects.get(&h)?;
            let mut obj = obj.clone();
            match &mut obj {
                codec::objects::ObjectType::Dictionary(d) => {
                    d.handle = new_h;
                    d.owner = rewrite(d.owner);
                    for (_, child) in &mut d.entries {
                        *child = rewrite(*child);
                    }
                    d.xdictionary_handle = d.xdictionary_handle.map(rewrite);
                    for r in &mut d.reactors {
                        *r = rewrite(*r);
                    }
                }
                codec::objects::ObjectType::XRecord(x) => {
                    x.handle = new_h;
                    x.owner = rewrite(x.owner);
                    x.xdictionary_handle = x.xdictionary_handle.map(rewrite);
                    for r in &mut x.reactors {
                        *r = rewrite(*r);
                    }
                    for entry in &mut x.entries {
                        if let codec::objects::XRecordValue::Handle(h) = &mut entry.value {
                            *h = rewrite(*h);
                        }
                    }
                    for entry in &mut x.object_references {
                        entry.handle = rewrite(entry.handle);
                    }
                    // The raw DWG stream describes the source's object bytes;
                    // the clone regenerates its own from the remapped entries.
                    x.raw_dwg_data = None;
                    x.raw_dwg_handle_bits = 0;
                    x.raw_dwg_version = None;
                    x.preserve_object_reference_stream = false;
                }
                // A dynamic reference's AcDbRepData naming its definition.
                codec::objects::ObjectType::DynamicBlock(b) => {
                    b.handle = new_h;
                    b.owner = rewrite(b.owner);
                    for r in &mut b.reactors {
                        *r = rewrite(*r);
                    }
                    if let Some(&definition) = doc.block_representations.get(&h) {
                        representations.push((new_h, definition));
                    }
                }
                // A FIELD under ACAD_FIELD: the copy evaluates its own field
                // tree, so its handle, owner and nested fields are remapped.
                codec::objects::ObjectType::Field(f) => {
                    f.handle = new_h;
                    f.owner = rewrite(f.owner);
                    for child in &mut f.child_fields {
                        *child = rewrite(*child);
                    }
                    for r in &mut f.referenced_objects {
                        *r = rewrite(*r);
                    }
                }
                _ => {}
            }
            clones.push((new_h, obj));
        }
        for (new_h, obj) in clones {
            doc.objects.insert(new_h, obj);
            created.push(new_h);
        }
        doc.block_representations.extend(representations);
        Some(remap[&src_root])
    }

    /// Add a clone to the active space with fresh top-level and inline handles.
    /// Source owner handles are document-local and must not cross drawings.
    /// (#129, #934)
    pub fn add_entity_clone(&mut self, mut entity: EntityType) -> Handle {
        Self::reset_clone_subhandles(&mut self.document, &mut entity);
        let common = entity.common_mut();
        common.handle = Handle::NULL;
        common.owner_handle = Handle::NULL;
        // An extension dictionary is exclusively owned; the clone must not
        // claim the source's, or the copy dangles into a document that does
        // not own that object.
        common.xdictionary_handle = None;
        common.entity_mode = Some(
            if self.current_layout != "Model" && self.active_viewport.is_none() {
                1
            } else {
                2
            },
        );
        self.add_entity(entity)
    }

    /// Duplicate the anonymous block `src_name`, transforming every sub-entity
    /// by `t`, and return the new block's name. A dimension's drawn geometry
    /// lives in such a baked `*D` block, so a copied dimension needs its own
    /// transformed block — otherwise it still references the source block and
    /// renders on top of the original instead of at the copy. Returns None when
    /// the source block is missing or empty. (#161)
    fn clone_transformed_block(
        &mut self,
        src_name: &str,
        placement: Option<&EntityTransform>,
        t: &EntityTransform,
    ) -> Option<String> {
        let sub_handles = self
            .document
            .block_records
            .iter()
            .find(|br| br.name.eq_ignore_ascii_case(src_name))
            .map(|br| br.entity_handles.clone())?;
        let subs: Vec<EntityType> = sub_handles
            .iter()
            .filter_map(|&sh| self.document.get_entity(sh).cloned())
            .map(|mut sub| {
                if let Some(placement) = placement {
                    view::dispatch::apply_transform(&mut sub, placement);
                }
                sub
            })
            .collect();
        self.define_transformed_block(&subs, t)
    }

    /// Build a fresh anonymous `*D<n>` block from `subs` — a source block's
    /// sub-entities in its own (WCS-baked) coordinates — transforming each by
    /// `t`, and return the new block's name. Shared by the in-drawing copy
    /// (source block still lives in this document, via `clone_transformed_block`)
    /// and clipboard paste (source block snapshotted into the clipboard, so a
    /// pasted dimension gets its own transformed block cross-drawing too — see
    /// `finalize_paste`, #290). Returns None when `subs` is empty.
    pub(crate) fn define_transformed_block(
        &mut self,
        subs: &[EntityType],
        t: &EntityTransform,
    ) -> Option<String> {
        if subs.is_empty() {
            return None;
        }
        // Smallest free `*D<n>` anonymous name.
        let mut n = 0u64;
        let new_name = loop {
            let cand = format!("*D{n}");
            if self.document.block_records.get(&cand).is_none() {
                break cand;
            }
            n += 1;
        };
        let next = self.document.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);
        let mut br = codec::tables::BlockRecord::new(&new_name);
        br.handle = br_handle;
        br.block_entity_handle = block_handle;
        br.block_end_handle = end_handle;
        // The DWG writer takes the anonymous bit from the flag alone; without
        // it AutoCAD's audit reports `Invalid anonymous block "*D<n>"`.
        br.flags.anonymous = true;
        self.document.block_records.add(br).ok()?;
        let mut block = Block::new(&new_name, codec::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        self.document.add_entity(EntityType::Block(block)).ok()?;
        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::BlockEnd(block_end))
            .ok()?;
        for sub in subs {
            let mut sub = sub.clone();
            transform_dimension_part(&mut sub, t);
            Self::reset_clone_subhandles(&mut self.document, &mut sub);
            sub.common_mut().handle = Handle::NULL;
            sub.common_mut().owner_handle = br_handle;
            // An extension dictionary is exclusively owned; the clone must
            // not claim the source's, or file validation reports
            // "Extension dictionary <H> Cannot access".
            sub.common_mut().xdictionary_handle = None;
            self.ensure_image_definition(&mut sub);
            let _ = self.document.add_entity(sub);
        }
        Some(new_name)
    }

    pub fn copy_entities(&mut self, handles: &[Handle], t: &EntityTransform) -> Vec<Handle> {
        let copy_handles = self.handles_expanded_for_leader_annotations(handles);

        // Objects on a locked layer can be selected but not copied.
        let clones: Vec<(Handle, EntityType, Vec<Handle>, Option<Handle>)> = copy_handles
            .iter()
            .filter(|&&h| !self.is_layer_locked(h))
            .filter_map(|&h| {
                let mut entity = self.document.get_entity(h)?.clone();

                // Any annotative entity (not only a LEADER/MTEXT pair) needs
                // its own context tree; a shared one ties the copy to the
                // source. (#700)
                let annotation_scales =
                    crate::scene::annotative::annotation_scale_handles_for_entity(
                        &self.document,
                        h,
                    );

                // An annotative entity's extension dictionary is a context tree
                // recreated fresh on the copy; any other copy's tree is
                // deep-cloned after insertion, so the source root is captured
                // here and cleared before add_entity.
                let src_xdict = if annotation_scales.is_empty() {
                    entity.common().xdictionary_handle
                } else {
                    entity.common_mut().xdictionary_handle = None;
                    None
                };

                Some((h, entity, annotation_scales, src_xdict))
            })
            .collect();
        // MIRRTEXT also governs the copy path (default MIRROR keeps the source
        // and adds a mirrored copy): keep the copied text right-reading when the
        // header flag is off.
        let preserve_text_orientation =
            matches!(t, EntityTransform::Mirror { .. }) && !self.document.header.mirror_text;
        let mirror_true =
            matches!(t, EntityTransform::Mirror { .. }) && self.document.header.mirror_text;
        let mut new_handles = Vec::with_capacity(clones.len());
        let mut handle_map = rustc_hash::FxHashMap::default();
        let mut refresh_solid_handles = Vec::new();
        // Precompute the union of every copied entity's tree so a ref crossing
        // between two copied trees lands on the sibling's authoritative clone
        // instead of a private duplicate; objects outside the union are
        // deep-cloned inline (matching the reference product, which
        // deep-clones an out-of-set dictionary target rather than keeping
        // it verbatim). Each root belongs to its own entity even when a
        // sibling's tree refs it, and every other object is claimed by the
        // first traversal that reaches it, so ownership stays consistent
        // with the walker below regardless of selection order.
        let roots = clones
            .iter()
            .map(|(h, _, _, root)| (*h, *root))
            .collect::<Vec<_>>();
        let (cross, owner_of) = self.prepare_xdict_copies(&roots);
        let mut created_objects = Vec::new();
        for (src_handle, mut entity, annotation_scales, src_xdict) in clones {
            let text_orient = if preserve_text_orientation {
                capture_text_orient(&entity)
            } else {
                None
            };
            let picture_placement = render_graph::dim_picture_placement(&entity);
            view::dispatch::apply_transform(&mut entity, t);
            render_graph::clear_dim_picture_placement(&mut entity);
            if let Some(o) = &text_orient {
                restore_text_orient(&mut entity, o);
            }
            if mirror_true {
                mirror_true_text_flags(&mut entity);
            }
            // A dimension draws from its baked `*D` block; give the copy its own
            // transformed block so it lands at the copy position rather than
            // rendering on top of the source. (#161)
            if let EntityType::Dimension(d) = &entity {
                let bn = d.base().block_name.clone();
                if !bn.trim().is_empty() {
                    let placement = picture_placement.as_ref();
                    if let Some(new_bn) = self.clone_transformed_block(&bn, placement, t) {
                        // Delta-undo: a copied dimension adds a fresh anonymous
                        // *D block record — non-entity state a pure-entity delta
                        // can't restore. Poison so the app keeps a full snapshot.
                        self.poison_undo_recording();
                        if let EntityType::Dimension(d) = &mut entity {
                            d.base_mut().block_name = new_bn;
                        }
                    }
                }
            }
            // An attribute's own extension dictionary (a field ATTRIB keeps
            // its FIELD there) is cloned for the copy like the entity's; the
            // source handles are read before the attributes get new ones.
            let attribute_xdicts: Vec<(usize, Handle, Handle)> = match &mut entity {
                EntityType::Insert(ins) => ins
                    .attributes
                    .iter_mut()
                    .enumerate()
                    .filter_map(|(k, att)| {
                        let root = att.common.xdictionary_handle.take()?;
                        Some((k, att.common.handle, root))
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Self::reset_clone_subhandles(&mut self.document, &mut entity);

            // The source root was captured above; the clone must never carry
            // the source's handle through add_entity.
            entity.common_mut().xdictionary_handle = None;

            entity.common_mut().handle = Handle::NULL;
            self.ensure_image_definition(&mut entity);
            let h = self.document.add_entity(entity).unwrap_or(Handle::NULL);
            if !h.is_null() {
                // Deep-clone the extension-dictionary tree so edits on the
                // copy never touch the source. A dangling or missing root
                // leaves the copy without one — never shared.
                if let Some(src_root) = src_xdict {
                    if let Some(new_root) = Self::clone_xdict_tree(
                        &mut self.document,
                        src_handle,
                        src_root,
                        h,
                        &cross,
                        &owner_of,
                        &mut created_objects,
                    ) {
                        if let Some(clone) = self.document.get_entity_mut(h) {
                            clone.common_mut().xdictionary_handle = Some(new_root);
                        }
                        // The cloned tree lives in document.objects — outside
                        // a pure-entity delta; keep undo on the full snapshot.
                        self.poison_undo_recording();
                    }
                }
                for (k, src_attribute, src_root) in attribute_xdicts {
                    let Some(new_attribute) = (match self.document.get_entity(h) {
                        Some(EntityType::Insert(ins)) => {
                            ins.attributes.get(k).map(|a| a.common.handle)
                        }
                        _ => None,
                    }) else {
                        continue;
                    };
                    let Some(new_root) = Self::clone_xdict_tree(
                        &mut self.document,
                        src_attribute,
                        src_root,
                        new_attribute,
                        &cross,
                        &owner_of,
                        &mut created_objects,
                    ) else {
                        continue;
                    };
                    if let Some(EntityType::Insert(ins)) = self.document.get_entity_mut(h) {
                        ins.attributes[k].common.xdictionary_handle = Some(new_root);
                    }
                    self.poison_undo_recording();
                }
                if !annotation_scales.is_empty() {
                    for scale_handle in annotation_scales {
                        crate::scene::annotative::create_annotation_context(
                            &mut self.document,
                            h,
                            scale_handle,
                        );
                    }

                    // Annotation contexts add dictionary/object records outside the entity
                    // delta itself, so keep undo on the safe full-snapshot path.
                    if self.is_recording_undo() {
                        self.poison_undo_recording();
                    }
                }
                // Delta-undo: a copy's before-image is "nothing" (undo erases it).
                if self.is_recording_undo() {
                    self.record_undo_before(h, None);
                }
                let new_model = match self.document.get_entity(h) {
                    Some(EntityType::Hatch(dxf)) => {
                        let color = convert::tess_util::aci_to_rgba(&dxf.common.color);
                        Self::hatch_model_from_dxf(dxf, color)
                    }
                    Some(EntityType::Solid(s)) => {
                        let color = convert::tess_util::aci_to_rgba(&s.common.color);
                        Some(Self::solid_hatch_model(s, color))
                    }
                    _ => None,
                };
                if let Some(model) = new_model {
                    self.hatches.insert(h, model);
                }
                // Images draw from a derived model, not the wire frame; without
                // one the copy shows only its frame. (#829)
                let image = self.document.get_entity(h).and_then(|entity| {
                    matches!(
                        entity,
                        EntityType::RasterImage(_)
                            | EntityType::Ole2Frame(_)
                            | EntityType::Underlay(_)
                    )
                    .then(|| self.image_seed_for(entity))
                    .flatten()
                });
                if let Some(model) = image {
                    self.images.insert(h, model);
                }
                let rebuilt_history =
                    self.copy_solid_history(src_handle, h) && self.transform_solid_history(h, t);
                if !rebuilt_history
                    && self.document.get_entity(h).is_some_and(|entity| {
                        matches!(
                            entity,
                            EntityType::Solid3D(_)
                                | EntityType::Region(_)
                                | EntityType::Body(_)
                                | EntityType::Surface(_)
                        )
                    })
                {
                    refresh_solid_handles.push(h);
                }
            }
            new_handles.push(h);
            if !h.is_null() {
                handle_map.insert(src_handle, h);
            }
        }
        // A copied LEADER must reference the copied annotation, never the
        // source annotation. Both entities now exist, so remap the stored handle.
        let leader_links: Vec<(Handle, Handle)> = handle_map
            .iter()
            .filter_map(|(&source_handle, &copied_handle)| {
                let EntityType::Leader(source_leader) = self.document.get_entity(source_handle)?
                else {
                    return None;
                };

                let copied_annotation = handle_map
                    .get(&source_leader.annotation_handle)
                    .copied()
                    .unwrap_or(Handle::NULL);

                Some((copied_handle, copied_annotation))
            })
            .collect();

        for (leader_handle, annotation_handle) in leader_links {
            if let Some(EntityType::Leader(leader)) = self.document.get_entity_mut(leader_handle) {
                leader.annotation_handle = annotation_handle;
            }

            let _ = self.sync_displayed_annotation_context(leader_handle);
        }
        remap_copied_reactors(&mut self.document, &handle_map);
        remap_copied_viewport_clips(&mut self.document, &handle_map, false);
        // A tree cloned in one pass can hold refs to sibling copied entities;
        // only once every copy exists can those resolve.
        remap_copied_xdict_cross_refs(&mut self.document, &handle_map, &created_objects);
        // Complete group copies record their new Group objects and dictionary
        // entry as targeted object deltas inside copy_complete_groups.
        self.copy_complete_groups(&handle_map);
        self.copy_dimension_associations(&handle_map);
        self.copy_hatch_associations(&handle_map);
        // The copies are new handles (natural memo misses, tessellated fresh)
        // and reference only already-cached blocks — no block defn changes.
        // Report them as additions so derived caches patch in exactly the copies.
        let changes: Vec<(Handle, ChangeKind)> = new_handles
            .iter()
            .map(|&h| (h, ChangeKind::Added))
            .collect();
        self.bump_entities(&changes);
        // Constraints wholly within the duplicated selection follow it.
        self.duplicate_parametric_constraints_for(&handle_map);
        self.refresh_meshes_for_handles(&refresh_solid_handles);
        new_handles
    }

    // ── Grip editing ──────────────────────────────────────────────────────

    pub(crate) fn solid_history_objects(
        &self,
        handle: Handle,
    ) -> Vec<(Handle, codec::objects::ObjectType)> {
        let Some(graph) = self.document.solid_history_graph(handle) else {
            return Vec::new();
        };
        std::iter::once(graph.root)
            .chain(graph.nodes)
            .chain(graph.evaluation_graph)
            .filter_map(|object_handle| {
                self.document
                    .objects
                    .get(&object_handle)
                    .cloned()
                    .map(|object| (object_handle, object))
            })
            .collect()
    }

    fn record_solid_history_before(&mut self, handle: Handle) {
        if !self.is_recording_undo() {
            return;
        }
        for (object_handle, object) in self.solid_history_objects(handle) {
            self.record_undo_object_before(object_handle, Some(object));
        }
    }

    pub fn create_solid_history(
        &mut self,
        handle: Handle,
        operation: codec::objects::SolidHistoryOperation,
    ) -> bool {
        let Some(graph) = self.document.create_solid_history(handle, operation) else {
            return false;
        };
        self.record_undo_object_before(graph.root, None);
        for node in graph.nodes.into_iter().chain(graph.evaluation_graph) {
            self.record_undo_object_before(node, None);
        }
        self.sync_solid_reference_point(handle);
        true
    }

    pub fn append_solid_history(
        &mut self,
        handle: Handle,
        operation: codec::objects::SolidHistoryOperation,
    ) -> bool {
        let previous: Vec<Handle> = self
            .document
            .solid_history_graph(handle)
            .map(|graph| {
                graph
                    .nodes
                    .into_iter()
                    .chain(graph.evaluation_graph)
                    .collect()
            })
            .unwrap_or_default();
        self.record_solid_history_before(handle);
        let Some(graph) = self.document.append_solid_history(handle, operation) else {
            return false;
        };
        // A history saved without an evaluation graph gets one on append.
        for node in graph.nodes.into_iter().chain(graph.evaluation_graph) {
            if !previous.contains(&node) {
                self.record_undo_object_before(node, None);
            }
        }
        self.sync_solid_reference_point(handle);
        true
    }

    /// Make `target`'s history the boolean steps that built it: starting from
    /// `first`'s history, each of `steps` joins a solid's history into the
    /// result so far. When `first` is `target` its own history is kept; a
    /// solid with no history starts from its body in `bodies`.
    ///
    /// False, with `target` left without a history, when one of the solids
    /// cannot carry one.
    pub fn join_boolean_histories(
        &mut self,
        target: Handle,
        first: Handle,
        steps: &[(Handle, u8)],
        bodies: &std::collections::HashMap<Handle, kernel::brep::Body>,
    ) -> bool {
        let solids = std::iter::once(target)
            .chain(std::iter::once(first))
            .chain(steps.iter().map(|(handle, _)| *handle))
            .collect::<Vec<_>>();
        let solid = |handle: &Handle| {
            matches!(
                self.document.get_entity(*handle),
                Some(EntityType::Solid3D(_))
            )
        };
        if !solids.iter().all(solid) {
            return false;
        }
        for handle in solids.iter().skip(1) {
            if self.document.solid_history_graph(*handle).is_some() {
                continue;
            }
            let Some(body) = bodies.get(handle) else {
                return false;
            };
            if !self
                .create_solid_history(*handle, crate::scene::model::solid_history::brep_op(body))
            {
                return false;
            }
        }
        if first != target {
            self.delete_solid_history(target);
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(target);
                self.record_undo_before(target, before);
            }
            if !self.copy_solid_history(first, target) {
                return false;
            }
        }
        for &(tool, operation) in steps {
            if !self.merge_solid_history_boolean(target, tool, operation) {
                self.delete_solid_history(target);
                return false;
            }
        }
        true
    }

    fn merge_solid_history_boolean(&mut self, target: Handle, tool: Handle, operation: u8) -> bool {
        let previous: Vec<Handle> = [target, tool]
            .iter()
            .filter_map(|handle| self.document.solid_history_graph(*handle))
            .flat_map(|graph| graph.nodes.into_iter().chain(graph.evaluation_graph))
            .collect();
        self.record_solid_history_before(target);
        self.record_solid_history_before(tool);
        if self.is_recording_undo() {
            // The tool loses its history link; undo must give it back.
            let before = self.document.get_entity_arc(tool);
            self.record_undo_before(tool, before);
        }
        let Some(graph) = self
            .document
            .merge_solid_history_boolean(target, tool, operation)
        else {
            return false;
        };
        for node in graph.nodes.into_iter().chain(graph.evaluation_graph) {
            if !previous.contains(&node) {
                self.record_undo_object_before(node, None);
            }
        }
        true
    }

    pub(crate) fn sync_solid_reference_point(&mut self, handle: Handle) {
        let reference = self
            .document
            .solid_history_operations(handle)
            .and_then(|operations| {
                operations
                    .iter()
                    .rev()
                    .find_map(crate::scene::model::solid_history::reference_point)
            });
        let Some(reference) = reference else {
            return;
        };
        let point = codec::types::Vector3::new(reference.x, reference.y, reference.z);
        match self.document.get_entity_mut(handle) {
            Some(EntityType::Solid3D(entity)) => entity.point_of_reference = point,
            Some(EntityType::Surface(entity)) => entity.point_of_reference = point,
            _ => {}
        }
    }

    fn copy_solid_history(&mut self, source: Handle, target: Handle) -> bool {
        let Some(graph) = self.document.copy_solid_history(source, target) else {
            return false;
        };
        self.record_undo_object_before(graph.root, None);
        for node in graph.nodes {
            self.record_undo_object_before(node, None);
        }
        true
    }

    pub(crate) fn delete_solid_history(&mut self, handle: Handle) {
        if self.is_recording_undo() {
            // The codec also clears the entity's history handle. Capture that
            // link before deletion so undo reconnects the restored graph.
            let before = self.document.get_entity_arc(handle);
            self.record_undo_before(handle, before);
        }
        self.record_solid_history_before(handle);
        self.document.delete_solid_history(handle);
    }

    fn rebuild_history_body(
        &self,
        handle: Handle,
        operation: &codec::objects::SolidHistoryOperation,
    ) -> Option<kernel::brep::Body> {
        use codec::objects::SolidHistoryOperation;

        match (self.document.get_entity(handle)?, operation) {
            (EntityType::Surface(_), SolidHistoryOperation::Extrusion(value)) => {
                kernel::acis::rebuild_extrusion_with_mode(value, true).ok()
            }
            (EntityType::Surface(_), SolidHistoryOperation::Loft(_)) => {
                kernel::acis::rebuild_body(operation).ok()
            }
            (EntityType::Solid3D(_), _) => {
                let mut tree = self.document.solid_history_tree(handle)?;
                tree.find_mut(operation.base()?.node_id())?.operation = operation.clone();
                kernel::acis::rebuild_history_tree(&tree).ok()
            }
            _ => None,
        }
    }

    fn history_surface_data(
        &self,
        handle: Handle,
        operation: &codec::objects::SolidHistoryOperation,
    ) -> Option<Option<codec::entities::SurfaceData>> {
        use codec::objects::SolidHistoryOperation;

        match (self.document.get_entity(handle)?, operation) {
            (EntityType::Surface(_), SolidHistoryOperation::Extrusion(value)) => {
                crate::scene::model::solid_history::extrusion_surface_data(value).map(Some)
            }
            (EntityType::Surface(_), SolidHistoryOperation::Loft(value)) => {
                let EntityType::Surface(surface) =
                    crate::scene::model::loft_command_model::surface_entity(value)
                else {
                    return None;
                };
                Some(Some(surface.surface_data))
            }
            (EntityType::Solid3D(_), _) => Some(None),
            _ => None,
        }
    }

    /// Swept surfaces linked to scale/twist expressions follow the named
    /// parameters: re-evaluated and rebuilt when a value changes, as the
    /// reference keeps them associative. Returns the surfaces rebuilt.
    pub fn refresh_expression_sweeps(&mut self) -> Vec<Handle> {
        use crate::scene::model::sweep_model;
        let linked = self
            .document
            .entities()
            .filter_map(|entity| {
                Some((
                    entity.common().handle,
                    sweep_model::sweep_expressions(entity)?,
                ))
            })
            .collect::<Vec<_>>();
        let mut rebuilt = Vec::new();
        for (handle, [scale, twist]) in linked {
            let Some(entity) = self.document.get_entity(handle) else {
                continue;
            };
            let Some(mut record) = sweep_model::surface_sweep_record(entity) else {
                continue;
            };
            let evaluate = |expression: &str| {
                let mut table = self.named_parameters().clone();
                table.set("sweepExpression", expression).ok()?;
                table
                    .resolve("sweepExpression")
                    .ok()
                    .filter(|value| value.is_finite())
            };
            let mut changed = false;
            if let Some(value) = scale.as_deref().and_then(evaluate) {
                changed |= (record.scale_factor - value).abs() > 1e-12;
                record.scale_factor = value;
            }
            if let Some(value) = twist.as_deref().and_then(evaluate).map(f64::to_radians) {
                changed |= (record.twist_angle - value).abs() > 1e-12;
                record.twist_angle = value;
            }
            if !changed {
                continue;
            }
            let Ok(body) = kernel::acis::rebuild_sweep_with_mode(&record, true) else {
                continue;
            };
            let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&body) else {
                continue;
            };
            let Some(display) = self.prepare_solid_model_display(handle, &body) else {
                continue;
            };
            let before = self
                .document
                .get_entity(handle)
                .cloned()
                .map(std::sync::Arc::new);
            self.record_undo_before(handle, before);
            if let Some(EntityType::Surface(surface)) = self.document.get_entity_mut(handle) {
                surface.acis_data = codec::entities::AcisData::from_sat(&document.to_sat_string());
                if let codec::entities::SurfaceData::Swept { options, .. } =
                    &mut surface.surface_data
                {
                    options.scale_factor = record.scale_factor;
                    options.twist_angle = record.twist_angle;
                }
            }
            self.register_prepared_solid_model(handle, body, display);
            rebuilt.push(handle);
        }
        rebuilt
    }

    /// Associative swept surfaces follow edits of their profile or path:
    /// swept again from them with the options they were made with. A
    /// surface edited along with its sources keeps its own edit. Returns the
    /// surfaces rebuilt.
    pub(crate) fn refresh_associative_sweeps(
        &mut self,
        changes: &[(Handle, crate::scene::ChangeKind)],
    ) -> Vec<(Handle, crate::scene::ChangeKind)> {
        use crate::scene::model::sweep_model;
        let changed = changes
            .iter()
            .filter(|(_, kind)| !matches!(kind, crate::scene::ChangeKind::Removed))
            .map(|(handle, _)| *handle)
            .collect::<rustc_hash::FxHashSet<_>>();
        // Every edit passes through here: a drawing with no associative
        // swept surface (no application registered for them) skips the scan.
        if changed.is_empty()
            || !self
                .document
                .app_ids
                .contains(sweep_model::SWEEP_SOURCES_APP)
        {
            return Vec::new();
        }
        let linked = self
            .document
            .entities()
            .filter(|entity| matches!(entity, EntityType::Surface(_)))
            .filter_map(|entity| {
                let handle = entity.common().handle;
                let (profile, path) = sweep_model::sweep_sources(entity)?;
                (!changed.contains(&handle)
                    && (changed.contains(&profile) || changed.contains(&path)))
                .then_some((handle, profile, path))
            })
            .collect::<Vec<_>>();
        let mut rebuilt = Vec::new();
        for (handle, profile, path) in linked {
            let (Some(surface), Some(profile), Some(path)) = (
                self.document.get_entity(handle),
                self.document.get_entity(profile),
                self.document.get_entity(path),
            ) else {
                continue;
            };
            let Some((record, body)) = sweep_model::resweep_from_sources(surface, profile, path)
            else {
                continue;
            };
            let EntityType::Surface(fresh) = sweep_model::swept_surface_entity(&record) else {
                continue;
            };
            let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&body) else {
                continue;
            };
            let Some(display) = self.prepare_solid_model_display(handle, &body) else {
                continue;
            };
            let before = self
                .document
                .get_entity(handle)
                .cloned()
                .map(std::sync::Arc::new);
            self.record_undo_before(handle, before);
            if let Some(EntityType::Surface(surface)) = self.document.get_entity_mut(handle) {
                surface.acis_data = codec::entities::AcisData::from_sat(&document.to_sat_string());
                surface.surface_data = fresh.surface_data;
                surface.point_of_reference = fresh.point_of_reference;
            }
            self.register_prepared_solid_model(handle, body, display);
            rebuilt.push((handle, crate::scene::ChangeKind::Modified));
        }
        rebuilt
    }

    /// Associative extruded, revolved and lofted surfaces follow edits of
    /// their sources: made again from them with the values they were made
    /// with. A surface edited along with its sources keeps its own edit.
    /// Returns the surfaces rebuilt.
    pub(crate) fn refresh_associative_surfaces(
        &mut self,
        changes: &[(Handle, crate::scene::ChangeKind)],
    ) -> Vec<(Handle, crate::scene::ChangeKind)> {
        use crate::scene::model::surface_sources::{
            axis_points, surface_sources, SurfaceSources, SURFACE_SOURCES_APP,
        };
        use crate::scene::model::sweep_model;
        use codec::objects::SolidHistoryOperation;
        let changed = changes
            .iter()
            .filter(|(_, kind)| !matches!(kind, crate::scene::ChangeKind::Removed))
            .map(|(handle, _)| *handle)
            .collect::<rustc_hash::FxHashSet<_>>();
        if changed.is_empty() || !self.document.app_ids.contains(SURFACE_SOURCES_APP) {
            return Vec::new();
        }
        let linked = self
            .document
            .entities()
            .filter(|entity| matches!(entity, EntityType::Surface(_)))
            .filter_map(|entity| {
                let handle = entity.common().handle;
                let sources = surface_sources(entity)?;
                (!changed.contains(&handle)
                    && sources
                        .handles()
                        .iter()
                        .any(|source| changed.contains(source)))
                .then_some((handle, sources))
            })
            .collect::<Vec<_>>();
        let mut rebuilt = Vec::new();
        for (handle, sources) in linked {
            let source = |handle: Handle| self.document.get_entity(handle).cloned();
            // The surface made again: a history step, or a body with its data.
            let made: Option<
                Result<SolidHistoryOperation, (kernel::brep::Body, codec::entities::SurfaceData)>,
            > = match &sources {
                SurfaceSources::Extrude {
                    profile,
                    height,
                    direction,
                    taper,
                } => (|| {
                    let entity = source(*profile)?;
                    let (shape, _) = sweep_model::extrusion_profile_of(&entity)?;
                    let direction = match direction {
                        Some(direction) => *direction,
                        None => glam::DVec3::from_array(shape.plane.normal()?) * *height,
                    };
                    if self.document.solid_history_operations(handle).is_some() {
                        return sweep_model::extrusion_history(
                            &entity,
                            None,
                            direction.to_array(),
                            *taper,
                            shape.plane.origin,
                        )
                        .map(Ok);
                    }
                    let body =
                        sweep_model::extruded_surface(&entity, direction.to_array(), *taper)?;
                    let EntityType::Surface(fresh) =
                        crate::modules::insert::solid3d_cmds::empty_extruded_surface(
                            direction, *taper,
                        )
                    else {
                        return None;
                    };
                    Some(Err((body, fresh.surface_data)))
                })(),
                SurfaceSources::Revolve {
                    profile,
                    axis,
                    start,
                    end,
                    angle,
                    start_angle,
                } => (|| {
                    let entity = source(*profile)?;
                    let (start, end) = axis
                        .and_then(|axis| self.document.get_entity(axis).and_then(axis_points))
                        .unwrap_or((*start, *end));
                    let body = sweep_model::revolved_surface(
                        &entity,
                        start.to_array(),
                        end.to_array(),
                        *angle,
                        *start_angle,
                    )?;
                    let EntityType::Surface(fresh) =
                        crate::modules::insert::solid3d_cmds::empty_revolved_surface(
                            &entity,
                            start,
                            end,
                            *angle,
                            *start_angle,
                        )
                    else {
                        return None;
                    };
                    Some(Err((body, fresh.surface_data)))
                })(),
                SurfaceSources::Loft { sections } => (|| {
                    let operations = self.document.solid_history_operations(handle)?;
                    let mut record =
                        operations
                            .into_iter()
                            .find_map(|operation| match operation {
                                SolidHistoryOperation::Loft(record) => Some(record),
                                _ => None,
                            })?;
                    if record.cross_sections.len() != sections.len() {
                        return None;
                    }
                    record.cross_sections = sections
                        .iter()
                        .map(|section| {
                            source(*section)
                                .as_ref()
                                .and_then(crate::scene::model::loft_command_model::embedded_section)
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some(Ok(SolidHistoryOperation::Loft(record)))
                })(),
            };
            let before = self
                .document
                .get_entity(handle)
                .cloned()
                .map(std::sync::Arc::new);
            match made {
                Some(Ok(operation)) => {
                    self.record_undo_before(handle, before);
                    if self.rebuild_solid_history(handle, operation) {
                        rebuilt.push((handle, crate::scene::ChangeKind::Modified));
                    }
                }
                Some(Err((body, data))) => {
                    let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&body)
                    else {
                        continue;
                    };
                    let Some(display) = self.prepare_solid_model_display(handle, &body) else {
                        continue;
                    };
                    self.record_undo_before(handle, before);
                    if let Some(EntityType::Surface(surface)) = self.document.get_entity_mut(handle)
                    {
                        surface.acis_data =
                            codec::entities::AcisData::from_sat(&document.to_sat_string());
                        surface.surface_data = data;
                    }
                    self.register_prepared_solid_model(handle, body, display);
                    rebuilt.push((handle, crate::scene::ChangeKind::Modified));
                }
                None => {}
            }
        }
        rebuilt
    }

    /// Swept surfaces the reference keeps associative to a scale or twist
    /// expression (an associative swept surface action whose value parameter
    /// depends on an expression variable) are linked the same way here.
    pub(crate) fn import_sweep_expression_links(&mut self) {
        use codec::objects::{AssocSurfaceActionKind, AssociativeData, ObjectType};
        let object = |handle: Handle| match self.document.objects.get(&handle) {
            Some(ObjectType::Associative(object)) => Some(&object.data),
            _ => None,
        };
        let mut links = Vec::new();
        let mut sources = Vec::new();
        let mut others = Vec::new();
        for item in self.document.objects.values() {
            let ObjectType::Associative(action) = item else {
                continue;
            };
            let AssociativeData::Action(action) = &action.data else {
                continue;
            };
            let Some(AssociativeData::SurfaceActionBody(body)) = object(action.action_body) else {
                continue;
            };
            let Some(AssociativeData::Dependency(written)) = object(body.surface_body.dependency)
            else {
                continue;
            };
            // The entities a named path parameter reads (an edge of a region
            // reads through a geometry dependency).
            let read = |name: &str| -> Vec<Handle> {
                action
                    .owned_parameters
                    .iter()
                    .filter_map(|parameter| {
                        let Some(AssociativeData::PathActionParam(path)) = object(*parameter)
                        else {
                            return None;
                        };
                        if path.compound.action_param.name != name {
                            return None;
                        }
                        let Some(AssociativeData::EdgeActionParam(edge)) =
                            object(*path.compound.parameters.first()?)
                        else {
                            return None;
                        };
                        match object(edge.single_dependency.dependency) {
                            Some(AssociativeData::Dependency(dependency)) => {
                                Some(dependency.dependent_on)
                            }
                            Some(AssociativeData::GeomDependency(dependency)) => {
                                Some(dependency.dependency.dependent_on)
                            }
                            _ => None,
                        }
                    })
                    .collect()
            };
            let real = |name: &str| {
                action
                    .values
                    .iter()
                    .find(|value| value.name == name)
                    .and_then(|value| value.variables.first())
                    .and_then(|variable| match variable.value.value {
                        codec::objects::AssocEvalValue::Real(value) => Some(value),
                        _ => None,
                    })
            };
            match body.kind {
                AssocSurfaceActionKind::Extruded => {
                    if let (Some(profile), Some(height), Some(taper)) = (
                        read("ExtrudeProfile").first().copied(),
                        real("ExtrusionHeight"),
                        real("ExtrusionTaperAngle"),
                    ) {
                        others.push((
                            written.dependent_on,
                            (body.kind, vec![profile], None, height, taper),
                        ));
                    }
                    continue;
                }
                AssocSurfaceActionKind::Revolved => {
                    if let (Some(profile), Some(angle)) = (
                        read("RevolveProfile").first().copied(),
                        real("RevolveAngle"),
                    ) {
                        others.push((
                            written.dependent_on,
                            (
                                body.kind,
                                vec![profile],
                                read("RevolvePath").first().copied(),
                                angle,
                                0.0,
                            ),
                        ));
                    }
                    continue;
                }
                AssocSurfaceActionKind::Lofted => {
                    let sections = read("CrossSection");
                    if sections.len() >= 2 {
                        others.push((written.dependent_on, (body.kind, sections, None, 0.0, 0.0)));
                    }
                    continue;
                }
                AssocSurfaceActionKind::Swept => {}
                _ => continue,
            }
            let mut expressions: [Option<String>; 2] = [None, None];
            for value in &action.values {
                let slot = match value.name.as_str() {
                    "ScaleFactor" => 0,
                    "TwistAngle" => 1,
                    _ => continue,
                };
                for variable in &value.variables {
                    let Some(AssociativeData::ValueDependency(dependency)) =
                        object(variable.handle)
                    else {
                        continue;
                    };
                    let Some(AssociativeData::Variable(source)) =
                        object(dependency.dependency.dependent_on)
                    else {
                        continue;
                    };
                    if !source.expression.trim().is_empty()
                        && source.expression.trim().parse::<f64>().is_err()
                    {
                        expressions[slot] = Some(source.expression.trim().to_string());
                    }
                }
            }
            if expressions.iter().any(Option::is_some) {
                links.push((written.dependent_on, expressions));
            }
            // The profile and path the action reads.
            if let (Some(profile), Some(path)) = (
                read("SweepProfile").first().copied(),
                read("SweepPath").first().copied(),
            ) {
                sources.push((written.dependent_on, profile, path));
            }
        }
        for (surface, profile, path) in sources {
            let Some(entity @ EntityType::Surface(value)) = self.document.get_entity(surface)
            else {
                continue;
            };
            if crate::scene::model::sweep_model::sweep_sources(entity).is_some() {
                continue;
            }
            let codec::entities::SurfaceData::Swept { options, .. } = &value.surface_data else {
                continue;
            };
            let linked = crate::command::SweepOptions {
                align: options.sweep_alignment_flags == 1,
                bank: options.bank,
                ..crate::command::SweepOptions::default()
            };
            crate::scene::model::sweep_model::link_sweep_sources(
                &mut self.document,
                surface,
                profile,
                path,
                &linked,
            );
        }
        for (handle, expressions) in links {
            let Some(entity) = self.document.get_entity(handle) else {
                continue;
            };
            if crate::scene::model::sweep_model::sweep_expressions(entity).is_some() {
                continue;
            }
            crate::scene::model::sweep_model::link_sweep_expressions(
                &mut self.document,
                handle,
                &expressions,
            );
        }
        // Associative extruded, revolved and lofted surfaces follow their
        // sources here too.
        use crate::scene::model::surface_sources::{
            link_surface_sources, surface_sources, SurfaceSources,
        };
        for (surface, (kind, handles, axis, amount, taper)) in others {
            let Some(entity @ EntityType::Surface(value)) = self.document.get_entity(surface)
            else {
                continue;
            };
            if surface_sources(entity).is_some() {
                continue;
            }
            let linked = match (kind, &value.surface_data) {
                (
                    AssocSurfaceActionKind::Extruded,
                    codec::entities::SurfaceData::Extruded { sweep_vector, .. },
                ) => {
                    let profile = handles[0];
                    let along = glam::DVec3::new(sweep_vector.x, sweep_vector.y, sweep_vector.z);
                    // A direction other than the profile's normal is kept.
                    let normal = self
                        .document
                        .get_entity(profile)
                        .and_then(|entity| {
                            crate::scene::model::sweep_model::extrusion_profile_of(entity)
                        })
                        .and_then(|(shape, _)| shape.plane.normal())
                        .map(glam::DVec3::from_array);
                    let direction = match normal {
                        Some(normal)
                            if (normal * amount - along).length()
                                <= 1e-9 * amount.abs().max(1.0) =>
                        {
                            None
                        }
                        _ if along.length() > 1e-12 => Some(along),
                        _ => None,
                    };
                    SurfaceSources::Extrude {
                        profile,
                        height: amount,
                        direction,
                        taper,
                    }
                }
                (
                    AssocSurfaceActionKind::Revolved,
                    codec::entities::SurfaceData::Revolved {
                        axis_point,
                        axis_vector,
                        start_angle,
                        ..
                    },
                ) => {
                    let start = glam::DVec3::new(axis_point.x, axis_point.y, axis_point.z);
                    SurfaceSources::Revolve {
                        profile: handles[0],
                        axis,
                        start,
                        end: start + glam::DVec3::new(axis_vector.x, axis_vector.y, axis_vector.z),
                        angle: amount,
                        start_angle: *start_angle,
                    }
                }
                (AssocSurfaceActionKind::Lofted, _) => SurfaceSources::Loft { sections: handles },
                _ => continue,
            };
            link_surface_sources(&mut self.document, surface, &linked);
        }
    }

    pub fn rebuild_solid_history(
        &mut self,
        handle: Handle,
        operation: codec::objects::SolidHistoryOperation,
    ) -> bool {
        let Some(body) = self.rebuild_history_body(handle, &operation) else {
            return false;
        };
        let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&body) else {
            return false;
        };
        let Some(surface_data) = self.history_surface_data(handle, &operation) else {
            return false;
        };
        let Some(display) = self.prepare_solid_model_display(handle, &body) else {
            return false;
        };
        if matches!(
            &operation,
            codec::objects::SolidHistoryOperation::Loft(_)
                | codec::objects::SolidHistoryOperation::Extrusion(_)
        ) && !display.0.complete
        {
            return false;
        }
        self.record_solid_history_before(handle);
        if self
            .document
            .update_solid_history_step(handle, operation)
            .is_none()
        {
            return false;
        }
        match self.document.get_entity_mut(handle) {
            Some(EntityType::Solid3D(entity)) => entity.set_sat_document(&document),
            Some(EntityType::Surface(entity)) => {
                entity.acis_data = codec::entities::AcisData::from_sat(&document.to_sat_string());
                if let Some(data) = surface_data {
                    if matches!(&data, codec::entities::SurfaceData::Extruded { .. }) {
                        entity.kind = codec::entities::SurfaceKind::Extruded;
                    }
                    entity.surface_data = data;
                }
            }
            _ => return false,
        }
        self.register_prepared_solid_model(handle, body, display);
        self.solid_history_preview_wires.remove(&handle);
        true
    }

    pub fn finalize_solid_history(&mut self, handle: Handle) -> bool {
        let Some(operation) = self.document.solid_history_operation(handle).cloned() else {
            return false;
        };
        let Some(body) = self.rebuild_history_body(handle, &operation) else {
            return false;
        };
        let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&body) else {
            return false;
        };
        let Some(surface_data) = self.history_surface_data(handle, &operation) else {
            return false;
        };
        let Some(display) = self.prepare_solid_model_display(handle, &body) else {
            return false;
        };
        if matches!(
            &operation,
            codec::objects::SolidHistoryOperation::Loft(_)
                | codec::objects::SolidHistoryOperation::Extrusion(_)
        ) && !display.0.complete
        {
            return false;
        }
        match self.document.get_entity_mut(handle) {
            Some(EntityType::Solid3D(entity)) => entity.set_sat_document(&document),
            Some(EntityType::Surface(entity)) => {
                entity.acis_data = codec::entities::AcisData::from_sat(&document.to_sat_string());
                if let Some(data) = surface_data {
                    if matches!(&data, codec::entities::SurfaceData::Extruded { .. }) {
                        entity.kind = codec::entities::SurfaceKind::Extruded;
                    }
                    entity.surface_data = data;
                }
            }
            _ => return false,
        }
        self.register_prepared_solid_model(handle, body, display);
        self.solid_history_preview_wires.remove(&handle);
        true
    }

    fn preview_solid_history(
        &mut self,
        handle: Handle,
        operation: codec::objects::SolidHistoryOperation,
    ) -> bool {
        let Some(body) = self.rebuild_history_body(handle, &operation) else {
            return false;
        };
        if self.history_surface_data(handle, &operation).is_none() {
            return false;
        }
        // An operand inside a composite is a step below the active one.
        let active = self
            .document
            .solid_history_operation(handle)
            .and_then(|active| active.base())
            .map(|base| base.node_id());
        let updated = if operation.base().map(|base| base.node_id()) == active {
            self.document
                .update_solid_history(handle, operation)
                .is_some()
        } else {
            self.document
                .update_solid_history_step(handle, operation)
                .is_some()
        };
        if !updated {
            return false;
        }
        let (isoline_counts, planar_isolines) = match self.document.get_entity(handle) {
            Some(EntityType::Surface(surface)) => (
                crate::entities::solid3d::surface_isoline_counts(surface),
                crate::entities::solid3d::surface_property_state(surface).isolines,
            ),
            _ => ([0; 2], false),
        };
        self.solid_history_preview_wires.insert(
            handle,
            crate::scene::model::solid_model::grip_preview_wires(
                &body,
                handle,
                isoline_counts,
                planar_isolines,
            ),
        );
        true
    }

    fn transform_solid_history(&mut self, handle: Handle, transform: &EntityTransform) -> bool {
        let Some(mut operation) = self.document.solid_history_operation(handle).cloned() else {
            return false;
        };
        if !crate::scene::model::solid_history::transform_operation(&mut operation, transform) {
            return false;
        }
        if let EntityTransform::Translate(delta) = transform {
            let surface_data = match (&operation, self.document.get_entity(handle)) {
                (
                    codec::objects::SolidHistoryOperation::Extrusion(value),
                    Some(EntityType::Surface(_)),
                ) => {
                    let Some(data) =
                        crate::scene::model::solid_history::extrusion_surface_data(value)
                    else {
                        return false;
                    };
                    Some(data)
                }
                _ => None,
            };
            if self
                .document
                .update_solid_history(handle, operation)
                .is_none()
            {
                return false;
            }
            if let (Some(data), Some(EntityType::Surface(entity))) =
                (surface_data, self.document.get_entity_mut(handle))
            {
                entity.surface_data = data;
            }
            self.translate_solid_geometry(handle, delta.to_array());
            return true;
        }
        self.rebuild_solid_history(handle, operation)
    }

    pub fn apply_solid_history_choice(&mut self, handle: Handle, field: &str, value: &str) -> bool {
        self.record_solid_history_before(handle);
        let mut created = false;
        if self.document.solid_history_graph(handle).is_none() {
            let create = match field {
                crate::scene::model::solid_history::PROP_HISTORY => {
                    if value.eq_ignore_ascii_case("Record") {
                        true
                    } else if value.eq_ignore_ascii_case("None") {
                        return false;
                    } else {
                        return false;
                    }
                }
                crate::scene::model::solid_history::PROP_SHOW_HISTORY
                    if self.document.header.show_solid_history.clamp(0, 2) == 1 =>
                {
                    if value.eq_ignore_ascii_case("Yes") {
                        true
                    } else if value.eq_ignore_ascii_case("No") {
                        return false;
                    } else {
                        return false;
                    }
                }
                _ => return false,
            };
            if create {
                if !matches!(
                    self.document.get_entity(handle),
                    Some(EntityType::Solid3D(_))
                ) {
                    return false;
                }
                self.restore_solid_models(&[handle]);
                let Some(body) = self.solid_models.get(&handle).cloned() else {
                    return false;
                };
                if self.is_recording_undo() {
                    let before = self.document.get_entity_arc(handle);
                    self.record_undo_before(handle, before);
                }
                if !self.create_solid_history(
                    handle,
                    crate::scene::model::solid_history::brep_op(&body),
                ) {
                    return false;
                }
                // A solid without a graph was displayed as History=None.
                // Preserve that object state before applying the requested
                // choice, independent of the drawing-wide SOLIDHIST setting.
                crate::scene::model::solid_history::apply_history_choice(
                    &mut self.document,
                    handle,
                    crate::scene::model::solid_history::PROP_HISTORY,
                    "None",
                );
                created = true;
            }
        }
        let applied = crate::scene::model::solid_history::apply_history_choice(
            &mut self.document,
            handle,
            field,
            value,
        );
        if created || applied {
            self.bump_entities(&[(handle, ChangeKind::Modified)]);
        }
        created || applied
    }

    fn apply_solid_history_grip(
        &mut self,
        handle: Handle,
        grip_id: usize,
        apply: GripApply,
    ) -> bool {
        let Some(mut operation) =
            crate::scene::model::solid_history::edit_operand(&self.document, handle)
                .or_else(|| self.document.solid_history_operation(handle).cloned())
        else {
            return false;
        };
        if grip_id == crate::scene::model::solid_history::GRIP_FILLET_RADIUS {
            let codec::objects::SolidHistoryOperation::Fillet(value) = &mut operation else {
                return false;
            };
            let Some(radius) = value.radii.first().copied() else {
                return false;
            };
            self.restore_solid_models(&[handle]);
            let Some(definition) = self.solid_models.get(&handle).and_then(|body| {
                crate::scene::model::solid_history::fillet_radius_grip(body, radius)
            }) else {
                return false;
            };
            let Some(axis) = definition.axis else {
                return false;
            };
            let change = match apply {
                GripApply::Absolute(world) => (world - definition.world).dot(axis),
                GripApply::Translate(delta) => delta.dot(axis),
            };
            let radius = radius + change;
            if !radius.is_finite() || radius <= 1.0e-6 {
                return false;
            }
            value.radii.clear();
            value.radii.push(radius);
            return self.preview_solid_history(handle, operation);
        }
        if matches!(
            grip_id,
            crate::scene::model::solid_history::GRIP_CHAMFER_DISTANCE1
                | crate::scene::model::solid_history::GRIP_CHAMFER_DISTANCE2
        ) {
            let definitions = {
                let codec::objects::SolidHistoryOperation::Chamfer(value) = &operation else {
                    return false;
                };
                crate::scene::model::solid_history::chamfer_distance_grips(
                    &self.document,
                    handle,
                    value,
                )
            };
            let Some(definition) = definitions.into_iter().find(|grip| grip.id == grip_id) else {
                return false;
            };
            let Some(axis) = definition.axis else {
                return false;
            };
            let change = match apply {
                GripApply::Absolute(world) => (world - definition.world).dot(axis),
                GripApply::Translate(delta) => delta.dot(axis),
            };
            let codec::objects::SolidHistoryOperation::Chamfer(value) = &mut operation else {
                return false;
            };
            let distance = if grip_id == crate::scene::model::solid_history::GRIP_CHAMFER_DISTANCE1
            {
                &mut value.base_distance
            } else {
                &mut value.other_distance
            };
            *distance += change;
            if !distance.is_finite() || *distance <= 1.0e-6 {
                return false;
            }
            return self.preview_solid_history(handle, operation);
        }
        if !crate::scene::model::solid_history::apply_primitive_grip(&mut operation, grip_id, apply)
        {
            return false;
        }
        let Some(operation) = crate::scene::model::solid_history::operand_to_history(
            &self.document,
            handle,
            operation,
        ) else {
            return false;
        };
        self.preview_solid_history(handle, operation)
    }

    pub fn apply_solid_history_property(
        &mut self,
        handle: Handle,
        field: &str,
        value: &str,
    ) -> bool {
        let Some(mut operation) = crate::scene::model::solid_history::primitive_property_operation(
            &self.document,
            handle,
        ) else {
            return false;
        };
        if !crate::scene::model::solid_history::apply_primitive_property(
            &mut operation,
            field,
            value,
        ) {
            return false;
        }
        let Some(operation) = crate::scene::model::solid_history::operand_to_history(
            &self.document,
            handle,
            operation,
        ) else {
            return false;
        };
        self.rebuild_solid_history(handle, operation)
    }

    /// The modeler error code the reference reports when it refuses a sweep
    /// property edit (twist or scale along a path with a corner).
    pub fn solid_history_property_refusal(
        &self,
        handle: Handle,
        field: &str,
        value: &str,
    ) -> Option<u32> {
        let mut operation = crate::scene::model::solid_history::primitive_property_operation(
            &self.document,
            handle,
        )?;
        if !crate::scene::model::solid_history::apply_primitive_property(
            &mut operation,
            field,
            value,
        ) {
            return None;
        }
        let codec::objects::SolidHistoryOperation::Sweep(record) = &operation else {
            return None;
        };
        kernel::acis::sweep_history_refusal(record)
            .map(crate::scene::model::sweep_model::sweep_refusal_code)
    }

    pub fn apply_solid_position_property(
        &mut self,
        handle: Handle,
        field: &str,
        value: &str,
        plane: crate::command::WorkingPlane,
    ) -> Option<bool> {
        let axis = match field {
            "s3d_px" | "rgn_px" | "bdy_px" | "srf_px" => 0,
            "s3d_py" | "rgn_py" | "bdy_py" | "srf_py" => 1,
            "s3d_pz" | "rgn_pz" | "bdy_pz" | "srf_pz" => 2,
            _ => return None,
        };
        let Ok(target) = value.trim().parse::<f64>() else {
            return Some(false);
        };
        if !target.is_finite() {
            return Some(false);
        }
        let Some(point) = self
            .document
            .get_entity(handle)
            .and_then(crate::entities::solid3d::point_of_reference)
        else {
            return Some(false);
        };
        let world = glam::DVec3::new(point.x, point.y, point.z);
        let mut local = plane.to_local(world);
        local[axis] = target;
        let delta = plane.to_world(local) - world;
        if delta.length_squared() <= f64::EPSILON {
            return Some(true);
        }
        self.transform_entities(&[handle], &EntityTransform::Translate(delta));
        Some(true)
    }

    pub(crate) fn translate_solid_geometry(&mut self, handle: Handle, delta: [f64; 3]) {
        if delta.iter().all(|value| value.abs() <= f64::EPSILON) {
            return;
        }
        let placement = kernel::brep::Placement::at(delta);
        if let Some(body) = self.solid_models.get(&handle).cloned() {
            if let Some(moved) = kernel::brep::transform(&body, &placement) {
                self.solid_models.insert(handle, moved);
            }
        }
        let Some(set) = self.meshes.get_mut(&handle) else {
            return;
        };
        let translate_split = |high: &mut [f32; 3], low: &mut [f32; 3]| {
            let absolute = [
                high[0] as f64 + low[0] as f64 + delta[0],
                high[1] as f64 + low[1] as f64 + delta[1],
                high[2] as f64 + low[2] as f64 + delta[2],
            ];
            *high = [absolute[0] as f32, absolute[1] as f32, absolute[2] as f32];
            *low = [
                (absolute[0] - high[0] as f64) as f32,
                (absolute[1] - high[1] as f64) as f32,
                (absolute[2] - high[2] as f64) as f32,
            ];
        };
        for lod in &mut set.lods {
            if lod.verts_low.len() != lod.verts.len() {
                lod.verts_low = vec![[0.0; 3]; lod.verts.len()];
            }
            for (high, low) in lod.verts.iter_mut().zip(lod.verts_low.iter_mut()) {
                translate_split(high, low);
            }
        }
        if set.edge_verts_low.len() != set.edge_verts.len() {
            set.edge_verts_low = vec![[0.0; 3]; set.edge_verts.len()];
        }
        for (high, low) in set.edge_verts.iter_mut().zip(set.edge_verts_low.iter_mut()) {
            translate_split(high, low);
        }
        for generator in &mut set.curved_gens {
            if let Some(source) =
                kernel::brep::mesh::transform_silhouette(&generator.source, &placement)
            {
                generator.source = source;
            }
        }
        set.metrics.translate(delta);
        set.recompute_aabb();
    }

    pub fn apply_grip(&mut self, handle: Handle, grip_id: usize, apply: GripApply) {
        // Objects on a locked layer can't be grip-edited.
        if self.is_layer_locked(handle) {
            return;
        }
        // An attribute grip sits on the displayed (annotation-scaled)
        // attribute; the stored one is unscaled.
        let apply = match self.document.get_entity(handle) {
            Some(EntityType::Insert(insert)) => crate::entities::insert::unscale_attribute_grip(
                insert,
                self.annotation_scale,
                grip_id,
                apply,
            ),
            _ => apply,
        };
        if self.apply_solid_history_grip(handle, grip_id, apply.clone()) {
            return;
        }
        // For Solid3D / Region / Body, record the old point_of_reference so we
        // can translate the pre-tessellated MeshModel by the same delta after
        // the grip is applied (the ACIS data itself is not modified).
        let old_por: Option<[f64; 3]> = self
            .document
            .get_entity(handle)
            .and_then(crate::entities::solid3d::point_of_reference)
            .map(|p| [p.x, p.y, p.z]);
        // A LEADER's final vertex is the end of its horizontal landing.
        // Remember its old position and linked MTEXT so the annotation can follow
        // when that grip stretches the landing.
        let leader_landing_before = self.document.get_entity(handle).and_then(|entity| {
            let EntityType::Leader(leader) = entity else {
                return None;
            };

            let n = leader.vertices.len();
            if n < 3 || (grip_id != n - 1 && grip_id != n - 2) || leader.annotation_handle.is_null()
            {
                return None;
            }

            let point = leader.vertices.last()?;

            Some((
                leader.annotation_handle,
                glam::DVec3::new(point.x, point.y, point.z),
            ))
        });
        if let Some(entity) = self.document.get_entity_mut(handle) {
            view::dispatch::apply_grip(entity, grip_id, apply);
        }
        self.sync_diameter_association_angle(handle);
        if let Some((annotation_handle, old_landing)) = leader_landing_before {
            let new_landing = self.document.get_entity(handle).and_then(|entity| {
                let EntityType::Leader(leader) = entity else {
                    return None;
                };

                let point = leader.vertices.last()?;
                Some(glam::DVec3::new(point.x, point.y, point.z))
            });

            if let Some(new_landing) = new_landing {
                let delta = new_landing - old_landing;

                if delta.length_squared() > 1.0e-20 {
                    if self.is_recording_undo() {
                        if let Some(before) = self.document.get_entity_arc(annotation_handle) {
                            self.record_undo_before(annotation_handle, Some(before));
                        }
                    }

                    if let Some(annotation) = self.document.get_entity_mut(annotation_handle) {
                        view::dispatch::apply_transform(
                            annotation,
                            &crate::command::EntityTransform::Translate(delta),
                        );
                    }

                    if self.sync_displayed_annotation_context(annotation_handle) {
                        self.poison_undo_recording();
                    }
                    self.bump_entities(&[(annotation_handle, crate::scene::ChangeKind::Modified)]);
                }
            }
        }
        if self.sync_displayed_annotation_context(handle) {
            self.poison_undo_recording();
        }
        // A dimension loaded from a file renders through its baked *D block;
        // the grip only moved a definition point, so the baked graphics are
        // stale — drop them so tessellation falls back to the live geometry
        // and the next save re-bakes (no-op for non-dimensions). (#398)
        self.invalidate_dim_block_recorded(handle);

        // Translate MeshModel vertices by the same delta the grip applied.
        if let Some(old) = old_por {
            let new_por: Option<[f64; 3]> = self
                .document
                .get_entity(handle)
                .and_then(crate::entities::solid3d::point_of_reference)
                .map(|p| [p.x, p.y, p.z]);
            if let Some(new) = new_por {
                let delta = [new[0] - old[0], new[1] - old[1], new[2] - old[2]];
                let moved_history = self.transform_solid_history(
                    handle,
                    &EntityTransform::Translate(glam::DVec3::from_array(delta)),
                );
                if !moved_history {
                    self.translate_solid_geometry(handle, delta);
                }
            }
        }

        // Rebuild GPU hatch/solid model when a boundary vertex or corner moves.
        match self.document.get_entity(handle) {
            Some(EntityType::Hatch(dxf)) => {
                let color = convert::tess_util::aci_to_rgba(&dxf.common.color);
                if let Some(model) = Self::hatch_model_from_dxf(dxf, color) {
                    self.hatches.insert(handle, model);
                } else {
                    self.hatches.remove(&handle);
                }
            }
            Some(EntityType::Solid(solid)) => {
                let color = convert::tess_util::aci_to_rgba(&solid.common.color);
                self.hatches
                    .insert(handle, Self::solid_hatch_model(solid, color));
            }
            _ => {}
        }
        // The grip-drag caller refreshes changed resident meshes per move.
        // Raster images, OLE frames and PDF underlays keep a derived ImageModel
        // with their textured quad geometry. A grip edit mutates the entity directly,
        // so rebuild that cache as well or only the wire frame follows the grip.
        let image_bearing = self.document.get_entity(handle).is_some_and(|entity| {
            matches!(
                entity,
                EntityType::RasterImage(_) | EntityType::Ole2Frame(_) | EntityType::Underlay(_)
            )
        });

        if image_bearing {
            let model = self
                .document
                .get_entity(handle)
                .and_then(|entity| self.image_seed_for(entity));

            self.images.remove(&handle);

            if let Some(model) = model {
                self.images.insert(handle, model);
            }
        }
        // A grip edit can move geometry used as the boundary of an associative hatch.
        // The source entity has already been mutated above, so rebuild any dependent
        // hatch from the live document geometry immediately.
        let source_change = [(handle, ChangeKind::Modified)];
        let hatch_changes = self.refresh_associative_hatches(&source_change);

        if !hatch_changes.is_empty() {
            self.bump_entities(&hatch_changes);
        }
    }
}

#[cfg(test)]
mod leader_annotation_reactor_tests {
    use super::*;

    fn add_pair(scene: &mut Scene) -> (Handle, Handle) {
        let leader = scene.add_entity(EntityType::Leader(codec::entities::Leader::default()));
        let annotation = scene.add_entity(EntityType::MText(Box::new(
            codec::entities::MText::with_value("note", codec::types::Vector3::new(10.0, 5.0, 0.0)),
        )));
        (leader, annotation)
    }

    #[test]
    fn linking_adds_annotation_reactor_exactly_once() {
        let mut scene = Scene::new();
        let (leader, annotation) = add_pair(&mut scene);
        assert!(scene.link_leader_annotation(leader, annotation));
        assert!(scene.link_leader_annotation(leader, annotation));

        let Some(EntityType::Leader(value)) = scene.document.get_entity(leader) else {
            panic!("leader missing");
        };
        assert_eq!(value.annotation_handle, annotation);
        let reactors = &scene
            .document
            .get_entity(annotation)
            .unwrap()
            .common()
            .reactors;
        assert_eq!(reactors.iter().filter(|&&h| h == leader).count(), 1);
    }

    #[test]
    fn copy_pair_remaps_annotation_reactor() {
        let mut scene = Scene::new();
        let (leader, annotation) = add_pair(&mut scene);
        assert!(scene.link_leader_annotation(leader, annotation));
        let copies = scene.copy_entities(
            &[leader, annotation],
            &EntityTransform::Translate(glam::DVec3::new(20.0, 0.0, 0.0)),
        );
        assert_eq!(copies.len(), 2);
        assert_ne!(copies[0], leader);
        assert_ne!(copies[1], annotation);
        let Some(EntityType::Leader(value)) = scene.document.get_entity(copies[0]) else {
            panic!("copied leader missing");
        };
        assert_eq!(value.annotation_handle, copies[1]);
        let reactors = &scene
            .document
            .get_entity(copies[1])
            .unwrap()
            .common()
            .reactors;
        assert_eq!(reactors.iter().filter(|&&h| h == copies[0]).count(), 1);
        assert!(!reactors.contains(&leader));
        assert!(scene
            .document
            .get_entity(annotation)
            .unwrap()
            .common()
            .reactors
            .contains(&leader));
    }

    #[test]
    fn linking_new_pair_preserves_absent_undo_before_images() {
        let mut scene = Scene::new();
        scene.begin_undo_recording();
        let (leader, annotation) = add_pair(&mut scene);
        assert!(scene.link_leader_annotation(leader, annotation));
        let recording = scene.take_undo_recording().unwrap();
        assert!(matches!(recording.before.get(&leader), Some(None)));
        assert!(matches!(recording.before.get(&annotation), Some(None)));
        assert!(scene
            .document
            .get_entity(annotation)
            .unwrap()
            .common()
            .reactors
            .contains(&leader));
    }
}

/// A copy reacts only to what was copied with it: each reactor of the source
/// that maps to a copy follows it there, and the rest (the source's dimension
/// association, group, associative hatch) are dropped. The association and
/// group copies add their own reactors afterwards.
pub(crate) fn remap_copied_reactors(
    document: &mut codec::Document,
    handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
) {
    for &copy in handle_map.values() {
        let Some(entity) = document.get_entity_mut(copy) else {
            continue;
        };
        let reactors = &mut entity.common_mut().reactors;
        *reactors = reactors
            .iter()
            .filter_map(|reactor| handle_map.get(reactor).copied())
            .collect();
    }
}

/// A copied clipped viewport is clipped by the copy of its boundary when that
/// was copied with it, never by the source's boundary (which sits on the
/// source viewport and would clip the copy somewhere else). With
/// `drop_unmapped` (a paste, whose source handles may name anything in this
/// drawing) a viewport whose boundary did not come along loses its clip.
pub(crate) fn remap_copied_viewport_clips(
    document: &mut codec::Document,
    handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
    drop_unmapped: bool,
) {
    for &copy in handle_map.values() {
        let Some(EntityType::Viewport(viewport)) = document.get_entity_mut(copy) else {
            continue;
        };
        if viewport.clip_boundary_handle.is_null() {
            continue;
        }
        if let Some(&boundary) = handle_map.get(&viewport.clip_boundary_handle) {
            viewport.clip_boundary_handle = boundary;
        } else if drop_unmapped {
            viewport.clip_boundary_handle = Handle::NULL;
            viewport.status.non_rectangular_clipping = false;
        }
    }
}

/// Resolve entity-valued refs inside freshly cloned object trees through the
/// completed copy map. Object-to-object cross-refs are already resolved
/// against the shared prealloc map inside `clone_xdict_tree`; this pass only
/// rewrites refs to copied entities, which may not have existed yet when an
/// earlier tree in the same call was cloned.
pub(super) fn remap_copied_xdict_cross_refs(
    document: &mut codec::Document,
    handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
    created: &[Handle],
) {
    if handle_map.is_empty() {
        return;
    }
    let rewrite = |h: Handle| handle_map.get(&h).copied().unwrap_or(h);
    for &oh in created {
        let Some(obj) = document.objects.get_mut(&oh) else {
            continue;
        };
        match obj {
            codec::objects::ObjectType::Dictionary(d) => {
                d.owner = rewrite(d.owner);
                for (_, child) in &mut d.entries {
                    *child = rewrite(*child);
                }
                d.xdictionary_handle = d.xdictionary_handle.map(rewrite);
                for r in &mut d.reactors {
                    *r = rewrite(*r);
                }
            }
            codec::objects::ObjectType::XRecord(x) => {
                x.owner = rewrite(x.owner);
                x.xdictionary_handle = x.xdictionary_handle.map(rewrite);
                for r in &mut x.reactors {
                    *r = rewrite(*r);
                }
                for entry in &mut x.entries {
                    if let codec::objects::XRecordValue::Handle(h) = &mut entry.value {
                        *h = rewrite(*h);
                    }
                }
                for entry in &mut x.object_references {
                    entry.handle = rewrite(entry.handle);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod mirror_text_tests {
    use super::*;
    use glam::DVec3;

    fn mirror(p2: DVec3) -> EntityTransform {
        EntityTransform::Mirror {
            p1: DVec3::ZERO,
            p2,
            working_normal: DVec3::Z,
        }
    }

    fn text(scene: &mut Scene, rotation_deg: f64) -> Handle {
        let mut t = codec::entities::Text::new();
        t.value = "ABC".to_string();
        t.rotation = rotation_deg.to_radians();
        scene.document.add_entity(EntityType::Text(t)).unwrap()
    }

    fn orient(scene: &Scene, handle: Handle) -> (f64, i16) {
        match scene.document.get_entity(handle) {
            Some(EntityType::Text(t)) => (t.rotation.to_degrees(), t.generation_flags),
            other => panic!("expected a text, got {other:?}"),
        }
    }

    /// MIRRTEXT on: rotation and group 71 as AutoCAD writes them, for a
    /// vertical, horizontal and sloped axis through the origin.
    #[test]
    fn mirrtext_on_matches_autocad() {
        let cases = [
            (DVec3::Y, 0.0, 0.0),
            (DVec3::Y, 30.0, 330.0),
            (DVec3::X, 0.0, 180.0),
            (DVec3::X, 30.0, 150.0),
            (DVec3::new(1.0, 1.0, 0.0), 0.0, 270.0),
            (DVec3::new(2.0, 1.0, 0.0), 90.0, 143.130),
        ];
        for (axis, rotation, expected) in cases {
            let mut scene = Scene::new();
            scene.document.header.mirror_text = true;
            let moved = text(&mut scene, rotation);
            scene.transform_entities(&[moved], &mirror(axis));
            let source = text(&mut scene, rotation);
            let copy = scene.copy_entities(&[source], &mirror(axis))[0];
            for handle in [moved, copy] {
                let (got, flags) = orient(&scene, handle);
                assert!(
                    (got - expected).abs() < 1e-3,
                    "rot {rotation} about {axis:?}: got {got}, AutoCAD {expected}"
                );
                assert_eq!(
                    flags, 2,
                    "rot {rotation} about {axis:?}: only backward is set"
                );
            }
        }
    }
}

#[cfg(test)]
mod define_transformed_block_xdict_tests {
    use super::*;

    /// No extension dictionary may be referenced by more than one entity.
    fn assert_no_shared_xdicts(doc: &codec::Document) {
        use std::collections::HashMap;
        let mut owners: HashMap<codec::Handle, usize> = HashMap::new();
        for entity in doc.entities() {
            if let Some(xd) = entity.common().xdictionary_handle {
                if !xd.is_null() {
                    *owners.entry(xd).or_default() += 1;
                }
            }
        }
        let shared: Vec<_> = owners
            .iter()
            .filter(|(_, &n)| n > 1)
            .map(|(h, n)| format!("{h:?} x{n}"))
            .collect();
        assert!(
            shared.is_empty(),
            "shared extension dictionaries: {}",
            shared.join(", ")
        );
    }

    #[test]
    fn clone_block_is_flagged_anonymous() {
        let mut scene = Scene::new();
        let line = scene
            .document
            .add_entity(codec::EntityType::Line(codec::entities::Line::new()))
            .unwrap();
        let subs = vec![scene.document.get_entity(line).unwrap().clone()];
        let name = scene
            .define_transformed_block(
                &subs,
                &crate::command::EntityTransform::Translate(glam::DVec3::ZERO),
            )
            .expect("clone block must be created");
        let record = scene.document.block_records.get(&name).unwrap();
        assert!(record.flags.anonymous);
    }

    /// Cloned sub-entities must not share the source's extension dictionary.
    /// `define_transformed_block` clones each sub wholesale; without clearing
    /// `xdictionary_handle` the clone claims the source's dictionary and
    /// file validation reports "Extension dictionary <H> Cannot access".
    #[test]
    fn cloned_subs_do_not_share_source_xdict() {
        let mut scene = Scene::new();

        let dict_handle = scene.document.allocate_handle();
        let mut dict = codec::objects::Dictionary::new();
        dict.handle = dict_handle;
        scene
            .document
            .objects
            .insert(dict_handle, codec::objects::ObjectType::Dictionary(dict));

        let mut br = codec::tables::BlockRecord::new("SRC");
        br.handle = scene.document.allocate_handle();
        scene.document.block_records.add(br).unwrap();
        let mut line = codec::entities::Line::new();
        line.common.xdictionary_handle = Some(dict_handle);
        let src_handle = scene
            .document
            .add_entity(codec::EntityType::Line(line))
            .unwrap();
        scene
            .document
            .block_records
            .get_mut("SRC")
            .unwrap()
            .entity_handles
            .push(src_handle);

        let subs = vec![scene.document.get_entity(src_handle).unwrap().clone()];
        let name = scene
            .define_transformed_block(
                &subs,
                &crate::command::EntityTransform::Translate(glam::DVec3::ZERO),
            )
            .expect("clone block must be created");
        assert!(
            name.starts_with("*D"),
            "anonymous block expected, got {name}"
        );

        assert_no_shared_xdicts(&scene.document);
        // Source keeps its dictionary.
        match scene.document.get_entity(src_handle) {
            Some(e) => assert_eq!(
                e.common().xdictionary_handle,
                Some(dict_handle),
                "the source must keep its own dictionary"
            ),
            None => panic!("source entity missing"),
        }
        // Clones carry none.
        let owned = scene
            .document
            .block_records
            .get(&name)
            .unwrap()
            .entity_handles
            .clone();
        assert!(!owned.is_empty(), "clone block should own sub-entities");
        for h in &owned {
            match scene.document.get_entity(*h) {
                Some(e) => assert!(
                    e.common().xdictionary_handle.is_none(),
                    "cloned child {h:?} must not carry an extension dictionary"
                ),
                None => panic!("cloned child {h:?} missing"),
            }
        }
    }
}

#[cfg(test)]
mod add_entity_clone_xdict_tests {
    use super::*;

    /// A cross-document copy must not carry the source's extension dictionary:
    /// the target document does not own that object, so sharing it leaves a
    /// dangling reference. An extension dictionary is exclusively owned, and a
    /// second claimant fails file validation with
    /// "Extension dictionary <H> Cannot access".
    #[test]
    fn exported_clone_does_not_share_source_xdict() {
        let mut source = Scene::new();

        let dict_handle = source.document.allocate_handle();
        let mut dict = codec::objects::Dictionary::new();
        dict.handle = dict_handle;
        source
            .document
            .objects
            .insert(dict_handle, codec::objects::ObjectType::Dictionary(dict));

        let mut line = codec::entities::Line::new();
        line.common.xdictionary_handle = Some(dict_handle);
        let src_handle = source
            .document
            .add_entity(codec::EntityType::Line(line))
            .unwrap();

        let cloned = source.document.get_entity(src_handle).unwrap().clone();
        let mut target = Scene::new();
        let new_handle = target.add_entity_clone(cloned);
        assert!(!new_handle.is_null(), "clone must be added");

        // Source keeps its dictionary.
        match source.document.get_entity(src_handle) {
            Some(e) => assert_eq!(
                e.common().xdictionary_handle,
                Some(dict_handle),
                "the source must keep its own dictionary"
            ),
            None => panic!("source entity missing"),
        }
        // The copy carries none.
        match target.document.get_entity(new_handle) {
            Some(e) => assert!(
                e.common().xdictionary_handle.is_none(),
                "exported clone must not carry an extension dictionary"
            ),
            None => panic!("exported clone missing"),
        }
    }
}

#[cfg(test)]
mod copy_entities_xdict_tests {
    use super::*;
    use crate::command::EntityTransform;
    use codec::objects::{
        Dictionary, ObjectType, ProxyObjectReference, ProxyReferenceKind, XRecord, XRecordValue,
    };
    use glam::DVec3;

    /// Line with an extension-dictionary tree:
    /// root { SUB -> sub { DEEP -> deep }, REC -> rec("CLONE_ME") };
    /// root owner and reactors both name the source entity.
    #[allow(clippy::type_complexity)]
    fn scene_with_xdict_tree() -> (
        Scene,
        codec::Handle,
        codec::Handle,
        codec::Handle,
        codec::Handle,
        codec::Handle,
    ) {
        let mut scene = Scene::new();
        let root_h = scene.document.allocate_handle();
        let sub_h = scene.document.allocate_handle();
        let deep_h = scene.document.allocate_handle();
        let rec_h = scene.document.allocate_handle();

        let mut deep = XRecord::named("DEEP");
        deep.handle = deep_h;
        deep.owner = sub_h;
        deep.add_string(1, "DEEP_VALUE");
        let mut sub = Dictionary::new();
        sub.handle = sub_h;
        sub.owner = root_h;
        sub.add_entry("DEEP", deep_h);
        let mut rec = XRecord::named("REC");
        rec.handle = rec_h;
        rec.owner = root_h;
        rec.add_string(1, "CLONE_ME");
        let mut root = Dictionary::new();
        root.handle = root_h;
        root.add_entry("SUB", sub_h);
        root.add_entry("REC", rec_h);

        scene
            .document
            .objects
            .insert(root_h, ObjectType::Dictionary(root));
        scene
            .document
            .objects
            .insert(sub_h, ObjectType::Dictionary(sub));
        scene
            .document
            .objects
            .insert(deep_h, ObjectType::XRecord(deep));
        scene
            .document
            .objects
            .insert(rec_h, ObjectType::XRecord(rec));

        let mut line = codec::entities::Line::new();
        line.common.xdictionary_handle = Some(root_h);
        let src = scene
            .document
            .add_entity(codec::EntityType::Line(line))
            .expect("add source entity");
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&root_h) {
            d.owner = src;
            d.reactors.push(src);
        }
        (scene, src, root_h, sub_h, deep_h, rec_h)
    }

    /// Every object reachable from `root` through dictionary entries and
    /// xdictionary links (cycle-safe).
    fn tree_handles(doc: &codec::Document, root: codec::Handle) -> Vec<codec::Handle> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(h) = stack.pop() {
            if !seen.insert(h) {
                continue;
            }
            out.push(h);
            match doc.objects.get(&h) {
                Some(ObjectType::Dictionary(d)) => {
                    for (_, c) in &d.entries {
                        stack.push(*c);
                    }
                    if let Some(xd) = d.xdictionary_handle {
                        stack.push(xd);
                    }
                }
                Some(ObjectType::XRecord(x)) => {
                    if let Some(xd) = x.xdictionary_handle {
                        stack.push(xd);
                    }
                }
                _ => {}
            }
        }
        out.sort_unstable();
        out
    }

    /// Byte-for-byte image of a tree for source-unchanged assertions.
    fn tree_image(
        doc: &codec::Document,
        root: codec::Handle,
    ) -> Vec<(codec::Handle, ObjectType)> {
        tree_handles(doc, root)
            .into_iter()
            .map(|h| (h, doc.objects.get(&h).expect("tree object").clone()))
            .collect()
    }

    #[test]
    fn copy_deep_clones_extension_dictionary_tree() {
        let (mut scene, src, root, _sub, _deep, rec) = scene_with_xdict_tree();
        let before = tree_image(&scene.document, root);

        let copies = scene.copy_entities(&[src], &EntityTransform::Translate(DVec3::ZERO));
        assert_eq!(copies.len(), 1);
        let copy = copies[0];
        assert!(!copy.is_null() && copy != src);

        let copy_root = scene
            .document
            .get_entity(copy)
            .expect("copy entity")
            .common()
            .xdictionary_handle
            .expect("copy must own a deep-cloned extension dictionary");
        assert_ne!(copy_root, root, "copy must not claim the source's root");

        // Structure mirrors the source shape on fresh handles.
        let copy_tree = tree_handles(&scene.document, copy_root);
        assert_eq!(
            copy_tree.len(),
            4,
            "cloned tree must mirror the source: {copy_tree:?}"
        );
        for h in &copy_tree {
            assert!(
                !tree_handles(&scene.document, root).contains(h),
                "clone must not reuse source handles"
            );
        }

        // Root wiring: owner and reactor rewritten to the copy.
        let ObjectType::Dictionary(copy_root_obj) = scene
            .document
            .objects
            .get(&copy_root)
            .expect("copy root object")
        else {
            panic!("copy root must be a dictionary");
        };
        assert_eq!(
            copy_root_obj.owner, copy,
            "cloned root must be owned by the copy"
        );
        assert_eq!(
            copy_root_obj.reactors,
            vec![copy],
            "reactor naming the source must be rewritten to the copy"
        );

        // Children resolve by key with preserved content.
        let sub_h = copy_root_obj.get("SUB").expect("SUB entry on copy");
        let copy_rec = copy_root_obj.get("REC").expect("REC entry on copy");
        let ObjectType::Dictionary(sub_obj) =
            scene.document.objects.get(&sub_h).expect("copy sub object")
        else {
            panic!("copy SUB must be a dictionary");
        };
        assert_eq!(
            sub_obj.owner, copy_root,
            "nested owner must point at the cloned parent"
        );
        let deep_h = sub_obj.get("DEEP").expect("DEEP entry on copy");
        let ObjectType::XRecord(deep_obj) = scene
            .document
            .objects
            .get(&deep_h)
            .expect("copy deep object")
        else {
            panic!("copy DEEP must be an XRecord");
        };
        assert_eq!(deep_obj.name, "DEEP");
        let ObjectType::XRecord(copy_rec_obj) = scene
            .document
            .objects
            .get(&copy_rec)
            .expect("copy REC object")
        else {
            panic!("copy REC must be an XRecord");
        };
        assert_eq!(copy_rec_obj.name, "REC");
        assert!(
            copy_rec_obj
                .entries
                .iter()
                .any(|e| matches!(&e.value, XRecordValue::String(s) if s == "CLONE_ME")),
            "cloned XRecord must carry the string content"
        );

        // Mutation independence: edit the copy, source stays untouched.
        if let Some(ObjectType::XRecord(x)) = scene.document.objects.get_mut(&copy_rec) {
            if let Some(e) = x
                .entries
                .iter_mut()
                .find(|e| matches!(&e.value, XRecordValue::String(s) if s == "CLONE_ME"))
            {
                e.value = XRecordValue::String("MUTATED".into());
            }
        }
        let ObjectType::XRecord(src_rec_obj) =
            scene.document.objects.get(&rec).expect("source REC object")
        else {
            panic!("source REC must be an XRecord");
        };
        assert!(
            src_rec_obj
                .entries
                .iter()
                .any(|e| matches!(&e.value, XRecordValue::String(s) if s == "CLONE_ME")),
            "source XRecord must be unaffected by edits on the copy"
        );

        // Source tree byte-for-byte unchanged.
        let after = tree_image(&scene.document, root);
        assert_eq!(
            before, after,
            "source extension-dictionary tree must be byte-for-byte unchanged"
        );
    }

    #[test]
    fn copy_without_xdict_adds_no_objects_and_keeps_delta_undo() {
        let mut scene = Scene::new();
        let line = codec::entities::Line::new();
        let src = scene
            .document
            .add_entity(codec::EntityType::Line(line))
            .expect("add source entity");
        let objects_before = scene.document.objects.len();

        scene.begin_undo_recording();
        let copies = scene.copy_entities(&[src], &EntityTransform::Translate(DVec3::ZERO));
        let objects_after = scene.document.objects.len();

        assert_eq!(copies.len(), 1);
        assert_ne!(copies[0], src);
        assert_eq!(
            objects_before, objects_after,
            "xdict-free copy must add no document.objects entries"
        );
        let recording = scene.take_undo_recording().expect("recording open");
        assert!(
            !recording.poisoned,
            "xdict-free copy must stay on the delta-undo path"
        );
    }

    #[test]
    fn copy_with_dangling_xdict_root_leaves_copy_without_one() {
        let mut scene = Scene::new();
        let missing = scene.document.allocate_handle();
        let mut line = codec::entities::Line::new();
        line.common.xdictionary_handle = Some(missing);
        let src = scene
            .document
            .add_entity(codec::EntityType::Line(line))
            .expect("add source entity");
        let objects_before = scene.document.objects.len();

        let copies = scene.copy_entities(&[src], &EntityTransform::Translate(DVec3::ZERO));

        let copy_root = scene
            .document
            .get_entity(copies[0])
            .expect("copy entity")
            .common()
            .xdictionary_handle;
        assert!(
            copy_root.is_none(),
            "dangling root must leave the copy without an extension dictionary, never shared"
        );
        assert_eq!(
            objects_before,
            scene.document.objects.len(),
            "dangling root must not invent objects"
        );
    }

    #[test]
    fn copy_cloned_xdict_cycle_terminates_and_remaps() {
        let (mut scene, src, root, ..) = scene_with_xdict_tree();
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&root) {
            d.add_entry("BACK", root);
        }

        let copies = scene.copy_entities(&[src], &EntityTransform::Translate(DVec3::ZERO));
        let copy_root = scene
            .document
            .get_entity(copies[0])
            .expect("copy entity")
            .common()
            .xdictionary_handle
            .expect("copy dictionary");
        let ObjectType::Dictionary(d) = scene.document.objects.get(&copy_root).expect("dict")
        else {
            panic!("dictionary");
        };
        assert_eq!(
            d.get("BACK"),
            Some(copy_root),
            "cyclic entry must be remapped to the cloned root, consistently"
        );
        assert_eq!(
            tree_handles(&scene.document, copy_root).len(),
            4,
            "cycle must not duplicate the tree"
        );
    }

    #[test]
    fn copy_keeps_references_outside_the_cloned_tree() {
        let (mut scene, src, root, ..) = scene_with_xdict_tree();
        let (_, rec) = {
            let ObjectType::Dictionary(d) = scene.document.objects.get(&root).expect("root") else {
                panic!("root must be a dictionary");
            };
            ((), d.get("REC").expect("REC entry"))
        };
        // An entity recorded inside the tree (e.g. a field pointing back at a
        // drawing entity) stays pointed at the original; entities are never
        // part of the object-tree clone.
        let mut other_line = codec::entities::Line::new();
        other_line.common.handle = Handle::NULL;
        let other = scene
            .document
            .add_entity(codec::EntityType::Line(other_line))
            .expect("other entity");
        // An object outside the reachable closure, referenced only through
        // the XRecord handle stream, likewise stays pointed at the original.
        let outside_h = scene.document.allocate_handle();
        let mut outside = Dictionary::new();
        outside.handle = outside_h;
        scene
            .document
            .objects
            .insert(outside_h, ObjectType::Dictionary(outside));
        if let Some(ObjectType::XRecord(x)) = scene.document.objects.get_mut(&rec) {
            x.add_handle(340, other);
            x.object_references.push(ProxyObjectReference {
                handle: outside_h,
                kind: ProxyReferenceKind::SoftPointer,
            });
        }

        let copies = scene.copy_entities(&[src], &EntityTransform::Translate(DVec3::ZERO));
        let copy_root = scene
            .document
            .get_entity(copies[0])
            .expect("copy entity")
            .common()
            .xdictionary_handle
            .expect("copy dictionary");
        let ObjectType::Dictionary(d) = scene.document.objects.get(&copy_root).expect("dict")
        else {
            panic!("dictionary");
        };
        let copy_rec = d.get("REC").expect("REC entry on copy");
        let ObjectType::XRecord(copy_rec_obj) =
            scene.document.objects.get(&copy_rec).expect("copy REC")
        else {
            panic!("copy REC must be an XRecord");
        };
        assert!(
            copy_rec_obj
                .entries
                .iter()
                .any(|e| matches!(&e.value, XRecordValue::Handle(h) if *h == other)),
            "entity references leaving the object tree must stay pointed at the original"
        );
        assert!(
            copy_rec_obj
                .object_references
                .iter()
                .any(|r| r.handle == outside_h),
            "handle-stream references leaving the source tree must stay pointed at the original"
        );
    }

    #[test]
    fn each_copy_gets_its_own_disjoint_tree() {
        let (mut scene, src, root, ..) = scene_with_xdict_tree();
        let source_tree = tree_handles(&scene.document, root);
        let mut all_trees = vec![source_tree];

        for _ in 0..3 {
            let copies = scene.copy_entities(&[src], &EntityTransform::Translate(DVec3::ZERO));
            let copy_root = scene
                .document
                .get_entity(copies[0])
                .expect("copy entity")
                .common()
                .xdictionary_handle
                .expect("each copy must own a dictionary");
            all_trees.push(tree_handles(&scene.document, copy_root));
        }

        for (i, a) in all_trees.iter().enumerate() {
            for (j, b) in all_trees.iter().enumerate() {
                if i == j {
                    continue;
                }
                let overlap = a.iter().any(|h| b.contains(h));
                assert!(!overlap, "tree {i} and tree {j} must be disjoint");
            }
        }
    }

    /// A line whose extension dictionary is a fresh single-root dictionary
    /// owned by (and reactor-linked to) that line.
    fn add_line_with_root(scene: &mut Scene) -> (codec::Handle, codec::Handle) {
        let root_h = scene.document.allocate_handle();
        let mut root = Dictionary::new();
        root.handle = root_h;
        scene
            .document
            .objects
            .insert(root_h, ObjectType::Dictionary(root));
        let mut line = codec::entities::Line::new();
        line.common.xdictionary_handle = Some(root_h);
        let src = scene
            .document
            .add_entity(codec::EntityType::Line(line))
            .expect("add line");
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&root_h) {
            d.owner = src;
            d.reactors.push(src);
        }
        (src, root_h)
    }

    fn xdict_root_of(scene: &Scene, entity: codec::Handle) -> codec::Handle {
        scene
            .document
            .get_entity(entity)
            .expect("entity")
            .common()
            .xdictionary_handle
            .expect("entity must own an extension dictionary")
    }

    #[test]
    fn copy_entities_remaps_cross_tree_refs_between_copied_owners() {
        let mut scene = Scene::new();
        let (a, a_root) = add_line_with_root(&mut scene);
        let (b, b_root) = add_line_with_root(&mut scene);
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&a_root) {
            d.add_entry("XREF", b_root);
        }

        let copies = scene.copy_entities(&[a, b], &EntityTransform::Translate(DVec3::ZERO));
        assert_eq!(copies.len(), 2);
        let (a_copy, b_copy) = (copies[0], copies[1]);
        let a_copy_root = xdict_root_of(&scene, a_copy);
        let b_copy_root = xdict_root_of(&scene, b_copy);
        assert_ne!(a_copy_root, a_root);
        assert_ne!(b_copy_root, b_root);

        let Some(ObjectType::Dictionary(a_copy_d)) = scene.document.objects.get(&a_copy_root)
        else {
            panic!("copied root must be a dictionary");
        };
        let xref = a_copy_d
            .get("XREF")
            .expect("cross-tree entry must survive on the copy");
        assert_eq!(
            xref, b_copy_root,
            "a cross-owner ref inside the copy set must land on the sibling's clone"
        );
        assert_ne!(
            xref, b_root,
            "a cross-owner ref must never stay pointed at the source sibling"
        );
    }

    #[test]
    fn copy_entities_remaps_cross_tree_ref_cycles_between_owners() {
        let mut scene = Scene::new();
        let (a, a_root) = add_line_with_root(&mut scene);
        let (b, b_root) = add_line_with_root(&mut scene);
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&a_root) {
            d.add_entry("TO_B", b_root);
        }
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&b_root) {
            d.add_entry("TO_A", a_root);
        }

        let copies = scene.copy_entities(&[a, b], &EntityTransform::Translate(DVec3::ZERO));
        let a_copy_root = xdict_root_of(&scene, copies[0]);
        let b_copy_root = xdict_root_of(&scene, copies[1]);
        let Some(ObjectType::Dictionary(a_copy_d)) = scene.document.objects.get(&a_copy_root)
        else {
            panic!("A copy root must be a dictionary");
        };
        let Some(ObjectType::Dictionary(b_copy_d)) = scene.document.objects.get(&b_copy_root)
        else {
            panic!("B copy root must be a dictionary");
        };
        assert_eq!(
            a_copy_d.get("TO_B"),
            Some(b_copy_root),
            "A's cross-ref must land on B's clone"
        );
        assert_eq!(
            b_copy_d.get("TO_A"),
            Some(a_copy_root),
            "B's cross-ref must land on A's clone"
        );
    }

    #[test]
    fn copy_entities_remaps_cross_refs_to_nested_sibling_objects() {
        let mut scene = Scene::new();
        let (a, a_root) = add_line_with_root(&mut scene);
        let (b, b_root) = add_line_with_root(&mut scene);
        let b_sub = scene.document.allocate_handle();
        let mut sub = Dictionary::new();
        sub.handle = b_sub;
        sub.owner = b_root;
        scene
            .document
            .objects
            .insert(b_sub, ObjectType::Dictionary(sub));
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&b_root) {
            d.add_entry("SUB", b_sub);
        }
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&a_root) {
            d.add_entry("XREF_SUB", b_sub);
        }

        let copies = scene.copy_entities(&[a, b], &EntityTransform::Translate(DVec3::ZERO));
        let a_copy_root = xdict_root_of(&scene, copies[0]);
        let b_copy_root = xdict_root_of(&scene, copies[1]);
        let Some(ObjectType::Dictionary(b_copy_d)) = scene.document.objects.get(&b_copy_root)
        else {
            panic!("B copy root must be a dictionary");
        };
        let b_copy_sub = b_copy_d.get("SUB").expect("SUB entry on B's clone");
        let Some(ObjectType::Dictionary(a_copy_d)) = scene.document.objects.get(&a_copy_root)
        else {
            panic!("A copy root must be a dictionary");
        };
        let xref_sub = a_copy_d
            .get("XREF_SUB")
            .expect("cross-tree entry to a nested object must survive");
        assert_eq!(
            xref_sub, b_copy_sub,
            "a cross-owner ref to a nested tree object must land on that object's clone"
        );
        assert_ne!(
            xref_sub, b_sub,
            "a nested cross-ref must never stay pointed at the source object"
        );
    }

    #[test]
    fn copy_entities_remaps_tree_refs_to_copied_entities() {
        let mut scene = Scene::new();
        let (a, a_root) = add_line_with_root(&mut scene);
        let (b, _b_root) = add_line_with_root(&mut scene);
        let rec_h = scene.document.allocate_handle();
        let mut rec = XRecord::named("LINK");
        rec.handle = rec_h;
        rec.owner = a_root;
        rec.add_handle(340, b);
        scene
            .document
            .objects
            .insert(rec_h, ObjectType::XRecord(rec));
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&a_root) {
            d.add_entry("LINK", rec_h);
        }

        let copies = scene.copy_entities(&[a, b], &EntityTransform::Translate(DVec3::ZERO));
        let b_copy = copies[1];
        let a_copy_root = xdict_root_of(&scene, copies[0]);
        let Some(ObjectType::Dictionary(a_copy_d)) = scene.document.objects.get(&a_copy_root)
        else {
            panic!("A copy root must be a dictionary");
        };
        let copy_rec_h = a_copy_d.get("LINK").expect("LINK entry on the copy");
        let Some(ObjectType::XRecord(copy_rec)) = scene.document.objects.get(&copy_rec_h) else {
            panic!("LINK must be an XRecord");
        };
        assert!(
            copy_rec
                .entries
                .iter()
                .any(|e| matches!(&e.value, XRecordValue::Handle(h) if *h == b_copy)),
            "a tree ref to a copied entity must land on that entity's copy"
        );
    }

    /// Same cross-tree structure as the both-copied test, but the sibling
    /// holding the cross-ref is copied second: ownership of a shared root
    /// must stay with its own entity regardless of selection order.
    #[test]
    fn copy_entities_remaps_cross_tree_refs_regardless_of_selection_order() {
        let mut scene = Scene::new();
        let (a, a_root) = add_line_with_root(&mut scene);
        let (b, b_root) = add_line_with_root(&mut scene);
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&a_root) {
            d.add_entry("XREF", b_root);
        }

        // Copies come back in ascending source-handle order regardless of
        // the order picked.
        let copies = scene.copy_entities(&[b, a], &EntityTransform::Translate(DVec3::ZERO));
        assert_eq!(copies.len(), 2);
        let (a_copy, b_copy) = (copies[0], copies[1]);
        let a_copy_root = xdict_root_of(&scene, a_copy);
        let b_copy_root = xdict_root_of(&scene, b_copy);
        let Some(ObjectType::Dictionary(a_copy_d)) = scene.document.objects.get(&a_copy_root)
        else {
            panic!("copied root must be a dictionary");
        };
        assert_eq!(
            a_copy_d.get("XREF"),
            Some(b_copy_root),
            "a cross-owner ref must land on the sibling's clone in any copy order"
        );
    }

    /// A sibling root ref'd by TWO other copied trees must stay owned by its
    /// own entity: ownership clobbered to the last entity whose tree reaches
    /// it would make the true owner's pass skip its own root and panic.
    #[test]
    fn copy_entities_shared_root_owner_survives_multiple_cross_refs() {
        let mut scene = Scene::new();
        let (a, a_root) = add_line_with_root(&mut scene);
        let (b, b_root) = add_line_with_root(&mut scene);
        let (c, c_root) = add_line_with_root(&mut scene);
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&a_root) {
            d.add_entry("XREF", b_root);
        }
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&c_root) {
            d.add_entry("XREF", b_root);
        }

        let copies = scene.copy_entities(&[a, b, c], &EntityTransform::Translate(DVec3::ZERO));
        assert_eq!(copies.len(), 3);
        let a_copy_root = xdict_root_of(&scene, copies[0]);
        let b_copy_root = xdict_root_of(&scene, copies[1]);
        let c_copy_root = xdict_root_of(&scene, copies[2]);
        let Some(ObjectType::Dictionary(a_copy_d)) = scene.document.objects.get(&a_copy_root)
        else {
            panic!("A copy root must be a dictionary");
        };
        let Some(ObjectType::Dictionary(c_copy_d)) = scene.document.objects.get(&c_copy_root)
        else {
            panic!("C copy root must be a dictionary");
        };
        assert_eq!(
            a_copy_d.get("XREF"),
            Some(b_copy_root),
            "A's cross-ref must land on B's clone"
        );
        assert_eq!(
            c_copy_d.get("XREF"),
            Some(b_copy_root),
            "C's cross-ref must land on B's clone"
        );
    }

    /// A ref to a tree outside the copy set must not dangle at the source:
    /// the reference product deep-clones the out-of-set dictionary target
    /// (measured via console probe on the drawing under test), so the copy
    /// owns an isomorphic private clone.
    #[test]
    fn copy_entities_deep_clones_uncopied_owner_trees() {
        let mut scene = Scene::new();
        let (a, a_root) = add_line_with_root(&mut scene);
        let (_b, b_root) = add_line_with_root(&mut scene);
        let b_sub = scene.document.allocate_handle();
        let mut sub = Dictionary::new();
        sub.handle = b_sub;
        sub.owner = b_root;
        scene
            .document
            .objects
            .insert(b_sub, ObjectType::Dictionary(sub));
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&b_root) {
            d.add_entry("SUB", b_sub);
        }
        if let Some(ObjectType::Dictionary(d)) = scene.document.objects.get_mut(&a_root) {
            d.add_entry("XREF", b_root);
        }

        let copies = scene.copy_entities(&[a], &EntityTransform::Translate(DVec3::ZERO));
        let a_copy_root = xdict_root_of(&scene, copies[0]);
        let Some(ObjectType::Dictionary(a_copy_d)) = scene.document.objects.get(&a_copy_root)
        else {
            panic!("A copy root must be a dictionary");
        };
        let xref = a_copy_d
            .get("XREF")
            .expect("cross-tree entry must survive on the copy");
        assert_ne!(
            xref, b_root,
            "an out-of-set target must be deep-cloned, never kept verbatim"
        );
        assert_ne!(
            xref, b_sub,
            "the inline clone is a fresh root, not the source's nested object"
        );
        // The inline clone is isomorphic: it carries its own SUB entry.
        let Some(ObjectType::Dictionary(cloned)) = scene.document.objects.get(&xref) else {
            panic!("out-of-set target clone must be a dictionary");
        };
        let cloned_sub = cloned.get("SUB").expect("SUB entry on the inline clone");
        assert_ne!(
            cloned_sub, b_sub,
            "the inline clone's subtree must also be cloned, not shared"
        );
        // The source's own tree is untouched by the copy.
        let Some(ObjectType::Dictionary(b_d)) = scene.document.objects.get(&b_root) else {
            panic!("source B root must still exist");
        };
        assert_eq!(b_d.get("SUB"), Some(b_sub));
    }
}

#[cfg(test)]
mod copy_entities_subhandle_tests {
    use super::*;
    use crate::command::EntityTransform;
    use codec::entities::{
        AttributeEntity, Insert, PolyfaceFace, PolyfaceMesh, PolyfaceVertex, PolygonMeshEntity,
        PolygonMeshVertex,
    };
    use codec::types::Vector3;
    use glam::DVec3;

    /// A polyface mesh as the DWG reader returns it: every vertex, face and
    /// the SEQEND carry their own handle.
    fn loaded_polyface(doc: &mut codec::Document) -> PolyfaceMesh {
        let mut mesh = PolyfaceMesh::new();
        for (x, y) in [(20.0, 0.0), (30.0, 0.0), (30.0, 10.0), (20.0, 10.0)] {
            let mut v = PolyfaceVertex::new(Vector3::new(x, y, 0.0));
            v.common.handle = doc.allocate_handle();
            mesh.add_vertex(v);
        }
        let mut face = PolyfaceFace::quad(1, 2, 3, 4);
        face.common.handle = doc.allocate_handle();
        mesh.add_face(face);
        mesh.seqend_handle = Some(doc.allocate_handle());
        mesh
    }

    fn sub_handles(e: &EntityType) -> Vec<codec::Handle> {
        match e {
            EntityType::Insert(ins) => ins
                .attributes
                .iter()
                .map(|a| a.common.handle)
                .chain(ins.seqend_handle)
                .collect(),
            EntityType::PolyfaceMesh(m) => m
                .vertices
                .iter()
                .map(|v| v.common.handle)
                .chain(m.faces.iter().map(|f| f.common.handle))
                .chain(m.seqend_handle)
                .collect(),
            EntityType::PolygonMesh(m) => m.vertices.iter().map(|v| v.common.handle).collect(),
            _ => Vec::new(),
        }
    }

    /// COPY must give every stored sub-entity handle of the copy a new value.
    /// The DWG writer reuses non-null sub-handles, so a shared one makes the
    /// copy point at the source's vertices / SEQEND in the saved file.
    #[test]
    fn copy_gives_complex_entities_fresh_sub_handles() {
        let mut scene = Scene::new();
        let doc = &mut scene.document;

        let mesh = loaded_polyface(doc);
        let mut grid = PolygonMeshEntity::new();
        grid.m_vertex_count = 2;
        grid.n_vertex_count = 2;
        for (x, y) in [(40.0, 0.0), (40.0, 10.0), (50.0, 0.0), (50.0, 10.0)] {
            let mut v = PolygonMeshVertex::at(Vector3::new(x, y, 0.0));
            v.common.handle = doc.allocate_handle();
            grid.vertices.push(v);
        }
        let mut ins = Insert::new("B", Vector3::new(0.0, 0.0, 0.0));
        let mut att = AttributeEntity::new("TAG1".into(), "val".into());
        att.common.handle = doc.allocate_handle();
        ins.attributes.push(att);
        ins.seqend_handle = Some(doc.allocate_handle());

        let sources: Vec<codec::Handle> = [
            EntityType::PolyfaceMesh(mesh),
            EntityType::PolygonMesh(grid),
            EntityType::Insert(ins),
        ]
        .into_iter()
        .map(|e| doc.add_entity(e).unwrap())
        .collect();

        let copies =
            scene.copy_entities(&sources, &EntityTransform::Translate(DVec3::new(100.0, 0.0, 0.0)));
        assert_eq!(copies.len(), sources.len());

        let mut seen = std::collections::HashSet::new();
        for h in sources.iter().chain(copies.iter()) {
            let subs = sub_handles(scene.document.get_entity(*h).unwrap());
            assert!(!subs.is_empty());
            for s in subs {
                assert!(!s.is_null(), "sub-handle of {h:?} must be set");
                assert!(seen.insert(s), "sub-handle {s:?} is shared between entities");
            }
        }
    }

    /// End to end through DWG: two copies of a polyface each read back with
    /// their own four vertices and face, at their own position.
    #[test]
    fn copied_polyfaces_keep_their_vertices_through_a_dwg_roundtrip() {
        let mut scene = Scene::new();
        let mesh = loaded_polyface(&mut scene.document);
        let src = scene.document.add_entity(EntityType::PolyfaceMesh(mesh)).unwrap();
        for dx in [100.0, 200.0] {
            scene.copy_entities(&[src], &EntityTransform::Translate(DVec3::new(dx, 0.0, 0.0)));
        }

        let bytes = codec::DwgWriter::write_to_vec(&scene.document).unwrap();
        let reopened = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
            .read()
            .unwrap();
        let mut first_x: Vec<f64> = reopened
            .entities()
            .filter_map(|e| match e {
                EntityType::PolyfaceMesh(m) => {
                    assert_eq!(m.vertices.len(), 4, "every polyface keeps its vertices");
                    assert_eq!(m.faces.len(), 1, "every polyface keeps its face");
                    Some(m.vertices[0].location.x)
                }
                _ => None,
            })
            .collect();
        first_x.sort_by(f64::total_cmp);
        assert_eq!(first_x, vec![20.0, 120.0, 220.0]);
    }
}

#[cfg(test)]
mod copy_viewport_clip_tests {
    use super::*;
    use crate::command::EntityTransform;
    use codec::entities::{LwPolyline, Viewport};
    use glam::DVec3;

    fn clipped_viewport(scene: &mut Scene) -> (Handle, Handle) {
        let mut boundary = LwPolyline::new();
        boundary.add_point(codec::types::Vector2::new(0.0, 0.0));
        boundary.add_point(codec::types::Vector2::new(10.0, 0.0));
        boundary.add_point(codec::types::Vector2::new(5.0, 8.0));
        boundary.is_closed = true;
        let boundary = scene.add_entity(EntityType::LwPolyline(boundary));
        let mut viewport = Viewport::new();
        viewport.clip_boundary_handle = boundary;
        viewport.status.non_rectangular_clipping = true;
        let viewport = scene.add_entity(EntityType::Viewport(Box::new(viewport)));
        (viewport, boundary)
    }

    fn clip_of(scene: &Scene, viewport: Handle) -> Handle {
        match scene.document.get_entity(viewport) {
            Some(EntityType::Viewport(v)) => v.clip_boundary_handle,
            other => panic!("expected a viewport, got {other:?}"),
        }
    }

    #[test]
    fn copied_viewport_is_clipped_by_the_copied_boundary() {
        let mut scene = Scene::new();
        let (viewport, boundary) = clipped_viewport(&mut scene);
        let copies = scene.copy_entities(
            &[viewport, boundary],
            &EntityTransform::Translate(DVec3::new(100.0, 0.0, 0.0)),
        );
        let copied_viewport = copies
            .iter()
            .copied()
            .find(|&h| matches!(scene.document.get_entity(h), Some(EntityType::Viewport(_))))
            .expect("copied viewport");
        let copied_boundary = copies
            .iter()
            .copied()
            .find(|&h| {
                matches!(
                    scene.document.get_entity(h),
                    Some(EntityType::LwPolyline(_))
                )
            })
            .expect("copied boundary");
        assert_eq!(clip_of(&scene, copied_viewport), copied_boundary);
        assert_eq!(clip_of(&scene, viewport), boundary);
    }
}

#[cfg(test)]
mod copy_field_tests {
    use super::*;
    use crate::command::EntityTransform;
    use codec::objects::{Dictionary, Field, ObjectType};
    use glam::DVec3;

    /// `owner`'s extension dictionary: ACAD_FIELD { TEXT -> field(child) }.
    /// Returns (root, text field, child field).
    fn add_field_tree(doc: &mut codec::Document, owner: Handle) -> (Handle, Handle, Handle) {
        let root_h = doc.allocate_handle();
        let fields_h = doc.allocate_handle();
        let text_h = doc.allocate_handle();
        let child_h = doc.allocate_handle();
        let mut root = Dictionary::new();
        root.handle = root_h;
        root.owner = owner;
        root.add_entry("ACAD_FIELD", fields_h);
        let mut fields = Dictionary::new();
        fields.handle = fields_h;
        fields.owner = root_h;
        fields.add_entry("TEXT", text_h);
        let text = Field {
            handle: text_h,
            owner: fields_h,
            evaluator_id: "_text".to_string(),
            child_fields: vec![child_h],
            ..Default::default()
        };
        let child = Field {
            handle: child_h,
            owner: text_h,
            evaluator_id: "AcVar".to_string(),
            code: r"\AcVar CustomDP.Client".to_string(),
            ..Default::default()
        };
        doc.objects.insert(root_h, ObjectType::Dictionary(root));
        doc.objects.insert(fields_h, ObjectType::Dictionary(fields));
        doc.objects.insert(text_h, ObjectType::Field(text));
        doc.objects.insert(child_h, ObjectType::Field(child));
        (root_h, text_h, child_h)
    }

    fn entry(doc: &codec::Document, dict: Handle, key: &str) -> Handle {
        match doc.objects.get(&dict) {
            Some(ObjectType::Dictionary(d)) => d
                .entries
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, h)| *h)
                .expect("dictionary entry"),
            other => panic!("expected a dictionary, got {other:?}"),
        }
    }

    fn field(doc: &codec::Document, h: Handle) -> &Field {
        match doc.objects.get(&h) {
            Some(ObjectType::Field(f)) => f,
            other => panic!("expected a field, got {other:?}"),
        }
    }

    /// The copy's ACAD_FIELD tree is its own: fresh handles stored on the
    /// objects themselves, owners and child links inside the new tree.
    fn assert_own_field_tree(
        doc: &codec::Document,
        copy_root: Handle,
        src_text: Handle,
        src_child: Handle,
    ) {
        let fields = entry(doc, copy_root, "ACAD_FIELD");
        let text_h = entry(doc, fields, "TEXT");
        assert_ne!(text_h, src_text);
        let text = field(doc, text_h);
        assert_eq!(text.handle, text_h, "field object keeps its source handle");
        assert_eq!(text.owner, fields);
        assert_eq!(text.child_fields.len(), 1);
        let child_h = text.child_fields[0];
        assert_ne!(child_h, src_child, "copy shares the source's child field");
        let child = field(doc, child_h);
        assert_eq!(child.handle, child_h);
        assert_eq!(child.owner, text_h);
        assert_eq!(child.code, r"\AcVar CustomDP.Client");
    }

    #[test]
    fn copied_text_gets_its_own_field_tree() {
        let mut scene = Scene::new();
        let src = scene
            .document
            .add_entity(EntityType::MText(Box::new(codec::entities::MText::new())))
            .unwrap();
        let (root, text, child) = add_field_tree(&mut scene.document, src);
        scene
            .document
            .get_entity_mut(src)
            .unwrap()
            .common_mut()
            .xdictionary_handle = Some(root);
        let copies = scene.copy_entities(
            &[src],
            &EntityTransform::Translate(DVec3::new(100.0, 0.0, 0.0)),
        );
        let copy_root = scene
            .document
            .get_entity(copies[0])
            .unwrap()
            .common()
            .xdictionary_handle
            .expect("copy has an extension dictionary");
        assert_ne!(copy_root, root);
        assert_own_field_tree(&scene.document, copy_root, text, child);
        assert_eq!(field(&scene.document, text).child_fields, vec![child]);
    }

    #[test]
    fn copied_insert_attribute_gets_its_own_field_tree() {
        let mut scene = Scene::new();
        let mut insert = codec::entities::Insert::new(
            "TITLE".to_string(),
            codec::types::Vector3::ZERO,
        );
        insert.attributes.push(codec::entities::AttributeEntity::new(
            "CLIENT".to_string(),
            "ACME".to_string(),
        ));
        let src = scene.document.add_entity(EntityType::Insert(insert)).unwrap();
        let src_attribute = match scene.document.get_entity(src) {
            Some(EntityType::Insert(ins)) => ins.attributes[0].common.handle,
            _ => unreachable!(),
        };
        let (root, text, child) = add_field_tree(&mut scene.document, src_attribute);
        if let Some(EntityType::Insert(ins)) = scene.document.get_entity_mut(src) {
            ins.attributes[0].common.xdictionary_handle = Some(root);
        }
        let copies = scene.copy_entities(
            &[src],
            &EntityTransform::Translate(DVec3::new(100.0, 0.0, 0.0)),
        );
        let (copy_attribute, copy_root) = match scene.document.get_entity(copies[0]) {
            Some(EntityType::Insert(ins)) => (
                ins.attributes[0].common.handle,
                ins.attributes[0].common.xdictionary_handle,
            ),
            other => panic!("expected the copied insert, got {other:?}"),
        };
        let copy_root = copy_root.expect("copied attribute has an extension dictionary");
        assert_ne!(copy_root, root, "copied attribute shares the source's dictionary");
        match scene.document.objects.get(&copy_root) {
            Some(ObjectType::Dictionary(d)) => assert_eq!(d.owner, copy_attribute),
            other => panic!("expected a dictionary, got {other:?}"),
        }
        match scene.document.objects.get(&root) {
            Some(ObjectType::Dictionary(d)) => assert_eq!(d.owner, src_attribute),
            other => panic!("expected a dictionary, got {other:?}"),
        }
        assert_own_field_tree(&scene.document, copy_root, text, child);
    }
}

#[cfg(test)]
mod table_picture_ownership_tests {
    use crate::command::{CadCommand, CmdResult, EntityTransform};
    use crate::modules::annotate::table_cmd::TableCellEditCommand;
    use crate::scene::Scene;
    use codec::entities::Table;
    use codec::types::Vector3;
    use codec::{EntityType, Handle};
    use glam::DVec3;

    fn table(scene: &Scene, handle: Handle) -> &Table {
        match scene.document.get_entity(handle) {
            Some(EntityType::Table(table)) => table,
            other => panic!("expected TABLE, got {other:?}"),
        }
    }

    fn source_table(scene: &mut Scene) -> Handle {
        let mut source = Table::new(Vector3::ZERO, 3, 2);
        source.set_cell_text(0, 0, "Title");
        source.set_cell_text(1, 0, "Header");
        source.set_cell_text(2, 0, "A");
        let handle = scene.add_entity(EntityType::Table(Box::new(source)));
        assert!(table(scene, handle).block_record_handle.is_some());
        assert!(table(scene, handle).block_name.starts_with("*T"));
        handle
    }

    fn picture(scene: &Scene, handle: Handle) -> (Handle, String, Vec<String>) {
        let table = table(scene, handle);
        let record = table
            .block_record_handle
            .and_then(|h| scene.document.block_records.iter().find(|r| r.handle == h))
            .or_else(|| scene.document.block_records.get(&table.block_name))
            .expect("table resolves to a picture block");
        let texts = record
            .entity_handles
            .iter()
            .filter_map(|h| match scene.document.get_entity(*h) {
                Some(EntityType::MText(text)) => Some(text.value.clone()),
                _ => None,
            })
            .collect();
        (record.handle, record.name.clone(), texts)
    }

    fn edit_copy(scene: &mut Scene, handle: Handle) {
        let mut command = TableCellEditCommand::new(handle, table(scene, handle), 2, 0);
        let result = command.on_text_input("B").expect("cell edit commits");
        let CmdResult::ReplaceMany(mut replacements, _) = result else {
            panic!("expected TABLEDIT replacement");
        };
        let (target, mut entities) = replacements.remove(0);
        assert_eq!(target, handle);
        // update_entity commits the replacement and runs bump_entities, the
        // same refresh_table_block rebuild used by the cell-edit path.
        assert!(scene.update_entity(entities.remove(0)));
    }

    #[test]
    fn copied_table_edit_keeps_source_picture_text() {
        let mut scene = Scene::new();
        let source = source_table(&mut scene);
        let before = picture(&scene, source);
        scene.begin_undo_recording();
        let copies = scene.copy_entities(
            &[source],
            &EntityTransform::Translate(DVec3::new(30.0, 0.0, 0.0)),
        );
        assert_eq!(copies.len(), 1);
        let copy = copies[0];
        edit_copy(&mut scene, copy);
        let original_picture = picture(&scene, source);
        let copied_picture = picture(&scene, copy);
        assert_ne!(
            original_picture.0, copied_picture.0,
            "TABLE picture handles are shared"
        );
        assert_ne!(
            original_picture.1, copied_picture.1,
            "TABLE picture names are shared"
        );
        assert_eq!(
            original_picture, before,
            "source picture was modified by the copy's edit"
        );
        assert!(original_picture.2.iter().any(|text| text == "A"));
        assert!(copied_picture.2.iter().any(|text| text == "B"));
        assert_eq!(table(&scene, source).cell_text(2, 0), Some("A"));
        assert_eq!(table(&scene, copy).cell_text(2, 0), Some("B"));
        assert!(scene.take_undo_recording().unwrap().is_poisoned());
    }

    #[test]
    fn editing_a_legacy_shared_picture_detaches_its_name() {
        let mut scene = Scene::new();
        let source = source_table(&mut scene);
        let before = picture(&scene, source);
        // Simulate a drawing already containing shared references, bypassing
        // the clone helper to exercise the independent edit-time protection.
        let mut copy = scene.document.get_entity(source).unwrap().clone();
        copy.common_mut().handle = Handle::NULL;
        let copy = scene.document.add_entity(copy).unwrap();
        edit_copy(&mut scene, copy);
        assert_ne!(picture(&scene, copy).0, before.0);
        assert_eq!(picture(&scene, source), before);
    }

    #[test]
    fn clone_helper_drops_both_table_picture_references() {
        let mut scene = Scene::new();
        let source = source_table(&mut scene);
        let mut copy = scene.document.get_entity(source).unwrap().clone();
        Scene::reset_clone_subhandles(&mut scene.document, &mut copy);
        let EntityType::Table(copy) = copy else {
            unreachable!()
        };
        assert!(copy.block_record_handle.is_none());
        assert!(copy.block_name.is_empty());
        assert!(table(&scene, source).block_record_handle.is_some());
    }
}
