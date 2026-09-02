// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;

/// Order sampled boundary edges into one tip-to-tail loop.
pub(super) fn chain_path_edges(polys: Vec<Vec<[f64; 2]>>) -> Vec<[f64; 2]> {
    chain_path_edges_with_directions(polys).0
}

pub(super) fn chain_path_edges_with_directions(
    polys: Vec<Vec<[f64; 2]>>,
) -> (Vec<[f64; 2]>, Vec<f64>) {
    let d2 = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
    let mut directions = vec![0.0; polys.len()];
    let mut polys: Vec<_> = polys
        .into_iter()
        .enumerate()
        .filter(|(_, points)| !points.is_empty())
        .collect();
    if polys.is_empty() {
        return (Vec::new(), directions);
    }
    let (first_index, first) = polys.swap_remove(0);
    directions[first_index] = 1.0;
    let mut chain: std::collections::VecDeque<[f64; 2]> = first.into();
    while !polys.is_empty() {
        let head = *chain.front().unwrap();
        let tail = *chain.back().unwrap();
        // (distance, index, reverse-points, attach-at-front)
        let mut best = (f64::MAX, 0usize, false, false);
        for (i, (_, p)) in polys.iter().enumerate() {
            let s = p[0];
            let e = *p.last().unwrap();
            for c in [
                (d2(tail, s), i, false, false),
                (d2(tail, e), i, true, false),
                (d2(head, e), i, false, true),
                (d2(head, s), i, true, true),
            ] {
                if c.0 < best.0 {
                    best = c;
                }
            }
        }
        let (_, idx, rev, at_front) = best;
        let (original_index, mut p) = polys.swap_remove(idx);
        directions[original_index] = if rev { -1.0 } else { 1.0 };
        if rev {
            p.reverse();
        }
        if at_front {
            if d2(p[p.len() - 1], head) < 1e-18 {
                p.pop();
            }
            for q in p.into_iter().rev() {
                chain.push_front(q);
            }
        } else {
            let mut it = p.into_iter();
            if let Some(first) = it.next() {
                if d2(first, tail) >= 1e-18 {
                    chain.push_back(first);
                }
            }
            chain.extend(it);
        }
    }
    (chain.into(), directions)
}

fn family_from_stored_line(
    ln: &acadrust::entities::hatch::HatchPatternLine,
) -> crate::scene::model::hatch_model::PatFamily {
    let (ca, sa) = (ln.angle.cos(), ln.angle.sin());
    let dx = ln.offset.x * ca + ln.offset.y * sa;
    let dy = -ln.offset.x * sa + ln.offset.y * ca;
    crate::scene::model::hatch_model::PatFamily {
        angle_deg: ln.angle.to_degrees() as f32,
        x0: 0.0,
        y0: 0.0,
        dx: dx as f32,
        dy: dy as f32,
        dashes: ln.dash_lengths.iter().map(|&d| d as f32).collect(),
    }
}

/// Preserve the selected hue while moving its HSL lightness towards the
/// persisted one-colour tint/shade target (0 = black, 1 = white).
fn gradient_tint_color(base: [f32; 4], target: f32) -> [f32; 4] {
    let max = base[0].max(base[1]).max(base[2]);
    let min = base[0].min(base[1]).min(base[2]);
    let lightness = (max + min) * 0.5;
    let target = target.clamp(0.0, 1.0);
    let mut result = base;
    if target <= lightness {
        let factor = if lightness > 1.0e-6 {
            target / lightness
        } else {
            0.0
        };
        for channel in &mut result[..3] {
            *channel *= factor;
        }
    } else {
        let factor = if lightness < 1.0 - 1.0e-6 {
            (target - lightness) / (1.0 - lightness)
        } else {
            1.0
        };
        for channel in &mut result[..3] {
            *channel += (1.0 - *channel) * factor;
        }
    }
    result
}

impl Scene {
    // ── Entity management ─────────────────────────────────────────────────

    /// Register `name` in the layer table if it isn't already there, giving the
    /// new layer a real handle so it survives a DWG save (handle-based format;
    /// issue #67). Called whenever an entity is added or edited: an entity that
    /// names a layer no explicit LAYER command ever created — e.g. one supplied
    /// by a plugin through `add_entity` — otherwise has no table entry, so the
    /// DWG writer resolves its layer name to a NULL handle and it reopens on
    /// layer 0. Auto-registering keeps it on its own layer (#252). Names are
    /// registered verbatim so the writer's (case-insensitive) lookup matches;
    /// the always-present default layer "0" and empty names are no-ops.
    pub fn ensure_layer(&mut self, name: &str) {
        if name.trim().is_empty() || self.document.layers.contains(name) {
            return;
        }
        let mut layer = acadrust::tables::Layer::new(name);
        layer.handle = self.document.allocate_handle();
        let _ = self.document.layers.add(layer);
    }

    fn ensure_app_id(&mut self, name: &str) {
        if name.trim().is_empty() || self.document.app_ids.contains(name) {
            return;
        }
        let mut app_id = acadrust::tables::AppId::new(name);
        app_id.handle = self.document.allocate_handle();
        let _ = self.document.app_ids.add(app_id);
    }

    pub fn add_entity(&mut self, entity: EntityType) -> Handle {
        self.add_entity_internal(entity, true)
    }

    /// Batch-add several entities, publishing geometry changes once at the end.
    /// This is the fast path used by plugin `add_entities` requests.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn add_entities(&mut self, entities: Vec<EntityType>) -> Vec<Handle> {
        let mut handles = Vec::with_capacity(entities.len());
        let mut changes = Vec::with_capacity(entities.len());
        let mut needs_geometry_bump = false;

        for entity in entities {
            let affects_blocks = matches!(
                &entity,
                EntityType::Block(_) | EntityType::BlockEnd(_)
            );
            let handle = self.add_entity_internal(entity, false);
            handles.push(handle);
            if handle.is_null() {
                continue;
            }
            if affects_blocks {
                needs_geometry_bump = true;
            } else {
                changes.push((handle, ChangeKind::Added));
            }
        }

        if needs_geometry_bump {
            self.bump_geometry();
        } else if !changes.is_empty() {
            self.bump_entities(&changes);
        }

