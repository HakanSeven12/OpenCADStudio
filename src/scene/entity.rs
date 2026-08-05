// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;

/// Convert a HATCH's own resolved pattern line (world-unit `offset` = step to
/// the next parallel line, plus a base angle) into a render `PatFamily` whose
/// geometry is already final — the HatchModel that carries it uses scale 1 and
/// angle_offset 0 (see `prebaked` in `hatch_model_from_dxf`). The world-space
/// offset is rotated into the line's local frame so `pattern_segments` and the
/// GPU shader, which rotate `(dx, dy)` back out by the family angle, reproduce
/// the exact stored step. `x0/y0` are filled in by the caller (from the stored
/// `base_point`, relative to `world_origin`) once the boundary anchor is known;
/// they set the pattern origin, observable for dashed / offset patterns.
/// Order a hatch boundary path's sampled edges into one tip-to-tail loop.
///
/// Real files do not store boundary edges as a sequential walk: associative
/// hatches list them in boundary-source-entity order, with arbitrary
/// direction — the next edge in the list may attach to either end of the
/// chain built so far, or belong to the far side of the loop entirely.
/// Concatenating them verbatim draws a self-crossing "bowtie" outline and
/// flips the even-odd fill over the wrong region.
///
/// Greedy nearest-endpoint assembly: keep the chain open at both ends and, at
/// each step, attach the unused edge whose endpoint lies closest to either
/// end (reversing / prepending as needed). Distance comparison, no tolerance:
/// a correctly-ordered file matches at distance 0 and reproduces exactly.
fn chain_path_edges(mut polys: Vec<Vec<[f64; 2]>>) -> Vec<[f64; 2]> {
    let d2 = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
    polys.retain(|p| !p.is_empty());
    if polys.is_empty() {
        return Vec::new();
    }
    let mut chain: std::collections::VecDeque<[f64; 2]> = polys.swap_remove(0).into();
    while !polys.is_empty() {
        let head = *chain.front().unwrap();
        let tail = *chain.back().unwrap();
        // (distance, index, reverse-points, attach-at-front)
        let mut best = (f64::MAX, 0usize, false, false);
        for (i, p) in polys.iter().enumerate() {
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
        let mut p = polys.swap_remove(idx);
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
    chain.into()
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

    pub fn add_entity(&mut self, mut entity: EntityType) -> Handle {
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
            crate::entities::solid3d::tessellate_volume(&entity, color, facet_res, isolines)
                .map(|m| offset_mesh_lod_set(m))
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
            // state (a new layer / block) so the app knows a pure-entity delta
            // would be incomplete. Raster image definitions are captured as
            // targeted object before-images above.
            if self.is_recording_undo() {
                self.record_undo_before(handle, None);
                if creates_layer || mutates_block_structure {
                    self.poison_undo_recording();
                }
            }
            if affects_blocks {
                self.bump_geometry();
            } else {
                // Plain top-level add: name the new handle so every derived cache
                // patches in just this one entity instead of rebuilding.
                self.bump_entities(&[(handle, ChangeKind::Added)]);
            }
        }
        handle
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
            crate::entities::solid3d::tessellate_volume(&entity, color, facet_res, isolines)
                .map(|m| offset_mesh_lod_set(m))
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
        let isolines = self.document.header.isolines.max(0) as usize;
        use crate::par::prelude::*;
        let built: Vec<(Handle, MeshLodSet, bool)> = entries
            .into_par_iter()
            .filter_map(|(handle, entity, color, top_level)| {
                crate::entities::solid3d::tessellate_volume(
                    entity.as_ref(),
                    color,
                    facet_res,
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
        Ok(self.add_entity(EntityType::Insert(insert)))
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
                            model::hatch_model::HatchPattern::Gradient { angle_deg, .. } => {
                                *angle_deg = dxf.pattern_angle.to_degrees() as f32;
                            }
                            model::hatch_model::HatchPattern::Pattern(_)
                            | model::hatch_model::HatchPattern::Solid => {}
                        }
                    }
                }
                if self.selected.contains(&handle) {
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
            true,
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
    ) -> Vec<HatchModel> {
        let depth_map = self.draw_depth_map();
        let graph = crate::scene::render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_viewport(viewport);
        let mut hatch_block_memo = std::collections::HashMap::new();
        let mut models = Vec::new();
        graph.walk_root(
            self.render_scene_root(layout_block),
            |entity, context| {
                if context.is_instanced() {
                    return true;
                }
                let common = entity.common();
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
                match entity {
                    EntityType::Insert(insert) => {
                        crate::scene::render_graph::block_contains_hatch(
                            &self.document,
                            &insert.block_name,
                            &mut hatch_block_memo,
                        )
                    }
                    EntityType::Dimension(dimension) => {
                        let name = dimension.base().block_name.trim();
                        !name.is_empty()
                            && crate::scene::render_graph::block_contains_hatch(
                                &self.document,
                                name,
                                &mut hatch_block_memo,
                            )
                    }
                    _ => true,
                }
            },
            |entity, context| {
                if !context.is_instanced() {
                    return;
                }
                let EntityType::Hatch(source_hatch) = entity else {
                    return;
                };
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
    ) -> Vec<HatchModel> {
        let depth_map = self.draw_depth_map();
        let graph = crate::scene::render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        );
        let mut models = Vec::new();
        graph.walk_root(
            self.render_scene_root(target_block),
            |entity, context| {
                context.is_instanced()
                    || !self.entity_temporarily_hidden(entity.common().handle)
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
                models.push(HatchModel {
                    boundary: Arc::new(boundary),
                    boundary_wcs: None,
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
        use acadrust::entities::WipeoutClipType;

        let origin = [wo.insertion_point.x, wo.insertion_point.y];

        let is_polygon = wo.clipping_enabled
            && wo.clip_boundary_vertices.len() >= 3
            && matches!(wo.clip_type, WipeoutClipType::Polygonal);

        if is_polygon {
            // DXF clip vertices live in image-pixel space, centred on the
            // image (range −size/2 … +size/2). Image-bottom-left → insertion,
            // image-y-axis points DOWN (per the DXF "v_vector points down the
            // image" convention), so map:
            //   x_off = (clip.x + size.x/2) × u_vec
            //   y_off = (size.y/2 − clip.y) × v_vec    ← y flipped
            // Offsets are relative to `origin` (the insertion point).
            let cx_of = |v: &acadrust::types::Vector2| v.x + wo.size.x * 0.5;
            let cy_of = |v: &acadrust::types::Vector2| wo.size.y * 0.5 - v.y;
            let mut poly: Vec<[f32; 2]> = wo
                .clip_boundary_vertices
                .iter()
                .map(|v| {
                    let cx = cx_of(v);
                    let cy = cy_of(v);
                    let wx = (wo.u_vector.x * cx + wo.v_vector.x * cy) as f32;
                    let wy = (wo.u_vector.y * cx + wo.v_vector.y * cy) as f32;
                    [wx, wy]
                })
                .collect();
            // Close the loop: the GPU `in_polygon` ray-cast walks
            // sequential pairs and doesn't wrap, so without an explicit
            // closing vertex the last edge (vN-1 → v0) is never tested and
            // the fill bleeds far past the boundary.
            if let Some(&first) = poly.first() {
                if poly.last() != Some(&first) {
                    poly.push(first);
                }
            }
            (origin, poly)
        } else {
            // Rectangular boundary from 4 corners, as offsets from `origin`.
            let ux = (wo.u_vector.x * wo.size.x) as f32;
            let uy = (wo.u_vector.y * wo.size.x) as f32;
            let vx = (wo.v_vector.x * wo.size.y) as f32;
            let vy = (wo.v_vector.y * wo.size.y) as f32;
            // Close the loop (repeat corner 0): the GPU `in_polygon` ray-cast
            // walks sequential vertex pairs and never wraps last→first, so an
            // unclosed quad leaves the v3→v0 edge untested and the solid mask
            // bleeds past the boundary — same reason the polygon branch closes.
            (
                origin,
                vec![
                    [0.0, 0.0],
                    [ux, uy],
                    [ux + vx, uy + vy],
                    [vx, vy],
                    [0.0, 0.0],
                ],
            )
        }
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

        let mut boundary: Vec<[f64; 2]> = Vec::new();

        for path in &dxf.paths {
            // Skip TEXTBOX boundary paths (flag bit 3). These are text
            // derived bounding boxes used for island detection; they are
            // never drawn or filled. Treating one as a fill boundary paints its
            // rectangle solid and creates a phantom bar.
            if path.flags.bits() & 8 != 0 {
                continue;
            }
            let before_path = boundary.len();
            if !boundary.is_empty() {
                boundary.push([f64::NAN, f64::NAN]);
            }
            let path_start = boundary.len();

            let mut edge_polys: Vec<Vec<[f64; 2]>> = Vec::new();
            for edge in &path.edges {
                match edge {
                    BoundaryEdge::Polyline(poly) => {
                        let verts = &poly.vertices;
                        let count = verts.len();
                        if count == 0 {
                            continue;
                        }
                        let seg_count = if poly.is_closed {
                            count
                        } else {
                            count.saturating_sub(1)
                        };
                        for i in 0..seg_count {
                            let v0 = &verts[i];
                            let v1 = &verts[(i + 1) % count];
                            let bulge = v0.z;
                            // Tess in f64 to preserve ~1 cm precision at
                            // UTM-scale WCS (the f32 path used to produce
                            // visibly wavy hatch arcs at 1e5+ magnitude).
                            let arc = if bulge.abs() < 1e-9 {
                                None
                            } else {
                                crate::entities::common::BulgeArc::from_bulge(
                                    [v0.x, v0.y],
                                    [v1.x, v1.y],
                                    bulge,
                                )
                            };
                            let Some(arc) = arc else {
                                boundary.push(to_xy(v0.x, v0.y));
                                continue;
                            };
                            let segs = convert::tess_util::arc_segments(
                                arc.radius,
                                arc.sweep.abs(),
                                convert::tess_util::fill_chord_tol(arc.radius),
                            );
                            for j in 0..segs {
                                let s = arc.sample(j as f64 / segs as f64);
                                boundary.push(to_xy(s[0], s[1]));
                            }
                        }
                        if poly.is_closed {
                            if let Some(&first) = boundary.get(path_start) {
                                boundary.push(first);
                            }
                        }
                    }
                    BoundaryEdge::Line(line) => {
                        edge_polys.push(vec![
                            to_xy(line.start.x, line.start.y),
                            to_xy(line.end.x, line.end.y),
                        ]);
                    }
                    BoundaryEdge::CircularArc(arc) => {
                        let (sa, span) = convert::tess_util::arc_signed_span(
                            arc.start_angle,
                            arc.end_angle,
                            arc.counter_clockwise,
                        );
                        let segs = convert::tess_util::arc_segments(
                            arc.radius,
                            span.abs(),
                            convert::tess_util::fill_chord_tol(arc.radius),
                        );
                        let mut pts = Vec::with_capacity(segs as usize + 1);
                        for i in 0..=segs {
                            let t = sa + span * (i as f64 / segs as f64);
                            pts.push(to_xy(
                                arc.center.x + arc.radius * t.cos(),
                                arc.center.y + arc.radius * t.sin(),
                            ));
                        }
                        edge_polys.push(pts);
                    }
                    BoundaryEdge::EllipticArc(ell) => {
                        let r_maj = (ell.major_axis_endpoint.x * ell.major_axis_endpoint.x
                            + ell.major_axis_endpoint.y * ell.major_axis_endpoint.y)
                            .sqrt();
                        let r_min = r_maj * ell.minor_axis_ratio;
                        let rot = ell
                            .major_axis_endpoint
                            .y
                            .atan2(ell.major_axis_endpoint.x);
                        let (sa, span) = convert::tess_util::arc_signed_span(
                            ell.start_angle,
                            ell.end_angle,
                            ell.counter_clockwise,
                        );
                        let segs = convert::tess_util::arc_segments(
                            r_maj,
                            span.abs(),
                            convert::tess_util::fill_chord_tol(r_maj),
                        );
                        let (cr, sr) = (rot.cos(), rot.sin());
                        let mut pts = Vec::with_capacity(segs as usize + 1);
                        for i in 0..=segs {
                            let t = sa + span * (i as f64 / segs as f64);
                            let lx = r_maj * t.cos();
                            let ly = r_min * t.sin();
                            pts.push(to_xy(
                                ell.center.x + lx * cr - ly * sr,
                                ell.center.y + lx * sr + ly * cr,
                            ));
                        }
                        edge_polys.push(pts);
                    }
                    BoundaryEdge::Spline(spline) => {
                        // DXF spline control_points pack (x, y, weight) into
                        // a Vector3 — the z field is the rational weight, NOT
                        // a Z coordinate. The legacy code dropped weight and
                        // sampled with a fixed 16 segments; both bugs
                        // produced visibly wrong fill regions for spline-
                        // bounded hatches (especially block-internal ones,
                        // where boundaries are often spline curves with
                        // rational weights and short cubic segments).
                        //
                        // Build a NurbsCurve when `rational`, otherwise a
                        // plain BSplineCurve, and sample adaptively via
                        // truck's `parameter_division` at the same chord
                        // tolerance the fill polygon uses for arcs.
                        let degree = spline.degree.max(0) as usize;
                        let knot_vec = if !spline.knots.is_empty() {
                            KnotVec::from(spline.knots.clone())
                        } else if spline.control_points.len() >= degree + 1 {
                            KnotVec::uniform_knot(degree, spline.control_points.len() - 1)
                        } else {
                            KnotVec::from(vec![])
                        };
                        let knot_ok = spline.control_points.len() >= 2
                            && degree >= 1
                            && knot_vec.len() == spline.control_points.len() + degree + 1;

                        // Rough chord-tolerance: 0.1% of the control-poly
                        // diagonal so adaptive sampling produces enough
                        // points to follow the curve without exploding on
                        // huge splines.
                        let (mut sp_min_x, mut sp_min_y) = (f64::INFINITY, f64::INFINITY);
                        let (mut sp_max_x, mut sp_max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
                        for cp in &spline.control_points {
                            sp_min_x = sp_min_x.min(cp.x);
                            sp_min_y = sp_min_y.min(cp.y);
                            sp_max_x = sp_max_x.max(cp.x);
                            sp_max_y = sp_max_y.max(cp.y);
                        }
                        let diag = ((sp_max_x - sp_min_x).powi(2)
                            + (sp_max_y - sp_min_y).powi(2))
                        .sqrt();
                        let tol = convert::tess_util::fill_chord_tol(diag.max(1.0));

                        let mut epts: Vec<[f64; 2]> = Vec::new();
                        let mut sampled = false;
                        if knot_ok {
                            if spline.rational {
                                // NURBS: pack (x, y, 0, w) into Vector4.
                                let cps: Vec<Vector4> = spline
                                    .control_points
                                    .iter()
                                    .map(|p| {
                                        let w = if p.z.abs() > 1e-12 { p.z } else { 1.0 };
                                        Vector4::new(p.x * w, p.y * w, 0.0, w)
                                    })
                                    .collect();
                                let bspl = TruckBSpline::new(knot_vec.clone(), cps);
                                let curve = NurbsCurve::new(bspl);
                                let (t0, t1) = curve.range_tuple();
                                let (_, pts) = curve.parameter_division((t0, t1), tol);
                                for p in pts {
                                    epts.push(to_xy(p.x, p.y));
                                }
                                sampled = true;
                            } else {
                                let cps: Vec<Point3> = spline
                                    .control_points
                                    .iter()
                                    .map(|p| Point3::new(p.x, p.y, 0.0))
                                    .collect();
                                let bspl = TruckBSpline::new(knot_vec, cps);
                                let (t0, t1) = bspl.range_tuple();
                                let (_, pts) = bspl.parameter_division((t0, t1), tol);
                                for p in pts {
                                    epts.push(to_xy(p.x, p.y));
                                }
                                sampled = true;
                            }
                        }
                        if !sampled {
                            // Fallback: prefer fit_points (which lie on the
                            // curve) over control_points (which usually
                            // don't). A control-point polyline would draw
                            // the convex-hull silhouette — visibly wrong.
                            let pts: &[_] = if !spline.fit_points.is_empty() {
                                &spline.fit_points
                            } else {
                                &[]
                            };
                            if !pts.is_empty() {
                                for p in pts {
                                    epts.push(to_xy(p.x, p.y));
                                }
                            } else {
                                for cp in &spline.control_points {
                                    epts.push(to_xy(cp.x, cp.y));
                                }
                            }
                        }
                        edge_polys.push(epts);
                    }
                }
            }
            boundary.extend(chain_path_edges(edge_polys));

            if boundary.len() == path_start {
                boundary.truncate(before_path);
                continue;
            }
            if boundary.len() >= path_start + 3 {
                let first = boundary[path_start];
                let last = *boundary.last().unwrap();
                if (first[0] - last[0]).abs() > 1e-5 || (first[1] - last[1]).abs() > 1e-5 {
                    boundary.push(first);
                }
            }
        }

        if boundary.is_empty() {
            return None;
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
            let color2 = stop(1).unwrap_or(color);
            let angle_deg = dxf.pattern_angle.to_degrees() as f32;
            let (kind, invert) =
                model::hatch_model::GradientKind::from_name(&dxf.gradient_color.name);
            model::hatch_model::HatchPattern::Gradient {
                angle_deg,
                color2,
                kind,
                invert,
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

        Some(HatchModel {
            boundary: std::sync::Arc::new(boundary_f32),
            boundary_wcs: None,
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

    /// Tessellate all `Solid3D` entities in the current document into
    /// GPU-ready `MeshModel`s and store them in `self.meshes`.
    ///
    /// Called after loading a document or after undo/redo so that every
    /// `Solid3D` entity is represented in the mesh cache.
    /// ImageModel for an edited image-bearing entity (RasterImage, or a PDF
    /// UNDERLAY resolved through its definition object). `None` for others.
    fn image_seed_for(&self, entity: &acadrust::entities::EntityType) -> Option<ImageModel> {
        match entity {
            EntityType::RasterImage(img) => ImageModel::from_raster_image(img),
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
        let isolines = self.document.header.isolines.max(0) as usize;
        // Top-level solids: offset into the render frame, drawn flat.
        // Block-definition solids: keep block-local coords for per-INSERT
        // instancing (no offset applied here).
        let built: Vec<(Handle, MeshLodSet, bool)> = entries
            .into_par_iter()
            .filter_map(|(handle, entity, color, top_level)| {
                crate::entities::solid3d::tessellate_volume(&entity, color, facet_res, isolines).map(|mut mesh| {
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
    /// Conventional DXF SOLID corners use Z-order; legacy entities may already
    /// be in perimeter order. Use the same non-crossing resolver as wire fill.
    pub(super) fn solid_hatch_model(solid: &DxfSolid, color: [f32; 4]) -> HatchModel {
        // Keep the corners in f64 until the AABB centre is known, then store
        // each as a small f32 offset from it — same precision-preserving anchor
        // `hatch_model_from_dxf` uses. Casting the absolute WCS corner straight
        // to f32 costs ~0.06 units of resolution at UTM magnitudes (~1e6), so
        // the quad snapped to a grid and the fill drifted off its outline.
        let wcs = crate::entities::solid::wcs_corners(solid);
        let order = crate::entities::solid::perimeter_indices(&wcs);
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
            boundary: std::sync::Arc::new(boundary),
            boundary_wcs: None,
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

    pub fn add_hatch(&mut self, model: HatchModel) -> Handle {
        let mut dxf = DxfHatch::new();
        dxf.is_solid = matches!(
            model.pattern,
            crate::scene::model::hatch_model::HatchPattern::Solid
        );
        // Prefer the command-supplied exact f64 boundary so a typed vertex is
        // persisted without f32 quantization (issue #311). Falling back to the
        // render-side `boundary`, the points arrive in local render space
        // (world_offset already subtracted), so add `world_origin` back —
        // otherwise the boundary wire lands `world_offset` away from the fill.
        // Build one DXF boundary path per ring. The command layer separates
        // the outer boundary from its holes with NaN sentinels in
        // `boundary_wcs`; split on those so nested hatches (e.g. a small
        // rectangle inside a big one) persist with real holes instead of a
        // single self-intersecting polyline. Only the FIRST ring carries the
        // external / outermost flags — every later ring is a hole and must not
        // be flagged external, or DXF/DWG consumers treat the inner loop as
        // another outer island rather than a hole.
        if let Some(wcs) = &model.boundary_wcs {
            let mut ring: Vec<Vector2> = Vec::new();
            let mut first = true;
            let mut push_ring = |r: &mut Vec<Vector2>, is_outer: bool| {
                if !r.is_empty() {
                    let edge = PolylineEdge::new(std::mem::take(r), true);
                    let mut path = if is_outer {
                        let mut p = BoundaryPath::external();
                        p.flags = acadrust::entities::hatch::BoundaryPathFlags::from_bits(
                            p.flags.bits()
                                | acadrust::entities::hatch::BoundaryPathFlags::OUTERMOST.bits(),
                        );
                        p
                    } else {
                        BoundaryPath::new()
                    };
                    path.add_edge(BoundaryEdge::Polyline(edge));
                    dxf.paths.push(path);
                }
            };
            for &[x, y] in wcs.iter() {
                if x.is_finite() && y.is_finite() {
                    ring.push(Vector2::new(x, y));
                } else {
                    let is_outer = first;
                    first = false;
                    push_ring(&mut ring, is_outer);
                }
            }
            push_ring(&mut ring, first);
        }
        if dxf.paths.is_empty() {
            let wx = model.world_origin[0];
            let wy = model.world_origin[1];
            let verts: Vec<Vector2> = model
                .boundary
                .iter()
                .filter(|v| v[0].is_finite() && v[1].is_finite())
                .map(|&[x, y]| Vector2::new(x as f64 + wx, y as f64 + wy))
                .collect();
            let edge = PolylineEdge::new(verts, true);
            let mut path = BoundaryPath::external();
            path.add_edge(BoundaryEdge::Polyline(edge));
            dxf.paths.push(path);
        }
        if let Some(entry) = crate::scene::model::hatch_patterns::find(&model.name) {
            dxf.pattern = crate::scene::model::hatch_patterns::build_dxf_pattern(entry);
        }
        dxf.pattern_angle = model.angle_offset as f64;
        dxf.pattern_scale = if model.scale.abs() > 1e-6 {
            model.scale as f64
        } else {
            1.0
        };
        // A gradient fill must be encoded on the DXF entity itself: the render
        // model is rebuilt from the entity below (`add_entity` →
        // `hatch_model_from_dxf`), so a gradient kept only on the command's
        // model silently degraded to a plain pattern hatch.
        if let crate::scene::model::hatch_model::HatchPattern::Gradient {
            angle_deg,
            color2,
            kind,
            invert,
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
            // The render model reads the gradient angle from pattern_angle
            // (radians); the gradient record keeps its own copy for the file.
            dxf.pattern_angle = (*angle_deg as f64).to_radians();
            dxf.gradient_color.angle = (*angle_deg as f64).to_radians();
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
        self.add_entity(EntityType::Hatch(dxf))
    }

    pub fn clear(&mut self) {
        self.document.record_all_entities_for_transaction();
        self.document = CadDocument::new();
        self.replace_selection(HashSet::default());
        self.preview_wires = vec![];
        self.preview_text = vec![];
        self.current_layout = "Model".to_string();
        self.hatches = HashMap::default();
        self.images = HashMap::default();
        self.meshes = HashMap::default();
        self.block_meshes = HashMap::default();
        self.solid_models = HashMap::default();
        *self.camera.borrow_mut() = Camera::default();
        self.camera_generation += 1;
        self.bump_geometry();
    }
}