        handles
    }

    fn add_entity_internal(&mut self, mut entity: EntityType, bump: bool) -> Handle {
        // Only block sentinels mutate a block definition and require rebuilding
        // the block cache. A top-level INSERT merely references an existing
        // definition, so adding it can patch just that new render handle.
        let affects_blocks = matches!(
            &entity,
            EntityType::Block(_) | EntityType::BlockEnd(_)
        );
        // INSERT invalidates rendered block instances, but it does not mutate
        // the referenced block definition. Only block sentinels require a
        // structure image for undo; ordinary owner membership is intrinsic add
        // bookkeeping and remains in place while an entity delta is undone.
        let mutates_block_structure =
            matches!(&entity, EntityType::Block(_) | EntityType::BlockEnd(_));
        let hatch_seed = if let EntityType::Hatch(dxf) = &entity {
            let color = self.render_style(&entity).0;
            Self::hatch_model_from_dxf(dxf, color)
        } else if let EntityType::Solid(solid) = &entity {
            let color = self.render_style(&entity).0;
            Some(Self::solid_hatch_model(solid, color))
        } else {
            None
        };
        let image_seed = self.image_seed_for(&entity);
        let facet_res = self.document.header.facet_resolution;
        let chordal_deflection =
            crate::entities::solid3d::display_deflection(&self.document.header, facet_res);
        let isolines = self.document.header.isolines.max(0) as usize;
        let mesh_seed = if matches!(
            &entity,
            EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
                | EntityType::Mesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::PolyfaceMesh(_)
        ) {
            let color = self.render_style(&entity).0;
            crate::entities::solid3d::tessellate_volume(
                &entity,
                color,
                facet_res,
                chordal_deflection,
                isolines,
            )
            .map(offset_mesh_lod_set)
        } else {
            None
        };

        // Auto-create an ImageDefinition object for new RasterImage entities
        // that don't already reference one.
        if let EntityType::RasterImage(ref mut img) = entity {
            if img.definition_handle.is_none() {
                use acadrust::objects::{ImageDefinition, ObjectType};
                let def_handle = Handle::new(self.document.next_handle());
                if self.is_recording_undo() {
                    self.record_undo_object_before(def_handle, None);
                }
                let mut img_def = ImageDefinition::with_dimensions(
                    &img.file_path,
                    img.size.x as u32,
                    img.size.y as u32,
                );
                img_def.handle = def_handle;
                img_def.is_loaded = true;
                self.document
                    .objects
                    .insert(def_handle, ObjectType::ImageDefinition(img_def));
                img.definition_handle = Some(def_handle);
            }
        }

        // Register the entity's layer if it names one no LAYER command created
        // (e.g. a plugin-supplied layer) so it survives a DWG save instead of
        // collapsing to layer 0 in the reopened file (#252).
        let layer = entity.common().layer.clone();
        // Delta-undo poison inputs (captured before the mutations below): an
        // add that also creates a new layer, adds a block, or inserts an image
        // definition mutates non-entity state a pure-entity delta can't undo.
        let creates_layer =
            self.is_recording_undo() && !layer.trim().is_empty() && !self.document.layers.contains(&layer);
        self.ensure_layer(&layer);
        let app_ids: Vec<String> = entity
            .common()
            .extended_data
            .records()
            .iter()
            .map(|record| record.application_name.clone())
            .collect();
        let creates_app_id = self.is_recording_undo()
            && app_ids.iter().any(|name| {
                !name.trim().is_empty() && !self.document.app_ids.contains(name)
            });
        for name in &app_ids {
            self.ensure_app_id(name);
        }

        // Route to the correct block based on current editing mode:
        //   - BEDIT block editor: geometry belongs to the edited block record,
        //     so it becomes part of the block definition (issue #261).
        //   - PSPACE (paper layout, no active viewport): paper-space layout block.
        //   - MSPACE or model layout: model space (document default).
        let handle = if let Some(br) = self.block_edit_block {
            entity.common_mut().owner_handle = br;
            self.document.add_entity(entity).unwrap_or(Handle::NULL)
        } else if self.current_layout != "Model" && self.active_viewport.is_none() {
            let layout_name = self.current_layout.clone();
            self.document
                .add_entity_to_layout(entity, &layout_name)
                .unwrap_or(Handle::NULL)
        } else {
            self.document.add_entity(entity).unwrap_or(Handle::NULL)
        };

        if !handle.is_null() {
            self.invalidate_dependency_index();
            if let Some(model) = hatch_seed {
                self.hatches.insert(handle, model);
            }
            if let Some(model) = image_seed {
                self.images.insert(handle, model);
            }
            if let Some(mut model) = mesh_seed {
                if let Some(entity) = self.document.get_entity(handle) {
                    let color = self.render_style(entity).0;
                    let material =
                        crate::scene::model::material_model::resolve_material_with_base(
                            &self.document,
                            entity,
                            color,
                            None,
                            self.material_base_dir.as_deref(),
                        );
                    material.apply_to_with_face_overrides(
                        &mut model,
                        &self.document,
                        self.material_base_dir.as_deref(),
                    );
                    crate::scene::model::visual_style_model::apply_mesh_visual_style(
                        &mut model,
                        &self.document,
                        entity,
                    );
                }
                self.meshes.insert(handle, model);
            }
            // Delta-undo: the new handle's before-image is "nothing" (it did not
            // exist). Poison the recording if this add also mutated non-entity
            // state (a new layer, application ID, or block).
            if self.is_recording_undo() {
                self.record_undo_before(handle, None);
                if creates_layer || creates_app_id || mutates_block_structure {
                    self.poison_undo_recording();
                }
            }
            if bump {
                if affects_blocks {
                    self.bump_geometry();
                } else {
                    // Plain top-level add: name the new handle so every derived cache
                    // patches in just this one entity instead of rebuilding.
                    self.bump_entities(&[(handle, ChangeKind::Added)]);
                }
            }
        }
        handle
    }

    pub fn rename_layer(&mut self, old: &str, new: &str) -> bool {
        if self.document.rename_layer(old, new).is_err() {
            return false;
        }
        self.invalidate_dependency_index();
        true
    }

    /// Rename a block definition: re-key its record, update the Block marker's
    /// name, and repoint every INSERT that referenced the old name so all
    /// instances keep resolving. Returns false if `old` is missing or
    /// anonymous/xref, `new` is invalid or already taken, or the names are
    /// equal (case-insensitive). (#261)
    pub fn rename_block(&mut self, old: &str, new: &str) -> bool {
        if !crate::scene::valid_block_name(new) {
            return false;
        }
        if old.eq_ignore_ascii_case(new) {
            return false;
        }
        if self.document.block_records.get(new).is_some() {
            return false;
        }
        // Anonymous (*) names are program-owned and re-numbered on save; an
        // xref('|') symbol name is bound to the referenced file.
        if self
            .document
            .block_records
            .get(old)
            .map(|br| br.is_anonymous() || br.flags.is_xref || br.name.contains('|'))
            .unwrap_or(true)
        {
            return false;
        }
        let Some(mut br) = self.document.block_records.remove(old) else {
            return false;
        };
        let block_marker = br.block_entity_handle;
        br.name = new.to_string();
        if self.document.block_records.add(br).is_err() {
            return false;
        }
        // Keep the Block marker entity's name in sync (used on DXF/DWG save).
        if let Some(EntityType::Block(b)) = self.document.get_entity_mut(block_marker) {
            b.name = new.to_string();
        }
        // Repoint every INSERT reference so all instances keep resolving.
        for e in self.document.entities_mut() {
            if let EntityType::Insert(ins) = e {
                if ins.block_name.eq_ignore_ascii_case(old) {
                    ins.block_name = new.to_string();
                }
            }
        }
        self.invalidate_dependency_index();
        self.bump_geometry();
        true
    }

    /// Replace the entity stored under `entity`'s handle with `entity`, keeping
    /// its identity (handle + owning block), and refresh the derived
    /// hatch/image/mesh caches so the edit is visible. Returns `false` when no
    /// entity has that handle. This is the in-place counterpart to
    /// [`add_entity`](Self::add_entity) used to commit a plugin's edit of an
    /// existing entity.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn update_entity(&mut self, mut entity: EntityType) -> bool {
        let handle = entity.common().handle;
        if self.is_layer_locked(handle) {
            return false;
        }
        let Some(existing) = self.document.get_entity(handle) else {
            return false;
        };
        // The caller edited a snapshot copy; keep the live entity in its block.
        entity.common_mut().owner_handle = existing.common().owner_handle;

        // Replacing (or becoming) a block sentinel forces a full block-cache
        // rebuild. INSERT edits (including retargeting to another existing
        // definition) only change that top-level render handle.
        let affects_blocks = matches!(
            existing,
            EntityType::Block(_) | EntityType::BlockEnd(_)
        ) || matches!(
            &entity,
            EntityType::Block(_) | EntityType::BlockEnd(_)
        );

        // A plugin edit may retarget the entity to a novel layer; register it
        // so the edited entity keeps that layer on save instead of collapsing
        // to layer 0 in the reopened file (#252).
        let new_layer = entity.common().layer.clone();
        let creates_layer = self.is_recording_undo()
            && !new_layer.trim().is_empty()
            && !self.document.layers.contains(&new_layer);
        self.ensure_layer(&new_layer);

        // Rebuild the derived-model seeds from the new entity (as add_entity).
        let hatch_seed = if let EntityType::Hatch(dxf) = &entity {
            let color = self.render_style(&entity).0;
            Self::hatch_model_from_dxf(dxf, color)
        } else if let EntityType::Solid(solid) = &entity {
            let color = self.render_style(&entity).0;
            Some(Self::solid_hatch_model(solid, color))
        } else {
            None
        };
        let image_seed = self.image_seed_for(&entity);
        let facet_res = self.document.header.facet_resolution;
        let chordal_deflection =
            crate::entities::solid3d::display_deflection(&self.document.header, facet_res);
        let isolines = self.document.header.isolines.max(0) as usize;
        let mesh_seed = if matches!(
            &entity,
            EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
                | EntityType::Mesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::PolyfaceMesh(_)
        ) {
            let color = self.render_style(&entity).0;
            crate::entities::solid3d::tessellate_volume(
                &entity,
                color,
                facet_res,
                chordal_deflection,
                isolines,
            )
            .map(offset_mesh_lod_set)
        } else {
            None
        };

        // Delta-undo: capture the entity's pre-edit image (before the slot is
        // overwritten) so an undo can restore it, and poison if this replace
        // also created a layer or crossed a block boundary.
        if self.is_recording_undo() {
            let before = self.document.get_entity_arc(handle);
            self.record_undo_before(handle, before);
            if creates_layer || affects_blocks {
                self.poison_undo_recording();
            }
        }

        // Write the new entity into the live slot.
        let Some(slot) = self.document.get_entity_mut(handle) else {
            return false;
        };
        *slot = entity;
        self.invalidate_dependency_index();

        // Drop stale derived caches for this handle, then reseed for the new
        // entity's type (which may differ from the old one).
        self.hatches.remove(&handle);
        self.images.remove(&handle);
        self.meshes.remove(&handle);
        self.solid_models.remove(&handle);
        if let Some(model) = hatch_seed {
            self.hatches.insert(handle, model);
        }
        if let Some(model) = image_seed {
            self.images.insert(handle, model);
        }
        if let Some(mut model) = mesh_seed {
            if let Some(entity) = self.document.get_entity(handle) {
                let color = self.render_style(entity).0;
                let material =
                    crate::scene::model::material_model::resolve_material_with_base(
                        &self.document,
                        entity,
                        color,
                        None,
                        self.material_base_dir.as_deref(),
                    );
                material.apply_to_with_face_overrides(
                    &mut model,
                    &self.document,
                    self.material_base_dir.as_deref(),
                );
                crate::scene::model::visual_style_model::apply_mesh_visual_style(
                    &mut model,
                    &self.document,
                    entity,
                );
            }
            self.meshes.insert(handle, model);
        }

        if affects_blocks {
            self.mark_entity_dirty(handle);
            self.bump_geometry();
        } else {
            // One entity changed in place: report just this handle so every
            // derived cache patches it instead of rebuilding (bump_entities also
            // drops it from the tessellation memos).
            self.bump_entities(&[(handle, ChangeKind::Modified)]);
        }
        true
    }

    /// Rebuild the per-entity derived caches (hatch fill / raster image / solid
    /// mesh) for a single handle from whatever entity currently lives at it —
    /// or drop them all if the handle is now absent. Mirrors the reseed block in
    /// [`Scene::update_entity`]; used by delta-undo when it re-applies an
    /// entity's before / after image so the fills and meshes follow.
    pub(crate) fn reseed_derived_caches(&mut self, handle: Handle) {
        let (hatch_seed, image_seed) = match self.document.get_entity(handle) {
            None => (None, None),
            Some(entity) => {
                let hatch_seed = if let EntityType::Hatch(dxf) = entity {
                    let color = self.render_style(entity).0;
                    Self::hatch_model_from_dxf(dxf, color)
                } else if let EntityType::Solid(solid) = entity {
                    let color = self.render_style(entity).0;
                    Some(Self::solid_hatch_model(solid, color))
                } else {
                    None
                };
                let image_seed = self.image_seed_for(entity);
                (hatch_seed, image_seed)
            }
        };
        self.hatches.remove(&handle);
        self.images.remove(&handle);
        self.meshes.remove(&handle);
        self.solid_models.remove(&handle);
        if let Some(model) = hatch_seed {
            self.hatches.insert(handle, model);
        }
        if let Some(model) = image_seed {
            self.images.insert(handle, model);
        }
        self.refresh_meshes_for_handles(&[handle]);
        self.restore_solid_models(&[handle]);
    }

    pub fn restore_solid_models(&mut self, handles: &[Handle]) {
        let bodies: Vec<(Handle, cadkernel::brep::Body)> = handles
            .iter()
            .filter(|handle| !self.solid_models.contains_key(handle))
            .filter_map(|&handle| {
                let from_history = self
                    .document
                    .solid_history_operation(handle)
                    .and_then(|operation| cadkernel::acis::rebuild_body(operation).ok());
                let body = from_history.or_else(|| match self.document.get_entity(handle) {
                    Some(EntityType::Solid3D(solid)) => {
                        crate::scene::convert::solid3d_tess::kernel_body(solid)
                    }
                    Some(EntityType::Surface(surface)) => {
                        crate::scene::convert::solid3d_tess::kernel_surface_body(surface)
                    }
                    _ => None,
                })?;
                Some((handle, body))
            })
            .collect();
        self.solid_models.extend(bodies);
    }

    /// Re-tessellate only the named ACIS entities. The former edit path
    /// cleared and rebuilt every solid in the drawing when one selected solid
    /// moved or was copied.
    pub fn refresh_meshes_for_handles(&mut self, handles: &[Handle]) {
        if handles.is_empty() {
            return;
        }
        let mesh_entities: Vec<(Handle, std::sync::Arc<EntityType>)> = handles
            .iter()
            .filter_map(|&handle| {
                let entity = self.document.get_entity_arc(handle)?;
                matches!(
                    entity.as_ref(),
                    EntityType::Solid3D(_)
                        | EntityType::Region(_)
                        | EntityType::Body(_)
                        | EntityType::Surface(_)
                        | EntityType::Mesh(_)
                        | EntityType::PolygonMesh(_)
                        | EntityType::PolyfaceMesh(_)
                )
                .then_some((handle, entity))
            })
            .collect();
        for handle in handles {
            self.meshes.remove(handle);
            self.block_meshes.remove(handle);
            self.solid_models.remove(handle);
        }
        // The overwhelmingly common Undo/Redo target is 2-D geometry. Return
        // before scanning every Layout object just to discover there is no mesh
        // to rebuild.
        if mesh_entities.is_empty() {
            return;
        }
        let layout_blocks: std::collections::HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|o| match o {
                acadrust::objects::ObjectType::Layout(l) if !l.block_record.is_null() => {
                    Some(l.block_record)
                }
                _ => None,
            })
            .collect();
        let entries: Vec<(Handle, std::sync::Arc<EntityType>, [f32; 4], bool)> =
            mesh_entities
                .into_iter()
                .map(|(handle, entity)| {
                    let color = self.render_style(entity.as_ref()).0;
                    let top_level = layout_blocks.contains(&entity.common().owner_handle);
                    (handle, entity, color, top_level)
                })
                .collect();
        let facet_res = self.document.header.facet_resolution;
        let chordal_deflection =
            crate::entities::solid3d::display_deflection(&self.document.header, facet_res);
        let isolines = self.document.header.isolines.max(0) as usize;
        use crate::par::prelude::*;
        let built: Vec<(Handle, MeshLodSet, bool)> = entries
            .into_par_iter()
            .filter_map(|(handle, entity, color, top_level)| {
                crate::entities::solid3d::tessellate_volume(
                    entity.as_ref(),
                    color,
                    facet_res,
                    chordal_deflection,
                    isolines,
                )
                .map(|mut mesh| {
                    crate::scene::model::material_model::resolve_material_with_base(
                        &self.document,
                        entity.as_ref(),
                        color,
                        None,
                        self.material_base_dir.as_deref(),
                    )
                    .apply_to_with_face_overrides(
                        &mut mesh,
                        &self.document,
                        self.material_base_dir.as_deref(),
                    );
                    crate::scene::model::visual_style_model::apply_mesh_visual_style(
                        &mut mesh,
                        &self.document,
                        entity.as_ref(),
                    );
                    let mesh = if top_level {
                        offset_mesh_lod_set(mesh)
                    } else {
                        mesh
                    };
                    (handle, mesh, top_level)
                })
            })
            .collect();
        for (handle, mut mesh, top_level) in built {
            if top_level {
                self.meshes.insert(handle, mesh);
            } else {
                mesh.prepare_instance_source(handle);
                self.block_meshes.insert(handle, mesh);
            }
        }
    }

    /// Returns the RGBA color for the given layer name.
    pub fn layer_color(&self, layer: &str) -> [f32; 4] {
        let layer_entry = self.document.layers.get(layer);
        let color = layer_entry
            .map(|l| &l.color)
            .unwrap_or(&acadrust::types::Color::WHITE);
        let [r, g, b, _] = crate::scene::convert::tess_util::aci_to_rgba(color);
        let alpha = layer_entry
            .map(|layer| 1.0 - layer.transparency.as_percent() as f32)
            .unwrap_or(1.0);
        [r, g, b, alpha]
    }

    pub fn custom_block_names(&self) -> Vec<String> {
        self.document
            .block_records
            .iter()
            .filter(|br| !br.is_standard() && !br.is_layout())
            .map(|br| br.name.clone())
            .collect()
    }

    pub fn create_block_from_entities(
        &mut self,
        handles: &[Handle],
        name: &str,
        world_to_block: &acadrust::types::Transform,
        block_to_world: &acadrust::types::Transform,
    ) -> Result<Handle, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Block name cannot be empty.".into());
        }
        if name.starts_with('*') {
            return Err("Block name cannot start with '*'.".into());
        }
        if self.document.block_records.get(name).is_some() {
            return Err(format!("Block \"{name}\" already exists."));
        }

        let source_entities: Vec<_> = handles
            .iter()
            .filter_map(|&h| self.document.get_entity(h).cloned().map(|e| (h, e)))
            .collect();
        if source_entities.is_empty() {
            return Err("No valid entities selected for block creation.".into());
        }

        let next = self.document.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);

        let mut block_record = acadrust::tables::BlockRecord::new(name);
        block_record.handle = br_handle;
        block_record.block_entity_handle = block_handle;
        block_record.block_end_handle = end_handle;
        self.document
            .block_records
            .add(block_record)
            .map_err(|e| e.to_string())?;

        let mut block = Block::new(name, acadrust::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::Block(block))
            .map_err(|e| e.to_string())?;

        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::BlockEnd(block_end))
            .map_err(|e| e.to_string())?;

        let local = EntityTransform::Affine(*world_to_block);
        for (old_handle, mut entity) in source_entities {
            view::dispatch::apply_transform(&mut entity, &local);
            entity = crate::modules::draw::modify::explode::normalize_entity_for_block(entity);
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = br_handle;
            self.document
                .add_entity(entity)
                .map_err(|e| e.to_string())?;
            self.erase_entities(&[old_handle]);
        }

        let mut insert = DxfInsert::new(name, acadrust::types::Vector3::ZERO);
        acadrust::Entity::apply_transform(&mut insert, block_to_world);
        let insert_handle = self.add_entity(EntityType::Insert(insert));
        // A new block definition landed in the document; advance the block
        // epoch so consumers (the block palette stale check) notice it even
        // when the panel stays open. Mirrors `define_block_from_owned_entities`.
        self.bump_geometry();
        Ok(insert_handle)
    }

    /// Define a new block named `name` from `entities` (owned, not yet in the
    /// document), with `base` as its insertion origin. Unlike
    /// [`create_block_from_entities`] this does NOT place an insert — the
    /// caller starts an interactive insert so paste-as-block can prompt for the
    /// drop point. The geometry comes from the clipboard rather than live
    /// entities, so there is nothing to stage or erase. (#129)
    pub fn define_block_from_owned_entities(
        &mut self,
        entities: Vec<EntityType>,
        name: &str,
        base: glam::DVec3,
    ) -> Result<Vec<Handle>, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Block name cannot be empty.".into());
        }
        if name.starts_with('*') {
            return Err("Block name cannot start with '*'.".into());
        }
        if self.document.block_records.get(name).is_some() {
            return Err(format!("Block \"{name}\" already exists."));
        }
        if entities.is_empty() {
            return Err("Nothing to make into a block.".into());
        }

        let next = self.document.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);

        let mut block_record = acadrust::tables::BlockRecord::new(name);
        block_record.handle = br_handle;
        block_record.block_entity_handle = block_handle;
        block_record.block_end_handle = end_handle;
        self.document
            .block_records
            .add(block_record)
            .map_err(|e| e.to_string())?;

        let mut block = Block::new(name, acadrust::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::Block(block))
            .map_err(|e| e.to_string())?;

        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::BlockEnd(block_end))
            .map_err(|e| e.to_string())?;

        let local = EntityTransform::Translate(-base);
        let mut entity_handles = Vec::with_capacity(entities.len());
        for mut entity in entities {
            view::dispatch::apply_transform(&mut entity, &local);
            entity = crate::modules::draw::modify::explode::normalize_entity_for_block(entity);
            Self::reset_clone_subhandles(&mut self.document, &mut entity);
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = br_handle;
            let handle = self
                .document
                .add_entity(entity)
                .map_err(|e| e.to_string())?;
            entity_handles.push(handle);
        }
        // Block defns don't render on their own, but the geometry cache must
        // pick up the new definition so the interactive insert can preview it.
        self.bump_geometry();
        Ok(entity_handles)
    }

    /// Recreate a block definition verbatim — the entities are already in
    /// block-local coordinates (unlike `define_block_from_owned_entities`,
    /// which folds in a base offset). No-op if the block already exists.
    /// Used when pasting an INSERT whose block this drawing lacks. (#135)
    pub fn define_block_raw(
        &mut self,
        name: &str,
        base_point: acadrust::types::Vector3,
        entities: Vec<EntityType>,
    ) {
        if name.is_empty() || self.document.block_records.get(name).is_some() {
            return;
        }
        let next = self.document.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);

        let mut block_record = acadrust::tables::BlockRecord::new(name);
        block_record.handle = br_handle;
        block_record.block_entity_handle = block_handle;
        block_record.block_end_handle = end_handle;
        if self.document.block_records.add(block_record).is_err() {
            return;
        }

        let mut block = Block::new(name, base_point);
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        let _ = self.document.add_entity(EntityType::Block(block));

        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        let _ = self.document.add_entity(EntityType::BlockEnd(block_end));

        for mut entity in entities {
            Self::reset_clone_subhandles(&mut self.document, &mut entity);
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = br_handle;
            let _ = self.document.add_entity(entity);
        }
        self.bump_geometry();
    }

    pub(super) fn synced_hatch_models(
        &self,
        target_block: Handle,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
        tint_selected: bool,
    ) -> Vec<HatchModel> {
        let layer_hidden = |layer: &str| {
            self.document
                .layers
                .get(layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
        };

        // synced_hatch_models is cached on geometry_epoch and the GPU
        // upload is keyed on geometry_epoch only (see render.rs — hatch
        // buffers are "static"). Don't view-cull here; the per-frame
        // skip flag in compute_hatch_lod handles frustum + sub-pixel
        // culling at draw time, which keeps the GPU upload set stable
        // across pan/zoom.
        //
        // Every content viewport supplies the block it renders. Do not depend
        // on camera/frustum culling to separate paper and model coordinates:
        // overlapping coordinates otherwise make foreign fills visible.
        let hatch_bg = if self.current_layout != "Model" {
            self.paper_bg_color
        } else {
            self.bg_color
        };
        let depth_map = self.draw_depth_map();
        let mut models: Vec<HatchModel> = self
            .hatches
            .iter()
            .filter(|(&handle, _)| {
                let Some(entity) = self.document.get_entity(handle) else {
                    return true;
                };
                // SOLID already renders through its WCS-aware fill triangles.
                // Its cached HatchModel is an XY-only plot fallback; sending
                // that to the screen too adds a flattened, grip-less copy at
                // z=0 for elevated or angled geometry (#617).
                if matches!(entity, EntityType::Solid(_)) {
                    return false;
                }
                let c = entity.common();
                if c.invisible
                    || self.entity_temporarily_hidden(handle)
                    || layer_hidden(&c.layer)
                    || crate::scene::annotative::annotative_offscale_for(
                        &self.document,
                        c,
                        annotation_scale_handle,
                        all_visible,
                    )
                {
                    return false;
                }
                // Per-viewport layer freeze: a content viewport that freezes
                // this layer hides its fills too, not just its wires.
                if self.layer_frozen_in(&c.layer, frozen) {
                    return false;
                }
                // Reject block-defn-only hatches (entities owned by a
                // BLOCK record that's neither model nor a paper layout
                // block) — the scene graph emits only their laid-out copies.
                self.belongs_to_visible_block(handle, c.owner_handle, target_block)
            })
            .flat_map(|(&handle, model)| {
                let contextual = self
                    .document
                    .get_entity(handle)
                    .map(|entity| {
                        crate::scene::annotative::entity_for_annotation_context(
                            &self.document,
                            entity,
                            annotation_scale_handle,
                        )
                    });
                let entity = contextual.as_deref();
                let mut m = match entity {
                    Some(EntityType::Hatch(dxf))
                        if crate::scene::annotative::active_object_context_for_scale(
                            &self.document,
                            handle,
                            annotation_scale_handle,
                        )
                        .is_some() =>
                    {
                        Self::hatch_model_from_dxf(dxf, model.color)
                            .unwrap_or_else(|| model.clone())
                    }
                    _ => model.clone(),
                };
                // Optional solid backdrop drawn behind the pattern/gradient when
                // the hatch carries a HATCHBACKGROUNDCOLOR. Same draw_depth +
                // emitted first so LessEqual layering keeps it underneath.
                let mut backdrop: Option<HatchModel> = None;
                if let Some(e) = entity {
                    let mut style = crate::scene::view::render::render_style_for_viewport(
                        &self.document,
                        e,
                        viewport,
                    );
                    style.0 = crate::scene::view::render::adapt_to_bg(style.0, hatch_bg);
                    m.aci = style.4;
                    m.line_weight_px = style.3;
                    // A gradient's colour is its first stop (already baked into
                    // the cached model); only solid / pattern fills take the
                    // entity's resolved colour.
                    if !matches!(m.pattern, model::hatch_model::HatchPattern::Gradient { .. }) {
                        m.color = style.0;
                    }
                    if let EntityType::Hatch(dxf) = e {
                        if let Some(bg) = crate::entities::hatch::background_color(dxf) {
                            let mut b = m.clone();
                            b.pattern = model::hatch_model::HatchPattern::Solid;
                            // ByLayer / ByBlock backgrounds resolve through the
                            // normal style chain instead of the raw ACI table
                            // (#415).
                            let (bg_color, bg_aci) = match bg {
                                acadrust::types::Color::ByLayer => {
                                    let layer = self.document.layers.get(&dxf.common.layer);
                                    let aci = layer
                                        .and_then(|layer| match &layer.color {
                                            acadrust::types::Color::Index(index) => Some(*index),
                                            _ => None,
                                        })
                                        .unwrap_or(0);
                                    (
                                        crate::scene::view::render::layer_render_style_viewport(
                                            &self.document,
                                            &dxf.common.layer,
                                            viewport,
                                        )
                                        .color,
                                        aci,
                                    )
                                }
                                acadrust::types::Color::ByBlock => (style.0, style.4),
                                acadrust::types::Color::Index(index) => (
                                    crate::scene::convert::tess_util::aci_to_rgba(
                                        &acadrust::types::Color::Index(index),
                                    ),
                                    index,
                                ),
                                other => (
                                    crate::scene::convert::tess_util::aci_to_rgba(&other),
                                    0,
                                ),
                            };
                            b.color = bg_color;
                            b.aci = bg_aci;
                            b.name = "SOLID".into();
                            backdrop = Some(b);
                        }
                        match &mut m.pattern {
                            // Pattern built from the hatch's own stored lines is
                            // already final (scale 1 / angle 0) — don't re-apply
                            // pattern_scale/angle. Only the catalog-derived path
                            // (empty stored lines) needs the override.
                            model::hatch_model::HatchPattern::Pattern(_)
                                if dxf.pattern.lines.is_empty() =>
                            {
                                m.angle_offset = dxf.pattern_angle as f32;
                                m.scale = dxf.pattern_scale as f32;
                            }
                            model::hatch_model::HatchPattern::Gradient {
                                angle_deg,
                                shift,
                                ..
                            } => {
                                *angle_deg = dxf.gradient_color.angle.to_degrees() as f32;
                                *shift = dxf.gradient_color.shift as f32;
                            }
                            model::hatch_model::HatchPattern::Pattern(_)
                            | model::hatch_model::HatchPattern::Solid => {}
                        }
                    }
                }
                if tint_selected && self.selected.contains(&handle) {
                    m.color = [0.15, 0.55, 1.00, m.color[3]];
                }
                let d = depth_map.get(&handle.value()).map_or(0.0, |d| d[0]);
                m.draw_depth = d;
                if let Some(b) = &mut backdrop {
                    b.draw_depth = d;
                }
                backdrop.into_iter().chain(std::iter::once(m))
            })
            .collect();

        // Background for adapting block-child hatch colours at the leaf (#221).
        // Instanced/owned hatch leaves are produced by the shared scene graph.
        models.extend(self.instanced_hatch_models(
            target_block,
            hatch_bg,
            tint_selected,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
        ));

        // Wide polyline bands remain on the wire path, including inside block
        // instances; the graph does not reclassify them as hatch fills.

        models
    }

    /// Materialize hatch leaves reached through visible scene containers in
    /// `layout_block`, with transforms and inherited styles already resolved.
    ///
    /// Shared by the on-screen hatch set (`synced_hatch_models`) and the
    /// paper/export hatch set (`paper_canvas_hatches`) so a plot draws
    /// block-internal hatches identically to the viewport.
    ///
    /// `hatch_bg` adapts pure black/white leaf colours to the target
    /// background; `tint_selected` re-colours fills of a selected INSERT
    /// (screen highlight) and should be `false` for export.
    pub(super) fn instanced_hatch_models(
        &self,
        layout_block: Handle,
        hatch_bg: [f32; 4],
        tint_selected: bool,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Vec<HatchModel> {
        self.instanced_hatch_models_filtered(
            layout_block,
            hatch_bg,
            tint_selected,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
            None,
            false,
            false,
        )
    }

    fn layer_plottable_in_context(
        &self,
        entity: &EntityType,
        context: &crate::scene::render_graph::InstanceContext,
    ) -> bool {
        let common = entity.common();
        let layer = if crate::scene::view::render::is_effective_layer_zero(&common.layer) {
            context
                .insert_path
                .iter()
                .rev()
                .find(|insert| {
                    !crate::scene::view::render::is_effective_layer_zero(&insert.common.layer)
                })
                .map(|insert| insert.common.layer.as_str())
                .unwrap_or(common.layer.as_str())
        } else {
            common.layer.as_str()
        };
        self.document
            .layers
            .get(layer)
            .map(|layer| layer.is_plottable)
            .unwrap_or(true)
    }

    pub(super) fn instanced_plot_hatch_models(
        &self,
        layout_block: Handle,
        hatch_bg: [f32; 4],
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Vec<HatchModel> {
        self.instanced_hatch_models_filtered(
            layout_block,
            hatch_bg,
            false,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
            None,
            false,
            true,
        )
    }
    /// Build live hatch overlays for INSERT grip previews. The edited INSERT
    /// is intentionally hidden from the resident scene while its current
    /// document entity moves, so include that hidden target and reuse the full
    /// block-expansion path for nested inserts, inherited styles and XCLIP.
    pub(super) fn preview_insert_hatch_models(&self, handles: &[Handle]) -> Vec<HatchModel> {
        let targets: rustc_hash::FxHashSet<Handle> = handles
            .iter()
            .copied()
            .filter(|&handle| {
                matches!(self.document.get_entity(handle), Some(EntityType::Insert(_)))
            })
            .collect();
        if targets.is_empty() {
            return Vec::new();
        }
        let hatch_bg = if self.current_layout != "Model" {
            self.paper_bg_color
        } else {
            self.bg_color
        };
        let frozen: rustc_hash::FxHashSet<Handle> = self
            .interaction_viewport_frozen_layers()
            .into_iter()
            .flatten()
            .copied()
            .collect();
        self.instanced_hatch_models_filtered(
            self.interaction_block_handle(),
            hatch_bg,
            true,
            (!frozen.is_empty()).then_some(&frozen),
            self.displayed_annotation_scale_handle(),
            self.annotation_all_visible(),
            self.active_viewport,
            Some(&targets),
            true,
            false,
        )
    }

    fn instanced_hatch_models_filtered(
        &self,
        layout_block: Handle,
        hatch_bg: [f32; 4],
        tint_selected: bool,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
        targets: Option<&rustc_hash::FxHashSet<Handle>>,
        include_preview_hidden: bool,
        plot_only: bool,
    ) -> Vec<HatchModel> {
        let depth_map = self.draw_depth_map();
        let graph = crate::scene::render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_annotation_scale(self.annotation_scale)
        .with_viewport(viewport);
        let mut hatch_block_memo = std::collections::HashMap::new();
        let mut models = Vec::new();
        let mut hatch_sources = rustc_hash::FxHashMap::default();
        graph.walk_root(
            self.render_scene_root(layout_block),
            |entity, context| {
                let common = entity.common();

                if plot_only && !self.layer_plottable_in_context(entity, context) {
                    return false;
                }

                if context.is_instanced() {
                    return true;
                }
                if self.object_isolation.hides(common.handle)
                    || (!include_preview_hidden
                        && self.preview_hidden.contains(&common.handle))
                {
                    return false;
                }
                if let Some(targets) = targets {
                    return matches!(entity, EntityType::Insert(_))
                        && targets.contains(&common.handle);
                }
                let block_uses: Vec<_> = crate::scene::render_graph::entity_render_block_uses(
                    &self.document,
                    entity,
                    1.0,
                )
                .into_iter()
                .filter(|block_use| block_use.active)
                .collect();
                if block_uses.is_empty() {
                    true
                } else {
                    block_uses.into_iter().any(|block_use| {
                        crate::scene::render_graph::block_contains_hatch(
                            &self.document,
                            &block_use.insert.block_name,
                            &mut hatch_block_memo,
                        )
                    })
                }
            },
            |entity, context| {
                if !context.is_instanced() {
                    return;
                }
                let EntityType::Hatch(source_hatch) = entity else {
                    return;
                };
                if plot_only && !self.layer_plottable_in_context(entity, context) {
                    return;
                }
                let style = context.style_for(&self.document, entity);
                let preserve_white_mask = source_hatch.is_solid
                    && matches!(
                        source_hatch.common.color,
                        acadrust::types::Color::Index(7)
                    );
                let color = if preserve_white_mask {
                    style.0
                } else {
                    crate::scene::view::render::adapt_to_bg(style.0, hatch_bg)
                };

                let mut placed = EntityType::Hatch(source_hatch.clone());
                placed.apply_transform(&context.transform);
                let EntityType::Hatch(hatch) = placed else {
                    return;
                };
                let Some(mut model) = Self::hatch_model_from_dxf(&hatch, color) else {
                    return;
                };
                model.aci = style.4;
                model.line_weight_px = style.3;
                model.draw_depth =
                    context.draw_depth(source_hatch.common.handle, depth_map.as_ref());
                for clip in &context.clips {
                    let clip: Vec<[f32; 2]> = clip
                        .iter()
                        .map(|point| [point[0] as f32, point[1] as f32])
                        .collect();
                    let clipped = pick::xclip::clip_hatch_boundary(
                        &model.boundary,
                        model.world_origin,
                        &clip,
                    );
                    if clipped.is_empty() {
                        return;
                    }
                    model.boundary = std::sync::Arc::new(clipped);
                }
                if tint_selected && self.selected.contains(&context.root_handle) {
                    model.color = [0.15, 0.55, 1.00, model.color[3]];
                }
                let matrix = &context.transform.matrix.m;
                let linear = [
                    matrix[0][0].to_bits(), matrix[0][1].to_bits(), matrix[0][2].to_bits(),
                    matrix[1][0].to_bits(), matrix[1][1].to_bits(), matrix[1][2].to_bits(),
                    matrix[2][0].to_bits(), matrix[2][1].to_bits(), matrix[2][2].to_bits(),
                ];
                let key = (
                    source_hatch.common.handle.value(),
                    linear,
                    model
                        .boundary
                        .iter()
                        .flat_map(|point| point.map(f32::to_bits))
                        .collect::<Vec<_>>(),
                    model.color.map(f32::to_bits),
                    model.angle_offset.to_bits(),
                    model.scale.to_bits(),
                    model.line_weight_px.to_bits(),
                    model.aci,
                );
                let source_id = *hatch_sources
                    .entry(key)
                    .or_insert_with(crate::scene::model::instance_model::next_source_id);
                model.render_instance = Some(
                    crate::scene::model::instance_model::RenderInstance {
                        source_id,
                        translation: [matrix[0][3], matrix[1][3], matrix[2][3]],
                    },
                );
                models.push(model);
            },
        );
        models
    }

    /// Wipeout fill models — rendered in a separate pass AFTER wires so that
    /// wipeouts correctly mask everything below them in the draw order.
    pub(crate) fn wipeout_models(
        &self,
        target_block: Handle,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
    ) -> Vec<HatchModel> {
        let bg_color = if self.current_layout != "Model" {
            self.paper_bg_color
        } else {
            self.bg_color
        };
        self.wipeout_models_for_block_graph(
            target_block,
            frozen,
            annotation_scale_handle,
            all_visible,
            bg_color,
            false,
            false,
        )
    }

    pub(super) fn wipeout_models_for_block_graph(
        &self,
        target_block: Handle,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        bg_color: [f32; 4],
        tint_insert_selection: bool,
        plot_only: bool,
    ) -> Vec<HatchModel> {
        let depth_map = self.draw_depth_map();
        let graph = crate::scene::render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_annotation_scale(self.annotation_scale);
        let mut models = Vec::new();
        let mut wipeout_sources = rustc_hash::FxHashMap::default();
        graph.walk_root(
            self.render_scene_root(target_block),
            |entity, context| {
                let common = entity.common();
                if plot_only && !self.layer_plottable_in_context(entity, context) {
                    return false;
                }
                context.is_instanced() || !self.entity_temporarily_hidden(common.handle)
            },
            |entity, context| {
                let EntityType::Wipeout(source) = entity else {
                    return;
                };
                let mut wipeout = source.clone();
                if context.is_instanced() {
                    wipeout.insertion_point =
                        context.transform.apply(source.insertion_point);
                    wipeout.u_vector =
                        context.transform.apply_rotation(source.u_vector);
                    wipeout.v_vector =
                        context.transform.apply_rotation(source.v_vector);
                }
                let Some(mut fill_plane) = Self::wipeout_fill_plane(&wipeout) else {
                    return;
                };
                let (world_origin, mut boundary) =
                    Self::wipeout_boundary_2d(&wipeout);
                for clip in &context.clips {
                    let clip: Vec<[f32; 2]> = clip
                        .iter()
                        .map(|point| [point[0] as f32, point[1] as f32])
                        .collect();
                    boundary = pick::xclip::clip_hatch_boundary(
                        &boundary,
                        world_origin,
                        &clip,
                    );
                }
                if boundary.len() < 3 {
                    return;
                }
                if !context.clips.is_empty() {
                    let Some(local) = Self::wipeout_boundary_at_xy(
                        fill_plane.0,
                        world_origin,
                        &boundary,
                    ) else {
                        return;
                    };
                    fill_plane.1 = local;
                }
                let selection_handle =
                    if tint_insert_selection && context.is_instanced() {
                        context.root_handle
                    } else {
                        source.common.handle
                    };
                let color = if self.selected.contains(&selection_handle) {
                    [0.15, 0.55, 1.00, 0.35]
                } else {
                    bg_color
                };
                let render_instance = if context.is_instanced() {
                    let matrix = &context.transform.matrix.m;
                    let linear = [
                        matrix[0][0].to_bits(), matrix[0][1].to_bits(), matrix[0][2].to_bits(),
                        matrix[1][0].to_bits(), matrix[1][1].to_bits(), matrix[1][2].to_bits(),
                        matrix[2][0].to_bits(), matrix[2][1].to_bits(), matrix[2][2].to_bits(),
                    ];
                    let key = (
                        source.common.handle.value(),
                        linear,
                        boundary
                            .iter()
                            .flat_map(|point| point.map(f32::to_bits))
                            .collect::<Vec<_>>(),
                        color.map(f32::to_bits),
                    );
                    let source_id = *wipeout_sources.entry(key).or_insert_with(
                        crate::scene::model::instance_model::next_source_id,
                    );
                    Some(crate::scene::model::instance_model::RenderInstance {
                        source_id,
                        translation: [matrix[0][3], matrix[1][3], matrix[2][3]],
                    })
                } else {
                    None
                };
                let fill_plane_boundary = Some(Arc::new(fill_plane.1));
                let fill_plane = Some(fill_plane.0);
                models.push(HatchModel {
                    render_instance,
                    boundary: Arc::new(boundary),
                    boundary_wcs: None,
                    fill_plane,
                    fill_plane_boundary,
                    boundary_exterior: None,
                    boundary_sources: None,
                    boundary_paths: None,
                    style: acadrust::entities::HatchStyleType::Normal,
                    pattern: model::hatch_model::HatchPattern::Solid,
                    name: "WIPEOUT_FILL".into(),
                    color,
                    aci: 0,
                    line_weight_px: 1.0,
                    angle_offset: 0.0,
                    scale: 1.0,
                    world_origin,
                    draw_depth: context
                        .draw_depth(source.common.handle, depth_map.as_ref()),
                });
            },
        );
        models
    }

    /// Compute the 2D (XY) boundary polygon for a Wipeout entity.
    /// Wipeout fill boundary as small f32 offsets from the returned world_origin
    /// (the insertion point, kept in f64).
    pub(super) fn wipeout_boundary_2d(
        wo: &acadrust::entities::Wipeout,
    ) -> ([f64; 2], Vec<[f32; 2]>) {
        let origin = [wo.insertion_point.x, wo.insertion_point.y];
        let plane = cadkernel::space::Plane::from_axes(
            [wo.insertion_point.x, wo.insertion_point.y, wo.insertion_point.z],
            [wo.u_vector.x, wo.u_vector.y, wo.u_vector.z],
            [wo.v_vector.x, wo.v_vector.y, wo.v_vector.z],
        );
        let boundary = Self::wipeout_local_boundary(wo)
            .into_iter()
            .map(|point| {
                if point[0].is_finite() && point[1].is_finite() {
                    let offset = plane.vector_at(point);
                    [offset[0] as f32, offset[1] as f32]
                } else {
                    [f32::NAN; 2]
                }
            })
            .collect();
        (origin, boundary)
    }

    fn wipeout_local_boundary(wo: &acadrust::entities::Wipeout) -> Vec<[f64; 2]> {
        use acadrust::entities::{WipeoutClipMode, WipeoutClipType};
        let mut outer = vec![
            [0.0, 0.0],
            [wo.size.x, 0.0],
            [wo.size.x, wo.size.y],
            [0.0, wo.size.y],
            [0.0, 0.0],
        ];
        if !wo.clipping_enabled
            || wo.clip_boundary_vertices.len() < 3
            || !matches!(wo.clip_type, WipeoutClipType::Polygonal)
        {
            return outer;
        }
        let mut clip: Vec<[f64; 2]> = wo
            .clip_boundary_vertices
            .iter()
            .map(|point| {
                [
                    point.x + wo.size.x * 0.5,
                    wo.size.y * 0.5 - point.y,
                ]
            })
            .collect();
        if clip.last() != clip.first() {
            clip.push(clip[0]);
        }
        if matches!(wo.clip_mode, WipeoutClipMode::Inside) {
            outer.push([f64::NAN; 2]);
            outer.extend(clip);
            outer
        } else {
            clip
        }
    }

    fn wipeout_fill_plane(
        wipeout: &acadrust::entities::Wipeout,
    ) -> Option<(model::hatch_model::FillPlane, Vec<[f32; 2]>)> {
        let origin = [
            wipeout.insertion_point.x,
            wipeout.insertion_point.y,
            wipeout.insertion_point.z,
        ];
        let x_axis = [wipeout.u_vector.x, wipeout.u_vector.y, wipeout.u_vector.z];
        let y_axis = [wipeout.v_vector.x, wipeout.v_vector.y, wipeout.v_vector.z];
        cadkernel::space::Plane::from_axes(origin, x_axis, y_axis).normal()?;
        let boundary = Self::wipeout_local_boundary(wipeout)
            .into_iter()
            .map(|point| [point[0] as f32, point[1] as f32])
            .collect();
        Some((
            model::hatch_model::FillPlane {
                origin,
                x_axis,
                y_axis,
            },
            boundary,
        ))
    }

    fn wipeout_boundary_at_xy(
        plane: model::hatch_model::FillPlane,
        world_origin: [f64; 2],
        boundary: &[[f32; 2]],
    ) -> Option<Vec<[f32; 2]>> {
        let plane = cadkernel::space::Plane::from_axes(
            plane.origin,
            plane.x_axis,
            plane.y_axis,
        );
        boundary
            .iter()
            .map(|point| {
                if point[0].is_finite() && point[1].is_finite() {
                    plane
                        .coordinates_at_xy([
                            world_origin[0] + point[0] as f64,
                            world_origin[1] + point[1] as f64,
                        ])
                        .map(|point| [point[0] as f32, point[1] as f32])
                } else {
                    Some([f32::NAN; 2])
                }
            })
            .collect()
    }

    pub(crate) fn hatch_model_from_dxf(
        dxf: &DxfHatch,
        color: [f32; 4],
    ) -> Option<HatchModel> {
        let normal = (dxf.normal.x, dxf.normal.y, dxf.normal.z);
        // Build the boundary in f64 first so the precision-preserving
        // origin computation below sees full WCS precision. We only cast
        // to f32 once at the end, after subtracting the AABB centre, so
        // the stored offsets are small-magnitude with high f32 precision
        // even on large UTM-scale drawings.
        let to_xy = |x: f64, y: f64| -> [f64; 2] {
            let (wx, wy, _) =
                crate::scene::view::transform::ocs_point_to_wcs((x, y, dxf.elevation), normal);
            [wx, wy]
        };
        if dxf.paths.is_empty() {
            return None;
        }

        let mut rings = Vec::new();
        let mut local_rings = Vec::new();
        let mut ring_sources = Vec::new();

        for path in &dxf.paths {
            // Skip TEXTBOX boundary paths (flag bit 3). These are text
            // derived bounding boxes used for island detection; they are
            // never drawn or filled. Treating one as a fill boundary paints its
            // rectangle solid and creates a phantom bar.
            if path.flags.bits() & 8 != 0 {
                continue;
            }
            let mut edge_polys: Vec<Vec<[f64; 2]>> = Vec::new();
            for edge in &path.edges {
                if let Some(curve) = crate::entities::hatch::edge_curve(edge) {
                    edge_polys.push(
                        curve.tessellate_angle(cadkernel::tessellation::DEFAULT_ANGLE),
                    );
                }
            }
            let mut local_ring = chain_path_edges(edge_polys);
            if local_ring.is_empty() {
                continue;
            }
            if local_ring.len() >= 3 {
                let first = local_ring[0];
                let last = *local_ring.last().unwrap();
                if (first[0] - last[0]).abs() > 1e-5 || (first[1] - last[1]).abs() > 1e-5 {
                    local_ring.push(first);
                }
            }
            let ring = local_ring
                .iter()
                .map(|point| to_xy(point[0], point[1]))
                .collect();
            rings.push(ring);
            local_rings.push(local_ring);
            ring_sources.push(path.boundary_handles.clone());
        }

        if rings.is_empty() {
            return None;
        }

        let depths = cadkernel::geom2d::ring_nesting_depths(&rings);
        let mut boundary = Vec::new();
        let mut local_boundary = Vec::new();
        let mut boundary_exterior = Vec::new();
        let mut boundary_sources = Vec::new();
        for (((ring, local_ring), sources), depth) in rings
            .into_iter()
            .zip(local_rings)
            .zip(ring_sources)
            .zip(depths)
        {
            let keep = match dxf.style {
                acadrust::entities::HatchStyleType::Normal => true,
                acadrust::entities::HatchStyleType::Outer => depth <= 1,
                acadrust::entities::HatchStyleType::Ignore => depth == 0,
            };
            if !keep {
                continue;
            }
            if !boundary.is_empty() {
                boundary.push([f64::NAN, f64::NAN]);
                local_boundary.push([f32::NAN, f32::NAN]);
            }
            boundary.extend(ring);
            local_boundary.extend(
                local_ring
                    .into_iter()
                    .map(|[x, y]| [x as f32, y as f32]),
            );
            boundary_exterior.push(depth == 0);
            boundary_sources.push(sources);
        }
        // The batched hatch renderer keeps boundaries in a GPU storage
        // buffer (no fixed length), so a hatch with many island loops must
        // retain *every* loop or even-odd island detection breaks. The old
        // flat `truncate(1024)` cut complex multi-loop hatches mid-boundary:
        // trailing islands were dropped and the final partial loop was left
        // open, flipping the even-odd parity so the fill bled across the
        // rest of the shape. Only guard against pathological vertex counts,
        // and when trimming, cut at a whole-loop (NaN sentinel) boundary so
        // no sub-loop is ever left open. (#148)
        const MAX_HATCH_MODEL_VERTS: usize = 16_384;
        if boundary.len() > MAX_HATCH_MODEL_VERTS {
            // Drop only whole trailing loops: cut at the last NaN sentinel
            // at/before the cap. If the first loop alone exceeds the cap,
            // keep it whole rather than leaving it open.
            let cut = boundary[..MAX_HATCH_MODEL_VERTS]
                .iter()
                .rposition(|&[x, y]| x.is_nan() || y.is_nan())
                .unwrap_or(boundary.len());
            boundary.truncate(cut);
        }

        // When the HATCH carries its own resolved pattern-line geometry
        // (angle + world-unit offset, exactly as the DWG stores it), use THAT
        // instead of re-deriving spacing from the name-matched catalog entry
        // × pattern_scale. The catalog's base spacing (e.g. metric acadiso
        // ANSI31 = 3.175) rarely matches what the drawing was authored against
        // (imperial 0.125), so the catalog path rendered lines up to ~25×
        // (= inch→mm) too coarse — a dense fill collapsed to a few stray
        // lines. The stored offset is the authoritative world-unit spacing, so
        // the resulting families are already final: no pattern_scale / angle
        // is re-applied (see `prebaked` below and the HatchModel fields).
        let prebaked = !dxf.is_solid
            && !dxf.gradient_color.is_enabled()
            && !dxf.pattern.lines.is_empty();

        // The gradient's first stop is the fill's start colour (not the
        // entity colour); capture it so the HatchModel draws stop-0 → stop-1.
        let mut gradient_color1: Option<[f32; 4]> = None;
        let mut pattern = if dxf.gradient_color.is_enabled() {
            let stop = |i: usize| {
                dxf.gradient_color.colors.get(i).and_then(|e| e.color.rgb()).map(
                    |(r, g, b)| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0],
                )
            };
            gradient_color1 = stop(0);
            let color1 = gradient_color1.unwrap_or(color);
            let color2 = if dxf.gradient_color.is_single_color {
                gradient_tint_color(color1, dxf.gradient_color.color_tint as f32)
            } else {
                stop(1).unwrap_or(color)
            };
            let angle_deg = dxf.gradient_color.angle.to_degrees() as f32;
            let (kind, invert) =
                model::hatch_model::GradientKind::from_name(&dxf.gradient_color.name);
            model::hatch_model::HatchPattern::Gradient {
                angle_deg,
                color2,
                kind,
                invert,
                shift: dxf.gradient_color.shift as f32,
            }
        } else if dxf.is_solid {
            model::hatch_model::HatchPattern::Solid
        } else if prebaked {
            model::hatch_model::HatchPattern::Pattern(
                dxf.pattern.lines.iter().map(family_from_stored_line).collect(),
            )
        } else {
            let pat_name = &dxf.pattern.name;
            if let Some(entry) = crate::scene::model::hatch_patterns::find(pat_name) {
                entry.gpu.clone()
            } else if matches!(
                dxf.pattern_type,
                acadrust::entities::hatch::HatchPatternType::UserDefined
            ) {
                // User-defined hatch: parallel lines at `pattern_angle`, spaced
                // `pattern_scale` apart, plus a perpendicular set when
                // `is_double`. Its name ("_USER") is not a catalog pattern.
                // Build BASE families (angle 0, and 90 for the cross set) with
                // unit perpendicular spacing; the HatchModel's angle_offset
                // (= pattern_angle) and scale (= pattern_scale) below rotate and
                // space them — exactly as a predefined .PAT pattern is applied —
                // so the angle/scale is applied once, not doubled. Replaces the
                // old fallback that forced every user-defined hatch to flat
                // horizontal lines at the wrong spacing (#278).
                let fam = |angle_deg: f32| model::hatch_model::PatFamily {
                    angle_deg,
                    x0: 0.0,
                    y0: 0.0,
                    dx: 0.0,
                    dy: 1.0,
                    dashes: vec![],
                };
                let mut fams = vec![fam(0.0)];
                if dxf.is_double {
                    fams.push(fam(90.0));
                }
                model::hatch_model::HatchPattern::Pattern(fams)
            } else {
                model::hatch_model::HatchPattern::Pattern(vec![model::hatch_model::PatFamily {
                    angle_deg: 0.0,
                    x0: 0.0,
                    y0: 0.0,
                    dx: 0.0,
                    dy: 5.0 * dxf.pattern_scale as f32,
                    dashes: vec![],
                }])
            }
        };

        let name = if dxf.gradient_color.is_enabled() {
            dxf.gradient_color.name.clone()
        } else if dxf.is_solid {
            "SOLID".into()
        } else {
            dxf.pattern.name.clone()
        };

        // Precision-preserving cast f64 → f32: pick an `world_origin`
        // anchor (boundary AABB centre in f64) and store every vertex
        // as a small f32 offset from it. NaN separators are preserved
        // so the in_polygon ray-cast still sees the path breaks.
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for &[x, y] in &boundary {
            if x.is_finite() && y.is_finite() {
                if x < min_x { min_x = x; }
                if y < min_y { min_y = y; }
                if x > max_x { max_x = x; }
                if y > max_y { max_y = y; }
            }
        }
        let world_origin = if min_x.is_finite() && min_y.is_finite() {
            [(min_x + max_x) * 0.5, (min_y + max_y) * 0.5]
        } else {
            [0.0, 0.0]
        };

        // Anchor the prebaked families near the geometry, tracking the pattern
        // origin. The DWG base points sit at the pattern's *authored* origin,
        // which on a UTM drawing can be ~1e6 units from the hatch. Handing the
        // shader `base - world_origin` (~1e6) is wrong two ways: it consumes
        // `x0/y0` as f32 (so ~0.5 m quantization shreds the phase — boundaries
        // are unaffected, they ride the double-single relative-to-eye path), AND
        // it evaluates the pattern that far from its origin. Multi-family
        // aggregate patterns (AR-CONC, AR-SAND, GRAVEL, …) are effectively
        // quasi-periodic: their families only cohere into the intended
        // stones/grains near their shared origin and dissolve into scattered
        // dashes at large offsets.
        //
        // So fold the origin's offset from `world_origin` down to a small,
        // coherence-safe remainder on a grid of `spacing * 64` (many multiples
        // of the pattern spacing — well inside the coherence range yet far
        // larger than any realistic grip drag). Applying ONE common fold (from
        // the reference line) preserves each family's relative phase, which is
        // what forms the stones. Because the origin grip / Origin X/Y edit
        // shifts every base point by the same delta, the sub-grid remainder
        // tracks that delta 1:1, so the grip still moves the fill; a grid
        // crossing (a whole `spacing * 64` drag) is never hit in practice.
        if prebaked {
            if let model::hatch_model::HatchPattern::Pattern(fams) = &mut pattern {
                if let Some(rb) = dxf.pattern.lines.first().map(|l| l.base_point) {
                    let spacing = dxf
                        .pattern
                        .lines
                        .iter()
                        .map(|l| l.offset.length())
                        .fold(0.0_f64, f64::max)
                        .max(1e-6);
                    let cell = spacing * 64.0;
                    let fold = |v: f64, o: f64| -> f64 {
                        let d = v - o;
                        d - (d / cell).round() * cell
                    };
                    let ox = fold(rb.x, world_origin[0]);
                    let oy = fold(rb.y, world_origin[1]);
                    for (fam, ln) in fams.iter_mut().zip(dxf.pattern.lines.iter()) {
                        fam.x0 = (ln.base_point.x - rb.x + ox) as f32;
                        fam.y0 = (ln.base_point.y - rb.y + oy) as f32;
                    }
                }
            }
        }
        let boundary_f32: Vec<[f32; 2]> = boundary
            .iter()
            .map(|&[x, y]| {
                if x.is_finite() && y.is_finite() {
                    [(x - world_origin[0]) as f32, (y - world_origin[1]) as f32]
                } else {
                    [f32::NAN, f32::NAN]
                }
            })
            .collect();

        let storage = crate::entities::curve::ocs_plane(dxf.normal, dxf.elevation);
        Some(HatchModel {
            render_instance: None,
            boundary: std::sync::Arc::new(boundary_f32),
            boundary_wcs: None,
            fill_plane: Some(model::hatch_model::FillPlane {
                origin: storage.origin,
                x_axis: storage.x_axis,
                y_axis: storage.y_axis,
            }),
            fill_plane_boundary: Some(std::sync::Arc::new(local_boundary)),
            boundary_exterior: Some(std::sync::Arc::new(boundary_exterior)),
            boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
            boundary_paths: Some(std::sync::Arc::new(dxf.paths.clone())),
            style: dxf.style,
            pattern,
            name,
            // A gradient starts from its first stop; other fills use the
            // entity colour.
            color: gradient_color1.unwrap_or(color),
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: if prebaked { 0.0 } else { dxf.pattern_angle as f32 },
            scale: if prebaked { 1.0 } else { dxf.pattern_scale as f32 },
            world_origin,
            draw_depth: 0.0,
        })
    }

    /// Decode and cache all RasterImage entities from the current document.
    /// Silently skips images whose files cannot be read.
    pub fn populate_images_from_document(&mut self) {
        self.populate_images_from_document_unbumped();
        self.bump_geometry();
    }

    fn populate_images_from_document_unbumped(&mut self) {
        self.images.clear();
        let entries: Vec<(Handle, acadrust::entities::RasterImage)> = self
            .document
            .entities()
            .filter_map(|e| {
                if let EntityType::RasterImage(img) = e {
                    Some((img.common.handle, img.clone()))
                } else {
                    None
                }
            })
            .collect();
        for (handle, img) in entries {
            if let Some(model) = ImageModel::from_raster_image(&img) {
                self.images.insert(handle, model);
            }
        }
    }

    /// Rebuild the cached fill model (hatch / DXF SOLID) for `handle` after
    /// its document entity was edited in place. The fill models are prebuilt
    /// at load, so a pattern-scale / background / boundary edit stays
    /// invisible until the cached model is refreshed (#415).
    pub fn refresh_fill_model(&mut self, handle: Handle) {
        let contextual = self
            .document
            .get_entity(handle)
            .map(|entity| {
                crate::scene::annotative::entity_for_annotation_context(
                    &self.document,
                    entity,
                    self.displayed_annotation_scale_handle(),
                )
            });
        let new_model = match contextual.as_deref() {
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
            self.hatches.insert(handle, model);
        }
    }

    pub fn populate_hatches_from_document(&mut self) {
        self.populate_hatches_from_document_unbumped();
        self.bump_geometry();
    }

    fn populate_hatches_from_document_unbumped(&mut self) {
        self.hatches.clear();

        let entries: Vec<(Handle, EntityType)> = self
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Hatch(h) => Some((
                    h.common.handle,
                    crate::scene::annotative::entity_for_annotation_context(
                        &self.document,
                        e,
                        self.displayed_annotation_scale_handle(),
                    )
                        .into_owned(),
                )),
                EntityType::Solid(s) => Some((s.common.handle, e.clone())),
                _ => None,
            })
            .collect();

        use crate::par::prelude::*;
        self.hatches = entries
            .into_par_iter()
            .filter_map(|(handle, kind)| {
                // Paper-space entities live in sheet coordinates — world_offset must not
                let model = match &kind {
                    EntityType::Hatch(dxf) => {
                        let color = convert::tess_util::aci_to_rgba(&dxf.common.color);
                        Self::hatch_model_from_dxf(dxf, color)
                    }
                    EntityType::Solid(solid) => {
                        let color = convert::tess_util::aci_to_rgba(&solid.common.color);
                        Some(Self::solid_hatch_model(solid, color))
                    }
                    _ => None,
                };
                model.map(|m| (handle, m))
            })
            .collect();
    }

    /// ImageModel for an image-bearing entity. `None` when it cannot decode.
    pub(super) fn image_seed_for(
        &self,
        entity: &acadrust::entities::EntityType,
    ) -> Option<ImageModel> {
        match entity {
            EntityType::RasterImage(img) => ImageModel::from_raster_image(img),
            EntityType::Ole2Frame(ole) => ImageModel::from_ole2frame(ole),
            EntityType::Underlay(u) => match self.document.objects.get(&u.definition_handle) {
                Some(acadrust::objects::ObjectType::UnderlayDefinition(def)) => {
                    ImageModel::from_underlay(u, def)
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub fn populate_meshes_from_document(&mut self) {
        self.populate_meshes_impl(false, true);
    }

    /// Like [`populate_meshes_from_document`] but tessellates only solids
    /// whose handle is not already cached — the existing meshes are kept.
    ///
    /// Used after an XREF merge: the host document's solids were already
    /// tessellated by the background loader, and the merge assigns brand-new
    /// handles to every imported xref entity (see `merge_xref_into_block`),
    /// so cached handles are guaranteed to be host solids. This turns the
    /// post-xref pass from "re-tessellate host + all xrefs" into "tessellate
    /// only the newly merged xref solids" — the dominant cost when a drawing
    /// attaches several large xrefs. (#203)
    pub fn populate_missing_meshes_from_document(&mut self) {
        self.populate_meshes_impl(true, true);
    }

    fn populate_meshes_impl(&mut self, incremental: bool, bump: bool) {
        if !incremental {
            self.meshes.clear();
            self.block_meshes.clear();
        }
        // BLOCK-entity handles of the layout (model + paper) blocks. A solid
        // owned by one of these is top-level; anything else lives in a block
        // definition and is instanced per INSERT instead. (#123)
        let layout_blocks: std::collections::HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|o| match o {
                acadrust::objects::ObjectType::Layout(l) if !l.block_record.is_null() => {
                    Some(l.block_record)
                }
                _ => None,
            })
            .collect();
        // Resolve color through `render_style` so the same bg adaptation
        // wires use kicks in (pure black on dark bg → white, pure white
        // on light bg → black). Without this, ACIS meshes ignore
        // `adapt_to_bg` and stay invisible against matching bg colours.
        let entries: Vec<(Handle, EntityType, [f32; 4], bool)> = self
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
                | EntityType::Mesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::PolyfaceMesh(_) => {
                    let handle = e.common().handle;
                    // Incremental (post-xref) pass: leave already-tessellated
                    // host solids untouched, only build the newly merged ones.
                    if incremental
                        && (self.meshes.contains_key(&handle) || self.block_meshes.contains_key(&handle))
                    {
                        return None;
                    }
                    let color = self.render_style(e).0;
                    let top_level = layout_blocks.contains(&e.common().owner_handle);
                    Some((handle, e.clone(), color, top_level))
                }
                _ => None,
            })
            .collect();

        use crate::par::prelude::*;
        let facet_res = self.document.header.facet_resolution;
        let chordal_deflection =
            crate::entities::solid3d::display_deflection(&self.document.header, facet_res);
        let isolines = self.document.header.isolines.max(0) as usize;
        // Top-level solids: offset into the render frame, drawn flat.
        // Block-definition solids: keep block-local coords for per-INSERT
        // instancing (no offset applied here).
        let built: Vec<(Handle, MeshLodSet, bool)> = entries
            .into_par_iter()
            .filter_map(|(handle, entity, color, top_level)| {
                crate::entities::solid3d::tessellate_volume(
                    &entity,
                    color,
                    facet_res,
                    chordal_deflection,
                    isolines,
                )
                .map(|mut mesh| {
                    let material = crate::scene::model::material_model::resolve_material_with_base(
                        &self.document,
                        &entity,
                        color,
                        None,
                        self.material_base_dir.as_deref(),
                    );
                    material.apply_to_with_face_overrides(
                        &mut mesh,
                        &self.document,
                        self.material_base_dir.as_deref(),
                    );
                    crate::scene::model::visual_style_model::apply_mesh_visual_style(
                        &mut mesh,
                        &self.document,
                        &entity,
                    );
                    let mesh = if top_level { offset_mesh_lod_set(mesh) } else { mesh };
                    (handle, mesh, top_level)
                })
            })
            .collect();
        for (handle, mut m, top_level) in built {
            if top_level {
                self.meshes.insert(handle, m);
            } else {
                m.prepare_instance_source(handle);
                self.block_meshes.insert(handle, m);
            }
        }

        if bump {
            self.bump_geometry();
        }
    }

    /// Rebuild hatch / image / mesh caches after the document is modified
    /// outside the normal `add_entity` path (e.g. REFCLOSE SAVE).
    pub fn rebuild_derived_caches(&mut self) {
        self.invalidate_dependency_index();
        self.populate_hatches_from_document_unbumped();
        self.populate_images_from_document_unbumped();
        self.populate_meshes_impl(false, false);
        self.bump_geometry();
    }

    /// Build a solid-fill HatchModel for a DXF Solid entity.
    /// SOLID corners use Z-order. Preserve it in the projected hatch as well so
    /// intentionally crossing geometry is not silently rewritten.
    pub(super) fn solid_hatch_model(solid: &DxfSolid, color: [f32; 4]) -> HatchModel {
        // Keep the corners in f64 until the AABB centre is known, then store
        // each as a small f32 offset from it — same precision-preserving anchor
        // `hatch_model_from_dxf` uses. Casting the absolute WCS corner straight
        // to f32 costs ~0.06 units of resolution at UTM magnitudes (~1e6), so
        // the quad snapped to a grid and the fill drifted off its outline.
        let wcs = crate::entities::solid::wcs_corners(solid);
        let order = [0, 1, 3, 2];
        let corners: [[f64; 2]; 4] = order.map(|index| [wcs[index][0], wcs[index][1]]);
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for c in &corners {
            for i in 0..2 {
                if c[i] < min[i] {
                    min[i] = c[i];
                }
                if c[i] > max[i] {
                    max[i] = c[i];
                }
            }
        }
        let world_origin = if min[0].is_finite() && min[1].is_finite() {
            [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5]
        } else {
            [0.0, 0.0]
        };
        let boundary = corners
            .iter()
            .map(|c| {
                [
                    (c[0] - world_origin[0]) as f32,
                    (c[1] - world_origin[1]) as f32,
                ]
            })
            .collect();
        HatchModel {
            render_instance: None,
            boundary: std::sync::Arc::new(boundary),
            boundary_wcs: None,
            fill_plane: None,
            fill_plane_boundary: None,
            boundary_exterior: None,
            boundary_sources: None,
            boundary_paths: None,
            style: acadrust::entities::HatchStyleType::Normal,
            pattern: model::hatch_model::HatchPattern::Solid,
            name: "SOLID".into(),
            color,
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            world_origin,
            draw_depth: 0.0,
        }
    }

    pub fn add_hatch(
        &mut self,
        model: HatchModel,
        layer: Option<&str>,
        entity_style: Option<(acadrust::types::Color, acadrust::types::Transparency)>,
    ) -> Handle {
        let mut dxf = DxfHatch::new();
        dxf.style = model.style;
        if let Some(plane) = model.fill_plane {
            let x = glam::DVec3::from_array(plane.x_axis);
            let y = glam::DVec3::from_array(plane.y_axis);
            let normal = x.cross(y).normalize_or(glam::DVec3::Z);
            dxf.normal = acadrust::types::Vector3::new(normal.x, normal.y, normal.z);
            dxf.elevation = glam::DVec3::from_array(plane.origin).dot(normal);
        }
        dxf.is_solid = matches!(
            model.pattern,
            crate::scene::model::hatch_model::HatchPattern::Solid
        );
        // Persist analytic command geometry when available.
        if let Some(paths) = model.boundary_paths.as_deref() {
            dxf.paths = paths.clone();
        } else {
            // Otherwise reconstruct every ring from render offsets.
            let reconstructed_wcs: Vec<[f64; 2]> = if model.boundary_wcs.is_none() {
            let [wx, wy] = model.world_origin;
            model
                .boundary
                .iter()
                .map(|&[x, y]| {
                    if x.is_finite() && y.is_finite() {
                        [x as f64 + wx, y as f64 + wy]
                    } else {
                        [f64::NAN, f64::NAN]
                    }
                })
                .collect()
            } else {
                Vec::new()
            };
            let wcs = model
                .boundary_wcs
                .as_deref()
                .map(|points| points.as_slice())
                .unwrap_or(reconstructed_wcs.as_slice());
            let mut ring: Vec<Vector2> = Vec::new();
            let mut first = true;
            let mut ring_index = 0usize;
            let mut push_ring = |r: &mut Vec<Vector2>, is_outer: bool, index: usize| {
            if !r.is_empty() {
                let edge = PolylineEdge::new(std::mem::take(r), true);
                let handles: Vec<_> = model
                    .boundary_sources
                    .as_deref()
                    .and_then(|sources| sources.get(index))
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|handle| handle.is_valid())
                    .collect();
                let mut bits = 0;
                if is_outer {
                    bits |= acadrust::entities::hatch::BoundaryPathFlags::OUTERMOST.bits();
                    bits |= acadrust::entities::hatch::BoundaryPathFlags::EXTERNAL.bits();
                }
                let mut path = BoundaryPath::with_flags(
                    acadrust::entities::hatch::BoundaryPathFlags::from_bits(bits),
                );
                path.add_edge(BoundaryEdge::Polyline(edge));
                for handle in handles {
                    path.add_boundary_handle(handle);
                }
                dxf.paths.push(path);
            }
            };
            for &[x, y] in wcs {
                if x.is_finite() && y.is_finite() {
                    ring.push(Vector2::new(x, y));
                } else {
                    let is_outer = model
                        .boundary_exterior
                        .as_deref()
                        .and_then(|roles| roles.get(ring_index))
                        .copied()
                        .unwrap_or(first);
                    first = false;
                    if !ring.is_empty() {
                        push_ring(&mut ring, is_outer, ring_index);
                        ring_index += 1;
                    }
                }
            }
            let is_outer = model
                .boundary_exterior
                .as_deref()
                .and_then(|roles| roles.get(ring_index))
                .copied()
                .unwrap_or(first);
            push_ring(&mut ring, is_outer, ring_index);
        }
        dxf.is_associative = dxf
            .paths
            .iter()
            .any(|path| !path.boundary_handles.is_empty());
        let pattern_scale = if model.scale.abs() > 1e-6 {
            model.scale as f64
        } else {
            1.0
        };
        let pattern_origin = model
            .fill_plane_boundary
            .as_deref()
            .and_then(|points| {
                points
                    .iter()
                    .find(|point| point[0].is_finite() && point[1].is_finite())
            })
            .map(|point| [point[0] as f64, point[1] as f64])
            .unwrap_or(model.world_origin);
        if let crate::scene::model::hatch_model::HatchPattern::Pattern(families) = &model.pattern {
            let mut pattern = acadrust::entities::HatchPattern::new(&model.name);
            let rotation = model.angle_offset as f64;
            let (rotation_sin, rotation_cos) = rotation.sin_cos();
            for family in families {
                let family_angle = (family.angle_deg as f64).to_radians();
                let angle = family_angle + rotation;
                let (family_sin, family_cos) = family_angle.sin_cos();
                let local_offset_x = family.dx as f64 * family_cos
                    - family.dy as f64 * family_sin;
                let local_offset_y = family.dx as f64 * family_sin
                    + family.dy as f64 * family_cos;
                let base_x = family.x0 as f64 * pattern_scale;
                let base_y = family.y0 as f64 * pattern_scale;
                pattern.lines.push(acadrust::entities::HatchPatternLine {
                    angle,
                    base_point: Vector2::new(
                        pattern_origin[0]
                            + base_x * rotation_cos
                            - base_y * rotation_sin,
                        pattern_origin[1]
                            + base_x * rotation_sin
                            + base_y * rotation_cos,
                    ),
                    offset: Vector2::new(
                        (local_offset_x * rotation_cos
                            - local_offset_y * rotation_sin)
                            * pattern_scale,
                        (local_offset_x * rotation_sin
                            + local_offset_y * rotation_cos)
                            * pattern_scale,
                    ),
                    dash_lengths: family
                        .dashes
                        .iter()
                        .map(|dash| *dash as f64 * pattern_scale)
                        .collect(),
                });
            }
            dxf.pattern = pattern;
        }
        dxf.pattern_angle = model.angle_offset as f64;
        dxf.pattern_scale = pattern_scale;
        // A gradient fill must be encoded on the DXF entity itself: the render
        // model is rebuilt from the entity below (`add_entity` →
        // `hatch_model_from_dxf`), so a gradient kept only on the command's
        // model silently degraded to a plain pattern hatch.
        if let crate::scene::model::hatch_model::HatchPattern::Gradient {
            angle_deg,
            color2,
            kind,
            invert,
            shift,
        } = &model.pattern
        {
            let to_color = |c: [f32; 4]| acadrust::types::Color::Rgb {
                r: (c[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                g: (c[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                b: (c[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            };
            dxf.is_solid = true;
            dxf.gradient_color.enabled = true;
            dxf.gradient_color.name = kind.dxf_name(*invert).to_string();
            // Keep both persisted angle fields aligned.
            dxf.pattern_angle = (*angle_deg as f64).to_radians();
            dxf.gradient_color.angle = (*angle_deg as f64).to_radians();
            dxf.gradient_color.shift = *shift as f64;
            dxf.gradient_color.is_single_color = false;
            // Linear has no INV name in the standard set: persist an inverted
            // linear by swapping the colour stops instead.
            let (c0, c1) =
                if *invert && matches!(kind, crate::scene::model::hatch_model::GradientKind::Linear)
                {
                    (*color2, model.color)
                } else {
                    (model.color, *color2)
                };
            dxf.gradient_color.colors = vec![
                acadrust::entities::hatch::GradientColorEntry {
                    value: 0.0,
                    color: to_color(c0),
                },
                acadrust::entities::hatch::GradientColorEntry {
                    value: 1.0,
                    color: to_color(c1),
                },
            ];
        }
        // `add_entity` already builds the render model from the DXF entity via
        // `hatch_model_from_dxf` and inserts it with a correct `world_origin`
        // (AABB-centred) for the relative-to-eye fill. The command-built `model`
        // carries `world_origin: [0, 0]`, which after the world_offset removal
        // leaves the fill mis-placed and effectively invisible until a later
        // edit rebuilds it from the DXF — so keep the seed, don't overwrite it.
        let mut entity = EntityType::Hatch(dxf);

        if let Some(layer) = layer {
            entity.as_entity_mut().set_layer(layer.to_string());
        }
        if let Some((color, transparency)) = entity_style {
            entity.common_mut().color = color;
            entity.common_mut().transparency = transparency;
        }

        self.add_entity(entity)
    }

    pub fn clear(&mut self) {
        self.document.record_all_entities_for_transaction();
        self.document = CadDocument::new();
        self.replace_selection(HashSet::default());
        self.preview_wires = vec![];
        self.preview_text = vec![];
        self.current_layout = "Model".to_string();
        self.hatches = HashMap::default();
        self.associative_hatch_source_cache.borrow_mut().take();
        self.images = HashMap::default();
        self.meshes = HashMap::default();
        self.block_meshes = HashMap::default();
        self.solid_models = HashMap::default();
        *self.camera.borrow_mut() = Camera::default();
        self.camera_generation += 1;
        self.bump_geometry();
    }
}
